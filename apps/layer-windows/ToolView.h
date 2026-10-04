#pragma once
#include "UiControls.h"

winrt::Microsoft::UI::Xaml::FrameworkElement ToolSetPanel(
    std::shared_ptr<CapyUi::WorkspaceData> const& data, CapyUi::Bindings& bindings, winrt::hstring const& panel=L"brushes",std::function<CapyUi::J()> projection={});
winrt::Microsoft::UI::Xaml::FrameworkElement ToolSettingsPanel(
    std::shared_ptr<CapyUi::WorkspaceData> const& data, CapyUi::Bindings& bindings);
winrt::Microsoft::UI::Xaml::FrameworkElement BrushSizePanel(
    std::shared_ptr<CapyUi::WorkspaceData> const& data, CapyUi::Bindings& bindings);
