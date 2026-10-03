#include "../CanvasWorkBuffer.h"
#include "../TextCompositionKeys.h"
#include "../CanvasPointerSample.h"
#include <limits>
#include "../CanvasQueryQueue.h"
#include "../CanvasSnapshotMailbox.h"
#include "../WorkspacePublication.h"
#include "../GamepadState.h"
#include <algorithm>
#include <string>
#include <vector>
#include <cassert>
#include <iostream>
#ifdef _WIN32
#include "../TraceFile.h"
#endif

int main() {
#ifdef _WIN32
    auto traceFolder=std::filesystem::temp_directory_path()/(L"capy-trace-"+std::to_wstring(GetCurrentProcessId())+L"-"+std::to_wstring(GetTickCount64()));
    assert(std::filesystem::create_directory(traceFolder));
    auto trace=traceFolder/L"windows.json";
    WriteTraceFile(trace,"old");
    auto reader=CreateFileW(trace.c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,nullptr);
    assert(reader!=INVALID_HANDLE_VALUE);
    WriteTraceFile(trace,"current");
    std::ifstream currentTrace(trace);std::string currentValue;currentTrace>>currentValue;
    assert(currentValue=="current");assert(!std::filesystem::exists(trace.wstring()+L".pending"));
    char retained[3];DWORD read=0;assert(ReadFile(reader,retained,3,&read,nullptr)&&read==3);assert(std::string(retained,3)=="old");
    currentTrace.close();assert(CloseHandle(reader));
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

    for(uint32_t phase=0;phase<=4;++phase) {
        assert(CanvasPointerPhase(phase,false)==phase);
        assert(CanvasPointerPhase(phase,true)==4);
    }
    std::cout<<"Canvas canceled samples retain cancellation across every native event phase\n";
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
    assert(!publication.Accept(false,10,4)); // Motion cannot establish models.
    assert(publication.Accept(true,10,4));
    assert(publication.Accept(false,12,4));
    assert(!publication.Accept(false,11,4)); // An older queued placement is stale.
    assert(!publication.Accept(false,13,5)); // A new model is required first.
    assert(publication.Revision()==12&&publication.ModelRevision()==4);
    assert(!publication.Accept(true,11,4)); // Nor can an old full snapshot rewind it.
    assert(publication.Accept(true,13,5));
    assert(!publication.Accept(false,14,4)); // Old content must never be moved.
    assert(publication.Accept(true,13,5)); // Host visibility/size refresh.
    assert(publication.Accept(false,14,5));
    std::cout<<"Workspace publication: matching models, ordered motion and host refresh passed\n";
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
    snapshots.Push("search query",false,false,{},true);
    snapshots.Push("models F (search closed)",true,true);
    shown=snapshots.Take();
    assert(shown.full=="models F (search closed)"&&shown.search.empty());
    shown=snapshots.Take();
    assert(shown.full.empty()&&shown.workspace.empty()&&shown.camera.empty());
    std::cout<<"Canvas presentation: retained models, independent motion/camera coalescing and completion boundaries passed\n";
    snapshots.Push("translated models",true,true,{},false,"Japanese context");
    snapshots.Push("newer models",true,true);
    shown=snapshots.Take();
    assert(shown.full=="newer models"&&shown.localization=="Japanese context");
    snapshots.Push("Chinese models",true,true,{},false,"Chinese context");
    snapshots.Push("Korean models",true,true,{},false,"Korean context");
    shown=snapshots.Take();assert(shown.localization=="Korean context");
    assert(snapshots.Take().localization.empty());
    struct Presentation {size_t fields;std::string value;size_t Size()const{return fields;}};
    auto encode=[](Presentation const& value){return value.value;};
    snapshots.Push("localized model",true,true,{},false,CanvasSnapshotMailbox::Localization(Presentation{3,"French context"},encode));
    snapshots.Push("new model with null localization",true,true,{},false,CanvasSnapshotMailbox::Localization(Presentation{0,"{}"},encode));
    shown=snapshots.Take();assert(shown.full=="new model with null localization"&&shown.localization=="French context");
    snapshots.Push("incremental localization",false,true,{},false,CanvasSnapshotMailbox::Localization(Presentation{3,"Thai context"},encode));
    snapshots.Push("incremental search without localization",false,false,{},true,CanvasSnapshotMailbox::Localization(Presentation{0,"{}"},encode));
    shown=snapshots.Take();assert(shown.workspace=="incremental localization"&&shown.search=="incremental search without localization"&&shown.localization.empty());
    assert(snapshots.Take().localization.empty());
    snapshots.Push("complete translated model",true,true);
    shown=snapshots.Take();assert(shown.full=="complete translated model"&&shown.localization=="Thai context");
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
