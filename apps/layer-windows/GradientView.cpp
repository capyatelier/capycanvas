#include "pch.h"
#include "EffectControls.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
using namespace CapyEffects;
namespace {
struct GradientEditor : std::enable_shared_from_this<GradientEditor> {
    std::shared_ptr<Property> property;
    StackPanel root,positionFields;
    ContentControl positionGate;
    Canvas bar,dots;
    Shapes::Rectangle ramp;
    LinearGradientBrush brush;
    Button remove{nullptr};
    Bindings fields,positionBindings;
    int selected=0;
    uint32_t generation=0;
    hstring drawn,positionContext;
    A stops()const{return array(object(property->model(),L"value"),L"value");}
    J stop()const{auto all=stops();return all.Size()?all.GetObjectAt(std::clamp(selected,0,int(all.Size())-1)):J{};}
    hstring context()const{return to_hstring(selected)+L"/"+to_hstring(stops().Size())+L"/"+to_hstring(generation);}
    void change(int index,double position,V color=JsonValue::CreateNullValue(),bool erase=false){
        property->action(O({{L"op",S(L"gradient_stop")},{L"index",index<0?JsonValue::CreateNullValue():N(index)},
            {L"position",N(position)},{L"color",color},{L"remove",B(erase)}}));
    }
    void add(double at){
        auto all=stops();selected=0;
        for(uint32_t i=0;i<all.Size();i++){
            double x=num(all.GetObjectAt(i),L"position");
            if(std::abs(x-at)<=.002){selected=i;refresh();return;}
            if(x<at)selected++;
        }
        if(all.Size()<32)change(-1,at);
    }
    void addMiddle(){
        auto all=stops();double gap=0,at=.5;
        for(uint32_t i=1;i<all.Size();i++){double a=num(all.GetObjectAt(i-1),L"position"),b=num(all.GetObjectAt(i),L"position");if(b-a>gap){gap=b-a;at=(a+b)/2;}}
        add(at);
    }
    void init(){
        auto data=property->data;auto weak=weak_from_this();root.Spacing(6);
        bar.Height(44);bar.Background(clear());bar.Children().Append(ramp);bar.Children().Append(dots);dots.IsHitTestVisible(false);
        brush.StartPoint({0,0});brush.EndPoint({1,0});brush.ColorInterpolationMode(ColorInterpolationMode::SRgbLinearInterpolation);
        ramp.Fill(brush);ramp.Height(32);Canvas::SetLeft(ramp,6);
        AutomationProperties::SetName(bar,L"Gradient");AutomationProperties::SetAutomationId(bar,property->id()+L"-gradient");
        bar.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
        bar.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->bar);
            if(!p.IsInContact()||(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed())||self->bar.ActualWidth()<=12)return;
            double width=self->bar.ActualWidth()-12,at=std::clamp((p.Position().X-6)/width,0.,1.);auto all=self->stops();
            for(uint32_t i=0;i<all.Size();i++)if(std::abs(num(all.GetObjectAt(i),L"position")-at)*width<12){
                self->selected=i;self->refresh();e.Handled(true);return;
            }
            self->add(at);e.Handled(true);
        }});
        positionGate.Content(positionFields);positionGate.IsTabStop(false);positionGate.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        root.Children().Append(bar);root.Children().Append(positionGate);
        root.Children().Append(ColorField(property,L"Color",[weak]{if(auto self=weak.lock())return array(self->stop(),L"color");return A{};},
            [weak](A color){if(auto self=weak.lock())self->change(self->selected,num(self->stop(),L"position"),color);},fields,
            [weak]{if(auto self=weak.lock())return self->context();return hstring{};}));
        StackPanel actions;actions.Orientation(Orientation::Horizontal);actions.Spacing(6);
        auto add=button(data,L"Add stop",[weak]{if(auto self=weak.lock())self->addMiddle();});
        remove=button(data,L"Remove stop",[weak]{if(auto self=weak.lock()){int index=self->selected;self->selected=std::max(0,index-1);self->change(index,0,JsonValue::CreateNullValue(),true);}});
        auto reset=button(data,L"Reset gradient",[weak]{if(auto self=weak.lock()){self->selected=0;++self->generation;self->property->reset();self->refresh();}});
        int i=0;for(auto pick:{add,remove,reset}){
            pick.Height(28);pick.Width(28);pick.Content(icon(std::array<hstring,3>{L"plus",L"minus",L"undo"}[i],data->theme()));
            ToolTipService::SetToolTip(pick,box_value(AutomationProperties::GetName(pick)));
            AutomationProperties::SetAutomationId(pick,property->id()+L"-"+std::array<hstring,3>{L"add",L"remove",L"reset"}[i++]);actions.Children().Append(pick);
        }
        fields.emplace_back([weak,add]{if(auto self=weak.lock())add.IsEnabled(self->stops().Size()<32);});
        root.Children().Append(actions);
    }
    void refresh(){
        auto data=property->data;Updating updating(data);auto all=stops();if(all.Size()<2)return;
        selected=std::clamp(selected,0,int(all.Size())-1);
        auto nextContext=context();
        if(nextContext!=positionContext){
            positionContext=nextContext;positionBindings.clear();positionFields.Children().Clear();auto weak=weak_from_this();
            positionFields.Children().Append(number(data,L"Position",object(data->catalog,L"opacity"),
                [weak]{if(auto self=weak.lock())return num(self->stop(),L"position");return 0.;},
                [weak,expected=positionContext](double value){if(auto self=weak.lock();self&&self->context()==expected)self->change(self->selected,value);},
                positionBindings,nullptr,false,property->id()+L"-position"));
        }
        bool interior=selected>0&&selected+1<int(all.Size());positionGate.IsEnabled(interior);remove.IsEnabled(interior);
        for(auto const& update:positionBindings)update();for(auto const& update:fields)update();
        double width=std::max(0.,bar.ActualWidth()-12);
        auto next=O({{L"stops",all},{L"width",N(width)},{L"selected",N(selected)}}).Stringify();
        if(next==drawn)return;drawn=next;ramp.Width(width);brush.GradientStops().Clear();dots.Children().Clear();
        for(uint32_t i=0;i<all.Size();i++){
            auto stop=all.GetObjectAt(i);auto color=array(stop,L"color");
            auto byte=[&](int index){return uint8_t(std::round(std::clamp(color.GetNumberAt(index),0.,1.)*255));};
            GradientStop entry;entry.Offset(num(stop,L"position"));entry.Color({byte(3),byte(0),byte(1),byte(2)});brush.GradientStops().Append(entry);
            double radius=int(i)==selected?4:2.5;Shapes::Ellipse dot;dot.Width(radius*2);dot.Height(radius*2);dot.Fill(data->brush(L"text"));
            Canvas::SetLeft(dot,6+num(stop,L"position")*width-radius);Canvas::SetTop(dot,39-radius);dots.Children().Append(dot);
        }
    }
};
}
FrameworkElement CapyEffects::GradientField(std::shared_ptr<Property> const& property,Bindings& bindings){
    auto view=std::make_shared<GradientEditor>();view->property=property;view->init();
    bindings.emplace_back([view]{view->refresh();});return view->root;
}
