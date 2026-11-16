#pragma once
#include "native/include/capy_windows.h"
#include <algorithm>
#include <cmath>

template<typename Device,typename Eraser,typename Inverted>
uint32_t CanvasPointerTool(Device type,Eraser eraser,Inverted inverted) {
    return type==Device::Mouse?1:type==Device::Touch?3:eraser()||inverted()?2:0;
}

inline uint32_t CanvasPointerPhase(uint32_t phase, bool canceled) {
    return canceled?4:phase;
}

template<typename Points>
uint32_t CanvasPointerHistoryCount(Points const& points,uint32_t phase) {
    return (phase==0||phase==2)&&points?points.Size():0;
}

template<typename Points,typename Point>
bool CanvasPointerHistoryNewestFirst(Points const& points,uint32_t count,Point const& current) {
    return count>1&&points.GetAt(0).FrameId()==current.FrameId()&&points.GetAt(count-1).FrameId()!=current.FrameId();
}

// Native pen contact/eraser identity owns drawing. Barrel button properties
// can also report right/middle pressed, but only a mouse uses those for pan.
inline uint32_t CanvasPointerButton(uint32_t tool, bool middle, bool right, bool extra=false) {
    return tool==1?(middle||right?1:extra?2:0):0;
}

// Windows' predictor extrapolates axes: pressure can exceed the sensor's [0,1]
// range. Optional predictions must not invalidate the real history sharing a
// batch. Real samples stay untouched and retain the Rust ingress validation.
inline bool PrepareCanvasPrediction(CapyPointer& p) {
    if(!(p.flags&1))return true;
    for(float value:{p.x,p.y,p.pressure,p.tilt_x,p.tilt_y,p.twist,p.distance})
        if(!std::isfinite(value))return false;
    p.pressure=std::clamp(p.pressure,0.0f,1.0f);
    p.distance=std::clamp(p.distance,0.0f,1.0f);
    return true;
}

inline CapyPointer CanvasReplaySample(CapyView const& view,uint32_t i,uint32_t count,bool pen,bool pan,bool backlog) {
    CapyPointer p{};
    p.view_revision=view.revision;p.tool=pen?0:1;p.button=pan?1:0;p.flags=2;
    p.pressure=pen?0.2f+0.7f*float(i)/float(count-1):1.0f;
    if(pen){p.tilt_x=0.4f*std::sin(float(i)/7);p.tilt_y=0.2f*std::cos(float(i)/7);p.twist=float(i)/42;}
    p.phase=i==0?1:i==count-1?3:2;
    float x=view.document[0]*(pan?0.5f:0.25f+0.5f*float(backlog?i%42:i)/41),y=view.document[1]*(pan?0.5f:0.5f+0.05f*std::sin(float(i)/6));
    auto const& m=view.document_to_surface;
    p.x=m[0]*x+m[2]*y+m[4];p.y=pan&&i?28.0f:m[1]*x+m[3]*y+m[5];
    return p;
}

inline bool CanvasEraserReplayPosition(float x,float y,float width,float height) {
    return std::isfinite(x)&&std::isfinite(y)&&x>=0&&y>=0&&x+72<width&&y<height;
}

inline CapyPointer CanvasEraserReplaySample(CapyView const& view,float x,float y,uint32_t i) {
    CapyPointer p{};p.view_revision=view.revision;p.tool=2;p.flags=2;p.pressure=0.5f;
    p.phase=i==0?0:i==1?1:i==20?3:2;
    p.x=x+4*float(i==0?0:std::min(i-1,18u));p.y=y;
    p.timestamp_ns=(i==0?0:i==1?35:i==20?217:35+(i-2)*10)*1000000ULL;
    return p;
}
