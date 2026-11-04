#pragma once
#include "UiControls.h"

class WorkspaceBubbles {
public:
    struct Impl;
    WorkspaceBubbles(std::shared_ptr<CapyUi::WorkspaceData>,winrt::Microsoft::UI::Xaml::Controls::Canvas,std::function<void()>,std::function<void()>);
    ~WorkspaceBubbles();
    void Attach(winrt::Microsoft::UI::Xaml::UIElement const&,std::function<void()>);
    void Apply();
    void Reattach();
    void Wake();
    bool Cancel();
    void Glass(CapyUi::A&,CapyUi::A&,CapyUi::A&,winrt::Microsoft::UI::Xaml::UIElement const&)const;
    CapyUi::A TraceOverlays(winrt::Microsoft::UI::Xaml::UIElement const&)const;
    CapyUi::A InputRegions(winrt::Microsoft::UI::Xaml::UIElement const&)const;
private:
    std::shared_ptr<Impl> impl;
};
