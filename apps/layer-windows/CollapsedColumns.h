#pragma once
#include "WorkspaceGestures.h"

class CollapsedColumns {
public:
    CollapsedColumns(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas root,std::shared_ptr<WorkspaceGestures> gestures);
    ~CollapsedColumns();
    void Apply();
    void Reset();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
