#pragma once
#include "UiControls.h"
#include <array>
#include <optional>

namespace CapyUi {
struct RangeControl : std::enable_shared_from_this<RangeControl> {
    std::shared_ptr<WorkspaceData> data;
    std::array<J,2> bounds;
    hstring label,prefix;
    std::function<void(int,double)> change;
    Grid root;
    Canvas track;
    Border trough,fill;
    std::array<Border,2> thumbs;
    std::array<double,2> values{};
    std::array<double,2> domain{0,1};
    Bindings fields;
    struct Contact {uint32_t id;int index;double before;std::array<double,2> domain;double offset;};
    std::optional<Contact> contact;
    bool retired=false;
    static std::shared_ptr<RangeControl> Create(std::shared_ptr<WorkspaceData> const& data,J const& lower,J const& upper,
        hstring const& label,hstring const& prefix,bool showTrack,std::function<void(int,double)> change);
    void Update(double lower,double upper);
    void Dispose();
    void init(bool showTrack);
    void paint();
    void set(int index,double value);
    void pick(double x);
    void end(bool cancel);
    double position(double value,double width)const;
};
}
