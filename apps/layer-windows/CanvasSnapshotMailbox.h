#pragma once
#include <optional>
#include <string>
#include <utility>

// Presentation only: input phases and commands stay in CanvasWorkBuffer.
// A full model supersedes older presentation. Motion and camera have separate
// latest-value slots, including a camera carried by a replaced motion packet.
class CanvasSnapshotMailbox {
public:
    struct Batch { std::string full, workspace, camera, search, localization; };
    template<class Object,class Encode> static std::string Localization(Object const& object,Encode encode){return object.Size()?encode(object):std::string();}
    void Push(std::string snapshot,bool full,bool workspace,std::optional<std::string> camera={},bool search=false,std::string localization={}) {
        if(!localization.empty())pending.localization=std::move(localization);
        if(full){auto presentation=std::move(pending.localization);pending={std::move(snapshot),{},{},{},std::move(presentation)};return;}
        if(workspace)pending.workspace=std::move(snapshot);
        else if(search)pending.search=std::move(snapshot);
        if(camera)pending.camera=std::move(*camera);
    }
    Batch Take(){auto batch=std::exchange(pending,{});if(batch.full.empty())pending.localization=std::exchange(batch.localization,{});return batch;}
private:
    Batch pending;
};
