#pragma once
#include "ColorEditor.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceShadow.h"
#include <winrt/Microsoft.UI.Input.h>

namespace CapyUi {
struct ColorStrip:std::enable_shared_from_this<ColorStrip>{
    static constexpr double Offscreen=-100000,Width=248;
    std::shared_ptr<WorkspaceData> data;
    Canvas host{nullptr};
    Button root{nullptr};
    Grid frame;
    Shapes::Path surface;
    Border original,sample;
    TextBlock hex,intensity,space,values;
    WorkspaceShadow shadow;
    hstring corner=L"top_right";
    std::optional<Windows::Foundation::Point> hover;
    bool visible=false;

    void init(Canvas const& canvas){
        host=canvas;auto weak=weak_from_this();
        root=button(data,L"",[weak]{if(auto self=weak.lock())self->tapped();});
        root.Padding({0,0,0,0});root.Width(Width);root.HorizontalContentAlignment(HorizontalAlignment::Stretch);root.VerticalContentAlignment(VerticalAlignment::Stretch);
        root.Resources().Insert(box_value(L"ButtonBackgroundPointerOver"),clear());root.Resources().Insert(box_value(L"ButtonBackgroundPressed"),clear());
        AutomationProperties::SetAutomationId(root,L"edit-color-strip");
        Grid pair;pair.CornerRadius({6,6,6,6});
        for(int column=0;column<2;++column){ColumnDefinition definition;definition.Width({20,GridUnitType::Pixel});pair.ColumnDefinitions().Append(definition);}
        original.Height(40);sample.Height(40);Grid::SetColumn(sample,1);pair.Children().Append(original);pair.Children().Append(sample);
        for(auto const& text:{hex,intensity,space,values}){text.FontFamily(FontFamily(L"Segoe UI"));text.IsHitTestVisible(false);text.VerticalAlignment(VerticalAlignment::Center);}
        hex.FontSize(15);hex.FontWeight(Windows::UI::Text::FontWeights::SemiBold());space.Opacity(.6);
        intensity.HorizontalAlignment(HorizontalAlignment::Right);values.HorizontalAlignment(HorizontalAlignment::Right);
        StackPanel text;text.Spacing(2);text.VerticalAlignment(VerticalAlignment::Center);
        for(auto const& [start,end]:{std::pair{hex,intensity},std::pair{space,values}}){
            Grid line;line.ColumnSpacing(8);
            for(auto width:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){ColumnDefinition definition;definition.Width(width);line.ColumnDefinitions().Append(definition);}
            Grid::SetColumn(end,1);line.Children().Append(start);line.Children().Append(end);text.Children().Append(line);
        }
        Grid content;content.ColumnSpacing(10);content.Padding({8,8,12,8});
        for(auto width:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star}}){ColumnDefinition definition;definition.Width(width);content.ColumnDefinitions().Append(definition);}
        Grid::SetColumn(text,1);content.Children().Append(pair);content.Children().Append(text);
        surface.IsHitTestVisible(false);frame.Children().Append(surface);frame.Children().Append(content);root.Content(frame);
        root.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->hovered(e);});
        root.PointerEntered([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->hovered(e);});
        root.PointerExited([weak](auto&&,auto&&){if(auto self=weak.lock()){self->hover.reset();}});
        Canvas::SetZIndex(root,950);
        attach();present();
    }
    void attach(){
        uint32_t index;
        if(!host.Children().IndexOf(shadow.Root(),index))host.Children().Append(shadow.Root());
        if(!host.Children().IndexOf(root,index))host.Children().Append(root);
    }
    void tapped(){
        if(flag(object(data->colorPreview,L"picker"),L"editor"))data->dispatch(O({{L"type",S(L"color_picker")},{L"action",O({{L"kind",S(L"toggle")}})}}));
    }
    void hovered(PointerRoutedEventArgs const& e){
        auto kind=e.Pointer().PointerDeviceType();
        if(kind==Microsoft::UI::Input::PointerDeviceType::Touch)return;
        hover=e.GetCurrentPoint(host).Position();present();
    }
    void Show(J const& content){
        original.Background(fill(previewColor(array(content,L"original"))));sample.Background(fill(previewColor(array(content,L"sample"))));
        hex.Text(str(content,L"hex"));intensity.Text(str(content,L"intensity"));space.Text(str(content,L"label"));values.Text(str(content,L"values"));
        for(auto const& text:{hex,intensity,space,values})text.Foreground(data->brush(L"text"));
        auto tip=data->caption(L"color",L"picking_strip");AutomationProperties::SetName(root,tip);tooltip(root,tip);
        surface.Fill(data->brush(L"panel"));
        visible=true;present();
    }
    void Hide(){
        hover.reset();
        if(visible){visible=false;present();}
    }
    void present(){
        auto camera=object(data->state,L"camera");auto area=array(camera,L"work_area"),viewport=array(camera,L"viewport");
        if(!visible||area.Size()!=4||viewport.Size()<1||host.ActualWidth()<=0){
            Canvas::SetLeft(root,Offscreen);root.Opacity(0);root.IsHitTestVisible(false);
            shadow.Layout({float(Offscreen),0,1,1},949,false);
            return;
        }
        root.Measure({float(Width),INFINITY});
        float width=float(Width),height=std::ceil(root.DesiredSize().Height);
        double scale=viewport.GetNumberAt(0)/host.ActualWidth();
        A avoid,size;size.Append(N(width*scale));size.Append(N(height*scale));
        auto point=object(data->colorPreview,L"picker").GetNamedValue(L"sample_point",JsonValue::CreateNullValue());
        if(point.ValueType()==JsonValueType::Array)avoid.Append(point);
        if(hover){A at;at.Append(N(hover->X*scale));at.Append(N(hover->Y*scale));avoid.Append(at);}
        auto placed=colorUi(data->localization.get(),O({{L"type",S(L"strip_placement")},{L"area",area},{L"size",size},{L"scale",N(scale)},{L"avoid",avoid},{L"corner",S(corner)}})).GetObject();
        corner=str(placed,L"corner",corner);auto origin=array(placed,L"origin");
        double left=std::round(origin.GetNumberAt(0)/scale),top=std::round(origin.GetNumberAt(1)/scale);
        Canvas::SetLeft(root,left);Canvas::SetTop(root,top);root.Height(height);
        std::array<float,4> radii{12,12,12,12};
        surface.Data(squircleRectangle(width,height,radii));
        shadow.Shape(squircleRectangle(width,height,radii),width,height,8,2,.27f);shadow.Cut(radii);
        shadow.Layout({float(left),float(top),width,height},949,true);
        root.Opacity(1);root.IsHitTestVisible(true);
    }
};
}
