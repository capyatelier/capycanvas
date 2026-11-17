#include "../NativeInputReceipt.h"
#include <cassert>
#include <iostream>
#include <thread>
#include <type_traits>

int main(){
    using namespace CapyUi;
    static_assert(std::is_trivially_copyable_v<NativeCanvasDown>);
    auto guarded=std::make_shared<NativeInputJournal>();auto authorized=std::make_shared<std::atomic<bool>>(false);
    auto receiver=NativeInputReceiver(guarded,authorized,{L"drawing-canvas"},NativeInputRoute::Independent);
    NativeCanvasDown point;point.pointer=41;point.time=100;point.screen={120,240};point.admitted=NativeInputAdmission::Accepted;
    std::thread before([&]{receiver(point);});before.join();assert(guarded->Read()->downs.empty());
    authorized->store(true);std::thread admitted([&]{receiver(point);});admitted.join();auto permitted=guarded->Read();
    assert(permitted->serial==1&&permitted->downs[0].path==std::vector<std::wstring>{L"drawing-canvas"}&&permitted->downs[0].admitted==NativeInputAdmission::Accepted);
    authorized->store(false);++point.time;receiver(point);assert(guarded->Read()==permitted);
    authorized->store(true);guarded->Close();auto stopped=guarded->Read();std::thread after([&]{receiver(point);});after.join();assert(guarded->Read()==stopped);
    NativeInputJournal journal;
    NativeInputDown received;received.pointer=7;received.time=11;received.device=L"touch";received.screen={120,240};received.path={L"",L"actual-button",L"owned-root"};
    auto empty=journal.Read();assert(journal.Record(received)==1);
    auto arrival=journal.Read();assert(arrival->downs[0].admitted==NativeInputAdmission::Unknown);
    received.path[1]=L"retired-source";assert(arrival->downs[0].path[1]==L"actual-button"&&empty->downs.empty());
    auto accepted=arrival->downs[0];accepted.admitted=NativeInputAdmission::Accepted;accepted.record=9;accepted.part=L"face";
    assert(journal.Record(accepted)==1);auto admission=journal.Read();
    assert(admission->downs[0].admitted==NativeInputAdmission::Accepted&&arrival->downs[0].admitted==NativeInputAdmission::Unknown);
    assert(journal.Record(received)==1&&journal.Read()==admission);
    assert(journal.Record(accepted)==1&&journal.Read()==admission);
    auto rejected=accepted;rejected.route=NativeInputRoute::Independent;rejected.admitted=NativeInputAdmission::Rejected;
    assert(journal.Record(rejected)==2);assert(journal.Read()->downs.back().admitted==NativeInputAdmission::Rejected);
    accepted.part=L"ring";assert(journal.Record(accepted)==1&&journal.Read()->serial==2&&journal.Read()->downs[0].part==L"ring");
    for(uint64_t i=0;i<100;++i){auto next=accepted;next.time=100+i;journal.Record(std::move(next));}
    auto bounded=journal.Read();assert(bounded->serial==102&&bounded->dropped==38&&bounded->downs.size()==64);
    assert(bounded->downs.front().serial==39&&bounded->downs.back().serial==102&&admission->downs.size()==1);
    NativeInputJournal parallel;
    auto writer=[&](uint64_t pointer){for(uint64_t i=0;i<100;++i){NativeInputDown next;next.pointer=pointer;next.time=i;parallel.Record(std::move(next));}};
    std::thread a(writer,1),b(writer,2);a.join();b.join();auto both=parallel.Read();
    assert(both->serial==200&&both->dropped==136&&both->downs.size()==64);
    for(size_t i=1;i<both->downs.size();++i)assert(both->downs[i].serial==both->downs[i-1].serial+1);
    parallel.Close();auto closed=parallel.Read();assert(closed->closed);
    assert(parallel.Record(accepted)==200&&parallel.Read()==closed);
    std::cout<<"Native Down receipts require authorization and retain exact identity, admission, immutable bounded snapshots and concurrent ordering\n";
}
