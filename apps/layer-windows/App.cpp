#include "pch.h"
#include "CanvasWindow.h"
#include <fstream>
#include <shellapi.h>
#include <map>
#include <winrt/Microsoft.UI.Xaml.XamlTypeInfo.h>
#include <winrt/Windows.UI.Xaml.Interop.h>

namespace {
using namespace winrt;
using namespace Microsoft::UI::Xaml;
constexpr ULONG_PTR OpenFilesMessage=0x43415059;
constexpr size_t MaxForwardedFiles=64;
std::vector<std::wstring> LaunchFiles() {
    std::vector<std::wstring> files;int count=0;
    auto arguments=CommandLineToArgvW(GetCommandLineW(),&count);
    if(!arguments)return files;
    for(int i=1;i<count&&files.size()<MaxForwardedFiles;++i){
        wchar_t full[32768];auto length=GetFullPathNameW(arguments[i],32768,full,nullptr);
        if(length==0||length>=32768)continue;
        auto attributes=GetFileAttributesW(full);
        if(attributes!=INVALID_FILE_ATTRIBUTES&&!(attributes&FILE_ATTRIBUTE_DIRECTORY))files.emplace_back(full,length);
    }
    LocalFree(arguments);
    return files;
}
std::wstring OpenFilesClass() {
    std::wstring profile=L"default";
    if(auto size=GetEnvironmentVariableW(L"CAPY_SETTINGS_DIRECTORY",nullptr,0)){
        std::wstring value(size,L'\0');value.resize(GetEnvironmentVariableW(L"CAPY_SETTINGS_DIRECTORY",value.data(),size));
        wchar_t full[32768];auto length=GetFullPathNameW(value.c_str(),32768,full,nullptr);
        if(length&&length<32768)value.assign(full,length);
        CharLowerBuffW(value.data(),DWORD(value.size()));profile=L"profile:"+value;
    }
    uint64_t hash=14695981039346656037ull;
    for(auto c:profile){hash^=uint16_t(c);hash*=1099511628211ull;}
    wchar_t name[64];swprintf(name,64,L"CapyCanvasOpenFiles-%016llx",static_cast<unsigned long long>(hash));
    return name;
}
bool ForwardFiles(std::vector<std::wstring> const& files) {
    auto target=FindWindowExW(HWND_MESSAGE,nullptr,OpenFilesClass().c_str(),nullptr);
    if(!target)return false;
    std::wstring payload;
    for(auto const& file:files){payload+=file;payload.push_back(L'\0');}
    DWORD process=0;GetWindowThreadProcessId(target,&process);AllowSetForegroundWindow(process);
    COPYDATASTRUCT data{OpenFilesMessage,DWORD(payload.size()*sizeof(wchar_t)),payload.data()};
    DWORD_PTR accepted=FALSE;
    return SendMessageTimeoutW(target,WM_COPYDATA,0,LPARAM(&data),SMTO_ABORTIFHUNG,10000,&accepted)&&accepted==TRUE;
}
std::function<bool(std::vector<std::wstring>)>& ForwardedFiles() {
    static std::function<bool(std::vector<std::wstring>)> receive;
    return receive;
}
LRESULT CALLBACK OpenFilesProc(HWND window,UINT message,WPARAM wparam,LPARAM lparam) {
    if(message!=WM_COPYDATA)return DefWindowProcW(window,message,wparam,lparam);
    auto data=reinterpret_cast<COPYDATASTRUCT const*>(lparam);
    if(!data||data->dwData!=OpenFilesMessage||!data->lpData||data->cbData%sizeof(wchar_t)
        ||data->cbData>MaxForwardedFiles*32768*sizeof(wchar_t)||!ForwardedFiles())return FALSE;
    std::wstring_view payload(static_cast<wchar_t const*>(data->lpData),data->cbData/sizeof(wchar_t));
    std::vector<std::wstring> files;
    for(size_t start=0;start<payload.size()&&files.size()<MaxForwardedFiles;){
        auto end=std::min(payload.find(L'\0',start),payload.size());
        std::wstring path(payload.substr(start,end-start));start=end+1;
        auto attributes=GetFileAttributesW(path.c_str());
        if(!path.empty()&&attributes!=INVALID_FILE_ATTRIBUTES&&!(attributes&FILE_ATTRIBUTE_DIRECTORY))files.push_back(std::move(path));
    }
    return !files.empty()&&ForwardedFiles()(std::move(files));
}
void ReceiveForwardedFiles() {
    auto name=OpenFilesClass();
    if(FindWindowExW(HWND_MESSAGE,nullptr,name.c_str(),nullptr))return;
    WNDCLASSEXW type{sizeof(type)};type.lpfnWndProc=OpenFilesProc;type.hInstance=GetModuleHandleW(nullptr);type.lpszClassName=name.c_str();
    if(RegisterClassExW(&type))CreateWindowExW(0,name.c_str(),L"",0,0,0,0,0,HWND_MESSAGE,nullptr,type.hInstance,nullptr);
}
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
    void OpenForwarded(std::vector<std::wstring> files) {
        std::shared_ptr<CanvasWindow> target;
        for(HWND handle=GetTopWindow(nullptr);handle&&!target;handle=GetWindow(handle,GW_HWNDNEXT)){
            if(!IsWindowVisible(handle))continue;
            for(auto const& [id,window]:windows)if(window->Handle()==handle&&!window->Closing()){target=window;break;}
        }
        if(!target){AddWindow(std::move(files));return;}
        target->Present();
        target->OpenFiles(std::move(files));
    }
    void AddWindow(std::vector<std::wstring> files={}) {
        auto next=std::make_shared<CanvasWindow>(
            [weak=get_weak()]{if(auto self=weak.get())self->AddWindow();},
            [weak=get_weak()](uint64_t id){if(auto self=weak.get()){self->windows.erase(id);self->TraceWindows();}},
            !launchedWindow,
            [weak=get_weak()](uint64_t source){if(auto self=weak.get())for(auto const& [id,window]:self->windows)
                if(id!=source)window->RefreshWorkspaceSwitcher();});
        windows.emplace(next->Id(),next);
        launchedWindow=true;
        TraceWindows();
        if(!files.empty())next->OpenFiles(std::move(files));
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
        ForwardedFiles()=[weak=get_weak()](std::vector<std::wstring> files){
            auto self=weak.get();
            if(!self||self->windows.empty())return false;
            return Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread().TryEnqueue([weak,files=std::move(files)]()mutable{
                if(auto self=weak.get())self->OpenForwarded(std::move(files));
            });
        };
        ReceiveForwardedFiles();
        AddWindow(LaunchFiles());
    }
};
}
int WINAPI wWinMain(HINSTANCE,HINSTANCE,PWSTR,int) {
    winrt::init_apartment(winrt::apartment_type::single_threaded);
    if(auto files=LaunchFiles();!files.empty()&&ForwardFiles(files))return 0;
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
