#pragma once
#include <cstdint>

class CanvasSurfaceState {
    uint64_t minimum=0;
    bool attached=false;
public:
    void Reset(uint64_t generation){minimum=generation+1;attached=false;}
    bool CanAttach(uint64_t generation,bool ready)const{return !attached&&ready&&generation>=minimum;}
    void DidAttach(){attached=true;}
    bool Attached()const{return attached;}
};
