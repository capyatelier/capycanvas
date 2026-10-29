#include "pch.h"
#include "NativeJson.h"
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
}
struct LayerRowDrag::Impl:std::enable_shared_from_this<Impl>{
    std::weak_ptr<LayersView> view;
    std::weak_ptr<WorkspaceData> context;
    uint64_t transient=0;
    Grid surface{nullptr};
    weak_ref<UIElement> inputSurface;
    std::pair<event_token,event_token> inputKey{};
    PointerEventHandler inputMove{nullptr},inputUp{nullptr},inputCancel{nullptr};
    ScrollView list{nullptr};
    std::weak_ptr<LayerRow> source;
    Input::Pointer pointer{nullptr};
    struct TouchContact { Input::Pointer pointer; double row; Point point; std::vector<weak_ref<UIElement>> path; };
    std::map<uint32_t,TouchContact> touches;
    bool multitouch=false;
    uint64_t touchTimestamp=0;
    hstring touchEpoch;
    std::optional<uint32_t> rejected;
    PointerGesture recognizer;
    NativeInput::PointerDeviceType device=NativeInput::PointerDeviceType::Mouse;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::vector<weak_ref<UIElement>> path;
    struct ScrollClaim {ManipulationModes input;ScrollingScrollMode horizontal,vertical;};
    std::optional<ScrollClaim> scrollInput;
    struct Hit {double id;int zone;bool thumbnail;bool operator==(Hit const&)const=default;};
    std::optional<Hit> hit,answered;
    hstring placement,epoch;
    double id=-1,normalizedTarget=-1,effectOwner=-1;
    Point origin{},position{};
    double initialScroll=0,sourceOpacity=1,initialSwipe=0;
    bool swiping=false;
    HWND windowHandle=nullptr;
    bool grip=false,mask=false,held=false,dragging=false,finishing=false,pickup=false;
    bool releasing=false,recognizing=false,ignoreClick=false,suppressContext=false,busy=false;
    bool trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    uint64_t generation=0,revision=0;
    J lastRelease,lastCancel,lastCaptureLoss,lastMove,lastIgnoredDown;
    std::chrono::steady_clock::time_point tickAt;
    bool focus()const{return windowHandle&&GetAncestor(GetForegroundWindow(),GA_ROOTOWNER)==windowHandle&&!IsIconic(windowHandle);}
    bool crossed()const{return recognizer.Moved();}
    bool edgeHeld()const{
        auto owner=view.lock();return pointer&&owner&&owner->data->edgeHolding&&owner->data->edgeHolding(pointer.PointerId());
    }
    bool owns(UIElement const& element)const{
        return pointer&&PointerGesture::Captured(element,pointer.PointerId());
    }
    bool capture(Input::Pointer const& contact,std::vector<weak_ref<UIElement>> const& route){
        for(auto const& entry:route)if(auto node=entry.get();node&&PointerGesture::Captured(node,contact.PointerId()))node.ReleasePointerCapture(contact);
        return surface.CapturePointer(contact);
    }
    void sendTouch(uint32_t contact,hstring const& phase,TouchContact const& value){
        auto owner=view.lock();if(!owner||!owner->data->input)return;
        double scale=surface.XamlRoot()?surface.XamlRoot().RasterizationScale():1.;A coordinates;
        coordinates.Append(N(value.point.X*scale));coordinates.Append(N(value.point.Y*scale));
        owner->data->input(CapyUi::JsonUnsignedFields(to_string(O({{L"type",S(L"layer_touch")},
            {L"phase",S(phase)},{L"position",coordinates}}).Stringify()),
            {{"surface",reinterpret_cast<uintptr_t>(get_abi(surface))},{"id",contact},
                {"row",uint64_t(value.row)},{"time_ns",touchTimestamp}}));
    }
    bool cancelTouches(hstring reason=L"touch_cancel"){
        if(touches.empty())return false;
        if(trace)lastCancel=O({{L"reason",S(reason)},{L"generation",N(double(generation))},
            {L"touch_contacts",N(double(touches.size()))},{L"multitouch",B(multitouch)}});
        auto pending=std::move(touches);touches.clear();
        for(auto const& [contact,value]:pending){
            sendTouch(contact,L"cancel",value);
            if(auto owner=view.lock())owner->data->pointerCancels.Forget(contact,this);
        }
        if(multitouch){multitouch=false;release();ignoreClick=true;deferClick();}
        publish();
        return true;
    }
    bool touch(PointerRoutedEventArgs const& e,hstring const& phase){
        auto point=e.GetCurrentPoint(surface);if(point.PointerDeviceType()!=NativeInput::PointerDeviceType::Touch)return false;
        auto contact=e.Pointer().PointerId();touchTimestamp=point.Timestamp()*1000;
        if(point.Properties().IsCanceled()){bool handled=multitouch;cancelTouches(L"pointer_canceled");if(handled)e.Handled(true);return handled;}
        auto owner=view.lock();if(!owner)return false;
        if(phase==L"down"){
            if(touches.contains(contact)){if(multitouch)e.Handled(true);return multitouch;}
            std::shared_ptr<LayerRow> row;
            auto original=e.OriginalSource().try_as<DependencyObject>();
            for(auto const& [element,candidate]:owner->rows)if(inside(original,candidate->root)&&!candidate->renaming){row=candidate;break;}
            if(!row)return false;
            if(touches.empty())touchEpoch=epochOf(owner->data);
            TouchContact value{e.Pointer(),row->id,point.Position(),{}};
            for(auto node=original;node;node=VisualTreeHelper::GetParent(node)){
                if(auto element=node.try_as<UIElement>())value.path.push_back(make_weak(element));if(node==surface)break;
            }
            touches.emplace(contact,std::move(value));
        }
        auto found=touches.find(contact);if(found==touches.end())return false;
        auto at=point.Position();
        if(phase==L"move"&&at.X==found->second.point.X&&at.Y==found->second.point.Y){if(multitouch)e.Handled(true);return multitouch;}
        found->second.point=at;sendTouch(contact,phase,found->second);
        if(touches.size()>=2&&!multitouch){
            if(owner->data->cancelEdge)for(auto const& contact:touches)if(!owner->data->cancelEdge(contact.first)){
                cancelTouches(L"edge_contact");e.Handled(true);return true;
            }
            cancel(L"multiple_contacts",true);multitouch=true;ignoreClick=true;suppressContext=true;
            releasing=true;bool captured=true;
            auto content=list.Content().as<UIElement>();scrollInput=ScrollClaim{content.ManipulationMode(),list.HorizontalScrollMode(),list.VerticalScrollMode()};
            content.ManipulationMode(ManipulationModes::None);list.HorizontalScrollMode(ScrollingScrollMode::Disabled);list.VerticalScrollMode(ScrollingScrollMode::Disabled);
            for(auto const& [contactId,value]:touches){
                owner->data->pointerCancels.Observe(contactId,this,[weak=weak_from_this()]{if(auto self=weak.lock())self->cancel(L"touch_capture_handoff");});
                captured=capture(value.pointer,value.path)&&captured;
            }
            releasing=false;
            if(!captured){cancelTouches(L"capture_failed");e.Handled(true);return true;}
            publish();
        }
        bool handled=multitouch;
        if(phase==L"up"){
            owner->data->pointerCancels.Forget(contact,this);touches.erase(contact);
            if(multitouch&&touches.empty()){multitouch=false;release();ignoreClick=true;deferClick();publish();}
        }
        if(handled)e.Handled(true);
        return handled;
    }
    bool movable(J const& row)const{
        if(grip)return flag(row,L"drag_from_handle");
        auto kind=device==NativeInput::PointerDeviceType::Mouse?L"mouse":device==NativeInput::PointerDeviceType::Pen?L"pen":L"touch";
        for(auto value:array(row,L"drag_from_body"))if(value.GetString()==kind)return true;
        return false;
    }
    bool current()const{
        auto owner=view.lock();if(!owner||epoch!=epochOf(owner->data)||!surface.IsLoaded()||list.Visibility()!=Visibility::Visible)return false;
        auto row=panelRow(owner->data->state,id);
        if(!row.Size())return false;
        auto rename=owner->view().GetNamedValue(L"rename_layer",JsonValue::CreateNullValue());
        if(rename.ValueType()==JsonValueType::Number&&rename.GetNumber()==id)return false;
        if(dragging&&!movable(row))return false;
        if(!held&&!dragging){auto item=source.lock();if(!item||!item->root.IsLoaded())return false;}
        return true;
    }
    void publish(){
        auto contact=(multitouch||held||dragging||swiping||(pointer&&grip))?L"input":L"";
        auto tag=surface.Tag().try_as<J>();if(!tag||str(tag,L"edge_contact")!=contact)edgeTag(surface,L"edge_contact",S(contact));
        if(!trace)return;
        auto owner=view.lock();
        auto sourceRow=source.lock();
        auto value=O({{L"phase",S(finishing?L"finishing":dragging?L"dragging":held?L"held":pointer?L"pressed":L"idle")},
            {L"source",id>=0?N(id):JsonValue::CreateNullValue()},{L"generation",N(double(generation))},
            {L"device",S(device==NativeInput::PointerDeviceType::Mouse?L"mouse":device==NativeInput::PointerDeviceType::Pen?L"pen":L"touch")},
            {L"grip",B(grip)},{L"mask",B(mask)},{L"captured",B(owns(surface))},{L"ignore_click",B(ignoreClick)},
            {L"touch_contacts",N(double(touches.size()))},{L"multitouch",B(multitouch)},
            {L"touch_captures",N(double(std::count_if(touches.begin(),touches.end(),[&](auto const& entry){return PointerGesture::Captured(surface,entry.first);})))},
            {L"pickup",B(pickup)},{L"initial_swipe",N(initialSwipe)},
            {L"source_swipe",sourceRow?N(sourceRow->swipeOffset):JsonValue::CreateNullValue()},
            {L"source_grip_opacity",sourceRow?N(sourceRow->grip.Opacity()):JsonValue::CreateNullValue()},
            {L"origin_x",N(origin.X)},{L"origin_y",N(origin.Y)},{L"pointer_x",N(position.X)},{L"pointer_y",N(position.Y)},
            {L"scroll_claimed",B(scrollInput.has_value())},{L"menu_open",B(owner&&owner->menuOwner.Open())},
            {L"pointer_id",pointer?N(pointer.PointerId()):JsonValue::CreateNullValue()},
            {L"effect_owner",effectOwner>=0?N(effectOwner):JsonValue::CreateNullValue()},{L"can_drop",B(!placement.empty())},{L"position",S(placement)},
            {L"target",normalizedTarget>=0?N(normalizedTarget):JsonValue::CreateNullValue()},{L"surface",S(hit&&hit->thumbnail?L"thumbnail":L"row")}});
        if(lastRelease.Size())value.Insert(L"last_release",lastRelease);
        if(lastCancel.Size())value.Insert(L"last_cancel",lastCancel);
        if(lastCaptureLoss.Size())value.Insert(L"last_capture_loss",lastCaptureLoss);
        if(lastMove.Size())value.Insert(L"last_move",lastMove);
        if(lastIgnoredDown.Size())value.Insert(L"last_ignored_down",lastIgnoredDown);
        AutomationProperties::SetItemStatus(list,value.Stringify());
    }
    void deferClick(){
        auto version=generation;
        surface.DispatcherQueue().TryEnqueue(Microsoft::UI::Dispatching::DispatcherQueuePriority::Low,
            [weak=weak_from_this(),version]{if(auto self=weak.lock();self&&self->generation==version&&!self->pointer&&!self->rejected){self->ignoreClick=false;self->publish();}});
    }
    void hideMenu(){
        if(auto owner=view.lock())owner->menuOwner.Cancel();
    }
    void marks(){
        if(auto owner=view.lock())for(auto const& [element,row]:owner->rows)
            row->highlight(normalizedTarget==row->id?(placement==L"attach"?4:placement==L"into"?3:placement==L"above"?1:placement==L"below"?2:0):0,effectOwner==row->id);
    }
    void release(){
        releasing=true;surface.ReleasePointerCaptures();
        if(scrollInput){
            list.HorizontalScrollMode(scrollInput->horizontal);list.VerticalScrollMode(scrollInput->vertical);
            list.Content().as<UIElement>().ManipulationMode(scrollInput->input);scrollInput.reset();
        }
        releasing=false;path.clear();
    }
    void clear(bool closeMenu){
        if(pointer)if(auto owner=view.lock())owner->data->pointerCancels.Forget(pointer.PointerId(),this);
        if(swiping)if(auto row=source.lock())row->swipe(0);
        if(dragging)if(auto row=source.lock())row->root.Opacity(sourceOpacity);
        ++generation;pointer=nullptr;source.reset();held=false;dragging=false;swiping=false;finishing=false;busy=false;
        hit.reset();answered.reset();placement=L"";normalizedTarget=-1;effectOwner=-1;timer.Stop();
        if(recognizing){recognizing=false;recognizer.CompleteGesture();}
        release();if(closeMenu)hideMenu();
        marks();deferClick();publish();
    }
    bool cancel(hstring reason=L"cancel",bool keepTouches=false){
        bool touched=!keepTouches&&cancelTouches(reason);
        auto owner=view.lock();bool active=bool(pointer)||finishing;
        if(pointer&&owner&&owner->data->cancelEdge)owner->data->cancelEdge(pointer.PointerId());
        bool revealed=false;if(owner)for(auto const& [key,row]:owner->rows)if(row->swipeOffset!=0){row->swipe(0);revealed=true;}
        if(!active&&!(owner&&owner->menuOwner.Active()))return revealed||touched;
        if(active){
            rejected=pointer?std::optional<uint32_t>(pointer.PointerId()):std::nullopt;
            ignoreClick=true;
            if(trace)lastCancel=O({{L"reason",S(reason)},{L"generation",N(double(generation))}});
        }
        clear(true);return true;
    }
    bool claim(){
        if(!pointer||!current())return false;
        if(auto owner=view.lock();owner&&owner->data->cancelEdge&&!owner->data->cancelEdge(pointer.PointerId()))return false;
        if(owns(surface))return true;
        releasing=true;
        auto content=list.Content().as<UIElement>();scrollInput=ScrollClaim{content.ManipulationMode(),list.HorizontalScrollMode(),list.VerticalScrollMode()};
        content.ManipulationMode(ManipulationModes::None);
        list.HorizontalScrollMode(ScrollingScrollMode::Disabled);list.VerticalScrollMode(ScrollingScrollMode::Disabled);
        bool captured=capture(pointer,path);releasing=false;
        if(!captured)cancel(L"capture_failed");
        return captured;
    }
    void hold(NativeInput::HoldingEventArgs const& e){
        if(e.HoldingState()!=NativeInput::HoldingState::Started||!pointer||held||dragging||swiping||device==NativeInput::PointerDeviceType::Mouse)return;
        if(!claim())return;
        held=true;ignoreClick=true;suppressContext=true;
        if(auto row=source.lock())if(auto owner=view.lock()){
            row->swipe(0);auto at=surface.TransformToVisual(row->root).TransformPoint(position);
            owner->context(id,mask,row->root,at,true);
        }
        publish();
    }
    void down(PointerRoutedEventArgs const& e){
        if(finishing){
            if(trace){lastIgnoredDown=O({{L"generation",N(double(generation))},{L"pointer_id",N(e.Pointer().PointerId())},{L"reason",S(L"drop_pending")}});publish();}
            e.Handled(true);return;
        }
        auto owner=view.lock();if(owner&&owner->data->claimBubbleContact(e))return;
        if(touch(e,L"down"))return;
        if(pointer){if(pointer.PointerId()!=e.Pointer().PointerId())cancel(L"multiple_contacts");return;}
        rejected.reset();ignoreClick=false;suppressContext=false;++generation;
        auto original=e.OriginalSource().try_as<DependencyObject>();
        if(!owner||!original)return;
        auto point=e.GetCurrentPoint(surface);
        if(!PointerGesture::PrimaryContact(point))return;
        for(auto const& [element,row]:owner->rows)if(inside(original,row->root)){
            if(row->renaming||inside(original,row->rename)||!row->current())return;
            source=row;id=row->id;epoch=row->epoch;pointer=e.Pointer();device=point.PointerDeviceType();
            owner->data->pointerCancels.Observe(pointer.PointerId(),this,[weak=weak_from_this()]{if(auto self=weak.lock())self->cancel(L"capture_handoff");});
            grip=inside(original,row->grip);mask=inside(original,row->mask);initialSwipe=std::max(0.,row->swipeOffset);row->swipe(initialSwipe,false);
            pickup=initialSwipe==0&&movable(row->model());
            for(auto const& [key,other]:owner->rows)if(other!=row)other->swipe(0);
            origin=position=point.Position();initialScroll=list.VerticalOffset();windowHandle=GetAncestor(GetForegroundWindow(),GA_ROOTOWNER);
            for(auto node=original;node&&node!=surface;node=VisualTreeHelper::GetParent(node))
                if(auto item=node.try_as<UIElement>())path.emplace_back(make_weak(item));
            if((mask||inside(original,row->content))&&row->loadThumbnail(mask,e.KeyModifiers())){ignoreClick=true;pickup=false;e.Handled(true);}
            if(grip&&pickup){ignoreClick=true;if(!claim())return;}
            recognizing=true;recognizer.Begin(row->root,point,device==NativeInput::PointerDeviceType::Mouse?NativeInput::GestureSettings::None:NativeInput::GestureSettings::Hold);
            tickAt=std::chrono::steady_clock::now();timer.Start();publish();return;
        }
    }
    std::optional<Hit> pick()const{
        auto at=surface.TransformToVisual(list).TransformPoint(position);
        if(at.X<0||at.Y<0||at.X>list.ActualWidth()||at.Y>list.ActualHeight())return {};
        if(auto owner=view.lock())for(auto const& [element,row]:owner->rows){
            if(!row->current()||!row->root.IsLoaded())continue;
            auto bounds=row->root.TransformToVisual(list).TransformBounds({0,0,float(row->root.ActualWidth()),float(row->root.ActualHeight())});
            if(at.X>=bounds.X&&at.X<bounds.X+bounds.Width&&at.Y>=bounds.Y&&at.Y<bounds.Y+bounds.Height&&bounds.Height>0){
                auto thumb=row->content.TransformToVisual(list).TransformBounds({0,0,float(row->content.ActualWidth()),float(row->content.ActualHeight())});
                bool thumbnail=at.X>=thumb.X&&at.X<thumb.X+thumb.Width&&at.Y>=thumb.Y&&at.Y<thumb.Y+thumb.Height;
                return Hit{row->id,std::clamp(int((at.Y-bounds.Y)/bounds.Height*4),0,3),thumbnail};
            }
        }
        return {};
    }
    void complete(){
        if(!finishing||busy||hit!=answered)return;
        bool commit=current()&&focus()&&hit&&!placement.empty();
        auto owner=view.lock();auto destination=hit;auto document=epoch;auto sourceId=id;
        if(trace)lastRelease=O({{L"source",N(id)},{L"commit",B(commit)},{L"generation",N(double(generation))},
            {L"target",normalizedTarget>=0?N(normalizedTarget):JsonValue::CreateNullValue()},{L"surface",S(hit&&hit->thumbnail?L"thumbnail":L"row")},{L"position",S(placement)}});
        clear(true);
        if(commit&&owner)owner->data->dispatchDocument(layerAction(O({{L"op",S(L"drop")},{L"id",N(sourceId)},
            {L"target",N(destination->id)},{L"fraction",N((destination->zone+.5)/4)},{L"surface",S(destination->thumbnail?L"thumbnail":L"row")}})),document);
    }
    void query(){
        if(!dragging||busy||!hit||hit==answered){complete();return;}
        auto owner=view.lock();if(!owner)return;
        auto version=generation,modelVersion=revision;auto requested=*hit;busy=true;
        auto request=O({{L"type",S(L"layer_drop")},{L"epoch",N(std::stod(to_string(epoch)))},
            {L"id",N(id)},{L"target",N(requested.id)},{L"fraction",N((requested.zone+.5)/4)},{L"surface",S(requested.thumbnail?L"thumbnail":L"row")}});
        if(!QueryWorkspace(owner->data->query,request,[weak=weak_from_this(),version,modelVersion,requested](J packet){
            auto self=weak.lock();if(!self||self->generation!=version)return;self->busy=false;
            if(!self->current()||!self->focus()){self->cancel(L"source_or_focus");return;}
            if(self->revision==modelVersion&&self->hit==requested){
                auto result=object(packet,L"result");self->answered=requested;
                self->placement=num(result,L"epoch",-1)==std::stod(to_string(self->epoch))?str(result,L"position"):L"";
                self->normalizedTarget=self->placement.empty()?-1:num(result,L"target",-1);
                auto attachment=result.GetNamedValue(L"effect_owner",JsonValue::CreateNullValue());
                self->effectOwner=!self->placement.empty()&&attachment.ValueType()==JsonValueType::Number?attachment.GetNumber():-1;
                self->marks();self->publish();
            }
            self->query();
        }))busy=false;
    }
    void target(){
        auto next=pick();
        if(hit!=next){hit=next;answered.reset();placement=L"";normalizedTarget=-1;effectOwner=-1;marks();publish();}
        query();
    }
    void motion(PointerRoutedEventArgs const& e){
        if(touch(e,L"move"))return;
        if(trace&&pointer){auto point=e.GetCurrentPoint(surface);lastMove=O({{L"pointer_id",N(e.Pointer().PointerId())},
            {L"x",N(point.Position().X)},{L"y",N(point.Position().Y)}});}
        if(!pointer||pointer.PointerId()!=e.Pointer().PointerId()||finishing)return;
        if(!current()){cancel(L"source_invalid");return;}
        position=e.GetCurrentPoint(surface).Position();
        if(recognizing)recognizer.Move(surface,e);
        if(edgeHeld()){if(crossed())ignoreClick=true;return;}
        if(!dragging&&!swiping&&crossed()){
            if(device!=NativeInput::PointerDeviceType::Mouse&&!grip&&!held){
                auto row=source.lock();double dx=position.X-origin.X,dy=position.Y-origin.Y;
                if(row&&std::abs(dx)>std::abs(dy)&&((dx<0&&row->canSwipe())||(dx>0&&(initialSwipe>0||object(row->model(),L"right_swipe").Size())))){
                    if(!claim())return;
                    swiping=true;ignoreClick=true;suppressContext=true;hideMenu();
                    if(recognizing){recognizing=false;recognizer.CompleteGesture();}
                }else{cancel(L"native_scroll");return;}
            }
            if(swiping){if(auto row=source.lock())row->swipe(initialSwipe>0?std::max(0.,initialSwipe-(position.X-origin.X)):initialSwipe-(position.X-origin.X),false);e.Handled(true);return;}
            auto owner=view.lock();auto row=owner?panelRow(owner->data->state,id):J{};
            auto sourceRow=source.lock();
            if(!pickup||!movable(row)||!sourceRow||sourceRow->swipeOffset!=0)return;
            if(!claim())return;
            dragging=true;ignoreClick=true;suppressContext=true;hideMenu();
            sourceOpacity=sourceRow->root.Opacity();sourceRow->root.Opacity(.55);
        }
        if(swiping){if(auto row=source.lock())row->swipe(initialSwipe>0?std::max(0.,initialSwipe-(position.X-origin.X)):initialSwipe-(position.X-origin.X),false);e.Handled(true);}
        if(dragging){target();e.Handled(true);publish();}
    }
    void up(PointerRoutedEventArgs const& e){
        if(touch(e,L"up"))return;
        if(rejected&&*rejected==e.Pointer().PointerId()){rejected.reset();deferClick();return;}
        if(!pointer||pointer.PointerId()!=e.Pointer().PointerId()||finishing)return;
        if(!focus()||e.GetCurrentPoint(surface).Properties().IsCanceled()){cancel(L"release_canceled");return;}
        if(recognizing){recognizing=false;recognizer.ProcessUpEvent(e.GetCurrentPoint(surface));}
        position=e.GetCurrentPoint(surface).Position();
        if(swiping){
            auto row=source.lock();auto operation=row&&row->swipeOffset<=-72*.4?object(row->model(),L"right_swipe"):J{};
            if(row)row->swipe(row->swipeOffset>=(initialSwipe>0?initialSwipe-72*.4:72*.4)?row->revealWidth():0);
            swiping=false;e.Handled(true);clear(true);if(row&&operation.Size())row->action(operation);return;
        }
        if(dragging){
            finishing=true;ignoreClick=true;e.Handled(true);
            if(auto owner=view.lock())owner->data->pointerCancels.Forget(pointer.PointerId(),this);
            release();
            // A release always asks current shared policy again, even when the
            // pointer stayed in a cached target while another action changed it.
            ++revision;answered.reset();placement=L"";normalizedTarget=-1;effectOwner=-1;target();publish();
        }else{
            if(auto row=source.lock()){
                bool action=false;for(auto const& weak:path)if(auto node=weak.get();node&&node==row->swipeActions)action=true;
                row->swipe(action&&initialSwipe>0?row->revealWidth():0);
            }
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
                scrollVertical(list,list.VerticalOffset()+speed*480*elapsed);
            }
            target();
        }else if(finishing)query();
        publish();
    }
    void detachInput(){
        if(auto root=inputSurface.get()){
            root.PreviewKeyDown(inputKey.first);root.KeyDown(inputKey.second);
            root.RemoveHandler(UIElement::PointerMovedEvent(),box_value(inputMove));
            root.RemoveHandler(UIElement::PointerReleasedEvent(),box_value(inputUp));
            root.RemoveHandler(UIElement::PointerCanceledEvent(),box_value(inputCancel));
        }
        inputSurface={};
    }
    void attachInput(){
        detachInput();if(!surface.XamlRoot())return;
        auto root=surface.XamlRoot().Content();if(!root)return;
        inputSurface=make_weak(root);
        inputMove=PointerEventHandler([weak=weak_from_this()](auto&&,auto&& e){if(auto self=weak.lock())self->motion(e);});
        inputUp=PointerEventHandler([weak=weak_from_this()](auto&&,auto&& e){if(auto self=weak.lock())self->up(e);});
        inputCancel=PointerEventHandler([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&!self->releasing&&self->touches.contains(e.Pointer().PointerId()))self->cancelTouches(L"pointer_canceled");
            if(auto self=weak.lock();self&&!self->releasing&&!self->finishing&&self->pointer&&self->pointer.PointerId()==e.Pointer().PointerId())self->cancel(L"pointer_canceled");
        });
        root.AddHandler(UIElement::PointerMovedEvent(),box_value(inputMove),true);
        root.AddHandler(UIElement::PointerReleasedEvent(),box_value(inputUp),true);
        root.AddHandler(UIElement::PointerCanceledEvent(),box_value(inputCancel),true);
        inputKey=ShortcutKeys(root,[weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(composingKey(e))return;
            if(e.Key()==Windows::System::VirtualKey::Escape&&self->cancel(L"escape"))e.Handled(true);
            else if(!self->pointer){self->rejected.reset();self->ignoreClick=false;self->suppressContext=false;}
        }});
    }
    ~Impl(){if(auto data=context.lock())data->transients.erase(transient);detachInput();}
    void init(){
        if(auto data=context.lock())transient=data->transient([weak=weak_from_this()]{if(auto self=weak.lock())return self->cancel();return false;});
        recognizer.Holding([weak=weak_from_this()](auto&&,NativeInput::HoldingEventArgs const& e){if(auto self=weak.lock())self->hold(e);});
        timer=surface.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(16));
        timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->tick();});
        surface.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,auto&& e){if(auto self=weak.lock())self->down(e);})),true);
        PointerGesture::OnLeaving(surface,[weak=weak_from_this()](PointerRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&!self->finishing&&self->pointer&&self->pointer.PointerId()==e.Pointer().PointerId()&&!self->owns(self->surface)){
                for(auto const& entry:self->path)if(auto node=entry.get();node&&self->owns(node))return;
                self->clear(false);
            }
        });
        surface.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&!self->releasing){
                auto contactId=e.Pointer().PointerId();bool paired=self->multitouch&&self->touches.contains(contactId);
                if(!paired&&(self->finishing||!self->pointer||self->pointer.PointerId()!=contactId))return;
                auto point=e.GetCurrentPoint(self->surface);bool captured=PointerGesture::Captured(self->surface,contactId);
                if(self->trace){
                    auto original=e.OriginalSource();auto node=original.try_as<DependencyObject>();
                    self->lastCaptureLoss=O({{L"surface_owned",B(captured)},{L"multitouch",B(paired)},
                        {L"held",B(self->held)},{L"dragging",B(self->dragging)},{L"in_contact",B(point.IsInContact())},
                        {L"canceled",B(point.Properties().IsCanceled())},
                        {L"source_type",S(original?get_class_name(original):hstring{})},
                        {L"source_id",S(node?AutomationProperties::GetAutomationId(node):hstring{})}});
                }
                if(paired){if(!captured)self->cancelTouches(L"capture_lost");self->publish();}
                else if(!self->held&&!self->dragging&&!self->swiping&&!self->grip&&!point.IsInContact())self->up(e);
                else if(point.Properties().IsCanceled())self->cancel(L"pointer_canceled");
                else self->cancel(L"capture_lost");
            }
        })),true);
        list.ViewChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&self->pointer&&!self->held&&!self->grip&&!self->dragging&&!self->swiping&&!self->edgeHeld()
            &&self->device!=NativeInput::PointerDeviceType::Mouse&&std::abs(self->list.VerticalOffset()-self->initialScroll)>.5)self->cancel(L"native_scroll");});
        surface.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->attachInput();});
        surface.Unloaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){self->cancel(L"unloaded");self->detachInput();}});
        if(surface.IsLoaded())attachInput();
    }
};
LayerRowDrag::LayerRowDrag(std::shared_ptr<LayersView> const& view):impl(std::make_shared<Impl>()){
    impl->view=view;impl->context=view->data;impl->surface=view->root;impl->list=view->list;impl->init();
}
LayerRowDrag::~LayerRowDrag(){impl->cancelTouches();impl->clear(true);}
void LayerRowDrag::Attach(std::shared_ptr<LayerRow> const& row){
    PointerGesture::OwnHolding(row->root);edgeTag(row->root,L"edge_contact",S(L"pickup"));edgeTag(row->grip,L"edge_contact",S(L"input"));
    row->root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler(
        [weak=std::weak_ptr(impl)](auto&&,auto&& e){if(auto self=weak.lock())self->down(e);})),true);
}
void LayerRowDrag::Refresh(){
    if(auto owner=impl->view.lock();owner&&!impl->touches.empty()&&impl->touchEpoch!=epochOf(owner->data))impl->cancel(L"document_changed");
    if(impl->pointer){
        if(!impl->current())impl->cancel(L"source_invalid");
        else if(impl->dragging){++impl->revision;impl->answered.reset();impl->placement=L"";impl->normalizedTarget=-1;impl->effectOwner=-1;impl->marks();impl->query();}
    }
}
void LayerRowDrag::MenuChanged(){impl->publish();}
bool LayerRowDrag::SuppressClick()const{return impl->ignoreClick;}
bool LayerRowDrag::SuppressContext()const{return impl->suppressContext;}
bool LayerRowDrag::Cancel(){return impl->cancel();}
