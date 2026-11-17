#pragma once
#include <algorithm>
#include <array>
#include <atomic>
#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace CapyUi {
enum class NativeInputRoute { Routed, XamlCanvas, Independent };
enum class NativeInputAdmission { Unknown, Accepted, Rejected };
enum class NativeInputDevice { Mouse, Pen, Touch };
struct NativeCanvasDown {
    uint64_t pointer=0,time=0;
    std::array<int32_t,2> screen{};
    NativeInputDevice device=NativeInputDevice::Mouse;
    NativeInputAdmission admitted=NativeInputAdmission::Unknown;
};
struct NativeInputDown {
    uint64_t serial=0,pointer=0,time=0;
    std::wstring device;
    std::array<int32_t,2> screen{};
    std::vector<std::wstring> path;
    NativeInputRoute route=NativeInputRoute::Routed;
    NativeInputAdmission admitted=NativeInputAdmission::Unknown;
    std::optional<uint64_t> record;
    std::wstring part;
    bool SameContact(NativeInputDown const& other)const{return pointer==other.pointer&&time==other.time&&device==other.device&&route==other.route;}
    bool operator==(NativeInputDown const&)const=default;
};
class NativeInputJournal {
public:
    static constexpr size_t Capacity=64;
    struct Snapshot {uint64_t serial=0,dropped=0;bool closed=false;std::vector<NativeInputDown> downs;};
private:
    std::atomic<std::shared_ptr<Snapshot const>> snapshot{std::make_shared<Snapshot>()};
public:
    std::shared_ptr<Snapshot const> Read()const{return snapshot.load();}
    uint64_t Record(NativeInputDown down){
        auto previous=Read();
        for(;;){
            if(previous->closed)return previous->serial;
            auto found=std::find_if(previous->downs.begin(),previous->downs.end(),[&](auto const& row){return row.SameContact(down);});
            if(found!=previous->downs.end()){
                down.serial=found->serial;
                if(found->admitted!=NativeInputAdmission::Unknown&&down.admitted==NativeInputAdmission::Unknown)return down.serial;
                if(*found==down)return down.serial;
            }
            auto next=std::make_shared<Snapshot>(*previous);
            if(found==previous->downs.end()){
                down.serial=++next->serial;
                if(next->downs.size()==Capacity){next->downs.erase(next->downs.begin());++next->dropped;}
                next->downs.push_back(down);
            }else next->downs[size_t(found-previous->downs.begin())]=down;
            std::shared_ptr<Snapshot const> sealed=std::move(next);
            if(snapshot.compare_exchange_weak(previous,sealed))return down.serial;
        }
    }
    void Close(){
        auto previous=Read();
        for(;;){
            if(previous->closed)return;
            auto next=std::make_shared<Snapshot>(*previous);next->closed=true;
            std::shared_ptr<Snapshot const> sealed=std::move(next);
            if(snapshot.compare_exchange_weak(previous,sealed))return;
        }
    }
};
inline std::function<void(NativeCanvasDown)> NativeInputReceiver(std::shared_ptr<NativeInputJournal> output,std::shared_ptr<std::atomic<bool>> authorized,std::vector<std::wstring> path,NativeInputRoute route){
    return [output=std::move(output),authorized=std::move(authorized),path=std::move(path),route](NativeCanvasDown point){
        if(!authorized->load())return;
        NativeInputDown down;down.pointer=point.pointer;down.time=point.time;down.screen=point.screen;down.path=path;
        down.device=point.device==NativeInputDevice::Mouse?L"mouse":point.device==NativeInputDevice::Pen?L"pen":L"touch";
        down.route=route;down.admitted=point.admitted;output->Record(std::move(down));
    };
}
}
