#pragma once
#include "native/include/capy_windows.h"
#include <algorithm>
#include <cmath>

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
