#include "pch.h"
#include "ToolView.h"
#include "RangeControl.h"
#include "NativeMenus.h"
#include "WorkspaceQuery.h"
#include "WorkspaceGeometry.h"
#include "EffectControls.h"

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
        keys.Append(O({{L"icon",S(str(item,L"icon"))},
            {L"action",object(item,L"action")},{L"preview",item.GetNamedValue(L"preview",JsonValue::CreateNullValue())}}));
    }return keys.Stringify();
}
Grid toolLabel(std::shared_ptr<WorkspaceData> const& data,J const& item,bool bold=true,bool trailingName=false,TextBlock* retainedTitle=nullptr){
    Grid row;row.ColumnSpacing(6);
    ColumnDefinition glyph;glyph.Width({16,GridUnitType::Pixel});row.ColumnDefinitions().Append(glyph);
    ColumnDefinition text;text.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(text);
    row.Children().Append(icon(str(item,L"icon"),data->theme()));
    auto title=label(data,str(item,L"label"),bold);title.TextTrimming(TextTrimming::CharacterEllipsis);
    title.VerticalAlignment(VerticalAlignment::Center);
    if(trailingName){title.TextAlignment(TextAlignment::Right);title.LineHeight(18);}
    if(retainedTitle)*retainedTitle=title;Grid::SetColumn(title,1);row.Children().Append(title);return row;
}
struct ToolSetView : std::enable_shared_from_this<ToolSetView> {
    std::shared_ptr<WorkspaceData> data;
    StackPanel root,list;
    Canvas groups;
    Border divider;
    std::vector<Button> groupButtons,subtoolButtons;
    std::vector<TextBlock> groupTitles,subtoolTitles;
    hstring groupKey,subtoolKey;
    hstring panel;
    std::function<J()> projection;
    bool media()const{return panel==L"brush_sets"||panel==L"sculpt_sets";}
    double arrangedWidth=-1;
    ToolSetView(std::shared_ptr<WorkspaceData> data,hstring panel,std::function<J()> projection):data(std::move(data)),panel(std::move(panel)),projection(std::move(projection)){}
    void init(){
        root.Spacing(6);list.Spacing(2);
        auto weak=weak_from_this();
        divider.Height(1);divider.Margin({4,0,4,0});divider.Background(data->brush(L"settings_secondary"));divider.Opacity(.3);
        AutomationProperties::SetAutomationId(divider,L"tool-set-divider");
        root.Children().Append(groups);root.Children().Append(divider);root.Children().Append(list);
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
        auto& titles=group?groupTitles:subtoolTitles;titles.clear();
        if(group)groups.Children().Clear();else list.Children().Clear();
        auto weak=weak_from_this();
        for(uint32_t i=0;i<items.Size();i++){
            auto item=items.GetObjectAt(i);auto action=object(item,L"action");TextBlock title;
            auto pick=button(data,str(item,L"label"),[weak,action]{if(auto self=weak.lock())self->data->dispatch(action);});
            pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            pick.Padding({17,5,17,5});actionTooltip(data,pick,[action]{return action;});
            AutomationProperties::SetAutomationId(pick,(group?L"tool-group-":L"tool-subtool-")+to_hstring(i));
            if(group&&media())AutomationProperties::SetAutomationId(pick,(panel==L"sculpt_sets"?L"sculpt-set-":L"brush-set-")+str(item,L"icon"));
            if(group&&media()){
                pick.Padding({6,5,6,5});pick.Content(toolLabel(data,item,false,false,&title));
            }else if(group){
                pick.Padding({6,0,6,0});pick.Content(toolLabel(data,item,true,true,&title));
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
                auto caption=toolLabel(data,item,true,brush,&title);
                Grid::SetRow(caption,1);content.Children().Append(caption);pick.Content(content);
            }
            buttons.push_back(pick);titles.push_back(title);if(group)groups.Children().Append(pick);else list.Children().Append(pick);
        }
        if(group){arrangedWidth=-1;arrange();}
    }
    void refresh(){
        auto view=panel==L"brushes"&&projection?projection():J{};
        if(!view.Size())view=panel==L"brushes"?object(data->state,L"tool_set"):object(object(data->state,L"tool_panels"),panel.c_str());
        bool tonal=false;for(auto value:array(data->state,L"tool_extra"))tonal=tonal||str(object(value.GetObject(),L"Choice"),L"id")==L"tonal-tones";
        for(bool group:{true,false}){
            auto items=array(view,group?L"groups":L"subtools");auto key=itemSchema(items)+data->theme();
            auto& previous=group?groupKey:subtoolKey;
            if(previous!=key){previous=key;rebuild(items,group);}
            auto const& buttons=group?groupButtons:subtoolButtons;auto const& titles=group?groupTitles:subtoolTitles;
            for(uint32_t i=0;i<items.Size();i++){
                auto item=items.GetObjectAt(i);auto title=str(item,L"label");titles[i].Text(title);AutomationProperties::SetName(buttons[i],title);
                bool active=flag(item,L"selected");buttons[i].Background(active?selected(data):clear());
                buttons[i].IsEnabled(flag(item,L"enabled",true));
                if(!group)buttons[i].MinHeight(items.GetObjectAt(i).GetNamedValue(L"preview",JsonValue::CreateNullValue()).ValueType()==JsonValueType::Number?34:tonal?36:44);
                AutomationProperties::SetItemStatus(buttons[i],active?data->caption(L"search",L"selected"):hstring());
            }
        }
        bool both=groups.Visibility()==Visibility::Visible&&list.Visibility()==Visibility::Visible&&!groupButtons.empty()&&!subtoolButtons.empty();
        divider.Visibility(both?Visibility::Visible:Visibility::Collapsed);
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
    void pickerChoice(std::function<hstring()> const& title,hstring const& id,std::function<std::vector<hstring>()> const& names,
        std::function<int(J const&)> current,std::function<void(int)> select){
        Grid row;row.ColumnSpacing(8);row.MinHeight(36);
        ColumnDefinition caption;caption.Width({68,GridUnitType::Pixel});row.ColumnDefinitions().Append(caption);
        ColumnDefinition value;value.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(value);
        auto text=label(data,title());text.FontSize(13);text.VerticalAlignment(VerticalAlignment::Center);
        text.TextWrapping(TextWrapping::NoWrap);row.Children().Append(text);
        ComboBox choices;choices.MinWidth(0);choices.MinHeight(32);choices.FontSize(13);choices.Padding({8,6,0,6});
        choices.HorizontalAlignment(HorizontalAlignment::Stretch);choices.VerticalAlignment(VerticalAlignment::Center);
        choices.Background(data->brush(L"input"));choices.BorderThickness({0,0,0,0});choices.CornerRadius({6,6,6,6});
        for(auto const& name:names())comboOption(choices,name);
        AutomationProperties::SetName(choices,title());AutomationProperties::SetAutomationId(choices,id);
        auto syncing=std::make_shared<bool>(false);
        choices.SelectionChanged([select,syncing,weak=make_weak(choices)](auto&&,auto&&){
            if(auto c=weak.get();c&&!*syncing&&c.SelectedIndex()>=0)select(c.SelectedIndex());
        });
        auto open=std::make_shared<bool>(false);
        choices.DropDownOpened([data=data,open](auto&&,auto&&){if(!std::exchange(*open,true))data->popup(true);});
        choices.DropDownClosed([data=data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
        choices.Unloaded([data=data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
        fields.emplace_back([weak=weak_from_this(),choices,syncing,current,title,names,text]{if(auto self=weak.lock()){
            auto caption=title();text.Text(caption);AutomationProperties::SetName(choices,caption);
            auto options=names();for(uint32_t i=0;i<options.size();++i)comboOptionText(choices,i,options[i]);
            auto index=current(object(self->data->state,L"color_picker"));
            if(choices.SelectedIndex()!=index){*syncing=true;choices.SelectedIndex(index);*syncing=false;}
        }});
        Grid::SetColumn(choices,1);row.Children().Append(choices);root.Children().Append(row);
    }
    void refreshPicker(){
        auto picker=object(data->state,L"color_picker");auto sizes=array(picker,L"sample_sizes");
        bool layers=flag(picker,L"can_sample_layer"),calibrating=flag(picker,L"calibrating");
        auto next=O({{L"picker",B(true)},{L"layers",B(layers)},{L"calibrating",B(calibrating)},{L"sizes",sizes}}).Stringify();
        if(next!=key){
            key=next;fields.clear();root.Children().Clear();
            auto weak=weak_from_this();
            auto sourceTitle=[weak]{if(auto self=weak.lock())return self->data->caption(L"sampler",L"source");return hstring();};
            auto sources=[weak,layers]{std::vector<hstring> names;if(auto self=weak.lock()){names.push_back(self->data->caption(L"sampler",L"visible_color"));if(layers)names.push_back(self->data->caption(L"sampler",L"selected_layer"));}return names;};
            if(!calibrating)pickerChoice(sourceTitle,L"picker-setting-source",sources,
                [](J const& value){return flag(value,L"layer")?1:0;},
                [weak](int index){if(auto self=weak.lock();self&&picking(self->data->state)&&flag(object(self->data->state,L"color_picker"),L"layer")!=(index==1))
                    self->data->dispatch(O({{L"type",S(L"color_picker")},{L"action",O({{L"kind",S(L"source")},{L"layer",B(index==1)}})}}));});
            std::vector<double> widths;for(auto value:sizes)widths.push_back(value.GetNumber());
            auto sizeTitle=[weak]{if(auto self=weak.lock())return self->data->caption(L"sampler",L"sample_size");return hstring();};
            auto names=[weak,widths]{std::vector<hstring> result;if(auto self=weak.lock())for(auto width:widths)
                for(auto value:array(object(object(self->data->catalog,L"native_copy"),L"sampler"),L"sizes")){
                    auto item=value.GetArray();if(item.GetNumberAt(0)==width){result.push_back(item.GetStringAt(1));break;}}
                return result;};
            pickerChoice(sizeTitle,L"picker-setting-size",names,
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
        A result;for(auto value:extra){
            auto entry=value.GetObject();
            if(auto gradient=object(entry,L"Gradient");gradient.Size()){result.Append(object(object(gradient,L"gradient"),L"destination"));continue;}
            auto choice=object(entry,L"Choice");
            result.Append(O({{L"id",S(str(choice,L"id"))},{L"columns",N(num(choice,L"columns"))},
                {L"beside",S(str(choice,L"beside"))},{L"items",S(itemSchema(array(choice,L"items")))}}));
        }return result;
    }
    static J toolGradient(J const& state){
        for(auto value:array(state,L"tool_extra"))if(auto gradient=object(value.GetObject(),L"Gradient");gradient.Size())return gradient;
        return J{};
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
        Button open=button(data,data->caption(L"tool_controls",L"selection_menu"),[]{});open.MinHeight(44);open.HorizontalAlignment(HorizontalAlignment::Stretch);
        open.HorizontalContentAlignment(HorizontalAlignment::Stretch);open.Padding({12,0,12,0});open.Flyout(menu);
        Grid content;content.ColumnSpacing(8);
        ColumnDefinition text;text.Width({1,GridUnitType::Star});content.ColumnDefinitions().Append(text);
        ColumnDefinition arrow;arrow.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(arrow);
        auto title=label(data,data->caption(L"tool_controls",L"selection_menu"),true);title.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(title);
        auto chevron=icon(L"chevron-down",data->theme(),12);chevron.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(chevron,1);content.Children().Append(chevron);
        open.Content(content);AutomationProperties::SetAutomationId(open,L"selection-actions-menu");
        fields.emplace_back([weak,open,title]{if(auto self=weak.lock()){auto caption=self->data->caption(L"tool_controls",L"selection_menu");title.Text(caption);AutomationProperties::SetName(open,caption);}});
        root.Children().Append(open);
    }
    void refresh(){
        if(picking(data->state))return refreshPicker();
        auto context=settingsContext(data->state);A schema;hstring previousGroup;
        for(auto value:array(data->state,L"tool_settings")){
            auto item=value.GetObject();auto group=str(item,L"group");
            schema.Append(O({{L"id",S(str(item,L"id"))},{L"group_boundary",B(previousGroup!=group)},{L"numeric",object(item,L"numeric")}}));previousGroup=group;
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
                AutomationProperties::SetName(modes,data->caption(L"tool_controls",L"selection_mode"));
                fields.emplace_back([weak,modes]{if(auto self=weak.lock())AutomationProperties::SetName(modes,self->data->caption(L"tool_controls",L"selection_mode"));});
                if(compact)root.Children().Append(modes);
                else{Border frame;frame.BorderThickness({1,1,1,1});frame.BorderBrush(data->tint(L"text",51));frame.CornerRadius({6,6,6,6});
                    frame.Child(modes);root.Children().Append(frame);}
            }
            std::map<std::wstring,J> beside;
            for(auto value:extra){auto spec=object(value.GetObject(),L"Choice");if(auto target=str(spec,L"beside");!target.empty())beside.emplace(target.c_str(),spec);}
            for(auto value:extra){
                if(object(value.GetObject(),L"Gradient").Size()){
                    root.Children().Append(CapyEffects::GradientEditor(data,{[weak]{if(auto self=weak.lock())return toolGradient(self->data->state);return J{};},
                        [weak,context](J action,hstring phase){
                            auto self=weak.lock();bool continuing=!phase.empty()&&phase!=L"down";
                            if(self&&(continuing||settingsContext(self->data->state)==context))
                                self->data->dispatch(O({{L"type",S(L"effect")},{L"action",CapyEffects::effectGesture(action,phase)}}));
                        },[]{return true;},L"tool-gradient"},fields));
                    continue;
                }
                auto spec=object(value.GetObject(),L"Choice");auto items=array(spec,L"items");auto specId=str(spec,L"id");
                if(!str(spec,L"beside").empty())continue;
                auto bar=segmented(true,items.Size());AutomationProperties::SetName(bar,str(spec,L"label"));
                bool labeled=flag(spec,L"labeled");
                auto caption=label(data,str(spec,L"label"));
                if(labeled){
                    Grid row;row.ColumnSpacing(8);
                    ColumnDefinition title;title.Width({1,GridUnitType::Auto});row.ColumnDefinitions().Append(title);
                    ColumnDefinition control;control.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(control);
                    caption.MinWidth(64);caption.VerticalAlignment(VerticalAlignment::Center);row.Children().Append(caption);
                    Grid::SetColumn(bar,1);row.Children().Append(bar);root.Children().Append(row);
                }
                AutomationProperties::SetAutomationId(bar,L"tool-choice-"+specId);
                fields.emplace_back([weak,bar,caption,specId]{if(auto self=weak.lock())for(auto option:array(self->data->state,L"tool_extra"))
                    if(auto current=object(option.GetObject(),L"Choice");str(current,L"id")==specId){AutomationProperties::SetName(bar,str(current,L"label"));caption.Text(str(current,L"label"));}});
                for(uint32_t i=0;i<items.Size();i++){
                    auto item=items.GetObjectAt(i);auto action=object(item,L"action");
                    auto pick=segment(str(item,L"icon"),str(item,L"label"),str(item,L"label"),L"tool-choice-"+specId+L"-"+to_hstring(i),true,i,
                        [weak,action,context]{if(auto self=weak.lock();self&&settingsContext(self->data->state)==context)self->data->dispatch(action);});
                    if(labeled)pick.Content(label(data,str(item,L"label")));
                    Grid::SetColumn(pick,int(i));bar.Children().Append(pick);
                    fields.emplace_back([weak,pick,specId,i,labeled]{if(auto self=weak.lock()){
                        J current;for(auto option:array(self->data->state,L"tool_extra"))if(str(object(option.GetObject(),L"Choice"),L"id")==specId)current=object(option.GetObject(),L"Choice");
                        auto items=array(current,L"items");bool chosen=i<items.Size()&&flag(items.GetObjectAt(i),L"selected");
                        pick.Background(chosen?selected(self->data):self->data->brush(L"input"));
                        if(i<items.Size()){auto title=str(items.GetObjectAt(i),L"label");AutomationProperties::SetName(pick,title);tooltip(pick,title);if(labeled)pick.Content(label(self->data,title));}
                        AutomationProperties::SetItemStatus(pick,chosen?self->data->caption(L"search",L"selected"):hstring());
                    }});
                }
                if(!labeled)root.Children().Append(bar);
            }
            auto settings=array(data->state,L"tool_settings");StackPanel numbers{nullptr};
            for(auto value:settings){
                auto item=value.GetObject();auto id=str(item,L"id");
                if(compact&&id==L"tonal_upper")continue;
                if(compact&&id==L"tonal_lower"){
                    auto upper=find(settings,L"id",L"tonal_upper");
                    range=RangeControl::Create(data,item,upper,data->caption(L"tool_controls",L"range_hint"),L"tool-setting",true,
                        [weak,context](int index,double value){if(auto self=weak.lock();self&&settingsContext(self->data->state)==context)
                            self->data->dispatch(O({{L"type",S(L"set_tool_setting")},{L"id",S(index?L"tonal_upper":L"tonal_lower")},{L"value",N(value)}}));});
                    root.Children().Append(range->root);
                    fields.emplace_back([weak]{if(auto self=weak.lock();self&&self->range){
                        auto current=array(self->data->state,L"tool_settings");
                        auto lower=find(current,L"id",L"tonal_lower"),upper=find(current,L"id",L"tonal_upper");
                        self->range->Relabel(lower,upper,self->data->caption(L"tool_controls",L"range_hint"));
                        self->range->Update(num(lower,L"value"),num(upper,L"value"));
                    }});
                    continue;
                }
                if(group!=str(item,L"group")){
                    group=str(item,L"group");numbers=nullptr;auto anchor=beside.find(id.c_str());
                    if(!group.empty()||anchor!=beside.end()){
                        auto heading=label(data,anchor!=beside.end()?str(anchor->second,L"label"):group,true);heading.Opacity(.55);heading.Margin({0,6,0,0});root.Children().Append(heading);
                        auto specId=anchor!=beside.end()?str(anchor->second,L"id"):hstring();
                        fields.emplace_back([weak,heading,id,specId]{if(auto self=weak.lock()){
                            auto title=str(find(array(self->data->state,L"tool_settings"),L"id",id),L"group");
                            if(!specId.empty())for(auto option:array(self->data->state,L"tool_extra"))if(auto spec=object(option.GetObject(),L"Choice");str(spec,L"id")==specId)title=str(spec,L"label");
                            heading.Text(title);}});
                    }
                    if(anchor!=beside.end()){
                        auto specId=str(anchor->second,L"id");
                        auto grid=choiceGrid(data,anchor->second,L"tool-choice-"+specId,[weak,context](J action){
                            if(auto self=weak.lock();self&&settingsContext(self->data->state)==context)self->data->dispatch(action);});
                        Grid row;row.ColumnSpacing(12);
                        for(auto width:{GridUnitType::Auto,GridUnitType::Star}){ColumnDefinition column;column.Width({1,width});row.ColumnDefinitions().Append(column);}
                        grid.grid.VerticalAlignment(VerticalAlignment::Center);row.Children().Append(grid.grid);
                        numbers=StackPanel();numbers.Spacing(6);Grid::SetColumn(numbers,1);row.Children().Append(numbers);root.Children().Append(row);
                        fields.emplace_back([weak,grid,specId]{if(auto self=weak.lock())for(auto option:array(self->data->state,L"tool_extra"))
                            if(auto spec=object(option.GetObject(),L"Choice");str(spec,L"id")==specId)grid.update(self->data,spec);});
                    }
                }
                NumberPresentation presentation;presentation.identity=[context]{return context;};
                presentation.title=[weak,id]{if(auto self=weak.lock())return str(find(array(self->data->state,L"tool_settings"),L"id",id),L"label");return hstring();};
                auto control=number(data,str(item,L"label"),object(item,L"numeric"),
                    [weak,id]{if(auto self=weak.lock())return num(find(array(self->data->state,L"tool_settings"),L"id",id),L"value");return 0.;},
                    [weak,id,context](double value){if(auto self=weak.lock();self&&settingsContext(self->data->state)==context)
                        self->data->dispatch(O({{L"type",S(L"set_tool_setting")},{L"id",S(id)},{L"value",N(value)}}));},fields,nullptr,bool(numbers),L"tool-setting-"+id,compact,presentation);
                AutomationProperties::SetAutomationId(control,L"number-root-tool-setting-"+id);
                if(numbers){
                    Grid line;line.ColumnSpacing(6);
                    for(auto width:{GridUnitType::Auto,GridUnitType::Star}){ColumnDefinition column;column.Width({1,width});line.ColumnDefinitions().Append(column);}
                    auto text=label(data,str(item,L"label"));text.MinWidth(16);text.VerticalAlignment(VerticalAlignment::Center);line.Children().Append(text);
                    fields.emplace_back([text,title=presentation.title]{text.Text(title());});
                    Grid::SetColumn(control,1);line.Children().Append(control);numbers.Children().Append(line);
                }else if(compact){
                    Grid row;row.ColumnSpacing(6);row.Height(28);
                    ColumnDefinition caption;caption.Width({1,GridUnitType::Auto});row.ColumnDefinitions().Append(caption);
                    ColumnDefinition body;body.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(body);
                    auto text=label(data,str(item,L"label"));text.MinWidth(62);text.VerticalAlignment(VerticalAlignment::Center);row.Children().Append(text);
                    fields.emplace_back([text,title=presentation.title]{text.Text(title());});
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
                        AutomationProperties::SetName(pick,str(command,L"label"));
                        AutomationProperties::SetItemStatus(pick,chosen?self->data->caption(L"search",L"selected"):hstring());
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
                        radio.Content().as<Grid>().Children().GetAt(1).as<TextBlock>().Text(str(command,L"label"));AutomationProperties::SetName(radio,str(command,L"label"));
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
                        check.Content().as<Grid>().Children().GetAt(1).as<TextBlock>().Text(str(command,L"label"));AutomationProperties::SetName(check,str(command,L"label"));
                        check.IsEnabled(flag(command,L"enabled"));check.IsChecked(flag(command,L"selected"));
                        tooltip(check,str(command,L"tooltip"));
                    }});
                }else{
                    auto pick=button(data,str(command,L"label"),invoke);pick.Height(selectionTool?44:36);pick.HorizontalAlignment(HorizontalAlignment::Stretch);
                    pick.Content(toolLabel(data,command));AutomationProperties::SetAutomationId(pick,L"tool-action-"+id);root.Children().Append(pick);
                    fields.emplace_back([weak,id,pick]{if(auto self=weak.lock()){
                        auto command=find(array(self->data->state,L"commands"),L"id",id);pick.Content().as<Grid>().Children().GetAt(1).as<TextBlock>().Text(str(command,L"label"));AutomationProperties::SetName(pick,str(command,L"label"));pick.IsEnabled(flag(command,L"enabled"));
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
FrameworkElement ToolSetPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,hstring const& panel,std::function<J()> projection){
    auto view=std::make_shared<ToolSetView>(data,panel,std::move(projection));view->init();bindings.emplace_back([view]{view->refresh();});return view->root;
}
FrameworkElement ToolSettingsPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<SettingsView>(data);bindings.emplace_back([view]{view->refresh();});return view->root;
}
FrameworkElement BrushSizePanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto style=toolbarUi(data->localization.get(),O({{L"type",S(L"style")},{L"style",S(L"small")}})).GetObject();
    auto tile=array(data->catalog,L"brush_size_tile");
    float width=float(tile.GetNumberAt(0)),height=float(tile.GetNumberAt(1));double gap=num(style,L"gap");
    double radius=array(style,L"size").GetNumberAt(0)/2*CornerFit;
    auto ink=data->brush(L"text").Color();
    Media::LinearGradientBrush fade;fade.MappingMode(BrushMappingMode::Absolute);fade.StartPoint({0,0});fade.EndPoint({0,height});
    for(auto [offset,alpha]:{std::pair{.4,1.},std::pair{.65,.2},std::pair{1.,0.}}){
        Media::GradientStop stop;stop.Offset(offset);auto color=ink;color.A=uint8_t(std::lround(ink.A*alpha));stop.Color(color);fade.GradientStops().Append(stop);
    }
    Canvas grid;auto tiles=std::make_shared<std::vector<Button>>();
    for(auto value:array(data->catalog,L"brush_sizes")){
        auto preset=value.GetObject();double size=num(preset,L"value");auto text=str(preset,L"label");
        auto action=O({{L"type",S(L"set_brush_size")},{L"value",N(size)}});
        auto pick=button(data,text+L" px",[data,action]{data->dispatch(action);});
        pick.Width(width);pick.Height(height);pick.CornerRadius({radius,radius,radius,radius});
        pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);pick.VerticalContentAlignment(VerticalAlignment::Stretch);
        Grid content;double diameter=num(preset,L"preview_diameter");
        Microsoft::UI::Xaml::Shapes::Path dot;Media::EllipseGeometry circle;circle.Center({width/2,width/2});
        circle.RadiusX(diameter/2);circle.RadiusY(diameter/2);dot.Data(circle);dot.Fill(fade);
        dot.HorizontalAlignment(HorizontalAlignment::Left);dot.VerticalAlignment(VerticalAlignment::Top);content.Children().Append(dot);
        auto caption=label(data,text);caption.FontWeight(Windows::UI::Text::FontWeights::Normal());caption.LineHeight(data->textSize());
        caption.HorizontalAlignment(HorizontalAlignment::Center);caption.VerticalAlignment(VerticalAlignment::Bottom);caption.Margin({0,0,0,2});
        content.Children().Append(caption);pick.Content(content);
        AutomationProperties::SetAutomationId(pick,L"size-preset-"+text);actionTooltip(data,pick,[action]{return action;});
        grid.Children().Append(pick);tiles->push_back(pick);
        bindings.emplace_back([data,pick,size]{
            bool chosen=num(object(data->state,L"brush"),L"diameter")==size;pick.Background(chosen?selected(data):clear());
            AutomationProperties::SetItemStatus(pick,chosen?data->caption(L"search",L"selected"):hstring());
        });
    }
    grid.SizeChanged([tiles,width,height,gap](auto const& sender,auto&&){
        auto canvas=sender.template as<Canvas>();double available=canvas.ActualWidth();
        int columns=std::max(1,int((available+gap)/(width+gap)));double cell=std::min(double(width),available);
        for(size_t i=0;i<tiles->size();++i){
            auto const& pick=(*tiles)[i];pick.Width(cell);
            Canvas::SetLeft(pick,double(i%columns)*(cell+gap));Canvas::SetTop(pick,double(i/columns)*(height+gap));
        }
        size_t rows=(tiles->size()+columns-1)/columns;
        canvas.Height(rows?rows*height+(rows-1)*gap:0);
    });
    return grid;
}
