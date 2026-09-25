#include "pch.h"
#include "RangeControl.h"

using namespace CapyUi;
namespace {
double valueWidth(std::shared_ptr<WorkspaceData> const& data,J const& spec){
    double widest=0;
    for(auto key:{L"min",L"max"}){
        std::wstring text=str(numeric(spec,num(spec,key),O({{L"type",S(L"format")}})),L"text").c_str();
        for(auto& ch:text)if(ch>=L'0'&&ch<=L'9')ch=L'8';
        TextBlock measure;measure.FontSize(data->textSize());measure.FontFamily(FontFamily(L"Segoe UI"));measure.Text(text);
        measure.Measure({1000,1000});widest=std::max(widest,double(measure.DesiredSize().Width));
    }
    return std::ceil(widest)+12;
}
}
std::shared_ptr<RangeControl> RangeControl::Create(std::shared_ptr<WorkspaceData> const& data,J const& lower,J const& upper,
    hstring const& label,hstring const& prefix,bool showTrack,std::function<void(int,double)> change){
    auto self=std::make_shared<RangeControl>();
    self->data=data;self->bounds={lower,upper};self->label=label;self->prefix=prefix;self->change=std::move(change);
    self->values={num(lower,L"value"),num(upper,L"value")};
    self->init(showTrack);return self;
}
void RangeControl::init(bool showTrack){
    auto weak=weak_from_this();
    root.Height(28);root.ColumnSpacing(showTrack?4:0);root.MinWidth(0);
    AutomationProperties::SetName(root,label);AutomationProperties::SetAutomationId(root,prefix+L"-range");
    for(auto star:{false,true,false}){
        ColumnDefinition column;
        column.Width(!star?GridLength{1,GridUnitType::Auto}:showTrack?GridLength{1,GridUnitType::Star}:GridLength{4,GridUnitType::Pixel});
        root.ColumnDefinitions().Append(column);
    }
    for(int i=0;i<2;i++){
        auto field=bounds[i];auto spec=object(field,L"numeric");
        auto control=number(data,str(field,L"label")+L" — "+label,spec,
            [weak,i]{if(auto self=weak.lock())return self->values[i];return 0.;},
            [weak,i](double value){if(auto self=weak.lock())self->set(i,value);},
            fields,nullptr,true,prefix+L"-"+str(field,L"id"));
        control.Width(valueWidth(data,spec));control.VerticalAlignment(VerticalAlignment::Center);
        Grid::SetColumn(control,i?2:0);root.Children().Append(control);
    }
    track.Height(28);track.MinWidth(64);track.Background(clear());track.ManipulationMode(ManipulationModes::None);
    track.Visibility(showTrack?Visibility::Visible:Visibility::Collapsed);
    AutomationProperties::SetAutomationId(track,prefix+L"-range-track");AutomationProperties::SetName(track,label);
    for(auto part:{trough,fill}){part.Height(4);Canvas::SetTop(part,12);part.IsHitTestVisible(false);track.Children().Append(part);}
    trough.Background(data->tint(L"text",51));fill.Background(data->tint(L"text",166));
    for(auto& thumb:thumbs){
        thumb.Width(6);thumb.Height(14);Canvas::SetTop(thumb,7);thumb.CornerRadius({2,2,2,2});
        thumb.Background(data->brush(L"text"));thumb.IsHitTestVisible(false);track.Children().Append(thumb);
    }
    Grid::SetColumn(track,1);root.Children().Append(track);
    track.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->paint();});
    track.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
        auto point=e.GetCurrentPoint(self->track);
        if(self->contact||self->retired||(point.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!point.Properties().IsLeftButtonPressed()))return;
        double width=self->track.ActualWidth(),x=point.Position().X;
        std::array<double,2> positions{self->position(self->values[0],width),self->position(self->values[1],width)};
        int index=std::abs(positions[1]-positions[0])<1?int(x>=positions[0]):int(std::abs(x-positions[1])<std::abs(x-positions[0]));
        if(!self->track.CapturePointer(e.Pointer()))return;
        self->contact=Contact{point.PointerId(),index,self->values[index],self->domain,std::abs(x-positions[index])<=12?x-positions[index]:0};
        self->pick(x);e.Handled(true);
    }});
    track.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->contact&&self->contact->id==e.Pointer().PointerId()){
        self->pick(e.GetCurrentPoint(self->track).Position().X);e.Handled(true);
    }});
    track.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->contact&&self->contact->id==e.Pointer().PointerId()){
        self->end(false);e.Handled(true);
    }});
    for(auto kind:{0,1}){
        auto cancel=[weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->contact&&self->contact->id==e.Pointer().PointerId())self->end(true);};
        if(kind)track.PointerCanceled(cancel);else track.PointerCaptureLost(cancel);
    }
    root.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->contact&&e.Key()==winrt::Windows::System::VirtualKey::Escape){
        self->end(true);e.Handled(true);
    }});
    paint();
}
double RangeControl::position(double value,double width)const{
    return 8+std::clamp((value-domain[0])/std::max(1e-9,domain[1]-domain[0]),0.,1.)*std::max(0.,width-16);
}
void RangeControl::paint(){
    auto spec=object(bounds[0],L"numeric");
    domain=contact?contact->domain:std::array<double,2>{std::min(num(spec,L"soft_min",num(spec,L"min")),values[0]),
        std::max(num(spec,L"soft_max",num(spec,L"max")),values[1])};
    double width=track.ActualWidth();if(width<=0)return;
    double lower=position(values[0],width),upper=position(values[1],width);
    trough.Width(std::max(0.,width-16));Canvas::SetLeft(trough,8);
    fill.Width(std::max(0.,upper-lower));Canvas::SetLeft(fill,lower);
    Canvas::SetLeft(thumbs[0],lower-7);Canvas::SetLeft(thumbs[1],upper+1);
}
void RangeControl::set(int index,double value){
    if(retired)return;
    auto resolved=num(numeric(object(bounds[0],L"numeric"),values[index],O({{L"type",S(L"value")},{L"value",N(value)}})),L"value");
    double next=index?std::max(values[0],resolved):std::min(values[1],resolved);
    if(next==values[index])return;
    values[index]=next;paint();
    for(auto const& bind:fields)bind();
    change(index,next);
}
void RangeControl::pick(double x){
    if(!contact)return;
    auto control=J::Parse(object(bounds[0],L"numeric").Stringify());
    control.SetNamedValue(L"soft_min",N(contact->domain[0]));control.SetNamedValue(L"soft_max",N(contact->domain[1]));
    double width=track.ActualWidth();
    auto result=numeric(control,contact->before,O({{L"type",S(L"position")},{L"position",N((x-contact->offset-8)/std::max(1.,width-16))}}));
    set(contact->index,num(result,L"value"));
}
void RangeControl::end(bool cancel){
    if(!contact)return;
    auto finished=*contact;contact.reset();
    if(cancel)set(finished.index,finished.before);
    track.ReleasePointerCaptures();paint();
}
void RangeControl::Update(double lower,double upper){
    if(contact)return;
    values={lower,upper};paint();
    for(auto const& bind:fields)bind();
}
void RangeControl::Dispose(){retired=true;contact.reset();track.ReleasePointerCaptures();}
