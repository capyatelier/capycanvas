#pragma once
#include "pch.h"
#include "CanvasQueryQueue.h"
#include <functional>
#include <memory>
#include <vector>

class HeaderView {
public:
    using Json=winrt::Windows::Data::Json::JsonObject;
    using Dispatch=std::function<void(std::string)>;
    HeaderView(Dispatch send,Json catalog,std::function<void(bool)> popup,
        std::function<void()> layout,std::function<void()> fullscreen,std::function<void()> newWindow,PreviewTransport queries,Dispatch input,Dispatch documents);
    ~HeaderView();
    winrt::Microsoft::UI::Xaml::Controls::Grid Root()const;
    void Apply(Json const& snapshot);
    void SetInsets(float left,float right);
    void SetFullscreen(bool active);
    void SetBlocked(bool blocked);
    bool Key(winrt::Microsoft::UI::Xaml::Input::KeyRoutedEventArgs const& event,bool pressed);
    std::vector<winrt::Windows::Graphics::RectInt32> DragRegions(float scale,uint32_t width)const;
    std::vector<winrt::Windows::Graphics::RectInt32> InputRegions(float scale,uint32_t width)const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
