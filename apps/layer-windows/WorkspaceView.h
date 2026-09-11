#pragma once
#include "pch.h"
#include "FilterPreviews.h"
#include <functional>
#include <memory>

// UI-thread-only native widgets. Rust owns layout, document and numeric policy.
class WorkspaceView {
public:
    using Json = winrt::Windows::Data::Json::JsonObject;
    using Dispatch = std::function<void(std::string)>;
    WorkspaceView(Dispatch dispatch, Json catalog, Dispatch overviews, PreviewTransport previews,std::function<void(bool)> popupChanged, Dispatch document);
    ~WorkspaceView();
    winrt::Microsoft::UI::Xaml::Controls::Canvas Root() const;
    void Apply(Json const& snapshot);
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
