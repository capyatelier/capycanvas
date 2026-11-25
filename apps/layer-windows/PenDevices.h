#pragma once
#include <windows.h>
#include <vector>

inline bool AllPensExternal(std::vector<POINTER_DEVICE_INFO> const& devices){
    bool external=false;
    for(auto const& device:devices){
        if(device.pointerDeviceType==POINTER_DEVICE_TYPE_INTEGRATED_PEN)return false;
        external|=device.pointerDeviceType==POINTER_DEVICE_TYPE_EXTERNAL_PEN;
    }
    return external;
}
inline bool AttachedPensExternal(){
    UINT32 count=0;
    if(!GetPointerDevices(&count,nullptr)||!count)return false;
    std::vector<POINTER_DEVICE_INFO> devices(count);
    if(!GetPointerDevices(&count,devices.data()))return false;
    devices.resize(count);
    return AllPensExternal(devices);
}
