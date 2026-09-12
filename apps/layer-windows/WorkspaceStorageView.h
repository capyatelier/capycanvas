#pragma once
#include "pch.h"
#include <functional>
#include <memory>

class WorkspaceStorageView {
public:
    using Json=winrt::Windows::Data::Json::JsonObject;
    using Dispatch=std::function<void(std::string)>;
    WorkspaceStorageView(Dispatch send,winrt::Microsoft::UI::Xaml::Window window,std::function<void()> changed);
    ~WorkspaceStorageView();
    winrt::Microsoft::UI::Xaml::FrameworkElement Root()const;
    void Apply(Json const& snapshot,bool blocked);
    bool IsOpen()const;
    void Hide();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
