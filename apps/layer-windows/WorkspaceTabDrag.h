#pragma once
#include "UiControls.h"

// Native preview copies; the shared session owns offsets and insertion policy.
// Original tab widgets stay in their measured slots throughout the gesture.
class WorkspaceTabDrag {
public:
    WorkspaceTabDrag(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas root);
    ~WorkspaceTabDrag();
    void Grab(CapyUi::J const& tab,std::vector<winrt::weak_ref<winrt::Microsoft::UI::Xaml::FrameworkElement>> const& tabs);
    CapyUi::J Begin();
    void Update(CapyUi::J const& preview);
    void Refresh(std::vector<winrt::weak_ref<winrt::Microsoft::UI::Xaml::FrameworkElement>> const& tabs);
    void Clear();
private:
    struct Impl;
    std::unique_ptr<Impl> impl;
};
