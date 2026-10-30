#include "pch.h"
#include "UiControls.h"
#include "FilterPreviews.h"
#include "BitmapUpload.h"
#include <chrono>
using namespace CapyUi;
namespace CapyUi {
struct FilterPreviewCache : std::enable_shared_from_this<FilterPreviewCache> {
    PreviewTransport transport;
    Microsoft::UI::Dispatching::DispatcherQueue dispatcher{Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread()};
    struct Tile {hstring key;std::shared_ptr<BitmapUpload> upload;};
    std::map<hstring,Tile> images;
    hstring context,key;
    std::map<uint64_t,std::vector<hstring>> views;
    bool busy=false;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    ~FilterPreviewCache(){if(timer)timer.Stop();for(auto& [id,tile]:images)tile.upload->Close();}
    void start(){
        if(!timer){timer=dispatcher.CreateTimer();timer.Interval(std::chrono::milliseconds(200));
            auto weak=weak_from_this();timer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->poll();});}
        if(!timer.IsRunning())timer.Start();
    }
    bool visible(hstring const& id)const{
        for(auto const& [view,ids]:views)if(std::find(ids.begin(),ids.end(),id)!=ids.end())return true;
        return false;
    }
    void upload(PreviewPacket packet,int width,int height,std::vector<hstring> const& ids){
        if(ids.empty()||ids.size()>8||width<1||width>512||height<1||height>128)return;
        auto left=std::make_shared<size_t>(ids.size());auto weak=weak_from_this();
        auto acceptedKey=key,acceptedContext=context;size_t tileSize=size_t(width)*height*4;busy=true;
        for(size_t i=0;i<ids.size();++i){
            auto id=ids[i];auto [found,inserted]=images.try_emplace(id);
            if(inserted)found->second.upload=BitmapUpload::Create();found->second.key=L"";
            auto bitmap=found->second.upload;std::weak_ptr<BitmapUpload> weakBitmap=bitmap;
            bitmap->Submit({BitmapUpload::ReadPacket(packet),i*tileSize,tileSize*ids.size(),width,height,true,true,
                [weak,id,weakBitmap,acceptedKey,acceptedContext]{
                    auto self=weak.lock();if(!self||self->key!=acceptedKey||self->context!=acceptedContext)return false;
                    auto found=self->images.find(id);return found!=self->images.end()&&found->second.upload==weakBitmap.lock();
                },
                [weak,id,weakBitmap,acceptedKey,left](bool accepted){
                    if(auto self=weak.lock()){
                        if(!--*left)self->busy=false;
                        if(accepted){auto found=self->images.find(id);if(found!=self->images.end()&&found->second.upload==weakBitmap.lock())found->second.key=acceptedKey;}
                    }
                }});
        }
    }
    void receive(PreviewPacket packet,hstring const& requestedContext,bool hadVisible){
        busy=false;
        if(!packet){timer.Interval(std::chrono::milliseconds(1000));return;}
        if(requestedContext!=context)return;
        try {
            auto status=J::Parse(to_hstring(NativeString(capy_preview_metadata(packet.get())).get()));
            if(status.GetNamedValue(L"epoch").Stringify()!=context)return;
            auto next=str(status,L"key");
            if(key!=next){key=next;for(auto& [id,tile]:images)tile.key=L"";}
            std::vector<hstring> retained;
            for(auto id:array(status,L"retained"))retained.push_back(id.GetString());
            std::erase_if(images,[&](auto const& row){
                bool remove=std::find(retained.begin(),retained.end(),row.first)==retained.end()&&!visible(row.first);
                if(remove)row.second.upload->Close();return remove;
            });
            timer.Interval(std::chrono::milliseconds(std::max(8,int(num(status,L"wait_ms")))));
            if(!hadVisible&&views.empty())timer.Stop();
            auto atlas=object(status,L"atlas");
            if(atlas.Size()){
                auto filters=array(atlas,L"filters");std::vector<hstring> ids;
                for(auto id:filters)ids.push_back(id.GetString());
                if(ids.empty()||ids.size()>8)return;
                int width=int(num(atlas,L"width")),height=int(num(atlas,L"height"))/int(ids.size());
                upload(std::move(packet),width,height,ids);
            }else if(hadVisible&&views.empty())poll();
        }catch(hresult_error const& e){OutputDebugStringW(e.message().c_str());timer.Interval(std::chrono::milliseconds(1000));}
    }
    void poll(){
        if(busy)return;
        A ids,cached;std::vector<hstring> unique;
        for(auto const& [view,visible]:views){
            for(auto const& id:visible)if(std::find(unique.begin(),unique.end(),id)==unique.end()){
                unique.push_back(id);ids.Append(S(id));
            }
        }
        for(auto const& [id,tile]:images)if(tile.key==key&&tile.upload->Image())cached.Append(S(id));
        auto query=O({{L"filters",ids},{L"cache",O({{L"key",S(key)},{L"rows",cached}})}});
        auto weak=weak_from_this();auto queue=dispatcher;auto requestedContext=context;bool hadVisible=ids.Size()>0;busy=true;
        if(!transport(CanvasQueryKind::Filters,to_string(query.Stringify()),[weak,queue,requestedContext,hadVisible](PreviewPacket packet){
            queue.TryEnqueue([weak,packet=std::move(packet),requestedContext,hadVisible]{
                if(auto self=weak.lock())self->receive(packet,requestedContext,hadVisible);
            });
        }))busy=false;
    }
    void publish(hstring const& epoch){
        if(context==epoch)return;
        context=epoch;key=L"";
        for(auto& [id,tile]:images)tile.key=L"";
    }
    void refresh(uint64_t view,hstring const& epoch,std::vector<hstring> const& visible){
        publish(epoch);
        views.insert_or_assign(view,visible);
        start();
    }
    void remove(uint64_t view){if(views.erase(view)){start();poll();}}

};
std::shared_ptr<FilterPreviewCache> CreateFilterPreviewCache(PreviewTransport transport){
    auto cache=std::make_shared<FilterPreviewCache>();cache->transport=std::move(transport);return cache;
}
void RefreshFilterPreviewContext(std::shared_ptr<FilterPreviewCache> const& cache,hstring const& epoch){
    if(cache)cache->publish(epoch);
}
void RefreshFilterPreviews(std::shared_ptr<FilterPreviewCache> const& cache,uint64_t view,hstring const& epoch,
    std::vector<hstring> const& visible){if(cache)cache->refresh(view,epoch,visible);}
void RemoveFilterPreviewView(std::shared_ptr<FilterPreviewCache> const& cache,uint64_t view){if(cache)cache->remove(view);}
ImageSource FilterPreviewSource(std::shared_ptr<FilterPreviewCache> const& cache,hstring const& epoch,hstring const& id,bool* current){
    if(current)*current=false;
    if(cache&&cache->context==epoch){auto found=cache->images.find(id);
        if(found!=cache->images.end()){
            auto source=found->second.upload->Image();if(current)*current=source&&!cache->key.empty()&&found->second.key==cache->key;
            return source;
        }
    }return nullptr;
}
}
