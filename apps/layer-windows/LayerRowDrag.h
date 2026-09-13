#pragma once
#include "pch.h"
#include <memory>

namespace CapyLayers {
struct LayersView;
struct LayerRow;
// Native contact arbitration and row feedback. Rust owns validation and history.
class LayerRowDrag {
public:
    explicit LayerRowDrag(std::shared_ptr<LayersView> const&);
    ~LayerRowDrag();
    void Attach(std::shared_ptr<LayerRow> const&);
    void Refresh();
    void MenuChanged();
    bool SuppressClick()const;
    bool SuppressContext()const;
    bool Cancel();
private:
    struct Impl;
    std::shared_ptr<Impl> impl;
};
}
