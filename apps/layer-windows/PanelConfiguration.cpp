#include "pch.h"
#include "PanelConfiguration.h"
#include "ToolView.h"
#include "ColorView.h"
#include "EffectControls.h"
#include "StatsView.h"
#include "ScopesView.h"
#include "WorkspaceGeometry.h"
#include "PanelScroll.h"

using namespace CapyUi;
namespace {
J editing(std::shared_ptr<WorkspaceData> const& data){return object(object(data->state,L"layer_tools"),L"editing_layer");}
hstring epoch(std::shared_ptr<WorkspaceData> const& data){return to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")));}
A optionStructure(A const& sections){
    A result;
    for(auto section:sections){A items;for(auto value:section.GetArray()){
        auto spec=value.GetObject();items.Append(O({{L"action",object(spec,L"action")},{L"radio",B(spec.GetNamedValue(L"selected",JsonValue::CreateNullValue()).ValueType()==JsonValueType::Boolean)},{L"sections",optionStructure(array(spec,L"sections"))}}));
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
    std::vector<J> optionModels;
    hstring structure;
    void customize(J const& action){data->dispatch(O({{L"type",S(L"customize")},{L"action",action}}));}
    FrameworkElement layerSelector(){
        ComboBox choice;choiceColors(data,choice);choice.HorizontalAlignment(HorizontalAlignment::Stretch);
        choice.FontSize(data->textSize());choice.MinHeight(data->controlHeight());
        AutomationProperties::SetAutomationId(choice,L"configure-layer-selection");
        auto weak=weak_from_this();
        choice.SelectionChanged([weak](auto&& sender,auto&&){if(auto self=weak.lock();self&&!self->data->updating){
            auto item=sender.template as<ComboBox>().SelectedItem().template try_as<ComboBoxItem>();if(!item)return;
            auto tag=item.Tag().template try_as<J>();if(!tag||str(tag,L"epoch")!=epoch(self->data))return;
            self->data->dispatchDocument(O({{L"type",S(L"select_layer")},{L"id",tag.GetNamedValue(L"id",JsonValue::CreateNullValue())}}),str(tag,L"epoch"));
        }});
        auto key=std::make_shared<hstring>();
        bindings.emplace_back([data=data,choice,key]{
            A identity;auto layers=array(data->state,L"layers");
            for(auto value:layers){auto row=value.GetObject();identity.Append(O({
                {L"id",row.GetNamedValue(L"id",JsonValue::CreateNullValue())},{L"label",S(str(row,L"label"))}}));}
            auto next=epoch(data)+identity.Stringify();
            if(next!=*key){
                *key=next;choice.Items().Clear();
                for(auto value:layers){auto row=value.GetObject();ComboBoxItem item;
                    item.Content(box_value(str(row,L"label")));
                    item.Tag(O({{L"epoch",S(epoch(data))},{L"id",row.GetNamedValue(L"id",JsonValue::CreateNullValue())}}));choice.Items().Append(item);}
            }
            int selected=-1;
            for(uint32_t i=0;i<layers.Size();++i)if(flag(layers.GetObjectAt(i),L"editing")){selected=int(i);break;}
            selectIndex(choice,selected);
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
                NumberPresentation presentation;presentation.title=[source=std::weak_ptr<WorkspaceData>(data)]{auto data=source.lock();return data?data->caption(L"layers",L"opacity"):hstring{};};
                gate.Content(number(data,data->caption(L"layers",L"opacity"),object(data->catalog,L"layer_opacity"),
                    [data]{return num(editing(data),L"opacity",1);},
                    [data,generation,id](double value){if(epoch(data)==generation&&num(editing(data),L"id",-1)==id)
                        data->dispatchDocument(O({{L"type",S(L"set_layer_opacity")},{L"opacity",N(value)}}),generation);
                    },state->values,nullptr,false,L"configure-layer-opacity",false,presentation));
            }
            gate.IsEnabled(flag(object(object(data->state,L"layer_tools"),L"controls"),L"opacity"));
            for(auto const& bind:state->values)bind();
        });
        return gate;
    }
    FrameworkElement control(hstring const& kind,hstring const& labelText){
        if(kind==L"brushes"||kind==L"brush_sets"||kind==L"sculpt_sets"||kind==L"tools")return ToolSetPanel(data,bindings,kind);
        if(kind==L"filter_types"){auto body=FilterTypesPanel(data,bindings);body.Height(data->panelMetric(L"drawer_filters_min_height"));return body;}
        if(kind==L"tool_settings")return ToolSettingsPanel(data,bindings);
        if(kind==L"color_wheel")return ColorPanel(data,bindings);
        if(kind==L"properties")return PropertiesPanel(data,bindings);
        if(kind==L"stats")return StatsPanel(data,bindings);
        if(kind==L"histogram"||kind==L"waveform")return CapyScopes::ScopePanel(data,bindings,kind==L"waveform");
        if(kind==L"layers")return layerSelector();
        if(kind==L"layer_opacity")return layerOpacity();
        if(kind==L"size_presets")return BrushSizePanel(data,bindings);
        if(kind==L"adjustments"){auto body=FiltersPanel(data,bindings);body.Height(data->panelMetric(L"drawer_filters_min_height"));return body;}
        if(kind==L"navigator"){
            navigator=std::make_unique<NavigatorView>(data,[weak=weak_from_this()]{if(auto self=weak.lock())self->measured();});
            navigator->Root().Height(navigator->NaturalHeight(data->panelMetric(L"configuration_width")-24));return navigator->Root();
        }
        if(kind==L"brush_size"||kind==L"brush_opacity"){
            bool size=kind==L"brush_size";
            NumberPresentation presentation;presentation.reset=[data=data,size]{data->dispatch(O({{L"type",S(L"reset_tool_setting")},{L"id",S(size?L"size":L"opacity")}}));};
            presentation.title=[weak=weak_from_this(),kind]{auto self=weak.lock();return self?str(find(array(self->panel,L"controls"),L"control",kind),L"label"):hstring{};};
            return number(data,labelText,object(data->catalog,size?L"brush_size":L"opacity"),
                [data=data,size]{return num(object(data->state,L"brush"),size?L"diameter":L"opacity");},
                [data=data,size](double value){data->dispatch(O({{L"type",S(size?L"set_brush_size":L"set_brush_opacity")},{L"value",N(value)}}));},
                bindings,nullptr,false,L"configure-"+kind,false,presentation);
        }
        if(kind==L"brush_color"){
            auto pick=button(data,labelText,[weak=weak_from_this()]{if(auto self=weak.lock())self->customize(
                O({{L"type",S(L"open_control")},{L"control",S(L"brush_color")}}));});
            pick.Height(34);pick.Padding({6,6,6,6});pick.HorizontalAlignment(HorizontalAlignment::Stretch);
            pick.VerticalContentAlignment(VerticalAlignment::Stretch);pick.Background(buttonBackground(data));
            pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            Border swatch;swatch.CornerRadius({4,4,4,4});pick.Content(swatch);anchors.insert_or_assign(L"brush_color",pick);
            bindings.emplace_back([weak=weak_from_this(),data=data,pick,swatch,key=std::make_shared<hstring>()]{
                if(auto self=weak.lock())AutomationProperties::SetName(pick,str(find(array(self->panel,L"controls"),L"control",L"brush_color"),L"label"));
                auto paint=array(object(data->model,L"paint_pair"),L"rgba");
                if(auto next=paint.Stringify();next!=*key){
                    *key=next;swatch.Background(fill(previewColor(paint)));
                }
            });
            return pick;
        }
        if(kind==L"layer_actions"){
            StackPanel row;row.Orientation(Orientation::Horizontal);row.Spacing(6);
            for(auto value:array(data->catalog,L"layer_commands")){
                auto id=value.GetString();auto command=find(array(data->state,L"commands"),L"id",id);
                auto pick=button(data,str(command,L"label"),[data=data,id]{data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(id)}}));});
                pick.Width(data->controlHeight());pick.Height(data->controlHeight());pick.Content(icon(str(command,L"icon"),data->theme()));row.Children().Append(pick);
                tooltip(pick,str(command,L"label"));
                bindings.emplace_back([data=data,id,pick]{
                    auto command=find(array(data->state,L"commands"),L"id",id);auto caption=str(command,L"label");
                    if(AutomationProperties::GetName(pick)!=caption){AutomationProperties::SetName(pick,caption);tooltip(pick,caption);}
                    pick.IsEnabled(flag(command,L"enabled"));});
            }
            return row;
        }
        return Border();
    }
    void init(){
        content.Padding({12,12,12,12});content.Spacing(12);content.Background(clear());root.Content(content);root.ContentOrientation(ScrollingContentOrientation::Vertical);panelScrolling(root,content,data);
        root.HorizontalScrollMode(ScrollingScrollMode::Disabled);root.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
        root.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);mouseScrollbars(root);
        AutomationProperties::SetAutomationId(root,L"panel-configuration-"+panelId);
        content.SizeChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->measured();});
    }
    void build(){
        bindings.clear();anchors.clear();navigator.reset();content.Children().Clear();optionModels.clear();
        title=label(data,str(panel,L"configuration_title"),true);title.TextWrapping(TextWrapping::Wrap);content.Children().Append(title);
        hint=label(data,str(panel,L"configuration_hint"));hint.Foreground(data->brush(L"settings_secondary"));hint.TextWrapping(TextWrapping::Wrap);content.Children().Append(hint);
        for(auto value:array(panel,L"controls")){
            auto spec=value.GetObject();auto kind=str(spec,L"control");
            StackPanel section;section.Spacing(6);
            auto visible=compactCheck(data,box_value(str(spec,L"label")));visible.FontSize(data->textSize());visible.Foreground(data->brush(L"text"));
            AutomationProperties::SetAutomationId(visible,L"configure-show-"+kind);
            auto weak=weak_from_this();auto selection=make_weak(visible);
            auto toggle=[weak,selection,kind](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating){
                auto checkbox=selection.get();if(!checkbox)return;
                self->customize(O({{L"type",S(L"set_control_visible")},{L"panel",S(self->panelId)},{L"control",S(kind)},
                    {L"visible",B(checkbox.IsChecked()&&checkbox.IsChecked().Value())}}));
            }};
            visible.Checked(toggle);visible.Unchecked(toggle);
            bindings.emplace_back([weak,visible,kind]{if(auto self=weak.lock()){
                auto spec=find(array(self->panel,L"controls"),L"control",kind);auto caption=str(spec,L"label");
                if(unbox_value<hstring>(visible.Content())!=caption)visible.Content(box_value(caption));
                visible.IsChecked(flag(spec,L"visible_in_panel"));}});
            section.Children().Append(visible);
            auto body=control(kind,str(spec,L"label"));AutomationProperties::SetAutomationId(body,L"configure-control-"+kind);
            section.Children().Append(body);content.Children().Append(section);
        }
        uint32_t sectionIndex=0;
        for(auto sectionValue:array(panel,L"toolbar_options")){
            StackPanel section;section.Spacing(0);auto group=panelId+L"-"+to_hstring(sectionIndex++);
            for(auto value:sectionValue.GetArray()){
                auto spec=value.GetObject();auto index=optionModels.size();optionModels.push_back(spec);if(!object(spec,L"action").Size())continue;
                auto weak=weak_from_this();auto selected=spec.GetNamedValue(L"selected",JsonValue::CreateNullValue());
                if(selected.ValueType()==JsonValueType::Boolean){
                    RadioButton pick;pick.GroupName(group);pick.MinHeight(data->controlHeight());pick.MinWidth(0);pick.Margin({9,9,9,9});pick.Padding({0});pick.FontSize(data->textSize());pick.FontFamily(FontFamily(L"Segoe UI"));pick.Content(label(data,str(spec,L"label")));pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
                    AutomationProperties::SetAutomationId(pick,L"configure-option-"+to_hstring(index));
                    pick.Checked([weak,index](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating&&index<self->optionModels.size())self->data->dispatch(object(self->optionModels[index],L"action"));});
                    bindings.emplace_back([weak,index,pick]{if(auto self=weak.lock();self&&index<self->optionModels.size()){auto spec=self->optionModels[index];pick.Content().as<TextBlock>().Text(str(spec,L"label"));AutomationProperties::SetName(pick,str(spec,L"label"));pick.IsEnabled(flag(spec,L"enabled",true));pick.IsChecked(checked(spec));}});section.Children().Append(pick);continue;
                }
                auto pick=button(data,str(spec,L"label"),[]{});pick.MinHeight(data->controlHeight());pick.Padding({data->panelMetric(L"field_padding"),0,data->panelMetric(L"field_padding"),0});pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
                Grid line;line.ColumnSpacing(data->panelMetric(L"group_spacing"));ColumnDefinition caption;caption.Width({1,GridUnitType::Star});line.ColumnDefinitions().Append(caption);line.ColumnDefinitions().Append(ColumnDefinition());
                auto text=label(data,str(spec,L"label"));text.TextWrapping(TextWrapping::Wrap);line.Children().Append(text);auto hint=label(data,str(spec,L"hint"));hint.Opacity(.55);hint.TextTrimming(TextTrimming::CharacterEllipsis);Grid::SetColumn(hint,1);line.Children().Append(hint);pick.Content(line);
                pick.Click([weak,index](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating&&index<self->optionModels.size())self->data->dispatch(object(self->optionModels[index],L"action"));});
                bindings.emplace_back([weak,index,pick,text,hint]{if(auto self=weak.lock();self&&index<self->optionModels.size()){auto spec=self->optionModels[index];text.Text(str(spec,L"label"));hint.Text(str(spec,L"hint"));AutomationProperties::SetName(pick,str(spec,L"label"));pick.IsEnabled(flag(spec,L"enabled",true));}});
                AutomationProperties::SetAutomationId(pick,L"configure-option-"+to_hstring(index));section.Children().Append(pick);
            }
            if(section.Children().Size()){Border card;card.Background(data->brush(L"card"));card.CornerRadius(data->controlCorner());card.Child(section);clipRounded(card,data->controlCorner());content.Children().Append(card);}
        }
    }
    void apply(J const& next,bool visible){
        CapyEffects::Updating updating(data);panel=next;
        A keys;for(auto value:array(panel,L"controls"))keys.Append(S(str(value.GetObject(),L"control")));
        std::vector<J> models;for(auto section:array(panel,L"toolbar_options"))for(auto value:section.GetArray())models.push_back(value.GetObject());
        auto key=O({{L"controls",keys},{L"options",optionStructure(array(panel,L"toolbar_options"))}}).Stringify();
        if(key!=structure){structure=key;build();}else optionModels=std::move(models);
        title.Text(str(panel,L"configuration_title"));AutomationProperties::SetName(root,title.Text());
        hint.Text(str(panel,L"configuration_hint"));
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
    auto slot=impl->navigator->Placement(reference,clip,order,true);if(slot.Size())slots.Append(slot);
}
