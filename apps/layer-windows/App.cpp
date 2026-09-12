#include "pch.h"
#include "CanvasWindow.h"
#include <fstream>
#include <map>
#include <winrt/Microsoft.UI.Xaml.XamlTypeInfo.h>
#include <winrt/Windows.UI.Xaml.Interop.h>

namespace {
using namespace winrt;
using namespace Microsoft::UI::Xaml;
// Use WinUI's metadata provider for its native controls and built-in templates.
// Workspace controls are composed directly from the shared Rust models.
struct App : ApplicationT<App, Markup::IXamlMetadataProvider> {
    Microsoft::UI::Xaml::XamlTypeInfo::XamlControlsXamlMetaDataProvider metadata{nullptr};
    std::map<uint64_t,std::shared_ptr<CanvasWindow>> windows;
    bool launchedWindow=false;
    void TraceWindows()const {
        if(!GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0))return;
        auto name=L"windows-"+std::to_wstring(GetCurrentProcessId())+L".json",pending=name+L".pending";
        {
            std::ofstream stream(pending);
            stream<<"{\"process_id\":"<<GetCurrentProcessId()<<",\"windows\":[";
            bool comma=false;
            for(auto const& [id,window]:windows){
                if(comma)stream<<",";comma=true;
                stream<<"{\"id\":"<<id<<",\"hwnd\":"<<uintptr_t(window->Handle())<<"}";
            }
            stream<<"]}";
            if(!stream)return;
        }
        MoveFileExW(pending.c_str(),name.c_str(),MOVEFILE_REPLACE_EXISTING);
    }
    void AddWindow() {
        auto next=std::make_shared<CanvasWindow>(
            [weak=get_weak()]{if(auto self=weak.get())self->AddWindow();},
            [weak=get_weak()](uint64_t id){if(auto self=weak.get()){self->windows.erase(id);self->TraceWindows();}},
            !launchedWindow);
        windows.emplace(next->Id(),next);
        launchedWindow=true;
        TraceWindows();
        next->Open();
    }
    App() {
        UnhandledException([](auto&&, UnhandledExceptionEventArgs const& event) {
            if(!GetEnvironmentVariableW(L"CAPY_TEST_DISPLAY",nullptr,0))return;
            std::ofstream log("startup-error.log");
            log << std::hex << uint32_t(event.Exception().value) << " " << to_string(event.Message()) << std::endl;
        });
        metadata=Microsoft::UI::Xaml::XamlTypeInfo::XamlControlsXamlMetaDataProvider();

    }
    Markup::IXamlType GetXamlType(Windows::UI::Xaml::Interop::TypeName const& type) {
        return metadata.GetXamlType(type);
    }
    Markup::IXamlType GetXamlType(hstring const& name) { return metadata.GetXamlType(name); }
    com_array<Markup::XmlnsDefinition> GetXmlnsDefinitions() { return metadata.GetXmlnsDefinitions(); }
    void OnLaunched(LaunchActivatedEventArgs const&) {
        Resources().MergedDictionaries().Append(Controls::XamlControlsResources());
        AddWindow();
    }
};
}
int WINAPI wWinMain(HINSTANCE,HINSTANCE,PWSTR,int) {
    winrt::init_apartment(winrt::apartment_type::single_threaded);
    try {
        winrt::Microsoft::UI::Xaml::Application::Start([](auto&&){winrt::make<App>();});
        CapyLifecycle("application_loop_returned");
        // All windows have released their hosts. Finish canceled shader work
        // before the process unloads WinUI and graphics-driver resources.
        if(capy_finish_process()<0)return 1;
        CapyLifecycle("application_returned");
        return 0;
    } catch(winrt::hresult_error const& error) {
        MessageBoxW(nullptr,error.message().c_str(),L"Capy Canvas could not start",MB_OK|MB_ICONERROR);
        return 1;
    }
}
