#pragma once
#include "native/include/capy_windows.h"
#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <functional>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

// Protected by CanvasWindow's mutex. Both item count and payload storage are
// bounded. Callers must split pointer histories into small, ordered batches.
enum class CanvasCommandKind { Action, Input, EdgeContact, Document, Workspace, Glass, Filters, Prediction, DeviceLoss, TestDisplay, LocalizationInput };
struct CanvasCommand { CanvasCommandKind kind=CanvasCommandKind::Action; std::string json; std::function<void(int32_t)> reply; };
struct CanvasScroll {
    float x, y, dx, dy, density;
    bool zoom, horizontal;
};
using CanvasWork = std::variant<std::vector<CapyPointer>, CanvasCommand, CanvasScroll>;

inline std::string_view CanvasStateSlot(CanvasCommand const& command) {
    if(command.reply)return {};
    if(command.kind==CanvasCommandKind::Glass)return "glass";
    std::string_view json=command.json;
    for(std::string_view report:{R"({"type":"measure_panels")",R"({"type":"measure_header")",R"({"type":"measure_titlebar")"})
        if(json.substr(0,report.size())==report)return report;
    std::string_view chrome=R"({"type":"chrome")";
    if(json.substr(0,chrome.size())==chrome&&json.find(R"("event":{"kind":"refresh"})")!=std::string_view::npos)return "chrome refresh";
    return {};
}

class CanvasWorkBuffer {
public:
    static constexpr size_t MaxItems=256;
    static constexpr size_t MaxBytes=1024*1024;
    static constexpr size_t PointerBatch=64;
    static constexpr size_t CommandSlots=32;
    static constexpr size_t CommandBytes=64*1024;
    bool Empty() const { return items.empty(); }
    bool HasBeforePointers() const { return !items.empty()&&!std::holds_alternative<std::vector<CapyPointer>>(items.front()); }
    bool CanPush(CanvasWork const& item) const {
        auto cost=Bytes(item);
        bool command=std::holds_alternative<CanvasCommand>(item);
        size_t limit=command?MaxBytes:MaxBytes-CommandBytes;
        size_t slots=command?MaxItems:MaxItems-CommandSlots;
        return items.size()<slots && bytes<=limit && cost<=limit-bytes;
    }
    bool Push(CanvasWork&& item) {
        auto command=std::get_if<CanvasCommand>(&item);
        auto slot=command?CanvasStateSlot(*command):std::string_view{};
        auto queued=slot.empty()?items.end():std::find_if(items.begin(),items.end(),[&](CanvasWork const& entry){
            auto other=std::get_if<CanvasCommand>(&entry);return other&&other->kind==command->kind&&CanvasStateSlot(*other)==slot;
        });
        if(queued!=items.end()){
            auto replaced=bytes-Bytes(*queued)+Bytes(item);
            if(replaced>MaxBytes)return false;
            *queued=std::move(item);bytes=replaced;
            return true;
        }
        if(!CanPush(item))return false;
        auto cost=Bytes(item);
        items.emplace_back(std::move(item));
        bytes+=cost;++pushed;
        return true;
    }
    std::deque<CanvasWork> Take() {
        std::deque<CanvasWork> result;
        result.swap(items);
        bytes=0;taken=pushed;
        return result;
    }
    std::deque<CanvasWork> TakeBeforePointers() {
        std::deque<CanvasWork> result;
        while(!items.empty()&&!std::holds_alternative<std::vector<CapyPointer>>(items.front())) {
            bytes-=Bytes(items.front());
            result.emplace_back(std::move(items.front()));
            items.pop_front();++taken;
        }
        return result;
    }
    uint64_t Pushed() const { return pushed; }
    uint64_t Taken() const { return taken; }
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
    uint64_t pushed=0,taken=0;
};
