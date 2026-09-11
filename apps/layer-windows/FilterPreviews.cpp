#include "pch.h"
#include "UiControls.h"
#include "FilterPreviews.h"
#include <winrt/Windows.Storage.Streams.h>
#include <robuffer.h>
#include <chrono>
using namespace CapyUi;
namespace CapyUi {
struct FilterPreviewCache : std::enable_shared_from_this<FilterPreviewCache> {
    PreviewTransport transport;
    Microsoft::UI::Dispatching::DispatcherQueue dispatcher{Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread()};
    struct Tile {hstring key;Imaging::WriteableBitmap image{nullptr};};
    std::map<hstring,Tile> images;
    hstring context,revision,pendingKey;
    uint64_t serial=0,pending=0;
    bool busy=false;
    std::chrono::steady_clock::time_point last{};
    hstring key(hstring const& state,int width,int height) const {
        return state+L":"+revision+L":"+to_hstring(width)+L"x"+to_hstring(height);
    }
    struct Pixels {hstring id;std::vector<uint8_t> bytes;};
    static fire_and_forget Convert(std::weak_ptr<FilterPreviewCache> weak,
        Microsoft::UI::Dispatching::DispatcherQueue queue,PreviewPacket packet,hstring acceptedKey,
        int width,int height,std::vector<hstring> ids) {
        // No XAML objects or strong UI owners cross onto this worker.
        co_await resume_background();
        std::vector<Pixels> converted;bool valid=true;
        try {
            size_t length=0;auto bytes=capy_preview_bytes(packet.get(),&length);
            size_t tileSize=size_t(width)*height*4;
            if(!bytes||ids.empty()||ids.size()>8||width<1||width>512||height<1||height>128||length!=tileSize*ids.size())
                valid=false;
            if(valid)for(size_t i=0;i<ids.size();i++){
                Pixels tile{ids[i],std::vector<uint8_t>(tileSize)};
                for(size_t p=0;p<tileSize;p+=4){
                    auto in=bytes+i*tileSize+p;auto out=tile.bytes.data()+p;unsigned a=in[3];
                    // Shared readback is straight RGBA; WriteableBitmap expects
                    // premultiplied BGRA in its native pixel buffer.
                    out[0]=uint8_t((unsigned(in[2])*a+127)/255);
                    out[1]=uint8_t((unsigned(in[1])*a+127)/255);
                    out[2]=uint8_t((unsigned(in[0])*a+127)/255);out[3]=uint8_t(a);
                }
                converted.emplace_back(std::move(tile));
            }
        }catch(...){valid=false;}
        packet.reset();
        queue.TryEnqueue([weak,converted=std::move(converted),valid,acceptedKey,width,height]{
        if(auto self=weak.lock()){
            self->busy=false;
            if(!valid||!acceptedKey.starts_with(self->context+L":"+self->revision+L":"))return;
            try {
                for(auto& tile:converted){
                    Imaging::WriteableBitmap bitmap(width,height);
                    auto buffer=bitmap.PixelBuffer();uint8_t* destination=nullptr;
                    check_hresult(buffer.as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&destination));
                    if(buffer.Capacity()!=tile.bytes.size())throw hresult_invalid_argument();
                    memcpy(destination,tile.bytes.data(),tile.bytes.size());bitmap.Invalidate();
                    // Retain at most 64 catalog rows (16 MiB at maximum size).
                    if(!self->images.contains(tile.id)&&self->images.size()>=64)self->images.erase(self->images.begin());
                    self->images.insert_or_assign(tile.id,Tile{acceptedKey,bitmap});
                }
            }catch(hresult_error const& e){OutputDebugStringW(e.message().c_str());}
        }
        });
    }
    void receive(PreviewPacket packet,uint64_t request,hstring const& requestedKey){
        busy=false;if(!packet)return;
        try {
            auto status=J::Parse(to_hstring(capy_preview_metadata(packet.get())));
            auto next=str(status,L"revision");
            if(revision!=next){revision=next;images.clear();}
            if(flag(status,L"accepted")&&requestedKey.starts_with(context+L":"+revision+L":")){pending=request;pendingKey=requestedKey;}
            auto atlas=object(status,L"atlas");if(!atlas.Size())return;
            auto returned=str(atlas,L"request");
            if(!pending||returned!=to_hstring(pending))return;
            pending=0;
            auto acceptedKey=std::exchange(pendingKey,L"");
            if(!acceptedKey.starts_with(context+L":"+revision+L":"))return;
            auto filters=array(atlas,L"filters");std::vector<hstring> ids;
            for(auto id:filters)ids.push_back(id.GetString());
            if(ids.empty()||ids.size()>8)return;
            int width=int(num(atlas,L"width")),height=int(num(atlas,L"height"))/int(ids.size());
            busy=true;Convert(weak_from_this(),dispatcher,std::move(packet),acceptedKey,width,height,std::move(ids));
        }catch(hresult_error const& e){OutputDebugStringW(e.message().c_str());}
    }
    void refresh(hstring const& state,int width,int height,std::vector<hstring> const& visible){
        // A replacement can destroy the old renderer and its pending readback.
        // Abandon that request; any late atlas is discarded by its request id.
        if(context!=state){context=state;revision=L"";pending=0;pendingKey=L"";images.clear();}
        auto now=std::chrono::steady_clock::now();
        if(busy||now-last<std::chrono::milliseconds(200))return;
        auto requestedKey=key(state,width,height);A missing;
        if(!pending)for(auto const& id:visible){
            auto item=images.find(id);
            if(item==images.end()||item->second.key!=requestedKey){missing.Append(S(id));if(missing.Size()==8)break;}
        }
        if(!pending&&!missing.Size()&&!revision.empty())return;
        auto request=++serial;
        A size;size.Append(N(width));size.Append(N(height));
        auto query=O({{L"request",S(to_hstring(request))},{L"revision",S(revision)},{L"filters",missing},{L"size",size}});
        auto weak=weak_from_this();auto queue=dispatcher;busy=true;
        bool sent=transport(to_string(query.Stringify()),[weak,queue,request,requestedKey](PreviewPacket packet){
            queue.TryEnqueue([weak,packet=std::move(packet),request,requestedKey]{
                if(auto self=weak.lock())self->receive(packet,request,requestedKey);
            });
        });
        if(sent)last=now;else busy=false;
    }
};
std::shared_ptr<FilterPreviewCache> CreateFilterPreviewCache(PreviewTransport transport){
    auto cache=std::make_shared<FilterPreviewCache>();cache->transport=std::move(transport);return cache;
}
void RefreshFilterPreviews(std::shared_ptr<FilterPreviewCache> const& cache,hstring const& context,
    int width,int height,std::vector<hstring> const& visible){if(cache)cache->refresh(context,width,height,visible);}
ImageSource FilterPreviewSource(std::shared_ptr<FilterPreviewCache> const& cache,hstring const& context,
    int width,int height,hstring const& id){
    if(cache){auto found=cache->images.find(id);
        if(found!=cache->images.end()&&found->second.key==cache->key(context,width,height))return found->second.image;
    }return nullptr;
}
}
