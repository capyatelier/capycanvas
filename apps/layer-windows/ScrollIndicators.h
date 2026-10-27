#pragma once
#include "pch.h"
#include <winrt/Microsoft.UI.Xaml.Media.Animation.h>
#include <memory>

namespace CapyUi {
struct ScrollIndicators {
    winrt::weak_ref<winrt::Microsoft::UI::Xaml::Controls::Control> owner;
    winrt::weak_ref<winrt::Microsoft::UI::Xaml::FrameworkElement> attached;
    void attach(){
        using namespace winrt::Microsoft::UI::Xaml;
        auto scroll=owner.get();if(!scroll||!scroll.IsLoaded())return;
        scroll.ApplyTemplate();
        if(!Media::VisualTreeHelper::GetChildrenCount(scroll))return;
        auto root=Media::VisualTreeHelper::GetChild(scroll,0).try_as<FrameworkElement>();
        if(!root||root==attached.get())return;attached=winrt::make_weak(root);
        auto groups=VisualStateManager::GetVisualStateGroups(root);
        for(uint32_t i=groups.Size();i>0;--i){
            auto group=groups.GetAt(i-1);if(group.Name()!=L"ScrollingIndicatorStates")continue;
            for(auto state:group.States())if(auto timeline=state.Storyboard())timeline.Stop();
            for(auto transition:group.Transitions())if(auto timeline=transition.Storyboard())timeline.Stop();
            groups.RemoveAt(i-1);
        }
        for(auto name:{L"PART_HorizontalScrollBar",L"PART_VerticalScrollBar",L"HorizontalScrollBar",L"VerticalScrollBar"})
            if(auto bar=root.FindName(name).try_as<Controls::Primitives::ScrollBar>())
                bar.IndicatorMode(Controls::Primitives::ScrollingIndicatorMode::MouseIndicator);
    }
};
inline void retainScrollIndicators(winrt::Microsoft::UI::Xaml::Controls::Control const& scroll){
    auto state=std::make_shared<ScrollIndicators>();state->owner=winrt::make_weak(scroll);
    scroll.Loaded([state](auto&&,auto&&){state->attach();});
    scroll.RegisterPropertyChangedCallback(winrt::Microsoft::UI::Xaml::Controls::Control::TemplateProperty(),
        [state](auto&&,auto&&){state->attach();});
    state->attach();
}
}
