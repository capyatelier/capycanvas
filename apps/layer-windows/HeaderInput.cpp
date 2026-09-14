#include "pch.h"
#include "HeaderInput.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceQuery.h"
#include "NativeMenus.h"
#include <chrono>

using namespace CapyUi;
using Windows::Foundation::Point;
namespace NativeInput=Microsoft::UI::Input;
namespace {
A point(Point p){A result;result.Append(N(p.X));result.Append(N(p.Y));return result;}
J edit(J const& action){return O({{L"type",S(L"customize")},{L"action",O({{L"type",S(L"header")},{L"action",action}})}});}
void ownHolding(DependencyObject const& node){
    if(auto element=node.try_as<UIElement>())element.IsHoldingEnabled(false);
    for(int i=0;i<VisualTreeHelper::GetChildrenCount(node);++i)ownHolding(VisualTreeHelper::GetChild(node,i));
}
}
struct HeaderInput::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr};
    std::function<void()> changed;
    J model,configuration,source,beginRequest,preview;
    hstring identity;
    weak_ref<FrameworkElement> originElement;
    std::vector<weak_ref<UIElement>> pressedPath;
    std::vector<std::pair<weak_ref<UIElement>,ManipulationModes>> scrollModes;
    std::optional<uint32_t> pointer;
    Input::Pointer contact{nullptr};
    NativeInput::GestureRecognizer recognizer;
    NativeInput::PointerDeviceType device=NativeInput::PointerDeviceType::Mouse;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    MenuFlyout menu{nullptr};
    Point origin{},position{};
    HWND owner=nullptr;
    uint32_t selected=0;
    uint64_t generation=0,motion=0,menuGeneration=0;
    double slopX=4,slopY=4;
    bool editing=false,dragging=false,starting=false,started=false,busy=false,ending=false,cancelled=false;
    bool dirty=false,held=false,recognizing=false,releasing=false,menuOpen=false;
    bool trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    ~Impl(){if(timer)timer.Stop();if(menu)menu.Hide();}
    bool focused()const{return owner&&GetAncestor(GetForegroundWindow(),GA_ROOTOWNER)==owner&&!IsIconic(owner);}
    bool captured(UIElement const& element)const{
        if(pointer)if(auto captures=element.PointerCaptures())for(auto p:captures)if(p.PointerId()==*pointer)return true;
        return false;
    }
    bool active()const{return pointer||starting||started||busy||ending;}
    void evidence(){
        if(!trace)return;
        AutomationProperties::SetHelpText(root,O({
            {L"phase",S(ending?L"finishing":dragging?L"dragging":held?L"held":pointer?L"pressed":L"idle")},
            {L"generation",N(double(generation))},{L"source",source},{L"requires_hold",B(false)},
            {L"device",S(device==NativeInput::PointerDeviceType::Mouse?L"mouse":device==NativeInput::PointerDeviceType::Pen?L"pen":L"touch")},
            {L"captured",B(captured(root))},{L"menu_open",B(menuOpen)},{L"preview",preview}}).Stringify());
    }
    void notify(){evidence();if(changed)changed();}
    void hideMenu(){++menuGeneration;if(menu)menu.Hide();}
    void stopRecognition(){if(recognizing){recognizing=false;recognizer.CompleteGesture();}}
    void release(){
        releasing=true;if(root.IsLoaded()&&captured(root))root.ReleasePointerCaptures();
        for(auto const& [weak,mode]:scrollModes)if(auto element=weak.get())element.ManipulationMode(mode);
        scrollModes.clear();releasing=false;
        pointer.reset();contact=nullptr;pressedPath.clear();stopRecognition();
    }
    void clear(bool keepMenu=false){
        if(!keepMenu)hideMenu();
        release();++generation;starting=started=busy=ending=cancelled=dragging=dirty=held=false;
        source=J{};beginRequest=J{};preview=J{};timer.Stop();notify();
    }
    bool cancel(){
        if(!active()&&!menuOpen)return false;
        cancelled=true;hideMenu();release();preview=J{};
        if(busy||started){ending=true;cancelled=true;pump();notify();}
        else clear();
        return true;
    }
    void deferCancel(){
        if(!active()&&!menuOpen)return;
        // Capture changes are forbidden while XAML is arranging/unloading.
        // Reject a release immediately, then retire capture on the dispatcher.
        cancelled=true;auto serial=generation;
        root.DispatcherQueue().TryEnqueue([weak=weak_from_this(),serial]{
            if(auto self=weak.lock();self&&self->generation==serial)self->cancel();
        });
    }
    bool claim(){
        if(!pointer||!contact)return false;
        if(captured(root))return true;
        releasing=true;
        for(auto const& weak:pressedPath)if(auto element=weak.get()){
            element.CancelDirectManipulations();
            if(captured(element))element.ReleasePointerCapture(contact);
        }
        // Once this placement surface owns the contact, native panning must
        // not steal pen/touch capture. Restore every mode on completion.
        for(auto const& weak:pressedPath)if(auto element=weak.get())if(auto scroll=element.try_as<ScrollViewer>())
            if(auto content=scroll.Content().try_as<UIElement>()){
                auto mode=content.ManipulationMode();scrollModes.emplace_back(make_weak(content),mode);
                content.ManipulationMode(mode&~ManipulationModes::System);
            }
        bool accepted=root.CapturePointer(contact);releasing=false;
        if(!accepted)cancel();
        return accepted;
    }
    void chrome(Point at){
        if(!data->input||!root.XamlRoot())return;
        auto size=root.XamlRoot().Size();if(size.Width<=0||size.Height<=0)return;
        auto facts=O({{L"held",B(held||dragging)},{L"dragging",B(dragging)},
            {L"popup_open",B(data->externalPopup||data->popupCount>0)}});
        data->input(to_string(O({{L"type",S(L"chrome")},{L"viewport",point({size.Width,size.Height})},
            {L"event",O({{L"kind",S(L"contact")},{L"position",point(at)},{L"canvas",B(false)}})},{L"facts",facts}}).Stringify()));
    }
    FrameworkElement target(Windows::Foundation::IInspectable const& original)const{
        for(auto node=original.try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node)){
            if(auto element=node.try_as<FrameworkElement>())
                if(auto tag=element.Tag().try_as<J>();tag&&tag.HasKey(L"header_source"))return element;
            if(node==root)break;
        }
        return nullptr;
    }
    bool current()const{
        auto element=originElement.get();
        return root.IsLoaded()&&root.IsHitTestVisible()&&!data->externalPopup&&
            (dragging||(element&&element.IsLoaded()));
    }
    void pump(){
        if(busy||(!starting&&!started))return;
        bool begin=starting,finish=!begin&&ending;
        if(!begin&&!finish&&!dirty)return;
        auto request=begin?beginRequest:O({{L"op",S(finish?L"finish":L"preview")},{L"position",point(position)}});
        if(finish)request.Insert(L"cancel",B(cancelled));
        auto serial=generation,at=motion;busy=true;
        bool accepted=QueryWorkspace(data->query,O({{L"type",S(L"header")},{L"request",request}}),
            [weak=weak_from_this(),serial,at,begin,finish](J reply){
                auto self=weak.lock();if(!self||self->generation!=serial)return;
                self->busy=false;
                auto result=reply.GetNamedValue(L"result",JsonValue::CreateNullValue());
                if(begin){
                    self->starting=false;
                    self->started=result.ValueType()==JsonValueType::Boolean&&result.GetBoolean();
                    if(!self->started){self->clear();return;}
                }else if(finish){
                    bool commit=!self->cancelled&&self->editing&&self->focused()&&self->current();
                    auto action=result.ValueType()==JsonValueType::Object?result.GetObject():J{};
                    self->clear();
                    if(commit&&action.Size())self->data->dispatch(action);
                    return;
                }else if(!self->ending&&at==self->motion){
                    self->preview=result.ValueType()==JsonValueType::Object?result.GetObject():J{};
                    self->notify();
                }
                self->pump();
            });
        if(!accepted)busy=false;
        else if(!begin&&!finish)dirty=false;
    }
    void context(uint32_t id,Point at,bool holding=false){
        owner=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);
        auto serial=++menuGeneration;
        QueryWorkspace(data->query,O({{L"type",S(L"context")},{L"target",O({{L"kind",S(L"header")},
            {L"id",id?N(id):JsonValue::CreateNullValue()}})}}),
            [weak=weak_from_this(),serial,at,holding](J reply){
                auto self=weak.lock();if(!self||serial!=self->menuGeneration||self->dragging||!self->focused()||self->data->externalPopup)return;
                auto model=object(reply,L"result");if(!array(model,L"sections").Size())return;
                self->menu=MenuFlyout();TrackPopup(self->menu,self->data);
                self->menu.Opened([weak](auto&&,auto&&){if(auto self=weak.lock()){self->menuOpen=true;self->notify();}});
                self->menu.Closed([weak](auto&&,auto&&){if(auto self=weak.lock()){self->menuOpen=false;self->notify();}});
                NativeMenuItems(self->menu.Items(),array(model,L"sections"),self->data,[weak](J action){
                    if(auto self=weak.lock()){self->cancel();self->data->dispatch(action);}
                });
                Primitives::FlyoutShowOptions options;options.Position(at);
                options.ShowMode(holding?Primitives::FlyoutShowMode::Transient:Primitives::FlyoutShowMode::Standard);
                self->menu.ShowAt(self->root,options);
            });
    }
    void down(PointerRoutedEventArgs const& e){
        if(active()){if(pointer!=e.Pointer().PointerId())cancel();return;}
        if(data->externalPopup)return;
        auto p=e.GetCurrentPoint(root);if(!p.IsInContact())return;
        auto element=target(e.OriginalSource());if(!element)return;
        auto tag=element.Tag().as<J>();auto item=object(tag,L"header_source");
        if(p.Properties().IsRightButtonPressed()||p.Properties().IsBarrelButtonPressed())return;
        if(p.PointerDeviceType()==NativeInput::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed())return;
        chrome(p.Position());auto kind=str(item,L"kind");
        if(kind==L"background"){if(editing){selected=0;notify();e.Handled(true);}return;}
        bool isItem=kind==L"item";
        if(!editing&&(!isItem||p.PointerDeviceType()==NativeInput::PointerDeviceType::Mouse))return;
        if(editing)selected=isItem?uint32_t(num(item,L"value")):0;
        ++generation;cancelled=false;hideMenu();source=item;originElement=make_weak(element);
        for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node)){
            if(auto target=node.try_as<UIElement>())pressedPath.push_back(make_weak(target));
            if(node==root)break;
        }
        origin=position=p.Position();pointer=p.PointerId();contact=e.Pointer();device=p.PointerDeviceType();
        owner=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);auto dpi=GetDpiForWindow(owner);
        slopX=std::max(2.,double(GetSystemMetricsForDpi(SM_CXDRAG,dpi))*96./std::max(96u,dpi));
        slopY=std::max(2.,double(GetSystemMetricsForDpi(SM_CYDRAG,dpi))*96./std::max(96u,dpi));
        beginRequest=J::Parse(configuration.Stringify());beginRequest.Insert(L"op",S(L"begin"));
        beginRequest.Insert(L"source",source);beginRequest.Insert(L"press",point(origin));
        beginRequest.Insert(L"grab",rectangle(visibleBounds(element,root)));
        if(isItem&&device!=NativeInput::PointerDeviceType::Mouse){
            recognizing=true;recognizer.GestureSettings(NativeInput::GestureSettings::Hold);recognizer.ProcessDownEvent(p);
        }
        if(editing){claim();e.Handled(true);}
        timer.Start();notify();
    }
    void move(PointerRoutedEventArgs const& e){
        if(!pointer||*pointer!=e.Pointer().PointerId()||ending)return;
        if(!focused()||!current()){cancel();return;}
        auto next=e.GetCurrentPoint(root).Position();
        if(recognizing&&!held)recognizer.ProcessMoveEvents(e.GetIntermediatePoints(root));
        bool moved=std::abs(next.X-origin.X)>slopX||std::abs(next.Y-origin.Y)>slopY;
        if(!dragging&&moved){
            if(!editing){clear();return;}
            if(!claim())return;
            hideMenu();stopRecognition();dragging=true;starting=true;
        }
        if(dragging){position=next;++motion;dirty=true;pump();e.Handled(true);notify();}
    }
    void up(PointerRoutedEventArgs const& e){
        if(!pointer||*pointer!=e.Pointer().PointerId())return;
        if(!focused()||!current()){cancel();e.Handled(true);return;}
        auto p=e.GetCurrentPoint(root);
        if(recognizing){recognizing=false;recognizer.ProcessUpEvent(p);}
        if(dragging){
            position=p.Position();++motion;ending=true;release();pump();e.Handled(true);notify();
        }else{
            bool handled=editing||held;clear(held);if(handled)e.Handled(true);
        }
    }
    void init(){
        timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(16));
        auto weak=weak_from_this();
        timer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock()){
            if(self->active()&&(!self->focused()||!self->current()))self->cancel();
            self->pump();
        }});
        recognizer.Holding([weak](auto&&,NativeInput::HoldingEventArgs const& e){if(auto self=weak.lock()){
            if(e.HoldingState()!=NativeInput::HoldingState::Started||!self->pointer||self->dragging||self->held)return;
            if(!self->focused()||!self->current()||!self->claim())return;
            self->held=true;self->context(uint32_t(num(self->source,L"value")),self->position,true);self->notify();
        }});
        root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->down(e);})),true);
        root.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->move(e);})),true);
        root.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->up(e);})),true);
        root.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&!self->releasing&&self->pointer==e.Pointer().PointerId())self->cancel();
        })),true);
        root.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&!self->releasing&&self->pointer==e.Pointer().PointerId()){
                if(e.OriginalSource().try_as<UIElement>()==self->root||e.GetCurrentPoint(self->root).IsInContact())self->cancel();
                else self->up(e);
            }
        })),true);
        root.ContextRequested([weak](auto&&,ContextRequestedEventArgs const& e){if(auto self=weak.lock()){
            auto element=self->target(e.OriginalSource());if(!element)return;
            e.Handled(true);if(self->active())return;
            auto item=object(element.Tag().as<J>(),L"header_source");auto kind=str(item,L"kind");
            if(kind!=L"item"&&kind!=L"background")return;
            Point at{};if(!e.TryGetPosition(self->root,at)){auto box=visibleBounds(element,self->root);at={box.X,box.Y+box.Height};}
            self->selected=uint32_t(num(item,L"value"));self->context(self->selected,at);self->notify();
        }});
        root.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->deferCancel();});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->deferCancel();});
    }
};
HeaderInput::HeaderInput(std::shared_ptr<WorkspaceData> data,Canvas root,std::function<void()> changed):impl(std::make_shared<Impl>()){
    impl->data=std::move(data);impl->root=root;impl->changed=std::move(changed);impl->init();
}
HeaderInput::~HeaderInput()=default;
void HeaderInput::Source(FrameworkElement const& element,J const& source,hstring const& label){
    auto prior=element.Tag().try_as<J>();
    if(!prior||!prior.HasKey(L"header_source")){
        element.Loaded([weak=make_weak(element)](auto&&,auto&&){if(auto element=weak.get())ownHolding(element);});
        if(element.IsLoaded())ownHolding(element);
    }
    element.Tag(O({{L"header_source",source}}));element.IsHoldingEnabled(false);element.CanDrag(false);
    AutomationProperties::SetName(element,label);
}
void HeaderInput::Configure(J const& model,bool editing,J const& request){
    auto identity=model.Stringify()+request.Stringify();
    if(impl->identity!=identity||impl->editing!=editing)impl->cancel();
    impl->model=model;impl->configuration=request;impl->identity=identity;impl->editing=editing;
    bool found=false;for(auto zone:array(model,L"zones"))for(auto entry:zone.GetArray())found|=num(entry.GetObject(),L"id")==impl->selected;
    if(!editing||!found)impl->selected=0;
}
bool HeaderInput::Key(KeyRoutedEventArgs const& e,bool pressed){
    if(!impl->editing||impl->data->externalPopup)return false;
    using K=Windows::System::VirtualKey;auto key=e.Key();
    if((GetKeyState(VK_CONTROL)&0x8000)||(GetKeyState(VK_MENU)&0x8000)||(GetKeyState(VK_LWIN)&0x8000)||(GetKeyState(VK_RWIN)&0x8000))return false;
    bool context=key==K::Application||(key==K::F10&&(GetKeyState(VK_SHIFT)&0x8000));
    if(key!=K::Escape&&key!=K::Left&&key!=K::Right&&key!=K::Delete&&key!=K::Back&&!context)return false;
    if(key!=K::Escape&&!impl->selected)return false;
    e.Handled(true);if(!pressed)return true;
    if(key==K::Escape){if(!impl->cancel())impl->data->dispatch(edit(O({{L"type",S(L"cancel")}})));return true;}
    if(impl->active())return true;
    if(context){impl->context(impl->selected,{6,48});return true;}
    if(key==K::Delete||key==K::Back)impl->data->dispatch(edit(O({{L"type",S(L"remove")},{L"id",N(impl->selected)}})));
    else {
        auto serial=impl->identity;auto id=impl->selected;
        QueryWorkspace(impl->data->query,O({{L"type",S(L"header")},{L"request",O({{L"op",S(L"step")},{L"id",N(id)},{L"forward",B(key==K::Right)}})}}),
            [weak=std::weak_ptr<Impl>(impl),serial](J reply){if(auto self=weak.lock();self&&self->editing&&self->identity==serial){
                auto action=object(reply,L"result");if(action.Size())self->data->dispatch(action);
            }});
    }
    return true;
}
bool HeaderInput::Cancel(){return impl->cancel();}
void HeaderInput::Select(uint32_t id){impl->selected=id;impl->notify();}
uint32_t HeaderInput::Selected()const{return impl->selected;}
J HeaderInput::Preview()const{return impl->preview;}
J HeaderInput::Source()const{return impl->source;}
bool HeaderInput::Busy()const{return impl->active();}
