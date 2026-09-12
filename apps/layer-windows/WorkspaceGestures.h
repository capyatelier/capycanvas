#pragma once
#include "UiControls.h"

class WorkspaceGestures {
public:
    WorkspaceGestures(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas root);
    ~WorkspaceGestures();
    // Action is DragWorkspace, a native tile_drag, DragDivider or ResizeFloating.
    void Source(winrt::Microsoft::UI::Xaml::FrameworkElement const& element,
        CapyUi::J const& action,CapyUi::J const& context={},bool doubleClick=false,
        CapyUi::J const& tab={});
    void Refresh();
    void ChromeChanged();
    bool Cancel();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
