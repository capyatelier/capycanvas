#include "pch.h"
#include "EffectControls.h"
#include "ScopesView.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <cmath>
#include <optional>
using namespace CapyEffects;
using Windows::Foundation::Point;
namespace {
hstring curveKey(Windows::System::VirtualKey key){
    using Windows::System::VirtualKey;
    switch(key){
        case VirtualKey::Left:return L"ArrowLeft";
        case VirtualKey::Right:return L"ArrowRight";
        case VirtualKey::Up:return L"ArrowUp";
        case VirtualKey::Down:return L"ArrowDown";
        case VirtualKey::Delete:return L"Delete";
        case VirtualKey::Back:return L"Backspace";
        case VirtualKey::Escape:return L"Escape";
        default:return L"";
    }
}
bool held(int key){return (GetKeyState(key)&0x8000)!=0;}
TextBlock axisText(std::shared_ptr<WorkspaceData> const& data){
    auto text=label(data,L"");text.FontSize(data->textSize()*.85);text.Opacity(.7);return text;
}
struct CurveEditor : std::enable_shared_from_this<CurveEditor> {
    std::shared_ptr<Property> property;
    std::shared_ptr<WorkspaceData> data;
    StackPanel root;
    Grid chart;
    ContentControl focus;
    Canvas graph,grid,dots;
    Shapes::Polyline line;
    Button reset{nullptr};
    std::array<std::array<TextBlock,3>,2> axes;
    std::array<TextBlock,2> ev;
    std::array<ContentControl,2> coordinates;
    std::optional<uint32_t> pointer;
    std::optional<std::pair<uint64_t,Point>> lastTap;
    std::pair<uint64_t,Point> press{};
    hstring heldKey,drawn,labels;
    double owner=0,firstCount=0;
    Point extent{};
    J model()const{return property?property->model():object(object(data->state,L"pressure_calibration"),L"editor");}
    J curve()const{return property?property->curve():object(model(),L"controls");}
    hstring caption()const{return property?str(model(),L"label"):str(object(data->state,L"pressure_calibration"),L"title");}
    double epoch()const{return num(curve(),L"epoch");}
    A points()const{return property?array(object(model(),L"value"),L"value"):array(model(),L"points");}
    void send(J operation,hstring const& phase={})const{
        if(!phase.empty())operation=O({{L"kind",S(L"gesture")},{L"phase",S(phase)},{L"action",operation}});
        bool continuing=(!phase.empty()&&phase!=L"down")||(str(operation,L"kind")==L"contact"&&str(operation,L"phase")!=L"down");
        if(!continuing&&(data->updating||(property&&(!property->current()||!flag(property->view(),L"enabled")))))return;
        auto target=property?O({{L"kind",S(L"effect")},{L"layer",N(property->layer)},{L"key",S(property->key)}}):O({{L"kind",S(L"pressure")}});
        auto request=O({{L"type",S(L"curve_editor")},{L"target",target},{L"action",operation}});
        if(property)data->dispatchDocument(request,to_hstring(uint64_t(property->epoch)));else data->dispatch(request);
    }
    J located(hstring const& op,Point at)const{
        return O({{L"kind",S(op)},{L"epoch",N(owner)},{L"point",values({at.X-num(curve(),L"inset"),at.Y-num(curve(),L"inset")})},{L"extent",values({extent.X,extent.Y})}});
    }
    bool secondTap(uint64_t time,Point at)const{
        return lastTap&&time-lastTap->first<=uint64_t(GetDoubleClickTime())*1000&&std::hypot(at.X-lastTap->second.X,at.Y-lastTap->second.Y)<=8;
    }
    void contact(hstring const& phase,Point at){auto operation=located(L"contact",at);operation.Insert(L"phase",S(phase));send(operation);}
    void cancel(){
        if(!pointer&&heldKey.empty())return;
        if(pointer){pointer.reset();graph.ReleasePointerCaptures();}else owner=epoch();
        heldKey=L"";contact(L"cancel",{});
    }
    void removeAt(Point at,std::optional<double> count){
        owner=epoch();extent={float(graph.ActualWidth()-2*num(curve(),L"inset")),float(graph.ActualHeight()-2*num(curve(),L"inset"))};
        auto operation=located(L"remove_at",at);operation.Insert(L"point_count",count?N(*count):JsonValue::CreateNullValue());send(operation);
    }
    void key(KeyRoutedEventArgs const& e,bool pressed){
        auto name=curveKey(e.Key());if(name.empty())return;
        bool control=held(VK_CONTROL),alt=held(VK_MENU);
        if(pressed&&name!=L"Escape"&&(control||alt))return;
        if(name==L"Escape"&&pointer){pointer.reset();graph.ReleasePointerCaptures();}
        if(name.starts_with(L"Arrow"))heldKey=pressed?name:hstring();
        auto status=e.KeyStatus();auto keyOwner=epoch();
        for(uint32_t i=0,count=pressed?std::max(1u,status.RepeatCount):1u;i<count;i++)
            send(O({{L"kind",S(L"key")},{L"epoch",N(keyOwner)},{L"key_event",S(name)},{L"pressed",B(pressed)},
                {L"repeat",B(pressed&&(status.WasKeyDown||i>0))},{L"modifiers",O({{L"command",B(control)},{L"shift",B(held(VK_SHIFT))},{L"alt",B(alt)}})}}));
        e.Handled(true);
    }
    FrameworkElement coordinate(int axis,Bindings& bindings){
        auto data=this->data;auto weak=weak_from_this();auto name=axis?L"output":L"input";
        auto value=[weak,name]{if(auto self=weak.lock())return self->curve().GetNamedValue(name,JsonValue::CreateNullValue());return JsonValue::CreateNullValue();};
        auto operation=[weak,name](double v){
            auto self=weak.lock();if(!self)return J();
            return O({{L"kind",S(L"number")},{L"epoch",N(self->epoch())},{L"axis",S(name)},{L"operation",O({{L"type",S(L"value")},{L"value",N(v)}})}});
        };
        auto captured=std::make_shared<bool>(false);
        NumberPresentation presentation;
        presentation.title=[weak,axis]{
            auto self=weak.lock();if(!self)return hstring();
            auto axes=array(self->curve(),L"axes");return uint32_t(axis)<axes.Size()?str(axes.GetObjectAt(axis),L"label"):hstring();
        };
        presentation.text=[value]{auto v=value();return v.ValueType()==JsonValueType::Object?str(v.GetObject(),L"text"):hstring();};
        presentation.identity=[weak]{if(auto self=weak.lock())return self->curve().GetNamedValue(L"selected",JsonValue::CreateNullValue()).Stringify()+L"/"+to_hstring(self->epoch());return hstring();};
        presentation.phase=[weak,operation,captured](hstring const& phase,double v){*captured=phase==L"down";if(auto self=weak.lock())if(auto action=operation(v))self->send(action,phase);};
        auto field=number(data,presentation.title(),object(curve(),L"numeric"),
            [value]{auto v=value();return v.ValueType()==JsonValueType::Object?num(v.GetObject(),L"value"):0.;},
            [weak,operation,captured](double v){if(auto self=weak.lock())if(auto action=operation(v))self->send(action,*captured?L"move":L"");},
            bindings,nullptr,true,property->id()+(axis?L"-output":L"-input"),false,presentation);
        auto& gate=coordinates[axis];gate.Content(field);gate.IsTabStop(false);gate.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        auto caption=label(data,presentation.title());caption.TextTrimming(TextTrimming::CharacterEllipsis);
        bindings.emplace_back([value,gate,caption,title=presentation.title,property=property]{
            auto v=value();gate.IsEnabled(v.ValueType()==JsonValueType::Object&&!flag(v.GetObject(),L"read_only")&&flag(property->view(),L"enabled"));
            auto text=title();if(caption.Text()!=text){caption.Text(text);tooltip(caption,text);}
        });
        StackPanel result;result.Children().Append(caption);result.Children().Append(gate);ev[axis]=axisText(data);ev[axis].HorizontalAlignment(HorizontalAlignment::Right);
        AutomationProperties::SetAutomationId(ev[axis],property->id()+(axis?L"-output-ev":L"-input-ev"));
        result.Children().Append(ev[axis]);Grid::SetColumn(result,axis);return result;
    }
    void init(Bindings& bindings){
        auto data=this->data;auto weak=weak_from_this();root.Spacing(6);
        graph.Background(clear());graph.Children().Append(grid);graph.Children().Append(line);graph.Children().Append(dots);
        grid.IsHitTestVisible(false);line.IsHitTestVisible(false);dots.IsHitTestVisible(false);
        graph.ManipulationMode(ManipulationModes::None);
        line.Stroke(data->brush(L"text"));line.StrokeThickness(1.5);
        focus.Content(graph);focus.IsTabStop(true);focus.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        focus.VerticalContentAlignment(VerticalAlignment::Stretch);
        AutomationProperties::SetAutomationId(focus,property?property->id()+L"-curve":L"pen-pressure-curve");
        reset=button(data,L"",[weak]{if(auto self=weak.lock()){self->cancel();self->send(O({{L"kind",S(L"reset")}}));}});
        reset.Width(28);reset.Height(28);reset.Content(icon(L"reset",data->theme()));reset.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(reset,property?property->id()+L"-reset":L"pen-pressure-curve-reset");
        chart.Height(200);chart.MinWidth(64);chart.Background(data->brush(L"input"));
        if(property)chart.Children().Append(CapyScopes::TonalPlot(data,bindings));chart.Children().Append(focus);
        Grid frame;frame.ColumnSpacing(6);frame.RowSpacing(4);
        for(auto width:{GridUnitType::Auto,GridUnitType::Star}){ColumnDefinition column;column.Width({1,width});frame.ColumnDefinitions().Append(column);}
        for(auto height:{GridUnitType::Star,GridUnitType::Auto}){RowDefinition row;row.Height({1,height});frame.RowDefinitions().Append(row);}
        Grid vertical,horizontal;
        for(auto& axis:axes)for(auto& text:axis)text=axisText(data);
        for(int i=0;i<3;i++){
            RowDefinition row;row.Height({1,i==1?GridUnitType::Star:GridUnitType::Auto});vertical.RowDefinitions().Append(row);
            ColumnDefinition column;column.Width({1,i==1?GridUnitType::Star:GridUnitType::Auto});horizontal.ColumnDefinitions().Append(column);
            Grid::SetColumn(axes[0][i],i==1?0:i);if(i==1)Grid::SetColumnSpan(axes[0][i],3);horizontal.Children().Append(axes[0][i]);
            Grid::SetRow(axes[1][2-i],i);vertical.Children().Append(axes[1][2-i]);
        }
        if(!flag(curve(),L"coordinate_readouts")){
            vertical.Width(36);axes[1][1].HorizontalAlignment(HorizontalAlignment::Center);
            RotateTransform rotation;rotation.Angle(-90);axes[1][1].RenderTransform(rotation);axes[1][1].RenderTransformOrigin({.5,.5});
        }
        axes[0][1].HorizontalAlignment(HorizontalAlignment::Center);axes[1][1].VerticalAlignment(VerticalAlignment::Center);
        Grid::SetColumn(chart,1);Grid::SetColumn(horizontal,1);Grid::SetRow(horizontal,1);
        frame.Children().Append(vertical);frame.Children().Append(chart);frame.Children().Append(horizontal);root.Children().Append(frame);
        if(flag(curve(),L"coordinate_readouts")){
        Grid coordinateRow;coordinateRow.ColumnSpacing(6);
        for(int axis=0;axis<2;axis++){ColumnDefinition column;column.Width({1,GridUnitType::Star});coordinateRow.ColumnDefinitions().Append(column);}
        for(int axis=0;axis<2;axis++)coordinateRow.Children().Append(coordinate(axis,bindings));
        root.Children().Append(coordinateRow);
        }
        if(property)root.Children().Append(CapyScopes::ScopeFooter(data,L"curve",[data]{return object(data->state,L"tonal_histogram");},bindings,reset));
        graph.Clip(RectangleGeometry{});
        graph.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock()){
            self->graph.Clip().as<RectangleGeometry>().Rect({0,0,float(self->graph.ActualWidth()),float(self->graph.ActualHeight())});
            self->refresh();
        }});
        graph.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto raw=e.GetCurrentPoint(self->graph);
            if(self->pointer||self->graph.ActualWidth()<1||self->graph.ActualHeight()<1
                ||!raw.IsInContact()||(raw.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!raw.Properties().IsLeftButtonPressed()))return;
            auto at=raw.Position();if(!self->secondTap(raw.Timestamp(),at))self->firstCount=self->points().Size();
            self->press={raw.Timestamp(),at};
            if(!self->graph.CapturePointer(e.Pointer()))return;
            self->pointer=raw.PointerId();self->owner=self->epoch();
            self->extent={float(self->graph.ActualWidth()-2*num(self->curve(),L"inset")),float(self->graph.ActualHeight()-2*num(self->curve(),L"inset"))};
            self->focus.Focus(FocusState::Programmatic);self->contact(L"down",at);e.Handled(true);
        }});
        graph.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){
            self->contact(L"move",e.GetCurrentPoint(self->graph).Position());e.Handled(true);
        }});
        graph.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){
            auto at=e.GetCurrentPoint(self->graph).Position();self->pointer.reset();self->contact(L"up",at);
            self->graph.ReleasePointerCaptures();e.Handled(true);
            bool tap=std::hypot(at.X-self->press.second.X,at.Y-self->press.second.Y)<=4;
            if(tap&&self->secondTap(self->press.first,self->press.second)){self->lastTap.reset();self->removeAt(at,self->firstCount);}
            else self->lastTap=tap?std::optional(self->press):std::nullopt;
        }});
        graph.PointerCanceled([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->pointer)self->cancel();});
        graph.PointerCaptureLost([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->pointer)self->cancel();});
        graph.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        graph.RightTapped([weak](auto&&,RightTappedRoutedEventArgs const& e){if(auto self=weak.lock();self&&e.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse){
            self->cancel();self->removeAt(e.GetPosition(self->graph),std::nullopt);e.Handled(true);
        }});
        focus.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock())self->key(e,true);});
        focus.KeyUp([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock())self->key(e,false);});
        focus.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
    }
    ~CurveEditor(){cancel();}
    void refresh(){
        auto data=this->data;auto model=this->model();auto view=curve();auto p=points();
        if(p.Size()<2||!view.Size())return;
        auto help=str(view,L"help"),resetLabel=str(view,L"reset_label");
        if(auto copy=caption()+L"\n"+help+L"\n"+resetLabel;copy!=labels){labels=copy;
            AutomationProperties::SetName(focus,caption());AutomationProperties::SetHelpText(focus,help);CapyUi::tooltip(focus,help);
            AutomationProperties::SetName(reset,resetLabel);CapyUi::tooltip(reset,resetLabel);
        }
        auto axisViews=array(view,L"axes");
        for(int axis=0;axis<2&&axis<int(axisViews.Size());axis++){
            auto spec=axisViews.GetObjectAt(axis);
            for(int i=0;i<3;i++)if(auto text=str(spec,i==0?L"minimum":i==1?L"label":L"maximum");axes[axis][i].Text()!=text)axes[axis][i].Text(text);
        }
        bool log=str(object(view,L"domain"),L"kind")==L"log_hdr";
        if(flag(view,L"coordinate_readouts"))for(int axis=0;axis<2;axis++){
            auto value=view.GetNamedValue(axis?L"output":L"input",JsonValue::CreateNullValue());
            ev[axis].Text(value.ValueType()==JsonValueType::Object?str(value.GetObject(),L"ev"):hstring());
            ev[axis].Visibility(log?Visibility::Visible:Visibility::Collapsed);
        }
        bool modified=flag(model,L"modified");reset.Visibility(property&&modified?Visibility::Visible:Visibility::Collapsed);
        double inset=num(view,L"inset"),width=graph.ActualWidth()-2*inset,height=graph.ActualHeight()-2*inset;if(width<=0||height<=0)return;
        auto selected=view.GetNamedValue(L"selected",JsonValue::CreateNullValue());
        A whites;for(auto axis:axisViews)whites.Append(axis.GetObject().GetNamedValue(L"white",JsonValue::CreateNullValue()));
        auto next=O({{L"points",p},{L"plot",array(model,L"plot")},{L"size",values({width,height})},{L"selected",selected},
            {L"whites",whites},{L"marker",model.GetNamedValue(L"marker",JsonValue::CreateNullValue())},{L"theme",S(data->theme())}}).Stringify();
        if(next==drawn)return;drawn=next;grid.Children().Clear();dots.Children().Clear();
        for(int i=1;i<4;i++)for(int axis=0;axis<2;axis++){
            Shapes::Line l;l.X1(inset+(axis?0:width*i/4));l.Y1(inset+(axis?height*i/4:0));l.X2(inset+(axis?width:width*i/4));l.Y2(inset+(axis?height*i/4:height));
            l.Stroke(data->brush(L"text"));l.StrokeThickness(1);l.Opacity(.2);grid.Children().Append(l);
        }
        for(int axis=0;axis<2&&axis<int(axisViews.Size());axis++){
            auto white=axisViews.GetObjectAt(axis).GetNamedValue(L"white",JsonValue::CreateNullValue());
            if(white.ValueType()!=JsonValueType::Number)continue;double at=white.GetNumber();
            Shapes::Line l;l.X1(inset+(axis?0:at*width));l.Y1(inset+(axis?(1-at)*height:0));l.X2(inset+(axis?width:at*width));l.Y2(inset+(axis?(1-at)*height:height));
            l.Stroke(data->brush(L"text"));l.StrokeThickness(1);l.Opacity(.7);
            DoubleCollection dash;dash.Append(3);dash.Append(3);l.StrokeDashArray(dash);grid.Children().Append(l);
        }
        std::vector<Point> path;for(auto value:array(model,L"plot")){auto at=value.GetArray();path.push_back({float(inset+at.GetNumberAt(0)*width),float(inset+(1-at.GetNumberAt(1))*height)});}
        line.Points().ReplaceAll(path);
        if(flag(view,L"control_polygon")){
            Shapes::Polyline polygon;std::vector<Point> handles;
            for(auto point:p){auto at=point.GetArray();handles.push_back({float(inset+at.GetNumberAt(0)*width),float(inset+(1-at.GetNumberAt(1))*height)});}
            polygon.Points().ReplaceAll(handles);polygon.Stroke(data->brush(L"text"));polygon.StrokeThickness(1);polygon.Opacity(.35);
            DoubleCollection dash;dash.Append(3);dash.Append(3);polygon.StrokeDashArray(dash);grid.Children().Append(polygon);
        }
        auto marker=array(model,L"marker");
        if(marker.Size()==2){Shapes::Ellipse dot;dot.Width(8);dot.Height(8);dot.Fill(accent(data));Canvas::SetLeft(dot,inset+marker.GetNumberAt(0)*width-4);Canvas::SetTop(dot,inset+(1-marker.GetNumberAt(1))*height-4);dots.Children().Append(dot);}
        for(uint32_t i=0;i<p.Size();i++){
            auto at=p.GetArrayAt(i);double x=inset+at.GetNumberAt(0)*width,y=inset+(1-at.GetNumberAt(1))*height;
            Shapes::Ellipse dot;dot.Width(7);dot.Height(7);dot.Fill(data->brush(L"text"));
            Canvas::SetLeft(dot,x-3.5);Canvas::SetTop(dot,y-3.5);dots.Children().Append(dot);
            if(selected.ValueType()==JsonValueType::Number&&uint32_t(selected.GetNumber())==i){
                Shapes::Ellipse ring;ring.Width(12);ring.Height(12);ring.Stroke(data->brush(L"text"));ring.StrokeThickness(1.5);
                Canvas::SetLeft(ring,x-6);Canvas::SetTop(ring,y-6);dots.Children().Append(ring);
            }
        }
    }
};
}
FrameworkElement CapyEffects::CurveField(std::shared_ptr<Property> const& property,Bindings& bindings){
    auto view=std::make_shared<CurveEditor>();view->property=property;view->data=property->data;view->init(bindings);
    bindings.emplace_back([view]{view->refresh();});return view->root;
}

FrameworkElement CapyEffects::PressureCurveField(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<CurveEditor>();view->data=data;view->init(bindings);
    bindings.emplace_back([view]{view->refresh();});return view->root;
}
