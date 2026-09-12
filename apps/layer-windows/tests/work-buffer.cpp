#include "../CanvasWorkBuffer.h"
#include "../CanvasQueryQueue.h"
#include "../CanvasSnapshotMailbox.h"
#include "../WorkspacePublication.h"
#include <cassert>
#include <iostream>

int main() {
    WorkspacePublication publication;
    assert(!publication.Accept(false,10,4)); // Motion cannot establish models.
    assert(publication.Accept(true,10,4));
    assert(publication.Accept(false,12,4));
    assert(!publication.Accept(false,11,4)); // An older queued placement is stale.
    assert(!publication.Accept(false,13,5)); // A new model is required first.
    assert(publication.Revision()==12&&publication.ModelRevision()==4);
    assert(!publication.Accept(true,11,4)); // Nor can an old full snapshot rewind it.
    assert(publication.Accept(true,13,5));
    assert(!publication.Accept(false,14,4)); // Old content must never be moved.
    assert(publication.Accept(true,13,5)); // Host visibility/size refresh.
    assert(publication.Accept(false,14,5));
    std::cout<<"Workspace publication: matching models, ordered motion and host refresh passed\n";
    CanvasSnapshotMailbox snapshots;
    snapshots.Push("models A",true,true);
    snapshots.Push("motion A + camera 1",false,true,"camera 1");
    snapshots.Push("motion B",false,true);
    auto shown=snapshots.Take();
    assert(shown.full=="models A"&&shown.workspace=="motion B"&&shown.camera=="camera 1");
    snapshots.Push("motion C + camera 2",false,true,"camera 2");
    snapshots.Push("camera 3",false,false,"camera 3");
    snapshots.Push("motion D",false,true);
    shown=snapshots.Take();
    assert(shown.full.empty()&&shown.workspace=="motion D"&&shown.camera=="camera 3");
    // A completed gesture's full model must discard every older motion/camera.
    snapshots.Push("motion E + camera 4",false,true,"camera 4");
    snapshots.Push("models B (release)",true,true);
    shown=snapshots.Take();
    assert(shown.full=="models B (release)"&&shown.workspace.empty()&&shown.camera.empty());
    snapshots.Push("models C",true,true);
    snapshots.Push("motion F",false,true);
    snapshots.Push("models D (cancel)",true,true);
    snapshots.Push("camera 5",false,false,"camera 5");
    shown=snapshots.Take();
    assert(shown.full=="models D (cancel)"&&shown.workspace.empty()&&shown.camera=="camera 5");
    shown=snapshots.Take();
    assert(shown.full.empty()&&shown.workspace.empty()&&shown.camera.empty());
    std::cout<<"Canvas presentation: retained models, independent motion/camera coalescing and completion boundaries passed\n";
    CanvasWorkBuffer queue;
    // Saturation must not consume the caller's rejected command. Every
    // accepted boundary and command remains in the original order.
    for(size_t i=0;i<CanvasWorkBuffer::MaxItems-CanvasWorkBuffer::CommandSlots;i++) {
        CapyPointer p{};p.sequence=i;p.phase=i==0?1:2;
        assert(queue.Push(std::vector<CapyPointer>{p}));
    }
    CanvasWork rejected=std::vector<CapyPointer>{CapyPointer{}};
    std::get<std::vector<CapyPointer>>(rejected)[0].phase=3;
    assert(!queue.Push(std::move(rejected)));
    assert(std::get<std::vector<CapyPointer>>(rejected)[0].phase==3);
    // Pointer saturation must leave room for UI actions, especially Blur.
    assert(queue.Push(CanvasCommand{CanvasCommandKind::Input,"blur"}));
    auto batch=queue.Take();
    assert(batch.size()==CanvasWorkBuffer::MaxItems-CanvasWorkBuffer::CommandSlots+1);
    for(size_t i=0;i+1<batch.size();i++) {
        auto p=std::get<std::vector<CapyPointer>>(batch[i])[0];
        assert(p.sequence==i);
        assert(p.phase==(i==0?1u:2u));
    }
    assert(std::get<CanvasCommand>(batch.back()).json=="blur");
    assert(queue.Empty());
    assert(queue.Push(CanvasCommand{CanvasCommandKind::Action,"undo"}));
    CapyPointer up{};up.phase=3;
    assert(queue.Push(std::vector<CapyPointer>{up}));
    batch=queue.Take();
    assert(std::get<CanvasCommand>(batch[0]).json=="undo");
    assert(std::get<std::vector<CapyPointer>>(batch[1])[0].phase==3);
    assert(queue.Push(std::move(rejected)));
    queue.Take();
    for(size_t i=0;i<CanvasWorkBuffer::MaxItems;i++)assert(queue.Push(CanvasCommand{CanvasCommandKind::Action,"fit"}));
    CanvasWork refused=CanvasCommand{CanvasCommandKind::Action,"undo"};
    assert(!queue.Push(std::move(refused)));
    assert(std::get<CanvasCommand>(refused).json=="undo");
    queue.Take();

    // Byte limits also apply to a single allocation and retained spare
    // capacity: shrinking a string/vector cannot bypass the memory bound.
    CanvasWork huge=CanvasCommand{CanvasCommandKind::Action,std::string(CanvasWorkBuffer::MaxBytes,'x')};
    assert(!queue.Push(std::move(huge)));
    std::get<CanvasCommand>(huge).json.resize(1);
    assert(!queue.Push(std::move(huge)));
    std::vector<CapyPointer> spare;
    spare.reserve(CanvasWorkBuffer::MaxBytes/sizeof(CapyPointer)+1);
    assert(!queue.Push(std::move(spare)));
    assert(queue.Empty());
    // Payload exhaustion must be possible before item exhaustion.
    size_t accepted=0;
    while(queue.Push(CanvasCommand{CanvasCommandKind::Action,std::string(16384,'x')}))++accepted;
    assert(accepted>0&&accepted<CanvasWorkBuffer::MaxItems);
    queue.Take();
    assert(queue.Push(CanvasScroll{1,2,3,4,2,true,false}));
    std::cout<<"Canvas queue: ordered boundaries, reserved commands, refusal ownership, item and allocation limits passed\n";

    CanvasQueryQueue queries;
    auto payload=std::make_shared<int>(7);std::weak_ptr<int> lifetime=payload;
    for(size_t i=0;i<CanvasQueryQueue::MaxItems;i++)
        assert(queries.Push({i==3?CanvasQueryKind::LayerMenu:CanvasQueryKind::Thumbnails,std::to_string(i),
            [payload](PreviewPacket){}}));
    payload.reset();
    CanvasQuery extra{CanvasQueryKind::Filters,"retry",{}};
    assert(!queries.Push(std::move(extra)));assert(extra.json=="retry");
    assert(queries.Take()->json=="3");
    for(auto expected:{"0","1","2","4","5","6","7"})assert(queries.Take()->json==expected);
    assert(queries.Empty()&&!queries.Take()&&lifetime.expired());
    extra.json.reserve(CanvasQueryQueue::MaxPayload+1);
    assert(!queries.Push(std::move(extra)));assert(extra.json=="retry");
    assert(queries.Push({CanvasQueryKind::Filters,"discard",{}}));queries.Clear();
    assert(queries.Empty());
    assert(queries.Push({CanvasQueryKind::LayerMenu,"old",{}}));
    assert(queries.Push({CanvasQueryKind::Thumbnails,"pixels",{}}));
    assert(queries.Push({CanvasQueryKind::LayerMenu,"latest",{}}));
    assert(queries.Take()->json=="latest");assert(queries.Take()->json=="pixels");assert(queries.Empty());
    // Workspace geometry must not replace another view's pending callback.
    // It shares the bounded FIFO with readbacks; context menus retain priority.
    assert(queries.Push({CanvasQueryKind::Workspace,"drawer",{}}));
    assert(queries.Push({CanvasQueryKind::Thumbnails,"thumbnail",{}}));
    assert(queries.Push({CanvasQueryKind::Workspace,"stats",{}}));
    assert(queries.Push({CanvasQueryKind::LayerMenu,"context",{}}));
    for(auto expected:{"context","drawer","thumbnail","stats"})assert(queries.Take()->json==expected);
    assert(queries.Empty());
    std::cout<<"Canvas queries: menu priority, FIFO geometry/pixels, bounded retained allocation, retry ownership and disposal passed\n";
}
