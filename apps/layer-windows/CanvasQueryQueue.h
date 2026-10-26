#pragma once
#include "NativeHandles.h"
#include <deque>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <atomic>

enum class CanvasQueryKind { Filters, Thumbnails, LayerMenu, Workspace, Document, Papers };
using PreviewPacket=NativePreview;
using PreviewReply=std::function<void(PreviewPacket)>;
using PreviewTransport=std::function<bool(CanvasQueryKind,std::string,PreviewReply)>;
struct CanvasQuery {CanvasQueryKind kind;std::string json;PreviewReply reply;};

// Optional UI queries never consume the input queue's reserved command space.
// Each cache allows one outstanding reply; this queue also bounds the window
// when multiple panels or native menus request work together.
class CanvasQueryQueue {
public:
    static constexpr size_t MaxItems=8,MaxPayload=8192;
    bool Empty() const{return items.empty();}
    bool Push(CanvasQuery&& item){
        if(item.json.capacity()>MaxPayload)return false;
        if(item.kind==CanvasQueryKind::LayerMenu)
            for(auto& queued:items)if(queued.kind==CanvasQueryKind::LayerMenu){queued=std::move(item);return true;}
        if(items.size()>=MaxItems)return false;
        items.emplace_back(std::move(item));return true;
    }
    std::optional<CanvasQuery> Take(std::optional<CanvasQueryKind> kind={}){
        auto next=items.end();
        for(auto it=items.begin();it!=items.end();++it){
            if(kind&&it->kind!=*kind)continue;
            if(next==items.end())next=it;
            if(it->kind==CanvasQueryKind::LayerMenu){next=it;break;}
        }
        if(next==items.end())return {};
        auto item=std::move(*next);items.erase(next);return item;
    }
    void Clear(){items.clear();}
private:
    std::deque<CanvasQuery> items;
};

enum class CanvasRenderStep { Wait, Answer, Frame };
inline CanvasRenderStep NextCanvasRenderStep(bool canvasPending,CanvasQueryQueue const& queries){
    return canvasPending?CanvasRenderStep::Frame:queries.Empty()?CanvasRenderStep::Wait:CanvasRenderStep::Answer;
}

class CanvasQuerySlots {
    struct Slot {
        std::shared_ptr<std::atomic<size_t>> count;
        bool held=false;
        explicit Slot(std::shared_ptr<std::atomic<size_t>> value):count(std::move(value)){}
        ~Slot(){if(held)--*count;}
    };
    std::shared_ptr<std::atomic<size_t>> count=std::make_shared<std::atomic<size_t>>(0);
public:
    std::shared_ptr<void> Acquire(){
        auto slot=std::make_shared<Slot>(count);
        if(count->fetch_add(1)>=CanvasQueryQueue::MaxItems){--*count;return {};}
        slot->held=true;return slot;
    }
};
