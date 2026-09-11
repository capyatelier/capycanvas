#include "pch.h"
#include "EffectControls.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <optional>
using namespace CapyEffects;
using Windows::Foundation::Point;
namespace {
struct CurveEditor : std::enable_shared_from_this<CurveEditor> {
    std::shared_ptr<Property> property;
    StackPanel root,details,coordinates;
    ContentControl inputGate;
    ContentControl focus;
    Canvas graph,grid,dots;
    Shapes::Polyline line;
    Button remove{nullptr};
    ComboBox pointChoice;
    Bindings numbers,coordinateBindings;
    hstring coordinateContext;
    uint32_t generation=0;
    std::optional<uint32_t> pointer;
    int selected=0,count=0;
    hstring drawn;
    A points()const{return array(object(property->model(),L"value"),L"value");}
    A point()const{auto p=points();return p.Size()?p.GetArrayAt(std::clamp(selected,0,int(p.Size())-1)):values({0,0});}
    void change(int index,Point p,bool removePoint=false){
        property->action(O({{L"op",S(L"curve_point")},{L"index",index<0?JsonValue::CreateNullValue():N(index)},
            {L"point",values({p.X,p.Y})},{L"remove",B(removePoint)}}));
    }
    Point position(Point p)const{
        return {float(std::clamp(p.X/graph.ActualWidth(),0.,1.)),float(std::clamp(1-p.Y/graph.ActualHeight(),0.,1.))};
    }
    void cancel(){pointer.reset();graph.ReleasePointerCaptures();}
    void add(){
        auto p=points();if(p.Size()<2||p.Size()>=32)return;
        double gap=0,x=0;int at=1;
        for(uint32_t i=1;i<p.Size();i++){double a=p.GetArrayAt(i-1).GetNumberAt(0),b=p.GetArrayAt(i).GetNumberAt(0);
            if(b-a>gap){gap=b-a;x=(a+b)/2;at=i;}}
        // Place the new handle on the sampled curve supplied by Rust.
        auto plot=array(property->model(),L"plot");double y=x;
        for(uint32_t i=1;i<plot.Size();i++){auto a=plot.GetArrayAt(i-1),b=plot.GetArrayAt(i);
            if(x<=b.GetNumberAt(0)){double t=(x-a.GetNumberAt(0))/(b.GetNumberAt(0)-a.GetNumberAt(0));y=a.GetNumberAt(1)*(1-t)+b.GetNumberAt(1)*t;break;}}
        selected=at;change(-1,{float(x),float(y)});
    }
    void erase(){auto p=points();if(selected>0&&selected+1<int(p.Size())){int old=selected--;change(old,{},true);}}
    void init(){
        auto data=property->data;auto weak=weak_from_this();root.Spacing(6);details.Spacing(6);details.Visibility(Visibility::Collapsed);
        graph.Background(data->brush(L"input"));graph.Children().Append(grid);graph.Children().Append(line);graph.Children().Append(dots);
        grid.IsHitTestVisible(false);line.IsHitTestVisible(false);dots.IsHitTestVisible(false);
        line.Stroke(data->brush(L"text"));line.StrokeThickness(1.5);
        focus.Content(graph);focus.IsTabStop(true);focus.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        focus.VerticalContentAlignment(VerticalAlignment::Stretch);
        AutomationProperties::SetName(focus,str(property->model(),L"label")+L" curve");
        AutomationProperties::SetAutomationId(focus,property->id()+L"-curve");
        root.Children().Append(focus);
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
            self->selected=index;
            if(index<0){self->selected=0;for(auto p:points)if(p.GetArray().GetNumberAt(0)<at.X)self->selected++;}
            self->change(index,at);e.Handled(true);
        }});
        graph.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){
            self->change(self->selected,self->position(e.GetCurrentPoint(self->graph).Position()));e.Handled(true);
        }});
        graph.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){
            self->change(self->selected,self->position(e.GetCurrentPoint(self->graph).Position()));self->cancel();e.Handled(true);
        }});
        graph.PointerCanceled([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        graph.PointerCaptureLost([weak](auto&&,auto&&){if(auto self=weak.lock())self->pointer.reset();});
        graph.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        focus.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
            using Windows::System::VirtualKey;
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
        StackPanel actions;actions.Orientation(Orientation::Horizontal);actions.Spacing(6);
        auto add=button(data,L"Add point",[weak]{if(auto self=weak.lock())self->add();});
        remove=button(data,L"Remove point",[weak]{if(auto self=weak.lock())self->erase();});
        auto reset=button(data,L"Reset curve",[weak]{if(auto self=weak.lock()){self->selected=0;++self->generation;self->cancel();self->property->reset();self->refresh();}});
        auto edit=button(data,L"Point values",[weak]{if(auto self=weak.lock())self->details.Visibility(self->details.Visibility()==Visibility::Visible?Visibility::Collapsed:Visibility::Visible);});
        int i=0;for(auto pick:{add,remove,reset,edit}){
            pick.Width(28);pick.Height(28);pick.Content(icon(std::array<hstring,4>{L"plus",L"minus",L"undo",L"properties"}[i],data->theme()));
            ToolTipService::SetToolTip(pick,box_value(AutomationProperties::GetName(pick)));
            AutomationProperties::SetAutomationId(pick,property->id()+L"-"+std::array<hstring,4>{L"add",L"remove",L"reset",L"values"}[i++]);actions.Children().Append(pick);
        }
        numbers.emplace_back([weak,add]{if(auto self=weak.lock())add.IsEnabled(self->points().Size()<32);});
        root.Children().Append(actions);
        pointChoice.MinWidth(0);pointChoice.MinHeight(32);pointChoice.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetName(pointChoice,L"Curve point");AutomationProperties::SetAutomationId(pointChoice,property->id()+L"-point");
        pointChoice.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->property->data->updating&&self->pointChoice.SelectedIndex()>=0){
            self->selected=self->pointChoice.SelectedIndex();self->refresh();
        }});
        details.Children().Append(pointChoice);
        coordinates.Spacing(6);details.Children().Append(coordinates);root.Children().Append(details);
        inputGate.IsTabStop(false);inputGate.HorizontalContentAlignment(HorizontalAlignment::Stretch);
    }
    void rebuildCoordinates(){
        coordinateBindings.clear();coordinates.Children().Clear();auto weak=weak_from_this();auto data=property->data;
        for(int axis=0;axis<2;axis++){
            auto field=number(data,axis?L"Output":L"Input",object(data->catalog,L"opacity"),
                [weak,axis]{if(auto self=weak.lock())return self->point().GetNumberAt(axis);return 0.;},
                [weak,axis,expected=coordinateContext](double v){if(auto self=weak.lock();self&&self->coordinateContext==expected){
                    auto p=self->point();Point at{float(p.GetNumberAt(0)),float(p.GetNumberAt(1))};if(axis)at.Y=float(v);else at.X=float(v);self->change(self->selected,at);
                }},coordinateBindings,nullptr,false,property->id()+(axis?L"-y":L"-x"));
            if(!axis){inputGate.Content(field);coordinates.Children().Append(inputGate);}else coordinates.Children().Append(field);
        }
    }
    void refresh(){
        auto data=property->data;Updating updating(data);auto p=points();if(p.Size()<2)return;
        // A submitted insertion can be ahead of its snapshot; preserve the
        // captured index until the owner acknowledges the new point.
        if(!pointer)selected=std::clamp(selected,0,int(p.Size())-1);
        if(count!=int(p.Size())){count=p.Size();pointChoice.Items().Clear();for(int i=0;i<count;i++)pointChoice.Items().Append(box_value(L"Point "+to_hstring(i+1)));}
        pointChoice.SelectedIndex(std::clamp(selected,0,count-1));remove.IsEnabled(selected>0&&selected+1<count);
        auto nextContext=to_hstring(selected)+L"/"+to_hstring(count)+L"/"+to_hstring(generation);
        if(nextContext!=coordinateContext){coordinateContext=nextContext;rebuildCoordinates();}
        inputGate.IsEnabled(selected>0&&selected+1<count);
        for(auto const& update:numbers)update();for(auto const& update:coordinateBindings)update();
        auto model=property->model();double side=graph.ActualWidth();if(side<=0)return;
        auto next=O({{L"points",p},{L"plot",array(model,L"plot")},{L"side",N(side)},{L"selected",N(selected)}}).Stringify();
        if(next==drawn)return;drawn=next;grid.Children().Clear();dots.Children().Clear();
        for(int i=1;i<4;i++)for(int axis=0;axis<2;axis++){
            Shapes::Line l;l.X1(axis?0:side*i/4);l.Y1(axis?side*i/4:0);l.X2(axis?side:side*i/4);l.Y2(axis?side*i/4:side);
            l.Stroke(data->brush(L"text"));l.StrokeThickness(1);l.Opacity(.2);grid.Children().Append(l);
        }
        std::vector<Point> path;for(auto value:array(model,L"plot")){auto at=value.GetArray();path.push_back({float(at.GetNumberAt(0)*side),float((1-at.GetNumberAt(1))*side)});}
        line.Points().ReplaceAll(path);
        for(uint32_t i=0;i<p.Size();i++){auto at=p.GetArrayAt(i);double radius=int(i)==selected?5:3.5;
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
