#pragma once
#include "WorkspaceGestures.h"

class CollapsedColumns {
public:
    CollapsedColumns(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas root,std::shared_ptr<WorkspaceGestures> gestures);
    ~CollapsedColumns();
    void Apply();
    void Reset();
    void AppendGlass(CapyUi::A& regions,CapyUi::A& connections,winrt::Microsoft::UI::Xaml::UIElement const& reference)const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
