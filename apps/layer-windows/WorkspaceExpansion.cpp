#include "pch.h"
#include "WorkspaceExpansion.h"
#include "WorkspaceQuery.h"
#include <chrono>

using namespace CapyUi;
struct WorkspaceExpansion::Impl:std::enable_shared_from_this<Impl>{
    using Clock=std::chrono::steady_clock;
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr};
    std::shared_ptr<WorkspaceGestures> gestures;
    std::function<void()> changed;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    hstring panel,layoutKey,lastFacts;
    J geometry,from;
    Clock::time_point started;
    double height=0;
    bool closing=false,animating=false,dirty=false,busy=false,disposed=false;
    uint64_t generation=0;
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
        if(next!=panel||closing!=wanted.empty()||layout!=layoutKey||std::abs(measured-height)>.5){
            from=geometry;panel=next;closing=wanted.empty();layoutKey=layout;height=measured;
            started=Clock::now();animating=true;dirty=true;++generation;timer.Start();
        }
    }
    void drive(){
        if(disposed||(!dirty&&!animating)){timer.Stop();return;}
        if(busy||!root.IsLoaded()||root.ActualWidth()<=0||root.ActualHeight()<=0)return;
        auto duration=std::max(1.,num(data->catalog,L"panel_expansion_ms",200))/1000.;
        double progress=animating?std::clamp(std::chrono::duration<double>(Clock::now()-started).count()/duration,0.,1.):1.;
        A heights;heights.Append(N(0));heights.Append(N(height));
        auto request=O({{L"type",S(L"expansion")},{L"panel",S(panel)},{L"heights",heights},{L"progress",N(progress)},
            {L"from",from.Size()?V(from):JsonValue::CreateNullValue()},{L"closing",B(closing)}});
        auto serial=generation;busy=true;dirty=false;
        if(!QueryWorkspace(data->query,request,[weak=weak_from_this(),serial,progress](J reply){
            if(auto self=weak.lock()){
                self->busy=false;if(self->disposed||serial!=self->generation)return;
                self->geometry=object(reply,L"result");
                if(progress>=1||!self->geometry.Size())self->animating=false;
                if(!self->geometry.Size()||(self->closing&&progress>=1)){
                    self->geometry=J{};self->panel=L"";self->from=J{};self->layoutKey=L"";
                    self->timer.Stop();
                }
                self->publish();
            }
        })){busy=false;dirty=true;}
    }
    void reset(){
        ++generation;timer.Stop();panel=L"";geometry=J{};from=J{};layoutKey=L"";height=0;
        dirty=false;animating=false;closing=false;
        data->chrome.Insert(L"expanded_panel",JsonValue::CreateNullValue());lastFacts=L"null";
        gestures->ChromeChanged();
    }
};
WorkspaceExpansion::WorkspaceExpansion(std::shared_ptr<WorkspaceData> data,Canvas root,
    std::shared_ptr<WorkspaceGestures> gestures,std::function<void()> changed):impl(std::make_shared<Impl>()){
    impl->data=std::move(data);impl->root=root;impl->gestures=std::move(gestures);
    impl->changed=std::move(changed);impl->init();
}
WorkspaceExpansion::~WorkspaceExpansion(){impl->disposed=true;impl->timer.Stop();++impl->generation;}
void WorkspaceExpansion::Apply(double height){impl->apply(height);}
void WorkspaceExpansion::Reset(){impl->reset();}
J WorkspaceExpansion::Geometry()const{return impl->geometry;}
hstring WorkspaceExpansion::Panel()const{return impl->panel;}
bool WorkspaceExpansion::Closing()const{return impl->closing;}
