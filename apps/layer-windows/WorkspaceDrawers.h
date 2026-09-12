#pragma once
#include "PanelBody.h"

// Projects shared content drawers. Every placement owns native panel widgets;
// the render owner remains responsible for geometry and GPU overview images.
class WorkspaceDrawers {
public:
    WorkspaceDrawers(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas root,
        std::shared_ptr<WorkspaceGestures> gestures,std::function<void()> changed);
    ~WorkspaceDrawers();
    void Apply();
    void Reset();
    void AppendOverviews(CapyUi::A& slots)const;
    winrt::Microsoft::UI::Xaml::FrameworkElement Anchor(std::wstring const& control)const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
