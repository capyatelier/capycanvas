#pragma once
#include <cstdint>
#include <optional>
#include <string>
#include <utility>

struct PenSourceCache {
    std::string session,source;
    std::optional<uint32_t> current;
    void Select(std::string value){if(value!=session){session=std::move(value);current.reset();source.clear();}}
    bool Describe(uint32_t id,bool down)const{return down||current!=id;}
    bool Update(uint32_t id,std::string value){
        if(current==id&&source==value)return false;
        current=id;source=std::move(value);return true;
    }
    void Forget(uint32_t id){if(current==id){current.reset();source.clear();}}
};
struct PenTwistCache {
    std::optional<uint32_t> pointer;
    bool seen=false;
    bool Report(uint32_t id,bool twisted){
        if(pointer!=id){pointer=id;seen=false;}
        seen|=twisted;return seen;
    }
};
