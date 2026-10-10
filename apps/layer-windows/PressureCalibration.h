#pragma once
#include "EffectControls.h"
#include "WorkspaceShadow.h"
#include "WorkspaceGeometry.h"

namespace CapyUi {
struct PressureCalibration:std::enable_shared_from_this<PressureCalibration>{
    std::shared_ptr<WorkspaceData> data;
    Canvas host{nullptr};
    ContentControl frame;
    Grid layers,header;
    StackPanel body,content;
    Shapes::Path surface,strip;
    ScrollViewer scroll;
    Border slot;
    TextBlock title{nullptr};
    Button close{nullptr},firmer{nullptr},lighter{nullptr},reset{nullptr},cancel{nullptr},apply{nullptr};
    WorkspaceShadow shadow;
    Bindings bindings;
    TranslateTransform translation;
    std::optional<uint32_t> pointer;
    hstring built,measured,shape;
    bool measuring=false;
    J view()const{return object(data->state,L"pressure_calibration");}
    A extent(double width,double height)const{return CapyEffects::values({width,height});}
    void send(J action)const{data->dispatch(O({{L"type",S(L"pressure_calibration")},{L"action",action}}));}
    void drag(hstring const& phase,Windows::Foundation::Point at={}){
        send(O({{L"kind",S(L"drag")},{L"phase",S(phase)},{L"position",extent(at.X,at.Y)},{L"viewport",extent(host.ActualWidth(),host.ActualHeight())}}));
    }
    void init(Canvas const& root){
        host=root;auto weak=weak_from_this();
        title=label(data,L"",true);
        close=button(data,L"",[weak]{if(auto self=weak.lock())self->send(O({{L"kind",S(L"cancel")}}));});
        close.Width(24);close.Height(24);close.MinWidth(0);close.MinHeight(0);close.Padding({0});close.VerticalAlignment(VerticalAlignment::Center);
        close.Margin({4,4,6,4});
        AutomationProperties::SetAutomationId(close,L"pen-pressure-close");
        ColumnDefinition labelColumn;labelColumn.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(labelColumn);
        ColumnDefinition closeColumn;closeColumn.Width({1,GridUnitType::Auto});header.ColumnDefinitions().Append(closeColumn);
        title.Margin({12,0,0,0});title.VerticalAlignment(VerticalAlignment::Center);header.MinHeight(34);
        header.Children().Append(title);Grid::SetColumn(close,1);header.Children().Append(close);body.Children().Append(header);
        auto action=[weak](wchar_t const* kind){if(auto self=weak.lock())self->send(O({{L"kind",S(kind)}}));};
        firmer=button(data,L"",[weak]{if(auto self=weak.lock())self->send(O({{L"kind",S(L"sensitivity")},{L"lighter",B(false)}}));});
        lighter=button(data,L"",[weak]{if(auto self=weak.lock())self->send(O({{L"kind",S(L"sensitivity")},{L"lighter",B(true)}}));});
        reset=button(data,L"",[weak]{if(auto self=weak.lock())self->data->dispatch(O({{L"type",S(L"curve_editor")},{L"target",O({{L"kind",S(L"pressure")}})},{L"action",O({{L"kind",S(L"reset")}})}}));});
        cancel=button(data,L"",[action]{action(L"cancel");});apply=button(data,L"",[action]{action(L"apply");});
        for(auto [control,id]:{std::pair{firmer,L"firmer"},std::pair{lighter,L"lighter"},std::pair{reset,L"reset"},std::pair{cancel,L"cancel"},std::pair{apply,L"apply"}}){
            control.MinWidth(0);control.Padding({10,5,10,5});control.MinHeight(32);AutomationProperties::SetAutomationId(control,hstring(L"pen-pressure-")+id);
        }
        Grid sensitivity;sensitivity.ColumnSpacing(8);
        for(int i=0;i<2;i++){ColumnDefinition column;column.Width({1,GridUnitType::Star});sensitivity.ColumnDefinitions().Append(column);}
        firmer.HorizontalAlignment(HorizontalAlignment::Stretch);lighter.HorizontalAlignment(HorizontalAlignment::Stretch);
        Grid::SetColumn(lighter,1);sensitivity.Children().Append(firmer);sensitivity.Children().Append(lighter);
        Grid actions;actions.ColumnSpacing(6);
        for(int i=0;i<4;i++){ColumnDefinition column;column.Width({1,i==1?GridUnitType::Star:GridUnitType::Auto});actions.ColumnDefinitions().Append(column);}
        Grid::SetColumn(cancel,2);Grid::SetColumn(apply,3);actions.Children().Append(reset);actions.Children().Append(cancel);actions.Children().Append(apply);
        content.Spacing(10);content.Padding({12,12,12,12});content.Children().Append(slot);content.Children().Append(sensitivity);content.Children().Append(actions);
        scroll.Content(content);scroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);
        body.Children().Append(scroll);surface.IsHitTestVisible(false);strip.IsHitTestVisible(false);header.Background(clear());
        layers.Children().Append(surface);layers.Children().Append(strip);layers.Children().Append(body);
        frame.Content(layers);frame.HorizontalContentAlignment(HorizontalAlignment::Stretch);frame.VerticalContentAlignment(VerticalAlignment::Stretch);
        frame.RenderTransform(translation);AutomationProperties::SetAutomationId(frame,L"pen-pressure-dialog");Canvas::SetZIndex(frame,1100);
        header.ManipulationMode(ManipulationModes::None);
        header.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node))if(node==self->close)return;
            auto raw=e.GetCurrentPoint(self->host);if(!raw.IsInContact()||self->pointer)return;
            if(raw.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!raw.Properties().IsLeftButtonPressed())return;
            if(!self->header.CapturePointer(e.Pointer()))return;
            self->pointer=raw.PointerId();self->drag(L"down",raw.Position());e.Handled(true);
        }});
        header.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){self->drag(L"move",e.GetCurrentPoint(self->host).Position());e.Handled(true);}});
        header.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){self->pointer.reset();self->drag(L"up",e.GetCurrentPoint(self->host).Position());self->header.ReleasePointerCaptures();e.Handled(true);}});
        auto cancelDrag=[weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->pointer){self->pointer.reset();self->drag(L"cancel");}};
        header.PointerCanceled(cancelDrag);header.PointerCaptureLost(cancelDrag);
        frame.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock();self&&!composingKey(e)&&e.Key()==Windows::System::VirtualKey::Escape){
            self->data->dispatch(O({{L"type",S(L"curve_editor")},{L"target",O({{L"kind",S(L"pressure")}})},{L"action",O({{L"kind",S(L"key")},{L"epoch",object(object(self->view(),L"editor"),L"controls").GetNamedValue(L"epoch")},
                {L"key_event",S(L"Escape")},{L"pressed",B(true)},{L"repeat",B(e.KeyStatus().WasKeyDown)},{L"modifiers",O({{L"command",B(false)},{L"shift",B(false)},{L"alt",B(false)}})}})}}));e.Handled(true);
        }});
        host.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->Publish();});
        attach();frame.Visibility(Visibility::Collapsed);
    }
    void attach(){uint32_t index;if(!host.Children().IndexOf(shadow.Root(),index))host.Children().Append(shadow.Root());if(!host.Children().IndexOf(frame,index))host.Children().Append(frame);}
    void Publish(){
        auto model=view();
        if(!model.Size()){frame.Visibility(Visibility::Collapsed);shadow.Layout({-100000,0,1,1},1099,false);if(!built.empty()){built=L"";bindings.clear();slot.Child(nullptr);measured=L"";}return;}
        auto key=data->theme()+object(data->state,L"palette").Stringify();
        if(key!=built){built=key;bindings.clear();slot.Child(CapyEffects::PressureCurveField(data,bindings));}
        title.Text(str(model,L"title"));title.Foreground(data->brush(L"text"));strip.Fill(data->brush(L"tabbar"));surface.Fill(data->brush(L"panel"));
        close.Content(icon(L"window-close",data->theme()));AutomationProperties::SetName(close,str(model,L"close"));tooltip(close,str(model,L"close"));
        AutomationProperties::SetName(frame,str(model,L"title"));
        for(auto [control,id]:{std::pair{firmer,L"firmer"},std::pair{lighter,L"lighter"},std::pair{reset,L"reset"},std::pair{cancel,L"cancel"},std::pair{apply,L"apply"}}){auto text=str(model,id);if(unbox_value_or<hstring>(control.Content(),L"")!=text)control.Content(box_value(text));AutomationProperties::SetName(control,text);}
        firmer.IsEnabled(flag(model,L"firmer_enabled"));lighter.IsEnabled(flag(model,L"lighter_enabled"));apply.Background(accent(data));apply.Foreground(data->brush(L"accent_foreground"));
        for(auto const& bind:bindings)bind();
        auto b=object(model,L"bounds");double width=std::min(num(b,L"width"),host.ActualWidth());if(width<=0)return;
        frame.Visibility(Visibility::Visible);frame.Width(width);frame.Height(std::numeric_limits<double>::quiet_NaN());scroll.MaxHeight(std::max(0.,host.ActualHeight()-34));
        frame.Measure({float(width),INFINITY});float height=std::min(frame.DesiredSize().Height,float(host.ActualHeight()));frame.Height(height);
        translation.X(num(b,L"x"));translation.Y(num(b,L"y"));
        std::array<float,4> radii{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};auto geometry=extent(width,height).Stringify();
        if(geometry!=shape){shape=geometry;surface.Data(squircleRectangle(float(width),height,radii));
            strip.Data(squircleRectangle(float(width),header.DesiredSize().Height,{SurfaceRadius,SurfaceRadius,0,0}));
            shadow.Shape(squircleRectangle(float(width),height,radii),float(width),height,8,2,.16f);shadow.Cut(radii);}
        shadow.Layout({float(num(b,L"x")),float(num(b,L"y")),float(width),height},1099,true);
        auto measurement=extent(width,height).Stringify()+extent(host.ActualWidth(),host.ActualHeight()).Stringify();
        if(measurement!=measured&&!measuring){measuring=true;auto weak=weak_from_this();host.DispatcherQueue().TryEnqueue([weak,measurement,width,height]{if(auto self=weak.lock()){
            self->measuring=false;if(!self->view().Size())return;self->measured=measurement;
            self->send(O({{L"kind",S(L"measure")},{L"extent",self->extent(width,height)},{L"viewport",self->extent(self->host.ActualWidth(),self->host.ActualHeight())}}));
        }});}
    }
};
}
