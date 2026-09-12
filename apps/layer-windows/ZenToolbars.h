#pragma once
#include "PanelBody.h"

// Core owns edge placement, section splitting and clipping. Native controls
// retain their identities while these transient projections move on resize.
class ZenToolbars {
public:
    ZenToolbars(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas const& root,std::shared_ptr<WorkspaceGestures> gestures);
    void Apply();
    void Reset();
    winrt::Microsoft::UI::Xaml::FrameworkElement Anchor(std::wstring const& control)const;
private:
    std::shared_ptr<CapyUi::WorkspaceData> data;
    winrt::Microsoft::UI::Xaml::Controls::Canvas root{nullptr};
    std::shared_ptr<WorkspaceGestures> gestures;
    struct Section {
        winrt::Microsoft::UI::Xaml::Controls::Border frame;
        winrt::hstring signature;
        std::unique_ptr<PanelBody> body;
    };
    std::map<std::wstring,Section> sections;
};
