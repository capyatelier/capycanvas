#include "../PanelScrollReceipt.h"
#include <cassert>
#include <iostream>
int main(){
    PanelScrollReceipt receipt;
    receipt.Begin(1,20,0,0);receipt.Target(21,40,30,false);receipt.Target(21,45,35,true);receipt.Applied(21,45,40);
    assert(!receipt.release&&!receipt.inertia&&!receipt.applied);
    receipt.Release(22,45,40);receipt.Applied(22,45,45);
    assert(!receipt.inertia&&!receipt.applied);
    receipt.Target(23,45,45,true);assert(!receipt.inertia);
    receipt.Target(24,60,45,false);assert(!receipt.inertia);
    auto target=receipt.Target(25,65,45,true);assert(receipt.inertia&&receipt.inertia->serial==target.serial);
    receipt.Applied(25,65,39);assert(!receipt.applied);
    auto applied=receipt.Applied(26,65,50);receipt.Complete(27,65,50);
    assert(receipt.applied&&receipt.applied->serial==applied.serial&&applied.serial>target.serial&&receipt.terminal);
    auto unchanged=receipt.serial;receipt.Applied(28,65,50);receipt.Complete(28,65,50);assert(receipt.serial==unchanged);
    for(unsigned i=0;i<128;++i)receipt.Applied(29+i,65,51+i);
    assert(receipt.events.size()==64&&receipt.release->tick==22&&receipt.inertia->tick==25&&receipt.applied->applied==178);
    PanelScrollReceipt settling;settling.Begin(7,1,0,0);settling.Started(2,0,0);settling.Target(3,60,0,false);settling.Complete(4,60,0);
    assert(settling.terminal&&!settling.holding&&!settling.running&&settling.finalTarget->target==60&&!settling.lastApplied);
    settling.Applied(5,60,40);assert(settling.lastApplied->applied==40&&settling.finalTarget->target==60);
    settling.Applied(6,60,60);assert(settling.lastApplied->applied==settling.finalTarget->target);
    auto serial=receipt.serial;receipt.Begin(1,160,0,0);
    assert(receipt.contact==2&&receipt.serial>serial&&!receipt.terminal&&!receipt.release&&!receipt.inertia&&!receipt.applied&&!receipt.lastApplied);
    std::cout<<"Panel scroll receipts retain ordered post-release inertia and applied movement across bounded history and pointer reuse\n";
}
