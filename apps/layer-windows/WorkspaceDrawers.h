#pragma once
#include "PanelBody.h"

class WorkspaceDrawers {
public:
    WorkspaceDrawers(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas root,
        std::shared_ptr<WorkspaceGestures> gestures,std::function<void()> changed);
    ~WorkspaceDrawers();
    void Apply(NativeLayoutInput input,float width,float height);
    void Reset();
    void AppendOverviews(CapyUi::A& slots)const;
    CapyUi::A Sources()const;
    void AppendGlass(CapyUi::A& regions,CapyUi::A& connections,winrt::Microsoft::UI::Xaml::UIElement const& reference)const;
    winrt::Microsoft::UI::Xaml::FrameworkElement Anchor(std::wstring const& control)const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
