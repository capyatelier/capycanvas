#pragma once
#include "UiControls.h"

// Clips lower native visuals where a higher GPU overview must be visible.
// No image copies, additional swap chains, or per-paint geometry work.
class OverviewOcclusion {
public:
    OverviewOcclusion();
    ~OverviewOcclusion();
    void Apply(winrt::Microsoft::UI::Xaml::Controls::Canvas const& root,
        CapyUi::A const& slots,CapyUi::J const& document);
private:
    struct Impl;
    std::unique_ptr<Impl> impl;
};
