#pragma once
#include "UiControls.h"
#include <winrt/Microsoft.UI.Composition.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <array>
#include <vector>

namespace CapyUi {
Microsoft::UI::Composition::CompositionPath CompositionPathFromContours(std::vector<std::vector<winrt::Windows::Foundation::Point>> const& contours);
void ClipOutside(UIElement const&,float width,float height,std::array<float,4> radii,bool squircle);
void ClipSurface(UIElement const&,float width,float height,std::array<float,4> radii,bool squircle);
// A retained compositor shadow beside the clipped panel. The padded root allows
// overview occlusion to preserve the entire blur while excluding GPU image holes.
class WorkspaceShadow {
public:
    WorkspaceShadow();
    Canvas Root()const{return root;}
    void Layout(winrt::Windows::Foundation::Rect bounds,int order,bool visible);
    void Shape(Geometry const& geometry,float width,float height,float blur,float offset,float opacity);
    void Rounded(float width,float height,std::array<float,4> radii,float blur,float offset,float opacity);
    void Cut(std::array<float,4> radii,std::vector<winrt::Windows::Foundation::Rect> extra={});
    void Cut(std::vector<winrt::Windows::Foundation::Point> contour);
    void Uncut();
private:
    Canvas root,maskHost;
    Microsoft::UI::Xaml::Shapes::Path mask;
    struct State;
    std::shared_ptr<State> state;
};
}
