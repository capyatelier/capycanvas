#include "pch.h"
#include "ToolView.h"
#include "RangeControl.h"
#include "NativeMenus.h"

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
Grid toolLabel(std::shared_ptr<WorkspaceData> const& data,J const& item,bool bold=true,bool trailingName=false){
    Grid row;row.ColumnSpacing(6);
    ColumnDefinition glyph;glyph.Width({16,GridUnitType::Pixel});row.ColumnDefinitions().Append(glyph);
    ColumnDefinition text;text.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(text);
    row.Children().Append(icon(str(item,L"icon"),data->theme()));
    auto title=label(data,str(item,L"label"),bold);title.TextTrimming(TextTrimming::CharacterEllipsis);
    title.VerticalAlignment(VerticalAlignment::Center);
    if(trailingName){title.TextAlignment(TextAlignment::Right);title.LineHeight(18);}
    Grid::SetColumn(title,1);row.Children().Append(title);return row;
}
struct ToolSetView : std::enable_shared_from_this<ToolSetView> {
    std::shared_ptr<WorkspaceData> data;
    StackPanel root,list;
    Canvas groups;
    std::vector<Button> groupButtons,subtoolButtons;
    hstring groupKey,subtoolKey;
    hstring panel;
    bool media()const{return panel==L"brush_sets"||panel==L"sculpt_sets";}
    double arrangedWidth=-1;
    ToolSetView(std::shared_ptr<WorkspaceData> data,hstring panel):data(std::move(data)),panel(std::move(panel)){}
    void init(){
        root.Spacing(6);list.Spacing(2);
        auto weak=weak_from_this();
        root.Children().Append(groups);root.Children().Append(list);
        groups.Visibility(panel==L"tools"?Visibility::Collapsed:Visibility::Visible);
        list.Visibility(media()?Visibility::Collapsed:Visibility::Visible);
        groups.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->arrange();});
    }
    void arrange(){
        double width=groups.ActualWidth();
        if(std::abs(arrangedWidth-width)<.01)return;arrangedWidth=width;
        int size=int(groupButtons.size());
        constexpr double tile=3*36+2*2;
        double gap=2,height=media()?44:36;
        int count=media()?1:std::max(1,std::min(size,int((width+gap)/(tile+gap))));
        for(int i=0;i<size;i++){
            int row=i/count;
            double itemWidth=media()?width:std::min(tile,width);
            auto const& pick=groupButtons[i];
            Canvas::SetLeft(pick,(i%count)*(itemWidth+gap));Canvas::SetTop(pick,row*(height+gap));
            pick.Width(itemWidth);pick.Height(height);
        }
        int rows=(size+count-1)/count;
        groups.Height(rows?rows*height+(rows-1)*gap:0);
    }
    void rebuild(A const& items,bool group){
        auto& buttons=group?groupButtons:subtoolButtons;buttons.clear();
        if(group)groups.Children().Clear();else list.Children().Clear();
        auto weak=weak_from_this();
        for(uint32_t i=0;i<items.Size();i++){
            auto item=items.GetObjectAt(i);auto action=object(item,L"action");
            auto pick=button(data,str(item,L"label"),[weak,action]{if(auto self=weak.lock())self->data->dispatch(action);});
            pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            pick.Padding({17,5,17,5});tooltip(pick,str(item,L"label"));
            AutomationProperties::SetAutomationId(pick,(group?L"tool-group-":L"tool-subtool-")+to_hstring(i));
            if(group&&media())AutomationProperties::SetAutomationId(pick,(panel==L"sculpt_sets"?L"sculpt-set-":L"brush-set-")+str(item,L"icon"));
            if(group&&media()){
                pick.Padding({6,5,6,5});pick.Content(toolLabel(data,item,false));
            }else if(group){
                pick.Padding({6,0,6,0});pick.Content(toolLabel(data,item,true,true));
            }else{
                // Match Web's full-width stroke over a compact icon/name row.
                // A fixed preview column squeezes names in narrow dock columns.
                pick.Padding({6,3,6,3});pick.MinHeight(34);
                Grid content;
                RowDefinition stroke;stroke.Height({1,GridUnitType::Auto});content.RowDefinitions().Append(stroke);
                RowDefinition labelRow;labelRow.Height({1,GridUnitType::Auto});content.RowDefinitions().Append(labelRow);
                auto preview=item.GetNamedValue(L"preview",JsonValue::CreateNullValue());
                bool brush=preview.ValueType()==JsonValueType::Number;
                if(brush){
                    Image image;image.Height(40);image.Stretch(Stretch::Fill);
                    image.HorizontalAlignment(HorizontalAlignment::Stretch);
                    image.Source(Imaging::BitmapImage(asset(L"brush-previews/"+std::to_wstring(int(preview.GetNumber()))+L"-"+std::wstring(data->theme().c_str())+L".png")));
                    Border frame;frame.CornerRadius({3,3,3,3});frame.Child(image);content.Children().Append(frame);
                }
                auto caption=toolLabel(data,item,true,brush);
                Grid::SetRow(caption,1);content.Children().Append(caption);pick.Content(content);
            }
            buttons.push_back(pick);if(group)groups.Children().Append(pick);else list.Children().Append(pick);
        }
        if(group){arrangedWidth=-1;arrange();}
    }
    void refresh(){
        auto view=panel==L"brushes"?object(data->state,L"tool_set"):object(object(data->state,L"tool_panels"),panel.c_str());
        bool tonal=false;for(auto value:array(data->state,L"tool_extra"))tonal=tonal||str(object(value.GetObject(),L"Choice"),L"id")==L"tonal-tones";
        for(bool group:{true,false}){
            auto items=array(view,group?L"groups":L"subtools");auto key=itemSchema(items)+data->theme();
            auto& previous=group?groupKey:subtoolKey;
            if(previous!=key){previous=key;rebuild(items,group);}
            auto const& buttons=group?groupButtons:subtoolButtons;
            for(uint32_t i=0;i<items.Size();i++){
                bool active=flag(items.GetObjectAt(i),L"selected");buttons[i].Background(active?selected(data):clear());
                if(!group)buttons[i].MinHeight(items.GetObjectAt(i).GetNamedValue(L"preview",JsonValue::CreateNullValue()).ValueType()==JsonValueType::Number?34:tonal?36:44);
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
    std::shared_ptr<RangeControl> range;
    explicit SettingsView(std::shared_ptr<WorkspaceData> data):data(std::move(data)){root.Spacing(6);}
    static bool picking(J const& state){
        auto tool=str(object(state,L"layer_tools"),L"tool");return tool==L"pick_visible"||tool==L"pick_layer";
    }
    void pickerChoice(hstring const& title,hstring const& id,std::vector<hstring> const& names,
        std::function<int(J const&)> current,std::function<void(int)> select){
        Grid row;row.ColumnSpacing(8);row.MinHeight(36);
        ColumnDefinition caption;caption.Width({68,GridUnitType::Pixel});row.ColumnDefinitions().Append(caption);
        ColumnDefinition value;value.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(value);
        auto text=label(data,title);text.FontSize(13);text.VerticalAlignment(VerticalAlignment::Center);
        text.TextWrapping(TextWrapping::NoWrap);row.Children().Append(text);
        ComboBox choices;choices.MinWidth(0);choices.MinHeight(32);choices.FontSize(13);choices.Padding({8,6,0,6});
        choices.HorizontalAlignment(HorizontalAlignment::Stretch);choices.VerticalAlignment(VerticalAlignment::Center);
        choices.Background(data->brush(L"input"));choices.BorderThickness({0,0,0,0});choices.CornerRadius({6,6,6,6});
        for(auto const& name:names)choices.Items().Append(box_value(name));
        AutomationProperties::SetName(choices,title);AutomationProperties::SetAutomationId(choices,id);
        auto syncing=std::make_shared<bool>(false);
        choices.SelectionChanged([select,syncing,weak=make_weak(choices)](auto&&,auto&&){
            if(auto c=weak.get();c&&!*syncing&&c.SelectedIndex()>=0)select(c.SelectedIndex());
        });
        auto open=std::make_shared<bool>(false);
        choices.DropDownOpened([data=data,open](auto&&,auto&&){if(!std::exchange(*open,true))data->popup(true);});
        choices.DropDownClosed([data=data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
        choices.Unloaded([data=data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
        fields.emplace_back([weak=weak_from_this(),choices,syncing,current]{if(auto self=weak.lock()){
            auto index=current(object(self->data->state,L"color_picker"));
            if(choices.SelectedIndex()!=index){*syncing=true;choices.SelectedIndex(index);*syncing=false;}
        }});
        Grid::SetColumn(choices,1);row.Children().Append(choices);root.Children().Append(row);
    }
    void refreshPicker(){
        auto picker=object(data->state,L"color_picker");auto sizes=array(picker,L"sample_sizes");
        bool layers=flag(picker,L"can_sample_layer");
        auto next=O({{L"picker",B(true)},{L"layers",B(layers)},{L"sizes",sizes}}).Stringify();
        if(next!=key){
            key=next;fields.clear();root.Children().Clear();
            auto weak=weak_from_this();
            std::vector<hstring> sources{L"Visible color"};if(layers)sources.push_back(L"Selected layer");
            pickerChoice(L"Source",L"picker-setting-source",sources,
                [](J const& value){return flag(value,L"layer")?1:0;},
                [weak](int index){if(auto self=weak.lock();self&&picking(self->data->state)&&flag(object(self->data->state,L"color_picker"),L"layer")!=(index==1))
                    self->data->dispatch(O({{L"type",S(L"color_picker")},{L"action",O({{L"kind",S(L"source")},{L"layer",B(index==1)}})}}));});
            std::vector<hstring> names;std::vector<double> widths;
            for(auto value:sizes){auto width=value.GetNumber();widths.push_back(width);
                names.push_back(width==1?hstring(L"Single pixel"):to_hstring(int(width))+L" px circle");}
            pickerChoice(L"Sample size",L"picker-setting-size",names,
                [widths](J const& value){auto at=std::find(widths.begin(),widths.end(),num(value,L"sample_width"));return at==widths.end()?-1:int(at-widths.begin());},
                [weak,widths](int index){if(auto self=weak.lock();self&&picking(self->data->state)&&size_t(index)<widths.size()
                    &&num(object(self->data->state,L"color_picker"),L"sample_width")!=widths[index])
                    self->data->dispatch(O({{L"type",S(L"set_color_sample_size")},{L"width",N(widths[index])}}));});
        }
        for(auto const& bind:fields)bind();
    }
    static bool mode(hstring const& id){
        return id==L"selection_new"||id==L"selection_add"||id==L"selection_subtract"||id==L"selection_intersect";
    }
    static bool source(hstring const& id){
        return id==L"selection_visible"||id==L"selection_editing"||id==L"selection_reference";
    }
    static A extraSchema(A const& extra){
        A result;
        for(auto value:extra){
            auto option=J::Parse(value.GetObject().Stringify());
            for(auto item:array(object(option,L"Choice"),L"items"))item.GetObject().SetNamedValue(L"selected",B(false));
            result.Append(option);
        }
        return result;
    }
    Grid segmented(bool compact,size_t count){
        Grid row;row.ColumnSpacing(0);row.HorizontalAlignment(HorizontalAlignment::Stretch);
        for(size_t i=0;i<count;i++){ColumnDefinition column;column.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(column);}
        if(compact){row.Margin({0,0,0,2});return row;}
        return row;
    }
    Button segment(hstring const& iconName,hstring const& name,hstring const& tooltip,hstring const& id,bool compact,size_t index,std::function<void()> action){
        auto pick=button(data,name,std::move(action));pick.Height(compact?36:44);pick.HorizontalAlignment(HorizontalAlignment::Stretch);
        pick.CornerRadius({0,0,0,0});pick.Content(icon(iconName,data->theme(),20));
        pick.Background(compact?data->brush(L"input"):clear());
        if(!compact&&index>0){pick.BorderThickness({1,0,0,0});pick.BorderBrush(data->tint(L"text",51));}
        CapyUi::tooltip(pick,tooltip);AutomationProperties::SetAutomationId(pick,id);
        return pick;
    }
    void selectionActions(){
        MenuFlyout menu;auto weak=weak_from_this();
        menu.Opening([weak](Windows::Foundation::IInspectable const& sender,auto&&){if(auto self=weak.lock()){
            auto menu=sender.as<MenuFlyout>();menu.Items().Clear();
            auto model=object(find(array(self->data->model,L"application_menus"),L"id",L"select"),L"model");
            NativeMenuItems(menu.Items(),array(model,L"sections"),self->data,[data=self->data](J action){data->dispatch(action);});
        }});
        TrackPopup(menu,data);
        Button open=button(data,L"Selection Actions\u2026",[]{});open.MinHeight(44);open.HorizontalAlignment(HorizontalAlignment::Stretch);
        open.HorizontalContentAlignment(HorizontalAlignment::Stretch);open.Padding({12,0,12,0});open.Flyout(menu);
        Grid content;content.ColumnSpacing(8);
        ColumnDefinition text;text.Width({1,GridUnitType::Star});content.ColumnDefinitions().Append(text);
        ColumnDefinition arrow;arrow.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(arrow);
        auto title=label(data,L"Selection Actions\u2026",true);title.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(title);
        auto chevron=icon(L"chevron-down",data->theme(),12);chevron.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(chevron,1);content.Children().Append(chevron);
        open.Content(content);AutomationProperties::SetAutomationId(open,L"selection-actions-menu");
        root.Children().Append(open);
    }
    void refresh(){
        if(picking(data->state))return refreshPicker();
        auto context=settingsContext(data->state);A schema;
        for(auto value:array(data->state,L"tool_settings")){
            auto item=value.GetObject();schema.Append(O({{L"id",S(str(item,L"id"))},{L"label",S(str(item,L"label"))},
                {L"group",S(str(item,L"group"))},{L"numeric",object(item,L"numeric")}}));
        }
        auto actions=array(data->state,L"tool_actions");auto extra=array(data->state,L"tool_extra");
        auto next=O({{L"context",S(context)},{L"fields",schema},{L"actions",actions},{L"extra",extraSchema(extra)}}).Stringify();
        if(next!=key){
            key=next;fields.clear();if(range){range->Dispose();range=nullptr;}root.Children().Clear();hstring group;
            auto weak=weak_from_this();
            bool compact=false;for(auto value:extra)compact=compact||str(object(value.GetObject(),L"Choice"),L"id")==L"tonal-tones";
            root.Spacing(compact?2:6);
            size_t modeCount=0;bool selectionTool=false;
            for(auto value:actions){auto id=str(value.GetObject(),L"command");if(mode(id))modeCount++;selectionTool=selectionTool||mode(id)||source(id);}
            Grid modes{nullptr};
            if(modeCount){
                modes=segmented(compact,modeCount);AutomationProperties::SetAutomationId(modes,L"selection-mode-row");
                AutomationProperties::SetName(modes,L"Selection mode");
                if(compact)root.Children().Append(modes);
                else{Border frame;frame.BorderThickness({1,1,1,1});frame.BorderBrush(data->tint(L"text",51));frame.CornerRadius({6,6,6,6});
                    frame.Child(modes);root.Children().Append(frame);}
            }
            for(auto value:extra){
                auto spec=object(value.GetObject(),L"Choice");auto items=array(spec,L"items");auto specId=str(spec,L"id");
                auto bar=segmented(true,items.Size());AutomationProperties::SetName(bar,str(spec,L"label"));
                AutomationProperties::SetAutomationId(bar,L"tool-choice-"+specId);
                for(uint32_t i=0;i<items.Size();i++){
                    auto item=items.GetObjectAt(i);auto action=object(item,L"action");
                    auto pick=segment(str(item,L"icon"),str(item,L"label"),str(item,L"label"),L"tool-choice-"+specId+L"-"+to_hstring(i),true,i,
                        [weak,action,context]{if(auto self=weak.lock();self&&settingsContext(self->data->state)==context)self->data->dispatch(action);});
                    Grid::SetColumn(pick,int(i));bar.Children().Append(pick);
                    fields.emplace_back([weak,pick,specId,i]{if(auto self=weak.lock()){
                        J current;for(auto option:array(self->data->state,L"tool_extra"))if(str(object(option.GetObject(),L"Choice"),L"id")==specId)current=object(option.GetObject(),L"Choice");
                        auto items=array(current,L"items");bool chosen=i<items.Size()&&flag(items.GetObjectAt(i),L"selected");
                        pick.Background(chosen?selected(self->data):self->data->brush(L"input"));
                        AutomationProperties::SetItemStatus(pick,chosen?L"Selected":L"");
                    }});
                }
                root.Children().Append(bar);
            }
            auto settings=array(data->state,L"tool_settings");
            for(auto value:settings){
                auto item=value.GetObject();auto id=str(item,L"id");
                if(compact&&id==L"tonal_upper")continue;
                if(compact&&id==L"tonal_lower"){
                    auto upper=find(settings,L"id",L"tonal_upper");
                    range=RangeControl::Create(data,item,upper,L"Range in stops relative to reference white (0)",L"tool-setting",true,
                        [weak,context](int index,double value){if(auto self=weak.lock();self&&settingsContext(self->data->state)==context)
                            self->data->dispatch(O({{L"type",S(L"set_tool_setting")},{L"id",S(index?L"tonal_upper":L"tonal_lower")},{L"value",N(value)}}));});
                    root.Children().Append(range->root);
                    fields.emplace_back([weak]{if(auto self=weak.lock();self&&self->range){
                        auto current=array(self->data->state,L"tool_settings");
                        self->range->Update(num(find(current,L"id",L"tonal_lower"),L"value"),num(find(current,L"id",L"tonal_upper"),L"value"));
                    }});
                    continue;
                }
                if(group!=str(item,L"group")){
                    group=str(item,L"group");if(!group.empty()){
                        auto heading=label(data,group,true);heading.Opacity(.55);heading.Margin({0,6,0,0});root.Children().Append(heading);
                    }
                }
                auto control=number(data,str(item,L"label"),object(item,L"numeric"),
                    [weak,id]{if(auto self=weak.lock())return num(find(array(self->data->state,L"tool_settings"),L"id",id),L"value");return 0.;},
                    [weak,id,context](double value){if(auto self=weak.lock();self&&settingsContext(self->data->state)==context)
                        self->data->dispatch(O({{L"type",S(L"set_tool_setting")},{L"id",S(id)},{L"value",N(value)}}));},fields,nullptr,false,L"tool-setting-"+id,compact);
                AutomationProperties::SetAutomationId(control,L"number-root-tool-setting-"+id);
                if(compact){
                    Grid row;row.ColumnSpacing(6);row.Height(28);
                    ColumnDefinition caption;caption.Width({1,GridUnitType::Auto});row.ColumnDefinitions().Append(caption);
                    ColumnDefinition body;body.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(body);
                    auto text=label(data,str(item,L"label"));text.MinWidth(62);text.VerticalAlignment(VerticalAlignment::Center);row.Children().Append(text);
                    control.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(control,1);row.Children().Append(control);root.Children().Append(row);
                }else root.Children().Append(control);
            }
            size_t modeIndex=0;
            for(auto value:actions){
                auto item=value.GetObject();auto id=str(item,L"command");auto command=find(array(data->state,L"commands"),L"id",id);
                auto invoke=[weak,id,context]{if(auto self=weak.lock();self&&settingsContext(self->data->state)==context)
                    self->data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(id)}}));};
                if(mode(id)&&modes){
                    auto pick=segment(str(command,L"icon"),str(command,L"label"),str(command,L"tooltip"),L"tool-action-"+id,compact,modeIndex,
                        [weak,id,invoke]{if(auto self=weak.lock();self&&!flag(find(array(self->data->state,L"commands"),L"id",id),L"selected"))invoke();});
                    Grid::SetColumn(pick,int(modeIndex++));modes.Children().Append(pick);
                    fields.emplace_back([weak,id,pick,compact]{if(auto self=weak.lock()){
                        auto command=find(array(self->data->state,L"commands"),L"id",id);bool chosen=flag(command,L"selected");
                        pick.IsEnabled(flag(command,L"enabled"));pick.Opacity(pick.IsEnabled()?1.:.36);
                        pick.Background(chosen?selected(self->data):compact?self->data->brush(L"input"):clear());
                        AutomationProperties::SetItemStatus(pick,chosen?L"Selected":L"");
                        tooltip(pick,str(command,L"tooltip"));
                    }});
                }else if(source(id)){
                    RadioButton radio;radio.GroupName(L"selection-source");auto text=toolLabel(data,command,false);text.Margin({6,0,0,0});radio.Content(text);
                    radio.MinWidth(0);radio.MinHeight(44);radio.Padding({0,0,0,0});radio.HorizontalAlignment(HorizontalAlignment::Stretch);
                    radio.VerticalContentAlignment(VerticalAlignment::Center);
                    AutomationProperties::SetName(radio,str(command,L"label"));AutomationProperties::SetAutomationId(radio,L"tool-action-"+id);
                    auto syncing=std::make_shared<bool>(false);
                    radio.Checked([invoke,syncing](auto&&,auto&&){if(!*syncing)invoke();});
                    root.Children().Append(radio);
                    fields.emplace_back([weak,id,radio,syncing]{if(auto self=weak.lock()){
                        auto command=find(array(self->data->state,L"commands"),L"id",id);
                        *syncing=true;radio.IsEnabled(flag(command,L"enabled"));radio.IsChecked(flag(command,L"selected"));*syncing=false;
                        tooltip(radio,str(command,L"tooltip"));
                    }});
                }else if(flag(item,L"checkable")){
                    CheckBox check;auto text=toolLabel(data,command,false);text.Margin({6,0,0,0});check.Content(text);
                    check.MinWidth(0);check.MinHeight(selectionTool?44:32);check.Padding({0,0,0,0});check.HorizontalAlignment(HorizontalAlignment::Stretch);
                    check.VerticalContentAlignment(VerticalAlignment::Center);
                    AutomationProperties::SetName(check,str(command,L"label"));AutomationProperties::SetAutomationId(check,L"tool-action-"+id);
                    check.Click([invoke](auto&&,auto&&){invoke();});root.Children().Append(check);
                    fields.emplace_back([weak,id,check]{if(auto self=weak.lock()){
                        auto command=find(array(self->data->state,L"commands"),L"id",id);
                        check.IsEnabled(flag(command,L"enabled"));check.IsChecked(flag(command,L"selected"));
                        tooltip(check,str(command,L"tooltip"));
                    }});
                }else{
                    auto pick=button(data,str(command,L"label"),invoke);pick.Height(selectionTool?44:36);pick.HorizontalAlignment(HorizontalAlignment::Stretch);
                    pick.Content(toolLabel(data,command));AutomationProperties::SetAutomationId(pick,L"tool-action-"+id);root.Children().Append(pick);
                    fields.emplace_back([weak,id,pick]{if(auto self=weak.lock()){
                        auto command=find(array(self->data->state,L"commands"),L"id",id);pick.IsEnabled(flag(command,L"enabled"));
                        tooltip(pick,str(command,L"tooltip"));
                    }});
                }
            }
            if(modeCount&&!compact)selectionActions();
        }
        for(auto const& bind:fields)bind();
    }
};
}
FrameworkElement ToolSetPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,hstring const& panel){
    auto view=std::make_shared<ToolSetView>(data,panel);view->init();bindings.emplace_back([view]{view->refresh();});return view->root;
}
FrameworkElement ToolSettingsPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<SettingsView>(data);bindings.emplace_back([view]{view->refresh();});return view->root;
}
