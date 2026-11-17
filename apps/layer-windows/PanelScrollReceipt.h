#pragma once
#include <cstdint>
#include <deque>
#include <optional>

struct PanelScrollReceipt {
    enum class Kind {Contact,Release,Target,Applied,Complete,Started,Native};
    struct Event {Kind kind;uint64_t serial,tick;double target,applied;bool inertial;};
    uint64_t contact=0,pointer=0,serial=0;
    bool terminal=false,holding=false,running=false;
    std::deque<Event> events;
    std::optional<Event> release,inertia,applied,finalTarget,lastApplied;
    std::optional<double> observed;
    Event Record(Kind kind,uint64_t tick,double target,double offset,bool inertial=false){
        Event event{kind,++serial,tick,target,offset,inertial};
        if(events.size()==64)events.pop_front();
        events.push_back(event);return event;
    }
    Event Begin(uint32_t id,uint64_t tick,double target,double offset){
        ++contact;pointer=id;terminal=false;holding=true;running=false;events.clear();release.reset();inertia.reset();applied.reset();lastApplied.reset();observed=offset;
        auto event=Record(Kind::Contact,tick,target,offset);finalTarget=event;return event;
    }
    Event Started(uint64_t tick,double target,double offset){running=true;return Record(Kind::Started,tick,target,offset);}
    Event Release(uint64_t tick,double target,double offset){
        holding=false;auto event=Record(Kind::Release,tick,target,offset);release=event;observed=offset;return event;
    }
    Event Target(uint64_t tick,double target,double offset,bool inertial){
        auto event=Record(Kind::Target,tick,target,offset,inertial);finalTarget=event;
        if(release&&inertial&&!inertia&&target!=release->target)inertia=event;
        return event;
    }
    Event Applied(uint64_t tick,double target,double offset){
        if(lastApplied&&observed&&*observed==offset)return {Kind::Applied,serial,tick,target,offset,false};
        observed=offset;auto event=Record(Kind::Applied,tick,target,offset);lastApplied=event;
        if(inertia&&(offset-release->applied)*(inertia->target-release->target)>0)applied=event;
        return event;
    }
    Event Complete(uint64_t tick,double target,double offset){
        if(terminal)return {Kind::Complete,serial,tick,target,offset,false};
        terminal=true;holding=false;running=false;return Record(Kind::Complete,tick,target,offset);
    }
};
