#pragma once
#include "ColorForm.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

namespace CapyUi {
inline J paintColors(std::shared_ptr<WorkspaceData> const& data){
    auto masked=object(object(object(data->state,L"layer_tools"),L"mask_editing"),L"colors");
    return masked.Size()?masked:object(data->state,L"colors");
}
struct ColorPair {
    Viewbox root;
    Microsoft::UI::Xaml::Shapes::Ellipse foreground,background;
    hstring key;
    explicit ColorPair(std::shared_ptr<WorkspaceData> const& data){
        Canvas art;art.Width(16);art.Height(16);
        auto circle=[&](Microsoft::UI::Xaml::Shapes::Ellipse const& shape,double cx,double cy,double r){
            shape.Width(2*r+1);shape.Height(2*r+1);Canvas::SetLeft(shape,cx-r-.5);Canvas::SetTop(shape,cy-r-.5);
            shape.StrokeThickness(1);shape.Stroke(data->brush(L"text"));art.Children().Append(shape);
        };
        circle(background,11,11,4.25);circle(foreground,6.75,6.75,6);
        root.Child(art);root.IsHitTestVisible(false);
    }
    void Update(std::shared_ptr<WorkspaceData> const& data,double size){
        root.Width(size);root.Height(size);
        auto colors=paintColors(data);
        A pair;pair.Append(object(colors,L"foreground"));pair.Append(object(colors,L"background"));
        auto next=pair.Stringify();if(next==key)return;key=next;
        auto previews=colorUi(O({{L"type",S(L"preview")},{L"colors",pair}}));
        if(previews.ValueType()!=JsonValueType::Array||previews.GetArray().Size()!=2)return;
        foreground.Fill(fill(displayColor(previews.GetArray().GetObjectAt(0))));
        background.Fill(fill(displayColor(previews.GetArray().GetObjectAt(1))));
    }
};
}
