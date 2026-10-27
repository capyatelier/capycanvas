#pragma once
#include "WorkspaceGeometry.h"
#include "WorkspaceShadow.h"
#include "WorkspaceSurfaceGeometry.h"
#include <winrt/Microsoft.UI.Xaml.Hosting.h>
#include <optional>

namespace CapyUi {
class RetainedShadow {
    using Compositor=Microsoft::UI::Composition::Compositor;
    Compositor compositor{nullptr};
    struct Patch {
        Microsoft::UI::Composition::SpriteVisual visual{nullptr};
        Microsoft::UI::Composition::CompositionVisualSurface surface{nullptr};
    };
    std::array<Patch,9> patches;
    struct Style {
        ShadowRaster raster;
        float blur=0,offset=0,opacity=1,scale=1;
        bool operator==(Style const&)const=default;
    };
    std::optional<Style> cached;
    float width=0,height=0,scale=1,blur=0,offset=0,opacity=1;
    std::array<float,4> radii{};
    bool presented=false;
    void layout(ShadowRaster const& raster){
        float padding=raster.padding/scale;
        float sourceWidth=raster.extent[0]/scale,sourceHeight=raster.extent[1]/scale;
        float targetWidth=width+2*padding,targetHeight=height+2*padding;
        float left=raster.insets[0]/scale,top=raster.insets[1]/scale,right=raster.insets[2]/scale,bottom=raster.insets[3]/scale;
        std::array<float,4> sourceX{0,left,sourceWidth-right,sourceWidth},sourceY{0,top,sourceHeight-bottom,sourceHeight};
        std::array<float,4> targetX{0,left,targetWidth-right,targetWidth},targetY{0,top,targetHeight-bottom,targetHeight};
        for(size_t row=0;row<3;++row)for(size_t column=0;column<3;++column){
            auto& patch=patches[row*3+column];
            float w=targetX[column+1]-targetX[column],h=targetY[row+1]-targetY[row];
            patch.visual.IsVisible(w>0&&h>0&&sourceX[column+1]>sourceX[column]&&sourceY[row+1]>sourceY[row]);
            patch.visual.Offset({targetX[column],targetY[row],0});patch.visual.Size({std::max(0.f,w),std::max(0.f,h)});
        }
    }
    void render(){
        if(!visual||!presented)return;
        bool visible=width>0&&height>0;
        visual.IsVisible(visible);
        visual.Size({visible?width+2*WorkspaceShadowPadding:0,visible?height+2*WorkspaceShadowPadding:0});
        if(width<=0||height<=0)return;
        Style next{shadowRaster(width,height,radii,scale,blur,offset),blur,offset,opacity,scale};
        layout(next.raster);
        if(cached&&*cached==next)return;
        auto const& raster=next.raster;
        auto sourceRadii=raster.radii;for(auto& radius:sourceRadii)radius/=scale;
        float sourceWidth=raster.panel[0]/scale,sourceHeight=raster.panel[1]/scale;
        auto outline=squircleOutline(sourceWidth,sourceHeight,sourceRadii);
        auto geometry=compositor.CreatePathGeometry(CompositionPathFromContours({outline}));
        auto shape=compositor.CreateSpriteShape(geometry);
        shape.FillBrush(compositor.CreateColorBrush({255,255,255,255}));
        auto maskVisual=compositor.CreateShapeVisual();maskVisual.BorderMode(Microsoft::UI::Composition::CompositionBorderMode::Soft);
        maskVisual.Size({sourceWidth,sourceHeight});maskVisual.Shapes().Append(shape);
        auto mask=compositor.CreateVisualSurface();mask.SourceVisual(maskVisual);mask.SourceSize(maskVisual.Size());
        auto alpha=compositor.CreateSurfaceBrush(mask);alpha.Stretch(Microsoft::UI::Composition::CompositionStretch::Fill);
        auto drop=compositor.CreateDropShadow();drop.Color({255,0,0,0});drop.BlurRadius(blur);drop.Offset({0,offset,0});drop.Opacity(opacity);drop.Mask(alpha);
        auto cast=compositor.CreateSpriteVisual();cast.Size(maskVisual.Size());cast.Offset({raster.padding/scale,raster.padding/scale,0});cast.Shadow(drop);
        float p=raster.padding/scale;
        std::vector<Windows::Foundation::Point> outer{{-p,-p},{sourceWidth+p,-p},{sourceWidth+p,sourceHeight+p},{-p,sourceHeight+p}};
        cast.Clip(compositor.CreateGeometricClip(compositor.CreatePathGeometry(CompositionPathFromContours({outer,outline}))));
        auto sourceTree=compositor.CreateContainerVisual();sourceTree.Size({raster.extent[0]/scale,raster.extent[1]/scale});sourceTree.Children().InsertAtTop(cast);
        float sourceWidthPadded=raster.extent[0]/scale,sourceHeightPadded=raster.extent[1]/scale;
        std::array<float,4> x{0,raster.insets[0]/scale,sourceWidthPadded-raster.insets[2]/scale,sourceWidthPadded};
        std::array<float,4> y{0,raster.insets[1]/scale,sourceHeightPadded-raster.insets[3]/scale,sourceHeightPadded};
        for(size_t row=0;row<3;++row)for(size_t column=0;column<3;++column){
            auto& patch=patches[row*3+column];float w=x[column+1]-x[column],h=y[row+1]-y[row];
            if(w<=0||h<=0){patch.surface.SourceVisual(nullptr);continue;}
            patch.surface.SourceVisual(sourceTree);patch.surface.SourceOffset({x[column],y[row]});patch.surface.SourceSize({w,h});
        }
        cached=next;
    }
public:
    Microsoft::UI::Composition::SpriteVisual visual{nullptr};
    void Create(Compositor const& value){
        if(compositor==value&&visual)return;
        compositor=value;visual=compositor.CreateSpriteVisual();visual.BorderMode(Microsoft::UI::Composition::CompositionBorderMode::Soft);
        for(auto& patch:patches){
            patch.surface=compositor.CreateVisualSurface();
            auto brush=compositor.CreateSurfaceBrush(patch.surface);brush.Stretch(Microsoft::UI::Composition::CompositionStretch::Fill);
            patch.visual=compositor.CreateSpriteVisual();patch.visual.BorderMode(Microsoft::UI::Composition::CompositionBorderMode::Hard);patch.visual.Brush(brush);
            visual.Children().InsertAtTop(patch.visual);
        }
        cached.reset();render();
    }
    void RasterizationScale(float value){
        if(!std::isfinite(value)||value<=0)throw winrt::hresult_invalid_argument();
        if(value==scale)return;scale=value;cached.reset();render();
    }
    void Present(float w,float h,std::array<float,4> corners,float blurRadius,float shadowOffset,float alpha){
        if(!std::isfinite(w)||!std::isfinite(h)||w<0||h<0||!std::isfinite(blurRadius)||blurRadius<0||!std::isfinite(shadowOffset)||!std::isfinite(alpha)||alpha<0||alpha>1
            ||std::any_of(corners.begin(),corners.end(),[](float radius){return !std::isfinite(radius)||radius<0;}))throw winrt::hresult_invalid_argument();
        width=w;height=h;radii=corners;blur=blurRadius;offset=shadowOffset;opacity=alpha;presented=true;render();
    }
};

class WorkspaceSurface {
    struct State:std::enable_shared_from_this<State>{
        Microsoft::UI::Composition::ShapeVisual visual{nullptr};
        Microsoft::UI::Composition::CompositionPathGeometry path{nullptr};
        Microsoft::UI::Composition::CompositionSpriteShape shape{nullptr};
        Microsoft::UI::Composition::CompositionColorBrush ink{nullptr};
        winrt::weak_ref<Canvas> host;
        XamlRoot::Changed_revoker changed;
        Microsoft::UI::Xaml::Shapes::Path irregular{nullptr};
        float width=0,height=0,pathWidth=-1,pathHeight=-1,rasterizationScale=1,renderScale=0;
        std::array<float,4> radii{},pathRadii{};
        Windows::UI::Color color{};
        std::vector<Windows::Foundation::Rect> holes,pathHoles;
        ContourCommands commands,pathCommands;
        bool rounded=true,contour=false;
        bool sameHoles()const{
            if(pathHoles.size()!=holes.size())return false;
            for(size_t i=0;i<holes.size();++i){auto a=holes[i],b=pathHoles[i];if(a.X!=b.X||a.Y!=b.Y||a.Width!=b.Width||a.Height!=b.Height)return false;}
            return true;
        }
        void apply(){
            if(!visual||!rounded)return;
            auto current=ink.Color();if(current.A!=color.A||current.R!=color.R||current.G!=color.G||current.B!=color.B)ink.Color(color);
            if(contour){
                using namespace Microsoft::UI::Composition;
                auto bounds=ContourBounds(width,height,commands,1/rasterizationScale);
                visual.IsVisible(!commands.empty());visual.Offset({bounds.X,bounds.Y,0});visual.Size({bounds.Width,bounds.Height});
                shape.Offset({-bounds.X,-bounds.Y});
                if(pathCommands!=commands||!sameHoles()||pathWidth<0){path.Path(CompositionPathFromCommands(commands,holes));pathCommands=commands;pathHoles=holes;pathWidth=width;}
                shape.FillBrush(ink);renderScale=0;return;
            }
            shape.FillBrush(ink);
            visual.IsVisible(width>0&&height>0);
            if(renderScale!=rasterizationScale){
                float padding=1/rasterizationScale;visual.Offset({-padding,-padding,0});shape.Offset({padding,padding});renderScale=rasterizationScale;
            }
            float w=(std::ceil(width*rasterizationScale)+2)/rasterizationScale,h=(std::ceil(height*rasterizationScale)+2)/rasterizationScale;
            auto size=visual.Size();if(size.x!=w||size.y!=h)visual.Size({w,h});
            if(width!=pathWidth||height!=pathHeight||radii!=pathRadii||!sameHoles()){
                std::vector<std::vector<Windows::Foundation::Point>> contours{squircleOutline(width,height,radii)};
                for(auto hole:holes)if(hole.Width>0&&hole.Height>0)contours.push_back({{hole.X,hole.Y},{hole.X+hole.Width,hole.Y},{hole.X+hole.Width,hole.Y+hole.Height},{hole.X,hole.Y+hole.Height}});
                path.Path(CompositionPathFromContours(contours));pathWidth=width;pathHeight=height;pathRadii=radii;pathHoles=holes;
            }
        }
        void scale(){
            if(auto hostCanvas=host.get())if(auto xaml=hostCanvas.XamlRoot()){
                float value=float(xaml.RasterizationScale());if(value==rasterizationScale)return;rasterizationScale=value;apply();
            }
        }
        void attach(){
            auto hostCanvas=host.get();if(!hostCanvas||!hostCanvas.IsLoaded())return;
            using Microsoft::UI::Xaml::Hosting::ElementCompositionPreview;
            scale();if(!rounded)return;
            auto compositor=ElementCompositionPreview::GetElementVisual(hostCanvas).Compositor();
            if(!visual||visual.Compositor()!=compositor){
                visual=compositor.CreateShapeVisual();visual.BorderMode(Microsoft::UI::Composition::CompositionBorderMode::Soft);
                path=compositor.CreatePathGeometry();ink=compositor.CreateColorBrush(color);
                shape=compositor.CreateSpriteShape(path);shape.FillBrush(ink);visual.Shapes().Append(shape);pathWidth=-1;renderScale=0;
            }
            apply();
            if(ElementCompositionPreview::GetElementChildVisual(hostCanvas)!=visual)ElementCompositionPreview::SetElementChildVisual(hostCanvas,visual);
        }
        void loaded(){
            changed.revoke();auto hostCanvas=host.get();if(!hostCanvas)return;
            if(auto xaml=hostCanvas.XamlRoot())changed=xaml.Changed(auto_revoke,[weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->scale();});
            attach();
        }
    };
    Canvas root;
    std::shared_ptr<State> state=std::make_shared<State>();
public:
    WorkspaceSurface(){
        root.IsHitTestVisible(false);state->host=make_weak(root);auto weak=std::weak_ptr<State>(state);
        root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->loaded();});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->changed.revoke();});
    }
    Canvas Root()const{return root;}
    void Rounded(float width,float height,std::array<float,4> radii,Windows::UI::Color color,std::vector<Windows::Foundation::Rect> holes={}){
        if(!std::isfinite(width)||!std::isfinite(height)||width<0||height<0
            ||std::any_of(radii.begin(),radii.end(),[](float radius){return !std::isfinite(radius)||radius<0;}))throw winrt::hresult_invalid_argument();
        if(state->irregular){root.Children().Clear();state->irregular=nullptr;root.ClearValue(FrameworkElement::WidthProperty());root.ClearValue(FrameworkElement::HeightProperty());}
        if(state->contour)state->pathWidth=-1;state->contour=false;
        state->width=width;state->height=height;state->radii=radii;state->color=color;state->holes=std::move(holes);state->rounded=true;state->attach();
    }
    void Contour(float width,float height,ContourCommands commands,Windows::UI::Color color,std::vector<Windows::Foundation::Rect> holes={}){
        if(!std::isfinite(width)||!std::isfinite(height)||width<0||height<0)throw winrt::hresult_invalid_argument();
        if(state->irregular){root.Children().Clear();state->irregular=nullptr;root.ClearValue(FrameworkElement::WidthProperty());root.ClearValue(FrameworkElement::HeightProperty());}
        if(!state->contour)state->pathWidth=-1;state->contour=true;state->rounded=true;state->width=width;state->height=height;state->commands=std::move(commands);state->holes=std::move(holes);state->color=color;state->attach();
    }
    void Shape(Geometry const& geometry,Windows::UI::Color color){
        using Microsoft::UI::Xaml::Hosting::ElementCompositionPreview;
        if(state->rounded){if(state->visual)ElementCompositionPreview::SetElementChildVisual(root,nullptr);state->rounded=false;}
        if(!state->irregular){state->irregular=Microsoft::UI::Xaml::Shapes::Path();state->irregular.IsHitTestVisible(false);root.Children().Append(state->irregular);}
        auto bounds=geometry.Bounds();float width=std::max(0.f,bounds.X+bounds.Width),height=std::max(0.f,bounds.Y+bounds.Height);
        if(root.Width()!=width)root.Width(width);if(root.Height()!=height)root.Height(height);
        state->irregular.Data(geometry);state->irregular.Fill(fill(color));
    }
};
}
