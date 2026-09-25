#pragma once
#include "UiControls.h"

winrt::Microsoft::UI::Xaml::FrameworkElement PalettesPanel(
    std::shared_ptr<CapyUi::WorkspaceData> const& data,CapyUi::Bindings& bindings,
    std::function<double()>* contentHeight=nullptr,std::function<CapyUi::J()>* scrollMetrics=nullptr);
