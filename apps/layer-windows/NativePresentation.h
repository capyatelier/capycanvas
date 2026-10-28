#pragma once
#include <winrt/Microsoft.UI.Xaml.Media.h>
#include <functional>

class NativePresentation {
    winrt::Microsoft::UI::Xaml::Media::CompositionTarget::Rendering_revoker pending;
public:
    void Cancel(){pending.revoke();}
    void AfterLayout(std::function<void()> ready){
        Cancel();
        pending=winrt::Microsoft::UI::Xaml::Media::CompositionTarget::Rendering(winrt::auto_revoke,
            [this,ready=std::move(ready)](auto&&,auto&&){Cancel();ready();});
    }
};
