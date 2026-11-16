#pragma once
#include "pch.h"
#include "NativeHandles.h"
#include "FilterPreviews.h"
#include "CanvasModelInputs.h"
#include <functional>
#include <memory>

// UI-thread-only native widgets. Rust owns layout, document and numeric policy.
class WorkspaceView {
public:
    using Json = winrt::Windows::Data::Json::JsonObject;
    using Dispatch = std::function<void(std::string)>;
    using EdgeTransport = std::function<void(std::string,std::function<void(int32_t)>)>;
    WorkspaceView(Dispatch dispatch, Json catalog,NativeLocalization localization, PreviewTransport previews,std::function<void(bool)> popupChanged, Dispatch document, Dispatch input, EdgeTransport edges);
    ~WorkspaceView();
    winrt::Microsoft::UI::Xaml::Controls::Canvas Root() const;
    bool Apply(Json const& snapshot,NativeLayoutInput input={},CanvasModelChanges const& changes={true});
    bool ApplyFields(Json const& snapshot,bool scopes,CanvasModelChanges const& changes);
    bool HasPendingChanges()const;
    void Resize(float width,float height);
    Json ChromeFacts(bool popupOpen);
    Json ChromeDocument()const;
    void SetWindowResizing(bool value);
    bool CancelGesture();
    bool CancelNativeContacts();
    void CancelPreviews();
    void SetTitlebarInsets(float left,float right,float height);
    void SetWindowId(uint64_t id);
    void SetFillFeedback(std::function<void(Json const&)> feedback);
    void SetGlassChanged(std::function<void()> changed);
    void SetNavigatorSurfaces(std::function<void(uint64_t,CapyNavigatorViewport const*,std::function<void(NativeSurfaceAttachment)>)>);
    void ResetNavigatorSurfaces();
    void SetReport(Dispatch report);
    void SetCommit(std::function<void(std::string,bool,std::function<void()>)> commit);
    void CanvasContact(bool active);
    void AttachOverlays(winrt::Microsoft::UI::Xaml::UIElement const&,std::function<void()>);
    winrt::Windows::Data::Json::JsonArray DrawerSources() const;
    winrt::Windows::Data::Json::JsonArray OverlayInputRegions(winrt::Microsoft::UI::Xaml::UIElement const&)const;
    winrt::Windows::Data::Json::JsonArray Glass(winrt::Microsoft::UI::Xaml::UIElement const& reference,winrt::Windows::Data::Json::JsonArray& connections,winrt::Windows::Data::Json::JsonArray& arcs,winrt::Windows::Data::Json::JsonArray& colors,winrt::Windows::Data::Json::JsonArray& panels) const;
    void ChromeSent(uint64_t revision);
    void MeasureChrome();
    void PublishTrace();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
