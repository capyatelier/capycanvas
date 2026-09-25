#pragma once
#include "UiControls.h"
#include "WorkspaceQuery.h"
#include <chrono>
#include <winrt/Microsoft.Windows.Storage.Pickers.h>

namespace CapyUi {
struct StrokeRecording:std::enable_shared_from_this<StrokeRecording>{
    std::shared_ptr<WorkspaceData> data;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    J status=O({{L"label",S(L"Start stroke recording")}});
    hstring error;
    bool saving=false,pending=false,awaiting=false;
    std::chrono::steady_clock::time_point deadline;
    std::map<uint64_t,std::function<void()>> listeners;uint64_t nextListener=0;
    ~StrokeRecording(){if(timer)timer.Stop();}
    uint64_t listen(std::function<void()> changed){listeners.emplace(++nextListener,std::move(changed));return nextListener;}
    void forget(uint64_t id){listeners.erase(id);}
    void notify(){auto current=listeners;for(auto const& [id,changed]:current)changed();}
    void start(){
        timer=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread().CreateTimer();timer.Interval(std::chrono::milliseconds(200));
        timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->poll(L"");});
        poll(L"");
    }
    void poll(hstring const& action){
        if(pending&&action.empty())return;
        pending=true;
        auto request=O({{L"type",S(L"stroke_recording")}});if(!action.empty())request.Insert(L"action",S(action));
        if(!QueryWorkspace(data->query,request,[weak=weak_from_this(),action](J reply){
            auto self=weak.lock();if(!self)return;
            self->pending=false;
            auto next=object(reply,L"result");
            self->error=str(reply,L"error");
            if(next.Size()){
                bool finished=flag(self->status,L"recording")&&!flag(next,L"recording")&&flag(next,L"ready");
                self->status=next;
                self->awaiting=self->awaiting&&flag(next,L"ready")&&std::chrono::steady_clock::now()<self->deadline;
                if(flag(next,L"recording")||self->awaiting)self->timer.Start();else self->timer.Stop();
                if(finished)self->save();
            }
            self->notify();
        }))pending=false;
    }
    void click(){
        if(saving)return;
        if(flag(status,L"recording"))poll(L"stop");
        else if(flag(status,L"ready"))save();
        else poll(L"start");
    }
    fire_and_forget save(){
        if(saving)co_return;
        auto lifetime=shared_from_this();saving=true;notify();
        try{
            winrt::Microsoft::Windows::Storage::Pickers::FileSavePicker picker(winrt::Microsoft::UI::WindowId{data->windowId});
            picker.SuggestedFileName(L"stroke-recording");picker.DefaultFileExtension(L".capystrokes");
            picker.FileTypeChoices().Insert(L"Capy stroke recording",single_threaded_vector<hstring>({L".capystrokes"}));
            auto file=co_await picker.PickSaveFileAsync();
            if(file){
                data->document(to_string(O({{L"operation",S(L"save_stroke_recording")},{L"path",S(file.Path())}}).Stringify()));
                awaiting=true;deadline=std::chrono::steady_clock::now()+std::chrono::seconds(10);
            }
        }catch(hresult_error const& failure){error=failure.message();}
        saving=false;poll(L"");
    }
};
}
