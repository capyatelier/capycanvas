#pragma once
#include <optional>
#include <string>
#include <utility>

// Presentation only: input phases and commands stay in CanvasWorkBuffer.
// A full model supersedes older presentation. Motion and camera have separate
// latest-value slots, including a camera carried by a replaced motion packet.
class CanvasSnapshotMailbox {
public:
    struct Batch { std::string full, workspace, camera; };
    void Push(std::string snapshot,bool full,bool workspace,std::optional<std::string> camera={}) {
        if(full){pending={std::move(snapshot),{}, {}};return;}
        if(workspace)pending.workspace=std::move(snapshot);
        if(camera)pending.camera=std::move(*camera);
    }
    Batch Take(){return std::exchange(pending,{});}
private:
    Batch pending;
};
