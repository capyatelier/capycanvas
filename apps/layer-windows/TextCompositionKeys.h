#pragma once
#include <cstdint>
#include <optional>
#include <unordered_map>

class TextCompositionKeys {
    struct Event {
        uint32_t key,time;
        bool released;
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
    bool Owns(uint32_t key,uint32_t time,bool released,bool repeat) {
        Event event{key,time,released};
        if(owned==event)return true;
        auto press=presses.find(key);
        if(!released&&!repeat&&press!=presses.end()&&!press->second.released&&press->second.time!=time){
            presses.erase(press);press=presses.end();
        }
        bool held=press!=presses.end()&&!press->second.released;
        bool completed=press!=presses.end()&&press->second.released;
        bool result=composing||(ended&&int32_t(time-*ended)<=0&&!completed)||held;
        if(released){if(held)press->second=event;}
        else if(result)presses[key]=event;
        if(result)owned=event;
        return result;
    }
    void Pointer() { ended.reset();owned.reset();presses.clear(); }
    void Clear() { composing=false;Pointer(); }
};
