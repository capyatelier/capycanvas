#pragma once
#include "UiControls.h"

namespace CapyUi {
inline Grid NavigationButtons(std::shared_ptr<WorkspaceData> const& data,hstring const& prefix,std::function<A()> states,Bindings& bindings){
    Grid row;row.ColumnSpacing(2);
    auto ids=array(data->catalog,L"navigator_commands");
    for(uint32_t i=0;i<ids.Size();++i){
        ColumnDefinition column;column.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(column);
        auto id=ids.GetStringAt(i);
        auto pick=button(data,L"",[data,id]{data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(id)}}));});
        pick.Height(32);pick.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetAutomationId(pick,prefix+L"-"+id);Grid::SetColumn(pick,int(i));row.Children().Append(pick);
        auto shown=std::make_shared<hstring>();
        bindings.emplace_back([data,states,id,pick,shown]{
            auto command=find(states(),L"id",id);if(!command.Size())return;
            if(auto name=str(command,L"icon");name!=*shown){*shown=name;pick.Content(icon(name,data->theme()));}
            AutomationProperties::SetName(pick,str(command,L"label"));tooltip(pick,str(command,L"tooltip",str(command,L"label")));
            pick.IsEnabled(flag(command,L"enabled"));pick.Background(flag(command,L"selected")?selected(data):clear());
            AutomationProperties::SetItemStatus(pick,flag(command,L"selected")?data->caption(L"search",L"selected"):hstring());
        });
    }
    return row;
}
}
