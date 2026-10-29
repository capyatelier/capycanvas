#pragma once
#include "UiControls.h"

namespace CapyUi {
struct PreferenceGroup {StackPanel section,list,actions;Grid heading;TextBlock title{nullptr},summary{nullptr};};
PreferenceGroup preferenceGroup(std::shared_ptr<WorkspaceData> const&,hstring const&,hstring const& summary=L"");

class ShortcutPage {
public:
    ShortcutPage(std::shared_ptr<WorkspaceData> data,Grid overlay);
    ~ShortcutPage();
    StackPanel Container(hstring const& page,StackPanel const& node);
    void Apply(J const& snapshot);
    hstring Title()const;
    void Back();
    bool Dismiss();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
}
