#pragma once
#include "pch.h"
#include "native/include/capy_windows.h"
#include <functional>
#include <memory>

class SizeDialog {
public:
    using Json=winrt::Windows::Data::Json::JsonObject;
    using Dispatch=std::function<void(std::string)>;
    enum class Kind{Canvas,Image};
    SizeDialog(Kind kind,Dispatch send,Json catalog,std::shared_ptr<CapyLocalization> localization,winrt::Microsoft::UI::Xaml::XamlRoot root,std::function<void()> changed);
    ~SizeDialog();
    void Apply(Json const& snapshot,bool blocked);
    bool IsOpen()const;
    void CancelAll();
    void Hide();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
