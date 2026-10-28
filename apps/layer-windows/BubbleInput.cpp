#include "pch.h"
#include "BubblePresentation.h"

using namespace CapyBubbles;
namespace NativeInput=Microsoft::UI::Input;
namespace {
bool contains(Rect r,Point p){return p.X>=r.X&&p.Y>=r.Y&&p.X<r.X+r.Width&&p.Y<r.Y+r.Height;}
J grab(uint32_t id,hstring type=L"bubble"){return O({{L"kind",S(type)},{L"id",N(id)}});}
void stop(std::shared_ptr<Contact> const& c){c->holdAt=-1;if(std::exchange(c->recognizing,false))c->recognizer.CompleteGesture();}
}
void WorkspaceBubbles::Impl::send(J const& action){data->dispatch(O({{L"type",S(L"bubble")},{L"action",action}}));wake();}
void WorkspaceBubbles::Impl::drag(J const& source,hstring const& phase,Point at){
    send(O({{L"type",S(L"drag")},{L"grab",source},{L"phase",S(phase)},{L"position",point(at)},
        {L"viewport",point({float(workspace.ActualWidth()),float(workspace.ActualHeight())})}}));
}
V WorkspaceBubbles::Impl::hit(Record const& record,Point at)const{
    return ui(data,O({{L"type",S(L"hit")},{L"view",record.view},{L"point",point(relative(at,record.center))}}));
}
std::shared_ptr<Record> WorkspaceBubbles::Impl::target(Point at)const{
    std::shared_ptr<Record> found;
    for(auto const& [id,r]:records)if(!r->faded&&distance(at,r->center)<=r->reach*1.415&&hit(*r,at).ValueType()!=JsonValueType::Null)
        if(!found||std::pair(r->lifted,r->order)>std::pair(found->lifted,found->order))found=r;
    return found;
}
J WorkspaceBubbles::Impl::source(J const& grab)const{
    if(str(grab,L"kind")==L"header")return findId(array(object(data->model,L"header"),L"items"),num(grab,L"id"));
    if(str(grab,L"kind")==L"tile")return findId(array(find(array(data->model,L"panels"),L"id",str(grab,L"panel")),L"tiles"),num(grab,L"tile"));
    return {};
}
bool WorkspaceBubbles::Impl::current(Candidate const& c)const{
    if(c.epoch!=epoch)return false;if(c.moving)return true;
    auto element=c.source.get();if(!element||!element.IsLoaded())return false;
    if(str(c.grab,L"kind")==L"variant"){
        auto tag=element.Tag().try_as<J>();return tag&&object(tag,L"bubble_choice").Stringify()==c.identity;
    }
    return object(source(c.grab),L"resolved_control").Stringify()==c.identity;
}
bool WorkspaceBubbles::Impl::capture(Input::Pointer const& pointer,std::vector<weak_ref<UIElement>> const& path){
    releasing=true;
    data->pointerCancels.Cancel(pointer.PointerId());
    for(auto weak:path)if(auto element=weak.get()){
        if(PointerGesture::Captured(element,pointer.PointerId()))element.ReleasePointerCapture(pointer);
    }
    bool captured=inputRoot.CapturePointer(pointer);releasing=false;return captured;
}
void WorkspaceBubbles::Impl::contactSurface(){
    bool touch=contact&&contact->device==NativeInput::PointerDeviceType::Touch;
    contactReach.Visibility(touch?Visibility::Visible:Visibility::Collapsed);
    if(touch){
        auto radius=float(num(contact->record->packet,L"reach")+90);auto center=contact->record->center;
        contactReach.Width(radius*2);contactReach.Height(radius*2);Canvas::SetLeft(contactReach,center.X-radius);Canvas::SetTop(contactReach,center.Y-radius);
    }
    bool hide=pointerPen||contact&&object(contact->record->view,L"stamp").Size();
    if(pointerHidden!=hide){pointerHidden=hide;layer.as<IUIElementProtected>().ProtectedCursor(hide?nullptr:NativeInput::InputSystemCursor::Create(NativeInput::InputSystemCursorShape::Arrow));}
}
void WorkspaceBubbles::Impl::pick(std::shared_ptr<Contact> const& c,Point at){
    if(!c->wheel)return;
    auto action=O({{L"type",S(c->wheel==3?L"intensity":L"wheel")},{L"id",N(c->record->id)},{L"phase",S(c->colorStarted?L"move":L"down")},{L"position",point(relative(at,c->record->center))}});
    if(c->wheel!=3)action.Insert(L"part",S(c->wheel==1?L"hue":L"field"));send(action);
    c->colorStarted=true;
}
void WorkspaceBubbles::Impl::finishColor(std::shared_ptr<Contact> const& c,bool cancelled){
    if(!std::exchange(c->colorStarted,false))return;
    auto action=O({{L"type",S(c->wheel==3?L"intensity":L"wheel")},{L"id",N(c->record->id)},{L"phase",S(cancelled?L"cancel":L"up")},{L"position",point(relative(c->last,c->record->center))}});
    if(c->wheel!=3)action.Insert(L"part",S(c->wheel==1?L"hue":L"field"));send(action);
}
void WorkspaceBubbles::Impl::restore(std::shared_ptr<Contact> const& c){
    finishColor(c,true);
    if(c->mode==L"ring"||c->mode==L"beads"||c->mode==L"choose")
        send(O({{L"type",S(c->mode)},{L"id",N(c->record->id)},{L"phase",S(L"cancel")},{L"position",point(Point{})}}));
    if(c->mode==L"moving")drag(c->grab,L"cancel",c->last);
    c->mode=L"pending";
}
void WorkspaceBubbles::Impl::begin(std::shared_ptr<Contact> const& c,Point at){
    auto hitKind=kind(c->hit);auto local=relative(at,c->record->center);
    if(hitKind==L"wheel"){
        c->mode=L"wheel";auto size=num(c->record->view,L"diameter")-14;
        auto panel=object(data->colorPreview,L"view");if(!panel.Size())panel=object(data->model,L"color_panel");
        auto shape=str(panel,L"shape");c->wheel=capy_color_hit(float(local.X+size/2),float(local.Y+size/2),float(size),shape==L"circle"?2:shape==L"triangle"?1:0);pick(c,at);
    }else if(hitKind==L"ring"||hitKind==L"intensity"){
        auto scale=workspace.XamlRoot()?workspace.XamlRoot().RasterizationScale():1.;
        auto slop=std::max(8.,double(std::max(GetSystemMetrics(SM_CXDOUBLECLK),GetSystemMetrics(SM_CYDOUBLECLK)))/scale);
        bool repeated=lastRing&&std::get<0>(*lastRing)==c->record->id&&milliseconds()-std::get<1>(*lastRing)<=GetDoubleClickTime()&&distance(at,std::get<2>(*lastRing))<=slop;
        if(repeated)lastRing.reset();else lastRing=std::tuple(c->record->id,milliseconds(),at);
        if(hitKind==L"intensity"){
            c->wheel=3;c->mode=repeated?L"reset":L"intensity";
            if(repeated)send(O({{L"type",S(L"set_intensity")},{L"id",N(c->record->id)},{L"stops",N(0)}}));else pick(c,at);
            auto face=c->record->parts.find(L"face");if(face!=c->record->parts.end()&&face->second->intensity)face->second->intensity->value.Focus(FocusState::Pointer);return;
        }
        c->mode=repeated?L"reset":L"ring";
        auto action=O({{L"type",S(repeated?L"reset_ring":L"ring")},{L"id",N(c->record->id)},{L"position",point(local)}});
        if(!repeated)action.Insert(L"phase",S(L"down"));send(action);
    }else if(hitKind==L"bead"||c->hit.ValueType()==JsonValueType::Object&&c->hit.GetObject().HasKey(L"item")){
        c->mode=hitKind==L"bead"?L"beads":L"choose";
        send(O({{L"type",S(c->mode)},{L"id",N(c->record->id)},{L"phase",S(L"down")},{L"position",point(local)}}));
    }
}
void WorkspaceBubbles::Impl::colorTouch(PointerRoutedEventArgs const& e,hstring const& phase,std::shared_ptr<Record> const& record){
    auto p=e.GetCurrentPoint(workspace);
    bool inside=phase==L"down"&&p.PointerDeviceType()==NativeInput::PointerDeviceType::Touch&&record&&str(record->view,L"kind")==L"color"
        &&hit(*record,p.Position()).ValueType()!=JsonValueType::Null;
    colorTouches.forward(*data,layer,p,phase,inside);
}
bool WorkspaceBubbles::Impl::join(PointerRoutedEventArgs const& e){
    auto c=contact;if(!c)return false;
    auto p=e.GetCurrentPoint(workspace);auto at=p.Position();
    if(p.Properties().IsCanceled()){cancel();e.Handled(true);return true;}
    if(c->fingers.contains(p.PointerId())){if(c->paired)e.Handled(true);return c->paired;}
    if(c->device!=NativeInput::PointerDeviceType::Touch||p.PointerDeviceType()!=c->device||distance(at,c->record->center)>num(c->record->packet,L"reach")+90)return false;
    std::vector<weak_ref<UIElement>> path;
    for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node)){
        if(auto element=node.try_as<UIElement>())path.push_back(make_weak(element));if(node==inputRoot)break;
    }
    if(!c->paired){stop(c);restore(c);dismiss();if(!capture(c->pointer,c->path)){cancel();e.Handled(true);return true;}}
    if(!capture(e.Pointer(),path)){cancel();e.Handled(true);return true;}
    c->fingers[p.PointerId()]=at;
    if(!c->paired){
        c->paired=true;c->mode=L"pair";c->grab=grab(c->record->id);
        auto a=c->fingers.begin()->second,b=std::next(c->fingers.begin())->second;c->midpoint={(a.X+b.X)/2,(a.Y+b.Y)/2};c->spread=std::max(1.,distance(a,b));
        c->pairOrigin=c->midpoint;
    }
    e.Handled(true);return true;
}
void WorkspaceBubbles::Impl::down(PointerRoutedEventArgs const& e){
    auto p=e.GetCurrentPoint(workspace);auto at=p.Position();if(p.Properties().IsCanceled()){cancel();return;}
    pointerPen=p.PointerDeviceType()==NativeInput::PointerDeviceType::Pen;
    if(contact){colorTouch(e,L"down",contact->record);join(e);return;}
    if(menuOwner.Active()||data->externalPopup||flag(object(data->model,L"header"),L"editing"))return;
    auto r=target(at);colorTouch(e,L"down",r);bool secondary=p.Properties().IsRightButtonPressed()||p.Properties().IsBarrelButtonPressed();
    if(r){
        if(secondary){showMenu(r,at);e.Handled(true);return;}
        if(!PointerGesture::PrimaryContact(p))return;
        auto h=hit(*r,at);if(kind(h)==L"pill")return;
        auto c=std::make_shared<Contact>();contact=c;c->record=r;c->hit=h;c->pointer=e.Pointer();c->device=p.PointerDeviceType();c->start=c->last=at;c->started=milliseconds();c->fingers[p.PointerId()]=at;
        bool nativeControl=false;
        for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node)){
            if(auto element=node.try_as<UIElement>())c->path.push_back(make_weak(element));
            if(auto element=node.try_as<FrameworkElement>()){
                auto id=AutomationProperties::GetAutomationId(element);bool button=bool(element.try_as<Primitives::ButtonBase>());
                bool layerList=prefix(id,L"layer-row-")||id==L"layer-add-paper";
                bool edits=element.try_as<Primitives::RangeBase>()||element.try_as<TextBox>()||element.try_as<ComboBox>()||element.try_as<ToggleSwitch>()||
                    element.try_as<Primitives::ScrollBar>()||prefix(id,L"number-root-")||layerList;
                if(auto focus=element.try_as<ContentControl>();focus&&!button)edits=edits||focus.IsTabStop();
                c->nativeDrags=c->nativeDrags||edits;nativeControl=nativeControl||edits||button;
            }if(node==inputRoot)break;
        }
        bool native=kind(h)==L"card";
        if(native)c->mode=L"native";
        else {if(!capture(c->pointer,c->path)){cancel();return;}begin(c,at);}
        if(!native||!e.Handled()&&!nativeControl){
            auto weak=weak_from_this();std::weak_ptr<Contact> owner=c;
            c->recognizer.Holding([weak,owner](auto&&,NativeInput::HoldingEventArgs const& event){if(auto self=weak.lock())if(auto c=owner.lock();c&&self->contact==c&&c->recognizing&&event.HoldingState()==NativeInput::HoldingState::Started){
                c->holdAt=c->started+(milliseconds()-c->started)*(c->wheel?2:1);self->holdTimer.Start();self->hold();
            }});
            c->recognizing=true;c->recognizer.Begin(nullptr,p,NativeInput::GestureSettings::Hold|NativeInput::GestureSettings::HoldWithMouse);
        }
        if(!native){r->hovered=h;r->pressed=true;e.Handled(true);}contactSurface();wake();return;
    }
    if(secondary||!PointerGesture::PrimaryContact(p)||data->popupCount)return;
    FrameworkElement element{nullptr};J sourceGrab,tag;
    for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node))if(auto sourceElement=node.try_as<FrameworkElement>())if(auto value=sourceElement.Tag().try_as<J>()){
        if(value.HasKey(L"bubble_choice")){auto found=ui(data,O({{L"type",S(L"choice")},{L"action",object(value,L"bubble_choice")}}));if(found.ValueType()==JsonValueType::Object)sourceGrab=found.GetObject();}
        else if(str(object(value,L"header_source"),L"kind")==L"item")sourceGrab=O({{L"kind",S(L"header")},{L"id",N(num(object(value,L"header_source"),L"value"))}});
        else if(str(object(value,L"workspace_action"),L"type")==L"tile_drag")sourceGrab=object(object(value,L"workspace_action"),L"item");
        if(sourceGrab.Size()){element=sourceElement;tag=value;break;}
    }
    if(!element||str(sourceGrab,L"kind")!=L"variant"&&!flag(source(sourceGrab),L"tears_bubble"))return;
    auto boundsOf=[&](FrameworkElement const& element){auto common=inputRoot.as<FrameworkElement>();return common.TransformToVisual(workspace).TransformBounds(visibleBounds(element,common));};
    Rect bounds=boundsOf(element);
    for(DependencyObject node=element;node&&node!=inputRoot;node=VisualTreeHelper::GetParent(node))if(auto ancestor=node.try_as<FrameworkElement>()){
        auto id=AutomationProperties::GetAutomationId(ancestor);
        if(prefix(id,L"workspace-group-")||id==L"tool-drawer"||prefix(id,L"column-drawer-")||id==L"title-bar"){bounds=boundsOf(ancestor);break;}
    }
    Candidate value;value.pointer=e.Pointer();value.last=at;value.bounds=bounds;value.grab=sourceGrab;value.source=make_weak(element);value.epoch=epoch;
    for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node)){if(auto view=node.try_as<UIElement>())value.path.push_back(make_weak(view));if(node==inputRoot)break;}
    value.identity=str(sourceGrab,L"kind")==L"variant"?object(tag,L"bubble_choice").Stringify():object(source(sourceGrab),L"resolved_control").Stringify();candidate=value;
}
void WorkspaceBubbles::Impl::move(PointerRoutedEventArgs const& e){
    colorTouch(e,L"move",nullptr);
    auto p=e.GetCurrentPoint(workspace);auto at=p.Position();if(p.Properties().IsCanceled()){cancel();return;}
    if(candidate&&candidate->pointer.PointerId()==p.PointerId()){
        if(!current(*candidate)){cancel(true);return;}candidate->last=at;
        if(!candidate->moving&&!contains(candidate->bounds,at)&&!data->popupCount&&!flag(data->chrome,L"dragging")){
            releasing=true;if(cancelSources)cancelSources();if(auto source=candidate->source.get())source.CancelDirectManipulations();bool captured=capture(candidate->pointer,candidate->path);releasing=false;
            if(!captured){cancel();return;}candidate->moving=true;drag(candidate->grab,L"down",at);
        }
        if(candidate->moving){drag(candidate->grab,L"move",at);e.Handled(true);}return;
    }
    if(!contact){
        auto selected=target(at);if(selected&&!hovering&&data->input)data->input(to_string(O({{L"type",S(L"cursor_leave")}}).Stringify()));hovering=bool(selected);
        pointerPen=p.PointerDeviceType()==NativeInput::PointerDeviceType::Pen;contactSurface();
        for(auto const& [id,r]:records){auto next=selected==r?hit(*r,at):JsonValue::CreateNullValue();if(!r->hovered||r->hovered.Stringify()!=next.Stringify()){r->hovered=next;wake();}}
        return;
    }
    auto c=contact;if(!c->fingers.contains(p.PointerId()))return;c->last=at;c->fingers[p.PointerId()]=at;c->moved=c->moved||distance(at,c->start)>8;bool moved=c->moved;if(moved)c->record->pressed=false;
    if(c->paired){
        if(!c->pairEnded&&c->fingers.size()==2){auto a=c->fingers.begin()->second,b=std::next(c->fingers.begin())->second;c->midpoint={(a.X+b.X)/2,(a.Y+b.Y)/2};
            if(!c->lifted){
                if(distance(c->midpoint,c->pairOrigin)<=8&&std::abs(distance(a,b)-c->spread)<=8){e.Handled(true);return;}
                c->lifted=true;drag(c->grab,L"down",c->pairOrigin);
            }
            if(str(c->record->view,L"kind")==L"color"){auto size=num(c->record->view,L"diameter");c->scale=ui(data,O({{L"type",S(L"pinched_size")},{L"size",N(size)},{L"scale",N(distance(a,b)/c->spread)}})).GetNumber()/size;c->record->pinch=c->scale;}
            drag(c->grab,L"move",c->midpoint);
        }e.Handled(true);return;
    }
    if(moved)stop(c);else if(c->recognizing)c->recognizer.Move(workspace,e);
    if(c->mode==L"native"){
        if(c->nativeDrags||!moved||e.Handled())return;
        if(!capture(c->pointer,c->path)){cancel();return;}c->mode=L"pending";
    }
    if(c->held){if(!moved){e.Handled(true);return;}c->held=false;dismiss();if(kind(c->hit)==L"wheel")c->mode=L"wheel";else begin(c,at);}
    if(c->mode==L"wheel"){
        if(ui(data,O({{L"type",S(L"color_drag_out")},{L"size",N(num(c->record->view,L"diameter"))},{L"point",point(relative(at,c->record->center))}})).GetBoolean()){
            restore(c);c->grab=grab(c->record->id,L"rim");drag(c->grab,L"down",at);c->mode=L"moving";
        }else pick(c,at);
    }else if(c->mode==L"intensity"){pick(c,at);
    }else if(c->mode==L"pending"&&moved){c->grab=grab(c->record->id);drag(c->grab,L"down",c->start);c->mode=L"moving";}
    if(c->mode==L"moving")drag(c->grab,L"move",at);
    else if(c->mode==L"ring"||c->mode==L"beads"||c->mode==L"choose")send(O({{L"type",S(c->mode)},{L"id",N(c->record->id)},{L"phase",S(L"move")},{L"position",point(relative(at,c->record->center))}}));
    e.Handled(true);wake();
}
void WorkspaceBubbles::Impl::up(PointerRoutedEventArgs const& e,bool cancelled){
    auto p=e.GetCurrentPoint(workspace);cancelled=cancelled||p.Properties().IsCanceled();
    colorTouch(e,cancelled?L"cancel":L"up",nullptr);
    if(candidate&&candidate->pointer.PointerId()==p.PointerId()){
        if(candidate->moving){drag(candidate->grab,cancelled?L"cancel":L"up",p.Position());e.Handled(true);}candidate.reset();
    }
    auto c=contact;if(!c||!c->fingers.contains(p.PointerId()))return;stop(c);c->last=p.Position();c->record->pressed=false;
    if(c->paired){if(!c->pairEnded){c->pairEnded=true;c->record->pinch=1;if(c->lifted){drag(c->grab,cancelled?L"cancel":L"up",c->midpoint);
        if(!cancelled&&str(c->record->view,L"kind")==L"color"&&std::abs(c->scale-1)>.01)send(O({{L"type",S(L"resize")},{L"id",N(c->record->id)},{L"scale",N(c->scale)}}));}}}
    else if(cancelled){data->pointerCancels.Cancel(c->pointer.PointerId());restore(c);if(c->held)dismiss();}
    else if(!c->held){
        if(c->mode==L"moving")drag(c->grab,L"up",c->last);
        else if(c->mode==L"ring"||c->mode==L"beads"||c->mode==L"choose")send(O({{L"type",S(c->mode)},{L"id",N(c->record->id)},{L"phase",S(L"up")},{L"position",point(relative(c->last,c->record->center))}}));
        else if(c->mode==L"pending")send(O({{L"type",S(L"tap")},{L"id",N(c->record->id)},{L"position",point(relative(c->last,c->record->center))}}));
    }
    finishColor(c,false);if(c->mode!=L"native")e.Handled(true);c->fingers.erase(p.PointerId());
    if(c->fingers.empty()){contact.reset();c->record->hovered=nullptr;}
    releasing=true;inputRoot.ReleasePointerCapture(e.Pointer());releasing=false;contactSurface();wake();
}
bool WorkspaceBubbles::Impl::cancel(bool abandon){
    bool had=contact||candidate||menuOwner.Active();dismiss();auto c=std::exchange(contact,{});auto source=std::exchange(candidate,{});
    for(auto const& [id,record]:records){had|=record->pressed||record->hovered&&record->hovered.ValueType()!=JsonValueType::Null;record->pressed=false;record->hovered=nullptr;}
    hovering=false;
    if(c){releasing=true;data->pointerCancels.Cancel(c->pointer.PointerId());for(auto weak:c->path)if(auto element=weak.get();element&&PointerGesture::Captured(element,c->pointer.PointerId()))element.ReleasePointerCapture(c->pointer);releasing=false;}
    if(c){stop(c);if(!abandon){if(c->paired&&!c->pairEnded){if(c->lifted)drag(c->grab,L"cancel",c->midpoint);}else restore(c);}c->record->pinch=1;c->record->pressed=false;c->record->hovered=nullptr;}
    colorTouches.cancel(*data,layer);
    if(source&&source->moving&&!abandon)drag(source->grab,L"cancel",source->last);
    releasing=true;if(inputRoot)inputRoot.ReleasePointerCaptures();releasing=false;holdTimer.Stop();contactSurface();if(had)wake();return had;
}
void WorkspaceBubbles::Impl::hold(){
    auto c=contact;if(!c||c->paired||c->holdAt<0){holdTimer.Stop();return;}
    if(milliseconds()<c->holdAt)return;if(c->mode==L"native"&&!capture(c->pointer,c->path)){cancel();return;}stop(c);restore(c);c->held=true;showMenu(c->record,c->start);holdTimer.Stop();
}
void WorkspaceBubbles::Impl::dismiss(){++menuGeneration;menuOwner.Cancel();menu=nullptr;menuId=0;menuSource=L"";keepMenu=false;}
void WorkspaceBubbles::Impl::showMenu(std::shared_ptr<Record> const& record,Point at,bool palettes){
    auto live=records.find(record->id);if(live==records.end()||live->second!=record)return;
    bool refresh=menuOwner.Open()&&menuId==record->id;
    if(!refresh){dismiss();menu=MenuFlyout();menuId=record->id;menuSource=record->source;
        menu.Closing([weak=weak_from_this()](auto&&,auto const& args){if(auto self=weak.lock();self&&std::exchange(self->keepMenu,false))args.Cancel(true);});}
    menuRequest=O({{L"type",S(L"context")},{L"target",O({{L"kind",S(palettes?L"bubble_palettes":L"bubble")},{L"id",N(record->id)}})}});
    auto anchor=record->root;
    auto ticket=refresh?OwnedPopup::Ticket{}:menuOwner.Begin(data,anchor,menu);auto serial=++menuGeneration;auto document=epoch;auto weak=weak_from_this();
    QueryWorkspace(data->query,menuRequest,[weak,record,anchor,at,serial,document,refresh,ticket](J reply){auto self=weak.lock();
        if(!self||document!=self->epoch||serial!=self->menuGeneration||(!refresh&&!ticket.Current()))return;
        auto current=[weak,record,document,shown=make_weak(self->menu)]()->std::shared_ptr<Impl>{
            auto self=weak.lock();if(!self||document!=self->epoch||self->menuId!=record->id||self->menu!=shown.get())return {};
            auto live=self->records.find(record->id);return live!=self->records.end()&&live->second==record?self:nullptr;
        };
        NativeMenuItems(self->menu.Items(),array(object(reply,L"result"),L"sections"),self->data,[current,record,at](J action){if(auto self=current()){
            bool keep=self->keepMenu;self->data->dispatch(action);self->wake();
            if(keep)self->workspace.DispatcherQueue().TryEnqueue([current,record,at]{if(auto self=current();self&&self->menuOwner.Open())self->showMenu(record,at);});
        }},[current](bool keep){if(auto self=current())self->keepMenu=keep;});
        if(!refresh){Primitives::FlyoutShowOptions options;options.Position(self->workspace.TransformToVisual(anchor).TransformPoint(at));ticket.Show(anchor,options);}
    });
}
void WorkspaceBubbles::Impl::attach(UIElement const& root,std::function<void()> cancelOther){
    inputRoot=root;cancelSources=std::move(cancelOther);auto weak=weak_from_this();
    data->bubbleContact=[weak](PointerRoutedEventArgs const& e){if(auto self=weak.lock())return self->join(e);return false;};
    root.as<Panel>().Children().Append(layer);Canvas::SetZIndex(layer,800);
    AutomationProperties::SetAutomationId(layer,L"tool-bubble-desk");
    auto bind=[&](RoutedEvent const& event,auto handler){root.AddHandler(event,box_value(PointerEventHandler([weak,handler](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())(self.get()->*handler)(e);})),true);};
    bind(UIElement::PointerPressedEvent(),&Impl::down);bind(UIElement::PointerMovedEvent(),&Impl::move);
    root.PointerExited([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&!self->contact&&!self->candidate){
        auto view=self->inputRoot.as<FrameworkElement>();auto at=e.GetCurrentPoint(view).Position();
        if(!contains({0,0,float(view.ActualWidth()),float(view.ActualHeight())},at))self->cancel();
    }});
    root.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->up(e,false);})),true);
    root.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&!self->releasing)self->up(e,true);})),true);
    root.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
        if(self->releasing)return;
        auto p=e.GetCurrentPoint(nullptr);
        if(e.OriginalSource()==self->inputRoot){
            bool contact=self->contact&&self->contact->fingers.contains(p.PointerId())&&self->contact->mode!=L"native";
            bool candidate=self->candidate&&self->candidate->moving&&self->candidate->pointer.PointerId()==p.PointerId();
            if(contact||candidate){self->cancel();return;}
        }
        if(p.Properties().IsCanceled())self->up(e,true);
        else if(!p.IsInContact())self->up(e,false);
    }})),true);
    root.as<FrameworkElement>().Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
    data->takeBubble=[weak](J item,Point at,std::function<bool()> valid,std::function<void()> done){auto self=weak.lock();if(!self){done();return;}
        auto identity=object(self->source(item),L"resolved_control").Stringify();auto epoch=self->epoch;
        auto current=[weak,item,identity,epoch,valid]{auto self=weak.lock();return self&&self->epoch==epoch&&valid()&&object(self->source(item),L"resolved_control").Stringify()==identity;};
        if(!QueryWorkspace(self->data->query,O({{L"type",S(L"bubble_grab")},{L"grab",item},{L"held",B(true)}}),[weak,at,current,done](J reply){auto self=weak.lock();auto grab=object(reply,L"result");if(!self||!current()||!grab.Size()){done();return;}
            if(!QueryWorkspace(self->data->query,O({{L"type",S(L"can_take_bubble")},{L"grab",grab},{L"position",point(at)},{L"viewport",point({float(self->workspace.ActualWidth()),float(self->workspace.ActualHeight())})}}),[weak,at,grab,current,done](J reply){if(auto self=weak.lock();self&&current()&&flag(reply,L"result")){self->drag(grab,L"down",at);self->drag(grab,L"up",at);}done();}))done();
        }))done();
    };
}
