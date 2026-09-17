#pragma once
#include "pch.h"
#include "CanvasQueryQueue.h"
#include <functional>
#include <memory>

class DocumentView {
public:
    using Json=winrt::Windows::Data::Json::JsonObject;
    using Dispatch=std::function<void(std::string)>;
    DocumentView(Dispatch send,Json catalog,winrt::Microsoft::UI::Xaml::Window window,
        std::function<void()> changed,PreviewTransport query,Dispatch report);
    ~DocumentView();
    void Apply(Json const& snapshot,bool blocked);
    bool IsOpen()const;
    void Hide();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
