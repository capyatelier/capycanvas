#include "pch.h"
#include "WorkspaceShadow.h"
#include <winrt/Microsoft.UI.Xaml.Hosting.h>

namespace CapyUi {
using namespace Microsoft::UI::Composition;
using Microsoft::UI::Xaml::Hosting::ElementCompositionPreview;
namespace {constexpr float padding=48;}
struct WorkspaceShadow::State {
    SpriteVisual visual{nullptr};
    DropShadow shadow{nullptr};
    float width=0,height=0,blur=8,offset=2,opacity=.16f;
    void apply(){
        if(!visual)return;
        visual.Size({width,height});visual.Offset({padding,padding,0});
        shadow.BlurRadius(blur);shadow.Offset({0,offset,0});shadow.Opacity(opacity);
    }
};
WorkspaceShadow::WorkspaceShadow():state(std::make_shared<State>()){
    root.IsHitTestVisible(false);
    AutomationProperties::SetAccessibilityView(root,Automation::Peers::AccessibilityView::Raw);
    // The mask has no navigator cutouts. Hide its parent so only the shadow is
    // painted; GetAlphaMask reads the shape's own fill, independently of ancestors.
    mask.Fill(fill({255,255,255,255}));maskHost.Opacity(0);maskHost.Children().Append(mask);
    Canvas::SetLeft(maskHost,padding);Canvas::SetTop(maskHost,padding);root.Children().Append(maskHost);
    root.Loaded([state=state,host=make_weak(root),shape=make_weak(mask)](auto&&,auto&&){
        auto root=host.get();auto mask=shape.get();if(!root||!mask)return;
        auto compositor=ElementCompositionPreview::GetElementVisual(root).Compositor();
        state->shadow=compositor.CreateDropShadow();state->shadow.Color({255,0,0,0});
        state->shadow.Mask(mask.GetAlphaMask());
        state->visual=compositor.CreateSpriteVisual();state->visual.Shadow(state->shadow);
        state->apply();ElementCompositionPreview::SetElementChildVisual(root,state->visual);
    });
}
void WorkspaceShadow::Layout(Windows::Foundation::Rect bounds,int order,bool visible){
    root.Width(bounds.Width+2*padding);root.Height(bounds.Height+2*padding);
    Canvas::SetLeft(root,bounds.X-padding);Canvas::SetTop(root,bounds.Y-padding);
    Canvas::SetZIndex(root,order);root.Visibility(visible?Visibility::Visible:Visibility::Collapsed);
}
void WorkspaceShadow::Shape(Geometry const& geometry,float width,float height,float blur,float offset,float opacity){
    mask.Data(geometry);mask.Width(width);mask.Height(height);
    state->width=width;state->height=height;state->blur=blur;state->offset=offset;state->opacity=opacity;state->apply();
}
}
