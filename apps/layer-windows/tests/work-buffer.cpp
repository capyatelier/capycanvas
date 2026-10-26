#include "../CanvasWorkBuffer.h"
#include "../TextCompositionKeys.h"
#include "../NativeKeyRepeats.h"
#include "../CanvasPointerSample.h"
#include "../PenSourceCache.h"
#include "../PointerCompletion.h"
#include "../ThumbnailOwners.h"
#include "../PointerScrub.h"
#include <limits>
#include "../CanvasQueryQueue.h"
#include "../CanvasLatencyTrace.h"
#include "../CanvasSnapshotMailbox.h"
#include "../WorkspacePublication.h"
#include "../GamepadState.h"
#include <algorithm>
#include <string>
#include <vector>
#include <cassert>
#include <iostream>
#include <atomic>
#include <mutex>
#include <thread>
#ifdef _WIN32
#include "../TraceFile.h"
#endif

int main() {
    PenSourceCache penSources;
    penSources.Select("document-1/epoch-1");assert(penSources.Describe(7,false));
    assert(penSources.Update(7,"physical-pen"));assert(!penSources.Describe(7,false));assert(penSources.Describe(7,true));
    assert(!penSources.Update(7,"physical-pen"));
    penSources.Select("document-1/epoch-1");assert(!penSources.Describe(7,false));
    penSources.Select("document-2/epoch-1");assert(penSources.Describe(7,false));assert(penSources.Update(7,"physical-pen"));
    penSources.Select("document-2/epoch-2");assert(penSources.Describe(7,false));assert(penSources.Update(7,"physical-pen"));
    penSources.Forget(7);assert(penSources.Describe(7,false));
    assert(penSources.Update(7,"physical-pen"));assert(penSources.Describe(8,false));
    assert(penSources.Update(8,"physical-pen"));assert(penSources.Describe(7,false));
    penSources.Forget(7);assert(!penSources.Describe(8,false));
    assert(penSources.Update(7,"physical-pen"));assert(!penSources.Describe(7,false));
    std::cout<<"Pen sources register again when drawing sessions or epochs change\n";
    static_assert(offsetof(CapySnapshotInfo,camera)==((4*sizeof(uint32_t)+alignof(size_t)-1)&~(alignof(size_t)-1)));
    static_assert(offsetof(CapySnapshotInfo,brush_ready)==offsetof(CapySnapshotInfo,camera)+6*sizeof(size_t));
    static_assert(offsetof(CapySnapshotInfo,glass)==offsetof(CapySnapshotInfo,camera)+8*sizeof(size_t));
    static_assert(sizeof(CapySnapshotInfo)==offsetof(CapySnapshotInfo,camera)+10*sizeof(size_t));
#ifdef _WIN32
    auto traceFolder=std::filesystem::temp_directory_path()/(L"capy-trace-"+std::to_wstring(GetCurrentProcessId())+L"-"+std::to_wstring(GetTickCount64()));
    assert(std::filesystem::create_directory(traceFolder));
    auto trace=traceFolder/L"windows.json";
    WriteTraceFile(trace,"old");
    auto reader=CreateFileW(trace.c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,nullptr);
    assert(reader!=INVALID_HANDLE_VALUE);
    auto pending=trace.wstring()+L".pending";
    {std::ofstream replacement(pending);replacement<<"pending";}
    auto replacementReader=CreateFileW(pending.c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,nullptr);
    assert(replacementReader!=INVALID_HANDLE_VALUE);
    WriteTraceFile(trace,"current");
    std::ifstream currentTrace(trace);std::string currentValue;currentTrace>>currentValue;
    assert(currentValue=="current");assert(!std::filesystem::exists(trace.wstring()+L".pending"));
    char retained[3];DWORD read=0;assert(ReadFile(reader,retained,3,&read,nullptr)&&read==3);assert(std::string(retained,3)=="old");
    currentTrace.close();assert(CloseHandle(reader));assert(CloseHandle(replacementReader));
    auto latencyFolder=traceFolder/L"latency";assert(std::filesystem::create_directory(latencyFolder));
    auto latencyPrefix=(latencyFolder/L"capture").string();
    {CanvasLatencyTrace latency(true);uint64_t stats[6]{7,6,5,4,3,2};
        latency.Frame(10,11,12,13,stats,0);latency.Frame(20,21,22,23,stats,0);latency.Dump(latencyPrefix);}
    std::ifstream presentations(latencyPrefix+"-presentations.csv");std::string line;size_t presented=0;
    while(std::getline(presentations,line))++presented;presentations.close();assert(presented==3);
    std::ifstream movingFrames(latencyPrefix+"-frames.csv");size_t moving=0;
    while(std::getline(movingFrames,line))++moving;movingFrames.close();assert(moving==1);
    assert(std::filesystem::remove_all(latencyFolder)==6);
    assert(std::filesystem::remove_all(traceFolder)==2);
    std::cout<<"Trace publication atomically replaces window manifests while readers retain complete snapshots\n";
#endif

    for(uint32_t key:{13u,27u,40u}) {
        TextCompositionKeys input;
        assert(!input.Busy());input.Update(true,10);assert(input.Busy());
        input.Update(true,20);
        input.Update(false,30);
        assert(input.Owns(key,30,false,false));
        assert(input.Owns(key,30,false,false));
        assert(input.Owns(key,40,false,true));assert(input.Busy());
        assert(input.Owns(key,50,true,true));assert(!input.Busy());
        assert(input.Owns(key,50,true,true));assert(!input.Busy());
        assert(!input.Owns(key,60,false,false));
        input.Update(true,70);input.Update(false,80);
        assert(!input.Owns(key,90,false,false));
        input.Update(true,100);input.Update(false,110);
        assert(input.Owns(key,110,false,false));
        assert(!input.Owns(key,120,false,false));
        input.Update(true,130);input.Update(false,140);
        assert(input.Owns(key,140,false,false));input.Pointer();
        assert(!input.Owns(key,150,false,false));
        input.Update(true,160);assert(input.Active());input.Clear();
        assert(!input.Active()&&!input.Owns(key,170,false,false));
        input.Update(true,180);input.Pointer();assert(input.Active());
        input.Update(false,190);assert(!input.Owns(key,200,false,false));
        input.Update(true,210);input.Update(false,220);
        assert(input.Owns(key,220,false,false));
        assert(input.Owns(key,220,true,true));
        assert(!input.Owns(key,220,false,false));
        TextCompositionKeys nested;
        nested.Update(true,3000);nested.Update(false,3031);
        assert(nested.Owns(key,3015,false,false));
        assert(nested.Owns(key,3015,true,true));
        assert(!nested.Owns(key,3015,false,false));
        nested.Clear();nested.Update(true,0xffffffe0);nested.Update(false,0x10);
        assert(nested.Owns(key,0xfffffff0,false,false));
        assert(nested.Owns(key,0x11,true,true));
        assert(!nested.Owns(key,0x20,false,false));
        nested.Update(true,0x30);nested.Update(false,0x40);
        assert(nested.Owns(key,0x3f,false,false));
        TextCompositionKeys delayed;
        delayed.Update(true,230);delayed.Update(false,240);
        assert(delayed.Owns(key,240,true,true));
        assert(delayed.Owns(key,240,false,false));
        assert(delayed.Owns(key,240,false,false));
        assert(!delayed.Owns(key,250,false,false));
        delayed.Clear();delayed.Update(true,260);delayed.Update(false,270);
        assert(!delayed.Owns(16,280,true,true));
        assert(delayed.Owns(key,270,false,false));
        delayed.Clear();delayed.Update(true,290);
        assert(delayed.Owns(16,300,false,false));
        delayed.Update(false,310);
        assert(delayed.Owns(16,320,true,true));
        assert(delayed.Owns(key,310,false,false));
    }
    std::cout<<"Composition retains confirming keys through release, without consuming pointer confirmation or the next press\n";

    std::vector<bool> repeats;
    auto collect=[&](bool repeat){repeats.push_back(repeat);};
    ForwardKeyRepeats(true,false,3,collect);
    ForwardKeyRepeats(true,true,4,collect);
    ForwardKeyRepeats(false,true,1,collect);
    assert((repeats==std::vector<bool>{false,true,true,true,true,true,true,false}));
    repeats.clear();
    ForwardKeyRepeats(true,false,0,collect);
    ForwardKeyRepeats(false,false,0,collect);
    assert((repeats==std::vector<bool>{false,false}));
    std::cout<<"Combined native key presses retain every repeat and one release\n";

    using namespace CapyUi;
    {
        PointerCancelRegistry contacts;int slider,scroll,row;std::vector<std::string> canceled;
        contacts.Observe(1,&slider,[&]{canceled.push_back("slider");contacts.Forget(1,&scroll);contacts.Observe(1,&slider,[&]{canceled.push_back("next");});});
        contacts.Observe(1,&scroll,[&]{canceled.push_back("scroll");contacts.Forget(1,&row);});
        contacts.Observe(1,&row,[&]{canceled.push_back("row");});
        contacts.Observe(2,&scroll,[&]{canceled.push_back("other");});
        contacts.Forget(1,&row);contacts.Cancel(1);
        std::sort(canceled.begin(),canceled.end());assert((canceled==std::vector<std::string>{"scroll","slider"}));
        contacts.Cancel(1);assert(canceled.back()=="next"&&canceled.size()==3);
        contacts.Cancel(1);assert(canceled.size()==3);
        contacts.Cancel(2);assert(canceled.back()=="other"&&canceled.size()==4);
        contacts.Observe(3,&scroll,[&]{canceled.push_back("cleared");});contacts.Clear();contacts.Cancel(3);assert(canceled.size()==4);
    }
    std::cout<<"Capture transfer cancels every native owner once while preserving reentrant and unrelated contacts\n";

    {
        ThumbnailOwners<std::string,std::string> thumbnails;int list,face,card;
        thumbnails.Update(&list,{{"a","paint:1"}});
        thumbnails.Update(&face,{{"b","paint:2"}});
        assert(thumbnails.Items().at("a")=="paint:1"&&thumbnails.Items().at("b")=="paint:2");
        thumbnails.Update(&card,{{"a","paint:1"}});thumbnails.Forget(&list);
        assert(thumbnails.Items().size()==2&&thumbnails.Items().at("a")=="paint:1");
        thumbnails.Update(&list,{{"a","paint:3"}});
        thumbnails.Update(&card,{{"a","paint:1"}});
        assert(thumbnails.Items().at("a")=="paint:3");
        thumbnails.Update(&card,{{"a","mask:3"}});
        assert(thumbnails.Items().at("a")=="mask:3");
        thumbnails.Update(&face,{});assert(!thumbnails.Items().contains("b"));
        thumbnails.Forget(&card);assert(thumbnails.Items().at("a")=="paint:3");
        thumbnails.Clear();assert(thumbnails.Items().empty());
        thumbnails.Update(&face,{{"a","new-drawing:1"}});
        thumbnails.Forget(&list);assert(thumbnails.Items().at("a")=="new-drawing:1");
    }
    std::cout<<"Thumbnail owners retain independent pending targets, deduplicate shared rows and retire changed sources\n";

    struct CompletionCase {PointerCompletion event;bool canceled,inContact,customCancel,nativeCancel;};
    for(auto test:{
        CompletionCase{PointerCompletion::Released,false,false,false,false},
        CompletionCase{PointerCompletion::Released,false,true,false,false},
        CompletionCase{PointerCompletion::Released,true,false,true,true},
        CompletionCase{PointerCompletion::Released,true,true,true,true},
        CompletionCase{PointerCompletion::Canceled,false,false,true,true},
        CompletionCase{PointerCompletion::Canceled,false,true,true,true},
        CompletionCase{PointerCompletion::CaptureLost,false,false,true,false},
        CompletionCase{PointerCompletion::CaptureLost,false,true,true,true},
        CompletionCase{PointerCompletion::CaptureLost,true,false,true,true},
        CompletionCase{PointerCompletion::CaptureLost,true,true,true,true}}){
        assert(CanceledCompletion(test.event,test.canceled,test.inContact)==test.customCancel);
        assert(CanceledCompletion(test.event,test.canceled,test.inContact,PointerCaptureLoss::CompleteLift)==test.nativeCancel);
    }
    std::cout<<"Canceled completion rolls back; native controls preserve normal lift capture loss\n";

    {
        PointerClick clicks;unsigned activations=0;
        auto activate=[&]{if(!clicks.SuppressClick())++activations;};
        auto complete=[&](uint32_t id,PointerCompletion phase,bool cancel=false,bool captured=false){return clicks.Complete(id,phase,cancel,captured);};
        assert(clicks.Begin(1));activate();assert(complete(1,PointerCompletion::CaptureLost));
        assert(!complete(1,PointerCompletion::Released));activate();assert(activations==2);
        assert(clicks.Begin(2));clicks.Suppress(2);activate();assert(complete(2,PointerCompletion::CaptureLost));
        assert(!complete(2,PointerCompletion::Released));activate();assert(activations==3);

        assert(clicks.Begin(3));clicks.Suppress(3);assert(complete(3,PointerCompletion::CaptureLost,false,true));
        assert(!complete(3,PointerCompletion::Released));activate();assert(activations==4);
        assert(clicks.Begin(4));clicks.Suppress(4);assert(complete(4,PointerCompletion::Released,false,true));
        activate();assert(activations==4);assert(!complete(4,PointerCompletion::CaptureLost,false,true));
        activate();assert(activations==5);
        assert(clicks.Begin(5));clicks.Suppress(5);assert(complete(5,PointerCompletion::CaptureLost,false,true));
        activate();assert(activations==5);assert(!complete(5,PointerCompletion::Released));
        activate();assert(activations==6);

        assert(clicks.Begin(6));clicks.Suppress(6);assert(complete(6,PointerCompletion::Canceled,true,true));
        activate();assert(activations==7&&!complete(6,PointerCompletion::Released));
        assert(clicks.Begin(7));activate();assert(complete(7,PointerCompletion::Released));assert(activations==8);
        assert(clicks.Begin(8));clicks.Suppress(8);assert(!clicks.Begin(9)&&!complete(9,PointerCompletion::Canceled,true));
        clicks.Reset();activate();assert(activations==8&&clicks.Owns(8));
        assert(complete(8,PointerCompletion::Released));activate();assert(activations==9);

        assert(clicks.Begin(10));clicks.Suppress(10);assert(complete(10,PointerCompletion::Released,false,true));
        assert(clicks.Begin(11)&&!clicks.SuppressClick());clicks.Suppress(11);
        assert(!complete(10,PointerCompletion::CaptureLost)&&clicks.Owns(11));
        assert(complete(11,PointerCompletion::Released,false,true));assert(!complete(10,PointerCompletion::CaptureLost));
        activate();assert(activations==9);assert(!complete(11,PointerCompletion::CaptureLost,false,true));
        activate();assert(activations==10);
        assert(clicks.Begin(12));clicks.Suppress(12);assert(clicks.Cancel()==12u);
        assert(!clicks.Owns(12)&&!complete(12,PointerCompletion::Released)&&!clicks.SuppressClick());
        activate();assert(activations==11&&!clicks.Cancel());
    }
    std::cout<<"Scrub suppression preserves native Click ordering and permits immediate accessibility activation after release\n";

    {
        PointerScrub motion{100};std::vector<double> values;
        auto move=[&](double y,bool generated,double speed){if(auto pixels=motion.Move(y,generated))values.push_back(255+*pixels*.5*speed);};
        move(108,false,1);move(108,false,10);move(120,true,10);move(108,false,10);
        assert((values==std::vector<double>{251}));
        move(112,false,10);move(112,false,.1);move(100,false,1);
        assert((values==std::vector<double>{251,195,255}));
    }
    std::cout<<"Stationary and generated pointer updates cannot reapply a scrub with new modifiers\n";

    for(uint32_t phase=0;phase<=4;++phase) {
        assert(CanvasPointerPhase(phase,false)==phase);
        assert(CanvasPointerPhase(phase,true)==4);
    }
    std::cout<<"Canvas canceled samples retain cancellation across every native event phase\n";
    struct History {
        bool present;uint32_t count;
        explicit operator bool()const{return present;}
        uint32_t Size()const{assert(present);return count;}
    };
    for(uint32_t phase=0;phase<=4;++phase){
        assert(CanvasPointerHistoryCount(History{false,3},phase)==0);
        assert(CanvasPointerHistoryCount(History{true,0},phase)==0);
        assert(CanvasPointerHistoryCount(History{true,3},phase)==((phase==0||phase==2)?3u:0u));
    }
    std::cout<<"Missing native pointer history uses the current point in every event phase\n";
    for(uint32_t tool:{0u,2u}) {
        for(bool middle:{false,true})for(bool right:{false,true})
            assert(CanvasPointerButton(tool,middle,right)==0);
    }
    assert(CanvasPointerButton(1,false,false)==0);
    assert(CanvasPointerButton(1,true,false)==1);
    assert(CanvasPointerButton(1,false,true)==1);
    assert(CanvasPointerButton(1,false,false,true)==2);
    for(bool right:{false,true})assert(CanvasPointerButton(0,false,right,true)==0);
    std::cout<<"Canvas pen/eraser side buttons ignored; mouse pan buttons preserved\n";
    CapyPointer actual{};actual.pressure=0.37f;actual.x=42.25f;actual.timestamp_ns=12345;
    auto real=actual;
    assert(PrepareCanvasPrediction(real));
    assert(real.pressure==actual.pressure&&real.x==actual.x&&real.timestamp_ns==actual.timestamp_ns);
    auto prediction=actual;prediction.flags=1;prediction.pressure=1.0001f;
    assert(PrepareCanvasPrediction(prediction)&&prediction.pressure==1.0f);
    prediction.pressure=-0.001f;
    assert(PrepareCanvasPrediction(prediction)&&prediction.pressure==0.0f);
    prediction.x=std::numeric_limits<float>::quiet_NaN();
    assert(!PrepareCanvasPrediction(prediction));
    prediction=actual;prediction.flags=1;prediction.pressure=std::numeric_limits<float>::infinity();
    assert(!PrepareCanvasPrediction(prediction));
    actual.pressure=1.1f;
    assert(PrepareCanvasPrediction(actual)&&actual.pressure==1.1f); // real validation remains in Rust
    std::cout<<"Canvas predictions: extrapolated pressure bounded, invalid predictions rejected, real input preserved\n";
    WorkspacePublication publication;
    assert(!publication.Accept(false,10,4,40));
    assert(!publication.Accept(false,10,4,40,true));
    assert(publication.Accept(true,10,4,40));
    assert(publication.Accept(false,12,4,40));
    assert(!publication.Accept(false,11,4,40));
    assert(!publication.Accept(false,13,5,50));
    assert(publication.Revision()==12&&publication.ModelRevision()==4);
    assert(!publication.Accept(true,11,4,40));
    assert(publication.Accept(true,13,5,50));
    assert(!publication.Accept(false,14,4,40));
    assert(publication.Accept(true,13,5,50));
    assert(publication.Accept(false,14,5,50));
    assert(publication.Accept(false,15,6,50,true));
    assert(publication.Revision()==15&&publication.ModelRevision()==6);
    assert(publication.Accept(false,16,6,50));
    assert(!publication.Accept(false,17,5,50));
    assert(!publication.Accept(false,15,7,50,true));
    assert(!publication.Accept(false,17,5,50,true));
    assert(!publication.Accept(false,17,7,60,true));
    assert(publication.Revision()==16&&publication.ModelRevision()==6);
    assert(publication.Accept(false,17,6,50,true));
    assert(publication.Accept(true,18,7,70));
    assert(!publication.Accept(false,19,8,50,true));
    assert(!publication.Accept(false,19,6,70,true));
    assert(publication.Accept(false,19,8,70,true));
    assert(publication.Accept(false,20,8,70));
    std::cout<<"Workspace publication: ordered motion, retained content reflow and full replacement passed\n";
    CanvasSnapshotMailbox snapshots;
    snapshots.Push("models A",true,true);
    snapshots.Push("motion A + camera 1",false,true,"camera 1");
    snapshots.Push("motion B",false,true);
    auto shown=snapshots.Take();
    assert(shown.full=="models A"&&shown.workspace=="motion B"&&shown.camera=="camera 1");
    snapshots.Push("motion C + camera 2",false,true,"camera 2");
    snapshots.Push("camera 3",false,false,"camera 3");
    snapshots.Push("motion D",false,true);
    shown=snapshots.Take();
    assert(shown.full.empty()&&shown.workspace=="motion D"&&shown.camera=="camera 3");
    // A completed gesture's full model must discard every older motion/camera.
    snapshots.Push("motion E + camera 4",false,true,"camera 4");
    snapshots.Push("models B (release)",true,true);
    shown=snapshots.Take();
    assert(shown.full=="models B (release)"&&shown.workspace.empty()&&shown.camera.empty());
    snapshots.Push("models C",true,true);
    snapshots.Push("motion F",false,true);
    snapshots.Push("models D (cancel)",true,true);
    snapshots.Push("camera 5",false,false,"camera 5");
    shown=snapshots.Take();
    assert(shown.full=="models D (cancel)"&&shown.workspace.empty()&&shown.camera=="camera 5");
    snapshots.Push("models E",true,true);
    snapshots.Push("search open",false,false,{},true);
    snapshots.Push("motion G",false,true);
    shown=snapshots.Take();
    assert(shown.full=="models E"&&shown.search=="search open"&&shown.workspace=="motion G");
    snapshots.Push("search + glass 1",false,false,{},true,{},false,"glass 1");
    snapshots.Push("layout + glass 2",false,true,{},false,{},true,"glass 2");
    snapshots.Push("camera + glass 3",false,false,"camera 3",false,{},false,"glass 3");
    shown=snapshots.Take();
    assert(shown.search=="search + glass 1"&&shown.layout=="layout + glass 2"&&shown.camera=="camera 3"&&shown.glass=="glass 3");
    snapshots.Push("glass 4",false,false,{},false,{},false,"glass 4");
    snapshots.Push("replacement model + glass 5",true,true,{},false,{},false,"glass 5");
    shown=snapshots.Take();
    assert(shown.full=="replacement model + glass 5"&&shown.glass=="glass 5"&&shown.search.empty());
    snapshots.Push("search query",false,false,{},true);
    snapshots.Push("models F (search closed)",true,true);
    shown=snapshots.Take();
    assert(shown.full=="models F (search closed)"&&shown.search.empty());
    shown=snapshots.Take();
    assert(shown.full.empty()&&shown.workspace.empty()&&shown.camera.empty());
    std::cout<<"Canvas presentation: retained models, independent motion/camera coalescing and completion boundaries passed\n";
    CanvasSnapshotMailbox decoding;
    decoding.Push("full A",true,true,{},false,"catalog A",false,"glass A");
    auto firstDecode=decoding.BeginDecode();
    assert(firstDecode&&firstDecode->full=="full A"&&firstDecode->localization=="catalog A");
    auto firstSequence=firstDecode->sequence;
    assert(!decoding.IsPosted(firstSequence));
    decoding.Push("layout B",false,true,"camera B",false,{},true,"glass B");
    decoding.Push("motion C",false,true);
    decoding.Push("motion D",false,true,"camera D");
    decoding.Push("search D",false,false,{},true);
    auto querySequence=decoding.Sequence();
    assert(querySequence>firstSequence&&!decoding.IsPosted(querySequence));
    decoding.Posted(firstSequence);
    assert(decoding.IsPosted(firstSequence)&&!decoding.IsPosted(querySequence));
    assert(!decoding.BeginDecode());
    decoding.CompleteDecode();
    auto secondDecode=decoding.BeginDecode();
    assert(secondDecode&&secondDecode->full.empty()&&secondDecode->layout=="layout B"&&secondDecode->workspace=="motion D");
    assert(secondDecode->camera=="camera D"&&secondDecode->search=="search D"&&secondDecode->glass=="glass B");
    assert(secondDecode->sequence==querySequence);
    decoding.Posted(secondDecode->sequence);decoding.Posted(firstSequence);
    assert(decoding.IsPosted(querySequence));
    decoding.Push("layout E",false,true,{},false,{},true);
    decoding.Push("full F",true,true);
    decoding.Push("new camera",false,false,"camera F");
    assert(!decoding.BeginDecode());
    decoding.CompleteDecode();
    auto thirdDecode=decoding.BeginDecode();
    assert(thirdDecode&&thirdDecode->full=="full F"&&thirdDecode->layout.empty()&&thirdDecode->workspace.empty());
    assert(thirdDecode->camera=="camera F"&&thirdDecode->localization.empty()&&firstDecode->localization=="catalog A");
    decoding.Push("dependent delta",false,true);
    decoding.Close();decoding.CompleteDecode();
    decoding.Push("late full",true,true);
    assert(decoding.Closed()&&!decoding.BeginDecode()&&decoding.Take().full.empty()&&!decoding.IsPosted(querySequence));
    std::mutex decodeMutex;
    CanvasSnapshotMailbox concurrent;
    concurrent.Push("initial full",true,true);
    std::atomic<bool> published=false;
    unsigned activeDecoders=0,decodeCount=0;
    bool fullApplied=false;
    std::string lastDecodedCamera;
    auto consume=[&]{
        for(;;){
            std::optional<CanvasSnapshotMailbox::Batch> batch;
            {
                std::lock_guard lock(decodeMutex);
                batch=concurrent.BeginDecode();
                if(batch){assert(activeDecoders==0);++activeDecoders;++decodeCount;}
                else if(published.load()&&activeDecoders==0)break;
            }
            if(!batch){std::this_thread::yield();continue;}
            std::this_thread::yield();
            {
                std::lock_guard lock(decodeMutex);
                if(!batch->full.empty())fullApplied=true;
                if(!batch->camera.empty()){assert(fullApplied);lastDecodedCamera=batch->camera;}
                assert(activeDecoders==1);--activeDecoders;concurrent.CompleteDecode();
            }
        }
    };
    std::thread decodeOne(consume),decodeTwo(consume);
    std::thread publish([&]{
        for(unsigned i=0;i<1000;++i){
            std::lock_guard lock(decodeMutex);
            concurrent.Push("camera update",false,false,std::to_string(i));
        }
        published=true;
    });
    publish.join();decodeOne.join();decodeTwo.join();
    assert(fullApplied&&decodeCount>0&&lastDecodedCamera=="999");
    std::cout<<"Canvas decoding: bounded leases, dependent batches, catalog retention, concurrent publication and terminal close passed\n";
    snapshots.Push("translated models",true,true,{},false,"Japanese context");
    snapshots.Push("newer models",true,true);
    shown=snapshots.Take();
    assert(shown.full=="newer models"&&shown.localization=="Japanese context");
    snapshots.Push("Chinese models",true,true,{},false,"Chinese context");
    snapshots.Push("Korean models",true,true,{},false,"Korean context");
    shown=snapshots.Take();assert(shown.localization=="Korean context");
    assert(snapshots.Take().localization.empty());
    snapshots.Push("localized model",true,true,{},false,"French context");
    snapshots.Push("new model with null localization",true,true,{},false,"");
    shown=snapshots.Take();assert(shown.full=="new model with null localization"&&shown.localization=="French context");
    snapshots.Push("incremental localization",false,true,{},false,"Thai context");
    snapshots.Push("incremental search without localization",false,false,{},true,"");
    shown=snapshots.Take();assert(shown.workspace=="incremental localization"&&shown.search=="incremental search without localization"&&shown.localization.empty());
    assert(snapshots.Take().localization.empty());
    snapshots.Push("complete translated model",true,true);
    shown=snapshots.Take();assert(shown.full=="complete translated model"&&shown.localization=="Thai context");
    snapshots.Push("layout B + camera 6",false,true,"camera 6",false,{},true);
    snapshots.Push("motion B",false,true);
    shown=snapshots.Take();
    assert(shown.full.empty()&&shown.layout=="layout B + camera 6"&&shown.workspace=="motion B"&&shown.camera=="camera 6");
    snapshots.Push("layout B",false,true,{},false,{},true);
    snapshots.Push("motion B",false,true);
    snapshots.Push("layout C",false,true,{},false,{},true);
    snapshots.Push("camera 7",false,false,"camera 7");
    snapshots.Push("search retained",false,false,{},true);
    shown=snapshots.Take();
    assert(shown.layout=="layout C"&&shown.workspace.empty()&&shown.camera=="camera 7"&&shown.search=="search retained");
    snapshots.Push("layout D",false,true,"camera 8",false,"German context",true);
    snapshots.Push("motion D",false,true);
    snapshots.Push("search cleared",false,false,{},true);
    snapshots.Push("models G",true,true);
    shown=snapshots.Take();
    assert(shown.full=="models G"&&shown.layout.empty()&&shown.workspace.empty()&&shown.camera.empty()&&shown.search.empty()&&shown.localization=="German context");
    snapshots.Push("models H",true,true);
    snapshots.Push("layout H",false,true,{},false,{},true);
    snapshots.Push("motion H",false,true);
    snapshots.Push("camera 9",false,false,"camera 9");
    shown=snapshots.Take();
    assert(shown.full=="models H"&&shown.layout=="layout H"&&shown.workspace=="motion H"&&shown.camera=="camera 9");
    shown=snapshots.Take();
    assert(shown.full.empty()&&shown.layout.empty()&&shown.workspace.empty()&&shown.camera.empty()&&shown.search.empty()&&shown.localization.empty());
    std::cout<<"Canvas layout publications survive later motion and retire superseded geometry passed\n";
    CanvasWorkBuffer queue;
    // Saturation must not consume the caller's rejected command. Every
    // accepted boundary and command remains in the original order.
    for(size_t i=0;i<CanvasWorkBuffer::MaxItems-CanvasWorkBuffer::CommandSlots;i++) {
        CapyPointer p{};p.sequence=i;p.phase=i==0?1:2;
        assert(queue.Push(std::vector<CapyPointer>{p}));
    }
    CanvasWork rejected=std::vector<CapyPointer>{CapyPointer{}};
    std::get<std::vector<CapyPointer>>(rejected)[0].phase=3;
    assert(!queue.Push(std::move(rejected)));
    assert(std::get<std::vector<CapyPointer>>(rejected)[0].phase==3);
    // Pointer saturation must leave room for UI actions, especially Blur.
    assert(queue.Push(CanvasCommand{CanvasCommandKind::Input,"blur"}));
    auto batch=queue.Take();
    assert(batch.size()==CanvasWorkBuffer::MaxItems-CanvasWorkBuffer::CommandSlots+1);
    for(size_t i=0;i+1<batch.size();i++) {
        auto p=std::get<std::vector<CapyPointer>>(batch[i])[0];
        assert(p.sequence==i);
        assert(p.phase==(i==0?1u:2u));
    }
    assert(std::get<CanvasCommand>(batch.back()).json=="blur");
    assert(queue.Empty());
    assert(queue.Push(CanvasCommand{CanvasCommandKind::Action,"undo"}));
    CapyPointer up{};up.phase=3;
    assert(queue.Push(std::vector<CapyPointer>{up}));
    batch=queue.Take();
    assert(std::get<CanvasCommand>(batch[0]).json=="undo");
    assert(std::get<std::vector<CapyPointer>>(batch[1])[0].phase==3);
    assert(queue.Push(std::move(rejected)));
    queue.Take();
    for(size_t i=0;i<CanvasWorkBuffer::MaxItems;i++)assert(queue.Push(CanvasCommand{CanvasCommandKind::Action,"fit"}));
    CanvasWork refused=CanvasCommand{CanvasCommandKind::Action,"undo"};
    assert(!queue.Push(std::move(refused)));
    assert(std::get<CanvasCommand>(refused).json=="undo");
    queue.Take();

    // Byte limits also apply to a single allocation and retained spare
    // capacity: shrinking a string/vector cannot bypass the memory bound.
    CanvasWork huge=CanvasCommand{CanvasCommandKind::Action,std::string(CanvasWorkBuffer::MaxBytes,'x')};
    assert(!queue.Push(std::move(huge)));
    std::get<CanvasCommand>(huge).json.resize(1);
    assert(!queue.Push(std::move(huge)));
    std::vector<CapyPointer> spare;
    spare.reserve(CanvasWorkBuffer::MaxBytes/sizeof(CapyPointer)+1);
    assert(!queue.Push(std::move(spare)));
    assert(queue.Empty());
    // Payload exhaustion must be possible before item exhaustion.
    size_t accepted=0;
    while(queue.Push(CanvasCommand{CanvasCommandKind::Action,std::string(16384,'x')}))++accepted;
    assert(accepted>0&&accepted<CanvasWorkBuffer::MaxItems);
    queue.Take();
    assert(queue.Push(CanvasScroll{1,2,3,4,2,true,false}));
    std::cout<<"Canvas queue: ordered boundaries, reserved commands, refusal ownership, item and allocation limits passed\n";

    assert(queue.Push(CanvasCommand{CanvasCommandKind::Document,"close"}));
    CapyPointer down{};down.phase=1;
    assert(queue.Push(std::vector<CapyPointer>{down}));
    assert(queue.Push(CanvasCommand{CanvasCommandKind::Action,"undo"}));
    batch=queue.TakeBeforePointers();
    assert(batch.size()==2&&std::holds_alternative<CanvasScroll>(batch[0])&&std::get<CanvasCommand>(batch[1]).json=="close");
    assert(queue.TakeBeforePointers().empty()&&!queue.Empty());
    batch=queue.Take();
    assert(batch.size()==2&&std::get<std::vector<CapyPointer>>(batch[0])[0].phase==1&&std::get<CanvasCommand>(batch[1]).json=="undo");
    size_t commands=0;
    while(queue.Push(CanvasCommand{CanvasCommandKind::Action,std::string(16384,'x')}))++commands;
    assert(queue.TakeBeforePointers().size()==commands&&queue.Empty());
    for(size_t i=0;i<commands;i++)assert(queue.Push(CanvasCommand{CanvasCommandKind::Action,std::string(16384,'x')}));
    queue.Take();
    std::cout<<"Canvas queue: commands ahead of held pointer samples leave in order and release their capacity\n";

    CanvasQueryQueue queries;
    auto payload=std::make_shared<int>(7);std::weak_ptr<int> lifetime=payload;
    for(size_t i=0;i<CanvasQueryQueue::MaxItems;i++)
        assert(queries.Push({i==3?CanvasQueryKind::LayerMenu:CanvasQueryKind::Thumbnails,std::to_string(i),
            [payload](PreviewPacket){}}));
    payload.reset();
    CanvasQuery extra{CanvasQueryKind::Filters,"retry",{}};
    assert(!queries.Push(std::move(extra)));assert(extra.json=="retry");
    assert(queries.Take()->json=="3");
    for(auto expected:{"0","1","2","4","5","6","7"})assert(queries.Take()->json==expected);
    assert(queries.Empty()&&!queries.Take()&&lifetime.expired());
    extra.json.reserve(CanvasQueryQueue::MaxPayload+1);
    assert(!queries.Push(std::move(extra)));assert(extra.json=="retry");
    assert(queries.Push({CanvasQueryKind::Filters,"discard",{}}));queries.Clear();
    assert(queries.Empty());
    assert(queries.Push({CanvasQueryKind::LayerMenu,"old",{}}));
    assert(queries.Push({CanvasQueryKind::Thumbnails,"pixels",{}}));
    assert(queries.Push({CanvasQueryKind::LayerMenu,"latest",{}}));
    assert(queries.Take()->json=="latest");assert(queries.Take()->json=="pixels");assert(queries.Empty());
    // Workspace geometry must not replace another view's pending callback.
    // It shares the bounded FIFO with readbacks; context menus retain priority.
    assert(queries.Push({CanvasQueryKind::Workspace,"drawer",{}}));
    assert(queries.Push({CanvasQueryKind::Thumbnails,"thumbnail",{}}));
    assert(queries.Push({CanvasQueryKind::Workspace,"stats",{}}));
    assert(queries.Push({CanvasQueryKind::LayerMenu,"context",{}}));
    for(auto expected:{"context","drawer","thumbnail","stats"})assert(queries.Take()->json==expected);
    assert(queries.Empty());
    assert(queries.Push({CanvasQueryKind::Thumbnails,"thumbnail",{}}));
    assert(queries.Push({CanvasQueryKind::Workspace,"first",{}}));
    assert(queries.Push({CanvasQueryKind::LayerMenu,"menu",{}}));
    assert(queries.Push({CanvasQueryKind::Workspace,"second",{}}));
    assert(!queries.Take(CanvasQueryKind::Filters));
    assert(queries.Take(CanvasQueryKind::Workspace)->json=="first");
    assert(queries.Take(CanvasQueryKind::Workspace)->json=="second");
    assert(!queries.Take(CanvasQueryKind::Workspace));
    assert(queries.Take()->json=="menu");assert(queries.Take()->json=="thumbnail");assert(queries.Empty());
    CanvasQuerySlots slots;
    std::vector<std::shared_ptr<void>> held;
    std::deque<std::function<void()>> deferred;
    for(size_t i=0;i<CanvasQueryQueue::MaxItems;++i){held.push_back(slots.Acquire());assert(held.back());}
    assert(!slots.Acquire());
    for(auto const& slot:held)deferred.push_back([slot]{});
    held.clear();assert(!slots.Acquire());
    deferred.pop_front();auto replacement=slots.Acquire();assert(replacement&&!slots.Acquire());
    deferred.clear();replacement.reset();
    for(size_t i=0;i<CanvasQueryQueue::MaxItems;++i){held.push_back(slots.Acquire());assert(held.back());}
    held.clear();
    std::cout<<"Canvas queries: menu priority, FIFO geometry/pixels, bounded retained allocation, retry ownership and disposal passed\n";

    std::vector<std::string> sent;
    GamepadState pad([&](std::string json){sent.push_back(std::move(json));});
    auto start=GamepadState::Clock::now();
    auto at=[&](int ms){return start+std::chrono::milliseconds(ms);};
    auto count=[&](char const* text){return size_t(std::count_if(sent.begin(),sent.end(),[&](auto const& json){return json.find(text)!=std::string::npos;}));};
    pad.Read(GamepadSample{},at(0));
    assert(sent.empty());
    pad.Read(GamepadSample{1u},at(0));
    assert(sent.size()==1&&count(R"("key":"gamepad_a","pressed":true,"repeat":false)")==1);
    pad.Read(GamepadSample{1u},at(499));
    assert(sent.size()==1);
    pad.Read(GamepadSample{1u},at(500));pad.Read(GamepadSample{1u},at(549));pad.Read(GamepadSample{1u},at(550));
    assert(count(R"("key":"gamepad_a","pressed":true,"repeat":true)")==2);
    pad.Read(GamepadSample{},at(560));
    assert(count(R"("key":"gamepad_a","pressed":false)")==1);
    sent.clear();
    for(double value:{.49,.5,.31,.3})pad.Read(GamepadSample{0,value},at(600));
    assert(count(R"("gamepad_l2","pressed":true)")==1&&count(R"("gamepad_l2","pressed":false)")==1&&sent.size()==2);
    sent.clear();
    pad.Read(GamepadSample{0,0,0,.123,-1.7,.5},at(700));pad.Read(GamepadSample{0,0,0,.1249,-1.2,.5},at(716));
    assert(sent.size()==1&&sent[0]==R"({"type":"axes","pan":[0.12,1.00],"zoom":0.50})");
    pad.Read(GamepadSample{1u<<13,0,0,std::numeric_limits<double>::quiet_NaN(),.004},at(720));
    pad.Release(at(730));
    assert(count(R"("gamepad_right","pressed":false)")==1&&count(R"({"type":"axes","pan":[0.00,0.00],"zoom":0.00})")==1&&count("-0.00")==0);
    sent.clear();pad.Release(at(740));
    assert(sent.empty());
    std::cout<<"Gamepad buttons, 500/50 ms repeats, trigger hysteresis, clamped rounded axes and release passed\n";
}
