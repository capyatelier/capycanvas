#include "pch.h"
#include "WorkspaceShadow.h"
#include <winrt/Microsoft.UI.Xaml.Hosting.h>
#include <winrt/Windows.Graphics.h>
#include <d2d1.h>
#include <windows.graphics.interop.h>
#include <optional>

namespace CapyUi {
using namespace Microsoft::UI::Composition;
using Microsoft::UI::Xaml::Hosting::ElementCompositionPreview;
namespace {
constexpr float padding=48;
struct GeometrySource:winrt::implements<GeometrySource,winrt::Windows::Graphics::IGeometrySource2D,ABI::Windows::Graphics::IGeometrySource2DInterop>{
    winrt::com_ptr<ID2D1Geometry> geometry;
    explicit GeometrySource(winrt::com_ptr<ID2D1Geometry> value):geometry(std::move(value)){}
    HRESULT __stdcall GetGeometry(ID2D1Geometry** value)noexcept final{geometry.copy_to(value);return S_OK;}
    HRESULT __stdcall TryGetGeometryUsingFactory(ID2D1Factory*,ID2D1Geometry** value)noexcept final{*value=nullptr;return E_NOTIMPL;}
};
ID2D1Factory* factory(){
    static winrt::com_ptr<ID2D1Factory> value=[]{winrt::com_ptr<ID2D1Factory> created;winrt::check_hresult(D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED,created.put()));return created;}();
    return value.get();
}
struct ShadowCut{std::array<float,4> radii{};std::vector<winrt::Windows::Foundation::Rect> extra;};
winrt::com_ptr<ID2D1Geometry> outside(float width,float height,ShadowCut const& cut){
    winrt::com_ptr<ID2D1PathGeometry> path;winrt::check_hresult(factory()->CreatePathGeometry(path.put()));
    winrt::com_ptr<ID2D1GeometrySink> sink;winrt::check_hresult(path->Open(sink.put()));sink->SetFillMode(D2D1_FILL_MODE_ALTERNATE);
    auto rectangle=[&](float x,float y,float w,float h){
        sink->BeginFigure({x,y},D2D1_FIGURE_BEGIN_FILLED);sink->AddLine({x+w,y});sink->AddLine({x+w,y+h});sink->AddLine({x,y+h});sink->EndFigure(D2D1_FIGURE_END_CLOSED);
    };
    rectangle(-padding,-padding,width+2*padding,height+2*padding);
    auto radii=cut.radii;for(auto& radius:radii)radius=std::min(radius,std::max(0.f,std::min(width,height)*.5f));
    auto [tl,tr,br,bl]=radii;
    auto corner=[&](float cx,float cy,float sx,float sy,float ex,float ey){
        for(int i=0;i<=24;++i){
            double angle=i*3.14159265358979/48,along=std::sqrt(std::cos(angle)),across=std::sqrt(std::sin(angle));
            sink->AddLine({float(cx+sx*along+ex*across),float(cy+sy*along+ey*across)});
        }
    };
    sink->BeginFigure({tl,0},D2D1_FIGURE_BEGIN_FILLED);
    corner(width-tr,tr,0,-tr,tr,0);corner(width-br,height-br,br,0,0,br);corner(bl,height-bl,0,bl,-bl,0);corner(tl,tl,-tl,0,0,-tl);
    sink->EndFigure(D2D1_FIGURE_END_CLOSED);
    for(auto const& r:cut.extra)rectangle(r.X,r.Y,r.Width,r.Height);
    winrt::check_hresult(sink->Close());return path;
}
}
struct WorkspaceShadow::State {
    SpriteVisual visual{nullptr};
    DropShadow shadow{nullptr};
    float width=0,height=0,blur=8,offset=2,opacity=.16f;
    std::optional<ShadowCut> cut;
    void apply(){
        if(!visual)return;
        visual.Size({width,height});visual.Offset({padding,padding,0});
        if(cut&&width>0&&height>0){
            auto compositor=visual.Compositor();
            visual.Clip(compositor.CreateGeometricClip(compositor.CreatePathGeometry(CompositionPath(winrt::make<GeometrySource>(outside(width,height,*cut))))));
        }else visual.Clip(nullptr);
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
void WorkspaceShadow::Cut(std::array<float,4> radii,std::vector<Windows::Foundation::Rect> extra){
    state->cut=ShadowCut{radii,std::move(extra)};state->apply();
}
void WorkspaceShadow::Uncut(){state->cut.reset();state->apply();}
void WorkspaceShadow::Shape(Geometry const& geometry,float width,float height,float blur,float offset,float opacity){
    mask.Data(geometry);mask.Width(width);mask.Height(height);
    state->width=width;state->height=height;state->blur=blur;state->offset=offset;state->opacity=opacity;state->apply();
}
}
