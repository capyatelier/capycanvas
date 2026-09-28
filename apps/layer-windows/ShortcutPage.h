#pragma once
#include "UiControls.h"

namespace CapyUi {
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
