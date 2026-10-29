#pragma once
#include "WorkspaceShadow.h"
#include <d2d1_1.h>
#include <d2d1effects.h>
#include <windows.graphics.effects.interop.h>
#include <winrt/Windows.Graphics.Effects.h>
#include <winrt/Microsoft.UI.Xaml.Hosting.h>

namespace CapyUi {
namespace tab_surface {
using namespace Microsoft::UI::Composition;
struct CompositeEffect : winrt::implements<CompositeEffect,Windows::Graphics::Effects::IGraphicsEffect,
    Windows::Graphics::Effects::IGraphicsEffectSource,ABI::Windows::Graphics::Effects::IGraphicsEffectD2D1Interop> {
    hstring name;
    D2D1_COMPOSITE_MODE mode;
    std::array<Windows::Graphics::Effects::IGraphicsEffectSource,2> sources;
    CompositeEffect(D2D1_COMPOSITE_MODE operation,Windows::Graphics::Effects::IGraphicsEffectSource const& first,
        Windows::Graphics::Effects::IGraphicsEffectSource const& second):mode(operation),sources{first,second}{}
    hstring Name()const{return name;}
    void Name(hstring const& value){name=value;}
    HRESULT __stdcall GetEffectId(GUID* value)noexcept final{*value=CLSID_D2D1Composite;return S_OK;}
    HRESULT __stdcall GetNamedPropertyMapping(LPCWSTR propertyName,UINT* index,ABI::Windows::Graphics::Effects::GRAPHICS_EFFECT_PROPERTY_MAPPING* mapping)noexcept final{
        if(wcscmp(propertyName,L"Mode"))return E_INVALIDARG;
        *index=D2D1_COMPOSITE_PROP_MODE;*mapping=ABI::Windows::Graphics::Effects::GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;return S_OK;
    }
    HRESULT __stdcall GetPropertyCount(UINT* value)noexcept final{*value=1;return S_OK;}
    HRESULT __stdcall GetProperty(UINT index,ABI::Windows::Foundation::IPropertyValue** value)noexcept final{
        if(index)return E_INVALIDARG;
        try{
            auto property=Windows::Foundation::PropertyValue::CreateUInt32(mode);
            *value=reinterpret_cast<ABI::Windows::Foundation::IPropertyValue*>(winrt::detach_abi(property));return S_OK;
        }catch(...){return winrt::to_hresult();}
    }
    HRESULT __stdcall GetSourceCount(UINT* value)noexcept final{*value=UINT(sources.size());return S_OK;}
    HRESULT __stdcall GetSource(UINT index,ABI::Windows::Graphics::Effects::IGraphicsEffectSource** value)noexcept final{
        if(index>=sources.size())return E_INVALIDARG;
        *value=reinterpret_cast<ABI::Windows::Graphics::Effects::IGraphicsEffectSource*>(winrt::get_abi(sources[index]));
        (*value)->AddRef();return S_OK;
    }
};
struct Surface {
    SpriteVisual visual{nullptr};
    CompositionPathGeometry path{nullptr};CompositionSpriteShape shape{nullptr};InsetClip tabClip{nullptr};
    std::array<CompositionColorBrush,2> inks{nullptr,nullptr};
    CompositionVisualSurface mask{nullptr};
    std::vector<Windows::Foundation::Point> band,tab;
    Windows::UI::Color strip{},panel{};
    float width=0,height=0,offset=0,clipLeft=0,clipRight=0;
    void create(Compositor const& compositor){
        using Windows::Graphics::Effects::IGraphicsEffectSource;
        CompositionEffectSourceParameter stripSource(L"Strip"),panelSource(L"Panel"),maskSource(L"Mask");
        auto background=winrt::make<CompositeEffect>(D2D1_COMPOSITE_MODE_DESTINATION_OUT,stripSource,maskSource);
        auto foreground=winrt::make<CompositeEffect>(D2D1_COMPOSITE_MODE_DESTINATION_IN,panelSource,maskSource);
        auto effect=winrt::make<CompositeEffect>(D2D1_COMPOSITE_MODE_PLUS,background.as<IGraphicsEffectSource>(),foreground.as<IGraphicsEffectSource>());
        auto brush=compositor.CreateEffectFactory(effect).CreateBrush();
        path=compositor.CreatePathGeometry();
        shape=compositor.CreateSpriteShape(path);shape.FillBrush(compositor.CreateColorBrush({255,255,255,255}));
        auto source=compositor.CreateShapeVisual();source.BorderMode(CompositionBorderMode::Soft);source.Shapes().Append(shape);tabClip=compositor.CreateInsetClip();source.Clip(tabClip);
        mask=compositor.CreateVisualSurface();mask.SourceVisual(source);
        brush.SetSourceParameter(L"Mask",compositor.CreateSurfaceBrush(mask));
        inks[0]=compositor.CreateColorBrush();inks[1]=compositor.CreateColorBrush();
        brush.SetSourceParameter(L"Strip",inks[0]);brush.SetSourceParameter(L"Panel",inks[1]);
        visual=compositor.CreateSpriteVisual();visual.BorderMode(CompositionBorderMode::Soft);visual.Brush(brush);apply();
    }
    bool translate(float value,bool animate){
        if(offset==value)return false;offset=value;if(!shape)return true;
        if(animate){
            auto compositor=shape.Compositor();auto move=compositor.CreateVector2KeyFrameAnimation();
            move.InsertExpressionKeyFrame(0,L"this.StartingValue");move.InsertKeyFrame(1,{value,0},compositor.CreateCubicBezierEasingFunction({.215f,.61f},{.355f,1.f}));
            move.Duration(std::chrono::milliseconds(120));shape.StartAnimation(L"Offset",move);
        }else{shape.StopAnimation(L"Offset");shape.Offset({value,0});}
        return true;
    }
    void apply(){
        if(!visual||band.empty()||width<=0)return;
        path.Path(CompositionPathFromContours({tab}));shape.StopAnimation(L"Offset");shape.Offset({offset,0});tabClip.LeftInset(clipLeft);tabClip.RightInset(clipRight);
        auto compositor=visual.Compositor();
        visual.Clip(compositor.CreateGeometricClip(compositor.CreatePathGeometry(CompositionPathFromContours({band}))));
        inks[0].Color(strip);inks[1].Color(panel);
        visual.Size({width,height});
        mask.SourceSize({width,height});mask.SourceVisual().Size({width,height});
    }
};
}
class WorkspaceTabSurface {
    Canvas root;
    std::shared_ptr<tab_surface::Surface> state=std::make_shared<tab_surface::Surface>();
public:
    WorkspaceTabSurface(){
        root.IsHitTestVisible(false);
        root.Loaded([state=state,weak=make_weak(root)](auto&&,auto&&){
            if(auto host=weak.get()){
                using Microsoft::UI::Xaml::Hosting::ElementCompositionPreview;
                state->create(ElementCompositionPreview::GetElementVisual(host).Compositor());
                ElementCompositionPreview::SetElementChildVisual(host,state->visual);
            }
        });
    }
    Canvas Root()const{return root;}
    bool TranslateTab(float offset,bool animate){return state->translate(offset,animate);}
    void Present(float width,float height,std::vector<Windows::Foundation::Point> band,std::vector<Windows::Foundation::Point> tab,Windows::UI::Color strip,Windows::UI::Color panel,float clipX=0,float clipWidth=-1){
        root.Width(width);root.Height(height);root.Visibility(band.empty()?Visibility::Collapsed:Visibility::Visible);
        state->clipLeft=std::max(0.f,clipX);state->clipRight=clipWidth<0?0:std::max(0.f,width-clipX-clipWidth);
        state->width=width;state->height=height;state->band=std::move(band);state->tab=std::move(tab);state->strip=strip;state->panel=panel;state->apply();
    }
};
}
