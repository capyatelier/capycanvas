#include "pch.h"
#include "HeaderInput.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceQuery.h"
#include "NativeMenus.h"
#include "NativePresentation.h"
#include <chrono>

using namespace CapyUi;
using Windows::Foundation::Point;
namespace NativeInput=Microsoft::UI::Input;
namespace {
A point(Point p){A result;result.Append(N(p.X));result.Append(N(p.Y));return result;}
J edit(J const& action){return O({{L"type",S(L"customize")},{L"action",O({{L"type",S(L"header")},{L"action",action}})}});}
}
struct HeaderInput::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr};
    std::function<void()> changed;
    J model,configuration,source,beginRequest,preview,lastCancel;
    hstring identity;
    weak_ref<FrameworkElement> originElement,originFocus;
    std::vector<weak_ref<UIElement>> pressedPath;
    std::vector<std::pair<weak_ref<UIElement>,ManipulationModes>> scrollModes;
    std::optional<uint32_t> pointer;
    Input::Pointer contact{nullptr};
    PointerGesture recognizer;
    NativePresentation presentation;
    NativeInput::PointerDeviceType device=NativeInput::PointerDeviceType::Mouse;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    MenuFlyout menu{nullptr};
    OwnedPopup menuOwner;
    Point origin{},position{};
    HWND owner=nullptr;
    uint32_t selected=0;
    uint64_t generation=0;
    bool editing=false,dragging=false,starting=false,started=false,busy=false,ending=false,cancelled=false,committing=false;
    bool dirty=false,held=false,recognizing=false,releasing=false,workspaceMenu=false,keepMenu=false,normalContact=false,released=false;
    bool trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    ~Impl(){if(timer)timer.Stop();if(menu)menu.Hide();}
    bool focused()const{return owner&&GetAncestor(GetForegroundWindow(),GA_ROOTOWNER)==owner&&!IsIconic(owner);}
    bool captured(UIElement const& element)const{
        return pointer&&PointerGesture::Captured(element,*pointer);
    }
    bool active()const{return pointer||starting||started||busy||ending;}
    bool editable()const{return editing&&flag(object(data->model,L"header"),L"editing");}
    void evidence(){
        if(!trace)return;
        AutomationProperties::SetHelpText(root,O({
            {L"phase",S(ending?L"finishing":dragging?L"dragging":held?L"held":pointer?L"pressed":L"idle")},
            {L"generation",N(double(generation))},{L"source",source},{L"requires_hold",B(false)},
            {L"device",S(device==NativeInput::PointerDeviceType::Mouse?L"mouse":device==NativeInput::PointerDeviceType::Pen?L"pen":L"touch")},
            {L"captured",B(captured(root))},{L"menu_open",B(menuOwner.Open())},{L"menu_pending",B(menuOwner.Pending())},{L"preview",preview},{L"last_cancel",lastCancel}}).Stringify());
    }
    void notify(){evidence();if(changed)changed();}
    void hideMenu(){menuOwner.Cancel();}
    void stopRecognition(){if(recognizing){recognizing=false;recognizer.CompleteGesture();}}
    void release(){
        releasing=true;if(root.IsLoaded()&&captured(root))root.ReleasePointerCaptures();
        for(auto const& [weak,mode]:scrollModes)if(auto element=weak.get())element.ManipulationMode(mode);
        scrollModes.clear();releasing=false;
        pointer.reset();contact=nullptr;pressedPath.clear();stopRecognition();
    }
    void clear(){
        presentation.Cancel();release();++generation;starting=started=busy=ending=cancelled=dragging=dirty=held=normalContact=released=committing=false;
        dragCursor(root,false,false);
        source=J{};beginRequest=J{};preview=J{};timer.Stop();chrome(position,false);notify();
    }
    bool cancel(hstring reason=L"cancel"){
        if(!active()&&!menuOwner.Active())return false;
        if(trace)lastCancel=O({{L"reason",S(reason)},{L"source",source},{L"captured",B(captured(root))},{L"dragging",B(dragging)}});
        if(committing){hideMenu();clear();return true;}
        cancelled=true;hideMenu();release();preview=J{};
        if(busy||started){ending=true;cancelled=true;pump();notify();}
        else clear();
        return true;
    }
    void deferCancel(){
        if(committing||(!active()&&!menuOwner.Active()))return;
        // Capture changes are forbidden while XAML is arranging/unloading.
        // Reject a release immediately, then retire capture on the dispatcher.
        cancelled=true;auto serial=generation;
        root.DispatcherQueue().TryEnqueue([weak=weak_from_this(),serial]{
            if(auto self=weak.lock();self&&self->generation==serial)self->cancel(L"layout_changed");
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
        if(!accepted)cancel(L"capture_failed");
        return accepted;
    }
    void chrome(Point at,bool pressed=true){
        if(!data->input||!root.XamlRoot())return;
        auto size=root.XamlRoot().Size();if(size.Width<=0||size.Height<=0)return;
        auto facts=O({{L"held",B((pointer&&normalContact)||held||dragging)},{L"dragging",B(dragging)},
            {L"popup_open",B(data->externalPopup||data->popupCount>0)}});
        data->input(to_string(O({{L"type",S(L"chrome")},{L"viewport",point({size.Width,size.Height})},
            {L"event",pressed?O({{L"kind",S(L"contact")},{L"position",point(at)},{L"canvas",B(false)}}):O({{L"kind",S(L"refresh")}})},{L"facts",facts}}).Stringify()));
    }
    FrameworkElement target(Windows::Foundation::IInspectable const& original)const{
        for(auto node=original.try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node)){
            if(auto element=node.try_as<FrameworkElement>())
                if(auto tag=element.Tag().try_as<J>();tag&&tag.HasKey(L"header_source"))return element;
            if(node==root)break;
        }
        return nullptr;
    }
    bool available(FrameworkElement const& element)const{
        if(!element)return false;auto bounds=visibleBounds(element,root);auto control=element.try_as<Control>();
        return bounds.Width>0&&bounds.Height>0&&(!control||control.IsEnabled());
    }
    bool current()const{
        return root.IsLoaded()&&root.IsHitTestVisible()&&!data->externalPopup&&
            (dragging||available(originElement.get()));
    }
    void pump(){
        if(busy||(!starting&&!started))return;
        bool begin=starting,finish=!begin&&ending;
        if(!begin&&!finish&&!dirty)return;
        auto request=begin?beginRequest:O({{L"op",S(finish?L"finish":L"preview")},{L"position",point(position)}});
        if(finish)request.Insert(L"cancel",B(cancelled));
        auto serial=generation;busy=true;
        bool accepted=QueryWorkspace(data->query,O({{L"type",S(L"header")},{L"request",request}}),
            [weak=weak_from_this(),serial,begin,finish](J reply){
                auto self=weak.lock();if(!self||self->generation!=serial)return;
                self->busy=false;
                auto result=reply.GetNamedValue(L"result",JsonValue::CreateNullValue());
                if(begin){
                    self->starting=false;
                    self->started=result.ValueType()==JsonValueType::Boolean&&result.GetBoolean();
                    if(!self->started){self->clear();return;}
                }else if(finish){
                    bool commit=!self->cancelled&&self->editable()&&self->focused()&&self->current();
                    auto action=result.ValueType()==JsonValueType::Object?result.GetObject():J{};
                    if(commit&&action.Size()){
                        self->committing=true;self->timer.Stop();
                        self->data->dispatch(action,[weak,serial]{if(auto self=weak.lock();self&&self->generation==serial)
                            self->presentation.AfterLayout([weak,serial]{if(auto self=weak.lock();self&&self->generation==serial)self->clear();});});
                    }else self->clear();
                    return;
                }else if(!self->ending){
                    self->preview=result.ValueType()==JsonValueType::Object?result.GetObject():J{};
                    self->notify();
                }
                self->pump();
            });
        if(!accepted)busy=false;
        else if(!begin&&!finish)dirty=false;
    }
    bool workspace(uint32_t id)const{
        for(auto zone:array(model,L"zones"))for(auto value:zone.GetArray()){
            auto entry=value.GetObject();
            if(num(entry,L"id")==id)return str(object(entry,L"item"),L"kind")==L"workspaces";
        }
        return false;
    }
    FrameworkElement focusSource(Windows::Foundation::IInspectable const& original)const{
        for(auto node=original.try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node)){
            if(auto control=node.try_as<Control>();control&&control.IsTabStop())return control;
            if(node==root)break;
        }
        return target(original);
    }
    void refreshWorkspaceMenu(){
        if(!menu||!workspaceMenu)return;
        auto presentation=object(object(data->model,L"windows_workspace"),L"switcher_options");
        NativeMenuItems(menu.Items(),array(presentation,L"sections"),data,[weak=weak_from_this()](J action){
            if(auto self=weak.lock()){if(!self->keepMenu)self->cancel();self->data->dispatch(action);}
        },[weak=weak_from_this()](bool keep){if(auto self=weak.lock())self->keepMenu=keep;});
    }
    OwnedPopup::Ticket beginMenu(FrameworkElement const& invoking,bool workspaceOptions=false){
        hideMenu();menu=MenuFlyout();workspaceMenu=workspaceOptions;auto ticket=menuOwner.Begin(data,invoking,menu);
        menu.Closing([weak=weak_from_this()](auto&&,auto const& args){if(auto self=weak.lock();self&&std::exchange(self->keepMenu,false))args.Cancel(true);});
        auto weak=weak_from_this();auto control=invoking.try_as<Control>();
        if(!control)control=FocusManager::GetFocusedElement(root.XamlRoot()).try_as<Control>();
        auto focus=make_weak(control);
        menu.Opened([weak](auto const& sender,auto&&){if(auto self=weak.lock();self&&self->menu==sender.template as<MenuFlyout>())self->notify();});
        menu.Closed([weak,focus](auto const& sender,auto&&){if(auto self=weak.lock();self&&self->menu==sender.template as<MenuFlyout>()){
            self->workspaceMenu=false;self->notify();
            if(auto element=focus.get();element&&element.IsLoaded())element.Focus(FocusState::Keyboard);
        }});
        return ticket;
    }
    void showMenu(J const& presentation,Point at,bool holding,OwnedPopup::Ticket const& ticket){
        if(!ticket.Current())return;
        if(!array(presentation,L"sections").Size()){ticket.Cancel();return;}
        NativeMenuItems(menu.Items(),array(presentation,L"sections"),data,[weak=weak_from_this()](J action){
            if(auto self=weak.lock()){if(!self->keepMenu)self->cancel();self->data->dispatch(action);}
        },[weak=weak_from_this()](bool keep){if(auto self=weak.lock())self->keepMenu=keep;});
        Primitives::FlyoutShowOptions options;options.Position(at);
        options.ShowMode(holding?Primitives::FlyoutShowMode::Transient:Primitives::FlyoutShowMode::Standard);
        ticket.Show(root,options);
    }
    void context(uint32_t id,Point at,bool holding=false,FrameworkElement invoking=nullptr){
        owner=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);
        if(!invoking)invoking=originElement.get();if(!invoking)invoking=root;
        auto ticket=beginMenu(invoking,workspace(id));
        auto tag=invoking.Tag().try_as<J>();bool capy=tag&&str(object(tag,L"header_source"),L"kind")==L"capy";
        if(workspace(id)){
            showMenu(object(object(data->model,L"windows_workspace"),L"switcher_options"),at,holding,ticket);return;
        }
        evidence();
        if(!QueryWorkspace(data->query,O({{L"type",S(L"context")},{L"target",capy?O({{L"kind",S(L"zen_mode")}}):O({{L"kind",S(L"header")},
            {L"id",id?N(id):JsonValue::CreateNullValue()}})}}),
            [weak=weak_from_this(),ticket,at,holding](J reply){
                auto self=weak.lock();if(!self||!ticket.Current())return;
                if(self->dragging){ticket.Cancel();return;}
                self->showMenu(object(reply,L"result"),at,holding,ticket);self->evidence();
            })){ticket.Cancel();evidence();}
    }
    void down(PointerRoutedEventArgs const& e){
        if(active()){if(released)clear();else {if(pointer==e.Pointer().PointerId())return;cancel(L"additional_contact");if(active())return;}}
        if(data->externalPopup)return;
        auto p=e.GetCurrentPoint(root);if(!PointerGesture::PrimaryContact(p))return;
        auto element=target(e.OriginalSource());if(!element)return;
        auto tag=element.Tag().as<J>();auto item=object(tag,L"header_source");
        auto kind=str(item,L"kind");
        bool isItem=kind==L"item";
        if(editable())selected=isItem?uint32_t(num(item,L"value")):0;
        ++generation;cancelled=false;hideMenu();source=item;originElement=make_weak(element);originFocus=make_weak(focusSource(e.OriginalSource()));
        for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node)){
            if(auto target=node.try_as<UIElement>())pressedPath.push_back(make_weak(target));
            if(node==root)break;
        }
        origin=position=p.Position();pointer=p.PointerId();contact=e.Pointer();device=p.PointerDeviceType();normalContact=!flag(data->model,L"chrome_hidden");
        chrome(position);
        if(kind==L"background"&&editable()){selected=0;notify();e.Handled(true);}
        owner=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);
        beginRequest=J::Parse(configuration.Stringify());beginRequest.Insert(L"op",S(L"begin"));
        beginRequest.Insert(L"source",source);beginRequest.Insert(L"press",point(origin));
        beginRequest.Insert(L"grab",rectangle(visibleBounds(element,root)));
        recognizing=true;recognizer.Begin(element,p,(isItem||kind==L"capy"||kind==L"background")&&device!=NativeInput::PointerDeviceType::Mouse?NativeInput::GestureSettings::Hold:NativeInput::GestureSettings::None);
        if(editable()){claim();e.Handled(true);}
        timer.Start();notify();
    }
    void move(PointerRoutedEventArgs const& e){
        if(!pointer||*pointer!=e.Pointer().PointerId()||ending)return;
        if(!focused()||!current()){cancel(L"source_or_focus_lost");return;}
        if(!editable()&&device==NativeInput::PointerDeviceType::Mouse)return;
        auto next=e.GetCurrentPoint(root).Position();
        if(recognizing)recognizer.Move(root,e);
        bool moved=recognizer.Moved();
        if(!dragging&&moved){
            if(!editable()){stopRecognition();return;}
            if(!claim())return;
            hideMenu();stopRecognition();dragging=true;starting=true;dragCursor(root,device==NativeInput::PointerDeviceType::Touch,true);
        }
        if(dragging){position=next;dirty=true;pump();e.Handled(true);notify();}
    }
    void up(PointerRoutedEventArgs const& e){
        if(!pointer||*pointer!=e.Pointer().PointerId()||released)return;
        auto p=e.GetCurrentPoint(root);
        if(!focused()||!current()||p.Properties().IsCanceled()){cancel(L"release_canceled");e.Handled(true);return;}
        if(device==NativeInput::PointerDeviceType::Mouse&&p.Properties().IsLeftButtonPressed())return;
        if(recognizing){recognizing=false;recognizer.ProcessUpEvent(p);}
        if(dragging){
            position=p.Position();ending=true;release();pump();e.Handled(true);notify();
        }else{
            if(editable()||held){clear();e.Handled(true);}
            else {released=true;auto serial=generation;root.DispatcherQueue().TryEnqueue([weak=weak_from_this(),serial]{if(auto self=weak.lock();self&&self->generation==serial)self->clear();});}
        }
    }
    void init(){
        timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(16));
        auto weak=weak_from_this();
        timer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock()){
            if(self->active()&&(!self->focused()||!self->current()))self->cancel(L"source_or_focus_lost");
            self->pump();
        }});
        recognizer.Holding([weak](auto&&,NativeInput::HoldingEventArgs const& e){if(auto self=weak.lock()){
            if(e.HoldingState()!=NativeInput::HoldingState::Started||!self->pointer||self->dragging||self->held)return;
            if(!self->focused()||!self->current()||!self->claim())return;
            self->held=true;self->context(uint32_t(num(self->source,L"value")),self->position,true,self->originFocus.get());self->notify();
        }});
        root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->down(e);})),true);
        root.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->move(e);})),true);
        PointerGesture::OnLeaving(root,[weak](PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()&&!self->captured(self->root)){
            for(auto const& entry:self->pressedPath)if(auto element=entry.get();element&&self->captured(element))return;
            self->clear();
        }});
        root.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->up(e);})),true);
        root.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&!self->releasing&&!self->released&&self->pointer==e.Pointer().PointerId())self->cancel(L"pointer_canceled");
        })),true);
        root.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&!self->releasing&&!self->released&&self->pointer==e.Pointer().PointerId()){
                if(e.OriginalSource().try_as<UIElement>()==self->root||e.GetCurrentPoint(self->root).IsInContact())self->cancel(L"capture_lost");
                else self->up(e);
            }
        })),true);
        root.ContextRequested([weak](auto&&,ContextRequestedEventArgs const& e){if(auto self=weak.lock()){
            auto element=self->target(e.OriginalSource());if(!element)return;
            e.Handled(true);if(self->active())return;
            auto item=object(element.Tag().as<J>(),L"header_source");auto kind=str(item,L"kind");
            if(kind!=L"item"&&kind!=L"background"&&kind!=L"capy")return;
            Point at{};if(!e.TryGetPosition(self->root,at)){auto box=visibleBounds(element,self->root);at={box.X,box.Y+box.Height};}
            self->selected=uint32_t(num(item,L"value"));self->context(self->selected,at,false,self->focusSource(e.OriginalSource()));self->notify();
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
        PointerGesture::OwnHolding(element);
    }
    element.Tag(O({{L"header_source",source}}));
    AutomationProperties::SetName(element,label);
}
void HeaderInput::WorkspaceOptions(FrameworkElement const& invoking){
    impl->owner=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);impl->hideMenu();
    auto box=visibleBounds(invoking,impl->root);
    impl->showMenu(object(object(impl->data->model,L"windows_workspace"),L"switcher_options"),{box.X,box.Y+box.Height},false,impl->beginMenu(invoking,true));
}
void HeaderInput::Context(uint32_t id,FrameworkElement const& invoking){
    if(impl->active())return;
    impl->hideMenu();auto box=visibleBounds(invoking,impl->root);
    impl->selected=id;impl->context(id,{box.X,box.Y+box.Height},false,invoking);impl->notify();
}
void HeaderInput::Configure(J const& model,bool editing,J const& request){
    auto identity=model.Stringify()+request.Stringify();
    if(!impl->committing&&(impl->identity!=identity||impl->editing!=editing)&&str(impl->source,L"kind")!=L"capy"
        &&(impl->editing!=editing||!impl->workspaceMenu||impl->active()))impl->cancel(L"configuration_changed");
    impl->model=model;impl->configuration=request;impl->identity=identity;impl->editing=editing;
    impl->refreshWorkspaceMenu();
    bool found=false;for(auto zone:array(model,L"zones"))for(auto entry:zone.GetArray())found|=num(entry.GetObject(),L"id")==impl->selected;
    if(!editing||!found)impl->selected=0;
}
bool HeaderInput::Key(KeyRoutedEventArgs const& e,bool pressed){
    if(impl->data->externalPopup)return false;
    using K=Windows::System::VirtualKey;auto key=e.Key();
    if((GetKeyState(VK_CONTROL)&0x8000)||(GetKeyState(VK_MENU)&0x8000)||(GetKeyState(VK_LWIN)&0x8000)||(GetKeyState(VK_RWIN)&0x8000))return false;
    bool context=key==K::Application||(key==K::F10&&(GetKeyState(VK_SHIFT)&0x8000));
    if(context){
        auto element=impl->target(FocusManager::GetFocusedElement(impl->root.XamlRoot()));
        auto id=element?uint32_t(num(object(element.Tag().as<J>(),L"header_source"),L"value")):impl->selected;
        if(impl->workspace(id)||(element&&str(object(element.Tag().as<J>(),L"header_source"),L"kind")==L"capy")){
            e.Handled(true);if(pressed&&!impl->active()){
                auto box=visibleBounds(element?element:impl->root,impl->root);
                impl->context(id,{box.X,box.Y+box.Height},false,impl->focusSource(FocusManager::GetFocusedElement(impl->root.XamlRoot())));
            }return true;
        }
    }
    if(key==K::Escape&&impl->menuOwner.Active()){e.Handled(true);if(pressed)impl->cancel();return true;}
    if(!impl->editable())return false;
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
            [weak=std::weak_ptr<Impl>(impl),serial](J reply){if(auto self=weak.lock();self&&self->editable()&&self->identity==serial){
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
