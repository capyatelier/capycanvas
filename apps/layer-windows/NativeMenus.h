#pragma once
#include "UiControls.h"
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
    bool populated=false;
    for(auto sectionValue:sections){
        auto section=sectionValue.GetArray();if(!section.Size())continue;
        if(populated)target.Append(MenuFlyoutSeparator());populated=true;
        for(auto value:section){
            auto spec=value.GetObject();auto children=array(spec,L"sections");
            auto text=str(spec,L"label");auto action=object(spec,L"action");
            if(children.Size()){
                MenuFlyoutSubItem item;item.Text(text);item.IsEnabled(flag(spec,L"enabled",true));AutomationProperties::SetAutomationId(item,menuSlug(text));
                item.FontSize(data->textSize());NativeMenuItems(item.Items(),children,data,dispatch);target.Append(item);
            }else{
                auto identifier=action.Size()?str(action,L"command",L"layer-menu-"+str(object(action,L"action"),L"op")):menuSlug(text);
                auto checked=spec.GetNamedValue(L"selected",JsonValue::CreateNullValue());
                auto add=[&](auto item){
                    item.Text(text);item.IsEnabled(flag(spec,L"enabled",true));item.FontSize(data->textSize());item.MinHeight(34);
                    item.KeyboardAcceleratorTextOverride(str(spec,L"hint"));AutomationProperties::SetAutomationId(item,identifier);
                    item.Click([dispatch,action](auto&&,auto&&){if(action.Size())dispatch(action);});target.Append(item);
                };
                if(checked.ValueType()==JsonValueType::Boolean){
                    ToggleMenuFlyoutItem item;item.IsChecked(checked.GetBoolean());add(item);
                }else add(MenuFlyoutItem());
            }
        }
    }
}
inline void TrackPopup(Primitives::FlyoutBase const& popup,std::shared_ptr<WorkspaceData> const& data){
    auto open=std::make_shared<bool>(false);
    popup.Opened([data,open](auto&&,auto&&){if(!std::exchange(*open,true))data->popup(true);});
    popup.Closed([data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
}
}
