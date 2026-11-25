#pragma once
#include "UiControls.h"
#include <winrt/Windows.Devices.Input.h>
#include <winrt/Microsoft.UI.Input.Interop.h>

namespace CapyUi {
inline J penDevice(Microsoft::UI::Input::PointerPoint const& point) {
    if(point.PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Pen)return {};
    hstring key=L"windows:pen";
    try {
        if(auto pen=Microsoft::UI::Input::Interop::PenDeviceInterop::FromPointerPoint(point);pen&&pen.PenId()!=guid{})key=L"windows:pen:"+to_hstring(pen.PenId());
    } catch(hresult_error const&) {}
    return O({{L"id",N(point.PointerId())},{L"key",S(key)},{L"label",S(hstring{})},
        {L"native_to_surface",O({{L"reverse_horizontal",B(false)},{L"reverse_vertical",B(false)},{L"swap",B(false)}})}});
}
inline J penInput(J const& device,V const& sample=JsonValue::CreateNullValue()) {
    return O({{L"type",S(L"pen_input")},{L"device",device},{L"sample",sample}});
}
inline J penPreview(Microsoft::UI::Input::PointerPoint const& point,uint32_t phase) {
    auto device=penDevice(point);if(!device.Size())return {};
    auto properties=point.Properties();auto position=point.Position();
    auto radians=[](float degrees){return degrees*0.017453292519943295f;};
    A sample;
    for(double value:{double(position.X),double(position.Y),double(properties.Pressure()),double(radians(properties.XTilt())),double(radians(properties.YTilt())),
        double(radians(properties.Twist())),0.,double(point.Timestamp()*1000),double(properties.IsCanceled()?4:phase)})sample.Append(N(value));
    return penInput(device,sample);
}
}
