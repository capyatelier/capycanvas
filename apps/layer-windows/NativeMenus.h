#pragma once
#include "UiControls.h"
#include "WorkspaceQuery.h"
#include "WorkspaceGeometry.h"
#include <winrt/Microsoft.UI.Content.h>
#include <winrt/Microsoft.UI.Interop.h>
#include <cwctype>

namespace CapyUi {
inline hstring menuSlug(hstring const& text){
    std::wstring slug;
    for(auto ch:text){
        if(std::iswalnum(ch))slug+=wchar_t(std::towlower(ch));
        else if(!slug.empty()&&slug.back()!=L'-')slug+=L'-';
    }
    while(!slug.empty()&&slug.back()==L'-')slug.pop_back();
    return hstring(L"menu-"+slug);
}
inline void NativeMenuItems(winrt::Windows::Foundation::Collections::IVector<MenuFlyoutItemBase> const& target,
    A const& sections,std::shared_ptr<WorkspaceData> const& data,std::function<void(J)> const& dispatch,std::function<void(bool)> const& keepOpen={}){
    uint32_t at=0;
    auto updateIcon=[&](auto const& item,J const& spec){
        if(auto name=str(spec,L"icon");!name.empty()){
            auto glyph=item.Icon().template try_as<ImageIcon>();if(!glyph){glyph=ImageIcon();item.Icon(glyph);}
            glyph.Source(icon(name,data->theme()).Source());
        }else item.Icon(nullptr);
    };
    auto retain=[&](auto fresh){
        using T=decltype(fresh);T item{nullptr};
        if(at<target.Size())item=target.GetAt(at).try_as<T>();
        bool added=!item;
        if(added){if(at<target.Size())target.RemoveAt(at);item=fresh;target.InsertAt(at,item);}
        ++at;return std::pair{item,added};
    };
    bool populated=false;
    for(auto sectionValue:sections){
        auto section=sectionValue.GetArray();if(!section.Size())continue;
        if(populated)retain(MenuFlyoutSeparator());populated=true;
        for(auto value:section){
            auto spec=value.GetObject();auto children=array(spec,L"sections");
            auto text=str(spec,L"label");auto action=object(spec,L"action");
            if(children.Size()){
                auto item=retain(MenuFlyoutSubItem()).first;item.Text(text);item.IsEnabled(flag(spec,L"enabled",true));AutomationProperties::SetAutomationId(item,menuSlug(text));
                updateIcon(item,spec);
                item.FontFamily(FontFamily(L"Segoe UI"));item.FontSize(data->textSize());NativeMenuItems(item.Items(),children,data,dispatch,keepOpen);
            }else{
                auto identifier=action.Size()?str(action,L"command",L"layer-menu-"+str(object(action,L"action"),L"op")):menuSlug(text);
                if(str(action,L"type")==L"workspace_manager"){
                    auto command=object(action,L"command");auto kind=str(command,L"type");
                    identifier=kind==L"switch"?L"workspace-switch-"+str(command,L"id"):
                        kind==L"show_in_switcher"?L"workspace-switcher-show-"+str(command,L"id"):menuSlug(text);
                }
                if(str(action,L"type")==L"choose_tool_variant"||str(action,L"type")==L"bubble")identifier=menuSlug(text);
                if(str(action,L"type")==L"color_slot_menu")identifier=flag(action,L"library")?L"color-swatch-palettes":L"color-swatch-edit";
                if(str(action,L"type")==L"color"&&str(object(action,L"action"),L"op")==L"swap")identifier=L"color-swatch-swap";
                auto checked=spec.GetNamedValue(L"selected",JsonValue::CreateNullValue());
                auto add=[&](auto fresh){
                    auto [item,added]=retain(fresh);
                    auto heading=flag(spec,L"is_heading");
                    item.Text(text);item.IsEnabled(!heading&&flag(spec,L"enabled",true));item.IsTabStop(!heading);item.FontSize(data->textSize());item.MinHeight(data->controlHeight()+data->panelMetric(L"control_spacing"));item.FontFamily(FontFamily(L"Segoe UI"));
                    item.FontWeight(heading?winrt::Windows::UI::Text::FontWeights::SemiBold():Windows::UI::Text::FontWeights::Normal());
                    item.KeyboardAcceleratorTextOverride(str(spec,L"hint"));AutomationProperties::SetAutomationId(item,identifier);
                    updateIcon(item,spec);
                    item.Tag(spec);
                    if(added)item.Click([dispatch,keepOpen](auto const& sender,auto&&){
                        auto spec=sender.template as<FrameworkElement>().Tag().template as<J>();auto action=object(spec,L"action");
                        if(keepOpen)keepOpen(flag(spec,L"keep_open"));if(action.Size())dispatch(action);
                    });
                    return item;
                };
                if(checked.ValueType()==JsonValueType::Boolean){
                    auto item=add(ToggleMenuFlyoutItem());item.IsChecked(checked.GetBoolean());
                }else add(MenuFlyoutItem());
            }
        }
    }
    while(target.Size()>at)target.RemoveAtEnd();
}
struct PopupLifetime:std::enable_shared_from_this<PopupLifetime>{
    std::weak_ptr<WorkspaceData> data;
    weak_ref<Primitives::FlyoutBase> popup;
    uint64_t registration=0,generation=0;
    bool open=false,closing=false;
    event_token opening{},opened{},closingEvent{},closed{};
    virtual ~PopupLifetime()=default;
    void attach(Primitives::FlyoutBase const& flyout,std::shared_ptr<WorkspaceData> const& context){
        popup=make_weak(flyout);data=context;auto self=shared_from_this();
        opening=flyout.Opening([self](auto&&,auto&&){++self->generation;self->closing=false;self->enroll();});
        opened=flyout.Opened([self](auto&&,auto&&){self->didOpen();});
        closingEvent=flyout.Closing([self](auto const& sender,auto const& args){
            sender.DispatcherQueue().TryEnqueue([self,args,serial=self->generation]{
                if(serial!=self->generation||args.Cancel()||!self->registration)return;
                self->closing=true;self->uncount();
            });
        });
        closed=flyout.Closed([self](auto&&,auto&&){self->retire();});
    }
    void enroll(){if(!registration)if(auto context=data.lock())registration=context->transient([self=shared_from_this()]{return self->cancel();});}
    virtual void didOpen(){if(!registration||closing){if(auto flyout=popup.get())flyout.Hide();return;}if(!std::exchange(open,true))if(auto context=data.lock())context->popup(true);}
    void uncount(){if(std::exchange(open,false))if(auto context=data.lock())context->popup(false);}
    virtual void retire(){
        ++generation;closing=false;auto id=std::exchange(registration,0);
        if(auto context=data.lock())context->transients.erase(id);uncount();
    }
    bool cancel(){if(!registration)return false;retire();if(auto flyout=popup.get())flyout.Hide();return true;}
};
inline void TrackPopup(Primitives::FlyoutBase const& popup,std::shared_ptr<WorkspaceData> const& data){
    std::make_shared<PopupLifetime>()->attach(popup,data);
}
class OwnedPopup {
    struct State:PopupLifetime{
        weak_ref<FrameworkElement> anchor,root;
        HWND window=nullptr;
        winrt::Windows::Foundation::IInspectable press{nullptr};
        event_token unloaded{};
        bool current(){
            if(!registration||closing)return false;
            auto source=anchor.get(),surface=root.get();auto context=data.lock();
            bool valid=source&&surface&&context&&!context->externalPopup&&source.XamlRoot()&&source.XamlRoot().Content()==surface&&
                window&&GetAncestor(GetForegroundWindow(),GA_ROOTOWNER)==window&&!IsIconic(window);
            if(valid){auto bounds=visibleBounds(source,surface);valid=bounds.Width>0&&bounds.Height>0;}
            if(valid)for(DependencyObject node=source;node;node=VisualTreeHelper::GetParent(node)){
                if(auto element=node.try_as<UIElement>();element&&(!element.IsHitTestVisible()||element.Opacity()<=0))valid=false;
                if(auto control=node.try_as<Control>();control&&!control.IsEnabled())valid=false;
                if(node==surface)break;
            }
            if(!valid)cancel();return valid;
        }
        void stopPress(){if(auto surface=root.get();surface&&press)surface.RemoveHandler(UIElement::PointerPressedEvent(),press);press=nullptr;}
        void didOpen()override{if(current())stopPress();PopupLifetime::didOpen();}
        void retire()override{
            stopPress();if(auto source=anchor.get();source&&unloaded.value)source.Unloaded(std::exchange(unloaded,{}));
            if(auto flyout=popup.get()){
                if(opening.value)flyout.Opening(std::exchange(opening,{}));
                if(opened.value)flyout.Opened(std::exchange(opened,{}));
                if(closingEvent.value)flyout.Closing(std::exchange(closingEvent,{}));
                if(closed.value)flyout.Closed(std::exchange(closed,{}));
            }
            PopupLifetime::retire();
        }
    };
    std::shared_ptr<State> state;
public:
    struct Ticket{
        std::shared_ptr<State> state;
        bool Current()const{return state&&state->current();}
        bool Cancel()const{return state&&state->cancel();}
        bool Show(FrameworkElement const& anchor,Primitives::FlyoutShowOptions const& options)const{
            if(!Current())return false;
            auto popup=state->popup.get();
            if(!popup||!anchor||!anchor.XamlRoot()||anchor.XamlRoot().Content()!=state->root.get()){Cancel();return false;}
            popup.ShowAt(anchor,options);return true;
        }
    };
    OwnedPopup()=default;
    OwnedPopup(OwnedPopup const&)=delete;
    OwnedPopup& operator=(OwnedPopup const&)=delete;
    ~OwnedPopup(){Cancel();}
    bool Cancel(){return state&&state->cancel();}
    bool Active()const{return state&&state->registration;}
    bool Pending()const{return Active()&&!state->open&&!state->closing;}
    bool Open()const{return Active()&&state->open;}
    Ticket Begin(std::shared_ptr<WorkspaceData> const& data,FrameworkElement const& anchor,Primitives::FlyoutBase const& popup){
        Cancel();state=std::make_shared<State>();auto next=state;
        next->anchor=make_weak(anchor);
        if(auto xaml=anchor.XamlRoot()){
            next->root=make_weak(xaml.Content().as<FrameworkElement>());
            next->window=GetAncestor(Microsoft::UI::GetWindowFromWindowId(xaml.ContentIslandEnvironment().AppWindowId()),GA_ROOTOWNER);
        }
        next->attach(popup,data);next->enroll();auto weak=std::weak_ptr<State>(next);
        next->unloaded=anchor.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        anchor.DispatcherQueue().TryEnqueue([weak]{if(auto self=weak.lock();self&&self->registration&&!self->open){
            if(auto surface=self->root.get()){
                self->press=box_value(PointerEventHandler([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();}));
                surface.AddHandler(UIElement::PointerPressedEvent(),self->press,true);
            }
        }});
        return {next};
    }
};
inline Image ToolGroupMarker(std::shared_ptr<WorkspaceData> const& data){
    auto marker=icon(L"tool-group",data->theme(),16);
    marker.HorizontalAlignment(HorizontalAlignment::Right);marker.VerticalAlignment(VerticalAlignment::Bottom);
    return marker;
}

}
