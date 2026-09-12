#include "pch.h"
#include "PanelConfiguration.h"
#include "ToolView.h"
#include "ColorView.h"
#include "EffectControls.h"
#include "StatsView.h"
#include "NativeMenus.h"
#include "WorkspaceGeometry.h"

using namespace CapyUi;
namespace {
J editing(std::shared_ptr<WorkspaceData> const& data){return object(object(data->state,L"layer_tools"),L"editing_layer");}
hstring epoch(std::shared_ptr<WorkspaceData> const& data){return to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")));}
A optionStructure(A const& sections){
    A result;
    for(auto section:sections){A items;for(auto value:section.GetArray()){
        auto spec=value.GetObject();items.Append(O({{L"action",object(spec,L"action")},{L"sections",optionStructure(array(spec,L"sections"))}}));
    }result.Append(items);}return result;
}
bool checked(J const& spec){auto value=spec.GetNamedValue(L"selected",JsonValue::CreateNullValue());return value.ValueType()==JsonValueType::Boolean&&value.GetBoolean();}
}
struct PanelConfiguration::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data;
    hstring panelId;
    J panel;
    ScrollView root;
    StackPanel content;
    TextBlock title,hint;
    Bindings bindings;
    std::map<std::wstring,FrameworkElement> anchors;
    std::unique_ptr<NavigatorView> navigator;
    std::function<void()> measured;
    MenuFlyout menu{nullptr};
    std::vector<J> menuModels;
    hstring structure;
    ~Impl(){if(menu)menu.Hide();}
    void customize(J const& action){data->dispatch(O({{L"type",S(L"customize")},{L"action",action}}));}
    Grid presets(){
        Grid grid;grid.ColumnSpacing(6);grid.RowSpacing(6);
        for(auto value:array(data->catalog,L"brush_sizes")){
            double size=value.GetNumber();
            auto pick=button(data,to_hstring(int(size)),[data=data,size]{data->dispatch(O({{L"type",S(L"set_brush_size")},{L"value",N(size)}}));});
            pick.Width(52);pick.Height(34);pick.Background(buttonBackground(data));grid.Children().Append(pick);
            AutomationProperties::SetAutomationId(pick,L"configure-size-"+to_hstring(int(size)));
        }
        auto last=std::make_shared<int>(0);auto weak=make_weak(grid);
        auto layout=[weak,last](double width){if(auto grid=weak.get()){
            int columns=std::max(1,int((width+6)/58));if(columns==*last)return;*last=columns;
            grid.ColumnDefinitions().Clear();grid.RowDefinitions().Clear();
            for(int i=0;i<columns;++i){ColumnDefinition c;c.Width({52,GridUnitType::Pixel});grid.ColumnDefinitions().Append(c);}
            for(uint32_t i=0;i<grid.Children().Size();++i){
                if(i%columns==0){RowDefinition r;r.Height({34,GridUnitType::Pixel});grid.RowDefinitions().Append(r);}
                auto child=grid.Children().GetAt(i).as<FrameworkElement>();
                Grid::SetColumn(child,int(i)%columns);Grid::SetRow(child,int(i)/columns);
            }
        }};
        grid.SizeChanged([layout](auto&& sender,auto&&){layout(sender.template as<Grid>().ActualWidth());});layout(356);
        return grid;
    }
    FrameworkElement layerSelector(){
        ComboBox choice;choice.HorizontalAlignment(HorizontalAlignment::Stretch);
        choice.FontSize(data->textSize());choice.MinHeight(34);
        AutomationProperties::SetAutomationId(choice,L"configure-layer-selection");
        auto weak=weak_from_this();
        choice.SelectionChanged([weak](auto&& sender,auto&&){if(auto self=weak.lock();self&&!self->data->updating){
            auto item=sender.template as<ComboBox>().SelectedItem().template try_as<ComboBoxItem>();if(!item)return;
            auto tag=item.Tag().template try_as<J>();if(!tag||str(tag,L"epoch")!=epoch(self->data))return;
            self->data->dispatchDocument(O({{L"type",S(L"select_layer")},{L"id",tag.GetNamedValue(L"id")}}),str(tag,L"epoch"));
        }});
        auto open=std::make_shared<bool>(false);
        choice.DropDownOpened([data=data,open](auto&&,auto&&){if(!std::exchange(*open,true))data->popup(true);});
        choice.DropDownClosed([data=data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
        choice.Unloaded([data=data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
        auto key=std::make_shared<hstring>();
        bindings.emplace_back([data=data,choice,key]{
            A identity;auto layers=array(data->state,L"layers");
            for(auto value:layers){auto row=value.GetObject();identity.Append(O({
                {L"id",row.GetNamedValue(L"id")},{L"label",S(str(row,L"label"))},{L"editable",B(flag(row,L"editable"))}}));}
            auto next=epoch(data)+identity.Stringify();
            if(next!=*key){
                *key=next;choice.Items().Clear();
                for(auto value:layers){auto row=value.GetObject();ComboBoxItem item;
                    item.Content(box_value(str(row,L"label")));item.IsEnabled(flag(row,L"editable"));
                    item.Tag(O({{L"epoch",S(epoch(data))},{L"id",row.GetNamedValue(L"id")}}));choice.Items().Append(item);}
            }
            int selected=-1;
            for(uint32_t i=0;i<layers.Size();++i)if(flag(layers.GetObjectAt(i),L"editing")){selected=int(i);break;}
            choice.SelectedIndex(selected);
        });
        return choice;
    }
    FrameworkElement layerOpacity(){
        ContentControl gate;gate.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        struct State{hstring key;Bindings values;};auto state=std::make_shared<State>();
        bindings.emplace_back([data=data,gate,state]{
            auto layer=editing(data);double id=num(layer,L"id",-1);auto generation=epoch(data),key=generation+L":"+to_hstring(id);
            if(key!=state->key){
                state->key=key;state->values.clear();
                gate.Content(number(data,L"Layer opacity",object(data->catalog,L"layer_opacity"),
                    [data]{return num(editing(data),L"opacity",1);},
                    [data,generation,id](double value){if(epoch(data)==generation&&num(editing(data),L"id",-1)==id)
                        data->dispatchDocument(O({{L"type",S(L"set_layer_opacity")},{L"opacity",N(value)}}),generation);
                    },state->values,nullptr,false,L"configure-layer-opacity"));
            }
            gate.IsEnabled(flag(object(object(data->state,L"layer_tools"),L"controls"),L"opacity"));
            for(auto const& bind:state->values)bind();
        });
        return gate;
    }
    FrameworkElement control(hstring const& kind,hstring const& labelText){
        if(kind==L"brushes")return ToolSetPanel(data,bindings);
        if(kind==L"tool_settings")return ToolSettingsPanel(data,bindings);
        if(kind==L"color_wheel")return ColorPanel(data,bindings);
        if(kind==L"properties")return PropertiesPanel(data,bindings);
        if(kind==L"stats")return StatsPanel(data,bindings);
        if(kind==L"layers")return layerSelector();
        if(kind==L"layer_opacity")return layerOpacity();
        if(kind==L"size_presets")return presets();
        if(kind==L"adjustments"){auto body=FiltersPanel(data,bindings);body.Height(480);return body;}
        if(kind==L"navigator"){
            navigator=std::make_unique<NavigatorView>(data,[weak=weak_from_this()]{if(auto self=weak.lock())self->measured();});
            navigator->Root().Height(272);return navigator->Root();
        }
        if(kind==L"brush_size"||kind==L"brush_opacity"){
            bool size=kind==L"brush_size";
            return number(data,labelText,object(data->catalog,size?L"brush_size":L"opacity"),
                [data=data,size]{return num(object(data->state,L"brush"),size?L"diameter":L"opacity");},
                [data=data,size](double value){data->dispatch(O({{L"type",S(size?L"set_brush_size":L"set_brush_opacity")},{L"value",N(value)}}));},
                bindings,nullptr,false,L"configure-"+kind);
        }
        if(kind==L"brush_color"){
            auto pick=button(data,labelText,[weak=weak_from_this()]{if(auto self=weak.lock())self->customize(
                O({{L"type",S(L"open_control")},{L"control",S(L"brush_color")}}));});
            pick.Height(34);pick.Padding({6,6,6,6});pick.HorizontalAlignment(HorizontalAlignment::Stretch);
            pick.VerticalContentAlignment(VerticalAlignment::Stretch);pick.Background(buttonBackground(data));
            pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            Border swatch;swatch.CornerRadius({4,4,4,4});pick.Content(swatch);anchors.insert_or_assign(L"brush_color",pick);
            AutomationProperties::SetAutomationId(pick,L"configure-brush-color");
            bindings.emplace_back([data=data,swatch]{auto rgba=array(object(data->state,L"brush"),L"color");
                if(rgba.Size()==4)swatch.Background(fill({uint8_t(std::round(rgba.GetNumberAt(3)*255)),
                    uint8_t(std::round(rgba.GetNumberAt(0)*255)),uint8_t(std::round(rgba.GetNumberAt(1)*255)),uint8_t(std::round(rgba.GetNumberAt(2)*255))}));});
            return pick;
        }
        if(kind==L"layer_actions"){
            StackPanel row;row.Orientation(Orientation::Horizontal);row.Spacing(6);
            for(auto value:array(data->catalog,L"layer_commands")){
                auto id=value.GetString();auto command=find(array(data->state,L"commands"),L"id",id);
                auto pick=button(data,str(command,L"label"),[data=data,id]{data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(id)}}));});
                pick.Width(34);pick.Height(34);pick.Content(icon(str(command,L"icon"),data->theme()));row.Children().Append(pick);
                ToolTipService::SetToolTip(pick,box_value(str(command,L"label")));
                bindings.emplace_back([data=data,id,pick]{pick.IsEnabled(flag(find(array(data->state,L"commands"),L"id",id),L"enabled"));});
            }
            return row;
        }
        return Border();
    }
    void init(){
        content.Padding({12,12,12,12});content.Spacing(12);root.Content(content);
        root.HorizontalScrollMode(ScrollingScrollMode::Disabled);root.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
        root.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);
        AutomationProperties::SetAutomationId(root,L"panel-configuration-"+panelId);
        content.SizeChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->measured();});
    }
    void build(){
        if(menu)menu.Hide();menu=nullptr;bindings.clear();anchors.clear();navigator.reset();content.Children().Clear();menuModels.clear();
        title=label(data,str(panel,L"configuration_title"),true);title.TextWrapping(TextWrapping::Wrap);content.Children().Append(title);
        hint=label(data,str(panel,L"configuration_hint"));hint.Foreground(data->brush(L"settings_secondary"));hint.TextWrapping(TextWrapping::Wrap);content.Children().Append(hint);
        for(auto value:array(panel,L"controls")){
            auto spec=value.GetObject();auto kind=str(spec,L"control");
            StackPanel section;section.Spacing(6);
            CheckBox visible;visible.Content(box_value(str(spec,L"label")));visible.FontSize(data->textSize());
            visible.Foreground(data->brush(L"text"));visible.MinHeight(28);
            AutomationProperties::SetAutomationId(visible,L"configure-show-"+kind);
            auto weak=weak_from_this();auto selection=make_weak(visible);
            auto toggle=[weak,selection,kind](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating){
                auto checkbox=selection.get();if(!checkbox)return;
                self->customize(O({{L"type",S(L"set_control_visible")},{L"panel",S(self->panelId)},{L"control",S(kind)},
                    {L"visible",B(checkbox.IsChecked()&&checkbox.IsChecked().Value())}}));
            }};
            visible.Checked(toggle);visible.Unchecked(toggle);
            bindings.emplace_back([weak,visible,kind]{if(auto self=weak.lock())
                visible.IsChecked(flag(find(array(self->panel,L"controls"),L"control",kind),L"visible_in_panel"));});
            section.Children().Append(visible);
            auto body=control(kind,str(spec,L"label"));AutomationProperties::SetAutomationId(body,L"configure-control-"+kind);
            section.Children().Append(body);content.Children().Append(section);
        }
        for(auto sectionValue:array(panel,L"toolbar_options")){
            StackPanel section;section.Spacing(2);
            for(auto value:sectionValue.GetArray()){
                auto spec=value.GetObject();auto index=menuModels.size();menuModels.push_back(spec);
                auto pick=button(data,str(spec,L"label"),[]{});
                pick.MinHeight(34);pick.Padding({8,4,8,4});pick.HorizontalAlignment(HorizontalAlignment::Stretch);
                pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
                Grid line;ColumnDefinition caption;caption.Width({1,GridUnitType::Star});line.ColumnDefinitions().Append(caption);
                ColumnDefinition mark;mark.Width({20,GridUnitType::Pixel});line.ColumnDefinitions().Append(mark);
                auto text=label(data,str(spec,L"label"),true);text.TextWrapping(TextWrapping::Wrap);line.Children().Append(text);
                auto glyph=icon(array(spec,L"sections").Size()?L"chevron-down":L"check",data->theme());Grid::SetColumn(glyph,1);line.Children().Append(glyph);
                pick.Content(line);auto weak=weak_from_this();auto anchor=make_weak(pick);
                pick.Click([weak,anchor,index](auto&&,auto&&){if(auto self=weak.lock();self&&index<self->menuModels.size()){
                    auto spec=self->menuModels[index];auto children=array(spec,L"sections");
                    if(children.Size()){
                        auto target=anchor.get();if(!target)return;
                        self->menu=MenuFlyout();TrackPopup(self->menu,self->data);
                        NativeMenuItems(self->menu.Items(),children,self->data,[data=self->data](J action){data->dispatch(action);});self->menu.ShowAt(target);
                    }else {auto action=object(spec,L"action");if(action.Size())self->data->dispatch(action);}
                }});
                bindings.emplace_back([weak,index,pick,glyph,text]{if(auto self=weak.lock();self&&index<self->menuModels.size()){
                    auto spec=self->menuModels[index];text.Text(str(spec,L"label"));AutomationProperties::SetName(pick,str(spec,L"label"));
                    pick.IsEnabled(flag(spec,L"enabled",true));glyph.Visibility(array(spec,L"sections").Size()||checked(spec)?Visibility::Visible:Visibility::Collapsed);
                    AutomationProperties::SetItemStatus(pick,checked(spec)?L"Selected":L"");
                }});
                AutomationProperties::SetAutomationId(pick,L"configure-option-"+to_hstring(index));section.Children().Append(pick);
            }
            content.Children().Append(section);
        }
    }
    void apply(J const& next,bool visible){
        CapyEffects::Updating updating(data);panel=next;
        A keys;for(auto value:array(panel,L"controls"))keys.Append(S(str(value.GetObject(),L"control")));
        A options;std::vector<J> models;
        for(auto section:array(panel,L"toolbar_options")){
            A items;for(auto value:section.GetArray()){
                auto spec=value.GetObject();models.push_back(spec);
                items.Append(O({{L"action",object(spec,L"action")},{L"children",optionStructure(array(spec,L"sections"))}}));
            }options.Append(items);
        }
        auto key=O({{L"controls",keys},{L"options",options}}).Stringify();
        if(key!=structure){structure=key;build();}else menuModels=std::move(models);
        title.Text(str(panel,L"configuration_title"));AutomationProperties::SetName(root,title.Text());
        for(auto const& bind:bindings)bind();if(navigator)navigator->Apply(visible);
    }
};
PanelConfiguration::PanelConfiguration(std::shared_ptr<WorkspaceData> data,J const& panel,std::function<void()> measured):impl(std::make_shared<Impl>()){
    impl->data=std::move(data);impl->panelId=str(panel,L"id");impl->panel=panel;impl->measured=std::move(measured);impl->init();impl->apply(panel,false);
}
PanelConfiguration::~PanelConfiguration()=default;
FrameworkElement PanelConfiguration::Root()const{return impl->root;}
hstring PanelConfiguration::Panel()const{return impl->panelId;}
void PanelConfiguration::Apply(J const& panel,bool visible){impl->apply(panel,visible);}
double PanelConfiguration::ContentHeight()const{return impl->content.IsLoaded()?impl->content.ActualHeight():0;}
void PanelConfiguration::SetVisible(bool visible){if(impl->navigator)impl->navigator->Apply(visible);}
FrameworkElement PanelConfiguration::Anchor(std::wstring const& control)const{
    auto found=impl->anchors.find(control);return found==impl->anchors.end()?nullptr:found->second;
}
void PanelConfiguration::AppendOverviews(A& slots,UIElement const& reference,Windows::Foundation::Rect clip,int order)const{
    if(!impl->navigator)return;
    clip=intersect(clip,visibleBounds(impl->navigator->Root(),reference.as<FrameworkElement>()));
    auto slot=impl->navigator->Placement(reference,clip,order);if(slot.Size())slots.Append(slot);
}
