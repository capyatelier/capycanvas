#include "pch.h"
#include "WorkspaceGestures.h"
#include "WorkspaceQuery.h"
#include "WorkspaceGeometry.h"
#include "NativeMenus.h"
#include <chrono>
#include <optional>

using namespace CapyUi;
using Windows::Foundation::Point;
using Windows::Foundation::Rect;
namespace {
A point(Point p){A result;result.Append(N(p.X));result.Append(N(p.Y));return result;}
J rect(Rect r){return O({{L"x",N(r.X)},{L"y",N(r.Y)},{L"width",N(r.Width)},{L"height",N(r.Height)}});}
}
struct WorkspaceGestures::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr};
    Border hint;
    MenuFlyout menu{nullptr};
    std::vector<weak_ref<FrameworkElement>> tabs;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::optional<uint32_t> pointer;
    J action;
    Point origin{},position{};
    bool dragging=false,finishing=false,busy=false,dirty=false,releasing=false;
    uint64_t generation=0,motion=0;
    double slop=6;
    ~Impl(){if(timer)timer.Stop();if(menu)menu.Hide();}
    A viewport()const{return point({float(root.ActualWidth()),float(root.ActualHeight())});}
    void chrome(J const& event){
        if(!data->input)return;
        auto facts=J::Parse(data->chrome.Stringify());
        facts.Insert(L"held",B(false));facts.Insert(L"dragging",B(dragging&&str(action,L"type")==L"tile_drag"));
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
    // DEPRECATED workspace publication path. Migrate capy_snapshot to
    // NativeHost::take_update_bytes and handle workspace_update before the
    // camera-only branch. Retain models by model_revision, applying absolute
    // geometry to native placement; preserve all DragWorkspace input phases.
    void send(hstring phase){
        if(str(action,L"type")==L"tile_drag")return;
        auto next=J::Parse(action.Stringify());
        next.Insert(L"phase",S(phase));next.Insert(L"position",point(position));next.Insert(L"viewport",viewport());
        if(str(action,L"type")==L"drag_workspace")next.Insert(L"tabs",tabHits());
        data->dispatch(next);
    }
    void clear(){
        ++generation;action=J{};pointer.reset();dragging=false;finishing=false;dirty=false;
        hint.Visibility(Visibility::Collapsed);timer.Stop();
        releasing=true;root.ReleasePointerCaptures();releasing=false;
        chrome(O({{L"kind",S(L"refresh")}}));
    }
    bool cancel(){
        if(!pointer&&!dragging)return false;
        if(dragging)send(L"cancel");
        clear();return true;
    }
    void dropQuery(){
        if(!dragging||busy||!dirty||!object(action,L"item").Size())return;
        auto request=O({{L"type",S(L"drop")},{L"item",object(action,L"item")},{L"position",point(position)},
            {L"tabs",tabHits()},{L"expansion",data->chrome.GetNamedValue(L"expanded_panel",JsonValue::CreateNullValue())}});
        auto serial=generation,at=motion;bool final=finishing;busy=true;
        if(!QueryWorkspace(data->query,request,[weak=weak_from_this(),serial,at,final](J reply){
            if(auto self=weak.lock()){
                self->busy=false;
                if(serial!=self->generation)return;
                if(at!=self->motion){self->dropQuery();return;}
                auto result=object(reply,L"result");
                if(final){
                    auto next=object(result,L"action");
                    if(next.Size())self->data->dispatch(next);
                    self->clear();return;
                }
                auto bounds=object(result,L"bounds");
                if(bounds.Size()){
                    place(self->hint,bounds);self->hint.Visibility(Visibility::Visible);self->refresh();
                }else self->hint.Visibility(Visibility::Collapsed);
            }
        }))busy=false;else dirty=false;
    }
    void begin(PointerRoutedEventArgs const& e){
        // Capture remains with this Canvas while source tabs are rebuilt/moved.
        if(!root.CapturePointer(e.Pointer())){clear();return;}
        dragging=true;++generation;position=origin;timer.Start();
        chrome(O({{L"kind",S(L"refresh")}}));send(L"down");
    }
    void down(PointerRoutedEventArgs const& e){
        if(pointer||dragging||data->externalPopup||data->popupCount)return;
        auto p=e.GetCurrentPoint(root);if(!p.IsInContact())return;
        if(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed())return;
        auto tag=target(e.OriginalSource());
        auto tab=object(tag,L"workspace_tab");
        if(tab.Size())data->chrome.Insert(L"contact_tab",S(str(tab,L"panel")));
        chrome(O({{L"kind",S(L"contact")},{L"position",point(p.Position())},{L"canvas",B(false)}}));
        if(!tag.Size()||flag(data->model,L"partial_zen"))return;
        action=object(tag,L"workspace_action");if(!action.Size())return;
        origin=position=p.Position();pointer=p.PointerId();
        slop=p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch?12:6;
        auto kind=str(action,L"type");
        if(kind!=L"drag_workspace"&&kind!=L"tile_drag"){begin(e);e.Handled(true);}
    }
    void move(PointerRoutedEventArgs const& e){
        if(!pointer||*pointer!=e.Pointer().PointerId()||finishing)return;
        auto next=e.GetCurrentPoint(root).Position();
        if(!dragging&&std::hypot(next.X-origin.X,next.Y-origin.Y)>slop)begin(e);
        if(!dragging)return;
        position=next;++motion;dirty=true;send(L"move");
        if(str(action,L"type")==L"tile_drag")chrome(O({{L"kind",S(L"motion")},{L"position",point(position)}}));
        dropQuery();e.Handled(true);
    }
    void up(PointerRoutedEventArgs const& e){
        if(!pointer||*pointer!=e.Pointer().PointerId())return;
        if(!dragging){pointer.reset();action=J{};return;}
        position=e.GetCurrentPoint(root).Position();++motion;
        if(str(action,L"type")==L"tile_drag"){
            finishing=true;dirty=true;dropQuery();
            releasing=true;root.ReleasePointerCaptures();releasing=false;pointer.reset();
        }else{send(L"up");clear();}
        e.Handled(true);
    }
    void context(Windows::Foundation::IInspectable const& original,Point at){
        if(dragging||data->externalPopup||data->popupCount)return;
        auto value=object(target(original),L"workspace_context");if(!value.Size())return;
        auto serial=++generation;
        QueryWorkspace(data->query,O({{L"type",S(L"context")},{L"target",value}}),
            [weak=weak_from_this(),serial,at](J reply){
                if(auto self=weak.lock()){
                    if(serial!=self->generation||self->dragging||self->data->externalPopup)return;
                    auto model=object(reply,L"result");if(!array(model,L"sections").Size())return;
                    self->pointer.reset();self->action=J{};
                    self->menu=MenuFlyout();TrackPopup(self->menu,self->data);
                    NativeMenuItems(self->menu.Items(),array(model,L"sections"),self->data,
                        [data=self->data](J action){data->dispatch(action);});
                    Primitives::FlyoutShowOptions options;options.Position(at);self->menu.ShowAt(self->root,options);
                }
            });
    }
    void doubleClick(DoubleTappedRoutedEventArgs const& e){
        if(flag(data->model,L"partial_zen"))return;
        auto tag=target(e.OriginalSource());if(!flag(tag,L"workspace_double"))return;
        auto source=object(tag,L"workspace_action");
        if(str(source,L"type")==L"drag_divider"){
            cancel();
            data->dispatch(O({{L"type",S(L"reset_column_width")},{L"id",N(num(source,L"id"))},{L"viewport",viewport()}}));
            e.Handled(true);return;
        }
        auto item=object(source,L"item");if(!item.Size())return;
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
    void refresh(){
        uint32_t index;if(!root.Children().IndexOf(hint,index))root.Children().Append(hint);
        hint.Background(selected());hint.BorderBrush(data->brush(L"text"));hint.BorderThickness({1});
    }
    void init(){
        hint.IsHitTestVisible(false);hint.Visibility(Visibility::Collapsed);Canvas::SetZIndex(hint,10000);
        timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(16));
        timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->dropQuery();});
        auto weak=weak_from_this();
        root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->down(e);})),true);
        root.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->move(e);})),true);
        root.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){if(auto self=weak.lock())self->up(e);})),true);
        root.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId())self->cancel();
        })),true);
        root.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock();self&&!self->releasing&&self->dragging&&e.OriginalSource().try_as<UIElement>()==self->root)self->cancel();
        })),true);
        // ContextRequested covers mouse, pen/touch hold and keyboard requests.
        // Native text editors handle their own event before it reaches us.
        root.ContextRequested([weak](auto&&,ContextRequestedEventArgs const& e){
            if(auto self=weak.lock();self&&object(self->target(e.OriginalSource()),L"workspace_context").Size()){
                Point at{};
                if(!e.TryGetPosition(self->root,at)){
                    auto focused=FocusManager::GetFocusedElement(self->root.XamlRoot()).try_as<FrameworkElement>();
                    if(focused){auto bounds=visibleBounds(focused,self->root);at={bounds.X,bounds.Y+bounds.Height};}
                }
                self->context(e.OriginalSource(),at);e.Handled(true);
            }
        });
        root.AddHandler(UIElement::DoubleTappedEvent(),box_value(DoubleTappedEventHandler([weak](auto&&,auto&& e){
            if(auto self=weak.lock())self->doubleClick(e);
        })),true);
        root.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->chrome(O({{L"kind",S(L"refresh")}}));});
        root.ContextCanceled([weak](auto&&,auto&&){if(auto self=weak.lock()){++self->generation;if(self->menu)self->menu.Hide();}});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
    }
};
WorkspaceGestures::WorkspaceGestures(std::shared_ptr<WorkspaceData> data,Canvas root):impl(std::make_shared<Impl>()){
    impl->data=std::move(data);impl->root=root;impl->init();
}
WorkspaceGestures::~WorkspaceGestures()=default;
void WorkspaceGestures::Source(FrameworkElement const& element,J const& action,J const& context,bool doubleClick,J const& tab){
    element.Tag(O({{L"workspace_action",action},{L"workspace_context",context},{L"workspace_double",B(doubleClick)},{L"workspace_tab",tab}}));
    if(tab.Size())impl->tabs.emplace_back(make_weak(element));
}
void WorkspaceGestures::Refresh(){impl->refresh();}
void WorkspaceGestures::ChromeChanged(){impl->chrome(O({{L"kind",S(L"refresh")}}));}
bool WorkspaceGestures::Cancel(){return impl->cancel();}
