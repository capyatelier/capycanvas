#pragma once
#include "UiControls.h"

// Native controls reveal a GPU overview in the window's existing canvas surface.
class NavigatorView {
public:
    NavigatorView(std::shared_ptr<CapyUi::WorkspaceData> data, std::function<void()> layoutChanged);
    ~NavigatorView();
    winrt::Microsoft::UI::Xaml::FrameworkElement Root() const;
    void Apply(bool visible);
    double NaturalHeight(double width) const;
    double FixedHeight() const;
    CapyUi::J Placement(winrt::Microsoft::UI::Xaml::UIElement const& reference,
        winrt::Windows::Foundation::Rect clip, int order,bool capture=false) const;
    CapyUi::J PresentedPlacement(winrt::Microsoft::UI::Xaml::UIElement const& reference)const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
