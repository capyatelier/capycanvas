#include "pch.h"
#include "OverviewOcclusion.h"
#include "WorkspaceGeometry.h"
#include <d2d1.h>
#include <windows.graphics.interop.h>
#include <winrt/Windows.Graphics.h>
#include <winrt/Microsoft.UI.Composition.h>
#include <winrt/Microsoft.UI.Xaml.Hosting.h>
#include <set>

using namespace CapyUi;
using namespace Microsoft::UI::Composition;
using Microsoft::UI::Xaml::Hosting::ElementCompositionPreview;
namespace {
struct ClipSource:implements<ClipSource,Windows::Graphics::IGeometrySource2D,ABI::Windows::Graphics::IGeometrySource2DInterop>{
    com_ptr<ID2D1Geometry> geometry;
    explicit ClipSource(com_ptr<ID2D1Geometry> value):geometry(std::move(value)){}
    HRESULT __stdcall GetGeometry(ID2D1Geometry** result)noexcept override{
        if(!result)return E_POINTER;geometry.copy_to(result);return S_OK;
    }
    HRESULT __stdcall TryGetGeometryUsingFactory(ID2D1Factory*,ID2D1Geometry** result)noexcept override{
        if(!result)return E_POINTER;*result=nullptr;return E_NOTIMPL;
    }
};
Rect rect(A const& values){
    if(values.Size()!=4)return {};
    return {float(values.GetNumberAt(0)),float(values.GetNumberAt(1)),float(values.GetNumberAt(2)),float(values.GetNumberAt(3))};
}
std::vector<Rect> subtract(std::vector<Rect> const& regions,Rect hole){
    std::vector<Rect> result;
    auto append=[&](Rect r){if(r.Width>0&&r.Height>0)result.push_back(r);};
    for(auto r:regions){
        auto overlap=intersect(r,hole);
        if(overlap.Width<=0||overlap.Height<=0){result.push_back(r);continue;}
        append({r.X,r.Y,r.Width,overlap.Y-r.Y});
        append({r.X,overlap.Y+overlap.Height,r.Width,r.Y+r.Height-overlap.Y-overlap.Height});
        append({r.X,overlap.Y,overlap.X-r.X,overlap.Height});
        append({overlap.X+overlap.Width,overlap.Y,r.X+r.Width-overlap.X-overlap.Width,overlap.Height});
    }
    return result;
}
}
struct OverviewOcclusion::Impl{
    struct Entry{
        weak_ref<FrameworkElement> element;
        CompositionClip original{nullptr},current{nullptr};
        hstring key;
    };
    std::map<uintptr_t,Entry> entries;
    com_ptr<ID2D1Factory> factory;
    static void restore(Entry const& entry){
        if(auto element=entry.element.get()){
            auto visual=ElementCompositionPreview::GetElementVisual(element);
            if(visual.Clip()==entry.current)visual.Clip(entry.original);
        }
    }
    ~Impl(){for(auto const& [id,entry]:entries)restore(entry);}
    void apply(Canvas const& root,A const& slots,J const& document){
        struct Hole{Rect bounds;int order;};std::vector<Hole> holes;
        for(auto value:slots){
            auto slot=value.GetObject();auto bounds=rect(array(slot,L"bounds"));float image[4]{};
            if(!capy_navigator_image(bounds.Width,bounds.Height,uint32_t(num(document,L"width")),uint32_t(num(document,L"height")),image))continue;
            auto hole=intersect({bounds.X+image[0],bounds.Y+image[1],image[2],image[3]},rect(array(slot,L"clip")));
            if(hole.Width>0&&hole.Height>0)holes.push_back({hole,int(num(slot,L"order"))});
        }
        std::set<uintptr_t> keep;
        for(auto child:root.Children()){
            auto element=child.try_as<FrameworkElement>();
            if(!element||!element.IsLoaded()||element.Visibility()!=Visibility::Visible)continue;
            Rect outer{0,0,float(element.ActualWidth()),float(element.ActualHeight())};
            if(outer.Width<=0||outer.Height<=0)continue;
            int order=visualOrder(root,element);A signature;std::vector<Rect> regions{outer};
            for(auto hole:holes)if(hole.order>order){
                auto local=intersect(outer,root.TransformToVisual(element).TransformBounds(hole.bounds));
                if(local.Width<=0||local.Height<=0)continue;
                signature.Append(rectangle(local));regions=subtract(regions,local);
            }
            if(!signature.Size())continue;
            auto id=reinterpret_cast<uintptr_t>(get_abi(element));keep.insert(id);
            auto [found,added]=entries.try_emplace(id);auto& entry=found->second;
            auto visual=ElementCompositionPreview::GetElementVisual(element);
            if(added){entry.element=make_weak(element);entry.original=visual.Clip();}
            signature.Append(rectangle(outer));auto key=signature.Stringify();
            if(key!=entry.key){
                if(!factory)check_hresult(D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED,factory.put()));
                com_ptr<ID2D1PathGeometry> path;check_hresult(factory->CreatePathGeometry(path.put()));
                com_ptr<ID2D1GeometrySink> sink;check_hresult(path->Open(sink.put()));sink->SetFillMode(D2D1_FILL_MODE_WINDING);
                // Disjoint rectangles subtract the union, including overlapping previews.
                for(auto r:regions){
                    sink->BeginFigure({r.X,r.Y},D2D1_FIGURE_BEGIN_FILLED);
                    sink->AddLine({r.X+r.Width,r.Y});sink->AddLine({r.X+r.Width,r.Y+r.Height});sink->AddLine({r.X,r.Y+r.Height});
                    sink->EndFigure(D2D1_FIGURE_END_CLOSED);
                }
                check_hresult(sink->Close());
                auto compositor=visual.Compositor();
                auto source=make<ClipSource>(path.as<ID2D1Geometry>());
                entry.current=compositor.CreateGeometricClip(compositor.CreatePathGeometry(CompositionPath(source)));
                entry.key=key;
            }
            if(visual.Clip()!=entry.current)visual.Clip(entry.current);
        }
        for(auto it=entries.begin();it!=entries.end();){
            if(!keep.contains(it->first)){restore(it->second);it=entries.erase(it);}else ++it;
        }
    }
};
OverviewOcclusion::OverviewOcclusion():impl(std::make_unique<Impl>()){}
OverviewOcclusion::~OverviewOcclusion()=default;
void OverviewOcclusion::Apply(Canvas const& root,A const& slots,J const& document){impl->apply(root,slots,document);}
