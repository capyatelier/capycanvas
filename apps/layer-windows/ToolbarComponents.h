#pragma once
#include "UiControls.h"
#include "WorkspaceGestures.h"

class ToolbarComponent {
public:
    ToolbarComponent(std::shared_ptr<CapyUi::WorkspaceData> data,CapyUi::J const& panel,CapyUi::J const& tile,
        std::shared_ptr<WorkspaceGestures> const& gestures);
    ~ToolbarComponent();
    winrt::Microsoft::UI::Xaml::FrameworkElement Root()const;
    void Update(CapyUi::J const& tile);
    void Layout(CapyUi::J const& bounds,bool vertical);
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
