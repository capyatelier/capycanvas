#pragma once
#include "UiControls.h"

// Retained native projection of Rust's ColorPanelView.
winrt::Microsoft::UI::Xaml::FrameworkElement ColorPanel(
    std::shared_ptr<CapyUi::WorkspaceData> const& data, CapyUi::Bindings& bindings);
