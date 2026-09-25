#include "pch.h"
#include "EffectControls.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
using namespace CapyEffects;
namespace {
Windows::UI::Color rgba(A const& a){
    if(a.Size()!=4)return {};
    auto byte=[&](int i){return uint8_t(std::round(std::clamp(a.GetNumberAt(i),0.,1.)*255));};
    return {byte(3),byte(0),byte(1),byte(2)};
}
struct ColorEditor : std::enable_shared_from_this<ColorEditor> {
    std::shared_ptr<Property> property;
    std::function<J()> get;
    std::function<void(J)> set;
    std::function<hstring()> context;
    StackPanel root,fields;
    Bindings numbers;
    SolidColorBrush sample{Windows::UI::Color{}};
    hstring editingContext;
    std::shared_ptr<ColorForm> form;
    void rebuild(){
        fields.Children().Clear();form=std::make_shared<ColorForm>();auto weak=weak_from_this();
        form->init([weak,expected=editingContext](J value){if(auto self=weak.lock();self&&(!self->context||self->context()==expected))self->set(value);},property->id()+L"-color");
        fields.Children().Append(form->root);
    }
    void init(hstring const& title){
        root.Spacing(6);fields.Spacing(6);fields.Visibility(Visibility::Collapsed);
        bool swatchOnly=object(property->model(),L"color_action").Size()!=0;
        Grid row;ColumnDefinition text;text.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(text);
        if(!swatchOnly){
            ColumnDefinition swatch;swatch.Width({48,GridUnitType::Pixel});row.ColumnDefinitions().Append(swatch);
            auto name=label(property->data,title);name.VerticalAlignment(VerticalAlignment::Center);row.Children().Append(name);
        }
        auto weak=weak_from_this();
        auto pick=button(property->data,title,[weak]{if(auto self=weak.lock()){
            self->fields.Visibility(self->fields.Visibility()==Visibility::Visible?Visibility::Collapsed:Visibility::Visible);
        }});
        pick.Height(swatchOnly?36:28);pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.Background(property->data->brush(L"input"));
        Shapes::Rectangle color;color.Fill(sample);color.Margin({2,2,2,2});pick.Content(color);
        pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);pick.VerticalContentAlignment(VerticalAlignment::Stretch);
        AutomationProperties::SetAutomationId(pick,property->id()+L"-color");
        Grid::SetColumn(pick,swatchOnly?0:1);row.Children().Append(pick);root.Children().Append(row);root.Children().Append(fields);
        rebuild();
    }
    void refresh(){
        auto next=context?context():L"";
        if(next!=editingContext){editingContext=next;rebuild();}
        auto space=str(object(property->data->model,L"color_panel"),L"rgb_space",L"Srgb");
        form->load(get(),space,object(property->data->model,L"color_panel"));
        sample.Color(displayColor(object(form->view,L"preview")));
    }
};
struct PropertiesView : std::enable_shared_from_this<PropertiesView> {
    std::shared_ptr<WorkspaceData> data;
    StackPanel root,body;
    ContentControl bodyGate;
    TextBlock title;
    Bindings fields;
    hstring schema;
    explicit PropertiesView(std::shared_ptr<WorkspaceData> source):data(std::move(source)){
        root.Spacing(6);body.Spacing(6);title=label(data,L"",true);
        AutomationProperties::SetAutomationId(root,L"layer-properties");AutomationProperties::SetName(root,L"Layer properties");
        bodyGate.Content(body);bodyGate.IsTabStop(false);bodyGate.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        root.Children().Append(title);root.Children().Append(bodyGate);
    }
    void refresh(){
        Updating updating(data);
        auto view=object(data->state,L"layer_properties");
        title.Text(str(view,L"title"));ToolTipService::SetToolTip(title,box_value(str(view,L"description")));
        A keys;for(auto value:array(view,L"controls")){
            auto c=value.GetObject();keys.Append(O({{L"key",S(str(c,L"key"))},{L"label",S(str(c,L"label"))},
                {L"section",S(str(c,L"section"))},{L"kind",object(c,L"kind")},{L"color_action",object(c,L"color_action")}}));
        }
        auto next=O({{L"epoch",N(num(object(data->state,L"document_file"),L"epoch"))},
            {L"layer",view.GetNamedValue(L"layer",JsonValue::CreateNullValue())},{L"controls",keys}}).Stringify();
        if(next!=schema){
            schema=next;fields.clear();body.Children().Clear();
            std::vector<std::pair<hstring,FrameworkElement>> curves;
            auto controls=array(view,L"controls");hstring section;
            for(auto value:controls){
                auto c=value.GetObject();auto property=std::make_shared<Property>(data,c);
                auto kind=object(c,L"kind");auto type=str(kind,L"kind");auto name=str(c,L"label");
                auto nextSection=str(c,L"section");
                if(section!=nextSection){section=nextSection;
                    if(body.Children().Size()){Shapes::Rectangle line;line.Height(1);line.Opacity(.15);line.Fill(data->brush(L"text"));line.Margin({0,3,0,3});body.Children().Append(line);}
                    if(!section.empty()){auto heading=label(data,section,true);heading.Margin({6,0,0,0});body.Children().Append(heading);}
                }
                if(type==L"number"){
                    body.Children().Append(number(data,name,object(kind,L"numeric"),
                        [property]{return num(object(property->model(),L"value"),L"value");},
                        [property](double v){property->set(N(v));},fields,nullptr,false,property->id()));
                }else if(type==L"choice"){
                    Grid row;row.ColumnSpacing(6);
                    ColumnDefinition caption;caption.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(caption);
                    ColumnDefinition choiceColumn;choiceColumn.Width({1,GridUnitType::Auto});row.ColumnDefinitions().Append(choiceColumn);
                    auto text=label(data,name);text.VerticalAlignment(VerticalAlignment::Center);row.Children().Append(text);
                    ComboBox choices;choices.MinWidth(0);choices.MinHeight(32);choices.Height(32);
                    choices.HorizontalAlignment(HorizontalAlignment::Right);choices.Padding({6,0,0,0});
                    choices.FontSize(data->textSize());choices.FontWeight(Windows::UI::Text::FontWeights::Bold());
                    choices.Background(data->brush(L"input"));choices.BorderThickness({0,0,0,0});choices.CornerRadius({6,6,6,6});
                    row.SizeChanged([weak=make_weak(choices)](auto&&,SizeChangedEventArgs const& event){
                        if(auto choices=weak.get())choices.MaxWidth(std::max(48.f,event.NewSize().Width*.6f));
                    });
                    for(auto option:array(kind,L"options"))choices.Items().Append(box_value(option.GetString()));
                    AutomationProperties::SetName(choices,name);AutomationProperties::SetAutomationId(choices,property->id());
                    choices.SelectionChanged([property,weak=make_weak(choices)](auto&&,auto&&){if(auto choices=weak.get();choices&&choices.SelectedIndex()>=0)property->set(N(choices.SelectedIndex()));});
                    fields.emplace_back([property,choices]{choices.SelectedIndex(int(num(object(property->model(),L"value"),L"value")));});
                    Grid::SetColumn(choices,1);row.Children().Append(choices);body.Children().Append(row);
                }else if(type==L"toggle"){
                    CheckBox check;check.Content(label(data,name));check.MinHeight(32);check.MinWidth(0);
                    AutomationProperties::SetName(check,name);AutomationProperties::SetAutomationId(check,property->id());
                    check.Click([property,weak=make_weak(check)](auto&&,auto&&){if(auto c=weak.get())property->set(B(c.IsChecked().Value()));});
                    fields.emplace_back([property,check]{check.IsChecked(flag(object(property->model(),L"value"),L"value"));});body.Children().Append(check);
                }else if(type==L"color"){
                    auto color=ColorField(property,name,[property]{return object(object(property->model(),L"value"),L"value");},
                        [property](J a){property->set(a);},fields);
                    auto action=object(c,L"color_action");
                    if(action.Size()){
                        Grid row;row.ColumnSpacing(6);
                        ColumnDefinition fill;fill.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(fill);
                        ColumnDefinition actionColumn;actionColumn.Width({36,GridUnitType::Pixel});row.ColumnDefinitions().Append(actionColumn);
                        row.Children().Append(color);
                        auto bucket=button(data,L"Use selected color",[property,action]{if(property->current()&&!property->data->updating&&flag(property->view(),L"enabled"))property->data->dispatchDocument(action,to_hstring(uint64_t(property->epoch)));});
                        bucket.Content(icon(L"fill",data->theme()));bucket.Height(36);bucket.VerticalAlignment(VerticalAlignment::Top);
                        ToolTipService::SetToolTip(bucket,box_value(L"Use selected color"));AutomationProperties::SetAutomationId(bucket,L"paper-color-bucket");Grid::SetColumn(bucket,1);row.Children().Append(bucket);body.Children().Append(row);
                    }else body.Children().Append(color);
                }else if(type==L"curve"){
                    curves.emplace_back(name,CurveField(property,fields));
                }else if(type==L"gradient"){
                    body.Children().Append(GradientField(property,fields));
                }
            }
            if(!curves.empty()){
                ComboBox channel;channel.MinWidth(0);channel.MinHeight(32);channel.HorizontalAlignment(HorizontalAlignment::Stretch);
                channel.FontSize(data->textSize());channel.Background(data->brush(L"input"));
                AutomationProperties::SetName(channel,L"Curve channel");AutomationProperties::SetAutomationId(channel,L"property-curve-channel");
                for(auto const& [name,graph]:curves)channel.Items().Append(box_value(name));
                Grid plots;for(auto const& [name,graph]:curves){graph.Visibility(Visibility::Collapsed);plots.Children().Append(graph);}
                channel.SelectionChanged([curves,weak=make_weak(channel)](auto&&,auto&&){if(auto select=weak.get()){
                    for(size_t i=0;i<curves.size();i++)curves[i].second.Visibility(int(i)==select.SelectedIndex()?Visibility::Visible:Visibility::Collapsed);
                }});
                channel.SelectedIndex(0);body.Children().InsertAt(0,plots);body.Children().InsertAt(0,channel);
            }
        }
        bodyGate.IsEnabled(flag(view,L"enabled"));body.Opacity(flag(view,L"enabled")?1.:.4);
        for(auto const& update:fields)update();
    }
};
}
FrameworkElement CapyEffects::ColorField(std::shared_ptr<Property> const& property,hstring const& title,
    std::function<J()> get,std::function<void(J)> set,Bindings& bindings,std::function<hstring()> context){
    auto editor=std::make_shared<ColorEditor>();editor->property=property;editor->get=std::move(get);editor->set=std::move(set);editor->context=std::move(context);
    editor->init(title);bindings.emplace_back([editor]{editor->refresh();});return editor->root;
}
FrameworkElement PropertiesPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<PropertiesView>(data);bindings.emplace_back([view]{view->refresh();});return view->root;
}
