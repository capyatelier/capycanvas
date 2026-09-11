#include "pch.h"
#include "UiControls.h"
#include "LayerThumbnails.h"
#include <winrt/Windows.Storage.Streams.h>
#include <robuffer.h>
#include <chrono>
#include <set>

namespace CapyUi {
namespace {
hstring slot(LayerThumbnail const& item){return item.layer+(item.mask?L":mask":L":content");}
hstring key(LayerThumbnail const& item){return slot(item)+L":"+item.target+L":"+item.revision;}
}
struct LayerThumbnailCache : std::enable_shared_from_this<LayerThumbnailCache> {
    PreviewTransport transport;
    Microsoft::UI::Dispatching::DispatcherQueue dispatcher{Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread()};
    struct Tile {hstring key;Imaging::WriteableBitmap image{nullptr};uint64_t used=0;};
    struct Pixels {LayerThumbnail item;std::vector<uint8_t> bytes;};
    struct Readback {LayerThumbnail item;size_t offset;};
    std::map<hstring,Tile> images;
    std::map<hstring,LayerThumbnail> pending;
    std::map<hstring,hstring> wanted;
    hstring epoch;
    uint64_t serial=0,clock=0;
    bool busy=false;
    std::chrono::steady_clock::time_point last{};
    static fire_and_forget Convert(std::weak_ptr<LayerThumbnailCache> weak,
        Microsoft::UI::Dispatching::DispatcherQueue queue,PreviewPacket packet,hstring generation,std::vector<Readback> readbacks){
        co_await resume_background();
        std::vector<Pixels> converted;
        try{
            size_t length=0;auto bytes=capy_preview_bytes(packet.get(),&length);
            if(!bytes||readbacks.size()>8||length>8*4096)throw hresult_invalid_argument();
            for(auto const& source:readbacks){
                if(source.offset>length||length-source.offset<4096)throw hresult_invalid_argument();
                Pixels tile{source.item,std::vector<uint8_t>(4096)};
                for(size_t p=0;p<4096;p+=4){
                    auto in=bytes+source.offset+p;auto out=tile.bytes.data()+p;unsigned a=in[3];
                    out[0]=uint8_t((unsigned(in[2])*a+127)/255);
                    out[1]=uint8_t((unsigned(in[1])*a+127)/255);
                    out[2]=uint8_t((unsigned(in[0])*a+127)/255);out[3]=uint8_t(a);
                }
                converted.emplace_back(std::move(tile));
            }
        }catch(...){converted.clear();}
        packet.reset();
        queue.TryEnqueue([weak,generation,converted=std::move(converted)]{
            if(auto self=weak.lock()){
                self->busy=false;if(self->epoch!=generation)return;
                try{
                    for(auto& tile:converted){
                        auto id=slot(tile.item),revision=key(tile.item);
                        auto wanted=self->wanted.find(id);
                        if(wanted==self->wanted.end()||wanted->second!=revision)continue;
                        Imaging::WriteableBitmap bitmap(32,32);uint8_t* destination=nullptr;
                        auto buffer=bitmap.PixelBuffer();
                        check_hresult(buffer.as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&destination));
                        if(buffer.Capacity()!=4096)throw hresult_invalid_argument();
                        memcpy(destination,tile.bytes.data(),4096);bitmap.Invalidate();
                        if(!self->images.contains(id)&&self->images.size()>=128){
                            auto oldest=std::min_element(self->images.begin(),self->images.end(),
                                [](auto const& a,auto const& b){return a.second.used<b.second.used;});
                            self->images.erase(oldest);
                        }
                        self->images.insert_or_assign(id,Tile{revision,bitmap,++self->clock});
                    }
                }catch(hresult_error const& e){OutputDebugStringW(e.message().c_str());}
            }
        });
    }
    void receive(PreviewPacket packet,hstring const& generation,std::map<hstring,LayerThumbnail> const& proposed){
        busy=false;if(!packet||epoch!=generation)return;
        try{
            auto status=J::Parse(to_hstring(capy_preview_metadata(packet.get())));
            if(str(status,L"epoch")!=epoch)return;
            for(auto value:array(status,L"accepted")){
                auto found=proposed.find(value.GetString());
                if(found!=proposed.end())pending.insert_or_assign(found->first,found->second);
            }
            std::vector<Readback> readbacks;
            for(auto value:array(status,L"images")){
                auto image=value.GetObject();auto found=pending.find(str(image,L"request"));
                if(found==pending.end())continue;
                auto item=found->second;pending.erase(found);
                if(num(image,L"width")!=32||num(image,L"height")!=32||num(image,L"length")!=4096)continue;
                double offset=num(image,L"offset",-1);
                if(offset<0||offset>7*4096||std::floor(offset)!=offset)continue;
                auto current=wanted.find(slot(item));
                if(current!=wanted.end()&&current->second==key(item))readbacks.push_back({item,size_t(offset)});
            }
            if(!readbacks.empty()){busy=true;Convert(weak_from_this(),dispatcher,std::move(packet),generation,std::move(readbacks));}
        }catch(hresult_error const& e){OutputDebugStringW(e.message().c_str());}
    }
    void refresh(hstring const& generation,std::vector<LayerThumbnail> const& visible){
        if(epoch!=generation){epoch=generation;pending.clear();wanted.clear();images.clear();}
        wanted.clear();for(auto const& item:visible)wanted.insert_or_assign(slot(item),key(item));
        auto now=std::chrono::steady_clock::now();
        if(busy||now-last<std::chrono::milliseconds(120))return;
        A requests;std::map<hstring,LayerThumbnail> proposed;
        std::set<hstring> outstanding;for(auto const& [id,item]:pending)outstanding.insert(slot(item));
        for(auto const& item:visible){
            if(pending.size()+proposed.size()>=8)break;
            auto id=slot(item);auto found=images.find(id);
            if(outstanding.contains(id)||(found!=images.end()&&found->second.key==key(item)))continue;
            auto request=to_hstring(++serial);A pair;pair.Append(S(request));pair.Append(S(item.target));requests.Append(pair);
            proposed.emplace(request,item);outstanding.insert(id);
        }
        if(pending.empty()&&proposed.empty())return;
        auto query=O({{L"epoch",S(epoch)},{L"requests",requests}});
        auto weak=weak_from_this();auto queue=dispatcher;busy=true;
        bool sent=transport(CanvasQueryKind::Thumbnails,to_string(query.Stringify()),
            [weak,queue,generation,proposed=std::move(proposed)](PreviewPacket packet){
                queue.TryEnqueue([weak,packet=std::move(packet),generation,proposed]{
                    if(auto self=weak.lock())self->receive(packet,generation,proposed);
                });
            });
        if(sent)last=now;else busy=false;
    }
};
std::shared_ptr<LayerThumbnailCache> CreateLayerThumbnailCache(PreviewTransport transport){
    auto cache=std::make_shared<LayerThumbnailCache>();cache->transport=std::move(transport);return cache;
}
void RefreshLayerThumbnails(std::shared_ptr<LayerThumbnailCache> const& cache,hstring const& epoch,std::vector<LayerThumbnail> const& visible){
    if(cache)cache->refresh(epoch,visible);
}
ImageSource LayerThumbnailSource(std::shared_ptr<LayerThumbnailCache> const& cache,hstring const& epoch,LayerThumbnail const& item){
    if(cache&&cache->epoch==epoch){
        auto found=cache->images.find(slot(item));
        if(found!=cache->images.end()&&found->second.key==key(item)){found->second.used=++cache->clock;return found->second.image;}
    }return nullptr;
}
}
