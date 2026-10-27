#pragma once
#include "WorkspaceGestures.h"

class CollapsedColumns {
public:
    CollapsedColumns(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas root,std::shared_ptr<WorkspaceGestures> gestures,std::function<void()> changed);
    ~CollapsedColumns();
    void Apply();
    void Reset();
    CapyUi::A Measure(winrt::Microsoft::UI::Xaml::UIElement const& reference)const;
    void PresentBridges(CapyUi::J const& frame,bool ready,bool resizing=false);
    double Opacity(uint32_t column)const;
    void AppendGlass(CapyUi::A& regions,winrt::Microsoft::UI::Xaml::UIElement const& reference)const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
