#include "pch.h"
#include "WorkspaceGestures.h"
#include "WorkspaceQuery.h"
#include "WorkspaceTabDrag.h"
#include "WorkspaceGeometry.h"
#include "NativeMenus.h"
#include <chrono>
#include <optional>

using namespace CapyUi;
using Windows::Foundation::Point;
using Windows::Foundation::Rect;
namespace NativeInput=Microsoft::UI::Input;
namespace {
A point(Point p){A result;result.Append(N(p.X));result.Append(N(p.Y));return result;}
J rect(Rect r){return O({{L"x",N(r.X)},{L"y",N(r.Y)},{L"width",N(r.Width)},{L"height",N(r.Height)}});}
void ownHolding(DependencyObject const& node){
    if(auto element=node.try_as<UIElement>())element.IsHoldingEnabled(false);
    for(int i=0;i<VisualTreeHelper::GetChildrenCount(node);++i)ownHolding(VisualTreeHelper::GetChild(node,i));
}
}
struct WorkspaceGestures::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr};
    Border hint;
    MenuFlyout menu{nullptr};
    std::vector<weak_ref<FrameworkElement>> tabs;
    std::vector<weak_ref<UIElement>> pressedPath;
    std::unique_ptr<WorkspaceTabDrag> tabSlide;
    J tabCapture;
    std::vector<std::pair<weak_ref<UIElement>,ManipulationModes>> scrollModes;
    std::vector<std::pair<weak_ref<ScrollViewer>,event_token>> scrollObservers;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::optional<uint32_t> pointer,rejectedPointer;
    J action,sourceTag;
    weak_ref<FrameworkElement> source;
    Input::Pointer contact{nullptr};
    NativeInput::GestureRecognizer recognizer;
    NativeInput::PointerDeviceType device=NativeInput::PointerDeviceType::Mouse;
    HWND owner=nullptr;
    Point origin{},position{};
    bool dragging=false,finishing=false,busy=false,dirty=false,releasing=false;
    bool needsHold=false,contextOnly=false,held=false,recognizing=false,ignoreClick=false,menuOpen=false,menuPending=false;
    bool trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    uint64_t generation=0,motion=0,menuGeneration=0;
    double slopX=4,slopY=4;
    J lastRelease,lastCancel;
    uint64_t contactVersion=0,contactToken=0;
    bool ownsFocus()const{return owner&&GetAncestor(GetForegroundWindow(),GA_ROOTOWNER)==owner&&!IsIconic(owner);}
    bool crossed(Point at)const{return std::abs(at.X-origin.X)>slopX||std::abs(at.Y-origin.Y)>slopY;}
    bool owns(UIElement const& element)const{
        if(auto captures=element.PointerCaptures())for(auto captured:captures)
            if(pointer&&captured.PointerId()==*pointer)return true;
        return false;
    }
    void evidence(){
        if(!trace)return;
        auto value=O({{L"phase",S(finishing?L"finishing":dragging?L"dragging":held?L"held":pointer?L"pressed":L"idle")},
            {L"generation",N(double(generation))},{L"source",object(action,L"item")},{L"requires_hold",B(needsHold)},{L"context_only",B(contextOnly)},
            {L"device",S(device==NativeInput::PointerDeviceType::Mouse?L"mouse":device==NativeInput::PointerDeviceType::Pen?L"pen":L"touch")},
            {L"captured",B(owns(root))},{L"menu_open",B(menuOpen)},{L"ignore_click",B(ignoreClick)},{L"scroll_claimed",B(!scrollModes.empty())},
            {L"can_drop",B(dragging&&hint.Visibility()==Visibility::Visible)},
            {L"pointer_id",pointer?N(*pointer):JsonValue::CreateNullValue()},
            {L"routed_pointer_id",contact?N(contact.PointerId()):JsonValue::CreateNullValue()},
            {L"recognizing",B(recognizing)}});
        if(lastRelease.Size())value.Insert(L"last_release",lastRelease);
        if(lastCancel.Size())value.Insert(L"last_cancel",lastCancel);
        AutomationProperties::SetHelpText(root,value.Stringify());
    }
    bool current()const{
        if(flag(data->model,L"partial_zen")!=contextOnly||data->externalPopup)return false;
        if(!dragging){
            auto element=source.get();
            if(!element||!element.IsLoaded())return false;
            auto bounds=visibleBounds(element,root);
            if(bounds.Width<=0||bounds.Height<=0)return false;
        }
        auto item=object(action,L"item");auto kind=str(item,L"kind");
        if(kind==L"tile"||kind==L"panel"){
            auto panel=find(array(data->model,L"panels"),L"id",str(item,L"panel"));
            if(!panel.Size())return false;
            if(kind==L"tile"&&!findId(array(panel,L"tiles"),num(item,L"tile")).Size())return false;
        }
        return true;
    }
    void hideMenu(){
        ++menuGeneration;menuPending=false;
        if(menu)menu.Hide();
    }
    void stopRecognition(){
        if(recognizing){recognizing=false;recognizer.CompleteGesture();}
    }
    void deferClick(){
        if(rejectedPointer)return;
        auto epoch=generation;
        root.DispatcherQueue().TryEnqueue(Microsoft::UI::Dispatching::DispatcherQueuePriority::Low,
            [weak=weak_from_this(),epoch]{if(auto self=weak.lock();self&&self->generation==epoch&&!self->pointer){self->ignoreClick=false;self->evidence();}});
    }
    ~Impl(){if(timer)timer.Stop();if(menu)menu.Hide();}
    A viewport()const{return point({float(root.ActualWidth()),float(root.ActualHeight())});}
    void chrome(J const& event){
        if(!data->input)return;
        auto facts=J::Parse(data->chrome.Stringify());
        facts.Insert(L"held",B(held));facts.Insert(L"dragging",B(dragging&&str(action,L"type")==L"tile_drag"));
        facts.Insert(L"popup_open",B(data->externalPopup||data->popupCount>0));
        data->chrome=facts;
        // Initial projection precedes native layout. Keep facts locally until
        // there is a valid viewport; zero extents must never reach the host.
        if(!root.IsLoaded()||root.ActualWidth()<=0||root.ActualHeight()<=0)return;
        data->input(to_string(O({{L"type",S(L"chrome")},{L"event",event},{L"facts",facts},{L"viewport",viewport()}}).Stringify()));
        if(data->chrome.HasKey(L"contact_tab"))data->chrome.Remove(L"contact_tab");
    }
    J target(Windows::Foundation::IInspectable const& original)const{
        auto node=original.try_as<DependencyObject>();
        while(node&&node!=root){
            if(auto element=node.try_as<FrameworkElement>())
                if(auto tag=element.Tag().try_as<J>();tag&&tag.HasKey(L"workspace_action"))return tag;
            node=VisualTreeHelper::GetParent(node);
        }
        return {};
    }
    A tabHits(){
        A result;
        for(auto it=tabs.begin();it!=tabs.end();){
            auto element=it->get();
            if(!element){it=tabs.erase(it);continue;}++it;
            if(!element.IsLoaded()||element.ActualWidth()<=0||element.ActualHeight()<=0)continue;
            auto tag=element.Tag().try_as<J>();if(!tag)continue;
            auto tab=object(tag,L"workspace_tab");if(!tab.Size())continue;
            auto bounds=visibleBounds(element,root);
            if(bounds.Width>0&&bounds.Height>0)
                result.Append(O({{L"group",N(num(tab,L"group"))},{L"index",N(num(tab,L"index"))},{L"bounds",rect(bounds)}}));
        }
        return result;
    }
    // Every input phase remains ordered; only published presentation is replaceable.
    void send(hstring phase){
        if(str(action,L"type")==L"tile_drag")return;
        auto next=J::Parse(action.Stringify());
        next.Insert(L"phase",S(phase));next.Insert(L"position",point(position));next.Insert(L"viewport",viewport());
        if(str(action,L"type")==L"drag_workspace")next.Insert(L"tabs",tabHits());
        if(phase==L"down"&&tabCapture.Size())next=O({{L"windows_tab_drag",tabCapture},{L"action",next}});
        data->dispatch(next);
    }
    void restoreScrolling(){
        for(auto const& [weak,token]:scrollObservers)if(auto scroll=weak.get())scroll.DirectManipulationStarted(token);
        scrollObservers.clear();
        for(auto const& [weak,mode]:scrollModes)if(auto element=weak.get())element.ManipulationMode(mode);
        scrollModes.clear();pressedPath.clear();
    }
    void rememberPath(Windows::Foundation::IInspectable const& original){
        pressedPath.clear();
        auto parent=original.try_as<DependencyObject>();
        while(parent&&parent!=root){
            if(auto element=parent.try_as<UIElement>())pressedPath.emplace_back(make_weak(element));
            parent=VisualTreeHelper::GetParent(parent);
        }
    }
    void claimScrolling(){
        if(!scrollModes.empty())return;
        for(auto const& weak:pressedPath)if(auto element=weak.get())if(auto scroll=element.try_as<ScrollViewer>())
            if(auto content=scroll.Content().try_as<UIElement>()){
                auto mode=content.ManipulationMode();
                scrollModes.emplace_back(make_weak(content),mode);
                content.ManipulationMode(mode&~ManipulationModes::System);
            }
    }
    void clear(bool closeMenu=true){
        contactToken=0;
        ++generation;pointer.reset();contact=nullptr;held=false;dragging=false;finishing=false;dirty=false;
        stopRecognition();
        if(closeMenu)hideMenu();
        hint.Visibility(Visibility::Collapsed);timer.Stop();tabSlide->Clear();tabCapture=J{};
        releasing=true;root.ReleasePointerCaptures();releasing=false;
        restoreScrolling();action=J{};sourceTag=J{};source={};contextOnly=false;
        chrome(O({{L"kind",S(L"refresh")}}));deferClick();evidence();
    }
    bool cancel(hstring reason=L"cancel"){
        if(!pointer&&!dragging&&!menuOpen&&!menuPending)return false;
        if(trace)lastCancel=O({{L"generation",N(double(generation))},{L"reason",S(reason)},{L"source",object(action,L"item")}});
        // Dismissing a menu alone has no pointer click to suppress. In
        // particular, a menu action may be followed immediately by UIA Invoke.
        if(pointer||dragging)ignoreClick=true;
        if(dragging)send(L"cancel");
        clear();return true;
    }
    void dropQuery(){
        if(!dragging||busy||!dirty||str(action,L"type")!=L"tile_drag"||!object(action,L"item").Size())return;
        auto request=O({{L"type",S(L"workspace_drag_preview")},{L"item",object(action,L"item")},{L"position",point(position)},
            {L"tabs",tabHits()},{L"expansion",data->chrome.GetNamedValue(L"expanded_panel",JsonValue::CreateNullValue())}});
        auto serial=generation,at=motion;bool final=finishing;busy=true;
        if(!QueryWorkspace(data->query,request,[weak=weak_from_this(),serial,at,final](J reply){
            if(auto self=weak.lock()){
                self->busy=false;
                if(serial!=self->generation)return;
                auto preview=object(reply,L"result");
                if(at!=self->motion){self->dropQuery();return;}
                auto result=object(preview,L"drop");
                if(final){
                    if(!self->ownsFocus()||!self->current()){self->cancel(L"final_invalid");return;}
                    auto next=object(result,L"action");
                    if(next.Size())self->data->dispatch(next);
                    self->clear();return;
                }
                auto bounds=object(result,L"bounds");
                if(bounds.Size()){
                    place(self->hint,bounds);self->hint.Visibility(Visibility::Visible);self->refresh();
                }else self->hint.Visibility(Visibility::Collapsed);
                self->evidence();
            }
        }))busy=false;else dirty=false;
    }
    bool claim(){
        if(!pointer||!contact||!current())return false;
        if(owns(root))return true;
        // Preserve native scrolling until a hold wins. Once admitted, transfer
        // a child Button's capture to the stable Canvas before reparenting.
        releasing=true;
        if(auto element=source.get())element.CancelDirectManipulations();
        for(auto const& weak:pressedPath)if(auto element=weak.get();element&&owns(element))element.ReleasePointerCapture(contact);
        claimScrolling();
        bool captured=root.CapturePointer(contact);
        releasing=false;
        if(!captured)cancel(L"capture_failed");
        return captured;
    }
    void begin(){
        if(!claim())return;
        ignoreClick=true;hideMenu();stopRecognition();
        dragging=true;++generation;position=origin;
        tabCapture=tabSlide->Begin();
        chrome(O({{L"kind",S(L"refresh")}}));send(L"down");evidence();
    }
    void hold(NativeInput::HoldingEventArgs const& e){
        if(e.HoldingState()!=NativeInput::HoldingState::Started||!pointer||held||dragging)return;
        if(device==NativeInput::PointerDeviceType::Mouse&&!needsHold)return;
        if(!ownsFocus()||!current()){cancel(L"source_invalid");return;}
        if(!claim())return;
        held=true;ignoreClick=true;
        chrome(O({{L"kind",S(L"refresh")}}));evidence();
        if(device!=NativeInput::PointerDeviceType::Mouse)context(object(sourceTag,L"workspace_context"),position,true);
    }
    void down(PointerRoutedEventArgs const& e){
        if(pointer){if(*pointer!=e.Pointer().PointerId())cancel(L"additional_contact");return;}
        if(dragging||data->externalPopup||data->popupCount)return;
        ignoreClick=false;rejectedPointer.reset();
        auto p=e.GetCurrentPoint(root);if(!p.IsInContact())return;
        if(p.Properties().IsRightButtonPressed()||p.Properties().IsBarrelButtonPressed())return;
        if(p.PointerDeviceType()==NativeInput::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed())return;
        auto tag=target(e.OriginalSource());
        auto tab=object(tag,L"workspace_tab");
        if(tab.Size())data->chrome.Insert(L"contact_tab",S(str(tab,L"panel")));
        chrome(O({{L"kind",S(L"contact")},{L"position",point(p.Position())},{L"canvas",B(false)}}));
        if(!tag.Size())return;
        // Zen still exposes pen/touch context menus, but never moves its
        // projected toolbars. Mouse retains ordinary long button presses.
        bool zen=flag(data->model,L"partial_zen");
        if(zen&&(p.PointerDeviceType()==NativeInput::PointerDeviceType::Mouse||!object(tag,L"workspace_context").Size()))return;
        action=object(tag,L"workspace_action");if(!action.Size())return;
        ++generation;hideMenu();sourceTag=tag;contextOnly=zen;needsHold=zen||flag(tag,L"workspace_hold");held=false;
        rememberPath(e.OriginalSource());
        for(auto const& weak:pressedPath)if(auto element=weak.get().try_as<FrameworkElement>()){
            if(element.Tag()==tag){source=make_weak(element);break;}
        }
        origin=position=p.Position();pointer=p.PointerId();contact=e.Pointer();device=p.PointerDeviceType();
        contactToken=(++contactVersion<<32)|uint64_t(p.PointerId());
        owner=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);
        auto dpi=GetDpiForWindow(owner);
        slopX=std::max(2.,double(GetSystemMetricsForDpi(SM_CXDRAG,dpi))*96./std::max(96u,dpi));
        slopY=std::max(2.,double(GetSystemMetricsForDpi(SM_CYDRAG,dpi))*96./std::max(96u,dpi));
        tabSlide->Grab(tab,tabs);
        // Handles and tabs keep immediate drag arbitration. Tile bodies defer
        // both capture and scroll interception until the native hold starts.
        if(!needsHold)claimScrolling();
        if(needsHold||device!=NativeInput::PointerDeviceType::Mouse){
            recognizing=true;
            recognizer.GestureSettings(NativeInput::GestureSettings::Hold|
                (needsHold?NativeInput::GestureSettings::HoldWithMouse:NativeInput::GestureSettings::None));
            recognizer.ProcessDownEvent(p);
        }
        timer.Start();evidence();
        if(needsHold)for(auto const& weak:pressedPath)if(auto element=weak.get())if(auto scroll=element.try_as<ScrollViewer>()){
            auto epoch=contactToken;
            auto token=scroll.DirectManipulationStarted([weak=weak_from_this(),epoch](auto&&,auto&&){
                if(auto self=weak.lock();self&&self->pointer&&self->contactToken==epoch&&!self->held&&!self->dragging&&!self->releasing){
                    self->rejectedPointer=self->pointer;self->cancel(L"native_scroll");
                }
            });
            scrollObservers.emplace_back(make_weak(scroll),token);
        }
        auto kind=str(action,L"type");
        if(kind!=L"drag_workspace"&&kind!=L"tile_drag"){begin();e.Handled(true);}
    }
    void move(PointerRoutedEventArgs const& e){
        if(!pointer||*pointer!=e.Pointer().PointerId()||finishing)return;
        if(!current()){cancel(L"source_invalid");return;}
        auto next=e.GetCurrentPoint(root).Position();
        if(recognizing&&!held)recognizer.ProcessMoveEvents(e.GetIntermediatePoints(root));
        if(!dragging&&crossed(next)){
            if(needsHold&&!held){rejectedPointer=pointer;cancel(L"motion_before_hold");return;}
            if(contextOnly){rejectedPointer=pointer;cancel(L"zen_motion");return;}
            begin();
        }
        if(!dragging)return;
        position=next;++motion;dirty=true;send(L"move");
        if(str(action,L"type")==L"tile_drag")chrome(O({{L"kind",S(L"motion")},{L"position",point(position)}}));
        dropQuery();e.Handled(true);evidence();
    }
    void up(PointerRoutedEventArgs const& e){
        if(rejectedPointer==e.Pointer().PointerId()){rejectedPointer.reset();deferClick();return;}
        if(!pointer||*pointer!=e.Pointer().PointerId())return;
        if(!ownsFocus()){cancel(L"focus_lost");e.Handled(true);return;}
        auto p=e.GetCurrentPoint(root);
        if(p.Properties().IsCanceled()){cancel(L"pointer_canceled");e.Handled(true);return;}
        if(needsHold&&!held&&!dragging&&crossed(p.Position())){cancel(L"motion_before_hold");e.Handled(true);return;}
        if(recognizing){recognizing=false;recognizer.ProcessUpEvent(p);}
        if(trace)lastRelease=O({{L"generation",N(double(generation))},{L"source",object(action,L"item")},
            {L"dragged",B(dragging)},{L"held",B(held)}});
        if(!dragging){bool handled=held;clear(!held);if(handled)e.Handled(true);return;}
        position=p.Position();++motion;
        if(str(action,L"type")==L"tile_drag"){
            finishing=true;dirty=true;dropQuery();
            releasing=true;root.ReleasePointerCaptures();releasing=false;pointer.reset();contact=nullptr;
        }else{send(L"up");clear();}
        e.Handled(true);evidence();
    }
    void context(J const& value,Point at,bool holding=false){
        if(dragging||data->externalPopup||data->popupCount||!value.Size())return;
        auto serial=++menuGeneration;menuPending=true;
        if(!holding)owner=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);
        if(!QueryWorkspace(data->query,O({{L"type",S(L"context")},{L"target",value}}),
            [weak=weak_from_this(),serial,at,holding](J reply){
                if(auto self=weak.lock()){
                    if(serial!=self->menuGeneration)return;
                    self->menuPending=false;
                    if(self->dragging||self->data->externalPopup||!self->ownsFocus())return;
                    auto model=object(reply,L"result");if(!array(model,L"sections").Size())return;
                    self->menu=MenuFlyout();TrackPopup(self->menu,self->data);
                    self->menu.Opened([weak](auto&&,auto&&){if(auto self=weak.lock()){self->menuOpen=true;self->evidence();}});
                    self->menu.Closed([weak](auto&&,auto&&){if(auto self=weak.lock()){self->menuOpen=false;self->evidence();}});
                    NativeMenuItems(self->menu.Items(),array(model,L"sections"),self->data,
                        [weak](J action){if(auto self=weak.lock()){self->cancel(L"menu_action");self->data->dispatch(action);}});
                    Primitives::FlyoutShowOptions options;options.Position(at);
                    options.ShowMode(holding?Primitives::FlyoutShowMode::Transient:Primitives::FlyoutShowMode::Standard);
                    self->menu.ShowAt(self->root,options);
                }
            }))menuPending=false;
    }
    void doubleClick(DoubleTappedRoutedEventArgs const& e){
        if(flag(data->model,L"partial_zen"))return;
        auto tag=target(e.OriginalSource());if(!flag(tag,L"workspace_double"))return;
        auto sourceAction=object(tag,L"workspace_action");
        if(str(sourceAction,L"type")==L"drag_divider"){
            cancel();
            data->dispatch(O({{L"type",S(L"reset_column_width")},{L"id",N(num(sourceAction,L"id"))},{L"viewport",viewport()}}));
            e.Handled(true);return;
        }
        auto item=object(sourceAction,L"item");if(!item.Size())return;
        cancel();auto serial=++generation;
        QueryWorkspace(data->query,O({{L"type",S(L"panel_handle_target")},{L"item",item}}),
            [weak=weak_from_this(),serial](J reply){
                if(auto self=weak.lock()){
                    if(serial!=self->generation)return;
                    auto group=reply.GetNamedValue(L"result",JsonValue::CreateNullValue());
                    if(group.ValueType()==JsonValueType::Number)self->data->dispatch(O({
                        {L"type",S(L"double_click_panel_handle")},{L"group",group},{L"viewport",self->viewport()}}));
                }
            });
        e.Handled(true);
    }
    void present(J const& drag){
        if(!dragging||str(action,L"type")!=L"drag_workspace")return;
        tabSlide->Update(object(drag,L"tab"));tabSlide->Refresh(tabs);
        auto bounds=object(object(drag,L"drop_hint"),L"bounds");
        if(bounds.Size()){place(hint,bounds);hint.Visibility(Visibility::Visible);}
        else hint.Visibility(Visibility::Collapsed);
        evidence();
    }
    void refresh(){
        uint32_t index;if(!root.Children().IndexOf(hint,index))root.Children().Append(hint);
        hint.Background(selected());hint.BorderBrush(data->brush(L"text"));hint.BorderThickness({1,1,1,1});
        tabSlide->Refresh(tabs);
    }
    void init(){
        tabSlide=std::make_unique<WorkspaceTabDrag>(data,root);
        hint.IsHitTestVisible(false);hint.Visibility(Visibility::Collapsed);Canvas::SetZIndex(hint,10000);
        timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(16));
        timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){
            if((self->pointer||self->finishing)&&!self->ownsFocus()){self->cancel(L"focus_lost");return;}
            if((self->pointer||self->finishing)&&!self->current()){self->cancel(L"source_invalid");return;}
            self->dropQuery();
        }});
        recognizer.Holding([weak=weak_from_this()](auto&&,NativeInput::HoldingEventArgs const& e){if(auto self=weak.lock())self->hold(e);});
        auto weak=weak_from_this();
        root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->down(e);})),true);
        root.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->move(e);})),true);
        root.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->up(e);})),true);
        root.AddHandler(UIElement::PointerExitedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()&&self->needsHold&&!self->held&&!self->dragging
                &&self->crossed(e.GetCurrentPoint(self->root).Position())){
                // Disabled commands can lack Button capture. Leaving their
                // hit target must retire pickup even over the canvas sibling.
                self->rejectedPointer=self->pointer;self->cancel(L"motion_before_hold");
            }
        })),true);
        root.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&!self->releasing){
                if(self->pointer==e.Pointer().PointerId())self->cancel(L"pointer_canceled");
                if(self->rejectedPointer==e.Pointer().PointerId()){self->rejectedPointer.reset();self->deferClick();}
            }
        })),true);
        root.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&!self->releasing&&self->pointer==e.Pointer().PointerId()){
                // A Button releases its capture before the normal routed Up.
                if(e.OriginalSource().try_as<UIElement>()==self->root||e.GetCurrentPoint(self->root).IsInContact())
                    self->cancel(L"capture_lost");
                else if(!self->held&&!self->dragging)self->up(e);
            }
        })),true);
        // Registered sources use our hold recognizer, preserving its contact.
        // Secondary click/barrel and keyboard requests retain native menus.
        // Native text editors handle their own event before it reaches us.
        root.ContextRequested([weak](auto&&,ContextRequestedEventArgs const& e){
            if(auto self=weak.lock();self&&object(self->target(e.OriginalSource()),L"workspace_context").Size()){
                if(self->pointer||self->rejectedPointer||self->ignoreClick){e.Handled(true);return;}
                Point at{};
                if(!e.TryGetPosition(self->root,at)){
                    auto focused=FocusManager::GetFocusedElement(self->root.XamlRoot()).try_as<FrameworkElement>();
                    if(focused){auto bounds=visibleBounds(focused,self->root);at={bounds.X,bounds.Y+bounds.Height};}
                }
                self->context(object(self->target(e.OriginalSource()),L"workspace_context"),at);e.Handled(true);
            }
        });
        root.AddHandler(UIElement::DoubleTappedEvent(),box_value(DoubleTappedEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock())self->doubleClick(e);
        })),true);
        root.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->chrome(O({{L"kind",S(L"refresh")}}));});
        root.PreviewKeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(e.Key()==Windows::System::VirtualKey::Escape&&self->cancel(L"escape"))e.Handled(true);
            else if(!self->pointer){self->ignoreClick=false;self->rejectedPointer.reset();}
        }});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
    }
};
WorkspaceGestures::WorkspaceGestures(std::shared_ptr<WorkspaceData> data,Canvas root):impl(std::make_shared<Impl>()){
    impl->data=std::move(data);impl->root=root;impl->init();
}
WorkspaceGestures::~WorkspaceGestures()=default;
void WorkspaceGestures::Source(FrameworkElement const& element,J const& action,J const& context,bool doubleClick,J const& tab,Pickup pickup){
    auto previous=element.Tag().try_as<J>();
    if(!previous||!previous.HasKey(L"workspace_action")){
        // IsHoldingEnabled is not inherited by template/content children.
        // A second WinUI hold can schedule a context timer against an icon
        // removed by tear-off. This source's native recognizer owns that hold.
        element.Loaded([weak=make_weak(element)](auto&&,auto&&){if(auto element=weak.get())ownHolding(element);});
        if(element.IsLoaded())ownHolding(element);
    }
    element.IsHoldingEnabled(false);element.CanDrag(false);
    element.Tag(O({{L"workspace_action",action},{L"workspace_context",context},{L"workspace_double",B(doubleClick)},
        {L"workspace_tab",tab},{L"workspace_hold",B(pickup==Pickup::Hold)}}));
    if(tab.Size())impl->tabs.emplace_back(make_weak(element));
}
void WorkspaceGestures::Refresh(){impl->refresh();}
void WorkspaceGestures::Present(J const& drag){impl->present(drag);}
void WorkspaceGestures::ChromeChanged(){impl->chrome(O({{L"kind",S(L"refresh")}}));}
bool WorkspaceGestures::Cancel(){return impl->cancel();}
bool WorkspaceGestures::SuppressClick()const{return impl->ignoreClick;}
