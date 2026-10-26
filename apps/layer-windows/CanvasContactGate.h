#pragma once
#include <cstdint>
#include <unordered_set>

class CanvasContactGate {
    std::unordered_set<uint64_t> consumed;
    void Finish(uint64_t id,uint32_t last){if(last==3||last==4)consumed.erase(id);}
public:
    void Block(uint64_t id,uint32_t first,uint32_t last){if(first!=0)consumed.insert(id);Finish(id,last);}
    bool Admit(uint64_t id,uint32_t first,uint32_t last,bool handled){
        if(first==1){if(handled)consumed.insert(id);else consumed.erase(id);}
        bool admitted=!consumed.contains(id);Finish(id,last);return admitted;
    }
};
