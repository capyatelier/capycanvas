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
winrt::com_ptr<ID2D1Geometry> combine(ID2D1Geometry* first,ID2D1Geometry* second,D2D1_COMBINE_MODE mode){
    winrt::com_ptr<ID2D1PathGeometry> result;winrt::check_hresult(factory()->CreatePathGeometry(result.put()));
    winrt::com_ptr<ID2D1GeometrySink> sink;winrt::check_hresult(result->Open(sink.put()));
    winrt::check_hresult(first->CombineWithGeometry(second,mode,nullptr,.05f,sink.get()));winrt::check_hresult(sink->Close());return result;
}
winrt::com_ptr<ID2D1Geometry> commandGeometry(ContourCommands const& commands,std::vector<Windows::Foundation::Rect> const& holes={}){
    winrt::com_ptr<ID2D1Geometry> result;
    winrt::com_ptr<ID2D1PathGeometry> figure;
    winrt::com_ptr<ID2D1GeometrySink> sink;
    for(auto const& c:commands){
        if(std::any_of(c.begin(),c.end(),[](float v){return !std::isfinite(v);}))throw winrt::hresult_invalid_argument();
        if(c[0]==0){
            if(sink)throw winrt::hresult_invalid_argument();
            winrt::check_hresult(factory()->CreatePathGeometry(figure.put()));winrt::check_hresult(figure->Open(sink.put()));
            sink->SetFillMode(D2D1_FILL_MODE_WINDING);sink->BeginFigure({c[1],c[2]},D2D1_FIGURE_BEGIN_FILLED);
        }else if(!sink)throw winrt::hresult_invalid_argument();
        else if(c[0]==1)sink->AddLine({c[1],c[2]});
        else if(c[0]==2)sink->AddBezier({{c[1],c[2]},{c[3],c[4]},{c[5],c[6]}});
        else if(c[0]==3){
            sink->EndFigure(D2D1_FIGURE_END_CLOSED);winrt::check_hresult(sink->Close());sink=nullptr;
            result=result?combine(result.get(),figure.get(),D2D1_COMBINE_MODE_UNION):figure.as<ID2D1Geometry>();figure=nullptr;
        }else throw winrt::hresult_invalid_argument();
    }
    if(sink)throw winrt::hresult_invalid_argument();
    if(!result){winrt::com_ptr<ID2D1RectangleGeometry> empty;winrt::check_hresult(factory()->CreateRectangleGeometry({0,0,0,0},empty.put()));result=empty;}
    for(auto const& hole:holes)if(hole.Width>0&&hole.Height>0){
        winrt::com_ptr<ID2D1RectangleGeometry> rectangle;winrt::check_hresult(factory()->CreateRectangleGeometry({hole.X,hole.Y,hole.X+hole.Width,hole.Y+hole.Height},rectangle.put()));
        result=combine(result.get(),rectangle.get(),D2D1_COMBINE_MODE_EXCLUDE);
    }
    return result;
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
CompositionPath CompositionPathFromCommands(ContourCommands const& commands,std::vector<Windows::Foundation::Rect> const& holes){
    return CompositionPath(winrt::make<GeometrySource>(commandGeometry(commands,holes)));
}
Windows::Foundation::Rect ContourBounds(float width,float height,ContourCommands const& commands,float padding){
    float left=0,top=0,right=width,bottom=height;
    for(auto const& c:commands){
        size_t count=c[0]==2?3:c[0]==0||c[0]==1?1:0;
        for(size_t i=0;i<count;++i){left=std::min(left,c[1+2*i]);right=std::max(right,c[1+2*i]);top=std::min(top,c[2+2*i]);bottom=std::max(bottom,c[2+2*i]);}
    }
    return {left-padding,top-padding,right-left+2*padding,bottom-top+2*padding};
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
    bool rounded=false,contoured=false;
    ContourCommands commands,cachedCommands;
    ShapeVisual contourMask{nullptr};
    CompositionPathGeometry contourPath{nullptr};
    CompositionSpriteShape contourShape{nullptr};
    CompositionVisualSurface contourSurface{nullptr};
    CompositionSurfaceBrush contourAlpha{nullptr};
    CompositionGeometricClip contourClip{nullptr};
    float cachedWidth=-1,cachedHeight=-1,cachedBlur=-1,cachedOffset=0;
    float width=0,height=0,blur=8,offset=2,opacity=.16f;
    std::optional<ShadowCut> cut;
    void apply(){
        if(!visual||rounded)return;
        if(contoured){
            auto compositor=visual.Compositor();
            if(!contourPath||contourPath.Compositor()!=compositor){
                contourPath=compositor.CreatePathGeometry();contourShape=compositor.CreateSpriteShape(contourPath);contourShape.FillBrush(compositor.CreateColorBrush({255,255,255,255}));
                contourMask=compositor.CreateShapeVisual();contourMask.BorderMode(CompositionBorderMode::Soft);contourMask.Shapes().Append(contourShape);
                contourSurface=compositor.CreateVisualSurface();contourSurface.SourceVisual(contourMask);contourAlpha=compositor.CreateSurfaceBrush(contourSurface);contourAlpha.Stretch(CompositionStretch::Fill);cachedWidth=-1;
            }
            auto bounds=ContourBounds(width,height,commands);
            visual.Size({bounds.Width,bounds.Height});visual.Offset({padding+bounds.X,padding+bounds.Y,0});
            contourMask.Size(visual.Size());contourSurface.SourceSize(visual.Size());contourShape.Offset({-bounds.X,-bounds.Y});
            if(cachedCommands!=commands||cachedWidth!=width||cachedHeight!=height||cachedBlur!=blur||cachedOffset!=offset){
                auto interior=commandGeometry(commands);contourPath.Path(CompositionPath(winrt::make<GeometrySource>(interior)));
                float reach=std::max(padding,3*blur+std::abs(offset));winrt::com_ptr<ID2D1RectangleGeometry> outer;
                winrt::check_hresult(factory()->CreateRectangleGeometry({bounds.X-reach,bounds.Y-reach,bounds.X+bounds.Width+reach,bounds.Y+bounds.Height+reach},outer.put()));
                auto exterior=combine(outer.get(),interior.get(),D2D1_COMBINE_MODE_EXCLUDE);
                contourClip=compositor.CreateGeometricClip(compositor.CreatePathGeometry(CompositionPath(winrt::make<GeometrySource>(exterior))));
                contourClip.TransformMatrix({1,0,0,1,-bounds.X,-bounds.Y});
                cachedCommands=commands;cachedWidth=width;cachedHeight=height;cachedBlur=blur;cachedOffset=offset;
            }
            shadow.Mask(contourAlpha);visual.Clip(contourClip);
            shadow.BlurRadius(blur);shadow.Offset({0,offset,0});shadow.Opacity(opacity);return;
        }
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
    bool rounded=std::exchange(state->rounded,false);state->contoured=false;maskHost.Visibility(Visibility::Visible);
    if(state->shadow)state->shadow.Mask(mask.GetAlphaMask());
    mask.Data(geometry);mask.Width(width);mask.Height(height);
    state->width=width;state->height=height;state->blur=blur;state->offset=offset;state->opacity=opacity;state->apply();
    if(rounded&&state->visual)ElementCompositionPreview::SetElementChildVisual(root,state->visual);
}
void WorkspaceShadow::Contour(ContourCommands const& commands,float width,float height,float blur,float offset,float opacity){
    if(!std::isfinite(width)||!std::isfinite(height)||width<0||height<0||!std::isfinite(blur)||blur<0||!std::isfinite(offset)||!std::isfinite(opacity)||opacity<0||opacity>1)throw winrt::hresult_invalid_argument();
    bool rounded=std::exchange(state->rounded,false);state->contoured=true;maskHost.Visibility(Visibility::Collapsed);
    state->commands=commands;state->width=width;state->height=height;state->blur=blur;state->offset=offset;state->opacity=opacity;state->apply();
    if(rounded&&state->visual)ElementCompositionPreview::SetElementChildVisual(root,state->visual);
}
void WorkspaceShadow::Rounded(float width,float height,std::array<float,4> radii,float blur,float offset,float opacity){
    bool rounded=std::exchange(state->rounded,true);state->contoured=false;maskHost.Visibility(Visibility::Collapsed);
    state->retained.Present(width,height,radii,blur,offset,opacity);
    if(!state->retained.visual&&root.IsLoaded())state->retained.Create(ElementCompositionPreview::GetElementVisual(root).Compositor());
    if(!rounded&&state->retained.visual)ElementCompositionPreview::SetElementChildVisual(root,state->retained.visual);
}
}
