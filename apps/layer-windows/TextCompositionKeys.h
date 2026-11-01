#pragma once
#include <cstdint>
#include <optional>
#include <unordered_map>

class TextCompositionKeys {
    struct Event {
        uint32_t key,time,physical;
        bool released,passed=false;
        bool operator==(Event const&) const = default;
    };
    bool composing=false;
    std::optional<uint32_t> ended;
    std::optional<Event> owned;
    std::unordered_map<uint32_t,Event> presses;
public:
    bool Active() const { return composing; }
    bool Busy() const {
        if(composing)return true;
        for(auto const& [key,event]:presses)if(!event.released)return true;
        return false;
    }
    void Update(bool active,uint32_t time) {
        if(composing&&!active){
            ended=time;
            std::erase_if(presses,[](auto const& item){return item.second.released;});
        }
        composing=active;
    }
    bool Owns(uint32_t key,uint32_t time,bool released,bool repeat,uint32_t physical=0) {
        if(!physical)physical=key;
        Event event{key,time,physical,released};
        if(owned==event)return true;
        auto press=presses.find(physical);
        if(!released&&!repeat&&press!=presses.end()&&!press->second.released&&press->second.time!=time){
            presses.erase(press);press=presses.end();
        }
        bool held=press!=presses.end()&&!press->second.released;
        bool completed=press!=presses.end()&&press->second.released;
        bool passed=held&&(press->second.passed||(!released&&key==13&&!composing&&press->second.key==229));
        bool result=key==229||(!passed&&(composing||(ended&&key!=13&&int32_t(time-*ended)<=0&&!completed)||held));
        if(released){if(held&&!(passed&&key==229)){event.passed=passed;press->second=event;}}
        else if(!(passed&&key==229)&&(result||key==13)){event.passed=!result;presses[physical]=event;}
        if(result)owned=event;
        return result;
    }
    void Pointer() { ended.reset();owned.reset();std::erase_if(presses,[](auto const& item){return item.second.released||!item.second.passed;}); }
    void Clear() { composing=false;Pointer(); }
};
