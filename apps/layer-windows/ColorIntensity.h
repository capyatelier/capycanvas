#pragma once
#include "ColorEditor.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

namespace CapyUi {
struct ColorIntensity:std::enable_shared_from_this<ColorIntensity>{
    std::shared_ptr<WorkspaceData> data;
    Canvas root;
    Shapes::Path track;
    Shapes::Line zero;
    Shapes::Ellipse halo,marker,focus;
    Slider access;
    std::vector<Shapes::Line> ramp;
    std::vector<SolidColorBrush> rampInks;
    SolidColorBrush markerInk=fill({255,0,0,0});
    std::function<void(double)> changed;
    double size=0,minimum=-2,maximum=6;
    bool syncing=false;
    hstring key;
    void init(hstring const& id){
        auto weak=weak_from_this();
        track.Stroke(clear());track.StrokeStartLineCap(PenLineCap::Round);track.StrokeEndLineCap(PenLineCap::Round);root.Children().Append(track);
        halo.Stroke(fill({128,0,0,0}));halo.StrokeThickness(4);marker.Stroke(fill({255,255,255,255}));marker.StrokeThickness(2);marker.Fill(markerInk);
        focus.StrokeThickness(2);focus.Opacity(.9);zero.StrokeThickness(1);zero.Opacity(.9);
        for(UIElement const& element:{UIElement(halo),UIElement(marker),UIElement(focus),UIElement(zero)}){element.IsHitTestVisible(false);root.Children().Append(element);}
        access.Minimum(minimum);access.Maximum(maximum);access.StepFrequency(.01);access.SmallChange(.1);access.LargeChange(1);
        access.Width(1);access.Height(1);access.Opacity(0);access.IsHitTestVisible(false);root.Children().Append(access);
        copyName(data,access,data->copyCaption(L"color",L"intensity"));AutomationProperties::SetAutomationId(access,id);
        access.ValueChanged([weak](auto&&,Primitives::RangeBaseValueChangedEventArgs const& e){if(auto self=weak.lock();self&&!self->syncing&&self->access.IsEnabled())self->changed(e.NewValue());});
        access.PreviewKeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->access.IsEnabled()&&e.Key()==Windows::System::VirtualKey::Home){self->changed(0);e.Handled(true);}});
        auto refreshFocus=[weak](auto&&,auto&&){if(auto self=weak.lock())self->focus.Visibility(self->access.FocusState()==FocusState::Keyboard?Visibility::Visible:Visibility::Collapsed);};
        access.GotFocus(refreshFocus);access.LostFocus(refreshFocus);focus.Visibility(Visibility::Collapsed);
        track.DoubleTapped([weak](auto&&,DoubleTappedRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->access.IsEnabled()){self->changed(0);e.Handled(true);}});
        track.PointerPressed([weak](auto&&,auto&&){if(auto self=weak.lock())self->access.Focus(FocusState::Pointer);});
    }
    J at(double fraction)const{return colorUi(data->localization.get(),O({{L"type",S(L"arc")},{L"size",N(size)},{L"fraction",N(fraction)}})).GetObject();}
    double pick(Windows::Foundation::Point point)const{
        A position;position.Append(N(point.X));position.Append(N(point.Y));
        return colorUi(data->localization.get(),O({{L"type",S(L"intensity_point")},{L"size",N(size)},{L"point",position},{L"minimum",N(minimum)},{L"maximum",N(maximum)}})).GetNumber();
    }
    void layout(double extent,A const& range){
        double low=range.Size()==2?range.GetNumberAt(0):-2,high=range.Size()==2?range.GetNumberAt(1):6;
        if(size==extent&&minimum==low&&maximum==high)return;
        bool resized=size!=extent;size=extent;minimum=low;maximum=high;key=L"";
        syncing=true;access.Minimum(std::min(low,access.Maximum()));access.Maximum(high);access.Minimum(low);syncing=false;
        auto start=at(0),end=at(1),geometry=object(start,L"geometry");
        double radius=num(geometry,L"radius"),width=num(geometry,L"width"),markerRadius=num(geometry,L"marker_radius");
        auto from=array(start,L"point"),to=array(end,L"point");
        if(resized){PathFigure figure;figure.StartPoint({float(from.GetNumberAt(0)),float(from.GetNumberAt(1))});figure.IsClosed(false);figure.IsFilled(false);
        ArcSegment segment;segment.Point({float(to.GetNumberAt(0)),float(to.GetNumberAt(1))});segment.Size({float(radius),float(radius)});segment.SweepDirection(SweepDirection::Counterclockwise);figure.Segments().Append(segment);
        PathGeometry path;path.Figures().Append(figure);track.Data(path);track.StrokeThickness(width);
        auto points=array(start,L"path");size_t count=points.Size()?points.Size()-1:0;
        while(ramp.size()<count){Shapes::Line line;auto ink=fill({255,0,0,0});line.Stroke(ink);line.StrokeStartLineCap(PenLineCap::Round);line.StrokeEndLineCap(PenLineCap::Round);line.IsHitTestVisible(false);root.Children().InsertAt(uint32_t(ramp.size()),line);ramp.push_back(line);rampInks.push_back(ink);}
        for(uint32_t i=0;i<ramp.size();++i){auto line=ramp[i];line.Visibility(i<count?Visibility::Visible:Visibility::Collapsed);if(i>=count)continue;auto a=points.GetArrayAt(i),b=points.GetArrayAt(i+1);line.X1(a.GetNumberAt(0));line.Y1(a.GetNumberAt(1));line.X2(b.GetNumberAt(0));line.Y2(b.GetNumberAt(1));line.StrokeThickness(width);}
        halo.Width(markerRadius*2+4);halo.Height(markerRadius*2+4);marker.Width(markerRadius*2+2);marker.Height(markerRadius*2+2);focus.Width((markerRadius+5)*2);focus.Height((markerRadius+5)*2);}
        auto origin=array(geometry,L"center"),point=array(at((0-minimum)/(maximum-minimum)),L"point");
        double x=point.GetNumberAt(0),y=point.GetNumberAt(1),dx=(x-origin.GetNumberAt(0))/radius,dy=(y-origin.GetNumberAt(1))/radius;
        zero.X1(x+dx*(width*.5+2));zero.Y1(y+dy*(width*.5+2));zero.X2(x+dx*(width*.5+5));zero.Y2(y+dy*(width*.5+5));
    }
    void present(J const& view,double extent,bool visible,bool enabled=true){
        root.Visibility(visible?Visibility::Visible:Visibility::Collapsed);if(!visible)return;
        root.Opacity(enabled?1:.36);access.IsEnabled(enabled);track.IsHitTestVisible(enabled);
        layout(extent,array(view,L"intensity_range"));
        auto next=view.Stringify()+data->theme();if(next==key)return;key=next;
        auto colors=array(view,L"intensity_ramp");for(uint32_t i=0;i<rampInks.size()&&i<colors.Size();++i)rampInks[i].Color(previewColor(colors.GetArrayAt(i)));
        double stops=num(view,L"intensity");auto point=array(at((stops-minimum)/(maximum-minimum)),L"point");
        for(auto const& dot:{halo,marker,focus}){Canvas::SetLeft(dot,point.GetNumberAt(0)-dot.Width()/2);Canvas::SetTop(dot,point.GetNumberAt(1)-dot.Height()/2);}
        markerInk.Color(previewColor(array(view,L"marker_color")));zero.Stroke(data->brush(L"text"));focus.Stroke(data->brush(L"text"));
        syncing=true;access.Value(stops);syncing=false;
        wchar_t text[32];swprintf(text,32,L"%.2f EV",stops);AutomationProperties::SetItemStatus(access,text);AutomationProperties::SetItemStatus(track,text);
    }
};
}
