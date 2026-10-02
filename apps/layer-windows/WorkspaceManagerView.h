#pragma once
#include "pch.h"
#include <functional>
#include <memory>

struct CapyLocalization;

class WorkspaceManagerView {
public:
    using Json=winrt::Windows::Data::Json::JsonObject;
    using Dispatch=std::function<void(std::string)>;
    WorkspaceManagerView(Dispatch send,Json catalog,std::shared_ptr<CapyLocalization> localization,winrt::Microsoft::UI::Xaml::XamlRoot root,std::function<void()> changed);
    ~WorkspaceManagerView();
    void Apply(Json const& snapshot,bool blocked);
    bool IsOpen()const;
    void CancelAll();
    void Hide();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
