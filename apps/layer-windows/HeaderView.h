#pragma once
#include "pch.h"
#include <functional>
#include <memory>
#include <vector>

class HeaderView {
public:
    using Json=winrt::Windows::Data::Json::JsonObject;
    using Dispatch=std::function<void(std::string)>;
    HeaderView(Dispatch send,Json catalog,std::function<void(bool)> popup,
        std::function<void()> layout,std::function<void()> fullscreen);
    ~HeaderView();
    winrt::Microsoft::UI::Xaml::Controls::Grid Root()const;
    void Apply(Json const& snapshot);
    void SetInsets(float left,float right);
    void SetFullscreen(bool active);
    std::vector<winrt::Windows::Graphics::RectInt32> DragRegions(float scale,uint32_t width)const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
