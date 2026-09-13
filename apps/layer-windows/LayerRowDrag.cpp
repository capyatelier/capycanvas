#include "pch.h"
#include "LayerRowDrag.h"
#include "LayersView.h"
#include "WorkspaceQuery.h"
#include <chrono>

using namespace CapyLayers;
using Windows::Foundation::Point;
namespace NativeInput=Microsoft::UI::Input;
namespace {
bool inside(DependencyObject node,DependencyObject const& parent){
    while(node){if(node==parent)return true;node=VisualTreeHelper::GetParent(node);}return false;
}
void ownHolding(DependencyObject const& node){
    // Native text editing owns its selection, holding, and text context menu.
    if(node.try_as<TextBox>())return;
    if(auto element=node.try_as<UIElement>()){element.CanDrag(false);element.IsHoldingEnabled(false);}
    for(int i=0;i<VisualTreeHelper::GetChildrenCount(node);++i)ownHolding(VisualTreeHelper::GetChild(node,i));
}
}
struct LayerRowDrag::Impl:std::enable_shared_from_this<Impl>{
    std::weak_ptr<LayersView> view;
    Grid surface{nullptr};
    ScrollView list{nullptr};
    std::weak_ptr<LayerRow> source;
    Input::Pointer pointer{nullptr};
    std::optional<uint32_t> rejected;
    NativeInput::GestureRecognizer recognizer;
    NativeInput::PointerDeviceType device=NativeInput::PointerDeviceType::Mouse;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::vector<weak_ref<UIElement>> path;
    struct ScrollClaim {ScrollingInputKinds input;ScrollingScrollMode horizontal,vertical;};
    std::optional<ScrollClaim> scrollInput;
    struct Hit {double id;int zone;bool operator==(Hit const&)const=default;};
    std::optional<Hit> hit,answered;
    hstring placement,epoch;
    double id=-1,slopX=4,slopY=4;
    Point origin{},position{};
    double initialScroll=0,sourceOpacity=1;
    HWND windowHandle=nullptr;
    bool grip=false,mask=false,held=false,dragging=false,finishing=false;
    bool releasing=false,recognizing=false,ignoreClick=false,suppressContext=false,busy=false;
    bool trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    uint64_t generation=0,revision=0;
    J lastRelease,lastCancel,lastCaptureLoss;
    std::chrono::steady_clock::time_point tickAt;
    bool focus()const{return windowHandle&&GetAncestor(GetForegroundWindow(),GA_ROOTOWNER)==windowHandle&&!IsIconic(windowHandle);}
    bool crossed(Point at)const{return std::abs(at.X-origin.X)>slopX||std::abs(at.Y-origin.Y)>slopY;}
    bool owns(UIElement const& element)const{
        if(auto captures=element.PointerCaptures())for(auto captured:captures)
            if(pointer&&captured.PointerId()==pointer.PointerId())return true;
        return false;
    }
    bool current()const{
        auto owner=view.lock();if(!owner||epoch!=epochOf(owner->data)||!surface.IsLoaded()||list.Visibility()!=Visibility::Visible)return false;
        auto row=findId(array(owner->data->state,L"layers"),id);
        if(!row.Size())return false;
        auto rename=owner->view().GetNamedValue(L"rename_layer",JsonValue::CreateNullValue());
        if(rename.ValueType()==JsonValueType::Number&&rename.GetNumber()==id)return false;
        if(dragging&&(flag(row,L"locked")||!flag(row,L"can_drop_below")))return false;
        if(!held&&!dragging){auto item=source.lock();if(!item||!item->root.IsLoaded())return false;}
        return true;
    }
    void evidence(){
        if(!trace)return;
        auto owner=view.lock();
        auto value=O({{L"phase",S(finishing?L"finishing":dragging?L"dragging":held?L"held":pointer?L"pressed":L"idle")},
            {L"source",id>=0?N(id):JsonValue::CreateNullValue()},{L"generation",N(double(generation))},
            {L"device",S(device==NativeInput::PointerDeviceType::Mouse?L"mouse":device==NativeInput::PointerDeviceType::Pen?L"pen":L"touch")},
            {L"grip",B(grip)},{L"mask",B(mask)},{L"captured",B(owns(surface))},{L"ignore_click",B(ignoreClick)},
            {L"scroll_claimed",B(scrollInput.has_value())},{L"menu_open",B(owner&&owner->menuOpen)},
            {L"pointer_id",pointer?N(pointer.PointerId()):JsonValue::CreateNullValue()},
            {L"can_drop",B(!placement.empty())},{L"position",S(placement)},
            {L"target",hit?N(hit->id):JsonValue::CreateNullValue()}});
        if(lastRelease.Size())value.Insert(L"last_release",lastRelease);
        if(lastCancel.Size())value.Insert(L"last_cancel",lastCancel);
        if(lastCaptureLoss.Size())value.Insert(L"last_capture_loss",lastCaptureLoss);
        // ScrollView's outer container is absent from the control automation tree.
        if(auto presenter=list.ScrollPresenter())AutomationProperties::SetItemStatus(presenter,value.Stringify());
    }
    void deferClick(){
        auto version=generation;
        surface.DispatcherQueue().TryEnqueue(Microsoft::UI::Dispatching::DispatcherQueuePriority::Low,
            [weak=weak_from_this(),version]{if(auto self=weak.lock();self&&self->generation==version&&!self->pointer&&!self->rejected){self->ignoreClick=false;self->evidence();}});
    }
    void hideMenu(){
        if(auto owner=view.lock()){++owner->menuGeneration;owner->menuPending=false;owner->menuOpen=false;if(owner->menu)owner->menu.Hide();}
    }
    void marks(){
        if(auto owner=view.lock())for(auto const& [element,row]:owner->rows)
            row->highlight(hit&&hit->id==row->id?(placement==L"into"?3:placement==L"above"?1:placement==L"below"?2:0):0);
    }
    void release(){
        releasing=true;surface.ReleasePointerCaptures();
        if(scrollInput){
            list.HorizontalScrollMode(scrollInput->horizontal);list.VerticalScrollMode(scrollInput->vertical);
            list.IgnoredInputKinds(scrollInput->input);scrollInput.reset();
        }
        releasing=false;path.clear();
    }
    void clear(bool closeMenu){
        if(dragging)if(auto row=source.lock())row->root.Opacity(sourceOpacity);
        ++generation;pointer=nullptr;source.reset();held=false;dragging=false;finishing=false;busy=false;
        hit.reset();answered.reset();placement=L"";timer.Stop();
        if(recognizing){recognizing=false;recognizer.CompleteGesture();}
        release();if(closeMenu)hideMenu();
        marks();deferClick();evidence();
    }
    bool cancel(hstring reason=L"cancel"){
        auto owner=view.lock();bool active=bool(pointer)||finishing;
        if(!active&&!(owner&&(owner->menuOpen||owner->menuPending)))return false;
        if(active){
            rejected=pointer?std::optional<uint32_t>(pointer.PointerId()):std::nullopt;
            ignoreClick=true;
            if(trace)lastCancel=O({{L"reason",S(reason)},{L"generation",N(double(generation))}});
        }
        clear(true);return true;
    }
    bool claim(){
        if(!pointer||!current())return false;
        if(owns(surface))return true;
        releasing=true;
        // Ignoring new input does not retire a contact already redirected to
        // InteractionTracker. Disable its scroll axes after pickup wins too;
        // programmatic edge scrolling remains available during the drag.
        scrollInput=ScrollClaim{list.IgnoredInputKinds(),list.HorizontalScrollMode(),list.VerticalScrollMode()};
        list.IgnoredInputKinds(scrollInput->input|ScrollingInputKinds::Touch|ScrollingInputKinds::Pen);
        list.HorizontalScrollMode(ScrollingScrollMode::Disabled);list.VerticalScrollMode(ScrollingScrollMode::Disabled);
        for(auto const& weak:path)if(auto node=weak.get();node&&owns(node))node.ReleasePointerCapture(pointer);
        bool captured=surface.CapturePointer(pointer);releasing=false;
        if(!captured)cancel(L"capture_failed");
        return captured;
    }
    void hold(NativeInput::HoldingEventArgs const& e){
        if(e.HoldingState()!=NativeInput::HoldingState::Started||!pointer||held||dragging||device==NativeInput::PointerDeviceType::Mouse)return;
        if(!claim())return;
        held=true;ignoreClick=true;suppressContext=true;
        if(auto owner=view.lock()){
            auto at=surface.TransformToVisual(list).TransformPoint(position);
            owner->context(id,mask,list,at,true);
        }
        evidence();
    }
    void down(PointerRoutedEventArgs const& e){
        if(finishing)cancel(L"new_contact");
        if(pointer){if(pointer.PointerId()!=e.Pointer().PointerId())cancel(L"multiple_contacts");return;}
        rejected.reset();ignoreClick=false;suppressContext=false;++generation;
        auto owner=view.lock();auto original=e.OriginalSource().try_as<DependencyObject>();
        if(!owner||!original)return;
        auto point=e.GetCurrentPoint(surface);
        if(!point.IsInContact()||point.Properties().IsRightButtonPressed()||point.Properties().IsBarrelButtonPressed())return;
        for(auto const& [element,row]:owner->rows)if(inside(original,row->root)){
            if(row->renaming||inside(original,row->rename)||!row->current())return;
            source=row;id=row->id;epoch=row->epoch;pointer=e.Pointer();device=point.PointerDeviceType();
            grip=inside(original,row->grip);mask=inside(original,row->mask);
            origin=position=point.Position();initialScroll=list.VerticalOffset();windowHandle=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);
            auto dpi=GetDpiForWindow(windowHandle);
            slopX=std::max(2.,double(GetSystemMetricsForDpi(SM_CXDRAG,dpi))*96./std::max(96u,dpi));
            slopY=std::max(2.,double(GetSystemMetricsForDpi(SM_CYDRAG,dpi))*96./std::max(96u,dpi));
            for(auto node=original;node&&node!=surface;node=VisualTreeHelper::GetParent(node))
                if(auto item=node.try_as<UIElement>())path.emplace_back(make_weak(item));
            if(grip){ignoreClick=true;if(!claim())return;}
            if(device!=NativeInput::PointerDeviceType::Mouse){recognizing=true;recognizer.ProcessDownEvent(point);}
            tickAt=std::chrono::steady_clock::now();timer.Start();evidence();return;
        }
    }
    std::optional<Hit> pick()const{
        auto at=surface.TransformToVisual(list).TransformPoint(position);
        if(at.X<0||at.Y<0||at.X>list.ActualWidth()||at.Y>list.ActualHeight())return {};
        if(auto owner=view.lock())for(auto const& [element,row]:owner->rows){
            if(!row->current()||!row->root.IsLoaded())continue;
            auto bounds=row->root.TransformToVisual(list).TransformBounds({0,0,float(row->root.ActualWidth()),float(row->root.ActualHeight())});
            if(at.X>=bounds.X&&at.X<bounds.X+bounds.Width&&at.Y>=bounds.Y&&at.Y<bounds.Y+bounds.Height&&bounds.Height>0)
                return Hit{row->id,std::clamp(int((at.Y-bounds.Y)/bounds.Height*4),0,3)};
        }
        return {};
    }
    void complete(){
        if(!finishing||busy||hit!=answered)return;
        bool commit=current()&&focus()&&hit&&!placement.empty();
        auto owner=view.lock();auto destination=hit;auto document=epoch;auto sourceId=id;
        if(trace)lastRelease=O({{L"source",N(id)},{L"commit",B(commit)},{L"generation",N(double(generation))},
            {L"target",hit?N(hit->id):JsonValue::CreateNullValue()},{L"position",S(placement)}});
        clear(true);
        if(commit&&owner)owner->data->dispatchDocument(layerAction(O({{L"op",S(L"drop")},{L"id",N(sourceId)},
            {L"target",N(destination->id)},{L"fraction",N((destination->zone+.5)/4)}})),document);
    }
    void query(){
        if(!dragging||busy||!hit||hit==answered){complete();return;}
        auto owner=view.lock();if(!owner)return;
        auto version=generation,modelVersion=revision;auto requested=*hit;busy=true;
        auto request=O({{L"type",S(L"layer_drop")},{L"epoch",N(std::stod(to_string(epoch)))},
            {L"id",N(id)},{L"target",N(requested.id)},{L"fraction",N((requested.zone+.5)/4)}});
        if(!QueryWorkspace(owner->data->query,request,[weak=weak_from_this(),version,modelVersion,requested](J packet){
            auto self=weak.lock();if(!self||self->generation!=version)return;self->busy=false;
            if(!self->current()||!self->focus()){self->cancel(L"source_or_focus");return;}
            if(self->revision==modelVersion&&self->hit==requested){
                auto result=object(packet,L"result");self->answered=requested;
                self->placement=num(result,L"epoch",-1)==std::stod(to_string(self->epoch))?str(result,L"position"):L"";
                self->marks();self->evidence();
            }
            self->query();
        }))busy=false;
    }
    void target(){
        auto next=pick();
        if(hit!=next){hit=next;answered.reset();placement=L"";marks();evidence();}
        query();
    }
    void motion(PointerRoutedEventArgs const& e){
        if(!pointer||pointer.PointerId()!=e.Pointer().PointerId()||finishing)return;
        if(!current()){cancel(L"source_invalid");return;}
        position=e.GetCurrentPoint(surface).Position();
        if(recognizing&&!held)recognizer.ProcessMoveEvents(e.GetIntermediatePoints(surface));
        if(!dragging&&crossed(position)){
            if(device!=NativeInput::PointerDeviceType::Mouse&&!grip&&!held){cancel(L"early_motion");return;}
            auto owner=view.lock();auto row=owner?findId(array(owner->data->state,L"layers"),id):J{};
            if(!flag(row,L"can_drop_below")||flag(row,L"locked")){cancel(L"source_locked");return;}
            if(!claim())return;
            dragging=true;ignoreClick=true;suppressContext=true;hideMenu();
            if(auto item=source.lock()){sourceOpacity=item->root.Opacity();item->root.Opacity(.55);}
        }
        if(dragging){target();e.Handled(true);evidence();}
    }
    void up(PointerRoutedEventArgs const& e){
        if(rejected&&*rejected==e.Pointer().PointerId()){rejected.reset();deferClick();return;}
        if(!pointer||pointer.PointerId()!=e.Pointer().PointerId()||finishing)return;
        if(!focus()||e.GetCurrentPoint(surface).Properties().IsCanceled()){cancel(L"release_canceled");return;}
        if(recognizing){recognizing=false;recognizer.ProcessUpEvent(e.GetCurrentPoint(surface));}
        position=e.GetCurrentPoint(surface).Position();
        if(dragging){
            finishing=true;ignoreClick=true;e.Handled(true);release();
            // A release always asks current shared policy again, even when the
            // pointer stayed in a cached target while another action changed it.
            ++revision;answered.reset();placement=L"";target();evidence();
        }else{
            if(held||grip)e.Handled(true);
            if(trace)lastRelease=O({{L"source",N(id)},{L"commit",B(false)},{L"held",B(held)}});
            clear(false);
        }
    }
    void tick(){
        if(!pointer)return;
        if(!focus()||!current()){cancel(L"source_or_focus");return;}
        auto now=std::chrono::steady_clock::now();
        auto elapsed=std::min(.1,std::chrono::duration<double>(now-tickAt).count());tickAt=now;
        if(dragging&&!finishing){
            auto at=surface.TransformToVisual(list).TransformPoint(position);auto height=list.ActualHeight();
            double speed=at.Y<32?-std::clamp((32-at.Y)/32.,0.,1.):at.Y>height-32?std::clamp((at.Y-height+32)/32.,0.,1.):0;
            if(speed&&at.X>=0&&at.X<=list.ActualWidth()){
                ScrollingScrollOptions options(ScrollingAnimationMode::Disabled,ScrollingSnapPointsMode::Ignore);
                list.ScrollBy(0,speed*480*elapsed,options);
            }
            target();
        }else if(finishing)query();
        evidence();
    }
    void init(){
        recognizer.GestureSettings(NativeInput::GestureSettings::Hold);
        recognizer.Holding([weak=weak_from_this()](auto&&,NativeInput::HoldingEventArgs const& e){if(auto self=weak.lock())self->hold(e);});
        timer=surface.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(16));
        timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->tick();});
        surface.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,auto&& e){if(auto self=weak.lock())self->down(e);})),true);
        surface.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,auto&& e){if(auto self=weak.lock())self->motion(e);})),true);
        surface.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,auto&& e){if(auto self=weak.lock())self->up(e);})),true);
        surface.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&!self->releasing&&self->pointer&&self->pointer.PointerId()==e.Pointer().PointerId())self->cancel(L"pointer_canceled");
        })),true);
        surface.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&!self->releasing&&self->pointer&&self->pointer.PointerId()==e.Pointer().PointerId()){
                if(self->trace){
                    auto original=e.OriginalSource();auto node=original.try_as<DependencyObject>();
                    auto point=e.GetCurrentPoint(self->surface);
                    self->lastCaptureLoss=O({{L"surface_owned",B(self->owns(self->surface))},
                        {L"held",B(self->held)},{L"dragging",B(self->dragging)},{L"in_contact",B(point.IsInContact())},
                        {L"canceled",B(point.Properties().IsCanceled())},
                        {L"source_type",S(original?get_class_name(original):hstring{})},
                        {L"source_id",S(node?AutomationProperties::GetAutomationId(node):hstring{})}});
                }
                if(!self->held&&!self->dragging&&!self->grip&&!e.GetCurrentPoint(self->surface).IsInContact())self->up(e);
                else if(e.GetCurrentPoint(self->surface).Properties().IsCanceled())self->cancel(L"pointer_canceled");
                else self->cancel(L"capture_lost");
            }
        })),true);
        list.ViewChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&self->pointer&&!self->held&&!self->grip&&!self->dragging
            &&self->device!=NativeInput::PointerDeviceType::Mouse&&std::abs(self->list.VerticalOffset()-self->initialScroll)>.5)self->cancel(L"native_scroll");});
        surface.PreviewKeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(e.Key()==Windows::System::VirtualKey::Escape&&self->cancel(L"escape"))e.Handled(true);
            else if(!self->pointer){self->rejected.reset();self->ignoreClick=false;self->suppressContext=false;}
        }});
        surface.Unloaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->cancel(L"unloaded");});
    }
};
LayerRowDrag::LayerRowDrag(std::shared_ptr<LayersView> const& view):impl(std::make_shared<Impl>()){
    impl->view=view;impl->surface=view->root;impl->list=view->list;impl->init();
}
LayerRowDrag::~LayerRowDrag(){impl->clear(true);}
void LayerRowDrag::Attach(std::shared_ptr<LayerRow> const& row){
    ownHolding(row->root);row->root.Loaded([](auto&& sender,auto&&){ownHolding(sender.template as<DependencyObject>());});
    // A grip must claim before ScrollPresenter sees Down and redirects the
    // contact to InteractionTracker. Row bodies still leave scrolling enabled.
    row->root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler(
        [weak=std::weak_ptr(impl)](auto&&,auto&& e){if(auto self=weak.lock())self->down(e);})),true);
}
void LayerRowDrag::Refresh(){
    if(impl->pointer){
        if(!impl->current())impl->cancel(L"source_invalid");
        else if(impl->dragging){++impl->revision;impl->answered.reset();impl->placement=L"";impl->marks();impl->query();}
    }
}
void LayerRowDrag::MenuChanged(){impl->evidence();}
bool LayerRowDrag::SuppressClick()const{return impl->ignoreClick;}
bool LayerRowDrag::SuppressContext()const{return impl->suppressContext;}
bool LayerRowDrag::Cancel(){return impl->cancel();}
