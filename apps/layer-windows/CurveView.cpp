#include "pch.h"
#include "EffectControls.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <cmath>
#include <optional>
using namespace CapyEffects;
using Windows::Foundation::Point;
namespace {
struct CurveEditor : std::enable_shared_from_this<CurveEditor> {
    std::shared_ptr<Property> property;
    StackPanel root;
    Grid chart;
    ContentControl focus;
    Canvas graph,grid,dots;
    Shapes::Polyline line;
    Button reset{nullptr};
    std::optional<uint32_t> pointer;
    std::optional<std::pair<int,uint64_t>> lastTap;
    Point press{},start{};
    int selected=0,pressed=-1;
    bool removable=false,detached=false;
    hstring drawn;
    A points()const{return array(object(property->model(),L"value"),L"value");}
    A point()const{auto p=points();return p.Size()?p.GetArrayAt(std::clamp(selected,0,int(p.Size())-1)):values({0,0});}
    void change(int index,Point p,bool removePoint=false,hstring const& phase={}){
        property->action(O({{L"op",S(L"curve_point")},{L"index",index<0?JsonValue::CreateNullValue():N(index)},
            {L"point",values({p.X,p.Y})},{L"remove",B(removePoint)}}),phase);
    }
    Point position(Point p)const{
        return {float(p.X/graph.ActualWidth()),float(1-p.Y/graph.ActualHeight())};
    }
    Point dragged(Point p)const{
        auto at=position(p);return {start.X+(at.X-press.X),start.Y+(at.Y-press.Y)};
    }
    static bool offGraph(Point p){
        constexpr float margin=.1f;
        return p.X<-margin||p.X>1+margin||p.Y<-margin||p.Y>1+margin;
    }
    void cancel(){
        if(!pointer)return;
        pointer.reset();detached=false;change(selected,{},false,L"cancel");graph.ReleasePointerCaptures();
    }
    ~CurveEditor(){cancel();}
    void erase(){auto p=points();if(selected>0&&selected+1<int(p.Size())){int old=selected--;change(old,{},true);}}
    void init(){
        auto data=property->data;auto weak=weak_from_this();root.Spacing(6);
        graph.Background(data->brush(L"input"));graph.Children().Append(grid);graph.Children().Append(line);graph.Children().Append(dots);
        grid.IsHitTestVisible(false);line.IsHitTestVisible(false);dots.IsHitTestVisible(false);
        graph.ManipulationMode(ManipulationModes::None);
        line.Stroke(data->brush(L"text"));line.StrokeThickness(1.5);
        focus.Content(graph);focus.IsTabStop(true);focus.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        focus.VerticalContentAlignment(VerticalAlignment::Stretch);
        AutomationProperties::SetName(focus,str(property->model(),L"label")+L" curve");
        AutomationProperties::SetAutomationId(focus,property->id()+L"-curve");
        hstring hint=L"Click to add a point and drag to shape the curve. Double-click a point or drag it off the graph to remove it.";
        AutomationProperties::SetHelpText(focus,hint);CapyUi::tooltip(focus,hint);
        reset=button(data,L"Reset curve",[weak]{if(auto self=weak.lock()){self->selected=0;self->lastTap.reset();self->cancel();self->property->reset();self->refresh();}});
        reset.Width(28);reset.Height(28);reset.Margin({2,2,2,2});reset.Content(icon(L"reset",data->theme()));
        reset.HorizontalAlignment(HorizontalAlignment::Right);reset.VerticalAlignment(VerticalAlignment::Bottom);reset.Visibility(Visibility::Collapsed);
        CapyUi::tooltip(reset,AutomationProperties::GetName(reset));
        AutomationProperties::SetAutomationId(reset,property->id()+L"-reset");
        chart.Children().Append(focus);chart.Children().Append(reset);root.Children().Append(chart);
        graph.SizeChanged([weak](auto&&,SizeChangedEventArgs const& e){if(auto self=weak.lock()){
            if(self->graph.Height()!=e.NewSize().Width)self->graph.Height(e.NewSize().Width);self->refresh();
        }});
        graph.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto raw=e.GetCurrentPoint(self->graph);
            if(self->pointer||self->graph.ActualWidth()<1||self->graph.ActualHeight()<1
                ||!raw.IsInContact()||(raw.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!raw.Properties().IsLeftButtonPressed()))return;
            auto at=self->position(raw.Position());auto points=self->points();int index=-1;
            for(uint32_t i=0;i<points.Size();i++){auto p=points.GetArrayAt(i);
                double dx=(p.GetNumberAt(0)-at.X)*self->graph.ActualWidth(),dy=(p.GetNumberAt(1)-at.Y)*self->graph.ActualHeight();
                // A curve has one value per x; edit an existing abscissa instead
                // of guessing an insertion index that Rust would reject.
                if(std::hypot(dx,dy)<12||std::abs(p.GetNumberAt(0)-at.X)<=.002){index=i;break;}
            }
            if(index<0&&points.Size()>=32)return;
            if(!self->graph.CapturePointer(e.Pointer()))return;
            self->pointer=raw.PointerId();self->focus.Focus(FocusState::Programmatic);
            self->selected=index;self->pressed=index;self->removable=index<0||(index>0&&index+1<int(points.Size()));
            if(index<0){self->selected=0;for(auto p:points)if(p.GetArray().GetNumberAt(0)<at.X)self->selected++;}
            self->press=at;self->start=at;
            // Preserve the grab offset, including a click with no movement.
            if(index>=0){auto p=points.GetArrayAt(index);self->start={float(p.GetNumberAt(0)),float(p.GetNumberAt(1))};}
            self->change(index,self->start,false,L"down");e.Handled(true);
        }});
        graph.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){
            auto at=self->dragged(e.GetCurrentPoint(self->graph).Position());
            self->detached=self->removable&&offGraph(at);
            self->change(self->selected,at,false,L"move");e.Handled(true);
        }});
        graph.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){
            auto raw=e.GetCurrentPoint(self->graph);auto at=self->position(raw.Position());
            bool tapped=self->pressed>=0&&std::hypot((at.X-self->press.X)*self->graph.ActualWidth(),(at.Y-self->press.Y)*self->graph.ActualHeight())<4;
            bool twice=tapped&&self->lastTap&&self->lastTap->first==self->pressed&&raw.Timestamp()-self->lastTap->second<uint64_t(GetDoubleClickTime())*1000;
            self->lastTap.reset();if(tapped&&!twice)self->lastTap=std::pair{self->pressed,raw.Timestamp()};
            auto released=self->dragged(raw.Position());
            int index=self->selected;
            if((twice&&index>0&&index+1<int(self->points().Size()))||(self->removable&&offGraph(released)))self->selected=index-1;
            self->detached=false;self->change(index,released,twice,L"up");
            self->pointer.reset();self->graph.ReleasePointerCaptures();e.Handled(true);
        }});
        graph.PointerCanceled([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        graph.PointerCaptureLost([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        graph.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        focus.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
            using Windows::System::VirtualKey;
            if(self->pointer){
                if(e.Key()==VirtualKey::Escape)self->cancel();
                e.Handled(true);return;
            }
            auto p=self->point();Point at{float(p.GetNumberAt(0)),float(p.GetNumberAt(1))};
            switch(e.Key()){
                case VirtualKey::Left:at.X-=.01f;break;
                case VirtualKey::Right:at.X+=.01f;break;
                case VirtualKey::Up:at.Y+=.01f;break;
                case VirtualKey::Down:at.Y-=.01f;break;
                case VirtualKey::Delete:self->erase();e.Handled(true);return;
                default:return;
            }
            self->change(self->selected,at);e.Handled(true);
        }});
    }
    void refresh(){
        auto data=property->data;auto p=points();if(p.Size()<2)return;
        // A submitted insertion can be ahead of its snapshot; preserve the
        // captured index until the owner acknowledges the new point.
        if(!pointer)selected=std::clamp(selected,0,int(p.Size())-1);
        auto model=property->model();bool modified=flag(model,L"modified");
        reset.Visibility(modified?Visibility::Visible:Visibility::Collapsed);
        double side=graph.ActualWidth();if(side<=0)return;
        auto view=property->view();
        auto peak=view.GetNamedValue(L"curve_max",JsonValue::CreateNullValue()),white=view.GetNamedValue(L"curve_white",JsonValue::CreateNullValue());
        bool hdr=peak.ValueType()==JsonValueType::Number&&white.ValueType()==JsonValueType::Number&&peak.GetNumber()>0;
        auto next=O({{L"points",p},{L"plot",array(model,L"plot")},{L"side",N(side)},{L"selected",N(detached?-1:selected)},
            {L"peak",peak},{L"white",white},{L"modified",B(modified)},{L"theme",S(data->theme())}}).Stringify();
        if(next==drawn)return;drawn=next;grid.Children().Clear();dots.Children().Clear();
        for(int i=1;i<4;i++)for(int axis=0;axis<2;axis++){
            Shapes::Line l;l.X1(axis?0:side*i/4);l.Y1(axis?side*i/4:0);l.X2(axis?side:side*i/4);l.Y2(axis?side*i/4:side);
            l.Stroke(data->brush(L"text"));l.StrokeThickness(1);l.Opacity(.2);grid.Children().Append(l);
        }
        auto caption=[&](hstring const& text,bool top){
            TextBlock value;value.Text(text);value.FontSize(11);value.FontFamily(FontFamily(L"Segoe UI"));
            value.Foreground(data->brush(L"text"));value.Opacity(.7);value.Measure({1000,1000});
            auto size=value.DesiredSize();
            Canvas::SetLeft(value,top?5.:std::max(5.,side-(modified?33.:5.)-size.Width));
            Canvas::SetTop(value,top?5.:side-5.-size.Height);grid.Children().Append(value);
        };
        if(hdr){
            double at=white.GetNumber();
            for(int axis=0;axis<2;axis++){
                Shapes::Line l;l.X1(axis?0:at*side);l.Y1(axis?(1-at)*side:0);l.X2(axis?side:at*side);l.Y2(axis?(1-at)*side:side);
                l.Stroke(data->brush(L"text"));l.StrokeThickness(1);l.Opacity(.7);
                DoubleCollection dash;dash.Append(3);dash.Append(3);l.StrokeDashArray(dash);grid.Children().Append(l);
            }
            wchar_t range[64];swprintf(range,64,L"%.0f \u00b7 %+.0f EV",peak.GetNumber(),std::log2(peak.GetNumber()));
            caption(L"SDR white \u00b7 0 EV",true);caption(range,false);
        }else{caption(L"Output",true);caption(L"Input",false);}
        std::vector<Point> path;for(auto value:array(model,L"plot")){auto at=value.GetArray();path.push_back({float(at.GetNumberAt(0)*side),float((1-at.GetNumberAt(1))*side)});}
        line.Points().ReplaceAll(path);
        for(uint32_t i=0;i<p.Size();i++){auto at=p.GetArrayAt(i);double radius=!detached&&int(i)==selected?5:3.5;
            Shapes::Ellipse dot;dot.Width(radius*2);dot.Height(radius*2);dot.Fill(data->brush(L"text"));
            Canvas::SetLeft(dot,at.GetNumberAt(0)*side-radius);Canvas::SetTop(dot,(1-at.GetNumberAt(1))*side-radius);dots.Children().Append(dot);
        }
    }
};
}
FrameworkElement CapyEffects::CurveField(std::shared_ptr<Property> const& property,Bindings& bindings){
    auto view=std::make_shared<CurveEditor>();view->property=property;view->init();
    bindings.emplace_back([view]{view->refresh();});return view->root;
}
