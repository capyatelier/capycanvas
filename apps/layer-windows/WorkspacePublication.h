#pragma once
#include <cstdint>

// UI models and replaceable placement have separate identities. A host-only
// refresh can legitimately replace models at the same workspace revision.
class WorkspacePublication {
public:
    bool Accept(bool full,uint64_t revision,uint64_t modelRevision) {
        if(ready&&revision<lastRevision)return false;
        if(!full&&(!ready||modelRevision!=lastModelRevision))return false;
        if(full){ready=true;lastModelRevision=modelRevision;}
        lastRevision=revision;
        return true;
    }
    uint64_t Revision()const{return lastRevision;}
    uint64_t ModelRevision()const{return lastModelRevision;}
private:
    bool ready=false;
    uint64_t lastRevision=0,lastModelRevision=0;
};
