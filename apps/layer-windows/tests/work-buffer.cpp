#include "../CanvasWorkBuffer.h"
#include "../CanvasSurfaceState.h"
#include "../TextCompositionKeys.h"
#include "../NativeKeyRepeats.h"
#include "../CanvasPointerSample.h"
#include "../ChildOrder.h"
#include "../PenSourceCache.h"
#include "../CanvasContactGate.h"
#include "../PointerCompletion.h"
#include "../PointerModality.h"
#include "../ThumbnailOwners.h"
#include "../PointerScrub.h"
#include "../PressRepeat.h"
#include "../BubbleMotion.h"
#include <limits>
#include "../CanvasQueryQueue.h"
#include "../CanvasLatencyTrace.h"
#include "../CanvasSnapshotMailbox.h"
#include "../CanvasModelSections.h"
#include "../WorkspacePublication.h"
#include "../WorkspaceSurfaceGeometry.h"
#include "../NativeJson.h"
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
    {
        CapyUi::TabShapeInputs shape{100,30,{-6,-6}};
        auto resized=shape;resized.width=120;assert(resized!=shape);
        auto corners=shape;corners.bottom[0]=0;assert(corners!=shape);
        assert((shape==CapyUi::TabShapeInputs{100,30,{-6,-6}}));
        using Inputs=CapyUi::TabSurfaceInputs<std::array<float,2>,std::array<unsigned char,4>>;
        Inputs surface{100,30,0,4,{{0,0},{100,30}},{{6,0},{30,30}},{255,10,20,30},{255,40,50,60}};
        assert(surface==Inputs(surface));
        for(unsigned field=0;field<8;++field){
            auto next=surface;
            switch(field){
                case 0:++next.width;break;case 1:++next.height;break;
                case 2:++next.clipLeft;break;case 3:++next.clipRight;break;
                case 4:++next.band[0][0];break;case 5:++next.tab[0][1];break;
                case 6:++next.strip[0];break;case 7:++next.panel[2];break;
            }
            assert(surface!=next);
        }
        CapyUi::ResizeHandleInputs<std::wstring> handle{true,17,L"top_left",false};
        for(unsigned field=0;field<4;++field){
            auto next=handle;
            switch(field){case 0:next.floating=false;break;case 1:++next.id;break;case 2:next.edge=L"bottom_right";break;case 3:next.resetColumn=true;break;}
            assert(handle!=next);
        }
    }
#ifdef _WIN32
    winrt::init_apartment();
    {
        using J=winrt::Windows::Data::Json::JsonObject;
        std::string identity="{\"process_id\":17,\"window_id\":23";
        CapyUi::CameraTrace trace;
        auto receipt=[&](wchar_t const* packet,bool full){
            auto bytes=trace.Payload(J::Parse(packet),full,identity);
            return bytes.empty()?J{nullptr}:J::Parse(winrt::to_hstring(bytes));
        };
        auto revisions=[](J const& value){return std::pair{value.GetNamedObject(L"camera").GetNamedNumber(L"revision"),value.GetNamedObject(L"navigation",J{}).GetNamedNumber(L"revision",-1)};};
        auto startup=receipt(LR"({"state":{"camera":{"revision":1}},"windows_tabs":null,"windows_glass":{}})",true);
        assert(startup.GetNamedObject(L"camera").GetNamedNumber(L"revision")==1&&!startup.HasKey(L"navigation"));
        auto full=receipt(LR"({"state":{"camera":{"revision":5,"zoom":0.657383918762207}},"windows_tabs":{"selected":2,"session_stamps":[{"id":1,"stamp":{"artwork":"other","epoch":8,"camera_navigation":90}},{"id":2,"stamp":{"artwork":"drawing","epoch":0,"camera_navigation":2}}]},"windows_glass":{}})",true);
        auto navigation=full.GetNamedObject(L"navigation");
        assert(navigation.GetNamedNumber(L"drawing_id")==2&&navigation.GetNamedString(L"artwork")==L"drawing"&&navigation.GetNamedNumber(L"epoch")==0);
        assert(full.GetNamedNumber(L"process_id")==17&&full.GetNamedNumber(L"window_id")==23);
        assert((revisions(full)==std::pair{5.,2.}));
        auto pan=receipt(LR"({"camera":{"revision":8,"zoom":0.657383918762207},"revision":40,"windows_glass":{}})",false);
        assert((revisions(pan)==std::pair{8.,5.}));
        assert(pan.GetNamedObject(L"navigation").GetNamedString(L"artwork")==L"drawing");
        assert(!receipt(LR"({"workspace_update":{"model_revision":30},"windows_tabs":{"selected":2,"session_stamps":[{"id":2,"stamp":{"artwork":"drawing","epoch":0,"camera_navigation":5}}]},"windows_glass":{}})",false));
        auto fit=receipt(LR"({"workspace_update":{"model_revision":30},"camera":{"revision":9},"windows_tabs":{"selected":2,"session_stamps":[{"id":2,"stamp":{"artwork":"drawing","epoch":0,"camera_navigation":6}}]},"windows_glass":{}})",false);
        assert((revisions(fit)==std::pair{9.,6.}));
        auto resized=receipt(LR"({"layout":{},"camera":{"revision":11},"windows_tabs":{"selected":2,"session_stamps":[{"id":2,"stamp":{"artwork":"drawing","epoch":0,"camera_navigation":6}}]},"windows_glass":{}})",false);
        assert((revisions(resized)==std::pair{11.,6.}));
        assert((revisions(receipt(LR"({"camera":{"revision":12},"revision":44,"windows_glass":{}})",false))==std::pair{12.,7.}));
        assert(!receipt(L"{}",false));
        receipt(LR"({"state":{"camera":{"revision":13}},"windows_tabs":null,"windows_glass":{}})",true);
        assert(!receipt(LR"({"camera":{"revision":14},"revision":46,"windows_glass":{}})",false).HasKey(L"navigation"));
    }
    std::cout<<"Camera traces carry the active navigation through camera-only packets\n";
#endif
    CapyUi::PointerModality modality;
    assert(!modality.Mouse());
    assert(modality.Observe(true,100)&&modality.Mouse());
    assert(!modality.Observe(true,200));
    assert(!modality.Observe(false,150)&&modality.Mouse());
    assert(modality.Observe(false,300)&&!modality.Mouse());
    assert(modality.Observe(true,400)&&modality.Mouse());
    assert(!modality.Observe(false,350)&&modality.Mouse());
    assert(modality.Observe(false,500)&&!modality.Mouse());
    std::thread older([&]{for(uint64_t i=600;i<700;++i)modality.Observe(true,i);});
    std::thread newer([&]{for(uint64_t i=700;i<800;++i)modality.Observe(false,i);});
    older.join();newer.join();assert(!modality.Mouse());
    std::cout<<"Pointer modality: ordered native/routed contacts, coalesced latest kind and stale callbacks passed\n";
    {
        auto start=CanvasLaunchTiming::Clock::time_point{};
        CanvasLaunchTiming timing(start);
        auto delay=std::chrono::seconds(5);
        assert(timing.CaptionRemaining(start,delay)==delay);
        assert(timing.CaptionRemaining(start+delay/2,delay)==delay-delay/2);
        assert(timing.CaptionRemaining(start+delay-std::chrono::milliseconds(1),delay)==std::chrono::milliseconds(1));
        assert(timing.CaptionRemaining(start+delay,delay).count()==0);
        assert(timing.CaptionRemaining(start+delay+std::chrono::seconds(1),delay).count()==0);
    }
    {
        for(auto extent:std::vector<std::array<double,3>>{{1400,1000,252},{320,150,60},{150,320,60}}){
            CanvasLaunchPlacement placement(extent[0],extent[1],extent[2]);
            assert(placement.left>=0&&placement.top>=0);
            assert(placement.left+placement.size/2==extent[0]/2&&placement.top+placement.size/2==extent[1]/2);
            for(double scale:{1.,1.5,2.}){
                auto raster=std::ceil(placement.size*std::max(2.,scale));
                auto top=placement.CaptionTop(18,.923271,raster,scale);
                assert(top>=placement.top+placement.size*.923271+18);
                assert(top+20<=extent[1]);
                assert(std::abs(top*scale-std::round(top*scale))<1e-9);
            }
        }
    }
    {
        CanvasLaunchPaint paint;
        int surface;
        auto request=paint.Request(&surface,1);
        auto now=CanvasLaunchTiming::Clock::time_point{};
        std::optional<CanvasLaunchTiming::Clock::time_point> began;
        auto presented=[&](CanvasLaunchPaint::Receipt const& receipt){if(paint.Presented(receipt,&surface,1))began=now;};
        presented(request);assert(!began);
        now+=std::chrono::milliseconds(600);
        assert(paint.Rendered(request));assert(!began);
        assert(!paint.Request(&surface,1));assert(!paint.Rendered(request));
        now+=std::chrono::milliseconds(16);
        presented(request);assert(began==now);
        auto duration=std::chrono::milliseconds(500);
        assert(duration-(now-*began)==duration);
        now+=duration/2;assert(duration-(now-*began)==duration/2);
        presented(request);assert(*began+duration==now+duration/2);
    }
    {
        CanvasLaunchPaint paint;
        int first,second;
        auto retired=paint.Request(&first,1);
        assert(paint.Rendered(retired));paint.Cancel();
        auto current=paint.Request(&first,1);
        assert(current!=retired&&!paint.Rendered(retired));
        assert(!paint.Presented(retired,&first,1));
        assert(!paint.Presented(current,&first,1));
        assert(paint.Rendered(current));
        assert(!paint.Presented(current,&second,1)&&!paint.Presented(current,&first,2));
        auto replacement=paint.Request(&second,2);
        assert(!paint.Presented(current,&first,1));
        assert(paint.Rendered(replacement)&&paint.Presented(replacement,&second,2));
        auto closing=paint.Request(&second,2);paint.Cancel();
        assert(!paint.Rendered(closing)&&!paint.Presented(closing,&second,2));
    }
    CanvasSurfaceState surface;
    surface.Reset(0);
    assert(!surface.CanAttach(0,true)&&!surface.CanAttach(1,false));
    assert(surface.CanAttach(1,true));surface.DidAttach();
    assert(surface.Attached()&&!surface.CanAttach(1,false)&&!surface.CanAttach(1,true));
    surface.Reset(4);
    assert(!surface.Attached()&&!surface.CanAttach(4,true)&&!surface.CanAttach(5,false));
    assert(surface.CanAttach(5,true)&&surface.CanAttach(6,true));surface.DidAttach();
    assert(surface.Attached()&&!surface.CanAttach(4,true)&&!surface.CanAttach(5,false));
    std::cout<<"Surface readiness: stale generations rejected and attachment retained until explicit reset passed\n";
    {
        auto input=CapyUi::JsonUnsignedFields("{\"type\":\"color_touch\",\"position\":[1.5,2.5]}",
            {{"surface",9007199254740993ULL},{"id",4294967295ULL},{"time_ns",1101152828577000ULL}});
        assert(input=="{\"type\":\"color_touch\",\"position\":[1.5,2.5],\"surface\":9007199254740993,\"id\":4294967295,\"time_ns\":1101152828577000}");
        assert(CapyUi::JsonUnsignedFields("{}",{{"time_ns",UINT64_MAX}})=="{\"time_ns\":18446744073709551615}");
    }
    std::cout<<"Native JSON preserves unsigned contact identities and long-uptime timestamps\n";
    for(float scale:{1.f,1.5f,2.f}){
        std::array<float,4> radii{18,0,12,6};
        auto shadow=CapyUi::shadowRaster(242,400,radii,scale,12,2);
        assert(shadow==CapyUi::shadowRaster(1200,900,radii,scale,12,2));
        assert(shadow.padding==48*scale&&shadow.insets[0]/scale==114);
        auto asymmetric=CapyUi::shadowRaster(500,500,{0,100,0,0},scale,12,2);
        assert(asymmetric.panel[0]>=200*scale&&asymmetric.panel[1]>=200*scale);
        auto shortPanel=CapyUi::shadowRaster(21,72,radii,scale,12,2);
        assert(shortPanel.panel[0]==21*scale&&shortPanel.panel[1]==72*scale);
        assert(shortPanel.radii[0]==10.5f*scale&&shortPanel.radii[2]==10.5f*scale);
        assert(shortPanel.insets[0]+shortPanel.insets[2]==shortPanel.extent[0]);
        assert(shortPanel!=CapyUi::shadowRaster(22,72,radii,scale,12,2));
        assert(shadow!=CapyUi::shadowRaster(242,400,{18,0,0,6},scale,12,2));
    }
    std::cout<<"Panel shadows: fixed raster under stretch, physical corner caps and exact short axes passed\n";
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
    {
        CanvasContactGate gate;
        gate.Block(1,0,0);
        assert(gate.Admit(1,1,1,false));assert(gate.Admit(1,2,2,false));assert(gate.Admit(1,3,3,false));
        gate.Block(1,1,1);
        assert(!gate.Admit(1,2,2,false));assert(!gate.Admit(1,3,3,false));assert(gate.Admit(1,1,1,false));
        gate.Block(2,2,2);
        assert(!gate.Admit(2,2,2,false));assert(gate.Admit(2,1,1,false));
        assert(!gate.Admit(3,1,1,true));assert(!gate.Admit(3,2,3,false));assert(gate.Admit(3,1,3,false));
        std::cout<<"Dialog-blocked hover leaves the next canvas press admitted; consumed contacts end at release or the next press\n";
    }
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

    for(uint32_t key:{13u,27u,40u,65u}) {
        TextCompositionKeys input;
        assert(!input.Owns(key,1,false,false)&&!input.Busy());
        assert(!input.Owns(key,2,true,true)&&!input.Busy());
        input.Update(true);assert(input.Owns(229,10,false,false,28));input.Update(false);
        assert(!input.Owns(key,10,false,true,28)&&input.Busy());
        assert(!input.Owns(key,10,false,true,28));
        assert(!input.Owns(key,40,false,true,28)&&input.Busy());
        input.Clear();input.Pointer();assert(input.Busy()&&!input.Active());
        assert(input.Owns(229,45,true,true,28)&&input.Busy());
        assert(!input.Owns(key,50,true,true,28)&&!input.Busy());
        assert(!input.Owns(key,50,true,true,28));
        assert(!input.Owns(key,60,false,false)&&!input.Busy());
        assert(!input.Owns(key,70,true,true)&&!input.Busy());
        input.Update(true);assert(input.Owns(key,90,false,false));
        input.Update(false);assert(input.Owns(key,90,false,false));
        assert(input.Owns(key,110,false,true)&&input.Busy());
        input.Clear();assert(!input.Busy());
        assert(!input.Owns(key,120,true,true)&&!input.Busy());
        assert(!input.Owns(key,130,false,false)&&!input.Busy());
        assert(!input.Owns(key,140,true,true)&&!input.Busy());
        assert(input.Owns(229,150,false,false)&&input.Busy());
        input.Clear();assert(!input.Busy());
        assert(input.Owns(229,160,true,true)&&!input.Busy());
        assert(input.Owns(229,170,false,false,28)&&input.Busy());
        assert(input.Owns(key,180,true,true,28)&&!input.Busy());
        assert(input.Owns(229,190,false,false,28));
        assert(!input.Owns(key,190,false,true,28)&&input.Busy());
        assert(!input.Owns(key,190,false,true,28));
        assert(!input.Owns(key,200,true,true,28)&&!input.Busy());
        assert(input.Owns(229,210,false,false,28));
        assert(!input.Owns(key,210,false,true,28)&&input.Busy());
        assert(input.Owns(229,220,true,true,28)&&input.Busy());
        assert(!input.Owns(key,230,false,true,28)&&input.Busy());
        assert(!input.Owns(key,240,true,true,28)&&!input.Busy());
        input.Update(true);assert(input.Owns(229,0xfffffff0,false,false,28));input.Update(false);
        assert(!input.Owns(key,0xfffffff0,false,true,28)&&input.Busy());
        assert(!input.Owns(key,0x11,true,true,28)&&!input.Busy());
    }
    std::cout<<"Passed IME keys remain busy through physical release; ordinary keys stay idle and active or Process keys stay owned\n";

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
    {
        PointerCancelRegistry contacts;int row,scroll;int rowCanceled=0,scrollCanceled=0,otherCanceled=0;
        contacts.Observe(1,&row,[&]{++rowCanceled;});contacts.Observe(1,&scroll,[&]{++scrollCanceled;});
        contacts.Observe(2,&row,[&]{++otherCanceled;});
        contacts.Forget(1,&row);contacts.Cancel(1);
        assert(rowCanceled==0&&scrollCanceled==1&&otherCanceled==0);
        contacts.Observe(1,&row,[&]{++rowCanceled;});contacts.Cancel(1);assert(rowCanceled==1);
        contacts.Cancel(2);assert(otherCanceled==1);
    }
    std::cout<<"Capture transfer cancels every native owner once while preserving reentrant and unrelated contacts\n";

    {
        PointerCancelRegistry header,workspace;int scrub=0,dial=0,slider=0,next=0;
        int scrubValue=18,dialValue=12,sliderValue=24,cancels=0;
        header.Observe(1,&scrub,[&]{scrubValue=9;++cancels;header.Forget(1,&scrub);header.Cancel(2);
            header.Observe(1,&next,[&]{++cancels;});});
        header.Observe(2,&dial,[&]{dialValue=18;++cancels;header.Forget(1,&scrub);});
        workspace.Observe(1,&slider,[&]{sliderValue=20;++cancels;workspace.Cancel(1);});
        assert(header.CancelAll()&&workspace.CancelAll());
        assert(scrubValue==9&&dialValue==18&&sliderValue==20&&cancels==3);
        assert(!workspace.CancelAll());
        header.Cancel(1);assert(cancels==4);
        assert(!header.CancelAll());
    }
    std::cout<<"Native lifecycle retirement restores every presenter contact and preserves future ownership\n";

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

    {
        struct Point {double X,Y;Point Position()const{return *this;}};
        PointerMovement movement;
        movement.Begin(76,813.33331298828125,1.5,6,6);
        movement.Move(std::vector<Point>{{78.66666412353516,816}});
        assert(!movement.Moved());
        for(double scale:{1.,1.5,2.}){
            movement.Begin(21.25,35.75,scale,6,8);
            movement.Move(std::vector<Point>{{21.25+4/scale,35.75+4/scale},{21.25,35.75}});
            assert(!movement.Moved());
            movement.Sample(21.25+6/scale,35.75+8/scale);assert(!movement.Moved());
            movement.Move(std::vector<Point>{{21.25,35.75},{21.25+7/scale,35.75},{21.25,35.75}});
            assert(movement.Moved());
            movement.Begin(21.25,35.75,scale,6,8);
            movement.Move(std::vector<Point>{{21.25,35.75},{21.25,35.75-9/scale},{21.25,35.75}});
            assert(movement.Moved());
            movement.Begin(21.25,35.75,scale,6,8);
            movement.Move(std::vector<Point>{{21.25,35.75},{21.25-7/scale,35.75},{21.25,35.75}});
            assert(movement.Moved());
            movement.Reset();assert(!movement.Moved());
            movement.Recognize();assert(movement.Moved());
        }
    }
    std::cout<<"Pointer movement: recorded short mouse travel stays inside DPI-scaled native bounds; every history sample can latch movement\n";

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

    {
        CapyUi::PressRepeat press;CapyUi::PressRepeat::Limits limits{500000,8};
        assert(!press.Next(limits,1000000,0,-12));
        assert(!press.Next(limits,1200000,-12,0));
        assert(press.Next(limits,1300000,-10,3));
        assert(!press.Next(limits,1400000,-10,3));
        assert(!press.Next(limits,2000000,0,0,1)&&!press.Next(limits,2100000,0,0,2)&&press.Next(limits,2200000,1,1,2));
        assert(!press.Next(limits,3000000,0,0)&&!press.Next(limits,3500001,0,0));
        press.Reset();assert(!press.Next(limits,3600000,0,0));
    }
    std::cout<<"Double presses need the second press within the double-click time and rectangle of the first\n";

    {
        CapyBubbles::Tween card{1};card.to(0,150);
        CapyBubbles::MotionTicks ticks;ticks.polling=true;
        double asked=0;std::vector<double> fading;
        for(int frame=0;frame<=30;++frame){
            auto now=frame*1000./60;if(ticks.busy&&now-asked>=200)ticks.replied(true);
            ticks.tick(now,[&](double at){bool moving=card.step(at,false);if(card.value>0&&card.value<1)fading.push_back(at);return moving;},[&]{asked=now;return true;});
        }
        assert(fading.size()>=8&&fading.back()<150&&card.value==0&&"A retiring card must fade on every frame while motion replies are 200 ms apart");
        ticks.replied(false);
        assert(!ticks.tick(1000,[&](double at){return card.step(at,false);},[]{return true;})&&"Idle bubbles must stop their frames");
    }
    std::cout<<"Retiring bubble parts fade on every frame however late motion replies arrive, and idle bubbles stop their frames\n";

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
    struct HistoryPoint {
        uint32_t frame;uint64_t time;
        uint32_t FrameId()const{return frame;}
        uint64_t Timestamp()const{return time;}
    };
    struct HistoryPoints {
        std::vector<HistoryPoint> points;
        HistoryPoint GetAt(uint32_t i)const{return points.at(i);}
    };
    HistoryPoint newest{42,7000};
    assert(CanvasPointerHistoryNewestFirst(HistoryPoints{{{42,7000},{41,7000},{40,7000}}},3,newest));
    assert(!CanvasPointerHistoryNewestFirst(HistoryPoints{{{40,7000},{41,7000},{42,7000}}},3,newest));
    assert(!CanvasPointerHistoryNewestFirst(HistoryPoints{{{42,7000}}},1,newest));
    std::cout<<"Native pointer history order follows input frames when timestamps tie\n";
    {
        enum class Device { Mouse, Touch, Pen };
        struct Case {Device device;uint32_t tools[4],eraserReads[4],invertedReads[4];};
        Case cases[]={
            {Device::Mouse,{1,1,1,1},{0,0,0,0},{0,0,0,0}},
            {Device::Touch,{3,3,3,3},{0,0,0,0},{0,0,0,0}},
            {Device::Pen,{0,2,2,2},{1,1,1,1},{1,0,1,0}}};
        for(auto const& test:cases)for(unsigned flags=0;flags<4;++flags){
            uint32_t eraserReads=0,invertedReads=0;
            auto tool=CanvasPointerTool(test.device,[&]{++eraserReads;return bool(flags&1);},[&]{++invertedReads;return bool(flags&2);});
            assert(tool==test.tools[flags]&&"Native pointer tool mapping ignored eraser or inverted identity");
            assert(eraserReads==test.eraserReads[flags]&&invertedReads==test.invertedReads[flags]&&"Native pointer flag getter short-circuit changed");
        }
        assert(CanvasEraserReplayPosition(10,20,200,100));
        for(auto x:{-1.f,128.f,std::numeric_limits<float>::infinity(),std::numeric_limits<float>::quiet_NaN()})
            assert(!CanvasEraserReplayPosition(x,20,200,100));
        assert(!CanvasEraserReplayPosition(10,-1,200,100)&&!CanvasEraserReplayPosition(10,100,200,100));
        CapyView view{};view.revision=17;
        uint32_t milliseconds[]={0,35,35,45,55,65,75,85,95,105,115,125,135,145,155,165,175,185,195,205,217};
        for(uint32_t i=0;i<=20;++i){
            auto p=CanvasEraserReplaySample(view,10,20,i);
            assert(p.view_revision==17&&p.tool==2&&p.flags==2&&p.button==0&&p.pressure==0.5f&&p.tilt_x==0&&p.tilt_y==0&&p.twist==0);
            assert(p.phase==(i==0?0u:i==1?1u:i==20?3u:2u)&&p.x==10+4*float(i==0?0:std::min(i-1,18u))&&p.y==20);
            assert(p.timestamp_ns==milliseconds[i]*1000000ULL);
        }
    }
    std::cout<<"Canvas tools preserve all eraser/inverted flags; isolated eraser samples preserve the pen stroke geometry and timing\n";

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
    {
        auto camera=[](float zoom,float rotation,bool flip,float x,float y,float width,float height){
            float sx=flip?-zoom:zoom;
            return CapyView{9,{std::cos(rotation)*sx,std::sin(rotation)*sx,-std::sin(rotation)*zoom,std::cos(rotation)*zoom,x,y},{width,height}};
        };
        auto inside=[](CapyView const& view,CapyPointer const& p){
            auto const& m=view.document_to_surface;float det=m[0]*m[3]-m[1]*m[2],x=p.x-m[4],y=p.y-m[5];
            float u=(m[3]*x-m[2]*y)/det,v=(m[0]*y-m[1]*x)/det;
            return u>0&&u<view.document[0]&&v>0&&v<view.document[1];
        };
        for(auto view:{camera(3.3257809f,0,false,709.65f,889.36f,128,96),camera(.35f,.7f,true,1500,-120,2048,1536),camera(1,-1.9f,false,240,1100,32,24)}){
            for(bool pen:{false,true})for(uint32_t i=0;i<42;i++){
                auto p=CanvasReplaySample(view,i,42,pen,false,false);
                assert(inside(view,p)&&p.view_revision==view.revision&&p.phase==(i==0?1u:i==41?3u:2u));
            }
            for(uint32_t i=0;i<32768;i+=97)assert(inside(view,CanvasReplaySample(view,i,32768,false,false,true)));
            assert(inside(view,CanvasReplaySample(view,0,3,false,true,false)));
        }
    }
    std::cout<<"Test replays stay inside the drawing under off-centre, rotated and flipped cameras\n";
    {
        struct Items{
            std::vector<int> values,removed;
            uint32_t Size()const{return uint32_t(values.size());}
            int GetAt(uint32_t index)const{return values[index];}
            bool IndexOf(int value,uint32_t& at)const{auto found=std::find(values.begin(),values.end(),value);at=uint32_t(found-values.begin());return found!=values.end();}
            void RemoveAt(uint32_t index){removed.push_back(values[index]);values.erase(values.begin()+index);}
            void InsertAt(uint32_t index,int value){values.insert(values.begin()+index,value);}
        };
        std::vector<int> start{1,2,3,4};
        for(auto target:std::vector<std::vector<int>>{{1,2,3,4},{1,2,3,4,5},{1,2,4}})do{
            for(int focused:start){
                Items items{start,{}};
                for(uint32_t index=0;index<target.size();++index)PlaceChild(items,index,target[index],[&](int value){return value==focused;});
                assert(std::equal(target.begin(),target.end(),items.values.begin())&&items.Size()==std::max(target.size(),start.size()));
                assert(std::find(items.removed.begin(),items.removed.end(),focused)==items.removed.end());
            }
        }while(std::next_permutation(target.begin(),target.end()));
    }
    std::cout<<"Reordering native children never removes the focused child\n";
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
    assert(CanvasSections({L"state",L"histogram",L"status"})==CanvasScopes);
    assert(CanvasSections({L"windows_scopes"})==CanvasScopes);
    assert(CanvasSections({L"state",L"revision"})==0);
    assert(CanvasSections({L"workspace_update",L"content_revision"})==0);
    assert(CanvasSections({L"windows_workspace",L"busy"})==(CanvasHeader|CanvasWindowState|CanvasDialogs));
    assert(CanvasSections({L"layout",L"groups"})==CanvasAllSections);
    for(auto initialSections:{uint32_t(0),uint32_t(CanvasWindowState)}){
        CanvasSnapshotMailbox acceptedModels;auto pendingSections=initialSections;
        acceptedModels.Push("close-ready",true,true);
        auto rejectedModel=acceptedModels.BeginDecode();assert(rejectedModel);
        assert(!acceptedModels.Posted(rejectedModel->sequence,false,true,pendingSections));
        assert(pendingSections==initialSections&&!acceptedModels.IsPosted(rejectedModel->sequence));
        acceptedModels.CompleteDecode();acceptedModels.Push("camera",false,false,"camera");
        auto patch=acceptedModels.BeginDecode();assert(patch&&patch->full.empty());
        assert(!acceptedModels.Posted(patch->sequence,true,false,pendingSections));
        assert(pendingSections==initialSections&&!acceptedModels.IsPosted(rejectedModel->sequence));
        acceptedModels.CompleteDecode();acceptedModels.Push("accepted model",true,true);
        auto acceptedModel=acceptedModels.BeginDecode();assert(acceptedModel);pendingSections|=CanvasScopes;
        assert(acceptedModels.Posted(acceptedModel->sequence,true,true,pendingSections));
        assert(!pendingSections&&acceptedModels.IsPosted(rejectedModel->sequence)&&acceptedModels.IsPosted(acceptedModel->sequence));
        acceptedModels.CompleteDecode();
    }
    std::cout<<"Canvas models: rejected applications retain sections and replies across patches until an accepted model passed\n";
    CanvasSnapshotMailbox continuous;
    continuous.Push("base",true,true);
    uint64_t lastApplied=0;int applied=0,skipped=0;
    for(int frame=0;frame<20;++frame){
        auto decoded=continuous.BeginDecode();assert(decoded);
        continuous.Push("newer model",true,true);
        if(continuous.ApplyDecoded(true)){
            continuous.Posted(decoded->sequence);lastApplied=decoded->sequence;++applied;skipped=0;
        }else ++skipped;
        assert(skipped<=1);
        assert(decoded->sequence-lastApplied<=1);
        continuous.CompleteDecode();
    }
    assert(applied==10&&continuous.IsPosted(lastApplied));
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
    std::string measuredLayout="{\"type\":\"panel_bridges\",\"layout\":{\"groups\":[";
    for(int i=0;i<128;++i){
        if(i)measuredLayout+=',';
        measuredLayout+="{\"id\":"+std::to_string(i)+",\"bounds\":{\"x\":123.333333333333,\"y\":456.666666666667,\"width\":240,\"height\":320}}";
    }
    measuredLayout+="]},\"measure\":{\"groups\":[],\"columns\":[]}}";
    assert(measuredLayout.size()>8192);
    auto expectedLayout=measuredLayout;
    assert(queries.Push({CanvasQueryKind::Workspace,std::move(measuredLayout),{}}));
    assert(queries.Take()->json==expectedLayout&&queries.Empty());
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
    CanvasWorkBuffer idleWork;
    assert(NextCanvasRenderStep(false,idleWork,queries)==CanvasRenderStep::Wait&&NextCanvasRenderStep(true,idleWork,queries)==CanvasRenderStep::Frame);
    assert(CanvasAcquisitionWaitEnds(4,false,false,true)&&!CanvasAcquisitionWaitEnds(4,false,false,false));
    assert(!CanvasAcquisitionWaitEnds(0,false,false,true)&&CanvasAcquisitionWaitEnds(0,true,false,false)&&CanvasAcquisitionWaitEnds(4,false,true,false));
    for(auto kind:{CanvasQueryKind::Filters,CanvasQueryKind::Thumbnails,CanvasQueryKind::LayerMenu,CanvasQueryKind::Workspace,CanvasQueryKind::Document,CanvasQueryKind::Papers}){
        assert(queries.Push({kind,"retry",{}}));
        assert(NextCanvasRenderStep(false,idleWork,queries)==CanvasRenderStep::Answer);
        assert(NextCanvasRenderStep(true,idleWork,queries)==CanvasRenderStep::Frame);
        queries.Clear();
    }
    {
        assert(CanAnswerCanvasQueryAfterFrame(false));
        assert(!CanAnswerCanvasQueryAfterFrame(true));
        for(bool command:{false,true}){
            CanvasWorkBuffer pendingInput;
            if(command)assert(pendingInput.Push(CanvasCommand{CanvasCommandKind::Action,"held-ring"}));
            else assert(pendingInput.Push(std::vector<CapyPointer>(1)));
            CanvasQueryQueue afterFrame;
            assert(afterFrame.Push({CanvasQueryKind::Workspace,"bubble_motion",{}}));
            assert(afterFrame.Push({CanvasQueryKind::Workspace,"bubble_stamp",{}}));
            assert(NextCanvasRenderStep(false,pendingInput,afterFrame)==(command?CanvasRenderStep::Commands:CanvasRenderStep::Frame));
            assert(CanAnswerCanvasQueryAfterFrame(false)&&"Post-frame UI query must run despite pending input");
            size_t answered=0;
            auto service=[&](bool closing){if(CanAnswerCanvasQueryAfterFrame(closing))if(afterFrame.Take())++answered;};
            service(true);assert(answered==0&&!afterFrame.Empty()&&!pendingInput.Empty());
            service(false);assert(answered==1&&!afterFrame.Empty()&&!pendingInput.Empty());
            service(false);assert(answered==2&&afterFrame.Empty()&&!pendingInput.Empty());
            service(false);assert(answered==2);
        }
        CanvasWorkBuffer work;CanvasQueryQueue ordered;
        assert(ordered.Push({CanvasQueryKind::Workspace,"held-stamp",{},work.Pushed()}));
        assert(work.Push(CanvasCommand{CanvasCommandKind::Action,"toggle-sliders"}));
        assert(ordered.Push({CanvasQueryKind::Workspace,"menu-refresh",{},work.Pushed()}));
        auto earlier=ordered.Take({},work.Taken());assert(earlier&&earlier->json=="held-stamp");
        assert(!ordered.Take({},work.Taken())&&"A post-frame query must not overtake a command queued before it");
        assert(work.Take().size()==1);
        auto refreshed=ordered.Take({},work.Taken());assert(refreshed&&refreshed->json=="menu-refresh"&&ordered.Empty());
        assert(work.Push(std::vector<CapyPointer>(1)));
        assert(work.Push(CanvasCommand{CanvasCommandKind::Action,"behind-held-pointer"}));
        assert(ordered.Push({CanvasQueryKind::LayerMenu,"context",{},work.Pushed()}));
        assert(work.TakeBeforePointers().empty()&&!ordered.Take({},work.Taken()));
        assert(work.Take().size()==2);
        auto context=ordered.Take({},work.Taken());assert(context&&context->json=="context");
    }
    {
        CanvasWorkBuffer admitted;
        assert(admitted.Push(CanvasCommand{CanvasCommandKind::Document,R"({"operation":"close"})"}));
        assert(NextCanvasRenderStep(false,admitted,queries)==CanvasRenderStep::Commands);
        assert(NextCanvasRenderStep(true,admitted,queries)==CanvasRenderStep::Commands);
        auto admittedCommands=admitted.TakeBeforePointers();
        assert(admittedCommands.size()==1&&std::get<CanvasCommand>(admittedCommands.front()).kind==CanvasCommandKind::Document&&admitted.Empty());
        assert(NextCanvasRenderStep(false,admitted,queries)==CanvasRenderStep::Wait);
        assert(admitted.Push(std::vector<CapyPointer>(1)));
        assert(admitted.Push(CanvasCommand{CanvasCommandKind::Document,R"({"operation":"close"})"}));
        assert(NextCanvasRenderStep(false,admitted,queries)==CanvasRenderStep::Frame&&admitted.TakeBeforePointers().empty());
        auto ordered=admitted.Take();assert(ordered.size()==2&&std::holds_alternative<std::vector<CapyPointer>>(ordered.front())&&std::get<CanvasCommand>(ordered.back()).kind==CanvasCommandKind::Document);
    }
    std::cout<<"Canvas queries: an idle canvas answers every query kind without acquiring or presenting a frame\n";
    {
        auto at=[](int ms){return std::chrono::steady_clock::time_point{}+std::chrono::hours(1)+std::chrono::milliseconds(ms);};
        auto refresh=std::chrono::milliseconds(16);auto services=at(250);
        CanvasSurfacePoll poll;
        assert(poll.Wake(services)==services&&!poll.Due(at(1000)));
        poll.Schedule(true,at(0),refresh);
        assert(poll.Wake(services)==at(16)&&!poll.Due(at(15))&&poll.Due(at(16)));
        poll.Schedule(true,at(16),refresh);assert(poll.Wake(services)==at(32));
        poll.Schedule(false,at(32),refresh);assert(poll.Wake(services)==services&&!poll.Due(at(1000)));
        poll.Schedule(true,at(240),refresh);assert(poll.Wake(services)==services&&poll.Due(at(256)));
    }
    std::cout<<"Pending chrome acknowledgements wake the idle render loop at the next refresh, not the service tick\n";
    {
        auto origin=PresentationTrace::Clock::time_point{}+std::chrono::hours(1);
        auto at=[&](int ms){return origin+std::chrono::milliseconds(ms);};
        PresentationTrace presentation(true);
        presentation.Observe(true,true,false,4,3,at(0));assert(presentation.required==4);
        presentation.Observe(false,true,false,4,3,at(100));assert(presentation.required==5);
        presentation.Observe(true,false,false,5,3,at(500));assert(presentation.required==5);
        presentation.Observe(false,false,false,5,4,at(800));
        assert(!presentation.Withheld(at(1500))&&presentation.Withheld(at(1800)));
        presentation.Repeat();assert(!presentation.Withheld(at(5000)));
        presentation.Observe(true,false,true,6,4,at(5000));assert(presentation.required==5&&presentation.epoch==5);
        presentation.Observe(false,false,false,6,5,at(5100));assert(presentation.required==5&&*presentation.displayed==5&&!presentation.Withheld(at(9000)));
        presentation.Observe(true,false,false,7,5,at(9000));assert(presentation.required==7);
        presentation.Observe(false,false,false,7,7,at(9500));assert(!presentation.Withheld(at(20000)));
        presentation.Observe(true,false,false,8,std::nullopt,at(20000));assert(*presentation.displayed==7&&presentation.Withheld(at(21000)));
        PresentationTrace untraced(false);
        untraced.Observe(true,false,false,2,1,at(0));assert(!untraced.Withheld(at(60000))&&!untraced.Stalled(at(60000)));
    }
    std::cout<<"Presentation trace: a withheld final acknowledgement asks for one unchanged repeat that never raises the requirement\n";
    {
        auto origin=PresentationTrace::Clock::time_point{}+std::chrono::hours(1);
        auto at=[&](int ms){return origin+std::chrono::milliseconds(ms);};
        PresentationTrace presentation(true);
        presentation.Observe(true,false,false,60,58,at(0));
        presentation.Observe(false,false,false,60,58,at(250));
        assert(presentation.Withheld(at(1250))&&!presentation.Stalled(at(1250)));presentation.Repeat();
        presentation.Observe(true,false,true,61,59,at(1300));assert(!presentation.Stalled(at(2000)));
        for(int ms=1500;ms<=11500;ms+=250)presentation.Observe(false,false,false,61,59,at(ms));
        assert(presentation.required==60&&*presentation.displayed==59);
        assert(presentation.Stalled(at(11500))&&!presentation.Withheld(at(11500)));
        presentation.Observe(false,false,false,61,61,at(11750));
        assert(presentation.required==60&&*presentation.displayed==61&&!presentation.Stalled(at(20000))&&!presentation.Withheld(at(20000)));
    }
    std::cout<<"Presentation trace: a final present still unacknowledged after its repeat asks for a compositor flush, never a second repeat\n";
    {
        auto origin=PresentationTrace::Clock::time_point{}+std::chrono::hours(1);
        auto at=[&](int ms){return origin+std::chrono::milliseconds(ms);};
        int asked=0;
        auto quiet=[&]{++asked;return true;};
        auto busy=[&]{++asked;return false;};
        PresentationTrace untraced(false);
        untraced.Observe(true,false,false,2,1,at(0));
        for(int ms=0;ms<=60000;ms+=250){if(untraced.Withheld(at(ms)))untraced.Repeat();assert(untraced.Repeats(quiet)==PresentationTrace::Repetition::None);}
        assert(asked==0);
        PresentationTrace presentation(true);
        presentation.Observe(true,false,false,10,9,at(0));
        assert(presentation.Repeats(quiet)==PresentationTrace::Repetition::None&&asked==0);
        assert(presentation.Withheld(at(1000)));presentation.Repeat();
        assert(presentation.Repeats(busy)==PresentationTrace::Repetition::None&&asked==1&&presentation.Repeats(quiet)==PresentationTrace::Repetition::None&&asked==1);
        presentation.Observe(true,false,false,11,10,at(1100));
        assert(presentation.required==11&&presentation.Withheld(at(2100)));presentation.Repeat();
        assert(presentation.Repeats(quiet)==PresentationTrace::Repetition::Held&&asked==2&&presentation.Repeats(quiet)==PresentationTrace::Repetition::None&&asked==2);
        presentation.Observe(true,false,true,12,10,at(2200));assert(presentation.required==11);
    }
    std::cout<<"Presentation trace: only a traced repeat with no input or frame demand re-presents the held image instead of drawing a canvas frame\n";
    {
        auto origin=PresentationTrace::Clock::time_point{}+std::chrono::hours(1);
        auto at=[&](int ms){return origin+std::chrono::milliseconds(ms);};
        auto quiet=[]{return true;};
        using Repetition=PresentationTrace::Repetition;
        auto flush=[&](PresentationTrace& trace,uint64_t last,uint64_t shown,int ms){
            assert(trace.Stalled(at(ms)));auto before=trace.displayed;trace.Observe(false,false,false,last,shown,at(ms));return trace.Flushed(before);
        };
        PresentationTrace presentation(true);
        presentation.Observe(true,false,false,20,18,at(0));
        assert(presentation.Withheld(at(1000)));presentation.Repeat();
        assert(presentation.Repeats(quiet)==Repetition::Held);
        presentation.Observe(true,false,true,21,18,at(1100));
        assert(!presentation.Stalled(at(2000)));
        assert(!flush(presentation,21,18,2100)&&presentation.Repeats(quiet)==Repetition::None);
        assert(flush(presentation,21,18,2350)&&presentation.Repeats(quiet)==Repetition::Drawn&&presentation.Repeats(quiet)==Repetition::None);
        presentation.Observe(true,false,true,22,18,at(2400));assert(presentation.required==20);
        for(int ms=2650;ms<=20000;ms+=250){
            if(presentation.Stalled(at(ms)))assert(!flush(presentation,22,18,ms));
            assert(!presentation.Withheld(at(ms))&&presentation.Repeats(quiet)==Repetition::None);
        }
        PresentationTrace moved(true);
        moved.Observe(true,false,false,30,28,at(0));
        assert(moved.Withheld(at(1000)));moved.Repeat();assert(moved.Repeats(quiet)==Repetition::Held);
        moved.Observe(true,false,true,31,28,at(1100));
        assert(!flush(moved,31,29,2100)&&moved.Repeats(quiet)==Repetition::None);
        assert(!flush(moved,31,30,3100)&&moved.Repeats(quiet)==Repetition::None&&!moved.Stalled(at(9000)));
        PresentationTrace untraced(false);
        untraced.Observe(true,false,false,40,38,at(0));
        for(int ms=0;ms<=60000;ms+=250){
            if(untraced.Withheld(at(ms)))untraced.Repeat();
            if(untraced.Stalled(at(ms))){auto before=untraced.displayed;untraced.Observe(false,false,false,40,38,at(ms));assert(!untraced.Flushed(before));}
            assert(untraced.Repeats(quiet)==Repetition::None);
        }
    }
    std::cout<<"Presentation trace: a held repeat whose stalled flushes leave the statistic unchanged draws one full frame per content, never more\n";
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
