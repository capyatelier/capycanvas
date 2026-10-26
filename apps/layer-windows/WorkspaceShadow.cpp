#include "pch.h"
#include "WorkspaceShadow.h"
#include "WorkspaceSurface.h"
#include <winrt/Microsoft.UI.Xaml.Hosting.h>
#include <winrt/Windows.Graphics.h>
#include <d2d1.h>
#include <windows.graphics.interop.h>
#include <optional>

namespace CapyUi {
using namespace Microsoft::UI::Composition;
using Microsoft::UI::Xaml::Hosting::ElementCompositionPreview;
namespace {
constexpr float padding=WorkspaceShadowPadding;
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
struct ShadowCut{std::array<float,4> radii{};std::vector<winrt::Windows::Foundation::Rect> extra;bool squircle=true;std::vector<winrt::Windows::Foundation::Point> contour;};
winrt::com_ptr<ID2D1Geometry> outside(float width,float height,ShadowCut const& cut,bool exclude=true){
    winrt::com_ptr<ID2D1PathGeometry> path;winrt::check_hresult(factory()->CreatePathGeometry(path.put()));
    winrt::com_ptr<ID2D1GeometrySink> sink;winrt::check_hresult(path->Open(sink.put()));sink->SetFillMode(D2D1_FILL_MODE_ALTERNATE);
    auto rectangle=[&](float x,float y,float w,float h){
        sink->BeginFigure({x,y},D2D1_FIGURE_BEGIN_FILLED);sink->AddLine({x+w,y});sink->AddLine({x+w,y+h});sink->AddLine({x,y+h});sink->EndFigure(D2D1_FIGURE_END_CLOSED);
    };
    if(exclude)rectangle(-padding,-padding,width+2*padding,height+2*padding);
    if(!cut.contour.empty()){
        sink->BeginFigure({cut.contour.front().X,cut.contour.front().Y},D2D1_FIGURE_BEGIN_FILLED);
        for(auto const& point:cut.contour)sink->AddLine({point.X,point.Y});
        sink->EndFigure(D2D1_FIGURE_END_CLOSED);
        winrt::check_hresult(sink->Close());return path;
    }
    auto radii=cut.radii;for(auto& radius:radii)radius=std::min(radius,std::max(0.f,std::min(width,height)*.5f));
    auto [tl,tr,br,bl]=radii;
    auto corner=[&](float cx,float cy,float sx,float sy,float ex,float ey){
        for(int i=0;i<=24;++i){
            double angle=i*3.14159265358979/48,along=std::pow(std::cos(angle),cut.squircle?.5:1.),across=std::pow(std::sin(angle),cut.squircle?.5:1.);
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
CompositionPath CompositionPathFromContours(std::vector<std::vector<Windows::Foundation::Point>> const& contours){
    winrt::com_ptr<ID2D1PathGeometry> path;winrt::check_hresult(factory()->CreatePathGeometry(path.put()));
    winrt::com_ptr<ID2D1GeometrySink> sink;winrt::check_hresult(path->Open(sink.put()));sink->SetFillMode(D2D1_FILL_MODE_ALTERNATE);
    for(auto const& contour:contours)if(!contour.empty()){
        sink->BeginFigure({contour.front().X,contour.front().Y},D2D1_FIGURE_BEGIN_FILLED);
        for(auto const& point:contour)sink->AddLine({point.X,point.Y});
        sink->EndFigure(D2D1_FIGURE_END_CLOSED);
    }
    winrt::check_hresult(sink->Close());
    return CompositionPath(winrt::make<GeometrySource>(path));
}
void ClipOutside(UIElement const& element,float width,float height,std::array<float,4> radii,bool squircle){
    auto visual=ElementCompositionPreview::GetElementVisual(element);auto compositor=visual.Compositor();
    visual.Clip(compositor.CreateGeometricClip(compositor.CreatePathGeometry(CompositionPath(winrt::make<GeometrySource>(outside(width,height,{radii,{},squircle}))))));
}
void ClipSurface(UIElement const& element,float width,float height,std::array<float,4> radii,bool squircle){
    auto visual=ElementCompositionPreview::GetElementVisual(element);auto compositor=visual.Compositor();
    visual.Clip(compositor.CreateGeometricClip(compositor.CreatePathGeometry(CompositionPath(winrt::make<GeometrySource>(outside(width,height,{radii,{},squircle},false))))));
}
struct WorkspaceShadow::State {
    SpriteVisual visual{nullptr};
    DropShadow shadow{nullptr};
    RetainedShadow retained;
    XamlRoot::Changed_revoker scaleChanged;
    bool rounded=false;
    float width=0,height=0,blur=8,offset=2,opacity=.16f;
    std::optional<ShadowCut> cut;
    void apply(){
        if(!visual||rounded)return;
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
    mask.Fill(fill({255,255,255,255}));ElementCompositionPreview::GetElementVisual(maskHost).Opacity(0);maskHost.Children().Append(mask);
    Canvas::SetLeft(maskHost,padding);Canvas::SetTop(maskHost,padding);root.Children().Append(maskHost);
    root.Loaded([state=state,host=make_weak(root),shape=make_weak(mask)](auto&&,auto&&){
        auto root=host.get();auto mask=shape.get();if(!root||!mask)return;
        auto compositor=ElementCompositionPreview::GetElementVisual(root).Compositor();
        state->scaleChanged.revoke();
        if(auto xaml=root.XamlRoot()){
            state->retained.RasterizationScale(float(xaml.RasterizationScale()));
            state->scaleChanged=xaml.Changed(auto_revoke,[weak=std::weak_ptr<State>(state)](auto&& sender,auto&&){
                if(auto value=weak.lock())value->retained.RasterizationScale(float(sender.RasterizationScale()));
            });
        }
        if(state->rounded)state->retained.Create(compositor);
        state->shadow=compositor.CreateDropShadow();state->shadow.Color({255,0,0,0});
        state->shadow.Mask(mask.GetAlphaMask());
        state->visual=compositor.CreateSpriteVisual();state->visual.Shadow(state->shadow);
        state->apply();ElementCompositionPreview::SetElementChildVisual(root,state->rounded?state->retained.visual:state->visual);
    });
    root.Unloaded([weak=std::weak_ptr<State>(state)](auto&&,auto&&){if(auto value=weak.lock())value->scaleChanged.revoke();});
}
void WorkspaceShadow::Layout(Windows::Foundation::Rect bounds,int order,bool visible){
    root.Width(bounds.Width+2*padding);root.Height(bounds.Height+2*padding);
    Canvas::SetLeft(root,bounds.X-padding);Canvas::SetTop(root,bounds.Y-padding);
    Canvas::SetZIndex(root,order);root.Visibility(visible?Visibility::Visible:Visibility::Collapsed);
}
void WorkspaceShadow::Cut(std::array<float,4> radii,std::vector<Windows::Foundation::Rect> extra){
    state->cut=ShadowCut{radii,std::move(extra)};state->apply();
}
void WorkspaceShadow::Cut(std::vector<Windows::Foundation::Point> contour){
    state->cut=ShadowCut{{},{},false,std::move(contour)};state->apply();
}
void WorkspaceShadow::Uncut(){state->cut.reset();state->apply();}
void WorkspaceShadow::Shape(Geometry const& geometry,float width,float height,float blur,float offset,float opacity){
    bool rounded=std::exchange(state->rounded,false);maskHost.Visibility(Visibility::Visible);
    mask.Data(geometry);mask.Width(width);mask.Height(height);
    state->width=width;state->height=height;state->blur=blur;state->offset=offset;state->opacity=opacity;state->apply();
    if(rounded&&state->visual)ElementCompositionPreview::SetElementChildVisual(root,state->visual);
}
void WorkspaceShadow::Rounded(float width,float height,std::array<float,4> radii,float blur,float offset,float opacity){
    bool rounded=std::exchange(state->rounded,true);maskHost.Visibility(Visibility::Collapsed);
    state->retained.Present(width,height,radii,blur,offset,opacity);
    if(!state->retained.visual&&root.IsLoaded())state->retained.Create(ElementCompositionPreview::GetElementVisual(root).Compositor());
    if(!rounded&&state->retained.visual)ElementCompositionPreview::SetElementChildVisual(root,state->retained.visual);
}
}
