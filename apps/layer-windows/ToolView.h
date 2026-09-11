#pragma once
#include "UiControls.h"

winrt::Microsoft::UI::Xaml::FrameworkElement ToolSetPanel(
    std::shared_ptr<CapyUi::WorkspaceData> const& data, CapyUi::Bindings& bindings);
winrt::Microsoft::UI::Xaml::FrameworkElement ToolSettingsPanel(
    std::shared_ptr<CapyUi::WorkspaceData> const& data, CapyUi::Bindings& bindings);
