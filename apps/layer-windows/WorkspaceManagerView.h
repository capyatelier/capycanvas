#pragma once
#include "pch.h"
#include "CanvasQueryQueue.h"
#include <functional>
#include <memory>

#include "NativeHandles.h"

class WorkspaceManagerView {
public:
    using Json=winrt::Windows::Data::Json::JsonObject;
    using Dispatch=std::function<void(std::string,std::function<void()>)>;
    WorkspaceManagerView(Dispatch send,Json catalog,NativeLocalization localization,winrt::Microsoft::UI::Xaml::XamlRoot root,std::function<void()> changed,PreviewTransport query);
    ~WorkspaceManagerView();
    void Apply(Json const& snapshot,bool blocked);
    bool IsOpen()const;
    void CancelAll();
    void Hide();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
