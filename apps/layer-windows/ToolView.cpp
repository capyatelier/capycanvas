#include "pch.h"
#include "ToolView.h"

using namespace CapyUi;
namespace {
hstring settingsContext(J const& state){
    A selectedActions;
    auto tools=object(state,L"tool_set");
    for(auto key:{L"groups",L"subtools"})for(auto item:array(tools,key)){
        auto choice=item.GetObject();if(flag(choice,L"selected"))selectedActions.Append(object(choice,L"action"));
    }
    auto target=object(object(state,L"layer_tools"),L"editing_layer");
    return O({{L"tools",selectedActions},{L"preset",N(num(object(state,L"brush"),L"preset"))},
        {L"target",N(num(target,L"id"))},{L"mask",B(flag(target,L"mask_selected"))}}).Stringify();
}
hstring itemSchema(A const& items){
    A keys;for(auto value:items){auto item=value.GetObject();
        keys.Append(O({{L"label",S(str(item,L"label"))},{L"icon",S(str(item,L"icon"))},
            {L"action",object(item,L"action")},{L"preview",item.GetNamedValue(L"preview",JsonValue::CreateNullValue())}}));
    }return keys.Stringify();
}
Grid toolLabel(std::shared_ptr<WorkspaceData> const& data,J const& item){
    Grid row;row.ColumnSpacing(6);
    ColumnDefinition glyph;glyph.Width({16,GridUnitType::Pixel});row.ColumnDefinitions().Append(glyph);
    ColumnDefinition text;text.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(text);
    row.Children().Append(icon(str(item,L"icon"),data->theme()));
    auto title=label(data,str(item,L"label"),true);title.TextTrimming(TextTrimming::CharacterEllipsis);
    title.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(title,1);row.Children().Append(title);return row;
}
struct ToolSetView : std::enable_shared_from_this<ToolSetView> {
    std::shared_ptr<WorkspaceData> data;
    StackPanel root,list;
    Canvas groups;
    std::vector<Button> groupButtons,subtoolButtons;
    hstring groupKey,subtoolKey;
    double arrangedWidth=-1;
    explicit ToolSetView(std::shared_ptr<WorkspaceData> data):data(std::move(data)){}
    void init(){
        root.Spacing(8);list.Spacing(4);
        auto weak=weak_from_this();
        root.Children().Append(groups);root.Children().Append(list);
        groups.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->arrange();});
    }
    void arrange(){
        double width=groups.ActualWidth();
        if(std::abs(arrangedWidth-width)<.01)return;arrangedWidth=width;
        int size=int(groupButtons.size());
        int count=std::max(1,std::min(size,int((width+4)/74.)));
        double height=32+data->textSize()*.85*1.66;
        // Match the shared flex rows: 70 DIP minimum, four-DIP gaps, and equal
        // widths within each row, including a partially filled final row.
        for(int i=0;i<size;i++){
            int row=i/count,items=std::min(count,size-row*count);
            double itemWidth=std::max(0.,(width-4*(items-1))/items);
            auto const& pick=groupButtons[i];
            Canvas::SetLeft(pick,(i%count)*(itemWidth+4));Canvas::SetTop(pick,row*(height+4));
            pick.Width(itemWidth);pick.Height(height);
        }
        int rows=(size+count-1)/count;
        groups.Height(rows?rows*height+(rows-1)*4:0);
    }
    void rebuild(A const& items,bool group){
        auto& buttons=group?groupButtons:subtoolButtons;buttons.clear();
        if(group)groups.Children().Clear();else list.Children().Clear();
        auto weak=weak_from_this();
        for(uint32_t i=0;i<items.Size();i++){
            auto item=items.GetObjectAt(i);auto action=object(item,L"action");
            auto pick=button(data,str(item,L"label"),[weak,action]{if(auto self=weak.lock())self->data->dispatch(action);});
            pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            pick.Padding({12,4,12,4});ToolTipService::SetToolTip(pick,box_value(str(item,L"label")));
            AutomationProperties::SetAutomationId(pick,(group?L"tool-group-":L"tool-subtool-")+to_hstring(i));
            auto title=label(data,str(item,L"label"),true);
            title.FontSize(data->textSize()*(group?.85:1.));title.LineHeight(title.FontSize()*1.66);
            title.TextTrimming(TextTrimming::CharacterEllipsis);title.VerticalAlignment(VerticalAlignment::Center);
            if(group){
                StackPanel content;content.Spacing(8);
                auto glyph=icon(str(item,L"icon"),data->theme());glyph.HorizontalAlignment(HorizontalAlignment::Center);
                title.TextAlignment(TextAlignment::Center);content.Children().Append(glyph);content.Children().Append(title);
                pick.Content(content);
            }else{
                Grid content;content.ColumnSpacing(8);
                auto preview=item.GetNamedValue(L"preview",JsonValue::CreateNullValue());
                bool brush=preview.ValueType()==JsonValueType::Number;
                ColumnDefinition glyph;glyph.Width({brush?82.:16.,GridUnitType::Pixel});content.ColumnDefinitions().Append(glyph);
                ColumnDefinition text;text.Width({1,GridUnitType::Star});content.ColumnDefinitions().Append(text);
                if(brush){
                    Image image;image.Width(82);image.Height(32);image.Stretch(Stretch::Uniform);
                    image.Source(Imaging::BitmapImage(asset(L"brush-previews/"+std::to_wstring(int(preview.GetNumber()))+L"-"+std::wstring(data->theme().c_str())+L".png")));
                    content.Children().Append(image);
                }else content.Children().Append(icon(str(item,L"icon"),data->theme()));
                Grid::SetColumn(title,1);content.Children().Append(title);pick.Content(content);
                pick.Height(8+std::max(brush?32.:16.,title.LineHeight()));
            }
            buttons.push_back(pick);if(group)groups.Children().Append(pick);else list.Children().Append(pick);
        }
        if(group){arrangedWidth=-1;arrange();}
    }
    void refresh(){
        auto view=object(data->state,L"tool_set");
        for(bool group:{true,false}){
            auto items=array(view,group?L"groups":L"subtools");auto key=itemSchema(items);
            auto& previous=group?groupKey:subtoolKey;
            if(previous!=key){previous=key;rebuild(items,group);}
            auto const& buttons=group?groupButtons:subtoolButtons;
            for(uint32_t i=0;i<items.Size();i++){
                bool active=flag(items.GetObjectAt(i),L"selected");buttons[i].Background(active?selected():clear());
                AutomationProperties::SetItemStatus(buttons[i],active?L"Selected":L"");
            }
        }

    }
};
struct SettingsView : std::enable_shared_from_this<SettingsView> {
    std::shared_ptr<WorkspaceData> data;
    StackPanel root;
    Bindings fields;
    hstring key;
    explicit SettingsView(std::shared_ptr<WorkspaceData> data):data(std::move(data)){root.Spacing(6);}
    void refresh(){
        auto context=settingsContext(data->state);A schema;
        for(auto value:array(data->state,L"tool_settings")){
            auto item=value.GetObject();schema.Append(O({{L"id",S(str(item,L"id"))},{L"label",S(str(item,L"label"))},
                {L"group",S(str(item,L"group"))},{L"numeric",object(item,L"numeric")}}));
        }
        auto actions=array(data->state,L"tool_actions");
        auto next=O({{L"context",S(context)},{L"fields",schema},{L"actions",actions}}).Stringify();
        if(next!=key){
            key=next;fields.clear();root.Children().Clear();hstring group;
            auto weak=weak_from_this();
            for(auto value:array(data->state,L"tool_settings")){
                auto item=value.GetObject();auto id=str(item,L"id");
                if(group!=str(item,L"group")){
                    group=str(item,L"group");if(!group.empty()){
                        auto heading=label(data,group,true);heading.Opacity(.55);heading.Margin({0,6,0,0});root.Children().Append(heading);
                    }
                }
                auto control=number(data,str(item,L"label"),object(item,L"numeric"),
                    [weak,id]{if(auto self=weak.lock())return num(find(array(self->data->state,L"tool_settings"),L"id",id),L"value");return 0.;},
                    [weak,id,context](double value){if(auto self=weak.lock();self&&settingsContext(self->data->state)==context)
                        self->data->dispatch(O({{L"type",S(L"set_tool_setting")},{L"id",S(id)},{L"value",N(value)}}));},fields,nullptr,false,L"tool-setting-"+id);
                AutomationProperties::SetAutomationId(control,L"tool-setting-"+id);root.Children().Append(control);
            }
            for(auto value:actions){
                auto item=value.GetObject();auto id=str(item,L"command");auto command=find(array(data->state,L"commands"),L"id",id);
                auto invoke=[weak,id,context]{if(auto self=weak.lock();self&&settingsContext(self->data->state)==context)
                    self->data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(id)}}));};
                if(flag(item,L"checkable")){
                    CheckBox check;auto text=label(data,str(command,L"label"));text.TextTrimming(TextTrimming::CharacterEllipsis);text.Margin({6,0,0,0});check.Content(text);
                    check.MinWidth(0);check.MinHeight(32);check.Padding({0,0,0,0});check.HorizontalAlignment(HorizontalAlignment::Stretch);
                    AutomationProperties::SetName(check,str(command,L"label"));AutomationProperties::SetAutomationId(check,L"tool-action-"+id);
                    check.Click([invoke](auto&&,auto&&){invoke();});root.Children().Append(check);
                    fields.emplace_back([weak,id,check]{if(auto self=weak.lock()){
                        auto command=find(array(self->data->state,L"commands"),L"id",id);
                        check.IsEnabled(flag(command,L"enabled"));check.IsChecked(flag(command,L"selected"));
                        ToolTipService::SetToolTip(check,box_value(str(command,L"tooltip")));
                    }});
                }else{
                    auto pick=button(data,str(command,L"label"),invoke);pick.Height(36);pick.HorizontalAlignment(HorizontalAlignment::Stretch);
                    pick.Content(toolLabel(data,command));AutomationProperties::SetAutomationId(pick,L"tool-action-"+id);root.Children().Append(pick);
                    fields.emplace_back([weak,id,pick]{if(auto self=weak.lock()){
                        auto command=find(array(self->data->state,L"commands"),L"id",id);pick.IsEnabled(flag(command,L"enabled"));
                        ToolTipService::SetToolTip(pick,box_value(str(command,L"tooltip")));
                    }});
                }
            }
        }
        for(auto const& bind:fields)bind();
    }
};
}
FrameworkElement ToolSetPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<ToolSetView>(data);view->init();bindings.emplace_back([view]{view->refresh();});return view->root;
}
FrameworkElement ToolSettingsPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<SettingsView>(data);bindings.emplace_back([view]{view->refresh();});return view->root;
}
