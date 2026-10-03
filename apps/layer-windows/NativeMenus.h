#pragma once
#include "UiControls.h"
#include "WorkspaceQuery.h"
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
inline void NativeMenuItems(Windows::Foundation::Collections::IVector<MenuFlyoutItemBase> const& target,
    A const& sections,std::shared_ptr<WorkspaceData> const& data,std::function<void(J)> const& dispatch){
    uint32_t at=0;
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
                item.FontSize(data->textSize());NativeMenuItems(item.Items(),children,data,dispatch);
            }else{
                auto identifier=action.Size()?str(action,L"command",L"layer-menu-"+str(object(action,L"action"),L"op")):menuSlug(text);
                if(str(action,L"type")==L"workspace_manager"){
                    auto command=object(action,L"command");auto kind=str(command,L"type");
                    identifier=kind==L"switch"?L"workspace-switch-"+str(command,L"id"):
                        kind==L"show_in_switcher"?L"workspace-switcher-show-"+str(command,L"id"):menuSlug(text);
                }
                if(str(action,L"type")==L"choose_tool_variant")identifier=menuSlug(text);
                auto checked=spec.GetNamedValue(L"selected",JsonValue::CreateNullValue());
                auto add=[&](auto fresh){
                    auto [item,added]=retain(fresh);
                    item.Text(text);item.IsEnabled(flag(spec,L"enabled",true));item.FontSize(data->textSize());item.MinHeight(34);
                    item.KeyboardAcceleratorTextOverride(str(spec,L"hint"));AutomationProperties::SetAutomationId(item,identifier);
                    if(auto name=str(spec,L"icon");!name.empty()){
                        auto glyph=item.Icon().template try_as<ImageIcon>();if(!glyph){glyph=ImageIcon();item.Icon(glyph);}
                        glyph.Source(icon(name,data->theme()).Source());
                    }else item.Icon(nullptr);
                    item.Tag(action);
                    if(added)item.Click([dispatch](auto const& sender,auto&&){
                        auto action=sender.template as<FrameworkElement>().Tag().template as<J>();if(action.Size())dispatch(action);
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
inline void TrackPopup(Primitives::FlyoutBase const& popup,std::shared_ptr<WorkspaceData> const& data){
    auto open=std::make_shared<bool>(false);
    popup.Opened([data,open](auto&&,auto&&){if(!std::exchange(*open,true))data->popup(true);});
    popup.Closed([data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
}
inline Button ToolVariantsButton(std::shared_ptr<WorkspaceData> const& data,J const& anchor,hstring const& identifier){
    auto popup=std::make_shared<MenuFlyout>(nullptr);
    auto pick=button(data,L"",[]{});pick.Width(16);pick.Height(16);pick.Padding({0});pick.Background(clear());
    pick.HorizontalAlignment(HorizontalAlignment::Right);pick.VerticalAlignment(VerticalAlignment::Bottom);
    pick.Content(icon(L"tool-group",data->theme(),16));AutomationProperties::SetAutomationId(pick,identifier);
    pick.Click([data,anchor,popup,owner=make_weak(pick)](auto&&,auto&&){
        QueryWorkspace(data->query,O({{L"type",S(L"context")},{L"target",O({{L"kind",S(L"tool_variants")},{L"anchor",anchor}})}}),
            [data,popup,owner](J reply){
                auto target=owner.get();auto model=object(reply,L"result");
                if(!target||!target.IsLoaded()||target.Visibility()!=Visibility::Visible||!array(model,L"sections").Size())return;
                if(*popup)(*popup).Hide();*popup=MenuFlyout();TrackPopup(*popup,data);
                NativeMenuItems((*popup).Items(),array(model,L"sections"),data,[data](J action){data->dispatch(action);});
                (*popup).ShowAt(target);
            });
    });
    return pick;
}

}
