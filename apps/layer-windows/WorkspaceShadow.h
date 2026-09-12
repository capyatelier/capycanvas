#pragma once
#include "UiControls.h"
#include <winrt/Microsoft.UI.Composition.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

namespace CapyUi {
// A retained compositor shadow beside the clipped panel. The padded root allows
// overview occlusion to preserve the entire blur while excluding GPU image holes.
class WorkspaceShadow {
public:
    WorkspaceShadow();
    Canvas Root()const{return root;}
    void Layout(Windows::Foundation::Rect bounds,int order,bool visible);
    void Shape(Geometry const& geometry,float width,float height,float blur,float offset,float opacity);
private:
    Canvas root,maskHost;
    Microsoft::UI::Xaml::Shapes::Path mask;
    struct State;
    std::shared_ptr<State> state;
};
}
