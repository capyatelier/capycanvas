#include "pch.h"
#include "ControlContext.h"
#include "BitmapUpload.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include "../LayerThumbnails.cpp"
#include "../FilterPreviews.cpp"
#include "../ScopesView.cpp"
#include "../FilterView.cpp"
#include "LayersView.h"
#include "ColorWheel.h"
#include "ColorEditor.h"
#include <winrt/Windows.Storage.Streams.h>
#include <robuffer.h>
#include <atomic>
#include <fstream>
#include <thread>
#include <set>
using namespace CapyUi;
namespace {
int resultCode=0;
struct Frame {
    std::shared_ptr<std::vector<uint8_t>> bytes;
    std::shared_ptr<std::atomic<unsigned>> reads=std::make_shared<std::atomic<unsigned>>(0);
    int width=3,height=2;bool rgba=true;
};
struct Frames {
    Frame rgba,bgra,newer,largeRed,largeGreen,paperWhite,paperBlue;
    PreviewPacket paperBluePacket;
    std::shared_ptr<std::vector<uint8_t>> paperBlueBgra;
    std::shared_ptr<std::vector<uint8_t>> blank=std::make_shared<std::vector<uint8_t>>(24);
    std::shared_ptr<std::vector<uint8_t>> expected=std::make_shared<std::vector<uint8_t>>(std::initializer_list<uint8_t>{0,0,255,255,0,128,0,128,0,0,0,0,255,0,0,255,10,20,30,64,23,22,21,255});
    Frames(){
        rgba.bytes=std::make_shared<std::vector<uint8_t>>(std::initializer_list<uint8_t>{
            255,0,0,255,0,255,0,128,50,100,200,0,0,0,255,255,120,80,40,64,21,22,23,255});
        bgra.bytes=std::make_shared<std::vector<uint8_t>>(*expected);bgra.rgba=false;
        newer.bytes=std::make_shared<std::vector<uint8_t>>(std::initializer_list<uint8_t>{11,22,33,255,11,22,33,255,11,22,33,255,11,22,33,255,11,22,33,255,11,22,33,255});newer.rgba=false;
        paperWhite.width=paperWhite.height=paperBlue.width=paperBlue.height=32;
        paperWhite.bytes=std::make_shared<std::vector<uint8_t>>(32*32*4,255);
        paperBlue.bytes=std::make_shared<std::vector<uint8_t>>(32*32*4);
        paperBlueBgra=std::make_shared<std::vector<uint8_t>>(32*32*4);
        for(size_t p=0;p<paperBlue.bytes->size();p+=4){
            (*paperBlue.bytes)[p]=220;(*paperBlue.bytes)[p+1]=236;(*paperBlue.bytes)[p+2]=251;(*paperBlue.bytes)[p+3]=255;
            (*paperBlueBgra)[p]=251;(*paperBlueBgra)[p+1]=236;(*paperBlueBgra)[p+2]=220;(*paperBlueBgra)[p+3]=255;
        }
        uint8_t blueWebp[]={82,73,70,70,30,0,0,0,87,69,66,80,86,80,56,76,17,0,0,0,47,31,192,7,0,7,80,246,114,119,191,255,129,136,232,127,0,0};
        uint32_t width=0,height=0;paperBluePacket=PreviewPacket(capy_decode_webp(blueWebp,sizeof(blueWebp),&width,&height),capy_preview_free);
        if(!paperBluePacket||width!=32||height!=32)throw hresult_error(E_FAIL,L"Blue thumbnail packet did not decode");
        for(auto [frame,green]:{std::pair{&largeRed,false},std::pair{&largeGreen,true}}){
            frame->width=frame->height=1024;frame->rgba=false;
            frame->bytes=std::make_shared<std::vector<uint8_t>>(1024*1024*4);
            for(size_t p=0;p<frame->bytes->size();p+=4){
                (*frame->bytes)[p+1]=green?255:0;(*frame->bytes)[p+2]=green?0:255;(*frame->bytes)[p+3]=255;
            }
        }
    }
};
struct Outcome {
    winrt::handle event{CreateEventW(nullptr,true,false,nullptr)};
    std::map<int,bool> accepted;size_t expected=1;std::atomic<bool> callbackFault=false;bool duplicate=false,precondition=false;
};
struct Fixture:std::enable_shared_from_this<Fixture> {
    std::shared_ptr<Frames> frames;
    Window window;Canvas surface;Image image;
    std::shared_ptr<BitmapUpload> upload;
    Microsoft::UI::Dispatching::DispatcherQueue queue{Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread()};
    winrt::apartment_context apartment;
    std::filesystem::path output;DWORD ui=GetCurrentThreadId();
    uint64_t epoch=0;unsigned checks=0;hstring phase;
    std::vector<BitmapUpload::Bitmap> retained;std::set<uintptr_t> identities;size_t poolSize=0;
    static void require(bool value,wchar_t const* message){if(!value)throw hresult_error(E_FAIL,message);}
    void observe(){
        poolSize=0;
        for(auto const& bitmap:upload->buffers)if(bitmap){
            ++poolSize;auto identity=reinterpret_cast<uintptr_t>(get_abi(bitmap));
            if(identities.insert(identity).second)retained.push_back(bitmap);
        }
    }
    void finish(std::shared_ptr<Outcome> const& result,int id,bool accepted){
        if(GetCurrentThreadId()!=ui){result->callbackFault=true;SetEvent(result->event.get());return;}
        if(result->accepted.contains(id)){result->duplicate=true;SetEvent(result->event.get());return;}
        result->accepted.emplace(id,accepted);observe();
        try{if(accepted)image.Source(upload->Image());}catch(...){result->callbackFault=true;}
        if(result->callbackFault||result->accepted.size()==result->expected)SetEvent(result->event.get());
    }
    BitmapUpload::Request request(Frame frame,std::shared_ptr<Outcome> result,int id,
        std::shared_ptr<winrt::handle> gate={},bool failRead=false){
        auto weak=weak_from_this();auto captured=epoch;auto thread=ui;
        return {[frame,thread,gate,failRead]{
                require(GetCurrentThreadId()!=thread,L"Pixels read on UI thread");
                ++*frame.reads;
                if(gate)require(WaitForSingleObject(gate->get(),10000)==WAIT_OBJECT_0,L"Worker fixture gate timed out");
                if(failRead)throw hresult_error(E_FAIL,L"Fixture byte reader failed");
                return std::span<uint8_t const>(*frame.bytes);
            },0,frame.bytes->size(),frame.width,frame.height,frame.rgba,true,
            [weak,captured]{auto self=weak.lock();return self&&self->epoch==captured;},
            [weak,result,id](bool accepted){if(auto self=weak.lock())self->finish(result,id,accepted);}
        };
    }
    winrt::Windows::Foundation::IAsyncAction wait(std::shared_ptr<Outcome> result){
        require(co_await resume_on_signal(result->event.get(),std::chrono::seconds(10)),L"Upload callback timed out");
        co_await apartment;
        require(!result->precondition,L"Async-cancellation prerequisite did not interleave");
        require(!result->callbackFault&& !result->duplicate,L"Completion thread, source binding or exactly-once check failed");
    }
    winrt::Windows::Foundation::IAsyncAction rendered(FrameworkElement target=nullptr){
        if(!target)target=image;
        auto root=target.XamlRoot();
        require(target.IsLoaded()&&root&&root.IsHostVisible(),L"Capture image is not loaded and connected before rendering wait");
        winrt::handle event{CreateEventW(nullptr,true,false,nullptr)};
        auto frame=CompositionTarget::Rendering(auto_revoke,[target,signal=event.get()](auto&&,auto&&){
            auto root=target.XamlRoot();
            if(root&&root.IsHostVisible()&&target.ActualWidth()>0&&target.ActualHeight()>0)SetEvent(signal);
        });
        bool signaled=co_await resume_on_signal(event.get(),std::chrono::seconds(10));
        co_await apartment;frame.revoke();
        require(signaled,L"Loaded visible image did not reach CompositionTarget.Rendering within 10 seconds");
    }
    winrt::Windows::Foundation::IAsyncAction pixels(std::shared_ptr<std::vector<uint8_t>> expected,int width=3,int height=2,bool current=true,FrameworkElement target=nullptr){
        if(!target)target=image;
        require(!current||image.Source()==upload->Image(),L"Image not bound to completed source");
        auto scale=target.XamlRoot().RasterizationScale();
        target.Width(width/scale);target.Height(height/scale);
        co_await rendered(target);
        Imaging::RenderTargetBitmap shot;co_await shot.RenderAsync(target);
        auto buffer=co_await shot.GetPixelsAsync();auto uiThread=ui;
        auto renderedWidth=shot.PixelWidth(),renderedHeight=shot.PixelHeight();
        auto sample=output.parent_path()/(L"pixels-"+std::wstring(phase)+L"-"+std::to_wstring(checks));
        co_await resume_background();bool valid=false;
        try{
            require(GetCurrentThreadId()!=uiThread,L"Readback comparison ran on UI");
            uint8_t* raw=nullptr;check_hresult(buffer.as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&raw));
            valid=buffer.Length()==expected->size()&&std::equal(expected->begin(),expected->end(),raw);
            if(!valid){
                std::ofstream actual(sample.wstring()+L"-actual.bgra",std::ios::binary);
                actual.write(reinterpret_cast<char const*>(raw),buffer.Length());
                std::ofstream wanted(sample.wstring()+L"-expected.bgra",std::ios::binary);
                wanted.write(reinterpret_cast<char const*>(expected->data()),expected->size());
                std::ofstream metadata(sample.wstring()+L"-geometry.json");
                metadata<<"{\"rendered_width\":"<<renderedWidth<<",\"rendered_height\":"<<renderedHeight
                    <<",\"scale\":"<<scale<<",\"actual_bytes\":"<<buffer.Length()<<",\"expected_bytes\":"<<expected->size()<<"}\n";
            }
        }catch(...){}
        co_await apartment;
        require(valid,L"Rendered BGRA/alpha/orientation differs exactly");
    }
    winrt::Windows::Foundation::IAsyncAction submit(Frame frame,bool failRead=false){
        auto result=std::make_shared<Outcome>();upload->Submit(request(frame,result,1,{},failRead));
        co_await wait(result);require(result->accepted.at(1)!=failRead,L"Byte-read result differs");
    }
    winrt::Windows::Foundation::IAsyncAction encoding(){
        co_await submit(frames->rgba);co_await pixels(frames->expected);++checks;
        co_await submit(frames->bgra);co_await pixels(frames->expected);++checks;
        co_await submit(frames->rgba);co_await pixels(frames->expected);
        require(identities.size()==2&&poolSize==2,L"Same-context frames allocated beyond two software slots");++checks;
        co_await submit(frames->rgba,true);co_await pixels(frames->expected);
        require(poolSize==1,L"Failed fill retained its back buffer");++checks;
        co_await submit(frames->rgba);co_await pixels(frames->expected);
        require(identities.size()==3&&poolSize==2,L"Fill recovery reused failed buffer or allocated unexpected slots");++checks;
    }
    winrt::Windows::Foundation::IAsyncAction coalescing(){
        auto result=std::make_shared<Outcome>();result->expected=4;
        auto gate=std::make_shared<winrt::handle>(CreateEventW(nullptr,true,false,nullptr));
        auto weak=weak_from_this();auto active=frames->rgba,discardedFrame=frames->rgba,displaced=frames->rgba,latest=frames->newer;
        for(auto frame:{&active,&discardedFrame,&displaced,&latest})frame->reads=std::make_shared<std::atomic<unsigned>>(0);
        upload->Submit(request(active,result,1,gate));
        auto discarded=request(discardedFrame,result,2);
        discarded.complete=[weak,result,latest](bool accepted){
            if(auto self=weak.lock()){
                self->finish(result,2,accepted);
                self->upload->Submit(self->request(latest,result,4));
            }
        };
        upload->Submit(std::move(discarded));upload->Submit(request(displaced,result,3));
        SetEvent(gate->get());co_await wait(result);
        require(result->accepted.at(1)&&!result->accepted.at(2)&&!result->accepted.at(3)&&result->accepted.at(4),
            L"Reentrant completion lost newest request");
        require(active.reads->load()==1&&discardedFrame.reads->load()==0&&displaced.reads->load()==0&&latest.reads->load()==1,
            L"Superseded request was read or active/newest was not read exactly once");
        co_await pixels(latest.bytes);++checks;
    }
    winrt::Windows::Foundation::IAsyncAction invalidation(){
        auto result=std::make_shared<Outcome>();result->expected=2;
        auto gate=std::make_shared<winrt::handle>(CreateEventW(nullptr,true,false,nullptr));
        upload->Submit(request(frames->rgba,result,1,gate));
        ++epoch;upload->Clear();image.Source(nullptr);
        upload->Submit(request(frames->newer,result,2));SetEvent(gate->get());
        co_await wait(result);
        require(!result->accepted.at(1)&&result->accepted.at(2),L"Context cancellation revived old frame");
        co_await pixels(frames->newer.bytes);++checks;
    }
    winrt::Windows::Foundation::IAsyncAction exceptions(){
        auto result=std::make_shared<Outcome>();result->expected=2;
        auto rejected=request(frames->rgba,result,1);
        rejected.current=[]()->bool{throw hresult_error(E_FAIL,L"Fixture predicate failed");};
        auto weak=weak_from_this();
        rejected.complete=[weak,result](bool accepted){
            if(auto self=weak.lock()){
                self->finish(result,1,accepted);
                self->upload->Submit(self->request(self->frames->bgra,result,2));
            }
            throw hresult_error(E_FAIL,L"Fixture completion failed");
        };
        upload->Submit(std::move(rejected));co_await wait(result);
        require(!result->accepted.at(1)&&result->accepted.at(2),L"Throwing callback stranded next upload");
        co_await pixels(frames->expected);++checks;
    }
    winrt::Windows::Foundation::IAsyncAction predicateReentry(){
        co_await submit(frames->rgba);auto result=std::make_shared<Outcome>();
        auto item=request(frames->newer,result,1);auto weak=weak_from_this();auto calls=std::make_shared<unsigned>(0);
        item.current=[weak,calls]{
            auto self=weak.lock();if(!self)return false;
            if(++*calls==2){++self->epoch;self->upload->Clear();}
            return true;
        };
        upload->Submit(std::move(item));co_await wait(result);
        require(!result->accepted.at(1),L"Predicate context change accepted stale upload");
        co_await pixels(frames->blank,3,2,false);++checks;
        co_await submit(frames->bgra);co_await pixels(frames->expected);
    }
    winrt::Windows::Foundation::IAsyncAction awaitedCancellation(){
        ++epoch;upload->Clear();image.Source(nullptr);image.Width(16);image.Height(16);
        co_await submit(frames->largeRed);
        auto result=std::make_shared<Outcome>();auto item=request(frames->largeGreen,result,1);
        auto weak=weak_from_this();auto captured=epoch;auto calls=std::make_shared<unsigned>(0);
        auto cleared=std::make_shared<bool>(false);
        item.current=[weak,captured,calls,cleared,result]{
            auto self=weak.lock();if(!self||self->epoch!=captured)return false;
            if(++*calls==2)require(self->queue.TryEnqueue([weak,cleared,result]{
                if(auto owner=weak.lock()){
                    if(!result->accepted.empty()){result->precondition=true;SetEvent(result->event.get());return;}
                    ++owner->epoch;owner->upload->Clear();owner->image.Source(nullptr);*cleared=true;
                }
            }),L"Cannot schedule context invalidation");
            return true;
        };
        upload->Submit(std::move(item));co_await wait(result);
        require(*cleared&&!result->accepted.at(1)&&!image.Source(),L"Awaited source revived after invalidation");
        image.Width(3);image.Height(2);co_await submit(frames->bgra);co_await pixels(frames->expected);++checks;
    }
    winrt::Windows::Foundation::IAsyncAction thumbnailEpoch(){
        auto calls=std::make_shared<unsigned>(0);
        auto cache=CreateLayerThumbnailCache([calls](CanvasQueryKind kind,std::string,PreviewReply){require(kind==CanvasQueryKind::Thumbnails,L"Unexpected cache transport kind");++*calls;return false;});
        cache->epoch=L"old";cache->busy=true;
        auto gate=std::make_shared<winrt::handle>(CreateEventW(nullptr,true,false,nullptr));
        auto entered=std::make_shared<winrt::handle>(CreateEventW(nullptr,true,false,nullptr));
        struct Release {std::shared_ptr<winrt::handle> gate;~Release(){SetEvent(gate->get());}} release{gate};
        auto result=std::make_shared<Outcome>();auto item=request(frames->rgba,result,1,gate);auto read=std::move(item.read);
        item.read=[read=std::move(read),entered]{SetEvent(entered->get());return read();};
        std::weak_ptr<LayerThumbnailCache> weakCache=cache;auto complete=std::move(item.complete);
        item.complete=[weakCache,complete=std::move(complete)](bool accepted){if(auto cache=weakCache.lock())cache->busy=false;complete(accepted);};
        auto bitmap=BitmapUpload::Create();cache->images.emplace(L"old:content",LayerThumbnailCache::Tile{L"old",L"old",bitmap,1});
        bitmap->Submit(std::move(item));bitmap.reset();
        require(co_await resume_on_signal(entered->get(),std::chrono::seconds(10)),L"Thumbnail reader did not enter background fill");
        co_await apartment;
        RefreshLayerThumbnails(cache,this,L"new",{{L"new",L"new",L"revision",false}});
        SetEvent(gate->get());
        require(*calls==1&&!cache->busy,L"Discarding old document upload stranded new-document thumbnail request");
        co_await wait(result);require(!result->accepted.at(1),L"Discarded document thumbnail was accepted");++checks;
    }
    void thumbnailOwners(){
        auto queries=std::make_shared<std::vector<J>>();
        auto cache=CreateLayerThumbnailCache([queries](CanvasQueryKind kind,std::string json,PreviewReply){require(kind==CanvasQueryKind::Thumbnails,L"Unexpected cache transport kind");queries->push_back(J::Parse(to_hstring(json)));return false;});
        int list,face,card;LayerThumbnail a{L"a",L"paint-a",L"1",false},b{L"b",L"paint-b",L"1",false};
        RefreshLayerThumbnails(cache,&list,L"document",{a});
        RefreshLayerThumbnails(cache,&face,L"document",{b});
        require(array(queries->back(),L"requests").Size()==2,L"Second thumbnail owner displaced the list request");
        cache->pending.emplace(L"waiting-a",a);cache->busy=true;
        RefreshLayerThumbnails(cache,&card,L"document",{a});
        ForgetLayerThumbnails(cache,&list);
        require(cache->targets.Items().size()==2&&cache->targets.Items().at(slot(a))==a,L"Retiring one list invalidated another owner's pending thumbnail");
        a.revision=L"2";RefreshLayerThumbnails(cache,&card,L"document",{a});
        require(key(cache->targets.Items().at(slot(a)))!=key(cache->pending.at(L"waiting-a")),L"Changed thumbnail revision accepted stale pending pixels");
        ForgetLayerThumbnails(cache,&card);
        require(cache->targets.Items().size()==1&&cache->targets.Items().at(slot(b))==b,L"Retiring a card discarded the folded face thumbnail");
        RefreshLayerThumbnails(cache,&face,L"document",{});
        require(cache->targets.Items().empty(),L"Empty owner retained thumbnail demand");++checks;
    }
    winrt::Windows::Foundation::IAsyncAction paperRecolor(){
        ImageBrush brush;brush.Stretch(Stretch::Fill);
        Shapes::Rectangle row;row.Fill(brush);row.UseLayoutRounding(false);
        row.Width(32/image.XamlRoot().RasterizationScale());row.Height(row.Width());
        winrt::handle loaded{CreateEventW(nullptr,true,false,nullptr)};
        auto loading=row.Loaded(auto_revoke,[signal=loaded.get()](auto&&,auto&&){SetEvent(signal);});
        surface.Children().Append(row);
        require(co_await resume_on_signal(loaded.get(),std::chrono::seconds(10)),L"Paper thumbnail row did not load");
        co_await apartment;loading.revoke();
        co_await submit(frames->paperWhite);
        auto source=upload->Image();require(source!=nullptr,L"White Paper upload has no source");
        brush.ImageSource(source);
        co_await pixels(frames->paperWhite.bytes,32,32,false,row);
        co_await submit(frames->paperBlue);
        require(upload->Image()==source&&brush.ImageSource()==source,L"Paper recolor replaced its retained ImageBrush source");
        require(frames->paperBlue.reads->load()==1,L"Blue Paper bytes were not read exactly once");
        co_await pixels(frames->paperBlueBgra,32,32,false,row);
        co_await submit(frames->paperWhite);
        auto cache=CreateLayerThumbnailCache([](CanvasQueryKind,std::string,PreviewReply){return false;});
        LayerThumbnail item{L"layer",L"paint",L"1",false};
        RefreshLayerThumbnails(cache,this,L"document",{item});
        cache->images.emplace(slot(item),LayerThumbnailCache::Tile{key(item),item.target,upload,1});
        bool current=false;
        require(LayerThumbnailSource(cache,L"document",item,&current)==source&&current,L"Completed thumbnail revision was not ready");
        item.revision=L"2";RefreshLayerThumbnails(cache,this,L"document",{item});
        require(LayerThumbnailSource(cache,L"document",item,&current)==source&&!current,L"Pending revision discarded the retained thumbnail or reported ready");
        cache->upload(frames->paperBluePacket,L"document",{{item,0}});
        require(LayerThumbnailSource(cache,L"document",item,&current)==source&&!current&&brush.ImageSource()==source,L"Pending upload cleared the retained thumbnail or reported ready");
        auto result=std::make_shared<Outcome>();auto complete=std::move(upload->active->request.complete);auto weak=weak_from_this();
        upload->active->request.complete=[weak,result,complete=std::move(complete)](bool accepted){complete(accepted);if(auto self=weak.lock())self->finish(result,1,accepted);};
        co_await wait(result);
        require(result->accepted.at(1)&&LayerThumbnailSource(cache,L"document",item,&current)==source&&current,L"New thumbnail revision did not publish on its retained source");
        co_await pixels(frames->paperBlueBgra,32,32,false,row);
        auto other=item;other.target=L"other";
        require(!LayerThumbnailSource(cache,L"document",other)&&!LayerThumbnailSource(cache,L"other",item),L"Thumbnail source crossed a target or document identity");
        surface.Children().RemoveAtEnd();++checks;
    }
    ControlContext controlContext;A retentionResults;
    std::shared_ptr<WorkspaceData> workspace(){
        auto data=std::make_shared<WorkspaceData>();controlContext.apply(data);
        data->catalog.Insert(L"panel_metrics",O({{L"layer_swipe_action_width",N(64)},{L"control_height",N(30)},{L"compound_spacing",N(2)}}));
        data->state=O({{L"theme",S(L"dark")},{L"document_file",O({{L"epoch",N(1)}})}});
        data->query=[](CanvasQueryKind,std::string,PreviewReply){return false;};
        return data;
    }
    winrt::Windows::Foundation::IAsyncAction filterRetention(bool contextChange){
        co_await submit(frames->paperWhite);auto source=upload->Image();
        auto cache=CreateFilterPreviewCache([](CanvasQueryKind,std::string,PreviewReply){return false;});
        cache->context=L"old";cache->key=L"first";cache->images.emplace(L"filter",FilterPreviewCache::Tile{cache->key,upload});
        bool current=false;require(FilterPreviewSource(cache,L"old",L"filter",&current)==source&&current,L"Accepted filter preview was not current");
        if(contextChange)RefreshFilterPreviewContext(cache,L"new");else cache->key=L"second";
        require(FilterPreviewSource(cache,cache->context,L"filter",&current)==source&&!current,
            L"Filter key/context change discarded retained preview pixels");
        co_await pixels(frames->paperWhite.bytes,32,32);++checks;
    }
    winrt::Windows::Foundation::IAsyncAction filterUpload(bool stale){
        co_await submit(frames->paperWhite);auto source=upload->Image();
        auto cache=CreateFilterPreviewCache([](CanvasQueryKind,std::string,PreviewReply){return false;});
        cache->context=L"old";cache->key=L"first";cache->images.emplace(L"filter",FilterPreviewCache::Tile{cache->key,upload});
        cache->key=L"second";cache->upload(frames->paperBluePacket,32,32,{L"filter"});
        auto result=std::make_shared<Outcome>();auto complete=std::move(upload->active->request.complete);auto weak=weak_from_this();
        upload->active->request.complete=[weak,result,complete=std::move(complete)](bool accepted){complete(accepted);if(auto self=weak.lock())self->finish(result,1,accepted);};
        bool current=true;bool retained=FilterPreviewSource(cache,L"old",L"filter",&current)==source&&!current;
        if(stale)RefreshFilterPreviewContext(cache,L"new");co_await wait(result);
        require(result->accepted.at(1)!=stale,L"Filter upload crossed its document context or rejected current pixels");
        require(retained&&FilterPreviewSource(cache,cache->context,L"filter",&current)==source&&current!=stale,
            L"Pending/stale filter upload discarded retained pixels");
        require(!FilterPreviewSource(cache,L"unrelated",L"filter",&current)&&!current,L"Filter freshness crossed its document context");
        co_await pixels(stale?frames->paperWhite.bytes:frames->paperBlueBgra,32,32);++checks;
    }
    winrt::Windows::Foundation::IAsyncAction filterRows(){
        co_await submit(frames->paperWhite);auto source=upload->Image();auto data=workspace();
        data->previews=CreateFilterPreviewCache(data->query);data->previews->context=L"1";data->previews->key=L"key";
        data->previews->images.emplace(L"filter",FilterPreviewCache::Tile{L"key",upload});
        data->state.Insert(L"filter_picker",O({{L"category",S(L"all")}}));
        A categories;categories.Append(O({{L"id",S(L"all")},{L"label",S(L"All")},{L"icon",S(L"adjustments")}}));data->state.Insert(L"filter_categories",categories);
        A choices;choices.Append(O({{L"id",S(L"filter")},{L"label",S(L"Filter")},{L"category",S(L"all")}}));data->state.Insert(L"adjustments",choices);
        auto view=std::make_shared<FiltersView>();view->data=data;view->init();view->refresh();
        require(view->previews.size()==1&&view->previews.front().image.Source()==source,L"New filter row did not bind its cached preview before the timer");++checks;
    }
    winrt::Windows::Foundation::IAsyncAction layerRow(){
        co_await submit(frames->paperWhite);auto source=upload->Image();auto data=workspace();
        A layers;layers.Append(O({{L"id",N(7)},{L"paint_revision",N(1)},{L"has_thumbnail",B(true)},{L"visible",B(true)},{L"label",S(L"Layer")},{L"selection_icon",S(L"selection")},{L"content_icon",S(L"paint")}}));data->state.Insert(L"layers",layers);
        data->thumbnails=CreateLayerThumbnailCache(data->query);LayerThumbnail item{L"7",L"7",L"1",false};
        RefreshLayerThumbnails(data->thumbnails,this,L"1",{item});data->thumbnails->images.emplace(slot(item),LayerThumbnailCache::Tile{key(item),item.target,upload,1});
        auto row=std::make_shared<CapyLayers::LayerRow>();row->data=data;row->id=7;row->epoch=L"1";row->init();row->refresh();
        require(row->contentPreview.ImageSource()==source,L"New layer row did not bind its cached thumbnail before the timer");++checks;
    }
    winrt::Windows::Foundation::IAsyncAction waveformResize(){
        auto data=workspace();auto feed=std::make_shared<ScopeFeed>();feed->owner=data;
        auto view=std::make_shared<WaveformView>();view->source=feed;view->upload=upload;view->context=feed->context();
        co_await submit(frames->paperWhite);view->image.Source(upload->Image());auto source=upload->Image();
        auto scale=image.XamlRoot().RasterizationScale();Grid chart;chart.Width(16/scale);chart.Height(16/scale);chart.Children().Append(view->image);surface.Children().Append(chart);
        co_await rendered(chart);feed->waveformSize={16,16};chart.Width(64);chart.Height(64);co_await rendered(chart);view->resize(chart);
        bool retained=view->image.Source()==source&&upload->Image()==source;
        feed->waveformContext=feed->context();feed->waveformRasterSize={1,1};view->install();
        require(retained&&view->image.Source()==source,L"Waveform resize discarded pixels while the new raster size was pending");
        co_await pixels(frames->paperWhite.bytes,32,32,false,view->image);
        data->state.Insert(L"document_file",O({{L"epoch",N(2)}}));view->install();
        require(!view->image.Source()&&!upload->Image(),L"Waveform document change retained another document's pixels");++checks;
    }
    winrt::Windows::Foundation::IAsyncAction raster(CapyWheel::FieldRequest request,std::shared_ptr<CapyWheel::FieldResult> result){
        co_await resume_background();std::vector<uint8_t> bytes;
        require(CapyWheel::rasterField(request,bytes),L"Wheel raster fixture request failed");
        com_ptr<ID2D1DeviceContext> context;check_hresult(request.device->d2d->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE,context.put()));
        *result={request.key,request.pixels,request.epoch,request.device,{}};
        check_hresult(context->CreateBitmap(D2D1::SizeU(request.pixels,request.pixels),bytes.data(),request.pixels*4,
            D2D1::BitmapProperties(D2D1::PixelFormat(DXGI_FORMAT_R8G8B8A8_UNORM,D2D1_ALPHA_MODE_PREMULTIPLIED)),result->bitmap.put()));
        co_await apartment;
    }
    winrt::Windows::Foundation::IAsyncAction wheelLayout(bool ringFirst){
        auto data=workspace();auto opened=colorUi(data->localization.get(),O({{L"type",S(L"editor_open")},{L"slot",S(L"foreground")}})).GetObject();
        auto model=object(object(opened,L"view"),L"panel");require(model.Size()>0,L"Wheel fixture has no shared color panel");
        CapyWheel::WheelImage wheel;wheel.start([]{});wheel.worker->running=wheel.ringWorker->running=true;
        Image shown;shown.Width(48);shown.Height(48);surface.Children().Append(shown);co_await rendered(shown);
        wheel.present(shown,model,data->state,48,1);
        auto ring=std::make_shared<CapyWheel::FieldResult>(),field=std::make_shared<CapyWheel::FieldResult>();
        co_await raster(*wheel.ringWorker->pending,ring);co_await raster(*wheel.worker->pending,field);
        wheel.ringReady=*ring;wheel.ready=*field;wheel.present(shown,model,data->state,48,1);
        auto source=shown.Source();auto oldRing=wheel.ring;auto oldField=wheel.field;
        auto changed=colorUi(data->localization.get(),O({{L"type",S(L"editor")},{L"editor",object(opened,L"editor")},{L"action",O({{L"op",S(L"wheel")},{L"action",O({{L"op",S(L"shape")},{L"shape",S(str(model,L"shape")==L"triangle"?L"circle":L"triangle")}})}})}})).GetObject();
        model=object(object(changed,L"view"),L"panel");wheel.present(shown,model,data->state,48,1);
        bool retained=shown.Source()==source&&wheel.ring==oldRing&&wheel.field==oldField;
        co_await raster(*wheel.ringWorker->pending,ring);co_await raster(*wheel.worker->pending,field);
        if(ringFirst)wheel.ringReady=*ring;else wheel.ready=*field;wheel.present(shown,model,data->state,56,1);
        require(retained&&shown.Source()==source&&wheel.ring==oldRing&&wheel.field==oldField,L"Wheel layout/partial result replaced the presented surface before both rasters were ready");
        if(ringFirst)wheel.ready=*field;else wheel.ringReady=*ring;wheel.present(shown,model,data->state,56,1);
        require(wheel.ring!=oldRing&&wheel.field!=oldField&&shown.Source()!=source&&wheel.pixels==56
            &&wheel.worker->pending&&wheel.worker->pending->pixels==56&&wheel.ringWorker->pending&&wheel.ringWorker->pending->pixels==56,
            L"Wheel did not adopt its ready layout pair while the latest size was pending");++checks;
    }
    winrt::Windows::Foundation::IAsyncAction retentionCases(){
        std::vector<hstring> errors;upload->Close();upload=BitmapUpload::Create();image.Source(nullptr);
        for(auto const& [name,run]:std::vector<std::pair<hstring,std::function<winrt::Windows::Foundation::IAsyncAction()>>>{
            {L"filter-key",[this]{return filterRetention(false);}},{L"filter-context",[this]{return filterRetention(true);}},
            {L"filter-upload",[this]{return filterUpload(false);}},{L"filter-stale-upload",[this]{return filterUpload(true);}},{L"filter-row",[this]{return filterRows();}},
            {L"layer-row",[this]{return layerRow();}},{L"waveform-resize",[this]{return waveformResize();}},{L"wheel-ring-first",[this]{return wheelLayout(true);}},{L"wheel-field-first",[this]{return wheelLayout(false);}}}){
            phase=name;hstring failure;try{co_await run();}catch(hresult_error const& e){failure=e.message();}catch(...){failure=L"Unhandled fixture failure";}
            retentionResults.Append(O({{L"case",S(name)},{L"passed",B(failure.empty())},{L"error",S(failure)}}));if(!failure.empty())errors.push_back(name+L": "+failure);
            surface.Children().Clear();surface.Children().Append(image);upload->Close();upload=BitmapUpload::Create();image.Source(nullptr);
        }
        if(!errors.empty()){hstring message;for(auto const& error:errors)message=message+error+L"\n";throw hresult_error(E_FAIL,message);}
    }
    winrt::fire_and_forget run(){
        auto self=shared_from_this();hstring error;
        try{
            phase=L"encoding";co_await encoding();phase=L"reentrancy";co_await coalescing();
            phase=L"before-copy-context";co_await invalidation();phase=L"throwing-callbacks";co_await exceptions();
            phase=L"predicate-reentry";co_await predicateReentry();phase=L"awaited-context";co_await awaitedCancellation();phase=L"thumbnail-epoch";co_await thumbnailEpoch();phase=L"thumbnail-owners";thumbnailOwners();phase=L"paper-recolor";co_await paperRecolor();co_await retentionCases();phase=L"close";
            auto result=std::make_shared<Outcome>();upload->Close();
            upload->Submit(request(frames->rgba,result,1));co_await wait(result);
            require(!result->accepted.at(1),L"Closed owner accepted new pixels");++checks;
        }catch(hresult_error const& e){resultCode=1;error=e.message();}
        catch(...){resultCode=1;error=L"Unhandled fixture failure";}
        auto path=output;auto count=checks;auto text=to_string(error);auto step=to_string(phase);auto allocations=identities.size();auto slots=poolSize;auto cases=to_string(retentionResults.Stringify());
        co_await resume_background();
        std::ofstream details(path.parent_path()/L"bitmap-retention-fixture-result.json");details<<cases;
        std::ofstream file(path);file<<"{\"passed\":"<<count<<",\"exit_code\":"<<resultCode<<",\"phase\":\""<<step<<"\",\"observed_bitmap_identities\":"<<allocations<<",\"current_pool_slots\":"<<slots<<"}\n";
        if(!text.empty()){std::ofstream raw(path.parent_path()/L"bitmap-upload-fixture-error.log");raw<<text;}
        if(!file)resultCode=1;
        co_await apartment;upload->Close();window.Close();Application::Current().Exit();
    }
    void init(std::filesystem::path path){
        output=path.parent_path()/L"bitmap-upload-fixture-result.json";upload=BitmapUpload::Create();
        surface.Width(64);surface.Height(64);surface.UseLayoutRounding(false);
        image.Width(3);image.Height(2);image.Stretch(Stretch::Fill);image.UseLayoutRounding(false);
        surface.Children().Append(image);window.Content(surface);window.Title(L"Capy Canvas bitmap upload review");
        surface.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->run();});
        window.Activate();
    }
};
struct App:ApplicationT<App,Markup::IXamlMetadataProvider>{
    Microsoft::UI::Xaml::XamlTypeInfo::XamlControlsXamlMetaDataProvider metadata;
    std::shared_ptr<Fixture> fixture;std::shared_ptr<Frames> frames;ControlContext context;
    App(std::shared_ptr<Frames> data,ControlContext source):frames(std::move(data)),context(std::move(source)){UnhandledException([this](auto&&,UnhandledExceptionEventArgs const& args){
        resultCode=1;args.Handled(true);if(fixture)fixture->upload->Close();Exit();
    });}
    Markup::IXamlType GetXamlType(winrt::Windows::UI::Xaml::Interop::TypeName const& type){return metadata.GetXamlType(type);}
    Markup::IXamlType GetXamlType(hstring const& name){return metadata.GetXamlType(name);}
    com_array<Markup::XmlnsDefinition> GetXmlnsDefinitions(){return metadata.GetXmlnsDefinitions();}
    void OnLaunched(LaunchActivatedEventArgs const&){
        Resources().MergedDictionaries().Append(XamlControlsResources());Resources().MergedDictionaries().Append(CapyUi::paletteResources());
        wchar_t path[32768];auto length=GetEnvironmentVariableW(L"CAPY_BITMAP_FIXTURE",path,32768);
        if(!length||length>=32768||!std::filesystem::path(path).is_absolute())throw hresult_invalid_argument();
        fixture=std::make_shared<Fixture>();fixture->frames=frames;fixture->controlContext=context;fixture->init(path);
    }
};
}
int WINAPI wWinMain(HINSTANCE,HINSTANCE,PWSTR,int){
    auto context=fixtureContext(L"CAPY_BITMAP_FIXTURE");std::shared_ptr<Frames> frames;
    {std::jthread startup([&]{frames=std::make_shared<Frames>();});}
    init_apartment(apartment_type::single_threaded);
    Application::Start([frames,context](auto&&){make<App>(frames,context);});return resultCode;
}
