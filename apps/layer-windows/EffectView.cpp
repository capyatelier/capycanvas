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
    std::function<A()> get;
    std::function<void(A)> set;
    std::function<hstring()> context;
    StackPanel root,fields;
    Bindings numbers;
    SolidColorBrush sample{Windows::UI::Color{}};
    hstring editingContext;
    void rebuild(){
        numbers.clear();fields.Children().Clear();
        auto weak=weak_from_this();
        std::array<hstring,4> labels{L"Red",L"Green",L"Blue",L"Alpha"};
        for(int i=0;i<4;i++)fields.Children().Append(number(property->data,labels[i],object(property->data->catalog,L"opacity"),
            [weak,i]{if(auto self=weak.lock()){auto a=self->get();if(a.Size()==4)return a.GetNumberAt(i);}return 0.;},
            [weak,i,expected=editingContext](double value){if(auto self=weak.lock();self&&(!self->context||self->context()==expected)){
                auto a=A::Parse(self->get().Stringify());if(a.Size()==4){a.SetAt(i,N(value));self->set(a);}
            }},numbers,nullptr,false,property->id()+L"-color-"+to_hstring(i)));
    }
    void init(hstring const& title){
        root.Spacing(6);fields.Spacing(6);fields.Visibility(Visibility::Collapsed);
        Grid row;ColumnDefinition text;text.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(text);
        ColumnDefinition swatch;swatch.Width({48,GridUnitType::Pixel});row.ColumnDefinitions().Append(swatch);
        auto name=label(property->data,title);name.VerticalAlignment(VerticalAlignment::Center);row.Children().Append(name);
        auto weak=weak_from_this();
        auto pick=button(property->data,title,[weak]{if(auto self=weak.lock()){
            self->fields.Visibility(self->fields.Visibility()==Visibility::Visible?Visibility::Collapsed:Visibility::Visible);
        }});
        pick.Height(28);pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.Background(property->data->brush(L"input"));
        Shapes::Rectangle color;color.Fill(sample);color.Margin({2,2,2,2});pick.Content(color);
        pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);pick.VerticalContentAlignment(VerticalAlignment::Stretch);
        AutomationProperties::SetAutomationId(pick,property->id()+L"-color");
        Grid::SetColumn(pick,1);row.Children().Append(pick);root.Children().Append(row);root.Children().Append(fields);
        rebuild();
    }
    void refresh(){
        auto next=context?context():L"";
        if(next!=editingContext){editingContext=next;rebuild();}
        sample.Color(rgba(get()));
        for(auto const& update:numbers)update();
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
                {L"section",S(str(c,L"section"))},{L"kind",object(c,L"kind")}}));
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
                    StackPanel row;row.Spacing(3);row.Children().Append(label(data,name));
                    ComboBox choices;choices.MinWidth(0);choices.MinHeight(32);choices.HorizontalAlignment(HorizontalAlignment::Stretch);
                    choices.FontSize(data->textSize());choices.Background(data->brush(L"input"));
                    for(auto option:array(kind,L"options"))choices.Items().Append(box_value(option.GetString()));
                    AutomationProperties::SetName(choices,name);AutomationProperties::SetAutomationId(choices,property->id());
                    choices.SelectionChanged([property,weak=make_weak(choices)](auto&&,auto&&){if(auto choices=weak.get();choices&&choices.SelectedIndex()>=0)property->set(N(choices.SelectedIndex()));});
                    fields.emplace_back([property,choices]{choices.SelectedIndex(int(num(object(property->model(),L"value"),L"value")));});
                    row.Children().Append(choices);body.Children().Append(row);
                }else if(type==L"toggle"){
                    CheckBox check;check.Content(label(data,name));check.MinHeight(32);check.MinWidth(0);
                    AutomationProperties::SetName(check,name);AutomationProperties::SetAutomationId(check,property->id());
                    check.Click([property,weak=make_weak(check)](auto&&,auto&&){if(auto c=weak.get())property->set(B(c.IsChecked().Value()));});
                    fields.emplace_back([property,check]{check.IsChecked(flag(object(property->model(),L"value"),L"value"));});body.Children().Append(check);
                }else if(type==L"color"){
                    body.Children().Append(ColorField(property,name,[property]{return array(object(property->model(),L"value"),L"value");},
                        [property](A a){property->set(a);},fields));
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
    std::function<A()> get,std::function<void(A)> set,Bindings& bindings,std::function<hstring()> context){
    auto editor=std::make_shared<ColorEditor>();editor->property=property;editor->get=std::move(get);editor->set=std::move(set);editor->context=std::move(context);
    editor->init(title);bindings.emplace_back([editor]{editor->refresh();});return editor->root;
}
FrameworkElement PropertiesPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<PropertiesView>(data);bindings.emplace_back([view]{view->refresh();});return view->root;
}
