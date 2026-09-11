#include "../CanvasWorkBuffer.h"
#include "../CanvasQueryQueue.h"
#include <cassert>
#include <iostream>

int main() {
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
    std::cout<<"Canvas queries: menu priority, FIFO pixels, bounded retained allocation, retry ownership and disposal passed\n";
}
