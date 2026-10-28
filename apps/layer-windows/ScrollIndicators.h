#pragma once
#include "pch.h"
#include <winrt/Microsoft.UI.Xaml.Media.Animation.h>
#include <memory>

namespace CapyUi {
struct ScrollIndicatorStates:winrt::Microsoft::UI::Xaml::VisualStateManagerT<ScrollIndicatorStates> {
    bool GoToStateCore(winrt::Microsoft::UI::Xaml::Controls::Control const& control,winrt::Microsoft::UI::Xaml::FrameworkElement const& root,
        winrt::hstring const& name,winrt::Microsoft::UI::Xaml::VisualStateGroup const& group,winrt::Microsoft::UI::Xaml::VisualState const& state,bool transitions){
        auto base=m_inner.as<winrt::Microsoft::UI::Xaml::IVisualStateManagerOverrides>();
        if(group&&group.Name()==L"ScrollingIndicatorStates")for(auto candidate:group.States())if(candidate.Name()==L"MouseIndicator")
            return base.GoToStateCore(control,root,L"MouseIndicator",group,candidate,false);
        return base.GoToStateCore(control,root,name,group,state,transitions);
    }
};
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
        VisualStateManager::SetCustomVisualStateManager(root,winrt::make<ScrollIndicatorStates>());
        VisualStateManager::GoToState(scroll,L"MouseIndicator",false);
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
