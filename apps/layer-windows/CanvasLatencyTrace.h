#pragma once
#include "native/include/capy_windows.h"
#include <chrono>
#include <cstdint>
#include <fstream>
#include <optional>
#include <string>
#include <vector>

class PresentationTrace {
public:
    using Clock=std::chrono::steady_clock;
    static constexpr Clock::duration Patience=std::chrono::seconds(1);
    explicit PresentationTrace(bool active):enabled(active) {}
    bool const enabled;
    uint64_t required=0,epoch=0;
    std::optional<uint64_t> displayed;
    void Observe(bool frame,bool dirty,bool repeat,uint64_t last,std::optional<uint64_t> shown,Clock::time_point now) {
        bool presented=last!=presentations;
        presentations=last;
        if(presented){presentedAt=now;if(!repeat)content=last;}
        if(shown&&shown!=displayed){displayed=shown;displayedAt=now;}
        required=dirty?last+(frame&&presented?0:1):content;
        ++epoch;
    }
    bool Withheld(Clock::time_point now) const {
        return enabled&&displayed&&*displayed<content&&repeated!=content&&now-presentedAt>=Patience&&now-displayedAt>=Patience;
    }
    void Repeat() { repeated=content; }
private:
    uint64_t presentations=0,content=0,repeated=0;
    Clock::time_point presentedAt{},displayedAt{};
};

// Opt-in measurement only. Each vector has one owner thread, reserves once,
// and stops at its bound. Dump runs after both owners have stopped. No per-frame
// disk writes and no pointer coordinates or other applications' input are stored.
class CanvasLatencyTrace {
public:
    explicit CanvasLatencyTrace(bool active):enabled(active) {
        if(enabled){inputs.reserve(Limit);consumed.reserve(Limit);frames.reserve(Limit);presentations.reserve(Limit);}
    }
    bool const enabled;
    void Input(CapyPointer const& p,uint64_t arrival) {
        if(!enabled)return;
        if(p.flags&1){++predictedPoints;return;}
        if(p.tool>0 || p.phase<1 || p.phase>3)return;
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
    void Frame(uint64_t acquireStart,uint64_t acquired,uint64_t renderStart,uint64_t renderEnd,uint64_t const* stats,int32_t error,uint64_t drained=0,CapyFrameTiming timing={}) {
        if(!enabled)return;
        if(presentations.size()==Limit)presentationOverflow=true;
        else presentations.push_back({uint64_t(presentations.size()+1),acquireStart,acquired,renderStart,renderEnd,stats[0],stats[1],stats[2],stats[3],stats[4],error,drained,timing});
        if(consumed.empty())return;
        auto last=consumed.back().frame-1;
        if(last<frames.size()&&renderStart>frames[last].renderEnd+500000000ULL)return;
        ++frameNumber;
        if(frames.size()==Limit){frameOverflow=true;return;}
        frames.push_back({frameNumber,acquireStart,acquired,renderStart,renderEnd,stats[0],stats[1],stats[2],stats[3],stats[4],error,drained,timing});
    }
    void Dump(std::string const& prefix) const {
        if(!enabled)return;
        std::ofstream input(prefix+"-input.csv"),consume(prefix+"-consumed.csv"),frame(prefix+"-frames.csv");
        input<<"sequence,sample_ns,arrival_ns,phase\n";
        for(auto const& p:inputs)input<<p.sequence<<','<<p.sample<<','<<p.arrival<<','<<p.phase<<'\n';
        consume<<"sequence,frame\n";
        for(auto const& p:consumed)consume<<p.sequence<<','<<p.frame<<'\n';
        auto write=[](std::ostream& stream,auto const& rows,char const* index){
            stream<<index<<",acquire_start_ns,acquired_ns,render_start_ns,render_end_ns,last_present,displayed_present,present_refresh,sync_refresh,sync_qpc,stats_error,drained_ns,pacing_ns,acquisition_ns,prepared_ns,composed_ns,submitted_ns,presented_ns\n";
            for(auto const& f:rows)stream<<f.number<<','<<f.acquireStart<<','<<f.acquired<<','<<f.renderStart<<','<<f.renderEnd<<','<<f.lastPresent<<','<<f.displayed<<','<<f.presentRefresh<<','<<f.syncRefresh<<','<<f.syncQpc<<','<<f.error<<','<<f.drained<<','<<f.timing.pacing_ns<<','<<f.timing.acquisition_ns<<','<<f.timing.prepared_ns<<','<<f.timing.composed_ns<<','<<f.timing.submitted_ns<<','<<f.timing.presented_ns<<'\n';
        };
        write(frame,frames,"frame");
        std::ofstream presentation(prefix+"-presentations.csv");
        write(presentation,presentations,"presentation");
        std::ofstream(prefix+"-status.json")<<"{\"overflow\":"<<(inputOverflow||consumeOverflow||frameOverflow||presentationOverflow?"true":"false")<<",\"predicted_points\":"<<predictedPoints<<"}";
    }
private:
    static constexpr size_t Limit=65536;
    struct InputRecord {uint64_t sequence,sample,arrival;uint32_t phase;};
    struct Consumption {uint64_t sequence,frame;};
    struct FrameRecord {uint64_t number,acquireStart,acquired,renderStart,renderEnd,lastPresent,displayed,presentRefresh,syncRefresh,syncQpc;int32_t error;uint64_t drained;CapyFrameTiming timing;};
    std::vector<InputRecord> inputs; // independent input thread
    std::vector<Consumption> consumed; // render owner
    std::vector<FrameRecord> presentations;
    std::vector<FrameRecord> frames; // render owner
    uint64_t frameNumber=0,predictedPoints=0;
    bool inputOverflow=false,consumeOverflow=false,frameOverflow=false,presentationOverflow=false;
};
