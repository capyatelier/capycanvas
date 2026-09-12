#pragma once
#include "NavigatorView.h"

class PanelConfiguration {
public:
    PanelConfiguration(std::shared_ptr<CapyUi::WorkspaceData> data,CapyUi::J const& panel,
        std::function<void()> measured);
    ~PanelConfiguration();
    winrt::Microsoft::UI::Xaml::FrameworkElement Root()const;
    winrt::hstring Panel()const;
    void Apply(CapyUi::J const& panel,bool visible);
    double ContentHeight()const;
    void SetVisible(bool visible);
    winrt::Microsoft::UI::Xaml::FrameworkElement Anchor(std::wstring const& control)const;
    void AppendOverviews(CapyUi::A& slots,winrt::Microsoft::UI::Xaml::UIElement const& reference,
        winrt::Windows::Foundation::Rect clip,int order)const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
