#pragma once
#include <atomic>
#include <cstdint>

namespace CapyUi {
class PointerModality {
    std::atomic<uint64_t> latest{0};
public:
    bool Observe(bool mouse,uint64_t timestamp){
        auto sample=(timestamp<<1)|uint64_t(mouse),previous=latest.load(std::memory_order_relaxed);
        while(timestamp>=(previous>>1))
            if(latest.compare_exchange_weak(previous,sample,std::memory_order_release,std::memory_order_relaxed))return (previous&1)!=uint64_t(mouse);
        return false;
    }
    bool Mouse()const{return (latest.load(std::memory_order_acquire)&1)!=0;}
};
}
