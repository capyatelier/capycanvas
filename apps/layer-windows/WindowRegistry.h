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
inline void PublishWindowRegistry(RegisteredWindows const& windows){
    struct State {std::mutex mutex;std::filesystem::path path;std::string pending;bool writing=false;};
    static auto state=std::make_shared<State>();
    std::array<wchar_t,32768> directory{};auto length=GetCurrentDirectoryW(DWORD(directory.size()),directory.data());
    if(!length)winrt::throw_last_error();
    if(length>=directory.size())throw winrt::hresult_invalid_argument(L"Window registry directory is too long");
    auto path=std::filesystem::path(directory.data())/(L"windows-"+std::to_wstring(GetCurrentProcessId())+L".json");
    std::ostringstream stream;stream<<"{\"process_id\":"<<GetCurrentProcessId()<<",\"windows\":[";
    bool comma=false;for(auto [id,hwnd]:windows){if(comma)stream<<",";comma=true;stream<<"{\"id\":"<<id<<",\"hwnd\":"<<uintptr_t(hwnd)<<"}";}stream<<"]}";
    {
        std::lock_guard lock(state->mutex);state->path=std::move(path);state->pending=stream.str();
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
inline void RegisterFixtureWindow(winrt::Microsoft::UI::Xaml::Window const& window,uint64_t id=0){
    HWND hwnd=nullptr;winrt::check_hresult(window.as<IWindowNative>()->get_WindowHandle(&hwnd));
    DWORD process=0;auto thread=GetWindowThreadProcessId(hwnd,&process);
    if(!hwnd||process!=GetCurrentProcessId()||thread!=GetCurrentThreadId())throw winrt::hresult_invalid_argument(L"Fixture window is not owned by this UI thread");
    struct Registry {std::mutex mutex;std::map<HWND,uint64_t> windows;};
    static auto registry=std::make_shared<Registry>();RegisteredWindows snapshot;
    {std::lock_guard lock(registry->mutex);registry->windows[hwnd]=id?id:uint64_t(uintptr_t(hwnd));for(auto [handle,identity]:registry->windows)snapshot.emplace_back(identity,handle);PublishWindowRegistry(snapshot);}
    window.Closed([registry=registry,hwnd](auto&&,auto&&){
        RegisteredWindows snapshot;
        {std::lock_guard lock(registry->mutex);registry->windows.erase(hwnd);for(auto [handle,id]:registry->windows)snapshot.emplace_back(id,handle);PublishWindowRegistry(snapshot);}
    });
}
}
