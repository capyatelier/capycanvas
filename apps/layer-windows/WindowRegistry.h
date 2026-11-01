#pragma once
#include "TraceFile.h"
#include <microsoft.ui.xaml.window.h>
#include <winrt/Microsoft.UI.Xaml.h>
#include <array>
#include <map>
#include <memory>
#include <mutex>
#include <sstream>
#include <utility>
#include <vector>

namespace CapyUi {
using RegisteredWindows=std::vector<std::pair<uint64_t,HWND>>;
inline void UpdateWindowRegistry(RegisteredWindows const* windows,HWND hwnd=nullptr,uint64_t id=0,bool applied=false,bool loaded=false){
    struct Entry {uint64_t id;bool applied=false,loaded=false;};
    struct State {
        std::mutex mutex;std::filesystem::path path;std::string pending;bool writing=false;
        std::map<HWND,Entry> windows;RegisteredWindows order;
    };
    static auto state=std::make_shared<State>();
    std::array<wchar_t,32768> directory{};auto length=GetCurrentDirectoryW(DWORD(directory.size()),directory.data());
    if(!length)winrt::throw_last_error();
    if(length>=directory.size())throw winrt::hresult_invalid_argument(L"Window registry directory is too long");
    auto path=std::filesystem::path(directory.data())/(L"windows-"+std::to_wstring(GetCurrentProcessId())+L".json");
    {
        std::lock_guard lock(state->mutex);
        if(windows){
            decltype(state->windows) next;
            for(auto [identity,handle]:*windows){
                auto old=state->windows.find(handle);
                next.emplace(handle,old!=state->windows.end()&&old->second.id==identity?old->second:Entry{identity});
            }
            state->windows=std::move(next);state->order=*windows;
        }else if(id){
            state->windows[hwnd]={id,applied,loaded};state->order.clear();
            for(auto const& [handle,entry]:state->windows)state->order.emplace_back(entry.id,handle);
        }else if(applied||loaded){
            auto entry=state->windows.find(hwnd);
            if(entry==state->windows.end()){
                if(applied)throw winrt::hresult_invalid_argument(L"Ready window is not registered");
                return;
            }
            if((!applied||entry->second.applied)&&(!loaded||entry->second.loaded))return;
            entry->second.applied|=applied;entry->second.loaded|=loaded;
        }else{state->windows.erase(hwnd);std::erase_if(state->order,[hwnd](auto const& entry){return entry.second==hwnd;});}
        std::ostringstream stream;stream<<"{\"process_id\":"<<GetCurrentProcessId()<<",\"windows\":[";
        bool comma=false;for(auto const& [identity,handle]:state->order){auto const& entry=state->windows.at(handle);if(comma)stream<<",";comma=true;stream<<"{\"id\":"<<identity<<",\"hwnd\":"<<uintptr_t(handle)<<",\"ready\":"<<(entry.applied&&entry.loaded?"true":"false")<<"}";}stream<<"]}";
        state->path=std::move(path);state->pending=stream.str();
        if(state->writing)return;state->writing=true;
    }
    [](std::shared_ptr<State> state)->winrt::fire_and_forget{
        co_await winrt::resume_background();
        for(;;){
            std::filesystem::path path;std::string payload;
            {std::lock_guard lock(state->mutex);if(state->pending.empty()){state->writing=false;co_return;}path=state->path;payload=std::move(state->pending);state->pending.clear();}
            try{WriteTraceFile(path,payload);}catch(...){OutputDebugStringW(L"Window registry publication failed\n");}
        }
    }(state);
}
inline void PublishWindowRegistry(RegisteredWindows const& windows){UpdateWindowRegistry(&windows);}
inline void ReadyRegisteredWindow(HWND hwnd,bool loaded=false){
    DWORD process=0;auto thread=GetWindowThreadProcessId(hwnd,&process);
    if(!hwnd||process!=GetCurrentProcessId()||thread!=GetCurrentThreadId())throw winrt::hresult_invalid_argument(L"Ready window is not owned by this UI thread");
    UpdateWindowRegistry(nullptr,hwnd,0,true,loaded);
}
inline void RegisterFixtureWindow(winrt::Microsoft::UI::Xaml::Window const& window,uint64_t id=0,bool applied=true){
    HWND hwnd=nullptr;winrt::check_hresult(window.as<IWindowNative>()->get_WindowHandle(&hwnd));
    DWORD process=0;auto thread=GetWindowThreadProcessId(hwnd,&process);
    if(!hwnd||process!=GetCurrentProcessId()||thread!=GetCurrentThreadId())throw winrt::hresult_invalid_argument(L"Fixture window is not owned by this UI thread");
    auto content=window.Content().as<winrt::Microsoft::UI::Xaml::FrameworkElement>();
    UpdateWindowRegistry(nullptr,hwnd,id?id:uint64_t(uintptr_t(hwnd)),applied,content.IsLoaded());
    auto token=std::make_shared<winrt::event_token>();auto weak=winrt::make_weak(content);
    if(!content.IsLoaded())*token=content.Loaded([weak,token,hwnd](auto&&,auto&&){
        if(auto root=weak.get()){root.Loaded(*token);*token={};}
        UpdateWindowRegistry(nullptr,hwnd,0,false,true);
    });
    window.Closed([weak,token,hwnd](auto&&,auto&&){
        if(token->value)if(auto root=weak.get()){root.Loaded(*token);*token={};}
        UpdateWindowRegistry(nullptr,hwnd);
    });
}
}
