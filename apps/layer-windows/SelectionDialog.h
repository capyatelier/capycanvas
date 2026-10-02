#pragma once
#include "pch.h"
#include "native/include/capy_windows.h"
#include <functional>
#include <memory>

class SelectionDialog {
public:
    using Json=winrt::Windows::Data::Json::JsonObject;
    using Dispatch=std::function<void(std::string)>;
    SelectionDialog(Dispatch send,Json catalog,std::shared_ptr<CapyLocalization> localization,winrt::Microsoft::UI::Xaml::XamlRoot root,std::function<void()> changed);
    ~SelectionDialog();
    void Apply(Json const& snapshot,bool blocked);
    bool IsOpen()const;
    void CancelAll();
    void Hide();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
