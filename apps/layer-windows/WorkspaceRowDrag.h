#pragma once
#include "pch.h"
#include <functional>
#include <memory>
#include <optional>
#include <vector>

// Native pickup and presentation only. A completed drop submits one shared
// SwitcherEdit::Move; pending contacts never change the workspace preview.
class WorkspaceRowDrag {
public:
    using Id=winrt::hstring;
    using Move=std::function<void(Id,std::optional<Id>)>;
    WorkspaceRowDrag(winrt::Microsoft::UI::Xaml::Controls::ListView list,
        winrt::Microsoft::UI::Xaml::Controls::Grid surface,
        std::function<bool()> available,Move move,std::function<void(Id)> select,std::function<void()> ended);
    ~WorkspaceRowDrag();
    void Attach(winrt::Microsoft::UI::Xaml::Controls::ListViewItem row,
        winrt::Microsoft::UI::Xaml::Controls::Button grip,
        winrt::Microsoft::UI::Xaml::Controls::Button more,
        winrt::Microsoft::UI::Xaml::Controls::MenuFlyout menu,Id id);
    void Refresh(std::vector<Id> const& order);
    bool FenceSelection()const;
    bool SuppressClick()const;
    bool Cancel();
    bool Escape();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
