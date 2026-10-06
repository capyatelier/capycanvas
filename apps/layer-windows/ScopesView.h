#pragma once
#include "UiControls.h"

namespace CapyScopes {
using namespace CapyUi;
FrameworkElement ScopePanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,bool waveform);
FrameworkElement TonalScope(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings);
FrameworkElement ScopeFooter(std::shared_ptr<WorkspaceData> const& data,hstring const& prefix,std::function<J()> view,Bindings& bindings,
    FrameworkElement const& trailing=nullptr);
FrameworkElement TonalPlot(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings);
}
