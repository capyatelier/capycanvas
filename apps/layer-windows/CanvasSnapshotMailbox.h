#pragma once
#include <optional>
#include <cstdint>
#include <string>
#include <utility>

// Presentation only: input phases and commands stay in CanvasWorkBuffer.
// A full model supersedes older presentation. Motion and camera have separate
// latest-value slots, including a camera carried by a replaced motion packet.
class CanvasSnapshotMailbox {
public:
    struct Batch { std::string full, layout, workspace, camera, search, localization, glass;uint64_t sequence=0; };
    void Push(std::string snapshot,bool full,bool workspace,std::optional<std::string> camera={},bool search=false,std::string localization={},bool layout=false,std::string glass={}) {
        if(closed)return;
        if(full||workspace||layout||search||camera||!glass.empty())++sequence;
        if(!localization.empty())pending.localization=std::move(localization);
        if(!glass.empty())pending.glass=std::move(glass);
        if(full){auto presentation=std::move(pending.localization);auto metadata=std::move(pending.glass);pending={std::move(snapshot),{},{},{},{},std::move(presentation),std::move(metadata)};return;}
        if(layout){pending.layout=std::move(snapshot);pending.workspace.clear();}
        else if(workspace)pending.workspace=std::move(snapshot);
        else if(search)pending.search=std::move(snapshot);
        if(camera)pending.camera=std::move(*camera);
    }
    Batch Take(){auto batch=std::exchange(pending,{});if(batch.full.empty())pending.localization=std::exchange(batch.localization,{});batch.sequence=sequence;return batch;}
    std::optional<Batch> BeginDecode(){
        if(closed||decoding||(pending.full.empty()&&pending.layout.empty()&&pending.workspace.empty()&&pending.camera.empty()&&pending.search.empty()&&pending.glass.empty()))return {};
        decoding=true;return Take();
    }
    void CompleteDecode(){decoding=false;}
    void Close(){closed=true;pending={};}
    bool Closed()const{return closed;}
    uint64_t Sequence()const{return sequence;}
    void Posted(uint64_t value){if(value>posted)posted=value;}
    bool IsPosted(uint64_t value)const{return !closed&&value<=posted;}
private:
    Batch pending;
    bool decoding=false,closed=false;
    uint64_t sequence=0,posted=0;
};
