#pragma once
#include "pch.h"
#include "NativeHandles.h"
#include "FilterPreviews.h"
#include <functional>
#include <memory>

// UI-thread-only native widgets. Rust owns layout, document and numeric policy.
class WorkspaceView {
public:
    using Json = winrt::Windows::Data::Json::JsonObject;
    using Dispatch = std::function<void(std::string)>;
    using EdgeTransport = std::function<void(std::string,std::function<void(int32_t)>)>;
    WorkspaceView(Dispatch dispatch, Json catalog,NativeLocalization localization, Dispatch overviews, PreviewTransport previews,std::function<void(bool)> popupChanged, Dispatch document, Dispatch input, EdgeTransport edges);
    ~WorkspaceView();
    winrt::Microsoft::UI::Xaml::Controls::Canvas Root() const;
    bool Apply(Json const& snapshot,std::string layoutBytes={});
    static void MergeLayout(Json const& model,Json const& patch);
    Json ChromeFacts(bool popupOpen);
    void SetWindowResizing(bool value);
    bool CancelGesture();
    void CancelNativeContacts();
    void CancelPreviews();
    void SetTitlebarInsets(float left,float right,float height);
    void SetWindowId(uint64_t id);
    void SetGlassChanged(std::function<void()> changed);
    void CanvasContact(bool active);
    void AttachOverlays(winrt::Microsoft::UI::Xaml::UIElement const&,std::function<void()>);
    winrt::Windows::Data::Json::JsonArray DrawerSources() const;
    winrt::Windows::Data::Json::JsonArray OverlayInputRegions(winrt::Microsoft::UI::Xaml::UIElement const&)const;
    winrt::Windows::Data::Json::JsonArray Glass(winrt::Microsoft::UI::Xaml::UIElement const& reference,winrt::Windows::Data::Json::JsonArray& connections,winrt::Windows::Data::Json::JsonArray& arcs,winrt::Windows::Data::Json::JsonArray& colors,winrt::Windows::Data::Json::JsonArray& panels,uint64_t& panelRevision) const;
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
