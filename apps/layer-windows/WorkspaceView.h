#pragma once
#include "pch.h"
#include "native/include/capy_windows.h"
#include "FilterPreviews.h"
#include <functional>
#include <memory>

// UI-thread-only native widgets. Rust owns layout, document and numeric policy.
class WorkspaceView {
public:
    using Json = winrt::Windows::Data::Json::JsonObject;
    using Dispatch = std::function<void(std::string)>;
    WorkspaceView(Dispatch dispatch, Json catalog,std::shared_ptr<CapyLocalization> localization, Dispatch overviews, PreviewTransport previews,std::function<void(bool)> popupChanged, Dispatch document, Dispatch input);
    ~WorkspaceView();
    winrt::Microsoft::UI::Xaml::Controls::Canvas Root() const;
    bool Apply(Json const& snapshot);
    Json ChromeFacts(bool popupOpen);
    bool CancelGesture();
    void CancelPreviews();
    void SetTitlebarInsets(float left,float right,float height);
    void SetZenButtonBounds(Json const& bounds);
    void SetWindowId(uint64_t id);
    void SetGlassChanged(std::function<void()> changed);
    void CanvasContact(bool active);
    winrt::Windows::Data::Json::JsonArray DrawerSources() const;
    winrt::Windows::Data::Json::JsonArray Glass(winrt::Microsoft::UI::Xaml::UIElement const& reference,winrt::Windows::Data::Json::JsonArray& connections) const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
