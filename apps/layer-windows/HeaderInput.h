#pragma once
#include "UiControls.h"
#include <optional>

// Native capture/timing; shared HeaderDrag owns geometry, validation and history.
class HeaderInput {
public:
    HeaderInput(std::shared_ptr<CapyUi::WorkspaceData> data,
        winrt::Microsoft::UI::Xaml::Controls::Canvas root,std::function<void()> changed);
    ~HeaderInput();
    void Source(winrt::Microsoft::UI::Xaml::FrameworkElement const& element,
        CapyUi::J const& source,winrt::hstring const& label);
    void Configure(CapyUi::J const& model,bool editing,CapyUi::J const& geometryRequest);
    bool Key(winrt::Microsoft::UI::Xaml::Input::KeyRoutedEventArgs const& event,bool pressed);
    bool Cancel();
    void Select(uint32_t id);
    uint32_t Selected()const;
    CapyUi::J Preview()const;
    CapyUi::J Source()const;
    bool Busy()const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
