#include "pch.h"
#include "CanvasWindow.h"
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
static uint64_t Now() {
    LARGE_INTEGER ticks, frequency;
    QueryPerformanceCounter(&ticks); QueryPerformanceFrequency(&frequency);
    return uint64_t(ticks.QuadPart / frequency.QuadPart) * 1000000000ULL
        + uint64_t(ticks.QuadPart % frequency.QuadPart) * 1000000000ULL / uint64_t(frequency.QuadPart);
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
    titlebar.PreferredHeightOption(TitleBarHeightOption::Standard);
    root.RequestedTheme(ElementTheme::Default);

    canvasFocus.Content(panel);
    canvasFocus.HorizontalContentAlignment(HorizontalAlignment::Stretch);
    canvasFocus.VerticalContentAlignment(VerticalAlignment::Stretch);
    canvasFocus.IsTabStop(true);
    Automation::AutomationProperties::SetName(canvasFocus,L"Drawing canvas");
    root.Children().Append(canvasFocus);
    root.AddHandler(UIElement::KeyDownEvent(),box_value(KeyEventHandler([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
        if(auto self=weak.lock())self->Key(e,true);
    })),true);
    root.AddHandler(UIElement::KeyUpEvent(),box_value(KeyEventHandler([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
        if(auto self=weak.lock())self->Key(e,false);
    })),true);
    // Only the GPU panel fills the client area. XAML chrome overlays that surface.
    toolbar.Orientation(Orientation::Horizontal);
    toolbar.HorizontalAlignment(HorizontalAlignment::Left);
    toolbar.VerticalAlignment(VerticalAlignment::Top);
    toolbar.Margin(Thickness{12,8,0,0});
    toolbar.Spacing(4);
    toolbar.SizeChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->Resize();});
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
        Button backlog;backlog.Content(box_value(L"Test backlog"));
        backlog.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->Replay(false,true);});
        toolbar.Children().Append(backlog);
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
            if(auto self=weak.lock()){self->heldKeys.clear();self->Send(R"({"type":"blur"})",true);}
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
    Size next{uint32_t(std::max(1L, std::lround(panel.ActualWidth()*scale))),
              uint32_t(std::max(1L, std::lround(panel.ActualHeight()*scale))),scale};
    // Physical-pixel drag regions leave the app controls and system caption buttons interactive.
    auto titlebar=window.AppWindow().TitleBar();
    auto dragStart=int32_t(std::ceil((toolbar.ActualWidth()+20)*scale));
    titlebar.SetDragRectangles({
        Windows::Graphics::RectInt32{dragStart,0,
            std::max(0,int32_t(next.width)-titlebar.RightInset()-dragStart),int32_t(40*scale)}});
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
    },Windows::Data::Json::JsonObject::Parse(to_hstring(catalog)));
    root.Children().InsertAt(1,workspace->Root());
    if(auto snapshot=capy_snapshot(host)){
        std::unique_ptr<char,decltype(&capy_string_free)> owned(snapshot,capy_string_free);
        workspace->Apply(Windows::Data::Json::JsonObject::Parse(to_hstring(snapshot)));
    }
    {std::lock_guard lock(mutex);revision=capy_view_revision(host);inputScale=desired.scale;}
    status.Text(L"Preparing brushes…");
    inputController=Microsoft::UI::Dispatching::DispatcherQueueController::CreateOnDedicatedThread();
    inputDispatcher=inputController.DispatcherQueue();
    renderer=std::jthread([this]{Run();});
}
void CanvasWindow::Send(std::string json, bool input) {
    bool overflow=false;
    {
        std::lock_guard lock(mutex);
        if(closing||!host||rendererDone.load()||transportFailed)return;
        if(!work.Push(CanvasCommand{input,std::move(json)}))overflow=transportFailed=true;
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
    bool editing=!canvas||key==VirtualKey::Tab;
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
    Send(to_string(input.Stringify()),true);
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
        bool prepared=capy_prepare_gpu(host)>=0;
        if(!prepared) Fail(capy_error());
        else {std::lock_guard lock(mutex);resize=true;}
        bool dirty=true;
        bool captured=false;
        bool inputStarted=false;
        bool brushReady=false;
        for(;prepared;) {
            {
                std::unique_lock lock(mutex);
                wake.wait(lock,[&]{return closing||resize||dirty||transportFailed||!work.Empty();});
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
            std::deque<CanvasWork> pending;
            bool overflow;
            {std::lock_guard lock(mutex);pending=work.Take();overflow=transportFailed;}
            space.notify_all();
            bool failed=false;
            for(auto& item:pending) {
                int result;
                if(auto points=std::get_if<std::vector<CapyPointer>>(&item))
                    result=capy_pointer(host,points->data(),points->size());
                else if(auto scroll=std::get_if<CanvasScroll>(&item))
                    result=capy_scroll(host,scroll->x,scroll->y,scroll->dx,scroll->dy,scroll->density,scroll->zoom,scroll->horizontal);
                else {
                    auto& command=std::get<CanvasCommand>(item);
                    result=command.input?capy_input(host,command.json.c_str()):capy_action(host,command.json.c_str());
                    if(result>0)Fail(capy_error()); // A rejected UI action leaves the canvas running.
                }
                if(result<0) {Fail(capy_error());failed=true;break;}
            }
            if(failed) break;
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
                    if(ready!=brushReady) {
                        brushReady=ready;
                        dispatcher.TryEnqueue([weak=weak_from_this(),ready]{
                            if(auto self=weak.lock();self&&!self->statusFailed){
                                self->status.Text(ready?L"":L"Preparing brushes…");
                                self->status.Visibility(ready?Visibility::Collapsed:Visibility::Visible);
                            }
                        });
                    }
                }
                if(!captured && !dirty && GetEnvironmentVariableW(L"CAPY_TEST_DISPLAY",nullptr,0)) {
                    std::ofstream("canvas-state.json") << snapshot;
                    captured=true;
                }
            }
            if(overflow)break;
        }
    } catch(hresult_error const& error) {Fail(to_string(error.message()));}
      catch(std::exception const& error) {Fail(error.what());}
      catch(...) {Fail("Unexpected render worker failure");}
    capy_suspend(host);
    rendererDone.store(true);
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
        if(auto self=weak.lock()){self->statusFailed=true;self->status.Text(to_hstring(message));self->status.Visibility(Visibility::Visible);}
    });
}
void CanvasWindow::Stop() {
    {std::lock_guard lock(mutex);if(closing)return;closing=true;}
    wake.notify_all();space.notify_all();
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

void CanvasWindow::Publish(std::string snapshot,bool full) {
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
        if(!full.empty())workspace->Apply(Windows::Data::Json::JsonObject::Parse(to_hstring(full)));
        if(!camera.empty())workspace->Apply(Windows::Data::Json::JsonObject::Parse(to_hstring(camera)));
    } catch(hresult_error const& error) {Fail(to_string(error.message()));}
}
