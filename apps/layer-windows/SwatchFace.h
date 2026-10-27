#pragma once
#include "ColorWheel.h"
#include "Checker.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

namespace CapyUi {
struct SwatchFace {
    Grid root;
    Shapes::Ellipse back,feedback,checker,paint,edge;
    hstring checkerKey;
    SwatchFace(){root.IsHitTestVisible(false);root.Children().Append(back);root.Children().Append(feedback);root.Children().Append(checker);root.Children().Append(paint);root.Children().Append(edge);}
    void Update(std::shared_ptr<WorkspaceData> const& data,A const& rgba,double size,double scale,bool selected,bool foreground,bool hovered){
        root.Width(size);root.Height(size);double pad=foreground?3:1;
        checker.Margin({pad,pad,pad,pad});paint.Margin({pad,pad,pad,pad});
        auto palette=object(data->state,L"palette");auto key=to_hstring(size)+L":"+to_hstring(scale)+L":"+to_hstring(pad)+L":"+str(palette,L"checker_light")+str(palette,L"checker_dark");
        if(key!=checkerKey){checkerKey=key;ImageBrush pixels;pixels.ImageSource(checkerBitmap(size-2*pad,scale,color(str(palette,L"checker_light")),color(str(palette,L"checker_dark"))));pixels.Stretch(Stretch::Fill);checker.Fill(pixels);}
        back.Fill(data->brush(L"panel"));auto ink=color(str(palette,L"text"));if(!(selected||hovered))ink.A=64;
        edge.Stroke(fill(ink));edge.StrokeThickness(selected||hovered?2:1);paint.Fill(fill(CapyWheel::rgba(rgba)));
    }
};
}
