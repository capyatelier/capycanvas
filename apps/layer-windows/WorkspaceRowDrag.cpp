#include "pch.h"
#include "WorkspaceRowDrag.h"
#include "UiControls.h"
#include <chrono>

using namespace CapyUi;
using Windows::Foundation::Point;
namespace NativeInput=Microsoft::UI::Input;
namespace {
bool inside(DependencyObject node,DependencyObject const& parent){
    while(node){if(node==parent)return true;node=VisualTreeHelper::GetParent(node);}return false;
}
ScrollViewer scrollIn(DependencyObject const& node){
    if(auto scroll=node.try_as<ScrollViewer>())return scroll;
    for(int i=0;i<VisualTreeHelper::GetChildrenCount(node);++i)
        if(auto found=scrollIn(VisualTreeHelper::GetChild(node,i)))return found;
    return nullptr;
}
}
struct WorkspaceRowDrag::Impl:std::enable_shared_from_this<Impl>{
    ListView list{nullptr};
    ScrollViewer observedScroll{nullptr};
    bool panning=false;
    Grid surface{nullptr};
    Border hint;
    MenuFlyout visibleMenu{nullptr};
    std::function<bool()> available;
    Move move;
    std::function<void(Id)> select;
    std::function<void()> ended;
    struct Row{
        weak_ref<ListViewItem> item;
        weak_ref<Button> grip,more;
        weak_ref<MenuFlyout> menu;
        hstring id;
    };
    std::vector<Row> rows;
    std::vector<hstring> order;
    std::optional<Row> source;
    Input::Pointer pointer{nullptr};
    NativeInput::GestureRecognizer recognizer;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::vector<weak_ref<UIElement>> path;
    std::vector<std::pair<weak_ref<UIElement>,ManipulationModes>> scrollModes;
    Point origin{},position{};
    NativeInput::PointerDeviceType device=NativeInput::PointerDeviceType::Mouse;
    HWND owner=nullptr;
    std::optional<hstring> before;
    double slopX=4,slopY=4,sourceOpacity=1;
    bool held=false,dragging=false,grip=false,child=false,releasing=false;
    bool ignoreClick=false,suppressContext=false,valid=false,recognizing=false;
    bool trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    uint64_t generation=0;
    J lastRelease,lastCancel;
    std::chrono::steady_clock::time_point tickAt;
    void evidence(){
        if(!trace)return;
        hstring phase=dragging?L"dragging":held?L"held":pointer?L"pressed":L"idle";
        auto value=O({{L"phase",S(phase)},{L"source",source?S(source->id):JsonValue::CreateNullValue()},
            {L"device",S(device==NativeInput::PointerDeviceType::Mouse?L"mouse":device==NativeInput::PointerDeviceType::Pen?L"pen":L"touch")},
            {L"grip",B(grip)},{L"panning",B(panning)},{L"can_drop",B(valid)},{L"before",before?S(*before):JsonValue::CreateNullValue()}});
        if(lastRelease.Size())value.Insert(L"last_release",lastRelease);
        if(lastCancel.Size())value.Insert(L"last_cancel",lastCancel);
        AutomationProperties::SetItemStatus(list,value.Stringify());
    }
    void deferClick(){
        auto epoch=generation;
        surface.DispatcherQueue().TryEnqueue(Microsoft::UI::Dispatching::DispatcherQueuePriority::Low,
            [weak=weak_from_this(),epoch]{if(auto self=weak.lock();self&&self->generation==epoch&&!self->pointer)self->ignoreClick=false;});
    }
    void clear(bool hideMenu){
        ++generation;auto old=source;source.reset();pointer=nullptr;
        bool wasDrag=dragging;held=false;dragging=false;valid=false;before.reset();
        timer.Stop();hint.Visibility(Visibility::Collapsed);
        if(recognizing){recognizing=false;recognizer.CompleteGesture();}
        releasing=true;surface.ReleasePointerCaptures();releasing=false;
        for(auto const& [weak,mode]:scrollModes)if(auto content=weak.get())content.ManipulationMode(mode);
        scrollModes.clear();path.clear();
        if(old){
            if(auto row=old->item.get();row&&wasDrag)row.Opacity(sourceOpacity);
            if(hideMenu)if(auto menu=old->menu.get())menu.Hide();
        }
        ended();deferClick();evidence();
    }
    bool cancel(hstring reason=L"cancel"){
        if(!pointer&&!source)return false;
        if(trace)lastCancel=O({{L"generation",N(double(generation))},{L"reason",S(reason)}});
        ignoreClick=true;clear(true);return true;
    }
    bool ownsFocus()const{return GetAncestor(GetForegroundWindow(),GA_ROOTOWNER)==owner&&!IsIconic(owner);}
    bool current()const{
        if(!source||!available())return false;
        auto row=source->item.get();uint32_t index=0;
        // Scrolling may unrealize a retained ListViewItem. Only logical removal
        // invalidates an accepted drag; capture belongs to the stable surface.
        return row&&(dragging||held||row.IsLoaded())&&list.Items().IndexOf(row,index)
            &&std::find(order.begin(),order.end(),source->id)!=order.end();
    }
    bool owns(UIElement const& node)const{
        if(auto captures=node.PointerCaptures())for(auto contact:captures)
            if(pointer&&contact.PointerId()==pointer.PointerId())return true;
        return false;
    }
    bool claim(){
        if(!current()||!pointer)return false;
        if(owns(surface))return true;
        // Transfer a child Button/ListViewItem capture only after this gesture
        // owns the contact. Touch/pen row bodies retain system panning before hold.
        releasing=true;
        if(auto row=source->item.get())row.CancelDirectManipulations();
        for(auto const& weak:path)if(auto node=weak.get();node&&owns(node))node.ReleasePointerCapture(pointer);
        for(auto const& weak:path)if(auto node=weak.get())if(auto scroll=node.try_as<ScrollViewer>())
            if(auto content=scroll.Content().try_as<UIElement>()){
                auto mode=content.ManipulationMode();scrollModes.emplace_back(make_weak(content),mode);
                content.ManipulationMode(mode&~ManipulationModes::System);
            }
        bool captured=surface.CapturePointer(pointer);
        releasing=false;
        if(!captured)cancel();
        return captured;
    }
    void menuAt(Row const& row,Point at,bool holding){
        if(auto menu=row.menu.get())if(auto item=row.item.get()){
            Primitives::FlyoutShowOptions options;options.Position(at);
            options.ShowMode(holding?Primitives::FlyoutShowMode::Transient:Primitives::FlyoutShowMode::Standard);
            menu.ShowAt(item,options);
        }
    }
    void hold(NativeInput::HoldingEventArgs const& e){
        if(!pointer||held||dragging||child||device==NativeInput::PointerDeviceType::Mouse)return;
        if(e.HoldingState()!=NativeInput::HoldingState::Started)return;
        if(!claim())return;
        held=true;ignoreClick=true;suppressContext=true;
        auto item=source->item.get();item.Focus(FocusState::Pointer);
        auto at=surface.TransformToVisual(item).TransformPoint(position);
        menuAt(*source,at,true);evidence();
    }
    void down(PointerRoutedEventArgs const& e){
        if(pointer){if(pointer.PointerId()!=e.Pointer().PointerId())cancel();return;}
        ignoreClick=false;suppressContext=false;++generation;
        if(!available())return;
        auto original=e.OriginalSource().try_as<DependencyObject>();if(!original)return;
        for(auto const& row:rows)if(auto item=row.item.get();item&&inside(original,item)){
            auto point=e.GetCurrentPoint(surface);
            if(!point.IsInContact())return;
            source=row;pointer=e.Pointer();device=point.PointerDeviceType();
            origin=position=point.Position();grip=inside(original,row.grip.get());
            child=inside(original,row.more.get())||point.Properties().IsRightButtonPressed()||point.Properties().IsBarrelButtonPressed();
            ignoreClick=grip||child;
            owner=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);
            auto dpi=GetDpiForWindow(owner);
            slopX=std::max(2.,double(GetSystemMetricsForDpi(SM_CXDRAG,dpi))*96./std::max(96u,dpi));
            slopY=std::max(2.,double(GetSystemMetricsForDpi(SM_CYDRAG,dpi))*96./std::max(96u,dpi));
            auto node=original;
            while(node&&node!=surface){
                if(auto element=node.try_as<UIElement>())path.emplace_back(make_weak(element));
                node=VisualTreeHelper::GetParent(node);
            }
            if(!child){
                // Grips bypass hold and claim panning immediately, but movement
                // still has to cross the system drag slop.
                if(grip&&!claim())return;
                if(device!=NativeInput::PointerDeviceType::Mouse){
                    recognizing=true;recognizer.ProcessDownEvent(e.GetCurrentPoint(surface));
                }
            }
            tickAt=std::chrono::steady_clock::now();timer.Start();evidence();return;
        }
    }
    bool crossed(Point at)const{return std::abs(at.X-origin.X)>slopX||std::abs(at.Y-origin.Y)>slopY;}
    void target(){
        valid=false;before.reset();hint.Visibility(Visibility::Collapsed);
        if(!dragging||!current()||position.X<0||position.X>surface.ActualWidth()
            ||position.Y<0||position.Y>surface.ActualHeight())return;
        std::optional<hstring> last;double line=0;
        for(auto value:list.Items()){
            auto row=value.try_as<ListViewItem>();if(!row||!row.IsLoaded())continue;
            auto bounds=row.TransformToVisual(surface).TransformBounds({0,0,float(row.ActualWidth()),float(row.ActualHeight())});
            if(bounds.Y+bounds.Height<=0||bounds.Y>=surface.ActualHeight())continue;
            auto id=unbox_value<hstring>(row.Tag());
            if(position.Y<bounds.Y+bounds.Height*.5f){before=id;line=bounds.Y;last.reset();break;}
            last=id;line=bounds.Y+bounds.Height;
        }
        if(last){
            auto found=std::find(order.begin(),order.end(),*last);
            if(found!=order.end()&&++found!=order.end())before=*found;
        }else if(!before)return;
        auto found=std::find(order.begin(),order.end(),source->id);
        std::optional<hstring> successor;
        if(found!=order.end()&&++found!=order.end())successor=*found;
        // Both edges of the source are the same no-op. Shared Rust still
        // validates the final id/before pair against current persisted order.
        if(before==source->id||before==successor)return;
        valid=true;
        hint.Width(std::max(0.,surface.ActualWidth()-16));hint.Margin({8,std::clamp(line,0.,std::max(0.,surface.ActualHeight()-2)),0,0});
        hint.Visibility(Visibility::Visible);
    }
    void motion(PointerRoutedEventArgs const& e){
        if(!pointer||pointer.PointerId()!=e.Pointer().PointerId())return;
        if(!current()){cancel();return;}
        position=e.GetCurrentPoint(surface).Position();
        if(child)return;
        if(recognizing&&!held)recognizer.ProcessMoveEvents(e.GetIntermediatePoints(surface));
        if(!dragging&&crossed(position)){
            if(!grip&&device!=NativeInput::PointerDeviceType::Mouse&&!held){
                // Ordinary scroll motion retires this hold without claiming it.
                cancel();return;
            }
            if(!claim())return;
            dragging=true;ignoreClick=true;suppressContext=true;
            auto row=source->item.get();sourceOpacity=row.Opacity();row.Opacity(.55);
            if(auto menu=source->menu.get())menu.Hide();
        }
        if(dragging){target();e.Handled(true);evidence();}
    }
    void up(PointerRoutedEventArgs const& e){
        if(!pointer||pointer.PointerId()!=e.Pointer().PointerId())return;
        if(!ownsFocus()){cancel(L"focus_lost");e.Handled(true);return;}
        if(e.GetCurrentPoint(surface).Properties().IsCanceled()){cancel(L"pointer_canceled");e.Handled(true);return;}
        if(recognizing){recognizing=false;recognizer.ProcessUpEvent(e.GetCurrentPoint(surface));}
        position=e.GetCurrentPoint(surface).Position();
        if(dragging)target();
        auto id=source->id;auto destination=before;bool commit=dragging&&valid&&current();
        if(trace)lastRelease=O({{L"generation",N(double(generation))},{L"source",S(id)},{L"commit",B(commit)},
            {L"canceled",B(e.GetCurrentPoint(surface).Properties().IsCanceled())},
            {L"before",before?S(*before):JsonValue::CreateNullValue()},
            {L"position",O({{L"x",N(position.X)},{L"y",N(position.Y)}})}});
        bool tapped=!ignoreClick&&!held&&!dragging&&!grip&&!child&&!crossed(position)&&current();
        bool handled=held||dragging||grip||tapped;
        ignoreClick=ignoreClick||handled;
        // A held release keeps its menu; dragging/cancellation closes it.
        clear(dragging);
        if(handled)e.Handled(true);
        if(commit)move(id,destination);
        else if(tapped)select(id);
    }
    void tick(){
        if(!pointer)return;
        if(!ownsFocus()){cancel(L"focus_lost");return;}
        if(!current()){cancel();return;}
        auto now=std::chrono::steady_clock::now();
        double elapsed=std::min(.1,std::chrono::duration<double>(now-tickAt).count());tickAt=now;
        if(!dragging)return;
        auto scroll=observedScroll;if(!scroll)return;
        constexpr double edge=32;
        double direction=position.Y<edge?-std::clamp((edge-position.Y)/edge,0.,1.):
            position.Y>surface.ActualHeight()-edge?std::clamp((position.Y-surface.ActualHeight()+edge)/edge,0.,1.):0;
        if(direction&&position.X>=0&&position.X<=surface.ActualWidth()){
            scroll.ChangeView(nullptr,std::clamp(scroll.VerticalOffset()+direction*480*elapsed,0.,scroll.ScrollableHeight()),nullptr,true);
            target();evidence();
        }
    }
    void attach(ListViewItem const& item,Button const& gripButton,Button const& more,MenuFlyout const& menu,hstring id){
        Row row{make_weak(item),make_weak(gripButton),make_weak(more),make_weak(menu),id};rows.push_back(row);
        menu.Opened([weak=weak_from_this()](auto&& sender,auto&&){if(auto self=weak.lock())self->visibleMenu=sender.template as<MenuFlyout>();});
        menu.Closed([weak=weak_from_this()](auto&& sender,auto&&){if(auto self=weak.lock();self&&self->visibleMenu==sender)self->visibleMenu=nullptr;});
        // WinUI's automatic CanDrag pickup would bypass pen/touch hold policy.
        item.CanDrag(false);item.IsHoldingEnabled(false);gripButton.IsHoldingEnabled(false);
        item.RightTapped([weak=weak_from_this(),row](auto&&,RightTappedRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&self->available()){
                e.Handled(true);
                if(self->suppressContext)return;
                self->cancel();self->ignoreClick=true;
                if(auto target=row.item.get())self->menuAt(row,e.GetPosition(target),false);
            }
        });
        item.KeyDown([weak=weak_from_this(),row](auto&&,KeyRoutedEventArgs const& e){
            bool context=e.Key()==Windows::System::VirtualKey::Application||
                e.Key()==Windows::System::VirtualKey::F10&&(GetKeyState(VK_SHIFT)&0x8000);
            if(context)if(auto self=weak.lock();self&&self->available()){
                self->cancel();self->menuAt(row,{24,24},false);e.Handled(true);
            }
        });
        // The accessible grip opens the same keyboard Move Up/Down commands.
        gripButton.Click([weak=weak_from_this(),row](auto&&,auto&&){
            if(auto self=weak.lock();self&&!self->ignoreClick&&self->available())self->menuAt(row,{24,24},false);
        });
        item.Unloaded([weak=weak_from_this(),id](auto&&,auto&&){
            if(auto self=weak.lock();self&&self->source&&self->source->id==id){
                auto epoch=self->generation;
                self->surface.DispatcherQueue().TryEnqueue([weak,id,epoch]{
                    // Retained rows can be removed/reinserted in one reconciliation.
                    if(auto owner=weak.lock();owner&&owner->generation==epoch&&owner->source&&owner->source->id==id&&!owner->current())owner->cancel();
                });
            }
        });
    }
    void init(){
        hint.Height(2);hint.Background(fill(color(L"#3584e4")));hint.IsHitTestVisible(false);
        hint.HorizontalAlignment(HorizontalAlignment::Left);hint.VerticalAlignment(VerticalAlignment::Top);
        hint.Visibility(Visibility::Collapsed);AutomationProperties::SetAutomationId(hint,L"workspace-manager-drop");
        surface.Children().Append(hint);
        list.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){
            auto scroll=scrollIn(self->list);if(!scroll||scroll==self->observedScroll)return;
            self->observedScroll=scroll;
            scroll.DirectManipulationStarted([weak](auto&&,auto&&){if(auto owner=weak.lock()){owner->panning=true;owner->evidence();}});
            scroll.DirectManipulationCompleted([weak](auto&&,auto&&){if(auto owner=weak.lock()){owner->panning=false;owner->evidence();}});
        }});
        recognizer.GestureSettings(NativeInput::GestureSettings::Hold);
        recognizer.Holding([weak=weak_from_this()](auto&&,NativeInput::HoldingEventArgs const& e){if(auto self=weak.lock())self->hold(e);});
        timer=surface.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(16));
        timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->tick();});
        surface.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,auto&& e){if(auto self=weak.lock())self->down(e);})),true);
        surface.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,auto&& e){if(auto self=weak.lock())self->motion(e);})),true);
        surface.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,auto&& e){if(auto self=weak.lock())self->up(e);})),true);
        auto canceled=PointerEventHandler([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&!self->releasing&&self->pointer&&self->pointer.PointerId()==e.Pointer().PointerId())self->cancel(L"pointer_canceled");
        });
        surface.AddHandler(UIElement::PointerCanceledEvent(),box_value(canceled),true);
        surface.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&!self->releasing&&self->pointer&&self->pointer.PointerId()==e.Pointer().PointerId()){
                // Native buttons release their capture before Click/ItemClick.
                // Preserve that ordinary release and the More button's flyout.
                if(self->child)self->clear(false);
                else if(!self->held&&!self->dragging&&!self->grip&&!e.GetCurrentPoint(self->surface).IsInContact())self->up(e);
                else self->cancel(L"capture_lost");
            }
        })),true);
        surface.PreviewKeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
            if(auto self=weak.lock()){
                if(e.Key()==Windows::System::VirtualKey::Escape&&self->cancel(L"escape"))e.Handled(true);
                else if(!self->pointer)self->ignoreClick=false;
            }
        });
    }
};
WorkspaceRowDrag::WorkspaceRowDrag(ListView list,Grid surface,std::function<bool()> available,Move move,std::function<void(Id)> select,std::function<void()> ended):impl(std::make_shared<Impl>()){
    impl->list=list;impl->surface=surface;impl->available=std::move(available);impl->move=std::move(move);impl->select=std::move(select);impl->ended=std::move(ended);impl->init();
}
WorkspaceRowDrag::~WorkspaceRowDrag(){impl->cancel();impl->timer.Stop();}
void WorkspaceRowDrag::Attach(ListViewItem row,Button grip,Button more,MenuFlyout menu,Id id){impl->attach(row,grip,more,menu,id);}
void WorkspaceRowDrag::Refresh(std::vector<Id> const& order){
    impl->order=order;
    std::erase_if(impl->rows,[](auto const& row){return !row.item.get();});
    if(impl->pointer&&!impl->current())impl->cancel();
}
bool WorkspaceRowDrag::FenceSelection()const{return bool(impl->pointer);}
bool WorkspaceRowDrag::SuppressClick()const{return impl->ignoreClick;}
bool WorkspaceRowDrag::Cancel(){return impl->cancel();}
bool WorkspaceRowDrag::Escape(){
    if(impl->cancel(L"escape"))return true;
    if(impl->visibleMenu){impl->visibleMenu.Hide();return true;}
    return false;
}
