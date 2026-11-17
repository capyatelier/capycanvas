#pragma once
#include "UiControls.h"

namespace CapyUi {
struct SmokeCommandControls{TextBox arguments;Button command;};
inline SmokeCommandControls smokeCommand(Panel const& owner,hstring const& id,hstring const& label,std::function<void(TextBox const&)> action){
    Canvas host;host.Width(0);host.Height(0);host.IsHitTestVisible(false);
    RectangleGeometry clip;clip.Rect({0,0,0,0});host.Clip(clip);
    TextBox arguments;Button command;
    for(auto control:{arguments.as<Control>(),command.as<Control>()}){
        control.Width(0);control.Height(0);control.MinWidth(0);control.MinHeight(0);
        control.Margin({0,0,0,0});control.Padding({0,0,0,0});
        control.IsTabStop(false);control.AllowFocusOnInteraction(false);
    }
    AutomationProperties::SetAutomationId(arguments,id);command.Content(box_value(label));
    command.Click([arguments,action=std::move(action)](auto&&,auto&&){action(arguments);});
    host.Children().Append(arguments);host.Children().Append(command);owner.Children().Append(host);
    return {arguments,command};
}
}
