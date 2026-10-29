#pragma once
#include "EffectControls.h"
#include "PenInput.h"
#include "PointerGesture.h"
#include "WorkspaceShadow.h"
#include "WorkspaceGeometry.h"

namespace CapyUi {
struct ResponseCalibration:std::enable_shared_from_this<ResponseCalibration>{
    std::shared_ptr<WorkspaceData> data;
    Canvas host{nullptr},presenter{nullptr},direction;
    ContentControl frame,directionControl;
    Grid layers,header,footer,controls,sensitivity;
    StackPanel body,content,heading,directionSection;
    Shapes::Path surface;
    Shapes::Ellipse circle;
    std::array<Shapes::Polyline,2> arrows;
    std::array<Primitives::ToggleButton,3> axes;
    ScrollViewer scroll;
    Border slot;
    TextBlock title{nullptr},device{nullptr},directionTitle{nullptr},sensitivityTitle{nullptr};
    Button close{nullptr},firmer{nullptr},lighter{nullptr},reset{nullptr},cancel{nullptr},apply{nullptr};
    WorkspaceShadow shadow;
    Bindings bindings;
    TranslateTransform translation;
    std::optional<uint32_t> pointer;
    hstring built,measured,shape,drawn,presentedTarget;
    bool measuring=false,publishing=false,projecting=false,exposed=false;
    bool tilt()const{return object(data->state,L"tilt_calibration").Size()!=0;}
    hstring target()const{return tilt()?L"tilt":L"pressure";}
    J view()const{return object(data->state,tilt()?L"tilt_calibration":L"pressure_calibration");}
    A extent(double width,double height)const{return CapyEffects::values({width,height});}
    J packet(J const& action,bool isTilt)const{
        return isTilt?O({{L"type",S(L"tilt_calibration")},{L"action",O({{L"kind",S(L"panel")},{L"action",action}})}})
            :O({{L"type",S(L"pressure_calibration")},{L"action",action}});
    }
    void send(J action)const{data->dispatch(packet(action,tilt()));}
    void curve(J const& action)const{data->dispatch(O({{L"type",S(L"curve_editor")},{L"target",O({{L"kind",S(target())}})},{L"action",action}}));}
    void drag(hstring const& phase,Windows::Foundation::Point at={}){
        send(O({{L"kind",S(L"drag")},{L"phase",S(phase)},{L"position",extent(at.X,at.Y)},{L"viewport",extent(host.ActualWidth(),host.ActualHeight())}}));
    }
    void preview(PointerRoutedEventArgs const& event,uint32_t phase){
        if(!view().Size())return;
        auto message=penPreview(event.GetCurrentPoint(host),phase);if(message.Size())data->dispatch(message);
    }
    void init(Canvas const& root){
        host=root;auto weak=weak_from_this();
        title=label(data,L"",true);title.MinHeight(24);title.VerticalAlignment(VerticalAlignment::Center);
        device=label(data,L"");device.Opacity(.55);device.TextWrapping(TextWrapping::Wrap);
        heading.Spacing(4);heading.Margin({16,12,0,12});heading.Children().Append(title);heading.Children().Append(device);
        close=button(data,L"",[weak]{if(auto self=weak.lock())self->send(O({{L"kind",S(L"cancel")}}));});
        close.Width(24);close.Height(24);close.MinWidth(0);close.MinHeight(0);close.Padding({0});close.VerticalAlignment(VerticalAlignment::Top);close.Margin({8,12,16,0});
        ColumnDefinition labelColumn;labelColumn.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(labelColumn);
        ColumnDefinition closeColumn;closeColumn.Width({1,GridUnitType::Auto});header.ColumnDefinitions().Append(closeColumn);
        header.Children().Append(heading);Grid::SetColumn(close,1);header.Children().Append(close);body.Children().Append(header);
        firmer=button(data,L"",[weak]{if(auto self=weak.lock())self->send(O({{L"kind",S(L"sensitivity")},{L"lighter",B(false)}}));});
        lighter=button(data,L"",[weak]{if(auto self=weak.lock())self->send(O({{L"kind",S(L"sensitivity")},{L"lighter",B(true)}}));});
        reset=button(data,L"",[weak]{if(auto self=weak.lock())self->curve(O({{L"kind",S(L"reset")}}));});
        cancel=button(data,L"",[weak]{if(auto self=weak.lock())self->send(O({{L"kind",S(L"cancel")}}));});
        apply=button(data,L"",[weak]{if(auto self=weak.lock())self->send(O({{L"kind",S(L"apply")}}));});
        for(auto control:{firmer,lighter,reset,cancel,apply}){control.MinWidth(0);control.Padding({12,0,12,0});control.MinHeight(36);}
        sensitivity.ColumnSpacing(8);
        sensitivityTitle=label(data,L"");sensitivityTitle.TextWrapping(TextWrapping::Wrap);sensitivityTitle.VerticalAlignment(VerticalAlignment::Center);
        controls.ColumnSpacing(8);
        for(int i=0;i<4;i++){ColumnDefinition column;column.Width({1,i%2?GridUnitType::Auto:GridUnitType::Star});controls.ColumnDefinitions().Append(column);}
        controls.Children().Append(sensitivityTitle);Grid::SetColumn(sensitivity,1);controls.Children().Append(sensitivity);
        for(int i=0;i<2;i++){ColumnDefinition column;column.Width({1,GridUnitType::Star});sensitivity.ColumnDefinitions().Append(column);}
        firmer.HorizontalAlignment(HorizontalAlignment::Stretch);lighter.HorizontalAlignment(HorizontalAlignment::Stretch);
        Grid::SetColumn(lighter,1);sensitivity.Children().Append(firmer);sensitivity.Children().Append(lighter);
        content.Spacing(12);content.Padding({16,0,16,0});content.Children().Append(slot);content.Children().Append(controls);
        directionTitle=label(data,L"");directionTitle.TextWrapping(TextWrapping::Wrap);directionTitle.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(directionTitle,2);controls.Children().Append(directionTitle);
        direction.Width(72);direction.Height(72);direction.Background(clear());
        directionControl.Content(direction);directionControl.Width(72);directionControl.Height(72);directionControl.HorizontalAlignment(HorizontalAlignment::Right);directionControl.VerticalAlignment(VerticalAlignment::Bottom);directionControl.Margin({0,0,8,8});directionControl.HorizontalContentAlignment(HorizontalAlignment::Stretch);directionControl.VerticalContentAlignment(VerticalAlignment::Stretch);
        circle.Width(62);circle.Height(62);circle.StrokeThickness(2);Canvas::SetLeft(circle,5);Canvas::SetTop(circle,5);circle.Opacity(.18);direction.Children().Append(circle);
        for(auto const& arrow:arrows){arrow.StrokeThickness(1.5);arrow.IsHitTestVisible(false);direction.Children().Append(arrow);}
        arrows[0].Opacity(.55);arrows[1].StrokeThickness(2.5);
        StackPanel segments;segments.Orientation(Orientation::Horizontal);segments.HorizontalAlignment(HorizontalAlignment::Center);
        for(size_t i=0;i<axes.size();++i){
            auto control=axes[i];control.Width(36);control.Height(36);control.MinWidth(0);control.MinHeight(0);control.Padding({0});control.BorderThickness({0});control.IsChecked(false);
            double radius=data->panelMetric(L"control_radius")*CornerFit;control.CornerRadius({i==0?radius:0.,i==2?radius:0.,i==2?radius:0.,i==0?radius:0.});
            control.Click([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->projecting){
                self->data->dispatch(O({{L"type",S(L"tilt_calibration")},{L"action",O({{L"kind",S(L"axes")},{L"axes",O({
                    {L"reverse_horizontal",B(self->axes[0].IsChecked().Value())},{L"reverse_vertical",B(self->axes[1].IsChecked().Value())},{L"swap",B(self->axes[2].IsChecked().Value())}})}})}}));
            }});
            segments.Children().Append(control);
        }
        directionSection.Children().Append(segments);Grid::SetColumn(directionSection,3);controls.Children().Append(directionSection);
        scroll.Content(content);scroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);body.Children().Append(scroll);
        footer.ColumnSpacing(8);footer.Margin({16,16,16,16});
        for(int i=0;i<4;i++){ColumnDefinition column;column.Width({1,i==1?GridUnitType::Star:GridUnitType::Auto});footer.ColumnDefinitions().Append(column);}
        Grid::SetColumn(cancel,2);Grid::SetColumn(apply,3);footer.Children().Append(reset);footer.Children().Append(cancel);footer.Children().Append(apply);body.Children().Append(footer);
        surface.IsHitTestVisible(false);header.Background(clear());layers.Background(clear());layers.Children().Append(surface);layers.Children().Append(body);
        frame.Content(layers);frame.HorizontalContentAlignment(HorizontalAlignment::Stretch);frame.VerticalContentAlignment(VerticalAlignment::Stretch);
        frame.RenderTransform(translation);Canvas::SetZIndex(frame,1100);
        auto observe=[weak](PointerRoutedEventArgs const& event,uint32_t phase){if(auto self=weak.lock())self->preview(event,phase);};
        frame.AddHandler(UIElement::PointerEnteredEvent(),box_value(PointerEventHandler([observe](auto&&,auto&& e){observe(e,0);})),true);
        frame.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([observe](auto&&,PointerRoutedEventArgs const& e){observe(e,e.GetCurrentPoint(nullptr).IsInContact()?2:0);})),true);
        frame.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([observe](auto&&,auto&& e){observe(e,1);})),true);
        frame.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([observe](auto&&,auto&& e){observe(e,3);})),true);
        frame.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler([observe](auto&&,auto&& e){observe(e,4);})),true);
        frame.AddHandler(UIElement::PointerExitedEvent(),box_value(PointerEventHandler([observe](auto&&,PointerRoutedEventArgs const& e){if(!e.Pointer().IsInRange())observe(e,4);})),true);
        header.ManipulationMode(ManipulationModes::None);
        header.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node))if(node==self->close)return;
            auto raw=e.GetCurrentPoint(self->host);if(self->pointer||!PointerGesture::PrimaryContact(raw))return;
            if(!self->header.CapturePointer(e.Pointer()))return;
            self->pointer=raw.PointerId();self->drag(L"down",raw.Position());e.Handled(true);
        }});
        header.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){self->drag(L"move",e.GetCurrentPoint(self->host).Position());e.Handled(true);}});
        PointerGesture::OnCompleted(header,[weak](PointerRoutedEventArgs const& e,bool cancel){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){
            self->pointer.reset();
            if(cancel)self->drag(L"cancel");else{self->drag(L"up",e.GetCurrentPoint(self->host).Position());self->header.ReleasePointerCaptures();}
            e.Handled(true);
        }});
        frame.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock();self&&!composingKey(e)&&e.Key()==Windows::System::VirtualKey::Escape){
            self->curve(O({{L"kind",S(L"key")},{L"epoch",object(object(self->view(),L"editor"),L"controls").GetNamedValue(L"epoch")},
                {L"key_event",S(L"Escape")},{L"pressed",B(true)},{L"repeat",B(e.KeyStatus().WasKeyDown)},{L"modifiers",O({{L"command",B(false)},{L"shift",B(false)},{L"alt",B(false)}})}}));e.Handled(true);
        }});
        host.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->Publish();});
        slot.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->Publish();});
        attach();frame.Visibility(Visibility::Collapsed);
    }
    void attach(Canvas const& parent){
        for(auto item:{shadow.Root().as<FrameworkElement>(),frame.as<FrameworkElement>()})if(auto previous=item.Parent().try_as<Panel>()){uint32_t index;if(previous.Children().IndexOf(item,index))previous.Children().RemoveAt(index);}
        presenter=parent;attach();
    }
    void attach(){auto parent=presenter?presenter:host;uint32_t index;if(!parent.Children().IndexOf(shadow.Root(),index))parent.Children().Append(shadow.Root());if(!parent.Children().IndexOf(frame,index))parent.Children().Append(frame);}
    A inputRegions(UIElement const& reference)const{
        A result;if(!exposed||frame.Visibility()!=Visibility::Visible||!frame.IsLoaded()||(presenter&&presenter.Visibility()!=Visibility::Visible))return result;
        auto bounds=frame.TransformToVisual(reference).TransformBounds({0,0,float(frame.ActualWidth()),float(frame.ActualHeight())});
        if(bounds.Width>0&&bounds.Height>0)result.Append(O({{L"x",N(bounds.X)},{L"y",N(bounds.Y)},{L"width",N(bounds.Width)},{L"height",N(bounds.Height)}}));return result;
    }
    void drawDirection(){
        auto model=object(data->state,L"tilt_calibration");
        auto key=model.GetNamedValue(L"input_direction",JsonValue::CreateNullValue()).Stringify()+model.GetNamedValue(L"adjusted_direction",JsonValue::CreateNullValue()).Stringify()+data->theme();
        if(drawn==key)return;drawn=key;circle.Stroke(data->brush(L"text"));arrows[0].Stroke(data->brush(L"text"));arrows[1].Stroke(accent(data));
        for(size_t i=0;i<arrows.size();++i){
            auto value=array(model,i?L"adjusted_direction":L"input_direction");arrows[i].Visibility(value.Size()==2?Visibility::Visible:Visibility::Collapsed);if(value.Size()!=2)continue;
            double x=value.GetNumberAt(0)*31,y=value.GetNumberAt(1)*31,length=std::hypot(x,y);double ux=length>0?x/length:0,uy=length>0?y/length:0;
            std::array<Windows::Foundation::Point,5> points{{{36,36},{float(36+x),float(36+y)},{float(36+x-ux*6-uy*3),float(36+y-uy*6+ux*3)},
                {float(36+x),float(36+y)},{float(36+x-ux*6+uy*3),float(36+y-uy*6-ux*3)}}};arrows[i].Points().ReplaceAll(points);
        }
    }
    void Publish(){
        if(std::exchange(publishing,true))return;
        struct End{bool& value;~End(){value=false;}} end{publishing};
        auto model=view();
        if(!model.Size()){exposed=false;presentedTarget=L"";frame.Visibility(Visibility::Collapsed);shadow.Layout({-100000,0,1,1},1099,false);if(!built.empty()){built=L"";bindings.clear();slot.Child(nullptr);measured=L"";}return;}
        if(presentedTarget!=target()){presentedTarget=target();exposed=false;}
        frame.Opacity(exposed?1:0);frame.IsHitTestVisible(exposed);
        auto key=target()+data->theme()+object(data->state,L"palette").Stringify();
        bool rebuilt=key!=built;
        if(rebuilt){built=key;bindings.clear();
            if(auto parent=directionControl.Parent().try_as<Panel>()){uint32_t index;if(parent.Children().IndexOf(directionControl,index))parent.Children().RemoveAt(index);}
            slot.Child(CapyEffects::ResponseCurveField(data,bindings,target(),tilt()?directionControl.as<FrameworkElement>():nullptr));drawn=L"";}
        auto prefix=L"pen-"+target();
        AutomationProperties::SetAutomationId(frame,prefix+L"-dialog");AutomationProperties::SetAutomationId(title,prefix+L"-title");AutomationProperties::SetAutomationId(close,prefix+L"-close");
        title.Text(str(model,L"title"));title.Foreground(data->brush(L"text"));surface.Fill(data->tint(L"panel",255));
        close.Content(icon(L"window-close",data->theme()));AutomationProperties::SetName(close,str(model,L"close"));tooltip(close,str(model,L"close"));AutomationProperties::SetName(frame,str(model,L"title"));
        for(auto [control,id]:{std::pair{firmer,L"firmer"},std::pair{lighter,L"lighter"},std::pair{reset,L"reset"},std::pair{cancel,L"cancel"},std::pair{apply,L"apply"}}){auto text=str(model,id);if(tilt()&&(control==firmer||control==lighter)){if(rebuilt)control.Content(icon(control==firmer?L"minus":L"plus",data->theme(),16));tooltip(control,text);}else if(unbox_value_or<hstring>(control.Content(),L"")!=text)control.Content(box_value(text));AutomationProperties::SetName(control,text);AutomationProperties::SetAutomationId(control,prefix+L"-"+id);}
        firmer.IsEnabled(flag(model,L"firmer_enabled"));lighter.IsEnabled(flag(model,L"lighter_enabled"));apply.IsEnabled(!tilt()||flag(object(data->state,L"tilt_calibration"),L"ready"));apply.Background(accent(data));apply.Foreground(data->brush(L"accent_foreground"));
        for(auto control:{firmer,lighter}){control.Width(tilt()?36.:std::numeric_limits<double>::quiet_NaN());control.Padding(tilt()?Thickness{0}:Thickness{12,0,12,0});auto corners=data->controlCorner();if(tilt()){if(control==firmer){corners.TopRight=0;corners.BottomRight=0;}else{corners.TopLeft=0;corners.BottomLeft=0;}}control.CornerRadius(corners);}
        sensitivity.ColumnSpacing(tilt()?0:8);Grid::SetColumn(sensitivity,tilt()?1:0);Grid::SetColumnSpan(sensitivity,tilt()?1:4);
        for(auto text:{sensitivityTitle,directionTitle}){text.Visibility(tilt()?Visibility::Visible:Visibility::Collapsed);text.Foreground(data->brush(L"text"));}
        sensitivityTitle.Text(str(model,L"sensitivity"));
        device.Visibility(tilt()?Visibility::Visible:Visibility::Collapsed);directionSection.Visibility(tilt()?Visibility::Visible:Visibility::Collapsed);
        if(tilt()){
            auto tiltView=object(data->state,L"tilt_calibration"),axis=object(tiltView,L"axes");device.Text(str(tiltView,L"device"));device.Foreground(data->brush(L"text"));directionTitle.Text(str(tiltView,L"direction"));directionTitle.Foreground(data->brush(L"text"));
            AutomationProperties::SetAutomationId(directionControl,L"pen-tilt-direction");AutomationProperties::SetName(directionControl,str(tiltView,L"direction"));AutomationProperties::SetAutomationId(device,L"pen-tilt-device");projecting=true;
            std::array<hstring,3> names{L"reverse_horizontal",L"reverse_vertical",L"swap"},ids{L"reverse-horizontal",L"reverse-vertical",L"swap-axes"},icons{L"flip-horizontal",L"flip-vertical",L"swap-orientation"};
            for(size_t i=0;i<axes.size();++i){auto label=str(tiltView,names[i]==L"swap"?L"swap_axes":names[i].c_str());if(rebuilt){axes[i].Content(icon(icons[i],data->theme(),16));buttonColors(data,axes[i]);}axes[i].IsChecked(flag(axis,names[i].c_str()));axes[i].Background(flag(axis,names[i].c_str())?selected(data):data->brush(L"input"));AutomationProperties::SetAutomationId(axes[i],L"pen-tilt-"+ids[i]);AutomationProperties::SetName(axes[i],label);tooltip(axes[i],label);}
            projecting=false;drawDirection();
        }
        for(auto const& bind:bindings)bind();
        double width=std::min(std::max(384.,num(object(model,L"bounds"),L"width")),host.ActualWidth());if(width<=0)return;
        frame.Visibility(Visibility::Visible);frame.Width(width);frame.Height(std::numeric_limits<double>::quiet_NaN());scroll.MaxHeight(std::max(0.,host.ActualHeight()-header.ActualHeight()-footer.ActualHeight()-32));
        frame.Measure({float(width),INFINITY});float height=std::min(frame.DesiredSize().Height,float(host.ActualHeight()));frame.Height(height);
        auto b=object(model,L"bounds");translation.X(num(b,L"x"));translation.Y(num(b,L"y"));
        std::array<float,4> radii{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};auto geometry=extent(width,height).Stringify();
        if(geometry!=shape){shape=geometry;surface.Data(squircleRectangle(float(width),height,radii));shadow.Shape(squircleRectangle(float(width),height,radii),float(width),height,8,2,.16f);shadow.Cut(radii);}
        auto measurement=extent(width,height).Stringify()+extent(host.ActualWidth(),host.ActualHeight()).Stringify();
        bool fitted=std::abs(num(b,L"width")-width)<.5&&std::abs(num(b,L"height")-height)<.5;
        exposed|=fitted;frame.Opacity(exposed?1:0);frame.IsHitTestVisible(exposed);
        shadow.Layout({float(num(b,L"x")),float(num(b,L"y")),float(width),height},1099,exposed);
        if((!fitted||measurement!=measured)&&!measuring){measuring=true;auto request=packet(O({{L"kind",S(L"measure")},{L"extent",extent(width,height)},{L"viewport",extent(host.ActualWidth(),host.ActualHeight())}}),tilt());auto mode=target();auto weak=weak_from_this();
            host.DispatcherQueue().TryEnqueue([weak,request,measurement,mode]{if(auto self=weak.lock()){self->measuring=false;if(!self->view().Size()||self->target()!=mode)return;self->measured=measurement;self->data->dispatch(request);}});
        }
    }
};
}
