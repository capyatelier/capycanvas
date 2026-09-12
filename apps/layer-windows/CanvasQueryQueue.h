#pragma once
#include "native/include/capy_windows.h"
#include <deque>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <utility>

enum class CanvasQueryKind { Filters, Thumbnails, LayerMenu, Workspace };
using PreviewPacket=std::shared_ptr<CapyPreview>;
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
    std::optional<CanvasQuery> Take(){
        if(items.empty())return {};
        // Menus contain no pixels and should not wait behind thumbnail batches.
        auto next=items.begin();
        for(auto it=items.begin();it!=items.end();++it)
            if(it->kind==CanvasQueryKind::LayerMenu){next=it;break;}
        auto item=std::move(*next);items.erase(next);return item;
    }
    void Clear(){items.clear();}
private:
    std::deque<CanvasQuery> items;
};
