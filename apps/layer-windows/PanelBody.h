#pragma once
#include "UiControls.h"
#include "NavigatorView.h"
#include "WorkspaceGestures.h"

// Undecorated native panel content. Each placement owns its widgets while
// WorkspaceData shares preview/thumbnail caches and shared document state.
class PanelBody {
public:
    PanelBody(std::shared_ptr<CapyUi::WorkspaceData> data,CapyUi::J const& panel,
        CapyUi::J const& geometry,std::function<void()> layoutChanged,std::shared_ptr<WorkspaceGestures> const& gestures={},bool scrollable=true);
    winrt::Microsoft::UI::Xaml::FrameworkElement Root()const{return root;}
    void Apply(bool visible);
    void Layout(CapyUi::J const& geometry);
    double ContentHeight()const;
    std::unique_ptr<NavigatorView> navigator;
    std::map<uint32_t,winrt::Microsoft::UI::Xaml::FrameworkElement> tileElements;
    std::map<std::wstring,winrt::Microsoft::UI::Xaml::FrameworkElement> anchors;
private:
    std::shared_ptr<CapyUi::WorkspaceData> data;
    winrt::Microsoft::UI::Xaml::FrameworkElement root{nullptr};
    CapyUi::Bindings bindings;
    std::function<double()> contentHeight;
    std::vector<uint32_t> tileOrder;
    std::map<uint32_t,winrt::Microsoft::UI::Xaml::Controls::Border> dividers;
    winrt::Microsoft::UI::Xaml::FrameworkElement tileGrip{nullptr};
    winrt::Microsoft::UI::Xaml::Controls::Image tileGripMark{nullptr};
    winrt::Microsoft::UI::Xaml::Controls::Grid sizes(double width);
};