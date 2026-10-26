#pragma once
#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>

namespace CapyUi {
inline constexpr float WorkspaceShadowPadding=48;
struct ShadowRaster {
    std::array<float,2> panel{},extent{};
    std::array<float,4> radii{},insets{};
    float padding=0;
    bool operator==(ShadowRaster const&)const=default;
};
inline ShadowRaster shadowRaster(float width,float height,std::array<float,4> radii,float scale,float blur,float offset){
    ShadowRaster result;
    float limit=std::max(0.f,std::min(width,height)*.5f);
    for(std::size_t i=0;i<radii.size();++i)result.radii[i]=std::clamp(radii[i],0.f,limit)*scale;
    result.padding=WorkspaceShadowPadding*scale;
    float reach=std::max(WorkspaceShadowPadding,3*blur+std::abs(offset))*scale;
    float diameter=2*(*std::max_element(result.radii.begin(),result.radii.end()));
    auto axis=[&](float size,float leading,float trailing,std::size_t index,std::size_t first,std::size_t last){
        leading+=reach;trailing+=reach;
        float canonical=std::max(leading+trailing+1.f,diameter);
        float source=std::min(size*scale,canonical);
        result.panel[index]=source;result.extent[index]=source+2*result.padding;
        if(source<canonical){
            result.insets[first]=result.insets[last]=result.extent[index]*.5f;
        }else{
            result.insets[first]=result.padding+leading;result.insets[last]=result.padding+trailing;
        }
    };
    auto [tl,tr,br,bl]=result.radii;
    axis(width,std::max(tl,bl),std::max(tr,br),0,0,2);
    axis(height,std::max(tl,tr),std::max(bl,br),1,1,3);
    return result;
}
}
