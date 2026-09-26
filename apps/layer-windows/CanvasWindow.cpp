#include "pch.h"
#include "CanvasWindow.h"
#include "CanvasPointerSample.h"
#include "UiControls.h"
#include "ExternalImages.h"
#include <microsoft.ui.xaml.media.dxinterop.h>
#include <microsoft.ui.xaml.window.h>
#include <ShellScalingApi.h>
#include <CommCtrl.h>
#include <dwmapi.h>
#include <winrt/Windows.Graphics.h>
#include <winrt/Microsoft.UI.Xaml.Automation.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <fstream>

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Input;
using namespace Microsoft::UI::Xaml::Media;
using namespace Microsoft::UI::Windowing;
using namespace Windows::Foundation;
static int DispatchCanvasCommand(CapyHost* host,CanvasCommand const& command) {
    auto json=command.json.c_str();
    switch(command.kind){
        case CanvasCommandKind::Input:return capy_input(host,json);
        case CanvasCommandKind::Prediction:return capy_native_prediction(host,command.json=="true");
        case CanvasCommandKind::Document:return capy_document_action(host,json);
        case CanvasCommandKind::Workspace:return capy_workspace_action(host,json);
        case CanvasCommandKind::Overviews:return capy_overviews(host,json);
        case CanvasCommandKind::Glass:return capy_glass(host,json);
        case CanvasCommandKind::Action:return capy_action(host,json);
        case CanvasCommandKind::Filters:return capy_load_filter_directory(host,json);
        case CanvasCommandKind::DeviceLoss:return capy_test_device_loss(host);
        case CanvasCommandKind::TestDisplay:return capy_test_display(host,command.json=="hdr");
    }
    return -1;
}
static uint64_t Now() {
    LARGE_INTEGER ticks, frequency;
    QueryPerformanceCounter(&ticks); QueryPerformanceFrequency(&frequency);
    return uint64_t(ticks.QuadPart / frequency.QuadPart) * 1000000000ULL
        + uint64_t(ticks.QuadPart % frequency.QuadPart) * 1000000000ULL / uint64_t(frequency.QuadPart);
}
static std::chrono::nanoseconds RefreshPeriod() {
    DWM_TIMING_INFO info{};info.cbSize=sizeof(info);LARGE_INTEGER frequency;
    if(SUCCEEDED(DwmGetCompositionTimingInfo(nullptr,&info))&&info.qpcRefreshPeriod&&QueryPerformanceFrequency(&frequency))
        return std::chrono::nanoseconds(int64_t(double(info.qpcRefreshPeriod)*1e9/double(frequency.QuadPart)));
    return std::chrono::microseconds(16667);
}
void CapyLifecycle(char const* event) {
    if(!GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0))return;
    static std::mutex logMutex;
    std::lock_guard lock(logMutex);
    std::ofstream("lifecycle.log",std::ios::app)
        << GetCurrentProcessId() << " " << Now() << " " << event << "\n";
}
CanvasWindow::CanvasWindow(std::function<void()> create,std::function<void(uint64_t)> close,bool primary,
    std::function<void(uint64_t)> preferencesChanged)
    :windowId(window.AppWindow().Id().Value),primaryWindow(primary),
     createWindow(std::move(create)),onClosed(std::move(close)),workspacePreferencesChanged(std::move(preferencesChanged)) {
    // HWND/WindowId can be reused after an earlier window closes. Invalidate
    // its old diagnostic model before publishing the new live-window manifest.
    if(GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0))TraceState("ui-state","{}");
}
namespace {
LRESULT CALLBACK AltKeyMenuFilter(HWND window,UINT message,WPARAM wparam,LPARAM lparam,UINT_PTR id,DWORD_PTR){
    if(message==WM_SYSCOMMAND&&(wparam&0xFFF0)==SC_KEYMENU&&lparam==0)return 0;
    if(message==WM_NCDESTROY)RemoveWindowSubclass(window,AltKeyMenuFilter,id);
    return DefSubclassProc(window,message,wparam,lparam);
}
}
HWND CanvasWindow::Handle()const {
    HWND handle=nullptr;
    check_hresult(window.as<IWindowNative>()->get_WindowHandle(&handle));
    return handle;
}
void CanvasWindow::TraceState(char const* kind,std::string const& value)const {
    auto write=[&](std::string const& name){
        auto pending=name+".pending";
        {std::ofstream stream(pending);stream<<value;if(!stream)return;}
        // Local tracing is opt-in. A transient file scanner or diagnostic reader
        // can deny replacement; do not silently leave the previous state forever.
        auto source=to_hstring(pending),destination=to_hstring(name);
        for(unsigned attempt=0;;++attempt){
            if(MoveFileExW(source.c_str(),destination.c_str(),MOVEFILE_REPLACE_EXISTING))break;
            auto error=GetLastError();
            if(attempt==4||(error!=ERROR_SHARING_VIOLATION&&error!=ERROR_ACCESS_DENIED)){
                OutputDebugStringW((std::wstring(L"Capy trace replacement failed: ")+std::to_wstring(error)+L"\n").c_str());
                std::ofstream(name)<<value;DeleteFileW(source.c_str());break;
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }
    };
    write(std::string(kind)+"-"+std::to_string(GetCurrentProcessId())+"-"+std::to_string(windowId)+".json");
    // Preserve the initial window's paths for existing single-window fixtures.
    if(primaryWindow)write(std::string(kind)+".json");
}
std::string CanvasWindow::SystemTheme(){
    using winrt::Windows::UI::ViewManagement::UIColorType;
    auto ink=uiSettings.GetColorValue(UIColorType::Foreground),accent=uiSettings.GetColorValue(UIColorType::Accent);
    char hex[8];sprintf_s(hex,"#%02x%02x%02x",accent.R,accent.G,accent.B);
    return std::string(R"({"type":"system_theme_changed","theme":")")+(ink.R+ink.G+ink.B>384?"dark":"light")+R"(","accent":")"+hex+"\"}";
}
CanvasWindow::~CanvasWindow() {
    if(colorValues)uiSettings.ColorValuesChanged(colorValues);
    { std::lock_guard lock(mutex); closing=true; paused=false; }
    wake.notify_all();space.notify_all();
    if (renderer.joinable()) renderer.join();
    latencyTrace.Dump("latency-"+std::to_string(windowId));
    if (host) capy_destroy(host);
}
void CanvasWindow::Open() {
    try {
    dispatcher=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread();
    window.Title(L"Capy Canvas");
    window.ExtendsContentIntoTitleBar(true);
    auto titlebar=window.AppWindow().TitleBar();
    Windows::UI::Color transparent{0,0,0,0};
    titlebar.ButtonBackgroundColor(transparent);
    titlebar.ButtonInactiveBackgroundColor(transparent);
    titlebar.ButtonForegroundColor(Windows::UI::Color{255,225,225,229});
    titlebar.PreferredHeightOption(TitleBarHeightOption::Tall);
    root.RequestedTheme(ElementTheme::Default);

    canvasFocus.Content(panel);
    canvasFocus.HorizontalContentAlignment(HorizontalAlignment::Stretch);
    canvasFocus.VerticalContentAlignment(VerticalAlignment::Stretch);
    canvasFocus.IsTabStop(true);
    Automation::AutomationProperties::SetName(canvasFocus,L"Drawing canvas");
    root.Children().Append(canvasFocus);
    root.PreviewKeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
        if(auto self=weak.lock())if(!self->dialogOpen.load()&&(!self->header||!self->header->Key(e,true))
            &&e.Key()!=Windows::System::VirtualKey::Escape)self->Key(e,true);
    });
    // Native editors and captured controls cancel first; only an unhandled
    // Escape reaches the shared drawer and application shortcuts.
    root.KeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
        if(auto self=weak.lock();self&&!self->dialogOpen.load()&&e.Key()==Windows::System::VirtualKey::Escape)self->Key(e,true);
    });
    root.PreviewKeyUp([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
        if(auto self=weak.lock())if(!self->dialogOpen.load()&&(!self->header||!self->header->Key(e,false)))self->Key(e,false);
    });
    PointerEventHandler contact([](winrt::Windows::Foundation::IInspectable const&,PointerRoutedEventArgs const& e){
        CapyUi::setTouchContact(e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch);
    });
    root.AddHandler(UIElement::PointerPressedEvent(),box_value(contact),true);
    root.AddHandler(UIElement::PointerMovedEvent(),box_value(contact),true);
    root.PointerMoved([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
        if(auto self=weak.lock())self->ChromeMotion(e);
    });
    root.PointerExited([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
        if(auto self=weak.lock())self->ChromeMotion(e,true);
    });
    // Controlled test commands stay outside the production header.
    toolbar.Orientation(Orientation::Horizontal);toolbar.Spacing(4);
    toolbar.HorizontalAlignment(HorizontalAlignment::Center);toolbar.VerticalAlignment(VerticalAlignment::Bottom);
    toolbar.Margin({0,0,0,8});
    if(GetEnvironmentVariableW(L"CAPY_SMOKE_TEST",nullptr,0)){
        for(auto [kind,label]:{std::pair{ReplayKind::Stroke,L"Test stroke"},
            {ReplayKind::Pan,L"Test pan"},{ReplayKind::Backlog,L"Test backlog"},
            {ReplayKind::Pen,L"Test pen"},{ReplayKind::PenBegin,L"Test pen begin"},{ReplayKind::PenEnd,L"Test pen end"}}){
            Button test;test.Content(box_value(label));
            test.Click([weak=weak_from_this(),kind](auto&&,auto&&){if(auto self=weak.lock())self->Replay(kind);});
            toolbar.Children().Append(test);
        }
        Button reload;reload.Content(box_value(L"Test filter reload"));
        reload.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())
            self->Send(R"({"mode":"merge"})",CanvasCommandKind::Filters);});
        toolbar.Children().Append(reload);
        Button recover;recover.Content(box_value(L"Test GPU loss"));
        recover.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())
            self->Send("",CanvasCommandKind::DeviceLoss);});
        toolbar.Children().Append(recover);
        if(GetEnvironmentVariableW(L"CAPY_TEST_HDR",nullptr,0))for(bool hdr:{false,true}){Button test;test.Content(box_value(hdr?L"Test HDR output":L"Test SDR output"));test.Click([weak=weak_from_this(),hdr](auto&&,auto&&){if(auto self=weak.lock())self->Send(hdr?"hdr":"sdr",CanvasCommandKind::TestDisplay);});toolbar.Children().Append(test);}
    }
    root.Children().Append(toolbar);
    status.Text(L"Preparing canvas…");
    Microsoft::UI::Xaml::Automation::AutomationProperties::SetAutomationId(status,L"canvas-status");
    status.IsHitTestVisible(false);
    status.HorizontalAlignment(HorizontalAlignment::Center);
    status.VerticalAlignment(VerticalAlignment::Bottom);
    status.Margin(Thickness{0,0,0,40});
    root.Children().Append(status);
    window.Content(root);
    panel.Loaded([weak=weak_from_this()](auto&&,auto&&) { if(auto self=weak.lock()) self->Start(); });
    panel.SizeChanged([weak=weak_from_this()](auto&&,auto&&) { if(auto self=weak.lock()) self->Resize(); });
    panel.CompositionScaleChanged([weak=weak_from_this()](auto&&,auto&&) { if(auto self=weak.lock()) self->Resize(); });
    window.Activated([weak=weak_from_this()](auto&&, WindowActivatedEventArgs const& e) {
        if(auto self=weak.lock()){
            if(e.WindowActivationState()==WindowActivationState::Deactivated){
                // A WinUI submenu can deactivate the main HWND while focus
                // stays inside its owned popup tree. Check after activation
                // settles before retiring the workspace contact or menu.
                self->dispatcher.TryEnqueue([weak]{if(auto self=weak.lock();self&&!self->closed&&self->workspace){
                    auto owner=self->Handle();
                    auto foreground=GetForegroundWindow();
                    // Tablet/IME helper HWNDs can briefly own activation without
                    // presenting another application. Preserve the idle placement
                    // across those transitions; visible app switches still blur.
                    if(IsIconic(owner)||(IsWindowVisible(foreground)&&GetAncestor(foreground,GA_ROOTOWNER)!=owner)){
                        self->workspace->CancelGesture();self->heldKeys.clear();self->sentModifiers.store(0);
                        self->Send(R"({"type":"blur"})",CanvasCommandKind::Input);
                    }
                }});
            }else{self->Resize();self->RefreshWorkspaceSwitcher();}
        }
    });
    window.AppWindow().Closing([weak=weak_from_this()](auto&&,AppWindowClosingEventArgs const& e) {
        if(auto self=weak.lock()) {
            if(!self->closed) {e.Cancel(true);self->RequestClose();}
        }
    });
    window.AppWindow().Changed([weak=weak_from_this()](auto&&,AppWindowChangedEventArgs const& e){
        if(e.DidPresenterChange())if(auto self=weak.lock())self->Resize();
    });
    window.AppWindow().Resize({1440,1000});
    // Benchmark placement is opt-in; ordinary launches use Windows placement.
    if(GetEnvironmentVariableW(L"CAPY_TEST_DISPLAY",nullptr,0)) {
        POINT position{LONG_MIN,LONG_MIN};
        EnumDisplayMonitors(nullptr,nullptr,[](HMONITOR monitor,HDC,LPRECT,LPARAM data)->BOOL {
            MONITORINFOEXW info{};info.cbSize=sizeof(info);
            DEVMODEW mode{};mode.dmSize=sizeof(mode);
            if(GetMonitorInfoW(monitor,&info)&&EnumDisplaySettingsW(info.szDevice,ENUM_CURRENT_SETTINGS,&mode)
                &&(GetEnvironmentVariableW(L"CAPY_TEST_PRIMARY",nullptr,0)?
                    (info.dwFlags&MONITORINFOF_PRIMARY)!=0:mode.dmDisplayFrequency>=120)) {
                *reinterpret_cast<POINT*>(data)={info.rcWork.left+48,info.rcWork.top+48};
                return FALSE;
            }
            return TRUE;
        },reinterpret_cast<LPARAM>(&position));
        if(position.x!=LONG_MIN) window.AppWindow().Move({position.x,position.y});
    }
    window.Activate();
    } catch(...) {Stop();throw;}
}
void CanvasWindow::Resize() {
    // Retain the surface and caption geometry while minimized. Activation or
    // layout will supply the restored window's dimensions and DPI.
    if(closing||closed||IsIconic(Handle()))return;
    float scale=panel.CompositionScaleX();
    Size next{uint32_t(std::max(1L, std::lround(panel.ActualWidth()*scale))),
              uint32_t(std::max(1L, std::lround(panel.ActualHeight()*scale))),scale};
    // Physical-pixel drag regions leave the app controls and system caption buttons interactive.
    auto titlebar=window.AppWindow().TitleBar();
    bool caption=window.AppWindow().Presenter().Kind()==AppWindowPresenterKind::Overlapped;
    auto left=caption?titlebar.LeftInset():0;
    auto right=caption?titlebar.RightInset():0;
    auto height=caption?titlebar.Height():0;
    // AppWindow can briefly return a negative inset even after IsIconic clears.
    // Preserve all caption projections together until Windows has valid metrics;
    // the swap chain can still follow the current client size below.
    if(left<0||right<0||height<0){
        if(!captionRetry){
            captionRetry=dispatcher.CreateTimer();
            captionRetry.IsRepeating(false);
            captionRetry.Interval(std::chrono::milliseconds(16));
            captionRetry.Tick([weak=weak_from_this()](auto&&,auto&&){
                if(auto self=weak.lock())self->Resize();
            });
        }
        if(!captionRetry.IsRunning())captionRetry.Start();
    }else{
        if(captionRetry)captionRetry.Stop();
        if(workspace)workspace->SetTitlebarInsets(float(left)/scale,float(right)/scale,float(height)/scale);
        std::vector<Windows::Graphics::RectInt32> regions,inputRegions;
        if(header){
            header->SetFullscreen(!caption);
            header->SetInsets(float(left)/scale,float(right)/scale);
            if(caption){regions=header->DragRegions(scale,next.width);inputRegions=header->InputRegions(scale,next.width);}
        } else if(caption)regions.push_back(Windows::Graphics::RectInt32{
            left,0,std::max(0,int32_t(next.width)-left-right),int32_t(48*scale)});
        if(caption){
            auto equal=[](auto const& a,auto const& b){
                return a.size()==b.size()&&std::equal(a.begin(),a.end(),b.begin(),[](auto x,auto y){
                    return x.X==y.X&&x.Y==y.Y&&x.Width==y.Width&&x.Height==y.Height;
                });
            };
            bool same=captionRegionsValid&&equal(regions,captionRegions)&&equal(inputRegions,captionInputRegions);
            // Painting snapshots can refresh header state without moving controls.
            // Only changed hit geometry needs a non-client window update.
            if(!same){
                titlebar.SetDragRectangles(regions);
                // Caption changes alone do not reliably retire pen/touch hit
                // regions. Publish the complementary interactive regions too.
                auto source=Microsoft::UI::Input::InputNonClientPointerSource::GetForWindowId(window.AppWindow().Id());
                source.SetRegionRects(Microsoft::UI::Input::NonClientRegionKind::Passthrough,inputRegions);
                captionRegions=std::move(regions);captionInputRegions=std::move(inputRegions);captionRegionsValid=true;
            }
        }else captionRegionsValid=false;
    }
    {
        std::lock_guard lock(mutex);
        bool changed=next.width!=desired.width||next.height!=desired.height||next.scale!=desired.scale;
        desired=next;if(host&&!closing&&changed)resize=true;
    }
    wake.notify_one();
    PublishGlass();
}
void CanvasWindow::PublishGlass(){
    if(closing||closed||!panel||!workspace)return;
    if(header)header->SetDrawerSources(workspace->DrawerSources());
    Windows::Data::Json::JsonArray connections;auto regions=workspace->Glass(panel,connections);
    if(header)header->AppendGlass(regions,panel);
    Windows::Data::Json::JsonObject request;request.Insert(L"regions",regions);request.Insert(L"connections",connections);
    auto json=to_string(request.Stringify());
    if(json==lastGlass)return;lastGlass=json;Send(json,CanvasCommandKind::Glass);
}
void CanvasWindow::Start() {
    if(host||closing) return;
    Resize();
    auto native=panel.as<ISwapChainPanelNative>();
    host=capy_create(native.get(),desired.width,desired.height,desired.scale);
    if(!host) {status.Text(to_hstring(capy_error()));return;}
    capy_set_window(host,Handle());
    SetWindowSubclass(Handle(),AltKeyMenuFilter,1,0);
    auto dark=panel.ActualTheme()==ElementTheme::Dark;
    capy_action(host,SystemTheme().c_str());
    colorValues=uiSettings.ColorValuesChanged([weak=weak_from_this()](auto&&,auto&&){
        if(auto self=weak.lock())self->dispatcher.TryEnqueue([weak]{
            if(auto self=weak.lock();self&&!self->closed&&self->host)self->Send(self->SystemTheme());
        });
    });
    window.AppWindow().TitleBar().ButtonForegroundColor(dark?
        Windows::UI::Color{255,225,225,229}:Windows::UI::Color{255,32,32,36});
    auto catalog=capy_query(host,R"({"type":"catalog"})");
    if(!catalog){status.Text(to_hstring(capy_error()));return;}
    std::unique_ptr<char,decltype(&capy_string_free)> ownedCatalog(catalog,capy_string_free);
    workspace=std::make_unique<WorkspaceView>([weak=weak_from_this()](std::string json){
        if(auto self=weak.lock())self->Send(std::move(json));
    },Windows::Data::Json::JsonObject::Parse(to_hstring(catalog)),[weak=weak_from_this()](std::string json){
        if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Overviews);
    },[weak=weak_from_this()](CanvasQueryKind kind,std::string json,PreviewReply reply){
        if(auto self=weak.lock())return self->RequestPreviews(kind,std::move(json),std::move(reply));return false;
    },[weak=weak_from_this()](bool open){if(auto self=weak.lock()){self->workspacePopupOpen=open;self->UpdatePopup();}},
    [weak=weak_from_this()](std::string json){if(auto self=weak.lock()){
        self->canvasFocus.Focus(FocusState::Programmatic);
        self->Send(std::move(json),CanvasCommandKind::Document);
    }},[weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Input);});
    workspace->SetWindowId(windowId);
    workspace->SetGlassChanged([weak=weak_from_this()]{if(auto self=weak.lock())self->PublishGlass();});
    root.Children().InsertAt(1,workspace->Root());
    auto send=[weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json));};
    auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(catalog));
    header=std::make_unique<HeaderView>(send,model,
        [weak=weak_from_this()](bool open){if(auto self=weak.lock())self->Popup(open);},
        [weak=weak_from_this()]{if(auto self=weak.lock())self->Resize();},
        [weak=weak_from_this()]{if(auto self=weak.lock())self->Fullscreen();},
        [weak=weak_from_this()]{
            auto self=weak.lock();
            if(!self||self->closing||!self->createWindow)throw hresult_error(E_ABORT,L"The source window is closing.");
            self->createWindow();
        },
        [weak=weak_from_this()](CanvasQueryKind kind,std::string json,PreviewReply reply){
            if(auto self=weak.lock())return self->RequestPreviews(kind,std::move(json),std::move(reply));return false;
        },[weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Input);},
        [weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Document);});
    root.Children().Append(header->Root());
    settings=std::make_unique<SettingsView>(send,model,root.XamlRoot(),
        [weak=weak_from_this()](KeyRoutedEventArgs const& e,bool pressed){if(auto self=weak.lock())self->Key(e,pressed);},
        [weak=weak_from_this()](std::string error){if(auto self=weak.lock())self->Fail("Cannot open Preferences: "+error);},
        [weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();},
        [weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Document);});
    documents=std::make_unique<DocumentView>(
        [weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Document);},
        model,window,[weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();},
        [weak=weak_from_this()](CanvasQueryKind kind,std::string json,PreviewReply reply){if(auto self=weak.lock())return self->RequestPreviews(kind,std::move(json),std::move(reply));return false;},
        [weak=weak_from_this()](std::string error){if(auto self=weak.lock())self->Fail(std::move(error));});
    panel.AllowDrop(true);
    auto dropKind=std::make_shared<CapyUi::DropKind>(CapyUi::DropKind::Images);
    panel.DragEnter([dropKind](auto&&,DragEventArgs const& event){if(CapyUi::fileDrag(event))CapyUi::classifyDrop(event,dropKind);});
    panel.DragOver([weak=weak_from_this(),dropKind](auto&&,DragEventArgs const& event){if(auto self=weak.lock();self&&!self->closing&&CapyUi::fileDrag(event)){
        auto kind=*dropKind;auto commands=CapyUi::array(CapyUi::object(self->lastModel,L"state"),L"commands");
        bool enabled=kind!=CapyUi::DropKind::Mixed&&CapyUi::flag(CapyUi::find(commands,L"id",kind==CapyUi::DropKind::Drawings?L"open_document":L"import_image"),L"enabled");
        using winrt::Windows::ApplicationModel::DataTransfer::DataPackageOperation;
        event.AcceptedOperation(enabled?DataPackageOperation::Copy:DataPackageOperation::None);event.DragUIOverride().Caption(CapyUi::dropCaption(kind));event.Handled(true);
    }});
    panel.Drop([weak=weak_from_this()](auto&&,DragEventArgs const& event){if(auto self=weak.lock();self&&!self->closing&&CapyUi::fileDrag(event)){
        auto action=CapyUi::imageDrop(CapyUi::object(self->lastModel,L"state"));auto point=event.GetPosition(self->panel);auto scale=self->panel.XamlRoot().RasterizationScale();
        action.Insert(L"screen",CapyUi::O({{L"x",CapyUi::N(point.X*scale)},{L"y",CapyUi::N(point.Y*scale)}}));
        CapyUi::receiveImageDrop(event,action,[weak](std::string json){if(auto self=weak.lock();self&&!self->closing)self->Send(std::move(json),CanvasCommandKind::Document);});
    }});
    selectionDialog=std::make_unique<SelectionDialog>(send,model,root.XamlRoot(),
        [weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();});
    workspaceDialogs=std::make_unique<WorkspaceDialogs>(send,model,root.XamlRoot(),
        [weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();},
        [weak=weak_from_this()](std::string error){if(auto self=weak.lock())self->Fail(std::move(error));});
    workspaceStorage=std::make_unique<WorkspaceStorageView>(
        [weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Workspace);},
        window,[weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();});
    root.Children().Append(workspaceStorage->Root());
    workspaceManager=std::make_unique<WorkspaceManagerView>(
        [weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Workspace);},
        root.XamlRoot(),[weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();});
    if(auto snapshot=capy_snapshot(host)){
        std::unique_ptr<char,decltype(&capy_string_free)> owned(snapshot,capy_string_free);
        ApplyModel(Windows::Data::Json::JsonObject::Parse(to_hstring(snapshot)));
    }
    {std::lock_guard lock(mutex);revision=capy_view_revision(host);inputScale=desired.scale;}
    status.Text(L"Preparing brushes…");
    inputController=Microsoft::UI::Dispatching::DispatcherQueueController::CreateOnDedicatedThread();
    inputDispatcher=inputController.DispatcherQueue();
    renderer=std::jthread([this]{Run();});
}
bool CanvasWindow::RequestPreviews(CanvasQueryKind kind,std::string json,PreviewReply reply) {
    {std::lock_guard lock(mutex);
        if(closing||!host||rendererDone.load()||transportFailed||(inputStopped&&kind!=CanvasQueryKind::Workspace))return false;
        if(!previewWork.Push(CanvasQuery{kind,std::move(json),std::move(reply)}))return false;
    }
    wake.notify_one();return true;
}
void CanvasWindow::Send(std::string json, CanvasCommandKind kind) {
    bool overflow=false;
    {
        std::lock_guard lock(mutex);
        if(closing||!host||rendererDone.load()||transportFailed)return;
        if(!work.Push(CanvasCommand{kind,std::move(json)}))overflow=transportFailed=true;
    }
    // UI callbacks never wait for the render worker. An exhausted command
    // channel fails explicitly, drains accepted work and cancels active input.
    if(overflow)Fail("Canvas input queue is full. Close and reopen the window to continue.");
    wake.notify_one();space.notify_all();
}
bool CanvasWindow::SendIndependent(CanvasWork item) {
    std::unique_lock lock(mutex);
    if(!work.CanPush(item)&&GetEnvironmentVariableW(L"CAPY_TRACE_TRANSPORT",nullptr,0))
        std::ofstream("input-transport.log",std::ios::app) << "waiting for bounded queue capacity\n";
    space.wait(lock,[&]{return closing||rendererDone.load()||transportFailed||inputStopped||work.CanPush(item);});
    if(closing||rendererDone.load()||transportFailed||inputStopped)return false;
    work.Push(std::move(item));
    lock.unlock();wake.notify_one();
    return true;
}
void CanvasWindow::Replay(ReplayKind kind) {
    inputDispatcher.TryEnqueue([weak=weak_from_this(),kind] {
        bool pan=kind==ReplayKind::Pan,backlog=kind==ReplayKind::Backlog;
        bool pen=kind==ReplayKind::Pen||kind==ReplayKind::PenBegin||kind==ReplayKind::PenEnd;
        auto self=weak.lock();if(!self)return;
        Size size;uint64_t view;
        {std::lock_guard lock(self->mutex);if(self->closing)return;size=self->desired;view=self->revision;}
        std::vector<CapyPointer> records;records.reserve(CanvasWorkBuffer::PointerBatch);
        uint32_t count=backlog?32768:pan?3:42;
        if(kind==ReplayKind::PenEnd&&!self->replayTime)return;
        uint64_t start=kind==ReplayKind::PenEnd?self->replayTime:Now()-count*1000000ULL;
        if(kind==ReplayKind::PenBegin)self->replayTime=start;
        uint32_t first=kind==ReplayKind::PenEnd?21:0,limit=kind==ReplayKind::PenBegin?21:count;
        for(uint32_t i=first;i<limit;i++) {
            CapyPointer p{};
            p.id=77;p.sequence=++self->sequence;p.timestamp_ns=start+i*1000000ULL;
            p.view_revision=view;p.tool=pen?0:1;p.button=pan?1:0;p.flags=2;
            p.pressure=pen?0.2f+0.7f*float(i)/float(count-1):1.0f;
            if(pen){p.tilt_x=0.4f*std::sin(float(i)/7);p.tilt_y=0.2f*std::cos(float(i)/7);p.twist=float(i)/42;}
            p.phase=i==0?1:i==count-1?3:2;
            p.x=size.width*0.4f+(pan?0.0f:float(backlog?i%42:i)*5);
            p.y=pan?(i==0?size.height*0.5f:28.0f):size.height*0.5f+24.0f*std::sin(float(i)/6);
            records.push_back(p);
            if(records.size()==CanvasWorkBuffer::PointerBatch){
                if(!self->SendIndependent(std::move(records)))return;
                records={};records.reserve(CanvasWorkBuffer::PointerBatch);
            }
        }
        if(!records.empty()&&!self->SendIndependent(std::move(records)))return;
        if(kind==ReplayKind::PenEnd)self->replayTime=0;
        if(pen)CapyLifecycle(kind==ReplayKind::PenBegin?"test_pen_begin_queued":
            kind==ReplayKind::PenEnd?"test_pen_end_queued":"test_pen_queued");
    });
}

void CanvasWindow::StartInput() {
    try {
        {std::lock_guard lock(mutex);if(closing)return;}
        using namespace Microsoft::UI::Input;
        inputSource=panel.CreateCoreIndependentInputSource(
            InputPointerSourceDeviceKinds::Mouse|InputPointerSourceDeviceKinds::Pen|InputPointerSourceDeviceKinds::Touch);
        // The renderer draws the brush cursor. Scope native cursor suppression
        // to the canvas input target so XAML buttons and editors keep theirs.
        inputSource.Cursor(nullptr);
        // The OS supplies prediction; shared Rust keeps it out of document truth.
        try {
            pointerPredictor=PointerPredictor::CreateForInputPointerSource(inputSource);
            pointerPredictor.PredictionTime(std::chrono::milliseconds(16));
        } catch(hresult_error const&) { pointerPredictor=nullptr; }
        SendIndependent(CanvasCommand{CanvasCommandKind::Prediction,pointerPredictor?"true":"false"});
        pickerHold=GestureRecognizer();pickerHold.GestureSettings(GestureSettings::Hold);
        pickerHold.Holding([weak=weak_from_this()](auto&&,HoldingEventArgs const& e){
            auto self=weak.lock();
            if(!self||e.HoldingState()!=HoldingState::Started||!self->holdContact||self->contacts.size()!=1)return;
            float scale;{std::lock_guard lock(self->mutex);if(self->closing)return;scale=self->inputScale;}
            auto at=e.Position();auto id=*self->holdContact;
            char json[160];
            std::snprintf(json,sizeof json,R"({"type":"color_picker_hold","id":%u,"position":[%.3f,%.3f],"offset":%.3f})",
                id,at.X*scale,at.Y*scale,self->PickerOffset(scale));
            self->SendIndependent(CanvasCommand{CanvasCommandKind::Input,json});
        });
        inputSource.PointerPressed([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){self->Pointer(e,1);self->PickerHold(e,1);}
        });
        inputSource.PointerMoved([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){auto phase=e.CurrentPoint().IsInContact()?2u:0u;self->Pointer(e,phase);self->PickerHold(e,phase);}
        });
        inputSource.PointerReleased([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){self->Pointer(e,3);self->PickerHold(e,3);}
        });
        inputSource.PointerCaptureLost([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){self->Pointer(e,4);self->PickerHold(e,4);}
        });
        inputSource.PointerRoutedAway([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){self->Pointer(e,4);self->PickerHold(e,4);}
        });
        inputSource.PointerRoutedReleased([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){self->Pointer(e,4);self->PickerHold(e,4);}
        });
        inputSource.PointerWheelChanged([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock())self->Wheel(e);
        });
        inputSource.PointerExited([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock();self&&!e.CurrentPoint().IsInContact())
                self->SendIndependent(CanvasCommand{CanvasCommandKind::Input,R"({"type":"cursor_leave"})"});
        });
    } catch(hresult_error const& error) {Fail(to_string(error.message()));}
}

float CanvasWindow::PickerOffset(float scale)const{
    UINT dpiX=0,dpiY=0;
    auto monitor=MonitorFromWindow(Handle(),MONITOR_DEFAULTTONEAREST);
    float logical=44;
    if(monitor&&SUCCEEDED(GetDpiForMonitor(monitor,MDT_RAW_DPI,&dpiX,&dpiY))&&dpiY>0)
        logical=std::clamp(float(dpiY)*10.f/25.4f/std::max(scale,.01f),36.f,64.f);
    return logical*scale;
}
void CanvasWindow::CancelPickerHold(){
    if(holdContact&&pickerHold)pickerHold.CompleteGesture();
    holdContact.reset();
}
void CanvasWindow::PickerHold(Microsoft::UI::Input::PointerEventArgs const& e, uint32_t phase){
    using namespace Microsoft::UI::Input;
    if(!pickerHold)return;
    auto point=e.CurrentPoint();auto id=point.PointerId();
    bool touch=point.PointerDeviceType()==PointerDeviceType::Touch;
    if(phase==1){
        bool first=contacts.empty();contacts.insert(id);
        if(touch&&first){CancelPickerHold();holdContact=id;pickerHold.ProcessDownEvent(point);}
        else CancelPickerHold();
    }else if(phase==2){
        if(holdContact==id)pickerHold.ProcessMoveEvents(e.GetIntermediatePoints());
    }else if(phase==3||phase==4){
        contacts.erase(id);
        if(holdContact==id){
            if(phase==3)pickerHold.ProcessUpEvent(point);
            CancelPickerHold();
        }
    }
}
void CanvasWindow::Pointer(Microsoft::UI::Input::PointerEventArgs const& e, uint32_t phase) {
    auto arrival=latencyTrace.enabled?Now():0;
    uint64_t view;float scale;
    {std::lock_guard lock(mutex);if(closing)return;view=revision;scale=inputScale;}
    std::vector<CapyPointer> samples;
    auto points=e.GetIntermediatePoints();
    // WinUI returns newest first. Phase boundaries use only the current point.
    uint32_t count=(phase==0||phase==2)?points.Size():1;
    samples.reserve(CanvasWorkBuffer::PointerBatch);
    if(phase==1)dispatcher.TryEnqueue([weak=weak_from_this()]{
        if(auto self=weak.lock())if(!self->closing)self->canvasFocus.Focus(FocusState::Pointer);
    });
    if(phase==1&&!SyncContactModifiers(e.KeyModifiers()))return;
    uint32_t deviceFlags=0;
    auto current=e.CurrentPoint();
    if(current.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Pen) {
        POINTER_INFO info{};
        POINTER_DEVICE_INFO device{};
        if(GetPointerInfo(current.PointerId(),&info) && GetPointerDevice(info.sourceDevice,&device)
            && device.pointerDeviceType==POINTER_DEVICE_TYPE_EXTERNAL_PEN) deviceFlags=0x40;
    }
    auto capture=[&](Microsoft::UI::Input::PointerPoint const& point,bool predicted) {
        auto props=point.Properties();
        auto type=point.PointerDeviceType();
        uint32_t tool=type==Microsoft::UI::Input::PointerDeviceType::Mouse?1:
            type==Microsoft::UI::Input::PointerDeviceType::Touch?3:props.IsEraser()?2:0;
        auto pos=point.Position();
        auto radians=[](float degrees){return degrees*0.017453292519943295f;};
        CapyPointer p{};
        p.id=point.PointerId();p.timestamp_ns=point.Timestamp()*1000;
        p.sequence=++sequence;p.view_revision=view;p.x=pos.X*scale;p.y=pos.Y*scale;
        p.pressure=tool==1?(point.IsInContact()?1.0f:0.0f):props.Pressure();
        p.tilt_x=radians(props.XTilt());p.tilt_y=radians(props.YTilt());p.twist=radians(props.Twist());
        p.phase=phase;p.tool=tool;
        p.button=CanvasPointerButton(tool,props.IsMiddleButtonPressed(),props.IsRightButtonPressed(),props.IsXButton1Pressed()||props.IsXButton2Pressed());
        p.flags=deviceFlags|(predicted?1:0)|(props.IsPrimary()?2:0)|(props.IsBarrelButtonPressed()?4:0)|(props.IsInverted()?8:0);
        if(!PrepareCanvasPrediction(p))return true;
        latencyTrace.Input(p,arrival);
        samples.push_back(p);
        if(GetEnvironmentVariableW(L"CAPY_TRACE_INPUT",nullptr,0)) std::ofstream("pointer-input.log",std::ios::app) << p.phase << " " << p.x << " " << p.y << " " << p.timestamp_ns << std::endl;
        if(samples.size()==CanvasWorkBuffer::PointerBatch) {
            if(!SendIndependent(std::move(samples)))return false;
            samples={};samples.reserve(CanvasWorkBuffer::PointerBatch);
        }
        return true;
    };
    for(uint32_t i=count;i>0;--i) {
        auto point=(phase==0||phase==2)?points.GetAt(i-1):e.CurrentPoint();
        if(!capture(point,false))return;
    }
    if(pointerPredictor&&phase==2) {
        try {
            auto predicted=pointerPredictor.GetPredictedPoints(e.CurrentPoint());
            std::sort(predicted.begin(),predicted.end(),[](auto const& a,auto const& b){return a.Timestamp()<b.Timestamp();});
            for(auto const& point:predicted)if(point.Timestamp()>=e.CurrentPoint().Timestamp()) {
                if(!capture(point,true))return;
            }
        } catch(hresult_error const&) {
            pointerPredictor.Close();pointerPredictor=nullptr;
            SendIndependent(CanvasCommand{CanvasCommandKind::Prediction,"false"});
        }
    }
    if(!samples.empty()&&!SendIndependent(std::move(samples)))return;
    e.Handled(true);
}
void CanvasWindow::Wheel(Microsoft::UI::Input::PointerEventArgs const& e) {
    auto point=e.CurrentPoint();auto properties=point.Properties();
    float density;Size size;
    {std::lock_guard lock(mutex);if(closing)return;size=desired;density=inputScale;}
    auto modifiers=e.KeyModifiers();
    using Mod=Windows::System::VirtualKeyModifiers;
    bool zoom=(modifiers&Mod::Control)!=Mod::None;
    bool horizontal=properties.IsHorizontalMouseWheel();
    UINT units=3;
    SystemParametersInfoW(horizontal?SPI_GETWHEELSCROLLCHARS:SPI_GETWHEELSCROLLLINES,0,&units,0);
    float distance=units==WHEEL_PAGESCROLL?
        (horizontal?float(size.width):float(size.height))/density:float(units)*16.0f;
    // Win32 vertical wheel is positive toward the user-facing top; shared
    // scrolling uses DOM-style positive-down deltas. Horizontal is positive-right.
    float delta=float(properties.MouseWheelDelta())/WHEEL_DELTA*(zoom?100.0f:distance);
    auto position=point.Position();
    CanvasScroll scroll{position.X*density,position.Y*density,
        horizontal?delta:0.0f,horizontal?0.0f:-delta,density,zoom,
        !horizontal&&(modifiers&Mod::Shift)!=Mod::None};
    if(SendIndependent(scroll))e.Handled(true);
}
void CanvasWindow::Key(KeyRoutedEventArgs const& e,bool pressed) {
    if(closing||closed)return;
    using VirtualKey=Windows::System::VirtualKey;
    auto key=e.Key();
    if(pressed&&key==VirtualKey::Escape&&workspace&&workspace->CancelGesture()){e.Handled(true);return;}
    auto focused=FocusManager::GetFocusedElement(root.XamlRoot());

    std::wstring name;
    switch(key) {
    case VirtualKey::Shift:name=L"shift";break;
    case VirtualKey::Control:name=L"control";break;
    case VirtualKey::Menu:name=L"alt";break;
    case VirtualKey::Escape:name=L"escape";break;
    case VirtualKey::Space:name=L" ";break;
    case VirtualKey::Enter:name=L"enter";break;
    case VirtualKey::Tab:name=L"tab";break;
    case VirtualKey::Back:name=L"backspace";break;
    case VirtualKey::Delete:name=L"delete";break;
    case VirtualKey::Insert:name=L"insert";break;
    case VirtualKey::Home:name=L"home";break;
    case VirtualKey::End:name=L"end";break;
    case VirtualKey::PageUp:name=L"pageup";break;
    case VirtualKey::PageDown:name=L"pagedown";break;
    case VirtualKey::Left:name=L"arrowleft";break;
    case VirtualKey::Right:name=L"arrowright";break;
    case VirtualKey::Up:name=L"arrowup";break;
    case VirtualKey::Down:name=L"arrowdown";break;
    default:
        if(key>=VirtualKey::F1&&key<=VirtualKey::F24)
            name=L"f"+std::to_wstring(uint32_t(key)-uint32_t(VirtualKey::F1)+1);
        else {
            BYTE state[256]{};
            GetKeyboardState(state);
            // Translate the layout's printable key without Ctrl/Alt changing
            // it into a control character. Flag 4 leaves dead-key state intact.
            state[VK_CONTROL]=state[VK_LCONTROL]=state[VK_RCONTROL]=0;
            state[VK_MENU]=state[VK_LMENU]=state[VK_RMENU]=0;
            wchar_t characters[8]{};
            int count=ToUnicodeEx(uint32_t(key),e.KeyStatus().ScanCode,state,characters,8,4,GetKeyboardLayout(0));
            if(count>0&&characters[0]>=L' ')name.assign(characters,count);
        }
    }
    // Release the same key identity even when Shift/layout changes while held.
    auto held=heldKeys.find(uint32_t(key));
    if(held!=heldKeys.end())name=held->second;
    if(name.empty())return;
    if(pressed)heldKeys.try_emplace(uint32_t(key),name);
    else heldKeys.erase(uint32_t(key));
    bool canvas=focused&&focused==canvasFocus;
    // Ordinary buttons keep application shortcuts after a click. Their native
    // activation/navigation keys, editors and open menus retain keyboard input.
    // Releases still clear shared held state when focus moves during a gesture.
    bool button=focused&&bool(focused.try_as<Primitives::ButtonBase>());
    bool ownedKeys=false;
    if(key==VirtualKey::Space||key==VirtualKey::Enter||key==VirtualKey::Delete||((key==VirtualKey::Z||key==VirtualKey::Y)&&(GetKeyState(VK_CONTROL)&0x8000)))
        for(auto node=focused.try_as<DependencyObject>();node&&!ownedKeys;node=VisualTreeHelper::GetParent(node))
            if(auto element=node.try_as<FrameworkElement>())if(auto tag=element.Tag().try_as<Windows::Data::Json::JsonObject>())ownedKeys=CapyUi::flag(tag,L"native_keys");
    bool navigation=key==VirtualKey::Space||key==VirtualKey::Enter||key==VirtualKey::Tab||
        key==VirtualKey::Escape||key==VirtualKey::Left||key==VirtualKey::Right||
        key==VirtualKey::Up||key==VirtualKey::Down||key==VirtualKey::Home||key==VirtualKey::End||
        key==VirtualKey::PageUp||key==VirtualKey::PageDown||key==VirtualKey::F2||key==VirtualKey::F10||
        key==VirtualKey::Menu||((GetKeyState(VK_MENU)&0x8000)&&!(GetKeyState(VK_CONTROL)&0x8000));
    // F11 remains a window action while a toolbar button or native field has focus.
    bool editing=key!=VirtualKey::F11&&(ownedKeys||menuOpen.load()||(!canvas&&(!button||navigation)));
    if(key==VirtualKey::F4&&(GetKeyState(VK_MENU)&0x8000))return;
    using namespace Windows::Data::Json;
    JsonObject modifiers;
    modifiers.Insert(L"command",JsonValue::CreateBooleanValue((GetKeyState(VK_CONTROL)&0x8000)!=0));
    modifiers.Insert(L"shift",JsonValue::CreateBooleanValue((GetKeyState(VK_SHIFT)&0x8000)!=0));
    modifiers.Insert(L"alt",JsonValue::CreateBooleanValue((GetKeyState(VK_MENU)&0x8000)!=0));
    JsonObject input;
    input.Insert(L"type",JsonValue::CreateStringValue(L"key"));
    input.Insert(L"key",JsonValue::CreateStringValue(name));
    input.Insert(L"pressed",JsonValue::CreateBooleanValue(pressed));
    input.Insert(L"repeat",JsonValue::CreateBooleanValue(pressed&&e.KeyStatus().WasKeyDown));
    input.Insert(L"editing",JsonValue::CreateBooleanValue(editing));
    input.Insert(L"modifiers",modifiers);
    sentModifiers.store(((GetKeyState(VK_CONTROL)&0x8000)?1u:0u)|((GetKeyState(VK_SHIFT)&0x8000)?2u:0u)|((GetKeyState(VK_MENU)&0x8000)?4u:0u));
    Send(to_string(input.Stringify()),CanvasCommandKind::Input);
    if(!editing)e.Handled(true);
}
bool CanvasWindow::SyncContactModifiers(Windows::System::VirtualKeyModifiers held) {
    using Mod=Windows::System::VirtualKeyModifiers;
    uint32_t state=((held&Mod::Control)!=Mod::None?1u:0u)|((held&Mod::Shift)!=Mod::None?2u:0u)|((held&Mod::Menu)!=Mod::None?4u:0u);
    uint32_t previous=sentModifiers.exchange(state);
    for(auto [bit,name]:{std::pair{1u,"control"},std::pair{2u,"shift"},std::pair{4u,"alt"}}){
        if(!((previous^state)&bit))continue;
        auto json=std::string(R"({"type":"key","key":")")+name+R"(","pressed":)"+((state&bit)?"true":"false")+
            R"(,"repeat":false,"editing":false,"modifiers":{"command":)"+((state&1)?"true":"false")+
            R"(,"shift":)"+((state&2)?"true":"false")+R"(,"alt":)"+((state&4)?"true":"false")+"}}";
        if(!SendIndependent(CanvasCommand{CanvasCommandKind::Input,std::move(json)}))return false;
    }
    return true;
}

int CanvasWindow::DispatchWork(CanvasWork const& item,bool retiring) {
    if(auto points=std::get_if<std::vector<CapyPointer>>(&item)){
        if(retiring||points->empty())return 0;
        auto const& first=points->front();auto const& last=points->back();
        if(dialogOpen.load()){
            consumedContacts.insert(first.id);
            if(last.phase==3||last.phase==4)consumedContacts.erase(first.id);
            return 0;
        }
        auto result=capy_chrome(host,first.phase==1?2:first.phase==4?3:1,
            first.phase==1?first.x:last.x,first.phase==1?first.y:last.y,true,menuOpen.load(),first.tool==3);
        if(result>=0){
            if(first.phase==1&&(result&1))consumedContacts.insert(first.id);
            bool accepted=!consumedContacts.contains(first.id);
            result=accepted?capy_pointer(host,points->data(),points->size()):0;
            if(accepted&&result==0)latencyTrace.Consume(*points);
            if(last.phase==3||last.phase==4)consumedContacts.erase(first.id);
        }
        return result;
    }
    if(auto scroll=std::get_if<CanvasScroll>(&item))
        return retiring||dialogOpen.load()?0:capy_scroll(host,scroll->x,scroll->y,scroll->dx,scroll->dy,scroll->density,scroll->zoom,scroll->horizontal);
    auto const& command=std::get<CanvasCommand>(item);
    return retiring&&command.kind==CanvasCommandKind::DeviceLoss?0:DispatchCanvasCommand(host,command);
}
void CanvasWindow::Run() {
    try {
        struct Apartment {
            Apartment(){init_apartment(apartment_type::multi_threaded);}
            ~Apartment(){uninit_apartment();}
        } apartment;
        // Device/shader preparation must not hold the UI thread. The existing resize
        // handshake performs the first SetSwapChain only after preparation finishes.
        bool prepared=capy_start_services(host,this,[](void* context) noexcept {
            auto self=static_cast<CanvasWindow*>(context);
            {std::lock_guard lock(self->mutex);self->servicesReady=true;}
            self->wake.notify_one();
        })>=0;
        if(prepared){
            if(auto snapshot=capy_snapshot(host)){
                std::unique_ptr<char,decltype(&capy_string_free)> owned(snapshot,capy_string_free);
                auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(snapshot));
                Publish(snapshot,model);
            }
            prepared=capy_prepare_gpu(host)>=0;
        }
        if(!prepared) Fail(capy_error());
        else {std::lock_guard lock(mutex);resize=true;}
        bool dirty=true;
        bool captured=false;
        bool inputStarted=false;
        bool brushReady=false;
        bool probe=GetEnvironmentVariableW(L"CAPY_PRESENT_PROBE",nullptr,0)!=0;
        bool probeReady=false;
        unsigned recoveryAttempts=0;
        std::optional<std::chrono::steady_clock::time_point> lastPresent;
        for(;prepared;) {
            if(capy_device_lost(host)){
                try {
                    if(++recoveryAttempts>3)throw std::runtime_error("GPU recovery failed repeatedly");
                    if(!RecoverGpu())break;
                    dirty=true;probeReady=false;brushReady=false;
                } catch(std::exception const& error) {
                    SaveAfterGpuFailure(error.what());break;
                }
            }
            bool pollServices=false;
            {
                std::unique_lock lock(mutex);
                if(!wake.wait_for(lock,std::chrono::milliseconds(250),[&]{return closing||resize||dirty||servicesReady||transportFailed||!work.Empty()||pendingHover.has_value()||!previewWork.Empty();}))
                    servicesReady=true;
                if(closing) break;
                if(resize) {
                    paused=true;resize=false;probeReady=false;
                    capy_suspend(host);
                    dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyResize();});
                    wake.wait(lock,[&]{return !paused||closing;});
                    dirty=true;continue;
                }
                pollServices=std::exchange(servicesReady,false);
            }
            if(pollServices){
                auto serviceResult=capy_poll_services(host);
                if(serviceResult<0){if(capy_device_lost(host))continue;Fail(capy_error());break;}
                dirty|=serviceResult!=0;
                if(auto snapshot=capy_snapshot(host)){
                    std::unique_ptr<char,decltype(&capy_string_free)> owned(snapshot,capy_string_free);
                    auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(snapshot));
                    Publish(snapshot,model);
                }
            }
            // An idle service deadline can save/renew without submitting a frame.
            {std::lock_guard lock(mutex);if(!dirty&&!transportFailed&&work.Empty()&&!pendingHover&&previewWork.Empty())continue;}
            if(lastPresent){
                std::unique_lock lock(mutex);
                auto input=[&]{return closing||resize||transportFailed||!work.Empty()||pendingHover.has_value()||!previewWork.Empty();};
                if(!input())wake.wait_until(lock,*lastPresent+RefreshPeriod(),input);
                if(closing)break;
                if(resize)continue;
            }
            // DXGI waits before draining input so a frame uses the freshest arrived samples.
            auto acquireStart=latencyTrace.enabled?Now():0;
            auto acquired=capy_acquire(host);
            auto acquiredAt=latencyTrace.enabled?Now():0;
            if(acquired<0) {if(capy_device_lost(host))continue;Fail(capy_error());break;}
            if(acquired==2) {std::lock_guard lock(mutex);resize=true;continue;}
            if(acquired==0) {
                std::unique_lock lock(mutex);
                wake.wait_for(lock,std::chrono::milliseconds(16),[&]{return closing||resize;});
                continue;
            }
            std::deque<CanvasWork> pending;
            bool overflow;std::optional<Hover> hover;
            // Samples admitted during reconstruction must wait for replacement
            // brushes; draining sooner would classify their presses as unready.
            {std::lock_guard lock(mutex);if(!recoveryAttempts)pending=work.Take();overflow=transportFailed;hover=std::exchange(pendingHover,std::nullopt);}
            space.notify_all();
            bool failed=false;
            if(hover&&capy_chrome(host,hover->leave?3:1,hover->x,hover->y,false,menuOpen.load(),hover->touch)<0){Fail(capy_error());break;}
            for(auto& item:pending) {
                auto result=DispatchWork(item,false);
                if(result>0)Fail(capy_error()); // A rejected UI action leaves the canvas running.
                if(result<0) {Fail(capy_error());failed=true;break;}
            }
            if(failed) break;
            if((!pending.empty()||hover)&&capy_chrome(host,0,0,0,false,menuOpen.load(),false)<0){Fail(capy_error());break;}
            if(overflow){sentModifiers.store(0);if(capy_input(host,R"({"type":"blur"})")<0){Fail(capy_error());break;}}
            if(capy_device_lost(host))continue;
            auto now=Now();
            auto result=capy_frame(host,now,now);
            if(latencyTrace.enabled){
                auto end=Now();uint64_t stats[6]{};auto error=capy_presentation_stats(host,stats);
                latencyTrace.Frame(acquireStart,acquiredAt,now,end,stats,error);
            }
            if(result<0){if(capy_device_lost(host))continue;Fail(capy_error());break;}
            dirty=result!=0;
            lastPresent=std::chrono::steady_clock::now();
            if(!inputStarted){inputStarted=true;inputDispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->StartInput();});}
            {std::lock_guard lock(mutex);revision=capy_view_revision(host);}
            if(auto snapshot=capy_snapshot(host)) {
                std::unique_ptr<char,decltype(&capy_string_free)> owned(snapshot,capy_string_free);
                auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(snapshot));
                Publish(snapshot,model);
                if(model.HasKey(L"brush_ready")) {
                    bool ready=model.GetNamedBoolean(L"brush_ready");
                    brushReady=ready;
                    if(ready)recoveryAttempts=0;
                }
                if(!captured && !dirty && GetEnvironmentVariableW(L"CAPY_TEST_DISPLAY",nullptr,0)) {
                    TraceState("canvas-state",snapshot);
                    captured=true;
                }
            }
            if((probe||latencyTrace.enabled)&&!probeReady&&result==0&&brushReady) {
                auto info=capy_surface_info(host);
                if(!info){Fail(capy_error());break;}
                std::unique_ptr<char,decltype(&capy_string_free)> owned(info,capy_string_free);
                auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(info));
                model.Insert(L"process_id",Windows::Data::Json::JsonValue::CreateNumberValue(GetCurrentProcessId()));
                model.Insert(L"ready_qpc_ns",Windows::Data::Json::JsonValue::CreateStringValue(to_hstring(std::to_string(Now()))));
                model.Insert(L"window_id",Windows::Data::Json::JsonValue::CreateNumberValue(double(windowId)));
                TraceState("presentation-probe",to_string(model.Stringify()));
                probeReady=true;
            }
            // Optional readbacks run after painting and yield to newly queued input.
            // The GPU poll never waits; CPU conversion belongs to another worker.
            std::optional<CanvasQuery> preview;
            {std::lock_guard lock(mutex);if(!closing&&work.Empty())preview=previewWork.Take();}
            if(preview){
                auto request=preview->kind==CanvasQueryKind::Filters?capy_filter_previews:
                    preview->kind==CanvasQueryKind::Thumbnails?capy_layer_thumbnails:
                    preview->kind==CanvasQueryKind::LayerMenu?capy_layer_menu:
                    preview->kind==CanvasQueryKind::Document?capy_document_preview:capy_workspace_query;
                PreviewPacket packet(request(host,preview->json.c_str()),capy_preview_free);
                if(!packet&&!capy_device_lost(host))Fail(capy_error());
                preview->reply(std::move(packet));
            }
            // Opt-in baseline only: present unchanged content at DXGI cadence.
            // No timer, per-frame disk I/O, synthetic input or display-time claim.
            if(probe&&probeReady)dirty=true;
            if(overflow)break;
        }
    } catch(hresult_error const& error) {Fail(to_string(error.message()));}
      catch(std::exception const& error) {Fail(error.what());}
      catch(...) {Fail("Unexpected render worker failure");}
    CapyLifecycle("render_loop_stopped");
    // UI close commits drafts before rejecting new work. Drain accepted commands
    // so a final preferences edit reaches storage even when no next frame runs.
    std::deque<CanvasWork> finalWork;
    {std::lock_guard lock(mutex);finalWork=work.Take();previewWork.Clear();}
    space.notify_all();
    for(auto& item:finalWork)if(auto command=std::get_if<CanvasCommand>(&item)){
        auto result=DispatchCanvasCommand(host,*command);
        if(result!=0)Fail(capy_error());
        if(result<0)break;
    }
    capy_suspend(host);
    CapyLifecycle("services_finishing");
    if(capy_finish_services(host)<0)Fail(capy_error());
    CapyLifecycle("services_finished");
    rendererDone.store(true);
    CapyLifecycle("renderer_done");
    space.notify_all();
    dispatcher.TryEnqueue([weak=weak_from_this()] {
        if(auto self=weak.lock()) { if(self->closing) self->Finish(); }
    });
}
bool CanvasWindow::RecoverGpu() {
    CapyLifecycle("gpu_recovery_started");
    if(capy_suspend(host)<0)throw std::runtime_error(capy_error());
    {
        std::lock_guard lock(mutex);
        if(closing)return false;
        paused=true;
    }
    if(!dispatcher.TryEnqueue([weak=weak_from_this()]{
        if(auto self=weak.lock())self->ResetSurface();
    }))throw std::runtime_error("GPU recovery could not reach the window dispatcher");
    {
        std::unique_lock lock(mutex);
        wake.wait(lock,[&]{return !paused||closing;});
        if(closing)return false;
    }
    {std::lock_guard lock(mutex);if(!surfaceError.empty())throw std::runtime_error(surfaceError);}
    CapyLifecycle("gpu_recovery_preparing");
    // Retired shader/document jobs and other windows can still own the removed
    // D3D12 singleton briefly. Yield between bounded attempts; close wakes us.
    auto deadline=std::chrono::steady_clock::now()+std::chrono::seconds(5);
    while(capy_prepare_gpu(host)<0){
        if(std::chrono::steady_clock::now()>=deadline)throw std::runtime_error(capy_error());
        std::unique_lock lock(mutex);
        if(wake.wait_for(lock,std::chrono::milliseconds(250),[&]{return closing;}))return false;
    }
    // The normal resize handshake attaches the new swap chain on XAML's thread.
    {std::lock_guard lock(mutex);resize=true;}
    CapyLifecycle("gpu_recovery_prepared");
    return true;
}
void CanvasWindow::SaveAfterGpuFailure(std::string const& reason) {
    std::deque<CanvasWork> admitted;
    {std::lock_guard lock(mutex);inputStopped=true;admitted=work.Take();pendingHover.reset();previewWork.Clear();}
    space.notify_all();
    capy_suspend(host);
    if(capy_suspend_renderer(host)<0)throw std::runtime_error(capy_error());
    // Raster recovery retains completed edits; queued contacts cannot be drawn
    // without a renderer. Drain accepted commands after closing canvas input.
    for(auto& item:admitted){
        auto result=DispatchWork(item,true);
        if(result<0)throw std::runtime_error(capy_error());
        if(result>0)Fail(capy_error());
    }
    OutputDebugStringA(reason.c_str());
    Fail("Painting is unavailable. The interrupted stroke was canceled.\nUse File > Save or Save As to keep completed edits, then reopen the drawing.");
    CapyLifecycle("gpu_recovery_save_available");
    for(;;){
        std::deque<CanvasWork> pending;std::optional<CanvasQuery> query;
        {
            std::unique_lock lock(mutex);
            wake.wait_for(lock,std::chrono::milliseconds(250),[&]{return closing||resize||servicesReady||!work.Empty()||!previewWork.Empty();});
            if(closing)return;
            servicesReady=false;
            if(resize){
                paused=true;resize=false;
                if(!dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyResize();}))
                    throw std::runtime_error("Could not resize the suspended canvas");
                wake.wait(lock,[&]{return !paused||closing;});
                if(closing)return;
            }
            pending=work.Take();query=previewWork.Take();
        }
        space.notify_all();
        for(auto& item:pending)if(auto command=std::get_if<CanvasCommand>(&item)){
            auto result=DispatchCanvasCommand(host,*command);
            if(result!=0)Fail(capy_error());
        }
        if(capy_poll_services(host)<0)throw std::runtime_error(capy_error());
        if(auto snapshot=capy_snapshot(host)){
            std::unique_ptr<char,decltype(&capy_string_free)> owned(snapshot,capy_string_free);
            Publish(snapshot,Windows::Data::Json::JsonObject::Parse(to_hstring(snapshot)));
        }
        if(query){
            PreviewPacket packet(query->kind==CanvasQueryKind::Workspace?capy_workspace_query(host,query->json.c_str()):nullptr,capy_preview_free);
            query->reply(std::move(packet));
        }
    }
}
void CanvasWindow::ResetSurface() {
    {std::lock_guard lock(mutex);if(closing)return;}
    auto native=panel.as<ISwapChainPanelNative>();
    auto detached=native->SetSwapChain(nullptr);
    if(FAILED(detached)||capy_reset_surface(host,native.get())<0){
        std::lock_guard lock(mutex);
        surfaceError=FAILED(detached)?"Could not detach the lost GPU surface":capy_error();
    }
    {std::lock_guard lock(mutex);paused=false;}
    wake.notify_one();
}

void CanvasWindow::ApplyResize() {
    Size next;
    {std::lock_guard lock(mutex);if(closing)return;next=desired;resize=false;}
    int result=capy_resize(host,next.width,next.height,next.scale);
    if(result<0&&!capy_device_lost(host)) {status.Text(to_hstring(capy_error()));Stop();return;}
    {std::lock_guard lock(mutex);revision=capy_view_revision(host);inputScale=next.scale;paused=false;}
    wake.notify_one();
}
void CanvasWindow::Fail(std::string message) {
    dispatcher.TryEnqueue([weak=weak_from_this(),message=std::move(message)] {
        if(auto self=weak.lock();self&&!self->closed&&self->status){self->statusFailed=true;self->status.Text(to_hstring(message));self->status.Visibility(Visibility::Visible);}
    });
}
void CanvasWindow::RequestClose() {
    {std::lock_guard lock(mutex);if(closing||closed)return;}
    if(!host||!renderer.joinable()||rendererDone.load()){Stop();return;}
    if((documents&&documents->IsOpen())||(workspaceStorage&&workspaceStorage->IsOpen()))return;
    if(workspace)workspace->CancelGesture();
    // Commit the focused workspace draft before shared close policy checks dirty state.
    canvasFocus.Focus(FocusState::Programmatic);
    if(settings)settings->CommitEdits();
    Send(R"({"type":"close_settings"})");
    if(workspaceDialogs)workspaceDialogs->CancelAll();
    if(selectionDialog)selectionDialog->CancelAll();
    if(workspaceManager)workspaceManager->CancelAll();
    CapyLifecycle("close_requested");
    Send(R"({"operation":"close"})",CanvasCommandKind::Document);
}
void CanvasWindow::Stop() {
    {std::lock_guard lock(mutex);if(closing)return;}
    if(settings)settings->CommitEdits();
    {std::lock_guard lock(mutex);closing=true;}
    CapyLifecycle("close_authorized");
    if(settings)settings->Hide();
    if(documents)documents->Hide();
    if(workspaceDialogs)workspaceDialogs->Hide();
    if(selectionDialog)selectionDialog->Hide();
    if(workspaceStorage)workspaceStorage->Hide();
    if(workspaceManager)workspaceManager->Hide();
    wake.notify_all();space.notify_all();
    if(inputController) {
        inputDispatcher.TryEnqueue([weak=weak_from_this()]{
            if(auto self=weak.lock()) {
                if(self->pointerPredictor){self->pointerPredictor.Close();self->pointerPredictor=nullptr;}
                self->inputSource=nullptr;
            }
        });
        inputController.ShutdownQueueAsync().Completed([weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock())self->dispatcher.TryEnqueue([weak]{
                if(auto self=weak.lock()){CapyLifecycle("input_dispatcher_done");self->inputDone=true;self->Finish();}
            });
        });
    } else inputDone=true;
    if(!renderer.joinable()||rendererDone.load()) Finish();
}
void CanvasWindow::Finish() {
    if(closed||finishing||!inputDone||(renderer.joinable()&&!rendererDone.load()))return;
    if((settings&&settings->IsOpen())||(documents&&documents->IsOpen())||(workspaceDialogs&&workspaceDialogs->IsOpen())||(selectionDialog&&selectionDialog->IsOpen())||(workspaceStorage&&workspaceStorage->IsOpen())||(workspaceManager&&workspaceManager->IsOpen()))return;
    auto lifetime=shared_from_this(); // App may release its last reference below.
    finishing=true;
    CapyLifecycle("join_renderer");
    if(renderer.joinable()) renderer.join();
    CapyLifecycle("detach_surface");
    // The worker no longer owns any acquired image; detach on the XAML thread.
    panel.as<ISwapChainPanelNative>()->SetSwapChain(nullptr);
    CapyLifecycle("destroy_host");
    capy_destroy(host);host=nullptr;
    CapyLifecycle("host_destroyed");
    // XAML controls and their retained bindings must be released while this
    // window still owns a live XAML context, not later from App destruction.
    settings.reset();documents.reset();workspaceDialogs.reset();selectionDialog.reset();workspaceStorage.reset();workspaceManager.reset();header.reset();workspace.reset();
    root.Children().Clear();toolbar.Children().Clear();canvasFocus.Content(nullptr);
    window.Content(nullptr);
    canvasFocus=nullptr;panel=nullptr;status=nullptr;toolbar=nullptr;root=nullptr;
    inputController=nullptr;inputDispatcher=nullptr;
    if(!workspaceOwnerProperty.empty()){RemovePropW(workspaceOwnerWindow,workspaceOwnerProperty.c_str());workspaceOwnerProperty.clear();}
    CapyLifecycle("views_released");
    closed=true;window.Close();
    CapyLifecycle("window_closed");
    if(onClosed)onClosed(windowId);
}

void CanvasWindow::Publish(std::string snapshot,Windows::Data::Json::JsonObject const& model) {
    bool full=model.HasKey(L"state");
    std::optional<std::string> camera;
    if(!full&&model.HasKey(L"camera"))camera=to_string(CapyUi::O({{L"camera",model.GetNamedValue(L"camera")}}).Stringify());
    // Explicit local test evidence. This can include user state and is never
    // enabled by ordinary or presentation-probe launches.
    if(GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)){
        auto identity="{\"process_id\":"+std::to_string(GetCurrentProcessId())+
            ",\"window_id\":"+std::to_string(windowId);
        if(full)TraceState("ui-state",identity+",\"model\":"+snapshot+"}");
        auto view=full?CapyUi::object(CapyUi::object(model,L"state"),L"camera"):CapyUi::object(model,L"camera");
        if(view.Size())TraceState("camera-state",identity+",\"camera\":"+to_string(view.Stringify())+"}");
    }
    if(full){
        bool pan=model.GetNamedBoolean(L"pan_cursor",false);
        if(panCursor.exchange(pan)!=pan&&inputDispatcher)inputDispatcher.TryEnqueue([weak=weak_from_this(),pan]{
            using namespace Microsoft::UI::Input;
            if(auto self=weak.lock();self&&self->inputSource)self->inputSource.Cursor(pan?InputCursor(InputSystemCursor::Create(InputSystemCursorShape::SizeAll)):InputCursor(nullptr));
        });
    }
    bool post;
    {
        std::lock_guard lock(mutex);if(closing)return;
        snapshots.Push(std::move(snapshot),full,model.HasKey(L"workspace_update"),std::move(camera),model.HasKey(L"command_search"));
        post=!snapshotPosted;snapshotPosted=true;
    }
    if(post)dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyPending();});
}
void CanvasWindow::ApplyPending() {
    CanvasSnapshotMailbox::Batch batch;
    {
        std::lock_guard lock(mutex);snapshotPosted=false;
        if(closing)return;
        batch=snapshots.Take();
    }
    try {
        if(!batch.full.empty())ApplyModel(Windows::Data::Json::JsonObject::Parse(to_hstring(batch.full)));
        if(!closing&&!batch.workspace.empty())workspace->Apply(Windows::Data::Json::JsonObject::Parse(to_hstring(batch.workspace)));
        if(!closing&&!batch.camera.empty())workspace->Apply(Windows::Data::Json::JsonObject::Parse(to_hstring(batch.camera)));
        if(!closing&&!batch.search.empty())workspace->Apply(Windows::Data::Json::JsonObject::Parse(to_hstring(batch.search)));
    } catch(hresult_error const& error) {Fail(to_string(error.message()));}
}

void CanvasWindow::ApplyDialogs() {
    if(closing){
        // Let the coroutine release its final dialog reference before tearing
        // down XAML. Completion callbacks may run before ShowAsync unwinds.
        dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->Finish();});
        return;
    }
    if(applyingDialogs||!settings||!documents||!workspaceDialogs||!selectionDialog||!workspaceStorage||!workspaceManager||!lastModel.Size())return;
    applyingDialogs=true;
    struct Reset{bool& flag;~Reset(){flag=false;}}reset{applyingDialogs};
    if(!documents->IsOpen()&&!workspaceDialogs->IsOpen()&&!selectionDialog->IsOpen()&&!workspaceStorage->IsOpen()&&!workspaceManager->IsOpen())settings->Apply(lastModel);
    workspaceDialogs->Apply(lastModel,documents->IsOpen()||settings->IsOpen()||selectionDialog->IsOpen()||workspaceStorage->IsOpen()||workspaceManager->IsOpen());
    selectionDialog->Apply(lastModel,documents->IsOpen()||settings->IsOpen()||workspaceDialogs->IsOpen()||workspaceStorage->IsOpen()||workspaceManager->IsOpen());
    documents->Apply(lastModel,settings->IsOpen()||workspaceDialogs->IsOpen()||selectionDialog->IsOpen()||workspaceStorage->IsOpen()||workspaceManager->IsOpen());
    workspaceStorage->Apply(lastModel,documents->IsOpen()||settings->IsOpen()||workspaceDialogs->IsOpen()||selectionDialog->IsOpen()||workspaceManager->IsOpen());
    workspaceManager->Apply(lastModel,documents->IsOpen()||settings->IsOpen()||workspaceDialogs->IsOpen()||selectionDialog->IsOpen()||workspaceStorage->IsOpen());
    UpdatePopup();
}
void CanvasWindow::Popup(bool open) {
    headerPopupOpen=open;UpdatePopup();
}
void CanvasWindow::UpdatePopup() {
    if(closing||closed)return;
    auto storage=CapyUi::object(lastModel,L"windows_workspace");
    bool unavailable=storage.Size()&&(!CapyUi::flag(storage,L"ready")||CapyUi::flag(storage,L"busy")||CapyUi::flag(storage,L"owner_lost")||CapyUi::flag(storage,L"close_requested"));
    unavailable|=CapyUi::flag(CapyUi::object(lastModel,L"windows_settings_close"),L"requested");
    bool blocked=unavailable||(settings&&settings->IsOpen())||(documents&&documents->IsOpen())||(workspaceDialogs&&workspaceDialogs->IsOpen())||(selectionDialog&&selectionDialog->IsOpen())||(workspaceStorage&&workspaceStorage->IsOpen())||(workspaceManager&&workspaceManager->IsOpen());
    canvasFocus.IsEnabled(!blocked);
    if(workspace)workspace->Root().IsHitTestVisible(!unavailable);
    if(header){header->Root().IsHitTestVisible(!unavailable);header->SetBlocked(blocked);}
    if(dialogOpen.exchange(blocked)!=blocked&&blocked){
        heldKeys.clear();sentModifiers.store(0);Send(R"({"type":"blur"})",CanvasCommandKind::Input);
    }
    bool open=headerPopupOpen||workspacePopupOpen||blocked;
    auto facts=workspace?workspace->ChromeFacts(headerPopupOpen||blocked):CapyUi::O({{L"held",CapyUi::B(false)},{L"dragging",CapyUi::B(false)}});
    if(menuOpen.exchange(open)==open)return;
    using namespace CapyUi;
    A viewport;viewport.Append(N(panel.ActualWidth()));viewport.Append(N(panel.ActualHeight()));
    facts.Insert(L"popup_open",B(open));
    Send(to_string(O({{L"type",S(L"chrome")},{L"event",O({{L"kind",S(L"refresh")}})},
        {L"facts",facts},
        {L"viewport",viewport}}).Stringify()),CanvasCommandKind::Input);
}
void CanvasWindow::ChromeMotion(PointerRoutedEventArgs const& e,bool leave) {
    if(closing||closed)return;
    auto point=e.GetCurrentPoint(root);auto position=point.Position();
    if(leave&&position.X>=0&&position.Y>=0&&position.X<root.ActualWidth()&&position.Y<root.ActualHeight())return;
    {
        std::lock_guard lock(mutex);
        if(closing||!host||rendererDone.load())return;
        pendingHover=Hover{position.X*inputScale,position.Y*inputScale,leave,
            point.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch};
    }
    wake.notify_one();
}
void CanvasWindow::Fullscreen() {
    if(closing||closed)return;
    auto app=window.AppWindow();
    app.SetPresenter(app.Presenter().Kind()==AppWindowPresenterKind::FullScreen?
        AppWindowPresenterKind::Overlapped:AppWindowPresenterKind::FullScreen);
    Resize();
}
void CanvasWindow::OpenFiles(std::vector<std::wstring> paths) {
    launchFiles.insert(launchFiles.end(),std::make_move_iterator(paths.begin()),std::make_move_iterator(paths.end()));
    if(lastModel.Size())SendLaunchFiles(lastModel);
}
void CanvasWindow::Present() {
    if(Closing())return;
    if(IsIconic(Handle()))ShowWindow(Handle(),SW_RESTORE);
    window.Activate();
}
void CanvasWindow::SendLaunchFiles(Windows::Data::Json::JsonObject const& model) {
    using namespace CapyUi;
    if(launchFiles.empty()||!flag(model,L"canvas_ready"))return;
    A paths;for(auto const& path:launchFiles)paths.Append(S(hstring(path)));launchFiles.clear();
    Send(to_string(O({{L"operation",S(L"open_paths")},{L"paths",paths}}).Stringify()),CanvasCommandKind::Document);
}
void CanvasWindow::RefreshWorkspaceSwitcher() {
    if(!closing&&!closed)Send(R"({"operation":"refresh_switcher"})",CanvasCommandKind::Workspace);
}
void CanvasWindow::ApplyModel(Windows::Data::Json::JsonObject const& model) {
    using namespace CapyUi;
    auto focus=settings&&settings->IsOpen()?FocusManager::GetFocusedElement(root.XamlRoot()).try_as<Control>():nullptr;
    if(focus)if(auto owner=ItemsControl::ItemsControlFromItemContainer(focus))focus=owner;
    if(!workspace->Apply(model))return;
    lastModel=model;
    auto suspended=flag(model,L"windows_rendering_suspended");
    workspace->Root().Visibility(suspended?Visibility::Collapsed:Visibility::Visible);
    panel.Visibility(suspended?Visibility::Collapsed:Visibility::Visible);
    auto state=object(model,L"state");auto theme=str(state,L"theme",L"dark");
    if(suspended)root.Background(SolidColorBrush(color(str(object(state,L"palette"),L"bg",L"#323232"))));
    else root.Background(nullptr);
    status.VerticalAlignment(suspended?VerticalAlignment::Center:VerticalAlignment::Bottom);
    status.TextAlignment(suspended?TextAlignment::Center:TextAlignment::Left);
    status.TextWrapping(suspended?TextWrapping::Wrap:TextWrapping::NoWrap);
    status.Margin(suspended?Thickness{24,24,24,24}:Thickness{0,0,0,40});
    SendLaunchFiles(model);
    auto storage=object(model,L"windows_workspace");
    if(flag(storage,L"ready")){
        auto next=uint64_t(num(storage,L"switcher_revision"));
        bool changed=workspacePreferencesRevision&&*workspacePreferencesRevision!=next;
        workspacePreferencesRevision=next;
        if(changed&&workspacePreferencesChanged)workspacePreferencesChanged(windowId);
    }
    auto owner=str(storage,L"owner");
    if(!owner.empty()){
        std::wstring property=L"CapyCanvas.WorkspaceOwner."+std::wstring(owner);
        if(property!=workspaceOwnerProperty){
            if(!workspaceOwnerWindow)check_hresult(window.as<IWindowNative>()->get_WindowHandle(&workspaceOwnerWindow));
            if(!workspaceOwnerProperty.empty())RemovePropW(workspaceOwnerWindow,workspaceOwnerProperty.c_str());
            if(!SetPropW(workspaceOwnerWindow,property.c_str(),reinterpret_cast<HANDLE>(1)))throw hresult_error(HRESULT_FROM_WIN32(GetLastError()));
            workspaceOwnerProperty=std::move(property);
        }
    }
    auto preferencesClose=object(model,L"windows_settings_close");
    if(flag(object(model,L"windows_tabs"),L"window_ready")&&flag(object(state,L"document_file"),L"close_ready")&&(!storage.Size()||flag(storage,L"close_ready"))
        &&(!preferencesClose.Size()||flag(preferencesClose,L"ready"))
        &&(!object(model,L"windows_recovery").Size()||flag(object(model,L"windows_recovery"),L"ready"))){Stop();return;}
    if(!statusFailed){
        auto message=str(model,L"error");
        if(message.empty())message=str(state,L"host_error");
        if(message.empty())message=str(object(model,L"windows_recovery"),L"error");
        if(message.empty()&&str(object(model,L"windows_document"),L"type")==L"workflow_busy")message=L"Preparing document…";
        if(message.empty())message=str(object(model,L"windows_filter_load"),L"error");
        if(message.empty()&&!flag(model,L"brush_ready"))message=L"Preparing brushes…";
        if(message.empty()&&flag(object(model,L"windows_filter_load"),L"pending"))message=L"Loading filters…";
        if(message.empty()&&flag(model,L"windows_importing")&&!flag(object(model,L"windows_image_import"),L"picking"))
            message=L"Importing image…";
        if(message.empty())message=str(object(model,L"windows_proof"),L"text");
        status.Text(message);status.Visibility(message.empty()?Visibility::Collapsed:Visibility::Visible);
    }
    auto nextTheme=theme==L"dark"?ElementTheme::Dark:ElementTheme::Light;
    bool retheme=root.RequestedTheme()!=nextTheme;root.RequestedTheme(nextTheme);
    // Artwork also reaches the caption area; keep the OS buttons readable on it.
    auto captionBackground=color(str(object(state,L"palette"),L"bg",L"#333333"));
    window.AppWindow().TitleBar().ButtonBackgroundColor(captionBackground);
    window.AppWindow().TitleBar().ButtonInactiveBackgroundColor(captionBackground);
    auto foreground=color(str(object(state,L"palette"),L"text",L"#fafafb"));
    window.AppWindow().TitleBar().ButtonForegroundColor(foreground);
    foreground.A=128;window.AppWindow().TitleBar().ButtonInactiveForegroundColor(foreground);
    auto tabs=array(state,L"tabs");
    if(tabs.Size())window.Title((flag(object(state,L"document_file"),L"modified")?hstring(L"• "):hstring())+str(tabs.GetObjectAt(0),L"title")+L" · Capy Canvas");
    header->Apply(model);ApplyDialogs();
    // Workspace theme replacement must not take focus from retained Preferences.
    if(retheme&&focus)dispatcher.TryEnqueue(Microsoft::UI::Dispatching::DispatcherQueuePriority::Low,
        [weak=weak_from_this(),target=make_weak(focus)]{
            if(auto self=weak.lock();self&&!self->closing&&self->settings->IsOpen())
                if(auto control=target.get();control&&control.IsLoaded())control.Focus(FocusState::Programmatic);
        });
}
