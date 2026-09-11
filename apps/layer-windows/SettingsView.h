#pragma once
#include "pch.h"
#include <functional>
#include <memory>

class SettingsView {
public:
    using Json=winrt::Windows::Data::Json::JsonObject;
    using Dispatch=std::function<void(std::string)>;
    using Key=std::function<void(winrt::Microsoft::UI::Xaml::Input::KeyRoutedEventArgs const&,bool)>;
    SettingsView(Dispatch send,Json catalog,winrt::Microsoft::UI::Xaml::XamlRoot root,Key key,Dispatch report,std::function<void()> changed);
    ~SettingsView();
    void Apply(Json const& snapshot);
    bool IsOpen()const;
    void CommitEdits();
    void Hide();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
