#include "pch.h"
#include "NativeJson.h"
#include "WorkspaceGestures.h"
#include "WorkspaceQuery.h"
#include "WorkspaceTabDrag.h"
#include "WorkspaceGeometry.h"
#include "NativeMenus.h"
#include "NativePresentation.h"
#include <winrt/Microsoft.UI.Composition.h>
#include <winrt/Microsoft.UI.Xaml.Hosting.h>
#include <chrono>
#include <optional>

using namespace CapyUi;
using Windows::Foundation::Point;
using Windows::Foundation::Rect;
namespace NativeInput=Microsoft::UI::Input;
namespace {
A point(Point p){A result;result.Append(N(p.X));result.Append(N(p.Y));return result;}
J rect(Rect r){return O({{L"x",N(r.X)},{L"y",N(r.Y)},{L"width",N(r.Width)},{L"height",N(r.Height)}});}
}
struct WorkspaceGestures::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr};
    Border hint;
    MenuFlyout menu{nullptr};
    OwnedPopup menuOwner;
    std::vector<weak_ref<FrameworkElement>> tabs;
    std::vector<weak_ref<UIElement>> pressedPath;
    std::unique_ptr<WorkspaceTabDrag> tabSlide;
    NativePresentation presentation;
    J tabCapture;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::optional<uint32_t> pointer,rejectedPointer;
    J action,sourceTag;
    weak_ref<FrameworkElement> source;
    Input::Pointer contact{nullptr};
    PointerGesture recognizer;
    NativeInput::PointerDeviceType device=NativeInput::PointerDeviceType::Mouse;
    HWND owner=nullptr;
    Point origin{},position{};
    bool dragging=false,finishing=false,busy=false,dirty=false,releasing=false;
    bool needsHold=false,contextOnly=false,held=false,recognizing=false,ignoreClick=false;
    bool trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    uint64_t generation=0,motion=0;
    J lastRelease,lastCancel,lastCaptureLoss,lastDown;
    NativeInput::InputPointerSource inputSource{nullptr};
    NativeInput::PointerPoint inputDown{nullptr};
    event_token inputPressed{},inputMoved{};
    struct Edge{
        uint32_t pointer=0;Input::Pointer contact{nullptr};hstring device,contactClass;
        bool vertical=false,hold=false,claimed=false;Point at{};uint64_t time=0;
        std::optional<Point> inputOffset;
        std::vector<weak_ref<UIElement>> path;
        std::vector<std::pair<weak_ref<UIElement>,ManipulationModes>> scrolls;
        std::vector<std::pair<weak_ref<Panel>,UIElement>> retired;
        std::vector<std::shared_ptr<void>> owners;
        ContentControl shield{nullptr};
    };
    std::optional<Edge> edge;
    uint64_t edgeSerial=0;
    J lastEdge,lastEdgeReply,lastEdgeInput;
    static hstring deviceName(NativeInput::PointerDeviceType type){
        return type==NativeInput::PointerDeviceType::Mouse?L"mouse":type==NativeInput::PointerDeviceType::Pen?L"pen":L"touch";
    }
    bool ownsFocus()const{return owner&&GetAncestor(GetForegroundWindow(),GA_ROOTOWNER)==owner&&!IsIconic(owner);}
    bool crossed()const{return recognizer.Moved();}
    bool owns(UIElement const& element)const{
        return pointer&&PointerGesture::Captured(element,*pointer);
    }
    static void nativeScrollers(DependencyObject const& node,A& result,A& editors,bool editor=false){
        editor|=bool(node.try_as<TextBox>())||bool(node.try_as<RichEditBox>())||bool(node.try_as<PasswordBox>());
        bool native=false;
        if(auto view=node.try_as<ScrollView>())native=(view.IgnoredInputKinds()&(ScrollingInputKinds::Touch|ScrollingInputKinds::Pen))!=(ScrollingInputKinds::Touch|ScrollingInputKinds::Pen);
        else if(auto scroll=node.try_as<ScrollViewer>())native=scroll.HorizontalScrollMode()!=ScrollMode::Disabled||scroll.VerticalScrollMode()!=ScrollMode::Disabled;
        if(native){
            hstring owner;
            for(auto parent=node;parent&&owner.empty();parent=VisualTreeHelper::GetParent(parent))owner=AutomationProperties::GetAutomationId(parent);
            auto value=O({{L"id",S(owner)},{L"type",S(get_class_name(node))}});
            if(auto scroll=node.try_as<ScrollViewer>()){
                value.Insert(L"horizontal",N(int(scroll.HorizontalScrollMode())));value.Insert(L"vertical",N(int(scroll.VerticalScrollMode())));
                value.Insert(L"width",N(scroll.ScrollableWidth()));value.Insert(L"height",N(scroll.ScrollableHeight()));
            }
            (editor?editors:result).Append(value);
        }
        for(int i=0;i<VisualTreeHelper::GetChildrenCount(node);++i)nativeScrollers(VisualTreeHelper::GetChild(node,i),result,editors,editor);
    }
    void evidence(){
        if(!trace)return;
        A scrollers,editors;nativeScrollers(root,scrollers,editors);
        auto value=O({{L"phase",S(finishing?L"finishing":dragging?L"dragging":held?L"held":pointer?L"pressed":L"idle")},
            {L"generation",N(double(generation))},{L"source",object(action,L"item")},{L"requires_hold",B(needsHold)},{L"context_only",B(contextOnly)},
            {L"device",S(deviceName(device))},
            {L"captured",B(owns(root))},{L"menu_open",B(menuOwner.Open())},{L"ignore_click",B(ignoreClick)},{L"native_scrollers",scrollers},{L"editor_scrollers",editors},
            {L"can_drop",B(dragging&&hint.Visibility()==Visibility::Visible)},
            {L"pointer_id",pointer?N(*pointer):JsonValue::CreateNullValue()},
            {L"routed_pointer_id",contact?N(contact.PointerId()):JsonValue::CreateNullValue()},
            {L"recognizing",B(recognizing)}});
        if(lastDown.Size())value.Insert(L"last_down",lastDown);
        if(lastRelease.Size())value.Insert(L"last_release",lastRelease);
        if(lastCancel.Size())value.Insert(L"last_cancel",lastCancel);
        if(lastCaptureLoss.Size())value.Insert(L"last_capture_loss",lastCaptureLoss);
        if(edge){
            auto edgeSource=edge->path.empty()?FrameworkElement{nullptr}:edge->path.front().get().try_as<FrameworkElement>();
            value.Insert(L"edge",O({{L"contact",S(edge->contactClass)},{L"device",S(edge->device)},{L"vertical",B(edge->vertical)},
            {L"source_loaded",B(edgeSource&&edgeSource.IsLoaded())},{L"retired",N(double(edge->retired.size()))},
            {L"position",point(edge->at)},{L"input_mapped",B(edge->inputOffset.has_value())},
            {L"hold",B(edge->hold)},{L"claimed",B(edge->claimed)},{L"captured",B(PointerGesture::Captured(root,edge->pointer))}}));
        }
        if(lastEdge.Size())value.Insert(L"last_edge",lastEdge);
        if(lastEdgeReply.Size())value.Insert(L"edge_reply",lastEdgeReply);
        if(lastEdgeInput.Size())value.Insert(L"edge_input",lastEdgeInput);
        AutomationProperties::SetHelpText(root,value.Stringify());
    }
    bool current()const{
        if(data->externalPopup)return false;
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
        menuOwner.Cancel();
    }
    void stopRecognition(){
        if(recognizing){recognizing=false;recognizer.CompleteGesture();}
    }
    void deferClick(){
        if(rejectedPointer)return;
        auto epoch=generation;
        root.DispatcherQueue().TryEnqueue(Microsoft::UI::Dispatching::DispatcherQueuePriority::Low,
            [weak=weak_from_this(),epoch]{if(auto self=weak.lock();self&&self->generation==epoch&&!self->pointer&&!self->edge&&!self->dragging&&!self->finishing){self->ignoreClick=false;self->evidence();}});
    }
    ~Impl(){stopInput();if(timer)timer.Stop();if(menu)menu.Hide();}
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
        bool control=false;
        while(node&&node!=root){
            if(auto element=node.try_as<FrameworkElement>()){
                if(auto tag=element.Tag().try_as<J>();tag&&tag.HasKey(L"workspace_action"))
                    return control&&!object(tag,L"workspace_action").Size()?J{}:tag;
                if(auto input=element.try_as<Control>();input&&!element.try_as<ScrollViewer>()&&!element.try_as<ScrollView>())
                    control|=input.IsTabStop();
                if(auto text=element.try_as<TextBlock>())control|=text.IsTextSelectionEnabled();
            }
            node=VisualTreeHelper::GetParent(node);
        }
        return {};
    }
    static bool scrollContent(DependencyObject const& node){
        auto parent=VisualTreeHelper::GetParent(node);
        return parent&&(parent.try_as<Primitives::ScrollPresenter>()||parent.try_as<ScrollContentPresenter>());
    }
    static void releaseCaptures(std::vector<weak_ref<UIElement>> const& path,Input::Pointer const& contact){
        for(auto const& weak:path)if(auto element=weak.get();element&&PointerGesture::Captured(element,contact.PointerId()))element.ReleasePointerCapture(contact);
    }
    static hstring swipeOwner(DependencyObject const& node,J const& tag,uint32_t id){
        if(tag&&tag.HasKey(L"edge_contact"))return str(tag,L"edge_contact");
        if(node.try_as<TextBox>()||node.try_as<RichEditBox>()||node.try_as<PasswordBox>()||node.try_as<Primitives::RangeBase>()||node.try_as<Primitives::Thumb>())return L"input";
        auto element=node.try_as<UIElement>();if(!element)return {};
        if(!scrollContent(node)&&element.ManipulationMode()==ManipulationModes::None)return L"input";
        if(!node.try_as<Primitives::ButtonBase>()&&PointerGesture::Captured(element,id))return L"input";
        if(auto scroll=node.try_as<ScrollView>();scroll&&scroll.ScrollableWidth()>0)return L"pickup";
        if(auto scroll=node.try_as<ScrollViewer>();scroll&&scroll.ScrollableWidth()>0)return L"pickup";
        return {};
    }
    hstring edgeContact(Windows::Foundation::IInspectable const& original,uint32_t id)const{
        hstring swipes;
        for(auto node=original.try_as<DependencyObject>();node&&node!=root;node=VisualTreeHelper::GetParent(node)){
            J tag{nullptr};if(auto element=node.try_as<FrameworkElement>())tag=element.Tag().try_as<J>();
            if(tag&&str(tag,L"edge_contact")==L"input")return L"input";
            if(swipes.empty()){
                swipes=swipeOwner(node,tag,id);
                if(swipes==L"free"&&tag&&flag(tag,L"workspace_hold")){
                    auto item=object(object(tag,L"workspace_action"),L"item");
                    if(str(item,L"kind")==L"tile"){
                        auto panel=find(array(data->model,L"panels"),L"id",str(item,L"panel"));
                        if(flag(findId(array(panel,L"tiles"),num(item,L"tile")),L"tears_bubble"))swipes=L"input";
                    }
                }
            }
            if(tag&&flag(tag,L"edge_surface"))return swipes.empty()?hstring(L"free"):swipes;
        }
        return {};
    }
    std::string edgeRequest(Edge const& sample,wchar_t const* phase)const{
        auto text=to_string(O({{L"type",S(L"edge_contact")},{L"phase",S(phase)},{L"kind",S(sample.device)},
            {L"contact",S(sample.contactClass)},{L"position",point(sample.at)},{L"viewport",viewport()}}).Stringify());
        return JsonUnsignedFields(std::move(text),{{"id",sample.pointer},{"time_ns",sample.time}});
    }
    void edgeSend(wchar_t const* phase){
        data->edges(edgeRequest(*edge,phase),[weak=weak_from_this(),serial=edgeSerial,phase=std::wstring(phase),time=edge->time](int32_t bits){
            if(auto self=weak.lock()){
                if(self->trace)self->lastEdgeReply=O({{L"phase",S(phase.c_str())},{L"bits",N(bits)},{L"time",N(double(time))}});
                self->edgeReply(serial,phase,bits);self->evidence();
            }
        });
    }
    void edgeReply(uint64_t serial,std::wstring const& phase,int32_t bits){
        if(!edge||serial!=edgeSerial||phase==L"up"||phase==L"cancel")return;
        if(bits<0||!(bits&9)){settleEdge();return;}
        if(edge->claimed)return;
        if(auto cursor=(bits>>4)&7)claimEdge(cursor);
        else edge->hold=(bits&(phase==L"down"?4:2))!=0;
        evidence();
    }
    void claimEdge(int shape){
        edge->claimed=true;edge->hold=false;
        if(pointer||dragging)cancel(L"edge_contact");
        if(rejectedPointer==edge->pointer)rejectedPointer.reset();
        ignoreClick=true;
        releasing=true;
        data->pointerCancels.Cancel(edge->pointer);
        releaseCaptures(edge->path,edge->contact);
        bool captured=root.CapturePointer(edge->contact);
        releasing=false;
        if(!captured){edgeEnd(L"cancel",L"capture_failed");return;}
        if(edge->device==L"touch")return;
        using Shape=NativeInput::InputSystemCursorShape;
        static constexpr Shape shapes[]{Shape::SizeWestEast,Shape::SizeNorthSouth,Shape::SizeWestEast,Shape::SizeNorthSouth,Shape::SizeNorthwestSoutheast,Shape::SizeNortheastSouthwest};
        root.as<IUIElementProtected>().ProtectedCursor(NativeInput::InputSystemCursor::Create(shapes[std::clamp(shape,1,6)-1]));
    }
    static bool scrollable(DependencyObject node){
        for(;node;node=VisualTreeHelper::GetParent(node)){
            if(auto scroll=node.try_as<ScrollView>())return scroll.ScrollableHeight()>0||scroll.ScrollableWidth()>0;
        }
        return false;
    }
    void holdEdgeScrolling(){
        for(auto const& weak:edge->path)if(auto element=weak.get();element&&scrollContent(element)&&element.ManipulationMode()!=ManipulationModes::None
            &&(edge->vertical||!scrollable(element))){
            edge->scrolls.emplace_back(weak,element.ManipulationMode());element.ManipulationMode(ManipulationModes::None);
        }
    }
    void mapInput(){
        if(edge&&inputDown&&edge->pointer==inputDown.PointerId()&&edge->time==inputDown.Timestamp()*1000){
            auto at=inputDown.Position();edge->inputOffset=Point{edge->at.X-at.X,edge->at.Y-at.Y};
        }
    }
    void startInput(){
        if(inputSource||!root.XamlRoot())return;
        inputSource=NativeInput::InputPointerSource::GetForIsland(root.XamlRoot().ContentIsland());
        if(inputSource)inputPressed=inputSource.PointerPressed([weak=weak_from_this()](auto&&,NativeInput::PointerEventArgs const& e){
            if(auto self=weak.lock()){self->inputDown=e.CurrentPoint();self->mapInput();}
        });
    }
    void stopInputMoves(){if(inputSource&&inputMoved.value){inputSource.PointerMoved(inputMoved);inputMoved={};}}
    void stopInput(){
        stopInputMoves();if(inputSource){inputSource.PointerPressed(inputPressed);inputSource=nullptr;}inputDown=nullptr;
    }
    void startInputMoves(){
        if(!inputSource||inputMoved.value)return;
        inputMoved=inputSource.PointerMoved([weak=weak_from_this()](auto&&,NativeInput::PointerEventArgs const& e){
            if(auto self=weak.lock();self&&self->edge&&self->edge->inputOffset){
                auto p=e.CurrentPoint();
                if(self->trace){self->lastEdgeInput=O({{L"pointer",N(p.PointerId())},{L"device",S(deviceName(p.PointerDeviceType()))},
                    {L"position",point(p.Position())},{L"time",N(double(p.Timestamp()*1000))},
                    {L"in_contact",B(p.IsInContact())},{L"canceled",B(p.Properties().IsCanceled())}});self->evidence();}
                if(p.PointerId()!=self->edge->pointer||!p.IsInContact()||p.Properties().IsCanceled())return;
                auto at=p.Position(),offset=*self->edge->inputOffset;
                self->edgeMotion(p,{at.X+offset.X,at.Y+offset.Y});
            }
        });
    }
    void edgeMotion(NativeInput::PointerPoint const& p,Point at){
        auto time=p.Timestamp()*1000;if(edge->time>=time)return;
        edge->at=at;edge->time=time;edgeSend(L"move");
        evidence();
    }
    bool retains(UIElement const& element)const{
        if(!edge)return false;
        for(auto node=element.as<DependencyObject>();node&&node!=root;node=VisualTreeHelper::GetParent(node))
            if(std::any_of(edge->path.begin(),edge->path.end(),[&](auto const& weak){return weak.get()==node;}))return true;
        return false;
    }
    bool deferRemoval(UIElement const& element,std::shared_ptr<void> retainedOwner){
        if(!retains(element))return false;
        auto parent=VisualTreeHelper::GetParent(element).try_as<Panel>();if(!parent)return false;
        if(!edge->shield){
            ContentControl shield;Grid hit;hit.Background(CapyUi::clear());shield.Content(hit);
            shield.HorizontalContentAlignment(HorizontalAlignment::Stretch);shield.VerticalContentAlignment(VerticalAlignment::Stretch);
            shield.Width(root.ActualWidth());shield.Height(root.ActualHeight());
            shield.IsTabStop(true);shield.UseSystemFocusVisuals(false);Canvas::SetZIndex(shield,20000);
            AutomationProperties::SetAccessibilityView(shield,Microsoft::UI::Xaml::Automation::Peers::AccessibilityView::Raw);
            root.Children().Append(shield);edge->shield=shield;AutomationProperties::SetName(shield,AutomationProperties::GetName(root));shield.Focus(FocusState::Programmatic);
        }
        std::function<void(DependencyObject const&)> hideFromNavigation=[&](DependencyObject const& node){
            if(auto control=node.try_as<Control>())control.IsTabStop(false);
            AutomationProperties::SetAccessibilityView(node,Microsoft::UI::Xaml::Automation::Peers::AccessibilityView::Raw);
            for(int n=0;n<VisualTreeHelper::GetChildrenCount(node);++n)hideFromNavigation(VisualTreeHelper::GetChild(node,n));
        };
        hideFromNavigation(element);auto visual=Microsoft::UI::Xaml::Hosting::ElementCompositionPreview::GetElementVisual(element);
        visual.StartAnimation(L"Opacity",visual.Compositor().CreateExpressionAnimation(L"0"));edge->retired.emplace_back(make_weak(parent),element);
        if(retainedOwner)edge->owners.push_back(std::move(retainedOwner));return true;
    }
    void edgeDown(PointerRoutedEventArgs const& e){
        auto p=e.GetCurrentPoint(root);
        if(edge&&edge->pointer==p.PointerId())edgeEnd(L"cancel");
        if(!data->edges||!PointerGesture::PrimaryContact(p))return;
        auto swipes=edgeContact(e.OriginalSource(),p.PointerId());
        Edge next;next.pointer=p.PointerId();next.contact=e.Pointer();next.device=deviceName(p.PointerDeviceType());next.contactClass=swipes.empty()?hstring(L"input"):swipes;
        next.at=p.Position();next.time=p.Timestamp()*1000;
        if(edge){
            data->edges(edgeRequest(next,L"down"),[weak=weak_from_this(),serial=edgeSerial](int32_t bits){
                if(auto self=weak.lock();self&&self->edge&&serial==self->edgeSerial&&(bits&128))self->settleEdge(L"cancel",L"additional_contact");
            });return;
        }
        if(dragging||data->externalPopup||data->popupCount||swipes.empty())return;
        for(auto node=e.OriginalSource().try_as<DependencyObject>();node&&node!=root;node=VisualTreeHelper::GetParent(node))
            if(auto element=node.try_as<UIElement>())next.path.emplace_back(make_weak(element));
        auto request=to_string(O({{L"position",point(next.at)},{L"kind",S(next.device)},{L"contact",S(swipes)}}).Stringify());
        request.insert(request.size()-1,",\"layout\":"+to_string(object(data->model,L"layout").Stringify()));
        auto reach=capy_edge_reach(request.c_str());if(!reach)return;
        next.vertical=next.hold=(reach&2)!=0;
        edge=std::move(next);++edgeSerial;mapInput();startInputMoves();
        if(edge->device!=L"mouse")holdEdgeScrolling();
        edgeSend(L"down");evidence();
    }
    void edgeMove(PointerRoutedEventArgs const& e){
        if(!edge||edge->pointer!=e.Pointer().PointerId())return;
        auto p=e.GetCurrentPoint(root);if(!p.IsInContact())return;
        edgeMotion(p,p.Position());
        if(edge->claimed)e.Handled(true);
    }
    void edgeUp(PointerRoutedEventArgs const& e){
        if(!edge||edge->pointer!=e.Pointer().PointerId())return;
        auto p=e.GetCurrentPoint(root);edge->at=p.Position();edge->time=p.Timestamp()*1000;
        if(edge->claimed)e.Handled(true);
        edgeEnd(p.Properties().IsCanceled()?L"cancel":L"up");
    }
    void edgeEnd(wchar_t const* phase,wchar_t const* reason=L""){
        if(!edge)return;
        edgeSend(phase);
        settleEdge(phase,reason);
    }
    void settleEdge(wchar_t const* phase=L"settled",wchar_t const* reason=L""){
        stopInputMoves();
        for(auto const& [weak,mode]:edge->scrolls)if(auto element=weak.get();element&&element.ManipulationMode()==ManipulationModes::None)element.ManipulationMode(mode);
        if(trace)lastEdge=O({{L"phase",S(phase)},{L"reason",S(reason)},{L"contact",S(edge->contactClass)},{L"device",S(edge->device)},{L"vertical",B(edge->vertical)},{L"claimed",B(edge->claimed)},
            {L"captured",B(PointerGesture::Captured(root,edge->pointer))}});
        bool claimed=edge->claimed;auto released=edge->contact;
        auto retired=std::move(edge->retired);auto owners=std::move(edge->owners);auto shield=edge->shield;edge.reset();++edgeSerial;
        if(claimed){releasing=true;root.ReleasePointerCapture(released);releasing=false;cursor();}
        deferClick();
        for(auto const& [weak,element]:retired)if(auto parent=weak.get()){uint32_t at=0;if(parent.Children().IndexOf(element,at))parent.Children().RemoveAt(at);}
        uint32_t at=0;if(shield&&root.Children().IndexOf(shield,at))root.Children().RemoveAt(at);
        owners.clear();
        evidence();
    }
    void edgeDoublePress(DoubleTappedRoutedEventArgs const& e){
        if(!data->input||data->externalPopup||data->popupCount)return;
        auto swipes=edgeContact(e.OriginalSource(),0);if(swipes.empty())return;
        if(swipes==L"free")for(auto node=e.OriginalSource().try_as<DependencyObject>();node&&node!=root;node=VisualTreeHelper::GetParent(node))
            if(node.try_as<Primitives::ButtonBase>()){swipes=L"pickup";break;}
        auto at=e.GetPosition(root);
        auto request=O({{L"layout",object(data->model,L"layout")},{L"position",point(at)},
            {L"kind",S(deviceName(e.PointerDeviceType()))},{L"contact",S(swipes)},{L"reset",B(true)}});
        if(!capy_edge_reach(to_string(request.Stringify()).c_str()))return;
        cancelAll(L"edge_reset");
        data->input(to_string(O({{L"type",S(L"edge_double_press")},{L"kind",S(deviceName(e.PointerDeviceType()))},{L"contact",S(swipes)},
            {L"position",point(at)},{L"viewport",viewport()}}).Stringify()));
        e.Handled(true);
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
    void send(hstring phase,std::function<void()> ready={}){
        if(str(action,L"type")==L"tile_drag")return;
        auto next=J::Parse(action.Stringify());
        next.Insert(L"phase",S(phase));next.Insert(L"position",point(position));next.Insert(L"viewport",viewport());
        if(str(action,L"type")==L"drag_workspace")next.Insert(L"tabs",tabHits());
        if(phase==L"down"&&tabCapture.Size())next=O({{L"windows_tab_drag",tabCapture},{L"action",next}});
        data->dispatch(next,std::move(ready));
    }
    void rememberPath(Windows::Foundation::IInspectable const& original){
        pressedPath.clear();
        auto parent=original.try_as<DependencyObject>();
        while(parent&&parent!=root){
            if(auto element=parent.try_as<UIElement>())pressedPath.emplace_back(make_weak(element));
            parent=VisualTreeHelper::GetParent(parent);
        }
    }
    void cursor(){
        if(auto shape=resizeCursor(action,object(data->model,L"layout"));shape&&dragging&&device!=NativeInput::PointerDeviceType::Touch)
            root.as<IUIElementProtected>().ProtectedCursor(NativeInput::InputSystemCursor::Create(*shape));
        else dragCursor(root,device==NativeInput::PointerDeviceType::Touch,dragging,held);
    }
    void clear(bool closeMenu=true){
        presentation.Cancel();
        ++generation;pointer.reset();contact=nullptr;held=false;dragging=false;finishing=false;dirty=false;
        stopRecognition();
        if(closeMenu)hideMenu();
        hint.Visibility(Visibility::Collapsed);timer.Stop();tabSlide->Clear();tabCapture=J{};
        releasing=true;root.ReleasePointerCaptures();releasing=false;
        pressedPath.clear();action=J{};sourceTag=J{};source={};contextOnly=false;
        cursor();
        chrome(O({{L"kind",S(L"refresh")}}));deferClick();evidence();
    }
    bool cancel(hstring reason=L"cancel",bool suppressClick=true){
        if(!pointer&&!dragging&&!menuOwner.Active())return false;
        if(trace)lastCancel=O({{L"generation",N(double(generation))},{L"reason",S(reason)},{L"source",object(action,L"item")}});
        // Dismissing a menu alone has no pointer click to suppress. In
        // particular, a menu action may be followed immediately by UIA Invoke.
        if(pointer||dragging)ignoreClick=suppressClick;
        if(contact&&suppressClick){releasing=true;releaseCaptures(pressedPath,contact);releasing=false;}
        if(dragging&&!finishing)send(L"cancel");
        clear();return true;
    }
    bool cancelAll(hstring reason=L"cancel"){
        bool canceled=cancel(reason);
        if(edge){
            releasing=true;releaseCaptures(edge->path,edge->contact);releasing=false;
            edgeEnd(L"cancel",reason.c_str());canceled=true;
        }
        return canceled;
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
                auto result=object(preview,L"drop");
                if(final){
                    if(at!=self->motion){self->dropQuery();return;}
                    if(!self->ownsFocus()||!self->current()){self->cancel(L"final_invalid");return;}
                    auto next=object(result,L"action");
                    if(!next.Size()&&self->data->takeBubble){
                        self->data->takeBubble(object(self->action,L"item"),self->position,
                            [weak,serial]{auto self=weak.lock();return self&&serial==self->generation&&self->current()&&self->ownsFocus();},
                            [weak,serial]{if(auto self=weak.lock();self&&serial==self->generation)self->clear();});
                        return;
                    }
                    if(next.Size())self->data->dispatch(next);
                    self->clear();return;
                }
                self->showHint(result);self->refresh();
                self->evidence();self->dropQuery();
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
        releaseCaptures(pressedPath,contact);
        bool captured=root.CapturePointer(contact);
        releasing=false;
        if(!captured)cancel(L"capture_failed");
        return captured;
    }
    void begin(){
        if(!claim())return;
        if(edge&&!edge->claimed)edgeEnd(L"cancel");
        ignoreClick=true;hideMenu();stopRecognition();
        dragging=true;++generation;position=origin;
        tabSlide->Grab(object(sourceTag,L"workspace_tab"),tabs);
        tabCapture=tabSlide->Begin();
        cursor();
        chrome(O({{L"kind",S(L"refresh")}}));send(L"down");evidence();
    }
    void hold(NativeInput::HoldingEventArgs const& e){
        if(e.HoldingState()!=NativeInput::HoldingState::Started||!pointer||held||dragging)return;
        if(device==NativeInput::PointerDeviceType::Mouse&&!needsHold)return;
        if(!ownsFocus()||!current()){cancel(L"source_invalid");return;}
        if(!claim())return;
        if(edge&&!edge->claimed)edgeEnd(L"cancel");
        held=true;ignoreClick=true;
        cursor();
        chrome(O({{L"kind",S(L"refresh")}}));evidence();
        if(device!=NativeInput::PointerDeviceType::Mouse)context(object(sourceTag,L"workspace_context"),position,true);
    }
    void down(PointerRoutedEventArgs const& e){
        if(edge&&(edge->claimed||edge->shield))return;
        if(pointer){if(*pointer!=e.Pointer().PointerId())cancel(L"additional_contact");return;}
        if(dragging||data->externalPopup||data->popupCount)return;
        ignoreClick=false;rejectedPointer.reset();
        auto p=e.GetCurrentPoint(root);if(!PointerGesture::PrimaryContact(p))return;
        auto tag=target(e.OriginalSource());
        if(flag(tag,L"workspace_control"))return;
        auto tab=object(tag,L"workspace_tab");
        if(tab.Size())data->chrome.Insert(L"contact_tab",S(str(tab,L"panel")));
        chrome(O({{L"kind",S(L"contact")},{L"position",point(p.Position())},{L"canvas",B(false)}}));
        if(!tag.Size())return;
        action=object(tag,L"workspace_action");
        if(!action.Size()&&e.Handled())return;
        // Empty column space offers a menu and double-click, without becoming
        // another drag source. Pen/touch holds retain native scroll arbitration.
        if(!action.Size()&&(p.PointerDeviceType()==NativeInput::PointerDeviceType::Mouse||!object(tag,L"workspace_context").Size()))return;
        ++generation;hideMenu();sourceTag=tag;contextOnly=!action.Size();
        needsHold=contextOnly||flag(tag,L"workspace_hold");held=false;
        rememberPath(e.OriginalSource());
        for(auto const& weak:pressedPath)if(auto element=weak.get().try_as<FrameworkElement>()){
            if(element.Tag()==tag){source=make_weak(element);break;}
        }
        origin=position=p.Position();pointer=p.PointerId();contact=e.Pointer();device=p.PointerDeviceType();
        if(trace)lastDown=O({{L"left",B(p.Properties().IsLeftButtonPressed())},{L"handled",B(e.Handled())},
            {L"in_contact",B(p.IsInContact())},{L"position",point(p.Position())},{L"pointer",N(p.PointerId())}});
        if(trace){
            auto original=e.OriginalSource().try_as<FrameworkElement>();
            lastDown.Insert(L"original",S(original?AutomationProperties::GetAutomationId(original):L""));
            auto element=source.get();auto border=element.try_as<Border>();
            auto button=border?border.Child().try_as<Button>():element.try_as<Button>();
            if(button){lastDown.Insert(L"button_enabled",B(button.IsEnabled()));lastDown.Insert(L"button_pressed",B(button.IsPressed()));lastDown.Insert(L"button_captured",B(owns(button)));}
        }
        owner=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);
        // Handles and tabs keep immediate drag arbitration. Tile bodies defer
        // both capture and scroll interception until the native hold starts.
        if(!needsHold){
            bool nativeButton=false;
            for(auto const& weak:pressedPath)if(auto element=weak.get())nativeButton|=bool(element.try_as<Primitives::ButtonBase>())&&owns(element);
            if(!nativeButton&&!claim())return;
        }
        recognizing=true;
        recognizer.Begin(source.get(),p,NativeInput::GestureSettings::Hold|
            (needsHold?NativeInput::GestureSettings::HoldWithMouse:NativeInput::GestureSettings::None));
        timer.Start();evidence();
        auto kind=str(action,L"type");
        if(tab.Size()&&!(edge&&edge->vertical))data->dispatch(O({{L"type",S(L"select_panel_tab")},{L"group",N(num(tab,L"group"))},{L"panel",S(str(tab,L"panel"))}}));
        if(!contextOnly&&kind!=L"drag_workspace"&&kind!=L"tile_drag"){begin();e.Handled(true);}
    }
    void move(PointerRoutedEventArgs const& e){
        if(!pointer||*pointer!=e.Pointer().PointerId()||finishing)return;
        if(!current()){cancel(L"source_invalid");return;}
        auto next=e.GetCurrentPoint(root).Position();
        if(recognizing)recognizer.Move(root,e);
        if(!dragging&&crossed()){
            // Motion disarms pickup; the native Button still owns a short tap.
            // Tablet jitter inside its bounds must not swallow its Click.
            if(needsHold&&!held){rejectedPointer=pointer;cancel(L"motion_before_hold",false);return;}
            if(contextOnly){rejectedPointer=pointer;cancel(L"context_motion");return;}
            if(edge&&edge->hold)return;
            begin();
        }
        if(!dragging)return;
        position=next;++motion;dirty=true;send(L"move");
        if(str(action,L"type")==L"tile_drag")chrome(O({{L"kind",S(L"motion")}}));
        dropQuery();e.Handled(true);evidence();
    }
    void up(PointerRoutedEventArgs const& e){
        if(rejectedPointer==e.Pointer().PointerId()){rejectedPointer.reset();deferClick();return;}
        if(!pointer||*pointer!=e.Pointer().PointerId())return;
        if(!ownsFocus()){cancel(L"focus_lost");e.Handled(true);return;}
        auto p=e.GetCurrentPoint(root);
        if(p.Properties().IsCanceled()){cancel(L"pointer_canceled");e.Handled(true);return;}
        if(needsHold&&!held&&!dragging&&crossed()){cancel(L"motion_before_hold",false);return;}
        if(recognizing){recognizing=false;recognizer.ProcessUpEvent(p);}
        if(trace)lastRelease=O({{L"generation",N(double(generation))},{L"source",object(action,L"item")},
            {L"dragged",B(dragging)},{L"held",B(held)}});
        if(!dragging){bool handled=held;clear(!held);if(handled)e.Handled(true);return;}
        position=p.Position();++motion;
        if(str(action,L"type")==L"tile_drag"){
            finishing=true;dirty=true;dropQuery();
            releasing=true;root.ReleasePointerCaptures();releasing=false;pointer.reset();contact=nullptr;
        }else if(str(action,L"type")==L"drag_workspace"&&tabCapture.Size()){
            finishing=true;timer.Stop();hint.Visibility(Visibility::Collapsed);
            auto serial=generation;
            send(L"up",[weak=weak_from_this(),serial]{if(auto self=weak.lock();self&&self->generation==serial){
                self->presentation.AfterLayout([weak,serial]{if(auto self=weak.lock();self&&self->generation==serial)self->clear();});
            }});
            releasing=true;root.ReleasePointerCaptures();releasing=false;pointer.reset();contact=nullptr;
        }else{send(L"up");clear();}
        e.Handled(true);evidence();
    }
    void context(J const& value,Point at,bool holding=false,FrameworkElement anchor=nullptr){
        if(dragging||data->externalPopup||data->popupCount||!value.Size())return;
        if(!anchor)anchor=source.get();if(!anchor)anchor=root;
        hideMenu();menu=MenuFlyout();auto ticket=menuOwner.Begin(data,anchor,menu);
        auto weak=weak_from_this();
        menu.Opened([weak](auto&&,auto&&){if(auto self=weak.lock())self->evidence();});
        menu.Closed([weak](auto&&,auto&&){if(auto self=weak.lock())self->evidence();});
        if(!QueryWorkspace(data->query,O({{L"type",S(L"context")},{L"target",value}}),
            [weak,ticket,at,holding](J reply){
                if(auto self=weak.lock()){
                    if(!ticket.Current())return;
                    auto model=object(reply,L"result");if(self->dragging||!array(model,L"sections").Size()){ticket.Cancel();return;}
                    NativeMenuItems(self->menu.Items(),array(model,L"sections"),self->data,
                        [weak](J action){if(auto self=weak.lock()){self->cancel(L"menu_action");self->data->dispatch(action);}});
                    Primitives::FlyoutShowOptions options;options.Position(at);
                    options.ShowMode(holding?Primitives::FlyoutShowMode::Transient:Primitives::FlyoutShowMode::Standard);
                    ticket.Show(self->root,options);
                }
            }))ticket.Cancel();
    }
    void doubleClick(DoubleTappedRoutedEventArgs const& e){
        auto tag=target(e.OriginalSource());if(!flag(tag,L"workspace_double")){edgeDoublePress(e);return;}
        auto sourceAction=object(tag,L"workspace_action");
        auto context=object(tag,L"workspace_context");
        if(!sourceAction.Size()&&str(context,L"kind")==L"column"){
            cancel();
            data->dispatch(O({{L"type",S(L"customize")},{L"action",O({
                {L"type",S(L"set_column_collapsed")},{L"group",N(num(context,L"column"))},{L"collapsed",B(false)}})}}));
            e.Handled(true);return;
        }
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
    void showHint(J const& value){
        auto bounds=object(value,L"bounds");
        if(!bounds.Size()){hint.Visibility(Visibility::Collapsed);return;}
        bool body=str(object(value,L"target"),L"kind")==L"tab"&&num(bounds,L"width")>3&&num(bounds,L"height")>3;
        hint.Background(body?data->tint(L"accent",64):accent(data));hint.BorderBrush(accent(data));
        hint.BorderThickness(body?Thickness{2,2,2,2}:Thickness{});
        hint.CornerRadius(body?CornerRadius{}:CornerRadius{2,2,2,2});
        place(hint,bounds);hint.Visibility(Visibility::Visible);
    }
    void present(J const& drag){
        if(!dragging||finishing||str(action,L"type")!=L"drag_workspace")return;
        tabSlide->Update(object(drag,L"tab"));tabSlide->Refresh(tabs);
        showHint(object(drag,L"drop_hint"));
        evidence();
    }
    void refresh(){
        uint32_t index;if(!root.Children().IndexOf(hint,index))root.Children().Append(hint);
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
        root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock()){self->edgeDown(e);self->down(e);}})),true);
        root.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock()){self->edgeMove(e);self->move(e);}})),true);
        root.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock()){self->up(e);self->edgeUp(e);}})),true);
        root.AddHandler(UIElement::PointerExitedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()&&!self->owns(self->root)){
                if(!self->current()){self->cancel(L"source_invalid");return;}
                for(auto const& entry:self->pressedPath)if(auto element=entry.get();element&&self->owns(element))return;
                auto source=self->source.get();if(source&&!PointerGesture::Outside(source,e))return;
                self->clear(false);
            }
        })),true);
        root.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&!self->releasing){
                if(e.OriginalSource().template try_as<UIElement>()!=self->root&&PointerGesture::Captured(self->root,e.Pointer().PointerId()))return;
                if(self->pointer==e.Pointer().PointerId())self->cancel(L"pointer_canceled");
                if(self->edge&&self->edge->pointer==e.Pointer().PointerId())self->edgeEnd(L"cancel",L"pointer_canceled");
                if(self->rejectedPointer==e.Pointer().PointerId()){self->rejectedPointer.reset();self->deferClick();}
            }
        })),true);
        root.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&!self->releasing&&e.OriginalSource().template try_as<UIElement>()!=self->root&&PointerGesture::Captured(self->root,e.Pointer().PointerId()))return;
            if(auto self=weak.lock();self&&!self->releasing&&self->edge&&self->edge->pointer==e.Pointer().PointerId()&&e.GetCurrentPoint(self->root).IsInContact()
                &&(!self->edge->claimed||e.OriginalSource().try_as<UIElement>()==self->root))self->edgeEnd(L"cancel",L"capture_lost");
            if(auto self=weak.lock();self&&!self->releasing&&self->pointer==e.Pointer().PointerId()){
                if(self->trace){
                    auto source=e.OriginalSource().try_as<UIElement>();
                    auto p=e.GetCurrentPoint(self->root);
                    self->lastCaptureLoss=O({{L"generation",N(double(self->generation))},
                        {L"source_is_root",B(source==self->root)},{L"root_captured",B(self->owns(self->root))},
                        {L"source_name",S(source?AutomationProperties::GetAutomationId(source):L"")},
                        {L"root_hit_test_visible",B(self->root.IsHitTestVisible())},{L"external_popup",B(self->data->externalPopup)},
                        {L"source_loaded",B(self->source.get()&&self->source.get().IsLoaded())},
                        {L"in_contact",B(p.IsInContact())},{L"canceled",B(p.Properties().IsCanceled())},{L"dragging",B(self->dragging)},
                        {L"position",point(p.Position())}});
                }
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
                FrameworkElement anchor{nullptr};
                for(auto node=e.OriginalSource().try_as<DependencyObject>();node&&node!=self->root;node=VisualTreeHelper::GetParent(node))
                    if(auto element=node.try_as<FrameworkElement>();element&&element.Tag().try_as<J>()){anchor=element;break;}
                self->context(object(self->target(e.OriginalSource()),L"workspace_context"),at,false,anchor);e.Handled(true);
            }
        });
        root.AddHandler(UIElement::DoubleTappedEvent(),box_value(DoubleTappedEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock())self->doubleClick(e);
        })),true);
        root.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->chrome(O({{L"kind",S(L"refresh")}}));});
        ShortcutKeys(root,[weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(e.Key()==Windows::System::VirtualKey::Escape&&self->cancelAll(L"escape"))e.Handled(true);
            else if(!self->pointer){self->ignoreClick=false;self->rejectedPointer.reset();}
        }});
        root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->startInput();});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->cancelAll();self->stopInput();}});
        if(root.IsLoaded())startInput();
    }
};
WorkspaceGestures::WorkspaceGestures(std::shared_ptr<WorkspaceData> data,Canvas root):impl(std::make_shared<Impl>()){
    impl->data=std::move(data);impl->root=root;impl->init();
    impl->data->edgeHolding=[weak=std::weak_ptr(impl)](uint32_t id){auto self=weak.lock();
        return self&&self->edge&&self->edge->pointer==id&&(self->edge->hold||self->edge->claimed);};
    impl->data->cancelEdge=[weak=std::weak_ptr(impl)](uint32_t id){
        if(auto self=weak.lock();self&&self->edge&&self->edge->pointer==id){
            if(self->edge->claimed)return false;
            self->edgeEnd(L"cancel");
        }
        return true;
    };
}
WorkspaceGestures::~WorkspaceGestures()=default;
void WorkspaceGestures::Source(FrameworkElement const& element,J const& action,J const& context,bool doubleClick,J const& tab,Pickup pickup){
    auto previous=element.Tag().try_as<J>();
    if(!previous||!previous.HasKey(L"workspace_action")){
        PointerGesture::OwnHolding(element);
    }
    auto tag=previous?J::Parse(previous.Stringify()):J{};
    tag.Insert(L"workspace_action",action);tag.Insert(L"workspace_context",context);tag.Insert(L"workspace_double",B(doubleClick));
    tag.Insert(L"workspace_tab",tab);tag.Insert(L"workspace_hold",B(pickup==Pickup::Hold));tag.Insert(L"workspace_control",B(pickup==Pickup::Control));
    auto kind=str(action,L"type");
    auto contact=tab.Size()||pickup==Pickup::Sideways?L"pickup":pickup==Pickup::Hold?L"free"
        :pickup==Pickup::Control||kind==L"drag_workspace"?L"input":L"";
    if(*contact)tag.Insert(L"edge_contact",S(contact));
    element.Tag(tag);
    if(tab.Size())impl->tabs.emplace_back(make_weak(element));
}
void WorkspaceGestures::Refresh(){impl->refresh();}
void WorkspaceGestures::Present(J const& drag){impl->present(drag);}
void WorkspaceGestures::ChromeChanged(){impl->chrome(O({{L"kind",S(L"refresh")}}));}
bool WorkspaceGestures::Cancel(){return impl->cancelAll();}
bool WorkspaceGestures::SuppressClick()const{return impl->ignoreClick;}
bool WorkspaceGestures::DeferRemoval(UIElement const& element,std::shared_ptr<void> owner){return impl->deferRemoval(element,std::move(owner));}
bool WorkspaceGestures::Retains(UIElement const& element)const{return impl->retains(element);}
