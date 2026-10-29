#pragma once
#include "UiControls.h"
#include <chrono>
#include <winrt/Windows.UI.ViewManagement.h>

namespace CapyUi {
struct ChromeVisibility:std::enable_shared_from_this<ChromeVisibility>{
    std::optional<bool> shown;bool interactive=true;
    weak_ref<FrameworkElement> boundElement;
    CompositionTarget::Rendering_revoker frames;
    void apply(FrameworkElement const& element,bool placed,bool hidden,std::function<void()> changed={}){
        if(boundElement.get()!=element){frames.revoke();shown.reset();boundElement=make_weak(element);interactive=element.IsHitTestVisible();}
        if(!placed){frames.revoke();shown.reset();element.IsHitTestVisible(false);element.Opacity(0);element.Visibility(Visibility::Collapsed);return;}
        bool visible=!hidden;
        element.IsHitTestVisible(visible&&interactive);
        if(shown==visible)return;
        bool initial=!shown;shown=visible;frames.revoke();
        auto target=visible?1.:0.;
        if(initial||!element.IsLoaded()||!winrt::Windows::UI::ViewManagement::UISettings().AnimationsEnabled()){
            element.Opacity(target);element.Visibility(visible?Visibility::Visible:Visibility::Collapsed);return;
        }
        auto from=element.Opacity();element.Visibility(Visibility::Visible);auto start=std::chrono::steady_clock::now();
        frames=CompositionTarget::Rendering(auto_revoke,[weak=weak_from_this(),view=make_weak(element),from,target,start,changed](auto&&,auto&&){
            auto self=weak.lock();auto element=view.get();if(!self)return;if(!element){self->frames.revoke();return;}
            if(!element.IsLoaded()){element.Opacity(target);element.Visibility(target?Visibility::Visible:Visibility::Collapsed);self->frames.revoke();return;}
            auto t=std::clamp(std::chrono::duration<double,std::milli>(std::chrono::steady_clock::now()-start).count()/180.,0.,1.);
            element.Opacity(from+(target-from)*(1-std::pow(1-t,3)));
            if(t>=1){element.Visibility(target?Visibility::Visible:Visibility::Collapsed);self->frames.revoke();}
            if(changed)changed();
        });
    }
};
}
