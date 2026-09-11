#include "pch.h"
#include "CanvasWindow.h"
#include "UiControls.h"
#include <microsoft.ui.xaml.media.dxinterop.h>
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
        case CanvasCommandKind::Document:return capy_document_action(host,json);
        case CanvasCommandKind::Overviews:return capy_overviews(host,json);
        case CanvasCommandKind::Action:return capy_action(host,json);
    }
    return -1;
}
static uint64_t Now() {
    LARGE_INTEGER ticks, frequency;
    QueryPerformanceCounter(&ticks); QueryPerformanceFrequency(&frequency);
    return uint64_t(ticks.QuadPart / frequency.QuadPart) * 1000000000ULL
        + uint64_t(ticks.QuadPart % frequency.QuadPart) * 1000000000ULL / uint64_t(frequency.QuadPart);
}
void CapyLifecycle(char const* event) {
    if(!GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0))return;
    static std::mutex logMutex;
    std::lock_guard lock(logMutex);
    std::ofstream("lifecycle.log",std::ios::app)
        << GetCurrentProcessId() << " " << Now() << " " << event << "\n";
}
CanvasWindow::~CanvasWindow() {
    { std::lock_guard lock(mutex); closing=true; paused=false; }
    wake.notify_all();space.notify_all();
    if (renderer.joinable()) renderer.join();
    if (host) capy_destroy(host);
}
void CanvasWindow::Open() {
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
        if(auto self=weak.lock())if(!self->dialogOpen.load())self->Key(e,true);
    });
    root.PreviewKeyUp([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
        if(auto self=weak.lock())if(!self->dialogOpen.load())self->Key(e,false);
    });
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
        for(int mode=0;mode<3;++mode){
            Button test;test.Content(box_value(mode==0?L"Test stroke":mode==1?L"Test pan":L"Test backlog"));
            test.Click([weak=weak_from_this(),mode](auto&&,auto&&){if(auto self=weak.lock())self->Replay(mode==1,mode==2);});
            toolbar.Children().Append(test);
        }
    }
    root.Children().Append(toolbar);
    status.Text(L"Preparing canvas…");
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
        if(e.WindowActivationState()==WindowActivationState::Deactivated)
            if(auto self=weak.lock()){self->heldKeys.clear();self->Send(R"({"type":"blur"})",CanvasCommandKind::Input);}
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
}
void CanvasWindow::Resize() {
    if(closing||closed)return;
    float scale=panel.CompositionScaleX();
    Size next{uint32_t(std::max(1L, std::lround(panel.ActualWidth()*scale))),
              uint32_t(std::max(1L, std::lround(panel.ActualHeight()*scale))),scale};
    // Physical-pixel drag regions leave the app controls and system caption buttons interactive.
    auto titlebar=window.AppWindow().TitleBar();
    bool caption=window.AppWindow().Presenter().Kind()==AppWindowPresenterKind::Overlapped;
    if(header){
        header->SetFullscreen(!caption);
        header->SetInsets(caption?float(titlebar.LeftInset())/scale:0,caption?float(titlebar.RightInset())/scale:0);
        if(caption)titlebar.SetDragRectangles(header->DragRegions(scale,next.width));
    } else if(caption)titlebar.SetDragRectangles({Windows::Graphics::RectInt32{
        titlebar.LeftInset(),0,std::max(0,int32_t(next.width)-titlebar.LeftInset()-titlebar.RightInset()),int32_t(48*scale)}});
    {
        std::lock_guard lock(mutex);
        bool changed=next.width!=desired.width||next.height!=desired.height||next.scale!=desired.scale;
        desired=next;if(host&&!closing&&changed)resize=true;
    }
    wake.notify_one();
}
void CanvasWindow::Start() {
    if(host||closing) return;
    Resize();
    auto native=panel.as<ISwapChainPanelNative>();
    host=capy_create(native.get(),desired.width,desired.height,desired.scale);
    if(!host) {status.Text(to_hstring(capy_error()));return;}
    auto dark=panel.ActualTheme()==ElementTheme::Dark;
    capy_action(host,dark?R"({"type":"system_theme_changed","theme":"dark"})":R"({"type":"system_theme_changed","theme":"light"})");
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
    },[weak=weak_from_this()](bool open){if(auto self=weak.lock()){self->workspacePopupOpen=open;self->UpdatePopup();}});
    root.Children().InsertAt(1,workspace->Root());
    auto send=[weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json));};
    auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(catalog));
    header=std::make_unique<HeaderView>(send,model,
        [weak=weak_from_this()](bool open){if(auto self=weak.lock())self->Popup(open);},
        [weak=weak_from_this()]{if(auto self=weak.lock())self->Resize();},
        [weak=weak_from_this()]{if(auto self=weak.lock())self->Fullscreen();});
    root.Children().Append(header->Root());
    settings=std::make_unique<SettingsView>(send,model,root.XamlRoot(),
        [weak=weak_from_this()](KeyRoutedEventArgs const& e,bool pressed){if(auto self=weak.lock())self->Key(e,pressed);},
        [weak=weak_from_this()](std::string error){if(auto self=weak.lock())self->Fail("Cannot open Preferences: "+error);},
        [weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();});
    documents=std::make_unique<DocumentView>(
        [weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Document);},
        model,window,[weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();},
        [weak=weak_from_this()](std::string error){if(auto self=weak.lock())self->Fail(std::move(error));});
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
        if(closing||!host||rendererDone.load()||transportFailed)return false;
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
    space.wait(lock,[&]{return closing||rendererDone.load()||transportFailed||work.CanPush(item);});
    if(closing||rendererDone.load()||transportFailed)return false;
    work.Push(std::move(item));
    lock.unlock();wake.notify_one();
    return true;
}
void CanvasWindow::Replay(bool pan,bool backlog) {
    inputDispatcher.TryEnqueue([weak=weak_from_this(),pan,backlog] {
        auto self=weak.lock();if(!self)return;
        Size size;uint64_t view;
        {std::lock_guard lock(self->mutex);if(self->closing)return;size=self->desired;view=self->revision;}
        std::vector<CapyPointer> records;records.reserve(CanvasWorkBuffer::PointerBatch);
        uint32_t count=backlog?32768:pan?3:42;
        uint64_t now=Now();
        for(uint32_t i=0;i<count;i++) {
            CapyPointer p{};
            p.id=77;p.sequence=++self->sequence;p.timestamp_ns=now-(count-i)*1000000ULL;
            p.view_revision=view;p.tool=1;p.button=pan?1:0;p.flags=2;p.pressure=0.5f;
            p.phase=i==0?1:i==count-1?3:2;
            p.x=size.width*0.4f+(pan?0.0f:float(backlog?i%42:i)*5);
            p.y=pan?(i==0?size.height*0.5f:28.0f):size.height*0.5f+24.0f*std::sin(float(i)/6);
            records.push_back(p);
            if(records.size()==CanvasWorkBuffer::PointerBatch){
                if(!self->SendIndependent(std::move(records)))return;
                records={};records.reserve(CanvasWorkBuffer::PointerBatch);
            }
        }
        if(!records.empty())self->SendIndependent(std::move(records));
    });
}

void CanvasWindow::StartInput() {
    try {
        {std::lock_guard lock(mutex);if(closing)return;}
        using namespace Microsoft::UI::Input;
        inputSource=panel.CreateCoreIndependentInputSource(
            InputPointerSourceDeviceKinds::Mouse|InputPointerSourceDeviceKinds::Pen|InputPointerSourceDeviceKinds::Touch);
        inputSource.PointerPressed([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock())self->Pointer(e,1);
        });
        inputSource.PointerMoved([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock())self->Pointer(e,e.CurrentPoint().IsInContact()?2:0);
        });
        inputSource.PointerReleased([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock())self->Pointer(e,3);
        });
        inputSource.PointerCaptureLost([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock())self->Pointer(e,4);
        });
        inputSource.PointerRoutedAway([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock())self->Pointer(e,4);
        });
        inputSource.PointerRoutedReleased([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock())self->Pointer(e,4);
        });
        inputSource.PointerWheelChanged([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock())self->Wheel(e);
        });
    } catch(hresult_error const& error) {Fail(to_string(error.message()));}
}

void CanvasWindow::Pointer(Microsoft::UI::Input::PointerEventArgs const& e, uint32_t phase) {
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
    for(uint32_t i=count;i>0;--i) {
        auto point=(phase==0||phase==2)?points.GetAt(i-1):e.CurrentPoint(); auto props=point.Properties();
        auto type=point.PointerDeviceType();
        uint32_t tool=type==Microsoft::UI::Input::PointerDeviceType::Mouse?1:
            type==Microsoft::UI::Input::PointerDeviceType::Touch?3:props.IsEraser()?2:0;
        auto pos=point.Position();
        auto radians=[](float degrees){return degrees*0.017453292519943295f;};
        CapyPointer p{};
        p.id=point.PointerId();p.timestamp_ns=point.Timestamp()*1000;
        p.sequence=++sequence;p.view_revision=view;p.x=pos.X*scale;p.y=pos.Y*scale;
        p.pressure=tool==1?(point.IsInContact()?0.5f:0.0f):props.Pressure();
        p.tilt_x=radians(props.XTilt());p.tilt_y=radians(props.YTilt());p.twist=radians(props.Twist());
        p.phase=phase;p.tool=tool;
        p.button=props.IsMiddleButtonPressed()?1:props.IsRightButtonPressed()?2:0;
        p.flags=(props.IsPrimary()?2:0)|(props.IsBarrelButtonPressed()?4:0)|(props.IsInverted()?8:0);
        samples.push_back(p);
        if(GetEnvironmentVariableW(L"CAPY_TRACE_INPUT",nullptr,0)) std::ofstream("pointer-input.log",std::ios::app) << p.phase << " " << p.x << " " << p.y << " " << p.timestamp_ns << std::endl;
        if(samples.size()==CanvasWorkBuffer::PointerBatch) {
            if(!SendIndependent(std::move(samples)))return;
            samples={};samples.reserve(CanvasWorkBuffer::PointerBatch);
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
    auto focused=FocusManager::GetFocusedElement(root.XamlRoot());
    bool canvas=focused&&focused==canvasFocus;
    // Native controls retain text, slider and focus-navigation keys. Releases
    // still reach shared state so moving focus cannot leave a pan key held.
    bool editing=!canvas;
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
    Send(to_string(input.Stringify()),CanvasCommandKind::Input);
    if(!editing)e.Handled(true);
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
                Publish(snapshot,model.HasKey(L"state"));
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
        for(;prepared;) {
            bool pollServices=false;
            {
                std::unique_lock lock(mutex);
                wake.wait(lock,[&]{return closing||resize||dirty||servicesReady||transportFailed||!work.Empty()||pendingHover.has_value()||!previewWork.Empty();});
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
                if(capy_poll_services(host)<0){Fail(capy_error());break;}
                if(auto snapshot=capy_snapshot(host)){
                    std::unique_ptr<char,decltype(&capy_string_free)> owned(snapshot,capy_string_free);
                    auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(snapshot));
                    Publish(snapshot,model.HasKey(L"state"));
                }
            }
            // DXGI waits before draining input so a frame uses the freshest arrived samples.
            auto acquired=capy_acquire(host);
            if(acquired<0) {Fail(capy_error());break;}
            if(acquired==2) {std::lock_guard lock(mutex);resize=true;continue;}
            if(acquired==0) {
                std::unique_lock lock(mutex);
                wake.wait_for(lock,std::chrono::milliseconds(16),[&]{return closing||resize;});
                continue;
            }
            std::deque<CanvasWork> pending;
            bool overflow;std::optional<Hover> hover;
            {std::lock_guard lock(mutex);pending=work.Take();overflow=transportFailed;hover=std::exchange(pendingHover,std::nullopt);}
            space.notify_all();
            bool failed=false;
            if(hover&&capy_chrome(host,hover->leave?3:1,hover->x,hover->y,false,menuOpen.load(),hover->touch)<0){Fail(capy_error());break;}
            for(auto& item:pending) {
                int result;
                if(auto points=std::get_if<std::vector<CapyPointer>>(&item)){
                    if(points->empty())continue;
                    auto const& first=points->front();auto const& last=points->back();
                    if(dialogOpen.load()){
                        consumedContacts.insert(first.id);
                        if(last.phase==3||last.phase==4)consumedContacts.erase(first.id);
                        continue;
                    }
                    result=capy_chrome(host,first.phase==1?2:first.phase==4?3:1,
                        first.phase==1?first.x:last.x,first.phase==1?first.y:last.y,true,menuOpen.load(),first.tool==3);
                    if(result>=0){
                        if(first.phase==1&&(result&1))consumedContacts.insert(first.id);
                        result=consumedContacts.contains(first.id)?0:capy_pointer(host,points->data(),points->size());
                        if(last.phase==3||last.phase==4)consumedContacts.erase(first.id);
                    }
                }
                else if(auto scroll=std::get_if<CanvasScroll>(&item))
                    result=dialogOpen.load()?0:capy_scroll(host,scroll->x,scroll->y,scroll->dx,scroll->dy,scroll->density,scroll->zoom,scroll->horizontal);
                else {
                    auto& command=std::get<CanvasCommand>(item);
                    result=DispatchCanvasCommand(host,command);
                    if(result>0)Fail(capy_error()); // A rejected UI action leaves the canvas running.
                }
                if(result<0) {Fail(capy_error());failed=true;break;}
            }
            if(failed) break;
            if((!pending.empty()||hover)&&capy_chrome(host,0,0,0,false,menuOpen.load(),false)<0){Fail(capy_error());break;}
            if(overflow&&capy_input(host,R"({"type":"blur"})")<0){Fail(capy_error());break;}
            auto now=Now();
            auto result=capy_frame(host,now,now);
            if(result<0){Fail(capy_error());break;}
            dirty=result!=0;
            if(!inputStarted){inputStarted=true;inputDispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->StartInput();});}
            {std::lock_guard lock(mutex);revision=capy_view_revision(host);}
            if(auto snapshot=capy_snapshot(host)) {
                std::unique_ptr<char,decltype(&capy_string_free)> owned(snapshot,capy_string_free);
                auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(snapshot));
                Publish(snapshot,model.HasKey(L"state"));
                if(model.HasKey(L"brush_ready")) {
                    bool ready=model.GetNamedBoolean(L"brush_ready");
                    brushReady=ready;
                }
                if(!captured && !dirty && GetEnvironmentVariableW(L"CAPY_TEST_DISPLAY",nullptr,0)) {
                    std::ofstream("canvas-state.json") << snapshot;
                    captured=true;
                }
            }
            if(probe&&!probeReady&&result==0&&brushReady) {
                auto info=capy_surface_info(host);
                if(!info){Fail(capy_error());break;}
                std::unique_ptr<char,decltype(&capy_string_free)> owned(info,capy_string_free);
                auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(info));
                model.Insert(L"process_id",Windows::Data::Json::JsonValue::CreateNumberValue(GetCurrentProcessId()));
                model.Insert(L"ready_qpc_ns",Windows::Data::Json::JsonValue::CreateStringValue(to_hstring(std::to_string(Now()))));
                std::ofstream("presentation-probe.json") << to_string(model.Stringify());
                probeReady=true;
            }
            // Optional readbacks run after painting and yield to newly queued input.
            // The GPU poll never waits; CPU conversion belongs to another worker.
            std::optional<CanvasQuery> preview;
            {std::lock_guard lock(mutex);if(!closing&&work.Empty())preview=previewWork.Take();}
            if(preview){
                auto request=preview->kind==CanvasQueryKind::Filters?capy_filter_previews:
                    preview->kind==CanvasQueryKind::Thumbnails?capy_layer_thumbnails:capy_layer_menu;
                PreviewPacket packet(request(host,preview->json.c_str()),capy_preview_free);
                if(!packet)Fail(capy_error());
                preview->reply(std::move(packet));
            }
            // Opt-in baseline only: present unchanged content at DXGI cadence.
            // No timer, per-frame disk I/O, synthetic input or display-time claim.
            if(probeReady)dirty=true;
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
void CanvasWindow::ApplyResize() {
    Size next;
    {std::lock_guard lock(mutex);if(closing)return;next=desired;resize=false;}
    int result=capy_resize(host,next.width,next.height,next.scale);
    if(result<0) {status.Text(to_hstring(capy_error()));Stop();return;}
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
    if(documents&&documents->IsOpen())return;
    // Commit the focused workspace draft before shared close policy checks dirty state.
    canvasFocus.Focus(FocusState::Programmatic);
    if(settings)settings->CommitEdits();
    Send(R"({"type":"close_settings"})");
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
    wake.notify_all();space.notify_all();
    if(inputController) {
        inputDispatcher.TryEnqueue([weak=weak_from_this()]{
            if(auto self=weak.lock())self->inputSource=nullptr;
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
    if((settings&&settings->IsOpen())||(documents&&documents->IsOpen()))return;
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
    settings.reset();documents.reset();header.reset();workspace.reset();
    root.Children().Clear();toolbar.Children().Clear();canvasFocus.Content(nullptr);
    window.Content(nullptr);
    canvasFocus=nullptr;panel=nullptr;status=nullptr;toolbar=nullptr;root=nullptr;
    inputController=nullptr;inputDispatcher=nullptr;
    CapyLifecycle("views_released");
    closed=true;window.Close();
    CapyLifecycle("window_closed");
}

void CanvasWindow::Publish(std::string snapshot,bool full) {
    // Explicit local test evidence. This can include user state and is never
    // enabled by ordinary or presentation-probe launches.
    if(full&&GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0))
        std::ofstream("ui-state.json") << "{\"process_id\":" << GetCurrentProcessId() << ",\"model\":" << snapshot << "}";
    bool post;
    {
        std::lock_guard lock(mutex);if(closing)return;
        if(full){pendingFull=std::move(snapshot);pendingCamera.clear();}
        else pendingCamera=std::move(snapshot);
        post=!snapshotPosted;snapshotPosted=true;
    }
    if(post)dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyPending();});
}
void CanvasWindow::ApplyPending() {
    std::string full,camera;
    {
        std::lock_guard lock(mutex);snapshotPosted=false;
        if(closing)return;
        full.swap(pendingFull);camera.swap(pendingCamera);
    }
    try {
        if(!full.empty())ApplyModel(Windows::Data::Json::JsonObject::Parse(to_hstring(full)));
        if(!closing&&!camera.empty())workspace->Apply(Windows::Data::Json::JsonObject::Parse(to_hstring(camera)));
    } catch(hresult_error const& error) {Fail(to_string(error.message()));}
}

void CanvasWindow::ApplyDialogs() {
    if(closing){
        // Let the coroutine release its final dialog reference before tearing
        // down XAML. Completion callbacks may run before ShowAsync unwinds.
        dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->Finish();});
        return;
    }
    if(applyingDialogs||!settings||!documents||!lastModel.Size())return;
    applyingDialogs=true;
    struct Reset{bool& flag;~Reset(){flag=false;}}reset{applyingDialogs};
    if(!documents->IsOpen())settings->Apply(lastModel);
    documents->Apply(lastModel,settings->IsOpen());
    UpdatePopup();
}
void CanvasWindow::Popup(bool open) {
    headerPopupOpen=open;UpdatePopup();
}
void CanvasWindow::UpdatePopup() {
    if(closing||closed)return;
    bool blocked=(settings&&settings->IsOpen())||(documents&&documents->IsOpen());
    canvasFocus.IsEnabled(!blocked);
    if(dialogOpen.exchange(blocked)!=blocked&&blocked){
        heldKeys.clear();Send(R"({"type":"blur"})",CanvasCommandKind::Input);
    }
    bool open=headerPopupOpen||workspacePopupOpen||blocked;
    if(menuOpen.exchange(open)==open)return;
    using namespace CapyUi;
    A viewport;viewport.Append(N(panel.ActualWidth()));viewport.Append(N(panel.ActualHeight()));
    Send(to_string(O({{L"type",S(L"chrome")},{L"event",O({{L"kind",S(L"refresh")}})},
        {L"facts",O({{L"held",B(false)},{L"dragging",B(false)},{L"popup_open",B(open)}})},
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
void CanvasWindow::ApplyModel(Windows::Data::Json::JsonObject const& model) {
    using namespace CapyUi;
    lastModel=model;
    auto state=object(model,L"state");auto theme=str(state,L"theme",L"dark");
    if(flag(object(state,L"document_file"),L"close_ready")){Stop();return;}
    if(!statusFailed){
        auto message=str(model,L"error");
        if(message.empty())message=str(state,L"host_error");
        if(message.empty()&&!flag(model,L"brush_ready"))message=L"Preparing brushes…";
        status.Text(message);status.Visibility(message.empty()?Visibility::Collapsed:Visibility::Visible);
    }
    root.RequestedTheme(theme==L"dark"?ElementTheme::Dark:ElementTheme::Light);
    auto foreground=color(str(object(state,L"palette"),L"text",L"#fafafb"));
    window.AppWindow().TitleBar().ButtonForegroundColor(foreground);
    foreground.A=128;window.AppWindow().TitleBar().ButtonInactiveForegroundColor(foreground);
    auto tabs=array(state,L"tabs");
    if(tabs.Size())window.Title(str(tabs.GetObjectAt(0),L"title")+L" · Capy Canvas");
    workspace->Apply(model);header->Apply(model);ApplyDialogs();
}
