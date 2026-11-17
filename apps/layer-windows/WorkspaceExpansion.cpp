#include "pch.h"
#include "WorkspaceExpansion.h"
#include "WorkspaceQuery.h"
#include "WorkspaceGeometry.h"
#include <array>
#include <chrono>
#include <deque>
#include <optional>
#include <winrt/Windows.UI.ViewManagement.h>

using namespace CapyUi;
struct WorkspaceExpansion::Impl:std::enable_shared_from_this<Impl>{
    using Clock=std::chrono::steady_clock;
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr};
    std::shared_ptr<WorkspaceGestures> gestures;
    std::function<void()> changed;
    std::function<double(double)> measure;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    hstring panel,layoutKey,lastFacts;
    J geometry,from;
    Clock::time_point started;
    double height=0;
    bool closing=false,animating=false,dirty=false,busy=false,disposed=false,measurePending=false,startPending=false;
    uint64_t generation=0;
    bool tracing=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    struct Frame {
        uint64_t frame;hstring panel;double group;std::array<double,8> boxes;
        bool Same(Frame const& other)const{return panel==other.panel&&group==other.group&&boxes==other.boxes;}
    };
    std::deque<Frame> frames;
    uint64_t frame=0,dropped=0;
    std::optional<Frame> lastFrame;
    void init(){
        timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(16));
        timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->drive();});
    }
    void publish(){
        auto next=geometry.Size()?geometry.Stringify():hstring{L"null"};
        if(next!=lastFacts){
            lastFacts=next;
            data->chrome.Insert(L"expanded_panel",geometry.Size()?V(geometry):JsonValue::CreateNullValue());
            gestures->ChromeChanged();
        }
        changed();
    }
    void apply(double measured){
        if(disposed)return;
        auto wanted=str(object(data->state,L"customization"),L"expanded");
        auto next=wanted.empty()?panel:wanted;
        if(next.empty())return;
        auto layout=object(data->model,L"layout").Stringify();
        measured=std::max(0.,measured);
        bool transition=next!=panel||closing!=wanted.empty();
        if(transition||layout!=layoutKey||std::abs(measured-height)>.5){
            if(transition){from=geometry;started=Clock::now();animating=true;measurePending=startPending=!wanted.empty();}
            if(tracing&&transition&&!wanted.empty()){frames.clear();dropped=0;lastFrame.reset();}
            if(!wanted.empty()&&(transition||layout!=layoutKey))measurePending=true;
            panel=next;closing=wanted.empty();layoutKey=layout;height=measured;
            dirty=true;++generation;timer.Start();
        }
    }
    void drive(){
        if(disposed||(!dirty&&!animating&&!busy)){timer.Stop();return;}
        if(busy||!root.IsLoaded()||root.ActualWidth()<=0||root.ActualHeight()<=0)return;
        auto duration=std::max(1.,num(data->catalog,L"panel_expansion_ms",200))/1000.;
        bool measuring=measurePending;
        double progress=!measuring&&animating&&SystemUiSettings().AnimationsEnabled()?std::clamp(std::chrono::duration<double>(Clock::now()-started).count()/duration,0.,1.):1.;
        A heights;heights.Append(N(0));heights.Append(N(measuring?0:height));
        auto request=O({{L"type",S(L"expansion")},{L"panel",S(panel)},{L"heights",heights},{L"progress",N(progress)},
            {L"from",!measuring&&from.Size()?V(from):JsonValue::CreateNullValue()},{L"closing",B(!measuring&&closing)}});
        auto serial=generation;busy=true;dirty=false;
        if(!QueryWorkspace(data->query,request,[weak=weak_from_this(),serial,progress,measuring](J reply){
            if(auto self=weak.lock()){
                self->busy=false;if(self->disposed||serial!=self->generation)return;
                auto geometry=object(reply,L"result");
                self->measurePending=false;
                if(measuring&&geometry.Size()){
                    self->height=std::max(0.,self->measure(num(object(geometry,L"configuration"),L"width")));
                    if(self->startPending){self->started=Clock::now();self->startPending=false;}
                    self->dirty=true;return;
                }
                self->geometry=std::move(geometry);
                for(auto key:{L"bounds",L"preview",L"configuration"})if(auto box=object(self->geometry,key);box.Size())self->geometry.Insert(key,rectangle(pixelBounds(rectangle(box),self->root)));
                if(progress>=1||!self->geometry.Size())self->animating=false;
                if(!self->geometry.Size()||(self->closing&&progress>=1)){
                    self->geometry=J{};self->panel=L"";self->from=J{};self->layoutKey=L"";
                    self->timer.Stop();
                }
                self->publish();
            }
        })){busy=false;dirty=true;}
    }
};
WorkspaceExpansion::WorkspaceExpansion(std::shared_ptr<WorkspaceData> data,Canvas root,
    std::shared_ptr<WorkspaceGestures> gestures,std::function<void()> changed,std::function<double(double)> measure):impl(std::make_shared<Impl>()){
    impl->data=std::move(data);impl->root=root;impl->gestures=std::move(gestures);
    impl->changed=std::move(changed);impl->measure=std::move(measure);impl->init();
}
WorkspaceExpansion::~WorkspaceExpansion(){impl->disposed=true;impl->timer.Stop();++impl->generation;}
void WorkspaceExpansion::Apply(double height){impl->apply(height);}
J WorkspaceExpansion::Geometry()const{return impl->geometry;}
hstring WorkspaceExpansion::Panel()const{return impl->panel;}
bool WorkspaceExpansion::Closing()const{return impl->closing;}

void WorkspaceExpansion::TraceFrame(uint64_t frame,J const& geometry){
    if(!impl->tracing)return;
    impl->frame=frame;
    if(!geometry.Size()){impl->lastFrame.reset();return;}
    Impl::Frame next{frame,impl->panel,num(geometry,L"group"),{}};
    auto bounds=object(geometry,L"bounds"),configuration=object(geometry,L"configuration");
    next.boxes={num(bounds,L"x"),num(bounds,L"y"),num(bounds,L"width"),num(bounds,L"height"),
        num(configuration,L"x"),num(configuration,L"y"),num(configuration,L"width"),num(configuration,L"height")};
    if(impl->lastFrame&&impl->lastFrame->Same(next))return;
    impl->lastFrame=next;
    if(impl->frames.size()==64){impl->frames.pop_front();++impl->dropped;}
    impl->frames.push_back(std::move(next));
}
J WorkspaceExpansion::TraceFrames()const{
    A records;
    for(auto const& row:impl->frames){
        auto box=[&](size_t offset){return O({{L"x",N(row.boxes[offset])},{L"y",N(row.boxes[offset+1])},
            {L"width",N(row.boxes[offset+2])},{L"height",N(row.boxes[offset+3])}});};
        records.Append(O({{L"frame",N(double(row.frame))},{L"panel",S(row.panel)},
            {L"geometry",O({{L"group",N(row.group)},{L"bounds",box(0)},{L"configuration",box(4)}})}}));
    }
    return O({{L"frame",N(double(impl->frame))},{L"dropped",N(double(impl->dropped))},{L"records",records}});
}
