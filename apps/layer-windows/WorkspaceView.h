#pragma once
#include "pch.h"
#include <functional>
#include <memory>

// UI-thread-only native widgets. Rust owns layout, document and numeric policy.
class WorkspaceView {
public:
    using Json = winrt::Windows::Data::Json::JsonObject;
    using Dispatch = std::function<void(std::string)>;
    WorkspaceView(Dispatch dispatch, Json catalog);
    ~WorkspaceView();
    winrt::Microsoft::UI::Xaml::Controls::Canvas Root() const;
    void Apply(Json const& snapshot);
private:
    struct Impl;
    std::unique_ptr<Impl> impl;
};
