#pragma once
#include "ColorForm.h"
#include "Checker.h"
#include <array>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

namespace CapyUi {
struct ColorPair {
    Viewbox root;
    Microsoft::UI::Xaml::Shapes::Ellipse foreground,background;
    hstring fillKey;
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
        auto pair=object(data->model,L"paint_pair");
        double scale=(root.XamlRoot()?root.XamlRoot().RasterizationScale():1.)*size/16.;
        auto next=O({{L"swatches",array(pair,L"swatches")},{L"checker_cell",N(num(pair,L"checker_cell"))}}).Stringify()+L"/"+to_hstring(scale);
        bool changed=next!=fillKey;if(changed)fillKey=next;
        auto front=str(pair,L"front_swatch");
        for(auto const& [slot,shape]:std::array<std::pair<hstring,Microsoft::UI::Xaml::Shapes::Ellipse>,2>{{{L"foreground",foreground},{L"background",background}}}){
            int z=slot==front?1:0;if(Canvas::GetZIndex(shape)!=z)Canvas::SetZIndex(shape,z);
            if(!changed)continue;
            auto swatch=find(array(pair,L"swatches"),L"slot",slot);auto rgba=array(swatch,L"rgba");
            if(rgba.Size()==4&&rgba.GetNumberAt(3)==1){shape.Fill(fill(previewColor(rgba)));continue;}
            auto checker=array(swatch,L"checker");if(checker.Size()!=2)continue;
            ImageBrush pixels;pixels.ImageSource(checkerBitmap(shape.Width()-1,scale,previewColor(checker.GetArrayAt(0)),previewColor(checker.GetArrayAt(1)),num(pair,L"checker_cell")));
            pixels.Stretch(Stretch::Fill);shape.Fill(pixels);
        }
    }
};
}
