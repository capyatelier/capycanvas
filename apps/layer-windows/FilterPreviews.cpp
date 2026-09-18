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
    hstring context,key;
    struct Geometry {int width,height;std::vector<hstring> ids;};
    std::map<uint64_t,Geometry> views;
    bool busy=false;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    ~FilterPreviewCache(){if(timer)timer.Stop();}
    void start(){
        if(!timer){timer=dispatcher.CreateTimer();timer.Interval(std::chrono::milliseconds(200));
            auto weak=weak_from_this();timer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->poll();});}
        if(!timer.IsRunning())timer.Start();
    }
    struct Pixels {hstring id;std::vector<uint8_t> bytes;};
    static fire_and_forget Convert(std::weak_ptr<FilterPreviewCache> weak,
        Microsoft::UI::Dispatching::DispatcherQueue queue,PreviewPacket packet,hstring acceptedKey,hstring acceptedContext,
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
        queue.TryEnqueue([weak,converted=std::move(converted),valid,acceptedKey,acceptedContext,width,height]{
        if(auto self=weak.lock()){
            self->busy=false;
            if(!valid||acceptedContext!=self->context||acceptedKey!=self->key)return;
            try {
                for(auto& tile:converted){
                    Imaging::WriteableBitmap bitmap(width,height);
                    auto buffer=bitmap.PixelBuffer();uint8_t* destination=nullptr;
                    check_hresult(buffer.as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&destination));
                    if(buffer.Capacity()!=tile.bytes.size())throw hresult_invalid_argument();
                    memcpy(destination,tile.bytes.data(),tile.bytes.size());bitmap.Invalidate();
                    self->images.insert_or_assign(tile.id,Tile{acceptedKey,bitmap});
                }
            }catch(hresult_error const& e){OutputDebugStringW(e.message().c_str());}
        }
        });
    }
    void receive(PreviewPacket packet,hstring const& requestedContext,bool hadVisible){
        busy=false;
        if(!packet){timer.Interval(std::chrono::milliseconds(1000));return;}
        if(requestedContext!=context)return;
        try {
            auto status=J::Parse(to_hstring(capy_preview_metadata(packet.get())));
            if(status.GetNamedValue(L"epoch").Stringify()!=context)return;
            auto next=str(status,L"key");
            if(key!=next){key=next;images.clear();}
            std::vector<hstring> retained;
            for(auto id:array(status,L"retained"))retained.push_back(id.GetString());
            std::erase_if(images,[&](auto const& row){return std::find(retained.begin(),retained.end(),row.first)==retained.end();});
            timer.Interval(std::chrono::milliseconds(std::max(8,int(num(status,L"wait_ms")))));
            if(!hadVisible&&views.empty())timer.Stop();
            auto atlas=object(status,L"atlas");
            if(atlas.Size()){
                auto filters=array(atlas,L"filters");std::vector<hstring> ids;
                for(auto id:filters)ids.push_back(id.GetString());
                if(ids.empty()||ids.size()>8)return;
                int width=int(num(atlas,L"width")),height=int(num(atlas,L"height"))/int(ids.size());
                busy=true;Convert(weak_from_this(),dispatcher,std::move(packet),key,context,width,height,std::move(ids));
            }else if(hadVisible&&views.empty())poll();
        }catch(hresult_error const& e){OutputDebugStringW(e.message().c_str());timer.Interval(std::chrono::milliseconds(1000));}
    }
    void poll(){
        if(busy)return;
        A ids,cached;int width=80,height=1;std::vector<hstring> unique;
        for(auto const& [view,geometry]:views){
            width=std::max(width,geometry.width);height=std::max(height,geometry.height);
            for(auto const& id:geometry.ids)if(std::find(unique.begin(),unique.end(),id)==unique.end()){
                unique.push_back(id);ids.Append(S(id));
            }
        }
        for(auto const& [id,tile]:images)cached.Append(S(id));
        A size;size.Append(N(width));size.Append(N(height));
        auto query=O({{L"filters",ids},{L"size",size},{L"cache",O({{L"key",S(key)},{L"rows",cached}})}});
        auto weak=weak_from_this();auto queue=dispatcher;auto requestedContext=context;bool hadVisible=ids.Size()>0;busy=true;
        if(!transport(CanvasQueryKind::Filters,to_string(query.Stringify()),[weak,queue,requestedContext,hadVisible](PreviewPacket packet){
            queue.TryEnqueue([weak,packet=std::move(packet),requestedContext,hadVisible]{
                if(auto self=weak.lock())self->receive(packet,requestedContext,hadVisible);
            });
        }))busy=false;
    }
    void refresh(uint64_t view,hstring const& epoch,int width,int height,std::vector<hstring> const& visible){
        // A queued native image conversion must not reach a replacement document.
        if(context!=epoch){context=epoch;key=L"";images.clear();}
        views.insert_or_assign(view,Geometry{width,height,visible});start();
    }
    void remove(uint64_t view){if(views.erase(view)){start();poll();}}

};
std::shared_ptr<FilterPreviewCache> CreateFilterPreviewCache(PreviewTransport transport){
    auto cache=std::make_shared<FilterPreviewCache>();cache->transport=std::move(transport);return cache;
}
void RefreshFilterPreviews(std::shared_ptr<FilterPreviewCache> const& cache,uint64_t view,hstring const& epoch,
    int width,int height,std::vector<hstring> const& visible){if(cache)cache->refresh(view,epoch,width,height,visible);}
void RemoveFilterPreviewView(std::shared_ptr<FilterPreviewCache> const& cache,uint64_t view){if(cache)cache->remove(view);}
ImageSource FilterPreviewSource(std::shared_ptr<FilterPreviewCache> const& cache,hstring const& epoch,hstring const& id){
    if(cache&&cache->context==epoch){auto found=cache->images.find(id);
        if(found!=cache->images.end()&&found->second.key==cache->key)return found->second.image;
    }return nullptr;
}
}
