#pragma once
#include "UiControls.h"

class WorkspaceGestures {
public:
    WorkspaceGestures(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas root);
    ~WorkspaceGestures();
    enum class Pickup { Immediate, Hold };
    // Classify the visible source independently of its Rust payload.
    // Action is DragWorkspace, a native tile_drag, DragDivider, ResizeFloating
    // or ResizeColumnPanel. Resize handles always use immediate pickup.
    void Source(winrt::Microsoft::UI::Xaml::FrameworkElement const& element,
        CapyUi::J const& action,CapyUi::J const& context={},bool doubleClick=false,
        CapyUi::J const& tab={},Pickup pickup=Pickup::Immediate);
    void Refresh();
    void Present(CapyUi::J const& drag);
    void ChromeChanged();
    bool Cancel();
    bool SuppressClick()const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
