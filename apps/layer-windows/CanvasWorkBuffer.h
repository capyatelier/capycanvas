#pragma once
#include "native/include/capy_windows.h"
#include <cstddef>
#include <deque>
#include <functional>
#include <string>
#include <utility>
#include <variant>
#include <vector>

// Protected by CanvasWindow's mutex. Both item count and payload storage are
// bounded. Callers must split pointer histories into small, ordered batches.
enum class CanvasCommandKind { Action, Input, EdgeContact, Document, Workspace, Overviews, Glass, Filters, Prediction, DeviceLoss, TestDisplay, LocalizationInput };
struct CanvasCommand { CanvasCommandKind kind=CanvasCommandKind::Action; std::string json; std::function<void(int32_t)> reply; };
struct CanvasScroll {
    float x, y, dx, dy, density;
    bool zoom, horizontal;
};
using CanvasWork = std::variant<std::vector<CapyPointer>, CanvasCommand, CanvasScroll>;

class CanvasWorkBuffer {
public:
    static constexpr size_t MaxItems=256;
    static constexpr size_t MaxBytes=1024*1024;
    static constexpr size_t PointerBatch=64;
    static constexpr size_t CommandSlots=32;
    static constexpr size_t CommandBytes=64*1024;
    bool Empty() const { return items.empty(); }
    bool CanPush(CanvasWork const& item) const {
        auto cost=Bytes(item);
        bool command=std::holds_alternative<CanvasCommand>(item);
        size_t limit=command?MaxBytes:MaxBytes-CommandBytes;
        size_t slots=command?MaxItems:MaxItems-CommandSlots;
        return items.size()<slots && bytes<=limit && cost<=limit-bytes;
    }
    bool Push(CanvasWork&& item) {
        if(!CanPush(item))return false;
        auto cost=Bytes(item);
        items.emplace_back(std::move(item));
        bytes+=cost;
        return true;
    }
    std::deque<CanvasWork> Take() {
        std::deque<CanvasWork> result;
        result.swap(items);
        bytes=0;
        return result;
    }
    std::deque<CanvasWork> TakeBeforePointers() {
        std::deque<CanvasWork> result;
        while(!items.empty()&&!std::holds_alternative<std::vector<CapyPointer>>(items.front())) {
            bytes-=Bytes(items.front());
            result.emplace_back(std::move(items.front()));
            items.pop_front();
        }
        return result;
    }
private:
    static size_t Bytes(CanvasWork const& item) {
        // Capacity counts retained allocations, including spare vector/string
        // space, rather than only the number of live samples or characters.
        if(auto points=std::get_if<std::vector<CapyPointer>>(&item))
            return sizeof(CanvasWork)+points->capacity()*sizeof(CapyPointer);
        if(auto command=std::get_if<CanvasCommand>(&item))
            return sizeof(CanvasWork)+command->json.capacity()+1;
        return sizeof(CanvasWork);
    }
    std::deque<CanvasWork> items;
    size_t bytes=0;
};
