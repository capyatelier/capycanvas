#include "pch.h"
#include "CanvasWindow.h"
#include <microsoft.ui.xaml.media.dxinterop.h>
#include <winrt/Windows.Graphics.h>
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
static uint64_t Now() {
    LARGE_INTEGER ticks, frequency;
    QueryPerformanceCounter(&ticks); QueryPerformanceFrequency(&frequency);
    return uint64_t(ticks.QuadPart / frequency.QuadPart) * 1000000000ULL
        + uint64_t(ticks.QuadPart % frequency.QuadPart) * 1000000000ULL / uint64_t(frequency.QuadPart);
}
CanvasWindow::~CanvasWindow() {
    { std::lock_guard lock(mutex); closing=true; paused=false; }
    wake.notify_all();
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
    titlebar.PreferredHeightOption(TitleBarHeightOption::Standard);
    Grid root;
    root.RequestedTheme(ElementTheme::Default);

    root.Children().Append(panel);
    // Only the GPU panel fills the client area. XAML chrome overlays that surface.
    StackPanel toolbar;
    toolbar.Orientation(Orientation::Horizontal);
    toolbar.HorizontalAlignment(HorizontalAlignment::Left);
    toolbar.VerticalAlignment(VerticalAlignment::Top);
    toolbar.Margin(Thickness{12,8,0,0});
    toolbar.Spacing(4);
    TextBlock name;
    name.Text(L"Capy Canvas");
    name.VerticalAlignment(VerticalAlignment::Center);
    name.Margin(Thickness{8,0,16,0});
    toolbar.Children().Append(name);
    auto button=[&](wchar_t const* label, char const* json) {
        Button b; b.Content(box_value(label));
        b.Click([weak=weak_from_this(), json=std::string(json)](auto&&,auto&&) {
            if(auto self=weak.lock()) self->Send(json);
        });
        toolbar.Children().Append(b);
    };
    button(L"Undo", R"({"type":"invoke","command":"undo"})");
    button(L"Redo", R"({"type":"invoke","command":"redo"})");
    button(L"Fit", R"({"type":"invoke","command":"fit_canvas"})");
    if(GetEnvironmentVariableW(L"CAPY_SMOKE_TEST",nullptr,0)) {
        for(bool pan:{false,true}) {
            Button test;test.Content(box_value(pan?L"Test pan":L"Test stroke"));
            test.Click([weak=weak_from_this(),pan](auto&&,auto&&){if(auto self=weak.lock())self->Replay(pan);});
            toolbar.Children().Append(test);
        }
    }
    root.Children().Append(toolbar);
    status.Text(L"Starting D3D12 canvas…");
    status.IsHitTestVisible(false);
    status.HorizontalAlignment(HorizontalAlignment::Left);
    status.VerticalAlignment(VerticalAlignment::Bottom);
    status.Margin(Thickness{16,0,0,12});
    root.Children().Append(status);
    window.Content(root);
    panel.Loaded([weak=weak_from_this()](auto&&,auto&&) { if(auto self=weak.lock()) self->Start(); });
    panel.SizeChanged([weak=weak_from_this()](auto&&,auto&&) { if(auto self=weak.lock()) self->Resize(); });
    panel.CompositionScaleChanged([weak=weak_from_this()](auto&&,auto&&) { if(auto self=weak.lock()) self->Resize(); });
    window.Activated([weak=weak_from_this()](auto&&, WindowActivatedEventArgs const& e) {
        if(e.WindowActivationState()==WindowActivationState::Deactivated)
            if(auto self=weak.lock()) self->Send(R"({"type":"blur"})",true);
    });
    window.AppWindow().Closing([weak=weak_from_this()](auto&&,AppWindowClosingEventArgs const& e) {
        if(auto self=weak.lock()) {
            if(!self->closed) {e.Cancel(true);self->Stop();}
        }
    });
    window.AppWindow().Resize({1440,1000});
    // Benchmark placement is opt-in; ordinary launches use Windows placement.
    if(GetEnvironmentVariableW(L"CAPY_TEST_DISPLAY",nullptr,0)) {
        POINT position{LONG_MIN,LONG_MIN};
        EnumDisplayMonitors(nullptr,nullptr,[](HMONITOR monitor,HDC,LPRECT,LPARAM data)->BOOL {
            MONITORINFOEXW info{};info.cbSize=sizeof(info);
            DEVMODEW mode{};mode.dmSize=sizeof(mode);
            if(GetMonitorInfoW(monitor,&info)&&EnumDisplaySettingsW(info.szDevice,ENUM_CURRENT_SETTINGS,&mode)
                &&mode.dmDisplayFrequency>=120) {
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
    float scale=panel.CompositionScaleX();
    inputScale.store(scale);
    Size next{uint32_t(std::max(1.0, panel.ActualWidth()*scale)),
              uint32_t(std::max(1.0, panel.ActualHeight()*scale)),scale};
    // Physical-pixel drag regions leave the app controls and system caption buttons interactive.
    window.AppWindow().TitleBar().SetDragRectangles({
        Windows::Graphics::RectInt32{int32_t(330*scale),0,
            std::max(0,int32_t(next.width)-int32_t(480*scale)),int32_t(40*scale)}});
    { std::lock_guard lock(mutex); desired=next; if(host&&!closing) resize=true; }
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
    revision.store(capy_view_revision(host));
    status.Text(L"D3D12 canvas");
    inputController=Microsoft::UI::Dispatching::DispatcherQueueController::CreateOnDedicatedThread();
    inputDispatcher=inputController.DispatcherQueue();
    renderer=std::jthread([this]{Run();});
}
void CanvasWindow::Send(std::string json, bool input) {
    {std::lock_guard lock(mutex);if(closing||!host)return;work.emplace_back(Command{input,std::move(json)});}
    wake.notify_one();
}
void CanvasWindow::Replay(bool pan) {
    inputDispatcher.TryEnqueue([weak=weak_from_this(),pan] {
        auto self=weak.lock();if(!self)return;
        Size size;
        {std::lock_guard lock(self->mutex);if(self->closing)return;size=self->desired;}
        std::vector<CapyPointer> records;
        uint32_t count=pan?3:42;
        uint64_t now=Now(), view=self->revision.load();
        for(uint32_t i=0;i<count;i++) {
            CapyPointer p{};
            p.id=77;p.sequence=++self->sequence;p.timestamp_ns=now-(count-i)*1000000ULL;
            p.view_revision=view;p.tool=1;p.button=pan?1:0;p.flags=2;p.pressure=0.5f;
            p.phase=i==0?1:i==count-1?3:2;
            p.x=size.width*0.4f+(pan?0.0f:float(i)*5);
            p.y=pan?(i==0?size.height*0.5f:28.0f):size.height*0.5f+24.0f*std::sin(float(i)/6);
            records.push_back(p);
        }
        {std::lock_guard lock(self->mutex);if(self->closing)return;self->work.emplace_back(std::move(records));}
        self->wake.notify_one();
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
        dispatcher.TryEnqueue([weak=weak_from_this()]{
            if(auto self=weak.lock())self->status.Text(L"Canvas ready");
        });
    } catch(hresult_error const& error) {Fail(to_string(error.message()));}
}

void CanvasWindow::Pointer(Microsoft::UI::Input::PointerEventArgs const& e, uint32_t phase) {
    {std::lock_guard lock(mutex);if(closing)return;}
    std::vector<CapyPointer> samples;
    auto points=e.GetIntermediatePoints();
    auto view=revision.load();
    float scale=inputScale.load();
    // WinUI returns newest first. Phase boundaries use only the current point.
    uint32_t count=(phase==0||phase==2)?points.Size():1;
    for(uint32_t i=count;i>0;--i) {
        auto point=points.GetAt(i-1); auto props=point.Properties();
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
    }
    {std::lock_guard lock(mutex);work.emplace_back(std::move(samples));}
    wake.notify_one();e.Handled(true);
}
void CanvasWindow::Run() {
    bool dirty=true;
    bool captured=false;
    bool inputStarted=false;
    for(;;) {
        {
            std::unique_lock lock(mutex);
            wake.wait(lock,[&]{return closing||resize||dirty||!work.empty();});
            if(closing) break;
            if(resize) {
                paused=true;resize=false;
                capy_suspend(host);
                dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyResize();});
                wake.wait(lock,[&]{return !paused||closing;});
                dirty=true;continue;
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
        std::deque<Work> pending;
        {std::lock_guard lock(mutex);pending.swap(work);}
        bool failed=false;
        for(auto& item:pending) {
            int result;
            if(auto points=std::get_if<std::vector<CapyPointer>>(&item))
                result=capy_pointer(host,points->data(),points->size());
            else {
                auto& command=std::get<Command>(item);
                result=command.input?capy_input(host,command.json.c_str()):capy_action(host,command.json.c_str());
            }
            if(result<0) {Fail(capy_error());failed=true;break;}
        }
        if(failed) break;
        auto now=Now();
        auto result=capy_frame(host,now,now);
        if(result<0){Fail(capy_error());break;}
        dirty=result!=0;
        if(!inputStarted){inputStarted=true;inputDispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->StartInput();});}
        revision.store(capy_view_revision(host));
        if(!captured && GetEnvironmentVariableW(L"CAPY_TEST_DISPLAY",nullptr,0)) {
            if(auto snapshot=capy_snapshot(host)) {
                std::ofstream("canvas-state.json") << snapshot;
                capy_string_free(snapshot);captured=true;
            }
        }
    }
    capy_suspend(host);
    rendererDone.store(true);
    dispatcher.TryEnqueue([weak=weak_from_this()] {
        if(auto self=weak.lock()) { if(self->closing) self->Finish(); }
    });
}
void CanvasWindow::ApplyResize() {
    Size next;
    {std::lock_guard lock(mutex);if(closing)return;next=desired;resize=false;}
    int result=capy_resize(host,next.width,next.height,next.scale);
    if(result<0) {status.Text(to_hstring(capy_error()));Stop();return;}
    revision.store(capy_view_revision(host));
    {std::lock_guard lock(mutex);paused=false;}
    wake.notify_one();
}
void CanvasWindow::Fail(std::string message) {
    dispatcher.TryEnqueue([weak=weak_from_this(),message=std::move(message)] {
        if(auto self=weak.lock()) self->status.Text(to_hstring(message));
    });
}
void CanvasWindow::Stop() {
    {std::lock_guard lock(mutex);if(closing)return;closing=true;}
    wake.notify_all();
    if(inputController) {
        inputDispatcher.TryEnqueue([weak=weak_from_this()]{
            if(auto self=weak.lock())self->inputSource=nullptr;
        });
        inputController.ShutdownQueueAsync().Completed([weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock())self->dispatcher.TryEnqueue([weak]{
                if(auto self=weak.lock()){self->inputDone=true;self->Finish();}
            });
        });
    } else inputDone=true;
    if(!renderer.joinable()||rendererDone.load()) Finish();
}
void CanvasWindow::Finish() {
    if(closed||!inputDone||(renderer.joinable()&&!rendererDone.load()))return;
    if(renderer.joinable()) renderer.join();
    // The worker no longer owns any acquired image; detach on the XAML thread.
    panel.as<ISwapChainPanelNative>()->SetSwapChain(nullptr);
    capy_destroy(host);host=nullptr;
    closed=true;window.Close();
}
