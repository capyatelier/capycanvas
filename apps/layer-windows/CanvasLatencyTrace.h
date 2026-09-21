#pragma once
#include "native/include/capy_windows.h"
#include <cstdint>
#include <fstream>
#include <string>
#include <vector>

// Opt-in measurement only. Each vector has one owner thread, reserves once,
// and stops at its bound. Dump runs after both owners have stopped. No per-frame
// disk writes and no pointer coordinates or other applications' input are stored.
class CanvasLatencyTrace {
public:
    explicit CanvasLatencyTrace(bool active):enabled(active) {
        if(enabled){inputs.reserve(Limit);consumed.reserve(Limit);frames.reserve(Limit);}
    }
    bool const enabled;
    void Input(CapyPointer const& p,uint64_t arrival) {
        if(!enabled || (p.flags&1) || p.tool>0 || p.phase<1 || p.phase>3)return;
        if(inputs.size()==Limit){inputOverflow=true;return;}
        inputs.push_back({p.sequence,p.timestamp_ns,arrival,p.phase});
    }
    void Consume(std::vector<CapyPointer> const& points) {
        if(!enabled)return;
        for(auto const& p:points)if(!(p.flags&1)&&p.tool==0&&p.phase>=1&&p.phase<=3){
            if(consumed.size()==Limit){consumeOverflow=true;return;}
            consumed.push_back({p.sequence,frameNumber+1});
        }
    }
    void Frame(uint64_t acquireStart,uint64_t acquired,uint64_t renderStart,uint64_t renderEnd,uint64_t const* stats,int32_t error) {
        if(!enabled||consumed.empty())return;
        auto last=consumed.back().frame-1;
        if(last<frames.size()&&renderStart>frames[last].renderEnd+500000000ULL)return;
        ++frameNumber;
        if(frames.size()==Limit){frameOverflow=true;return;}
        frames.push_back({frameNumber,acquireStart,acquired,renderStart,renderEnd,stats[0],stats[1],stats[2],stats[3],stats[4],error});
    }
    void Dump(std::string const& prefix) const {
        if(!enabled)return;
        std::ofstream input(prefix+"-input.csv"),consume(prefix+"-consumed.csv"),frame(prefix+"-frames.csv");
        input<<"sequence,sample_ns,arrival_ns,phase\n";
        for(auto const& p:inputs)input<<p.sequence<<','<<p.sample<<','<<p.arrival<<','<<p.phase<<'\n';
        consume<<"sequence,frame\n";
        for(auto const& p:consumed)consume<<p.sequence<<','<<p.frame<<'\n';
        frame<<"frame,acquire_start_ns,acquired_ns,render_start_ns,render_end_ns,last_present,displayed_present,present_refresh,sync_refresh,sync_qpc,stats_error\n";
        for(auto const& f:frames)frame<<f.number<<','<<f.acquireStart<<','<<f.acquired<<','<<f.renderStart<<','<<f.renderEnd<<','<<f.lastPresent<<','<<f.displayed<<','<<f.presentRefresh<<','<<f.syncRefresh<<','<<f.syncQpc<<','<<f.error<<'\n';
        std::ofstream(prefix+"-status.json")<<"{\"overflow\":"<<(inputOverflow||consumeOverflow||frameOverflow?"true":"false")<<"}";
    }
private:
    static constexpr size_t Limit=65536;
    struct InputRecord {uint64_t sequence,sample,arrival;uint32_t phase;};
    struct Consumption {uint64_t sequence,frame;};
    struct FrameRecord {uint64_t number,acquireStart,acquired,renderStart,renderEnd,lastPresent,displayed,presentRefresh,syncRefresh,syncQpc;int32_t error;};
    std::vector<InputRecord> inputs; // independent input thread
    std::vector<Consumption> consumed; // render owner
    std::vector<FrameRecord> frames; // render owner
    uint64_t frameNumber=0;
    bool inputOverflow=false,consumeOverflow=false,frameOverflow=false;
};
