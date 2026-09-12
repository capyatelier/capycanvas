#pragma once
#include "WorkspaceGestures.h"

// UI-thread scheduler for shared expansion geometry. It never owns a canvas,
// document or GPU resource, and keeps at most one optional query in flight.
class WorkspaceExpansion {
public:
    WorkspaceExpansion(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas root,
        std::shared_ptr<WorkspaceGestures> gestures,std::function<void()> changed);
    ~WorkspaceExpansion();
    void Apply(double configurationHeight);
    void Reset();
    CapyUi::J Geometry()const;
    winrt::hstring Panel()const;
    bool Closing()const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
