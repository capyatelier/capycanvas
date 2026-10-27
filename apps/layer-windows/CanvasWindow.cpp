#include "pch.h"
#include "CanvasWindow.h"
#include "TraceFile.h"
#include "CanvasPointerSample.h"
#include "PenInput.h"
#include "UiControls.h"
#include "KeyNames.h"
#include "NativeKeyRepeats.h"
#include "NavigationCursors.h"
#include "NewDrawingDialog.h"
#include "ExternalImages.h"
#include <microsoft.ui.xaml.media.dxinterop.h>
#include <microsoft.ui.xaml.window.h>
#include <ShellScalingApi.h>
#include <CommCtrl.h>
#include <dwmapi.h>
#include <shellapi.h>
#include <propsys.h>
#include <propkey.h>
#include <winrt/Windows.Graphics.h>
#include <winrt/Microsoft.UI.Content.h>
#include <winrt/Microsoft.UI.Xaml.Automation.h>
#include <algorithm>
#include <future>
#include <chrono>
#include <cmath>
#include <fstream>

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Input;
using namespace Microsoft::UI::Xaml::Media;
using Microsoft::UI::Xaml::Automation::AutomationProperties;
using namespace Microsoft::UI::Windowing;
using namespace Windows::Foundation;
static bool SameRegions(std::vector<Windows::Graphics::RectInt32> const& a,std::vector<Windows::Graphics::RectInt32> const& b){
    return a.size()==b.size()&&std::equal(a.begin(),a.end(),b.begin(),[](auto x,auto y){return x.X==y.X&&x.Y==y.Y&&x.Width==y.Width&&x.Height==y.Height;});
}
static std::string PreferredLanguages() {
    const auto tags=[] {
        DWORD count=0,size=0;
        Windows::Data::Json::JsonArray tags;
        if(GetUserPreferredUILanguages(MUI_LANGUAGE_NAME,&count,nullptr,&size)&&size){
            std::vector<wchar_t> languages(size);
            if(GetUserPreferredUILanguages(MUI_LANGUAGE_NAME,&count,languages.data(),&size)){
                for(size_t i=0;i<languages.size()&&languages[i];){
                    auto start=i;while(i<languages.size()&&languages[i])++i;
                    tags.Append(Windows::Data::Json::JsonValue::CreateStringValue(hstring(std::wstring_view(languages.data()+start,i-start))));++i;
                }
            }
        }
        return to_string(tags.Stringify());
    }();
    return tags;
}
static int DispatchCanvasCommand(CapyHost  host,CanvasCommand const& command) {
    auto json=command.json.c_str();
    switch(command.kind){
        case CanvasCommandKind::Input:return capy_input(host,json);
        case CanvasCommandKind::EdgeContact:{
            auto result=capy_edge_contact(host,json);
            if(result>=0&&command.reply)command.reply(result);
            return std::min(result,0);
        }
        case CanvasCommandKind::LocalizationInput:return capy_localization_input(host,command.json=="true");
        case CanvasCommandKind::Prediction:return capy_native_prediction(host,command.json=="true");
        case CanvasCommandKind::Document:return capy_document_action(host,json);
        case CanvasCommandKind::Workspace:return capy_workspace_action(host,json);
        case CanvasCommandKind::Overviews:return capy_overviews(host,json);
        case CanvasCommandKind::Glass:return capy_glass(host,json);
        case CanvasCommandKind::Action:return capy_action(host,json);
        case CanvasCommandKind::Filters:return capy_load_filter_directory(host,json);
        case CanvasCommandKind::DeviceLoss:return capy_test_device_loss(host,json);
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
static std::chrono::nanoseconds RefreshPeriod(HWND window) {
    struct Clock {HMONITOR monitor=nullptr;std::chrono::steady_clock::time_point checked{};std::chrono::nanoseconds period{16666667};};
    thread_local Clock clock;
    auto now=std::chrono::steady_clock::now();auto monitor=MonitorFromWindow(window,MONITOR_DEFAULTTONEAREST);
    if(monitor==clock.monitor&&now-clock.checked<std::chrono::seconds(1))return clock.period;
    clock.monitor=monitor;clock.checked=now;
    MONITORINFOEXW output{};output.cbSize=sizeof(output);
    DEVMODEW mode{};mode.dmSize=sizeof(mode);
    if(GetMonitorInfoW(monitor,reinterpret_cast<MONITORINFO*>(&output))
        &&EnumDisplaySettingsW(output.szDevice,ENUM_CURRENT_SETTINGS,&mode)&&mode.dmDisplayFrequency>1){
        clock.period=std::chrono::nanoseconds(1000000000LL/mode.dmDisplayFrequency);
        if(!(output.dwFlags&MONITORINFOF_PRIMARY))return clock.period;
    }
    DWM_TIMING_INFO info{};info.cbSize=sizeof(info);LARGE_INTEGER frequency;
    if(SUCCEEDED(DwmGetCompositionTimingInfo(nullptr,&info))&&info.qpcRefreshPeriod&&QueryPerformanceFrequency(&frequency))
        clock.period=std::chrono::nanoseconds(int64_t(double(info.qpcRefreshPeriod)*1e9/double(frequency.QuadPart)));
    return clock.period;
}
void CapyLifecycle(char const* event) {
    if(!GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0))return;
    static std::mutex logMutex;
    std::lock_guard lock(logMutex);
    std::ofstream("lifecycle.log",std::ios::app)
        << GetCurrentProcessId() << " " << Now() << " " << event << "\n";
}
CanvasWindow::CanvasWindow(std::function<void()> create,std::function<void(uint64_t)> close,
    std::function<void(uint64_t)> preferencesChanged,std::function<void()> deviceLoss)
    :windowId(window.AppWindow().Id().Value),
     createWindow(std::move(create)),testDeviceLoss(std::move(deviceLoss)),onClosed(std::move(close)),workspacePreferencesChanged(std::move(preferencesChanged)) {
    // HWND/WindowId can be reused after an earlier window closes. Invalidate
    // its old diagnostic model before publishing the new live-window manifest.
    if(GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0))for(auto kind:{"ui-state","camera-state","presentation-state"})TraceState(kind,"{}");
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
static winrt::fire_and_forget PublishPanelMetrics(std::filesystem::path name,std::string payload){
    try{
        co_await winrt::resume_background();
        WriteTraceFile(name,payload);
    }catch(...){co_return;}
}
void CanvasWindow::TraceState(char const* kind,std::string const& value)const {
    WriteTraceFile(std::string(kind)+"-"+std::to_string(GetCurrentProcessId())+"-"+std::to_string(windowId)+".json",value);
}
void CanvasWindow::TracePresentation(bool frame,bool dirty,bool repeat) {
    auto& trace=presentationTrace;
    if(!trace.enabled)return;
    uint64_t stats[6]{};
    auto shown=capy_presentation_stats(host,stats)==0?std::optional<uint64_t>(stats[1]):std::nullopt;
    trace.Observe(frame,dirty,repeat,stats[0],shown,std::chrono::steady_clock::now());
    if(frame)return;
    auto value="{\"process_id\":"+std::to_string(GetCurrentProcessId())+",\"window_id\":"+std::to_string(windowId)+
        ",\"presentation_epoch\":"+std::to_string(trace.epoch)+",\"required\":"+std::to_string(trace.required)+
        ",\"displayed\":"+(trace.displayed?std::to_string(*trace.displayed):std::string("null"))+"}";
    TraceState("presentation-state",value);
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
    latencyTrace.Dump("latency-"+std::to_string(GetCurrentProcessId())+"-"+std::to_string(windowId));
    if (host) capy_destroy(host);
}
void CanvasWindow::Open() {
    try {
    dispatcher=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread();
    window.Title(L"Capy Canvas");
    window.ExtendsContentIntoTitleBar(true);
    com_ptr<IPropertyStore> properties;
    if(SUCCEEDED(SHGetPropertyStoreForWindow(Handle(),IID_PPV_ARGS(properties.put())))){
        PROPVARIANT value{};value.vt=VT_BOOL;value.boolVal=VARIANT_TRUE;
        properties->SetValue(PKEY_EdgeGesture_DisableTouchWhenFullscreen,value);
    }
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
    root.Children().Append(canvasFocus);
    panel.as<IUIElementProtected>().ProtectedCursor(nullptr);
    StartCanvasTouch(xamlCanvasTouch);
    root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
        if(auto self=weak.lock();self&&e.OriginalSource()!=self->panel){
            self->CancelCanvasTouch(self->xamlCanvasTouch);
            if(self->inputDispatcher)self->inputDispatcher.TryEnqueue([weak]{if(auto self=weak.lock())self->CancelCanvasTouch(self->canvasTouch);});
        }
    })),true);
    panel.PointerPressed([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->XamlPointer(e,1);});
    panel.PointerMoved([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->XamlPointer(e,e.GetCurrentPoint(self->panel).IsInContact()?2u:0u);});
    CapyUi::PointerGesture::OnCompleted(panel,[weak=weak_from_this()](PointerRoutedEventArgs const& e,bool cancelled){if(auto self=weak.lock())self->XamlPointer(e,cancelled?4u:3u);},CapyUi::PointerCaptureLoss::CompleteLift);
    panel.PointerWheelChanged([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->inputDispatcher&&!e.Handled()&&e.OriginalSource()==self->panel){
        auto point=e.GetCurrentPoint(self->panel);auto props=point.Properties();auto sample=self->ScrollSample(point.Position(),props.MouseWheelDelta(),props.IsHorizontalMouseWheel(),e.KeyModifiers());
        self->inputDispatcher.TryEnqueue([weak,sample]{if(auto self=weak.lock();self&&!self->closing)self->SendIndependent(sample);});e.Handled(true);
    }});
    panel.PointerExited([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->inputDispatcher&&!e.Handled()){
        auto point=e.GetCurrentPoint(self->panel);auto id=point.PointerId();bool pen=point.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Pen;
        if(!point.IsInContact())self->inputDispatcher.TryEnqueue([weak,id,pen]{if(auto self=weak.lock();self&&!self->closing){if(pen){self->PenButtons(id,false);std::lock_guard lock(self->mutex);self->penSources.Forget(id);}self->SendIndependent(CanvasCommand{CanvasCommandKind::Input,R"({"type":"cursor_leave"})"});}});
    }});
    textFocus=FocusManager::GettingFocus(auto_revoke,[weak=weak_from_this()](auto&&,GettingFocusEventArgs const& e){
        if(auto self=weak.lock();self&&!self->closed)if(auto entry=e.NewFocusedElement().try_as<TextBox>();entry&&entry.XamlRoot()==self->root.XamlRoot()){
            entry.Language(self->root.Language());CapyUi::captureTextComposition(entry,[weak](bool busy){
                if(auto self=weak.lock();self&&!self->closing)self->Send(busy?"true":"false",CanvasCommandKind::LocalizationInput);
            });
        }
    });
    CapyUi::ShortcutKeys(root,[weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
        if(auto self=weak.lock();self&&!CapyUi::composingKey(e)&&!self->dialogOpen.load()&&(!self->header||!self->header->Key(e,true)))self->Key(e,true);
    });
    root.PreviewKeyUp([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
        if(auto self=weak.lock())if(!self->dialogOpen.load()&&(!self->header||!self->header->Key(e,false)))self->Key(e,false);
    });
    PointerEventHandler contact([](winrt::Windows::Foundation::IInspectable const&,PointerRoutedEventArgs const& e){
        CapyUi::setTouchContact(e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch);
    });
    root.AddHandler(UIElement::PointerPressedEvent(),box_value(contact),true);
    root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([](auto&&,auto&&){CapyUi::hideRevealedTooltip();})),true);
    root.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
        CapyUi::setTouchContact(e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch);
        if(auto self=weak.lock())self->ChromeMotion(e);
    })),true);
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
        recover.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->testDeviceLoss();});
        toolbar.Children().Append(recover);
        if(GetEnvironmentVariableW(L"CAPY_TEST_HDR",nullptr,0))for(bool hdr:{false,true}){Button test;test.Content(box_value(hdr?L"Test HDR output":L"Test SDR output"));test.Click([weak=weak_from_this(),hdr](auto&&,auto&&){if(auto self=weak.lock())self->Send(hdr?"hdr":"sdr",CanvasCommandKind::TestDisplay);});toolbar.Children().Append(test);}
    }
    root.Children().Append(toolbar);
    Microsoft::UI::Xaml::Automation::AutomationProperties::SetAutomationId(status,L"canvas-status");
    status.IsHitTestVisible(false);
    status.HorizontalAlignment(HorizontalAlignment::Center);
    status.VerticalAlignment(VerticalAlignment::Bottom);
    status.Margin(Thickness{0,0,0,40});
    root.Children().Append(status);
    screenChip.HorizontalAlignment(HorizontalAlignment::Center);screenChip.VerticalAlignment(VerticalAlignment::Bottom);screenChip.Margin({0,0,0,76});screenChip.Visibility(Visibility::Collapsed);screenChip.IsTabStop(false);
    AutomationProperties::SetAutomationId(screenChip,L"screen-status");
    StackPanel screenDetails;screenDetails.Width(312);screenDetails.Spacing(6);
    for(auto text:{screenTitle,screenHeadline,screenBody}){text.TextWrapping(TextWrapping::Wrap);screenDetails.Children().Append(text);}
    screenTitle.Opacity(.7);screenHeadline.FontWeight(Windows::UI::Text::FontWeights::SemiBold());screenDetails.Children().Append(screenClipped);screenPopup.Content(screenDetails);screenChip.Flyout(screenPopup);
    AutomationProperties::SetAutomationId(screenDetails,L"screen-details");AutomationProperties::SetAutomationId(screenHeadline,L"screen-details-headline");AutomationProperties::SetAutomationId(screenClipped,L"screen-show-clipped");
    screenPopup.Opened([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){self->screenPopupOpen=true;self->UpdatePopup();}});
    screenPopup.Closed([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){self->screenPopupOpen=false;self->UpdatePopup();}});
    auto clipped=[weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updatingScreen)self->Send(to_string(CapyUi::O({{L"type",CapyUi::S(L"show_clipped_colors")},{L"visible",CapyUi::B(self->screenClipped.IsChecked().Value())}}).Stringify()));};
    screenClipped.Checked(clipped);screenClipped.Unchecked(clipped);root.Children().Append(screenChip);

    window.Content(root);
    panel.Loaded([weak=weak_from_this()](auto&&,auto&&) { if(auto self=weak.lock()) self->Start(); });
    panel.SizeChanged([weak=weak_from_this()](auto&&,auto&&) { if(auto self=weak.lock()) self->Resize(); });
    panel.CompositionScaleChanged([weak=weak_from_this()](auto&&,auto&&) { if(auto self=weak.lock()) self->Resize(); });
    window.Activated([weak=weak_from_this()](auto&&, WindowActivatedEventArgs const& e) {
        if(auto self=weak.lock()){
            if(self->gamepad)self->gamepad->Active(e.WindowActivationState()!=WindowActivationState::Deactivated);
            if(e.WindowActivationState()==WindowActivationState::Deactivated){
                // A WinUI submenu can deactivate the main HWND while focus
                // stays inside its owned popup tree. Check after activation
                // settles before retiring the workspace contact or menu.
                self->dispatcher.TryEnqueue([weak]{if(auto self=weak.lock();self&&!self->closed){
                    auto owner=self->Handle();
                    auto foreground=GetForegroundWindow();
                    // Tablet/IME helper HWNDs can briefly own activation without
                    // presenting another application. Preserve the idle placement
                    // across those transitions; visible app switches still blur.
                    if(IsIconic(owner)||(IsWindowVisible(foreground)&&GetAncestor(foreground,GA_ROOTOWNER)!=owner)){
                        if(self->header)self->header->CancelNativeContacts();
                        if(self->workspace)self->workspace->CancelNativeContacts();
                        self->CancelGesture();self->heldKeys.clear();self->sentModifiers.store(0);
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
            bool same=captionRegionsValid&&SameRegions(regions,captionRegions)&&SameRegions(inputRegions,captionInputRegions);
            // Painting snapshots can refresh header state without moving controls.
            // Only changed hit geometry needs a non-client window update.
            if(!same){
                auto source=Microsoft::UI::Input::InputNonClientPointerSource::GetForWindowId(window.AppWindow().Id());
                source.SetRegionRects(Microsoft::UI::Input::NonClientRegionKind::Caption,regions);
                source.SetRegionRects(Microsoft::UI::Input::NonClientRegionKind::Passthrough,inputRegions);
                bubbleInputRegions=inputRegions;
                captionRegions=std::move(regions);captionInputRegions=std::move(inputRegions);captionRegionsValid=true;
            }
        }else if(captionRegionsValid){
            auto source=Microsoft::UI::Input::InputNonClientPointerSource::GetForWindowId(window.AppWindow().Id());
            source.ClearRegionRects(Microsoft::UI::Input::NonClientRegionKind::Caption);
            source.ClearRegionRects(Microsoft::UI::Input::NonClientRegionKind::Passthrough);
            captionRegions.clear();captionInputRegions.clear();bubbleInputRegions.clear();captionRegionsValid=false;
        }
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
    if(glassPosted)return;
    glassPosted=true;
    if(!dispatcher.TryEnqueue([weak=weak_from_this()]{
    auto self=weak.lock();if(!self)return;
    self->glassPosted=false;
    if(self->closing||self->closed||!self->panel||!self->workspace)return;
    if(self->captionRegionsValid){
        auto bubbles=self->workspace->OverlayInputRegions(self->panel);auto scale=self->root.XamlRoot().RasterizationScale();auto input=self->captionInputRegions;
        for(auto value:bubbles){auto bounds=value.GetObject();
            auto x=int32_t(std::floor(CapyUi::num(bounds,L"x")*scale)),y=int32_t(std::floor(CapyUi::num(bounds,L"y")*scale));
            auto right=int32_t(std::ceil((CapyUi::num(bounds,L"x")+CapyUi::num(bounds,L"width"))*scale)),bottom=int32_t(std::ceil((CapyUi::num(bounds,L"y")+CapyUi::num(bounds,L"height"))*scale));
            for(auto caption:self->captionRegions){auto left=std::max(x,caption.X),top=std::max(y,caption.Y),width=std::min(right,caption.X+caption.Width)-left,height=std::min(bottom,caption.Y+caption.Height)-top;
                if(width>0&&height>0)input.push_back({left,top,width,height});}
        }
        if(!SameRegions(input,self->bubbleInputRegions)){self->bubbleInputRegions=input;
            auto source=Microsoft::UI::Input::InputNonClientPointerSource::GetForWindowId(self->window.AppWindow().Id());source.SetRegionRects(Microsoft::UI::Input::NonClientRegionKind::Passthrough,input);
        }
    }
    Windows::Data::Json::JsonArray connections,arcs,colors,panels;uint64_t panelRevision=0;auto regions=self->workspace->Glass(self->panel,connections,arcs,colors,panels,panelRevision);
    if(self->header)self->header->AppendGlass(regions,self->panel);
    Windows::Data::Json::JsonObject request;request.Insert(L"regions",regions);request.Insert(L"connections",connections);request.Insert(L"arcs",arcs);request.Insert(L"color_bubbles",colors);
    request.Insert(L"panels",panels);request.Insert(L"panel_revision",CapyUi::N(double(panelRevision)));
    auto json=to_string(request.Stringify());
    if(json==self->lastGlass)return;self->lastGlass=json;self->Send(json,CanvasCommandKind::Glass);
    }))glassPosted=false;
}
void CanvasWindow::Start() {
    if(renderer.joinable()||closing)return;
    auto tags=PreferredLanguages();
    auto screen=to_string(CapyUi::drawingScreen(window).Stringify());
    renderer=std::jthread([this,tags=std::move(tags),screen=std::move(screen)]{
        try {
        struct Apartment {
            Apartment(){init_apartment(apartment_type::multi_threaded);}
            ~Apartment(){uninit_apartment();}
        } apartment;
        auto prepared=NativeLaunch(capy_launch(tags.c_str(),screen.c_str()),capy_launch_free);
        bool posted=prepared&&dispatcher.TryEnqueue([weak=weak_from_this(),prepared]{
            if(auto self=weak.lock()){
                bool initialized=false;
                try {if(!self->closing)initialized=self->StartPrepared(prepared.get());}
                catch(hresult_error const& error){OutputDebugStringW(error.message().c_str());self->Fail(to_string(CapyUi::str(self->bootstrap,L"canvas_init_failed")));}
                catch(std::exception const& error){OutputDebugStringA(error.what());self->Fail(to_string(CapyUi::str(self->bootstrap,L"canvas_init_failed")));}
                {std::lock_guard lock(self->mutex);self->launchReady=true;self->launchSucceeded=initialized;}
                self->wake.notify_one();
            }
        });
        if(posted){
            std::unique_lock lock(mutex);wake.wait(lock,[&]{return launchReady||closing;});
            if(host&&launchSucceeded&&!closing){lock.unlock();Run();return;}
        }else Fail(NativeString(capy_error()).get());
        prepared.reset();
        } catch(hresult_error const& error){OutputDebugStringW(error.message().c_str());Fail(to_string(CapyUi::str(bootstrap,L"canvas_init_failed")));}
          catch(std::exception const& error){OutputDebugStringA(error.what());Fail(to_string(CapyUi::str(bootstrap,L"canvas_init_failed")));}
        rendererDone.store(true);space.notify_all();
        dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock();self&&self->closing)self->Finish();});
    });
}
bool CanvasWindow::StartPrepared(CapyLaunch  prepared) {
    if(host||closing)return false;
    bootstrap=Windows::Data::Json::JsonObject::Parse(to_hstring(NativeString(capy_launch_view(prepared)).get()));
    window.Title(CapyUi::str(bootstrap,L"app_name"));
    localization=NativeLocalization(capy_launch_localization(prepared),capy_localization_free);
    if(!localization)throw hresult_error(E_ABORT);
    root.Language(CapyUi::str(bootstrap,L"active_tag"));
    TraceState("bootstrap",to_string(bootstrap.Stringify()));
    Automation::AutomationProperties::SetAutomationId(canvasFocus,L"drawing-canvas");
    Automation::AutomationProperties::SetName(canvasFocus,CapyUi::str(bootstrap,L"drawing_canvas"));
    Automation::AutomationProperties::SetHelpText(canvasFocus,CapyUi::str(bootstrap,L"drawing_canvas_help"));
    Automation::AutomationProperties::SetName(root,CapyUi::str(bootstrap,L"drawing_workspace"));
    status.Text(CapyUi::str(bootstrap,L"preparing_canvas"));
    Resize();
    host=capy_create(desired.width,desired.height,desired.scale,prepared);
    if(!host) {OutputDebugStringA(NativeString(capy_error()).get());status.Text(CapyUi::str(bootstrap,L"canvas_init_failed"));return false;}
    surfaceHandle=capy_surface_handle(host);if(!surfaceHandle)throw hresult_error(E_FAIL);
    surfaceState.Reset(capy_surface_generation(host));
    nativeWindow=Handle();capy_set_window(host,nativeWindow);
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
    if(!catalog){OutputDebugStringA(NativeString(capy_error()).get());status.Text(CapyUi::str(bootstrap,L"canvas_init_failed"));return false;}
    NativeString ownedCatalog(catalog,capy_string_free);
    auto catalogView=Windows::Data::Json::JsonObject::Parse(to_hstring(ownedCatalog.get()));
    catalogView.Insert(L"bootstrap",bootstrap);
    if(GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)){
        auto process=std::to_string(GetCurrentProcessId()),window=std::to_string(windowId);
        PublishPanelMetrics("panel-metrics-"+process+"-"+window+".json","{\"process_id\":"+process+",\"window_id\":"+window+",\"panel_metrics\":"+to_string(CapyUi::object(catalogView,L"panel_metrics").Stringify())+"}");
    }
    screenCopy=CapyUi::object(CapyUi::object(catalogView,L"native_copy"),L"color");
    screenFontSize=CapyUi::num(catalogView,L"text_size_pt",11)*4/3;
    workspace=std::make_unique<WorkspaceView>([weak=weak_from_this()](std::string json){
        if(auto self=weak.lock())self->Send(std::move(json));
    },catalogView,localization,[weak=weak_from_this()](std::string json){
        if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Overviews);
    },[weak=weak_from_this()](CanvasQueryKind kind,std::string json,PreviewReply reply){
        if(auto self=weak.lock())return self->RequestPreviews(kind,std::move(json),std::move(reply));return false;
    },[weak=weak_from_this()](bool open){if(auto self=weak.lock()){self->workspacePopupOpen=open;self->UpdatePopup();}},
    [weak=weak_from_this()](std::string json){if(auto self=weak.lock()){
        self->canvasFocus.Focus(FocusState::Programmatic);
        self->Send(std::move(json),CanvasCommandKind::Document);
    }},[weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Input);},
    [weak=weak_from_this()](std::string json,std::function<void(int32_t)> reply){if(auto self=weak.lock())
        self->Send(std::move(json),CanvasCommandKind::EdgeContact,[dispatcher=self->dispatcher,reply=std::move(reply)](int32_t bits){
            dispatcher.TryEnqueue([reply,bits]{reply(bits);});
        });});
    workspace->SetWindowId(windowId);
    workspace->SetGlassChanged([weak=weak_from_this()]{if(auto self=weak.lock())self->PublishGlass();});
    root.Children().InsertAt(1,workspace->Root());
    auto send=[weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json));};
    auto model=catalogView;
    header=std::make_unique<HeaderView>(send,model,localization,
        [weak=weak_from_this()](bool open){if(auto self=weak.lock())self->Popup(open);},
        [weak=weak_from_this()]{if(auto self=weak.lock()){
            self->Resize();
        }},
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
    workspace->AttachOverlays(root,[weak=weak_from_this()]{if(auto self=weak.lock();self&&self->header)self->header->CancelGesture();});
    settings=std::make_unique<SettingsView>(send,model,localization,root.XamlRoot(),
        [weak=weak_from_this()](KeyRoutedEventArgs const& e,bool pressed){if(auto self=weak.lock())self->Key(e,pressed);},
        [weak=weak_from_this()](std::string error){if(auto self=weak.lock()){OutputDebugStringA(error.c_str());self->Fail(to_string(CapyUi::str(CapyUi::object(self->bootstrap,L"recovery"),L"preferences_failed")));}},
        [weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();},
        [weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Document);});
    settings->SetWindowId(windowId);
    gamepad=std::make_unique<GamepadInput>([weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Input);});
    gamepad->Active(true);
    documents=std::make_unique<DocumentView>(
        [weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Document);},
        send,model,localization,window,[weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();},
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
        CapyUi::receiveFileDrop(event,action,[weak](std::string json){if(auto self=weak.lock();self&&!self->closing)self->Send(std::move(json),CanvasCommandKind::Document);});
    }});
    for(auto [dialog,kind]:{std::pair{&canvasSize,SizeDialog::Kind::Canvas},std::pair{&imageSize,SizeDialog::Kind::Image}})
        *dialog=std::make_unique<SizeDialog>(kind,send,model,localization,root.XamlRoot(),
            [weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();});
    workspaceDialogs=std::make_unique<WorkspaceDialogs>(send,model,localization,root.XamlRoot(),
        [weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();},
        [weak=weak_from_this()](std::string error){if(auto self=weak.lock())self->Fail(std::move(error));});
    workspaceStorage=std::make_unique<WorkspaceStorageView>(
        [weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Workspace);},
        window,[weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();});
    root.Children().Append(workspaceStorage->Root());
    workspaceManager=std::make_unique<WorkspaceManagerView>(
        [weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->Send(std::move(json),CanvasCommandKind::Workspace);},
        catalogView,localization,root.XamlRoot(),[weak=weak_from_this()]{if(auto self=weak.lock())self->ApplyDialogs();},
        [weak=weak_from_this()](CanvasQueryKind kind,std::string json,PreviewReply reply){if(auto self=weak.lock())return self->RequestPreviews(kind,std::move(json),std::move(reply));return false;});
    if(auto snapshot=capy_snapshot(host,nullptr)){
        NativeString owned(snapshot,capy_string_free);
        ApplyModel(Windows::Data::Json::JsonObject::Parse(to_hstring(owned.get())));
    }
    {std::lock_guard lock(mutex);revision=capy_view_revision(host);inputScale=desired.scale;}
    status.Text(CapyUi::str(bootstrap,L"preparing_brush"));
    inputController=Microsoft::UI::Dispatching::DispatcherQueueController::CreateOnDedicatedThread();
    inputDispatcher=inputController.DispatcherQueue();
    return true;
}
bool CanvasWindow::RequestPreviews(CanvasQueryKind kind,std::string json,PreviewReply reply) {
    {std::lock_guard lock(mutex);
        if(closing||!host||rendererDone.load()||transportFailed||(inputStopped&&kind!=CanvasQueryKind::Workspace)||json.capacity()>CanvasQueryQueue::MaxPayload)return false;
        if(kind==CanvasQueryKind::LayerMenu)previewWork.Take(kind);
        auto state=snapshots;
        {
            std::lock_guard snapshotLock(state->mutex);
            if(state->pending.Closed())return false;
        }
        auto reservation=state->queries.Acquire();if(!reservation)return false;
        auto ordered=[state,reservation,reply=std::move(reply)](PreviewPacket packet)mutable{
            auto deliver=[reservation,reply=std::move(reply),packet=std::move(packet)]()mutable{reply(std::move(packet));};
            {
                std::lock_guard snapshotLock(state->mutex);
                if(state->pending.Closed())return;
                auto sequence=state->pending.Sequence();
                if(!state->pending.IsPosted(sequence)){
                    state->replies.emplace_back(sequence,std::move(deliver));return;
                }
            }
            deliver();
        };
        if(!previewWork.Push(CanvasQuery{kind,std::move(json),std::move(ordered)}))return false;
    }
    wake.notify_one();return true;
}
void CanvasWindow::Send(std::string json, CanvasCommandKind kind, std::function<void(int32_t)> reply) {
    bool overflow=false;
    {
        std::lock_guard lock(mutex);
        if(closing||!host||rendererDone.load()||transportFailed)return;
        if(!work.Push(CanvasCommand{kind,std::move(json),std::move(reply)}))overflow=transportFailed=true;
    }
    // UI callbacks never wait for the render worker. An exhausted command
    // channel fails explicitly, drains accepted work and cancels active input.
    if(overflow)Fail("Canvas input queue is full. Close and reopen the window to continue.");
    wake.notify_one();space.notify_all();
}
bool CanvasWindow::SendIndependent(CanvasWork item) {
    if(keyBalance.load()>0){if(heldWork.empty())holdTimer.Start();heldWork.push_back(std::move(item));return true;}
    return FlushHeld()&&Enqueue(std::move(item));
}
bool CanvasWindow::FlushHeld() {
    if(keyBalance.load()>0)return true;
    if(holdTimer)holdTimer.Stop();
    for(auto& item:std::exchange(heldWork,{}))if(!Enqueue(std::move(item)))return false;
    return true;
}
bool CanvasWindow::Enqueue(CanvasWork item) {
    std::unique_lock lock(mutex);
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
        UpdateNavigationCursor();
        // The OS supplies prediction; shared Rust keeps it out of document truth.
        try {
            pointerPredictor=PointerPredictor::CreateForInputPointerSource(inputSource);
            pointerPredictor.PredictionTime(std::chrono::milliseconds(16));
        } catch(hresult_error const&) { pointerPredictor=nullptr; }
        holdTimer=inputDispatcher.CreateTimer();holdTimer.IsRepeating(false);holdTimer.Interval(std::chrono::seconds(1));
        holdTimer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){self->keyBalance.store(0);self->FlushHeld();}});
        SendIndependent(CanvasCommand{CanvasCommandKind::Prediction,pointerPredictor?"true":"false"});
        [[maybe_unused]] static ATOM keys=[]{
            WNDCLASSEXW type{sizeof(type)};type.hInstance=GetModuleHandleW(nullptr);type.lpszClassName=L"CapyCanvasOrderedKeys";
            type.lpfnWndProc=[](HWND target,UINT message,WPARAM wparam,LPARAM lparam)->LRESULT{
                auto owner=reinterpret_cast<std::weak_ptr<CanvasWindow>*>(GetWindowLongPtrW(target,GWLP_USERDATA));
                if(owner&&message==WM_APP)if(auto self=owner->lock())self->RawKeysHeld();
                if(owner&&message==WM_INPUT)if(auto self=owner->lock()){
                    RAWINPUT input{};UINT size=sizeof input;
                    if(GetRawInputData(reinterpret_cast<HRAWINPUT>(lparam),RID_INPUT,&input,&size,sizeof(RAWINPUTHEADER))!=UINT(-1)
                        &&input.header.dwType==RIM_TYPEKEYBOARD)self->RawKey(input.data.keyboard);
                }
                if(message==WM_DESTROY)PostQuitMessage(0);
                if(message==WM_NCDESTROY){SetWindowLongPtrW(target,GWLP_USERDATA,0);delete owner;}
                return DefWindowProcW(target,message,wparam,lparam);
            };
            return RegisterClassExW(&type);
        }();
        std::promise<HWND> reader;auto target=reader.get_future();
        std::thread([reader=std::move(reader),owner=weak_from_this()]() mutable {
            init_apartment(apartment_type::multi_threaded);
            HWND window=CreateWindowExW(0,L"CapyCanvasOrderedKeys",L"",0,0,0,0,0,HWND_MESSAGE,nullptr,GetModuleHandleW(nullptr),nullptr);
            if(window)SetWindowLongPtrW(window,GWLP_USERDATA,reinterpret_cast<LONG_PTR>(new std::weak_ptr<CanvasWindow>(owner)));
            reader.set_value(window);
            MSG message;
            while(window&&GetMessageW(&message,nullptr,0,0)>0)DispatchMessageW(&message);
            uninit_apartment();
        }).detach();
        orderedKeys=target.get();
        StartCanvasTouch(canvasTouch);
        inputSource.PointerPressed([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){self->Pointer(e,1);self->CanvasTouch(e,1);}
        });
        inputSource.PointerMoved([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){auto phase=e.CurrentPoint().IsInContact()?2u:0u;self->Pointer(e,phase);self->CanvasTouch(e,phase);}
        });
        inputSource.PointerReleased([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){self->Pointer(e,3);self->CanvasTouch(e,3);}
        });
        inputSource.PointerCaptureLost([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){self->Pointer(e,4);self->CanvasTouch(e,4);}
        });
        inputSource.PointerRoutedAway([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){self->Pointer(e,4);self->CanvasTouch(e,4);self->SendIndependent(CanvasCommand{CanvasCommandKind::Input,R"({"type":"cursor_leave"})"});}
        });
        inputSource.PointerRoutedReleased([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock()){self->Pointer(e,4);self->CanvasTouch(e,4);self->SendIndependent(CanvasCommand{CanvasCommandKind::Input,R"({"type":"cursor_leave"})"});}
        });
        inputSource.PointerWheelChanged([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock())self->Wheel(e);
        });
        inputSource.PointerExited([weak=weak_from_this()](auto&&,PointerEventArgs const& e){
            if(auto self=weak.lock();self&&!e.CurrentPoint().IsInContact()){
                auto point=e.CurrentPoint();self->PenButtons(point,4);
                if(point.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Pen){std::lock_guard lock(self->mutex);self->penSources.Forget(point.PointerId());}
                self->SendIndependent(CanvasCommand{CanvasCommandKind::Input,R"({"type":"cursor_leave"})"});
            }
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
void CanvasWindow::StartCanvasTouch(CanvasTouchGesture& gesture){
    using namespace Microsoft::UI::Input;
    gesture.recognizer=GestureRecognizer();gesture.recognizer.GestureSettings(GestureSettings::Hold|GestureSettings::Tap|GestureSettings::DoubleTap);
    gesture.recognizer.Tapped([weak=weak_from_this()](auto&&,TappedEventArgs const& e){
        auto self=weak.lock();if(!self||self->closing||!self->inputDispatcher||e.TapCount()!=2)return;
        auto at=e.Position();Size size;{std::lock_guard lock(self->mutex);size=self->desired;}
        char json[256];
        std::snprintf(json,sizeof json,R"({"type":"edge_double_press","kind":"touch","contact":"free","position":[%.3f,%.3f],"viewport":[%.3f,%.3f]})",
            at.X,at.Y,double(size.width)/size.scale,double(size.height)/size.scale);
        self->inputDispatcher.TryEnqueue([weak,text=std::string(json)]{if(auto self=weak.lock();self&&!self->closing)
            self->SendIndependent(CanvasCommand{CanvasCommandKind::Input,text});});
    });
    gesture.recognizer.Holding([weak=weak_from_this(),state=&gesture](auto&&,HoldingEventArgs const& e){
        auto self=weak.lock();if(!self||self->closing||!self->inputDispatcher||e.HoldingState()!=HoldingState::Started||!state->contact)return;
        auto id=*state->contact;auto at=e.Position();float scale;{std::lock_guard lock(self->mutex);scale=self->inputScale;}
        at.X*=scale;at.Y*=scale;auto offset=self->PickerOffset(scale);
        self->inputDispatcher.TryEnqueue([weak,id,at,offset]{
            auto self=weak.lock();if(!self||self->closing||self->holdContact!=id||self->contacts.size()!=1)return;
            char json[160];
            std::snprintf(json,sizeof json,R"({"type":"color_picker_hold","id":%u,"position":[%.3f,%.3f],"offset":%.3f})",
                id,at.X,at.Y,offset);
            self->SendIndependent(CanvasCommand{CanvasCommandKind::Input,json});
        });
    });
}
void CanvasWindow::CancelCanvasTouch(CanvasTouchGesture& gesture){
    if(gesture.recognizer)gesture.recognizer.CompleteGesture();
    gesture.contact.reset();
}
void CanvasWindow::TrackContact(uint32_t id,uint32_t phase,bool touch){
    if(phase==1){
        bool first=contacts.empty();contacts.insert(id);
        holdContact=first&&touch?std::optional(id):std::nullopt;
        if(first){OrderKeys(true);dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock();self&&!self->closing&&self->workspace)self->workspace->CanvasContact(true);});}
    }else if(phase==3||phase==4){
        if(holdContact==id)holdContact.reset();
        if(contacts.erase(id)&&contacts.empty()){
            OrderKeys(false);
            dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock();self&&!self->closing&&self->workspace)self->workspace->CanvasContact(false);});
        }
    }
}
void CanvasWindow::CanvasTouch(Microsoft::UI::Input::PointerEventArgs const& e,uint32_t phase){
    CanvasTouch(canvasTouch,e.CurrentPoint(),e.GetIntermediatePoints(),phase);
}
void CanvasWindow::CanvasTouch(CanvasTouchGesture& gesture,Microsoft::UI::Input::PointerPoint const& point,PointerPoints const& points,uint32_t phase){
    phase=CanvasPointerPhase(phase,point.Properties().IsCanceled());
    auto id=point.PointerId();bool touch=point.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch;
    if(phase==1){
        bool first=gesture.contacts.empty();gesture.contacts.insert(id);
        if(touch&&first&&!gesture.contact){gesture.contact=id;gesture.recognizer.ProcessDownEvent(point);}
        else CancelCanvasTouch(gesture);
    }else if(phase==2){
        if(gesture.contact==id)gesture.recognizer.ProcessMoveEvents(CanvasPointerHistoryCount(points,phase)?points:single_threaded_vector<Microsoft::UI::Input::PointerPoint>({point}));
    }else if(phase==3||phase==4){
        gesture.contacts.erase(id);
        if(gesture.contact==id){if(phase==3){gesture.recognizer.ProcessUpEvent(point);gesture.contact.reset();}else CancelCanvasTouch(gesture);}
    }
}
void CanvasWindow::Pointer(Microsoft::UI::Input::PointerEventArgs const& e,uint32_t phase){
    auto current=e.CurrentPoint();
    Pointer(PointerSamples(current,e.GetIntermediatePoints(),phase,true),e.KeyModifiers(),
        current.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Pen,current.Properties().IsBarrelButtonPressed(),latencyTrace.enabled?Now():0);
    e.Handled(true);
}
void CanvasWindow::XamlPointer(PointerRoutedEventArgs const& e,uint32_t phase){
    if(closing||!inputDispatcher)return;
    auto current=e.GetCurrentPoint(panel);auto id=current.PointerId();
    phase=CanvasPointerPhase(phase,current.Properties().IsCanceled());
    if(phase==1){
        if(e.Handled()||e.OriginalSource()!=panel||!panel.CapturePointer(e.Pointer()))return;
        xamlContacts.insert(id);
    }else if(phase!=0&&!xamlContacts.contains(id))return;
    else if(phase==0&&(e.Handled()||e.OriginalSource()!=panel))return;
    auto history=e.GetIntermediatePoints(panel);auto samples=PointerSamples(current,history,phase,false);
    auto modifiers=e.KeyModifiers();auto arrival=latencyTrace.enabled?Now():0;
    bool pen=current.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Pen,barrel=current.Properties().IsBarrelButtonPressed();
    inputDispatcher.TryEnqueue([weak=weak_from_this(),samples=std::move(samples),modifiers,pen,barrel,arrival]() mutable {
        if(auto self=weak.lock();self&&!self->closing)self->Pointer(std::move(samples),modifiers,pen,barrel,arrival);
    });
    CanvasTouch(xamlCanvasTouch,current,history,phase);e.Handled(true);
    if(phase==3||phase==4){xamlContacts.erase(id);panel.ReleasePointerCapture(e.Pointer());}
}
std::vector<CapyPointer> CanvasWindow::PointerSamples(Microsoft::UI::Input::PointerPoint const& current,PointerPoints const& points,uint32_t phase,bool predict){
    phase=CanvasPointerPhase(phase,current.Properties().IsCanceled());
    if(current.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Pen){
        bool describe;
        {std::lock_guard lock(mutex);describe=penSources.Describe(current.PointerId(),phase==1);}
        if(describe){
            auto device=CapyUi::penDevice(current);auto source=to_string(device.Stringify());bool changed;
            {std::lock_guard lock(mutex);changed=penSources.Update(current.PointerId(),std::move(source));}
            if(changed||phase==1)Send(to_string(CapyUi::penInput(device).Stringify()));
        }
        if(phase==4){std::lock_guard lock(mutex);penSources.Forget(current.PointerId());}
    }
    uint64_t view;float scale;{std::lock_guard lock(mutex);view=revision;scale=inputScale;}
    uint32_t deviceFlags=0;
    if(current.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Pen){
        POINTER_INFO info{};POINTER_DEVICE_INFO device{};
        if(GetPointerInfo(current.PointerId(),&info)&&GetPointerDevice(info.sourceDevice,&device)&&device.pointerDeviceType==POINTER_DEVICE_TYPE_EXTERNAL_PEN)deviceFlags=0x40;
        POINTER_PEN_INFO pen{};if(GetPointerPenInfo(current.PointerId(),&pen)&&(pen.penMask&PEN_MASK_ROTATION))deviceFlags|=0x80;
    }
    auto count=CanvasPointerHistoryCount(points,phase);
    std::vector<CapyPointer> samples;samples.reserve(std::max(1u,count));
    auto capture=[&](Microsoft::UI::Input::PointerPoint const& point,bool predicted){
        auto props=point.Properties();auto type=point.PointerDeviceType();auto pos=point.Position();
        uint32_t tool=type==Microsoft::UI::Input::PointerDeviceType::Mouse?1:type==Microsoft::UI::Input::PointerDeviceType::Touch?3:props.IsEraser()||props.IsInverted()?2:0;
        auto radians=[](float degrees){return degrees*0.017453292519943295f;};
        CapyPointer p{};p.id=point.PointerId();p.timestamp_ns=point.Timestamp()*1000;p.view_revision=view;p.x=pos.X*scale;p.y=pos.Y*scale;
        p.pressure=tool==1?(point.IsInContact()?1.f:0.f):props.Pressure();
        p.tilt_x=radians(props.XTilt());p.tilt_y=radians(props.YTilt());p.twist=radians(props.Twist());p.phase=phase;p.tool=tool;
        p.button=CanvasPointerButton(tool,props.IsMiddleButtonPressed(),props.IsRightButtonPressed(),props.IsXButton1Pressed()||props.IsXButton2Pressed());
        p.flags=deviceFlags|(predicted?1:0)|(props.IsPrimary()?2:0);
        if(PrepareCanvasPrediction(p))samples.push_back(p);
    };
    bool newestFirst=count>1&&points.GetAt(0).Timestamp()>points.GetAt(count-1).Timestamp();
    if(!count)capture(current,false);
    for(uint32_t i=0;i<count;++i)capture(points.GetAt(newestFirst?count-1-i:i),false);
    if(predict&&pointerPredictor&&phase==2){
        try{
            auto predicted=pointerPredictor.GetPredictedPoints(current);
            std::sort(predicted.begin(),predicted.end(),[](auto const& a,auto const& b){return a.Timestamp()<b.Timestamp();});
            for(auto const& point:predicted)if(point.Timestamp()>=current.Timestamp())capture(point,true);
        }catch(hresult_error const&){pointerPredictor.Close();pointerPredictor=nullptr;SendIndependent(CanvasCommand{CanvasCommandKind::Prediction,"false"});}
    }
    return samples;
}
void CanvasWindow::Pointer(std::vector<CapyPointer> samples,Windows::System::VirtualKeyModifiers modifiers,bool pen,bool barrel,uint64_t arrival){
    if(samples.empty())return;auto first=samples.front();auto id=uint32_t(first.id);
    {std::lock_guard lock(mutex);if(closing)return;}
    if(first.phase==1){
        dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock();self&&!self->closing)self->canvasFocus.Focus(FocusState::Pointer);});
        if(!SyncContactModifiers(modifiers))return;
    }
    if(pen&&!PenButtons(id,first.phase!=4&&barrel))return;
    for(auto& p:samples){p.sequence=++sequence;latencyTrace.Input(p,arrival);}
    if(samples.size()<=CanvasWorkBuffer::PointerBatch){if(!SendIndependent(std::move(samples)))return;}
    else for(size_t at=0;at<samples.size();at+=CanvasWorkBuffer::PointerBatch){
        auto end=std::min(samples.size(),at+CanvasWorkBuffer::PointerBatch);
        if(!SendIndependent(std::vector<CapyPointer>(samples.begin()+at,samples.begin()+end)))return;
    }
    TrackContact(id,first.phase,first.tool==3);
}
void CanvasWindow::Wheel(Microsoft::UI::Input::PointerEventArgs const& e){
    auto point=e.CurrentPoint();auto props=point.Properties();
    SendIndependent(ScrollSample(point.Position(),props.MouseWheelDelta(),props.IsHorizontalMouseWheel(),e.KeyModifiers()));e.Handled(true);
}
CanvasScroll CanvasWindow::ScrollSample(Point position,int32_t deltaUnits,bool horizontal,Windows::System::VirtualKeyModifiers modifiers){
    float density;Size size;
    {std::lock_guard lock(mutex);size=desired;density=inputScale;}
    using Mod=Windows::System::VirtualKeyModifiers;
    bool zoom=(modifiers&Mod::Control)!=Mod::None;
    UINT units=3;SystemParametersInfoW(horizontal?SPI_GETWHEELSCROLLCHARS:SPI_GETWHEELSCROLLLINES,0,&units,0);
    float distance=units==WHEEL_PAGESCROLL?(horizontal?float(size.width):float(size.height))/density:float(units)*16.f;
    float delta=float(deltaUnits)/WHEEL_DELTA*(zoom?100.f:distance);
    return CanvasScroll{position.X*density,position.Y*density,horizontal?delta:0.f,horizontal?0.f:-delta,density,zoom,
        !horizontal&&(modifiers&Mod::Shift)!=Mod::None};
}
bool CanvasWindow::CancelGesture(){
    CancelCanvasTouch(xamlCanvasTouch);
    if(inputDispatcher)inputDispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->CancelCanvasTouch(self->canvasTouch);});
    bool headerClosed=header&&header->CancelGesture();return (workspace&&workspace->CancelGesture())||headerClosed;
}
void CanvasWindow::Key(KeyRoutedEventArgs const& e,bool pressed) {
    using VirtualKey=Windows::System::VirtualKey;
    auto key=e.Key();
    struct Ordered{CanvasWindow& window;bool held;~Ordered(){if(held)window.ReleaseKeyHold();}} ordered{*this,pressed&&!e.KeyStatus().WasKeyDown&&
        (key==VirtualKey::Escape||((GetKeyState(VK_CONTROL)&0x8000)&&key>=VirtualKey::A&&key<=VirtualKey::Z))};
    if(closing||closed||(pressed&&CapyUi::composingKey(e)))return;
    if(pressed&&key==VirtualKey::Escape&&CancelGesture()){e.Handled(true);return;}
    auto focused=FocusManager::GetFocusedElement(root.XamlRoot());

    std::wstring name=CapyUi::KeyName(key,e.KeyStatus().ScanCode);
    if(CapyUi::DeviceKey(key)&&!heldKeys.contains(uint32_t(key))&&!(pressed&&ClaimsDeviceKey(name)))return;
    // Release the same key identity even when Shift/layout changes while held.
    auto held=heldKeys.find(uint32_t(key));
    if(held!=heldKeys.end())name=held->second;
    if(name.empty())return;
    if(pressed)heldKeys.try_emplace(uint32_t(key),name);
    else heldKeys.erase(uint32_t(key));
    bool canvas=focused&&focused==canvasFocus;
    std::optional<uint32_t> divider;
    if(auto element=focused.try_as<FrameworkElement>();element&&!canvas){
        std::wstring id=Automation::AutomationProperties::GetAutomationId(element).c_str();
        if(id.starts_with(L"divider-"))divider=uint32_t(std::stoul(id.substr(8)));
    }
    // Ordinary buttons keep application shortcuts after a click. Their navigation
    // keys, the activation keys the shared rule grants them, editors and open
    // menus retain keyboard input.
    // Releases still clear shared held state when focus moves during a gesture.
    bool button=focused&&bool(focused.try_as<Primitives::ButtonBase>());
    bool ownedKeys=false;
    if(key==VirtualKey::Space||key==VirtualKey::Enter||key==VirtualKey::Delete||((key==VirtualKey::Z||key==VirtualKey::Y)&&(GetKeyState(VK_CONTROL)&0x8000)))
        for(auto node=focused.try_as<DependencyObject>();node&&!ownedKeys;node=VisualTreeHelper::GetParent(node))
            if(auto element=node.try_as<FrameworkElement>())if(auto tag=element.Tag().try_as<Windows::Data::Json::JsonObject>())ownedKeys=CapyUi::flag(tag,L"native_keys");
    bool navigation=key==VirtualKey::Space||key==VirtualKey::Enter||
        key==VirtualKey::Left||key==VirtualKey::Right||
        key==VirtualKey::Up||key==VirtualKey::Down||key==VirtualKey::Home||key==VirtualKey::End||
        ((key==VirtualKey::Tab||key==VirtualKey::PageUp||key==VirtualKey::PageDown)&&!(GetKeyState(VK_CONTROL)&0x8000))||key==VirtualKey::F2||key==VirtualKey::F10||
        (key!=VirtualKey::Menu&&(GetKeyState(VK_MENU)&0x8000)&&!(GetKeyState(VK_CONTROL)&0x8000));
    // F11 remains a window action while a toolbar button or native field has focus.
    bool arrow=key==VirtualKey::Left||key==VirtualKey::Right||key==VirtualKey::Up||key==VirtualKey::Down;
    bool activation=key==VirtualKey::Space||key==VirtualKey::Enter;
    bool keyboardFocus=false;
    if(auto element=focused.try_as<UIElement>())keyboardFocus=element.FocusState()==FocusState::Keyboard;
    bool takes=button&&activation&&capy_control_takes_key(to_string(name).c_str(),(GetKeyState(VK_CONTROL)&0x8000)!=0,(GetKeyState(VK_MENU)&0x8000)!=0,keyboardFocus);
    bool editing=key!=VirtualKey::F11&&(ownedKeys||menuOpen.load()||(divider?navigation&&!arrow:!canvas&&(!button||takes||(navigation&&!activation))));
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
    input.Insert(L"editing",JsonValue::CreateBooleanValue(editing));
    input.Insert(L"modifiers",modifiers);
    if(divider)input.Insert(L"divider",JsonValue::CreateNumberValue(*divider));
    sentModifiers.store(((GetKeyState(VK_CONTROL)&0x8000)?1u:0u)|((GetKeyState(VK_SHIFT)&0x8000)?2u:0u)|((GetKeyState(VK_MENU)&0x8000)?4u:0u));
    auto keyStatus=e.KeyStatus();
    ForwardKeyRepeats(pressed,keyStatus.WasKeyDown,keyStatus.RepeatCount,[&](bool repeat){
        input.Insert(L"repeat",JsonValue::CreateBooleanValue(repeat));
        Send(to_string(input.Stringify()),CanvasCommandKind::Input);
    });
    if(!editing)e.Handled(true);
}
void CanvasWindow::OrderKeys(bool contact){
    if(!orderedKeys||orderingKeys==contact)return;
    if(contact){keyBalance.store(0);PostMessageW(orderedKeys,WM_APP,0,0);}
    RAWINPUTDEVICE keyboard{0x01,0x06,DWORD(contact?RIDEV_INPUTSINK:RIDEV_REMOVE),contact?orderedKeys:nullptr};
    if(RegisterRawInputDevices(&keyboard,1,sizeof keyboard))orderingKeys=contact;
}
void CanvasWindow::RawKeysHeld(){
    rawKeys.clear();
    for(uint32_t code:{VK_CONTROL,VK_SHIFT,VK_MENU,VK_ESCAPE})if(GetAsyncKeyState(int(code))<0)rawKeys.insert(code);
    for(uint32_t code='A';code<='Z';++code)if(GetAsyncKeyState(int(code))<0)rawKeys.insert(code);
}
void CanvasWindow::RawKey(RAWKEYBOARD const& raw){
    uint32_t code=raw.VKey;
    if(raw.Flags&RI_KEY_BREAK){rawKeys.erase(code);return;}
    if(!rawKeys.insert(code).second||!(code==VK_ESCAPE||(rawKeys.contains(VK_CONTROL)&&code>='A'&&code<='Z')))return;
    if(!dialogOpen.load()&&!menuOpen.load()&&GetForegroundWindow()==Handle())keyBalance.fetch_add(1);
}
void CanvasWindow::ReleaseKeyHold(){
    if(keyBalance.fetch_sub(1)==1&&inputDispatcher)inputDispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->FlushHeld();});
}
bool CanvasWindow::PenButtons(Microsoft::UI::Input::PointerPoint const& point,uint32_t phase){
    if(point.PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Pen)return true;
    return PenButtons(point.PointerId(),phase!=4&&point.Properties().IsBarrelButtonPressed());
}
bool CanvasWindow::PenButtons(uint32_t id,bool primary){
    auto held=penButtons.contains(id);
    if(held==primary)return true;
    if(primary)penButtons.insert(id);else penButtons.erase(id);
    return SendIndependent(CanvasCommand{CanvasCommandKind::Input,std::string(R"({"type":"pen_button","button":"primary","pressed":)")+(primary?"true}":"false}")});
}
bool CanvasWindow::ClaimsDeviceKey(std::wstring const& name)const{
    using namespace CapyUi;
    if(object(object(lastModel,L"preferences"),L"capture").Size())return true;
    auto stored=object(object(lastModel,L"state"),L"settings");
    for(auto hold:array(stored,L"hold_keys"))if(str(object(hold.GetObject(),L"key"),L"key")==name)return true;
    for(auto entry:object(stored,L"shortcuts"))
        if(entry.Value().ValueType()==JsonValueType::Array)
            for(auto chord:entry.Value().GetArray())if(str(chord.GetObject(),L"key")==name)return true;
    return false;
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
            canvasContacts.Block(first.id,first.phase,last.phase);
            return 0;
        }
        auto result=capy_chrome(host,first.phase==1?2:first.phase==4?0:1,
            first.phase==1?first.x:last.x,first.phase==1?first.y:last.y,true,menuOpen.load());
        if(result>=0){
            bool accepted=canvasContacts.Admit(first.id,first.phase,last.phase,(result&1)!=0);
            result=accepted?capy_pointer(host,points->data(),points->size()):0;
            if(accepted&&result==0)latencyTrace.Consume(*points);
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
        wchar_t executable[32768];auto length=GetModuleFileNameW(nullptr,executable,32768);
        check_bool(length>0&&length<32768);hstring module(executable,length);
        auto zoomIn=Microsoft::UI::Input::InputDesktopResourceCursor::CreateFromModule(module,CAPY_CURSOR_ZOOM_IN);
        auto zoomOut=Microsoft::UI::Input::InputDesktopResourceCursor::CreateFromModule(module,CAPY_CURSOR_ZOOM_OUT);
        bool prepared=capy_start_services(host,this,[](void* context) noexcept {
            auto self=static_cast<CanvasWindow*>(context);
            {std::lock_guard lock(self->mutex);self->servicesReady=true;}
            self->wake.notify_one();
        })>=0;
        if(prepared){
            CapySnapshotInfo routing{};
            if(auto snapshot=capy_snapshot(host,&routing)){
                NativeString owned(snapshot,capy_string_free);
                Publish(std::move(owned.bytes),routing);
            }
            prepared=capy_prepare_gpu(host)>=0;
        }
        if(!prepared) {std::string cause=NativeString(capy_error()).get();OutputDebugStringA(cause.c_str());Fail(to_string(CapyUi::str(bootstrap,L"canvas_init_failed"))+"\n\n"+cause);}
        else {std::lock_guard lock(mutex);resize=true;}
        bool dirty=true;
        auto refreshFrameDemand=[&]{
            auto requested=capy_needs_frame(host);
            if(requested<0)throw std::runtime_error(NativeString(capy_error()).get());
            dirty|=requested!=0;
        };
        auto answer=[&](CanvasQuery& query){
            auto request=query.kind==CanvasQueryKind::Filters?capy_filter_previews:
                query.kind==CanvasQueryKind::Papers?capy_paper_previews:
                query.kind==CanvasQueryKind::Thumbnails?capy_layer_thumbnails:
                query.kind==CanvasQueryKind::LayerMenu?capy_layer_menu:
                query.kind==CanvasQueryKind::Document?capy_document_preview:capy_workspace_query;
            PreviewPacket packet(request(host,query.json.c_str()),capy_preview_free);
            if(!packet&&!capy_device_lost(host))Fail(NativeString(capy_error()).get());
            refreshFrameDemand();
            query.reply(std::move(packet));
        };
        bool inputStarted=false;
        bool brushReady=false;
        bool probeReady=false;
        unsigned recoveryAttempts=0;
        bool repeat=false;
        std::optional<std::chrono::steady_clock::time_point> lastFrame;
        auto nextServices=std::chrono::steady_clock::now()+std::chrono::milliseconds(250);
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
            bool pollServices=false,resized=false;
            {
                std::unique_lock lock(mutex);
                wake.wait_until(lock,nextServices,[&]{return closing||resize||dirty||servicesReady||transportFailed||!work.Empty()||pendingHover.has_value()||!previewWork.Empty();});
                if(closing) break;
                if(resize) {
                    auto next=desired;resize=false;probeReady=false;
                    lock.unlock();
                    auto result=capy_resize(host,next.width,next.height,next.scale);
                    if(result<0){if(capy_device_lost(host))continue;throw std::runtime_error(NativeString(capy_error()).get());}
                    {std::lock_guard geometryLock(mutex);revision=capy_view_revision(host);inputScale=next.scale;}
                    dirty=true;resized=true;lock.lock();
                    if(closing)break;
                }
                pollServices=std::exchange(servicesReady,false)||std::chrono::steady_clock::now()>=nextServices;
            }
            if(pollServices){
                auto serviceResult=capy_poll_services(host);
                if(serviceResult<0){if(capy_device_lost(host))continue;Fail(NativeString(capy_error()).get());break;}
                nextServices=std::chrono::steady_clock::now()+std::chrono::milliseconds(250);
                dirty|=serviceResult!=0;
                TracePresentation(false,dirty);
                if(!dirty&&presentationTrace.Withheld(std::chrono::steady_clock::now())){presentationTrace.Repeat();repeat=dirty=true;}
                CapySnapshotInfo routing{};
                if(auto snapshot=capy_snapshot(host,&routing)){
                    NativeString owned(snapshot,capy_string_free);
                    Publish(std::move(owned.bytes),routing);
                }
            }
            std::optional<CanvasQuery> idleQuery;
            {std::lock_guard lock(mutex);
                auto step=NextCanvasRenderStep(dirty||transportFailed||!work.Empty()||pendingHover.has_value()||recoveryAttempts,previewWork);
                if(step==CanvasRenderStep::Wait)continue;
                if(step==CanvasRenderStep::Answer)idleQuery=previewWork.Take();
            }
            if(idleQuery){answer(*idleQuery);continue;}
            if(lastFrame){
                std::unique_lock lock(mutex);
                auto input=[&]{return closing||resize||transportFailed||!work.Empty()||pendingHover.has_value();};
                if(!input())wake.wait_until(lock,*lastFrame+RefreshPeriod(nativeWindow),input);
                if(closing)break;
                if(resize&&!resized)continue;
            }
            lastFrame=std::chrono::steady_clock::now();
            // DXGI waits before draining input so a frame uses the freshest arrived samples.
            auto acquireStart=latencyTrace.enabled?Now():0;
            auto acquired=capy_acquire(host);
            auto acquiredAt=latencyTrace.enabled?Now():0;
            if(acquired<0) {if(capy_device_lost(host))continue;Fail(NativeString(capy_error()).get());break;}
            if(acquired==2) {std::lock_guard lock(mutex);resize=true;continue;}
            if(acquired==0) {
                std::unique_lock lock(mutex);
                wake.wait_for(lock,std::chrono::milliseconds(16),[&]{return closing||resize;});
                continue;
            }
            std::deque<CanvasWork> pending;
            bool overflow;std::optional<bool> hover;
            // Samples admitted during reconstruction must wait for replacement
            // brushes; draining sooner would classify their presses as unready.
            {std::lock_guard lock(mutex);pending=recoveryAttempts?work.TakeBeforePointers():work.Take();overflow=transportFailed;hover=std::exchange(pendingHover,std::nullopt);}
            space.notify_all();
            bool failed=false;
            if(hover&&capy_chrome(host,*hover?0:1,0,0,false,menuOpen.load())<0){Fail(NativeString(capy_error()).get());break;}
            for(auto& item:pending) {
                auto result=DispatchWork(item,false);
                if(result>0)Fail(NativeString(capy_error()).get()); // A rejected UI action leaves the canvas running.
                if(result<0) {Fail(NativeString(capy_error()).get());failed=true;break;}
            }
            if(failed) break;
            if((!pending.empty()||hover)&&capy_chrome(host,0,0,0,false,menuOpen.load())<0){Fail(NativeString(capy_error()).get());break;}
            if(overflow){sentModifiers.store(0);if(capy_input(host,R"({"type":"blur"})")<0){Fail(NativeString(capy_error()).get());break;}}
            if(capy_device_lost(host))continue;
            auto now=Now();
            auto result=capy_frame(host,now,now);
            if(latencyTrace.enabled){
                auto end=Now();uint64_t stats[6]{};auto error=capy_presentation_stats(host,stats);
                latencyTrace.Frame(acquireStart,acquiredAt,now,end,stats,error);
            }
            if(result<0){if(capy_device_lost(host))continue;Fail(NativeString(capy_error()).get());break;}
            dirty=result!=0;
            TracePresentation(true,dirty,std::exchange(repeat,false)&&pending.empty()&&!hover);
            if(!inputStarted){
                inputStarted=true;inputDispatcher.TryEnqueue([weak=weak_from_this(),zoomIn,zoomOut]{
                    if(auto self=weak.lock()){self->zoomInCursor=zoomIn;self->zoomOutCursor=zoomOut;self->StartInput();}
                });
            }
            {std::lock_guard lock(mutex);revision=capy_view_revision(host);}
            CapySnapshotInfo routing{};
            if(auto snapshot=capy_snapshot(host,&routing)) {
                NativeString owned(snapshot,capy_string_free);
                if(routing.brush_ready.length) {
                    bool ready=Windows::Data::Json::JsonValue::Parse(to_hstring(std::string(owned.get()+routing.brush_ready.offset,routing.brush_ready.length))).GetBoolean();
                    brushReady=ready;
                    if(ready)recoveryAttempts=0;
                }
                Publish(std::move(owned.bytes),routing);
            }
            if(latencyTrace.enabled&&!probeReady&&result==0&&brushReady) {
                auto info=capy_surface_info(host);
                if(!info){Fail(NativeString(capy_error()).get());break;}
                NativeString owned(info,capy_string_free);
                auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(owned.get()));
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
            if(preview)answer(*preview);
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
        if(result!=0)Fail(NativeString(capy_error()).get());
        if(result<0)break;
    }
    capy_suspend(host);
    CapyLifecycle("services_finishing");
    if(capy_finish_services(host)<0)Fail(NativeString(capy_error()).get());
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
    if(capy_suspend(host)<0)throw std::runtime_error(NativeString(capy_error()).get());
    if(capy_reset_surface(host)<0)throw std::runtime_error(NativeString(capy_error()).get());
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
        if(std::chrono::steady_clock::now()>=deadline)throw std::runtime_error(NativeString(capy_error()).get());
        std::unique_lock lock(mutex);
        if(wake.wait_for(lock,std::chrono::milliseconds(250),[&]{return closing;}))return false;
    }
    {std::lock_guard lock(mutex);resize=true;}
    CapyLifecycle("gpu_recovery_prepared");
    return true;
}
void CanvasWindow::SaveAfterGpuFailure(std::string const& reason) {
    std::deque<CanvasWork> admitted;
    {std::lock_guard lock(mutex);inputStopped=true;admitted=work.Take();pendingHover.reset();previewWork.Clear();}
    space.notify_all();
    capy_suspend(host);
    if(capy_suspend_renderer(host)<0)throw std::runtime_error(NativeString(capy_error()).get());
    // Raster recovery retains completed edits; queued contacts cannot be drawn
    // without a renderer. Drain accepted commands after closing canvas input.
    for(auto& item:admitted){
        auto result=DispatchWork(item,true);
        if(result<0)throw std::runtime_error(NativeString(capy_error()).get());
        if(result>0)Fail(NativeString(capy_error()).get());
    }
    OutputDebugStringA(reason.c_str());
    Fail(to_string(CapyUi::str(bootstrap,L"painting_unavailable_save")));
    CapyLifecycle("gpu_recovery_save_available");
    for(;;){
        std::deque<CanvasWork> pending;std::optional<CanvasQuery> query;
        {
            std::unique_lock lock(mutex);
            wake.wait_for(lock,std::chrono::milliseconds(250),[&]{return closing||resize||servicesReady||!work.Empty()||!previewWork.Empty();});
            if(closing)return;
            servicesReady=false;
            if(resize){
                auto next=desired;resize=false;lock.unlock();
                if(capy_resize(host,next.width,next.height,next.scale)<0)throw std::runtime_error(NativeString(capy_error()).get());
                lock.lock();revision=capy_view_revision(host);inputScale=next.scale;
            }
            pending=work.Take();query=previewWork.Take();
        }
        space.notify_all();
        for(auto& item:pending)if(auto command=std::get_if<CanvasCommand>(&item)){
            auto result=DispatchCanvasCommand(host,*command);
            if(result!=0)Fail(NativeString(capy_error()).get());
        }
        if(capy_poll_services(host)<0)throw std::runtime_error(NativeString(capy_error()).get());
        CapySnapshotInfo routing{};
        if(auto snapshot=capy_snapshot(host,&routing)){
            NativeString owned(snapshot,capy_string_free);
            Publish(std::move(owned.bytes),routing);
        }
        if(query){
            PreviewPacket packet(query->kind==CanvasQueryKind::Workspace?capy_workspace_query(host,query->json.c_str()):0,capy_preview_free);
            query->reply(std::move(packet));
        }
    }
}
void CanvasWindow::ResetSurface() {
    {std::lock_guard lock(mutex);if(closing)return;}
    auto native=panel.as<ISwapChainPanelNative>();
    root.Background(SolidColorBrush(CapyUi::color(CapyUi::str(CapyUi::object(CapyUi::object(lastModel,L"state"),L"palette"),L"bg",L"#323232"))));
    auto detached=native->SetSwapChain(nullptr);
    surfaceState.Reset(capy_surface_generation(host));surfaceHandle=capy_surface_handle(host);
    auto attached=SUCCEEDED(detached)&&surfaceHandle?S_OK:FAILED(detached)?detached:E_FAIL;
    if(FAILED(attached)){
        std::lock_guard lock(mutex);
        surfaceError="Could not attach the recovered GPU surface";
    }
    {std::lock_guard lock(mutex);paused=false;}
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
    CancelGesture();
    // Commit the focused workspace draft before shared close policy checks dirty state.
    canvasFocus.Focus(FocusState::Programmatic);
    if(settings)settings->CommitEdits();
    Send(R"({"type":"close_settings"})");
    if(workspaceDialogs)workspaceDialogs->CancelAll();
    if(workspace)workspace->CancelPreviews();
    for(auto const& dialog:{&canvasSize,&imageSize})if(*dialog)(*dialog)->CancelAll();
    if(workspaceManager)workspaceManager->CancelAll();
    CapyLifecycle("close_requested");
    Send(R"({"operation":"close"})",CanvasCommandKind::Document);
}
void CanvasWindow::Stop() {
    {std::lock_guard lock(mutex);if(closing)return;}
    if(settings)settings->CommitEdits();
    {std::lock_guard lock(mutex);closing=true;}
    {std::lock_guard lock(snapshots->mutex);snapshots->Close();}
    CancelCanvasTouch(xamlCanvasTouch);xamlCanvasTouch.recognizer=nullptr;xamlCanvasTouch.contacts.clear();xamlContacts.clear();
    CapyLifecycle("close_authorized");
    if(settings)settings->Hide();
    if(documents)documents->Hide();
    if(workspaceDialogs)workspaceDialogs->Hide();
    for(auto const& dialog:{&canvasSize,&imageSize})if(*dialog)(*dialog)->Hide();
    if(workspaceStorage)workspaceStorage->Hide();
    if(workspaceManager)workspaceManager->Hide();
    wake.notify_all();space.notify_all();
    if(inputController) {
        inputDispatcher.TryEnqueue([weak=weak_from_this()]{
            if(auto self=weak.lock()) {
                self->CancelCanvasTouch(self->canvasTouch);self->canvasTouch.recognizer=nullptr;self->canvasTouch.contacts.clear();self->holdContact.reset();self->contacts.clear();
                if(self->pointerPredictor){self->pointerPredictor.Close();self->pointerPredictor=nullptr;}
                self->OrderKeys(false);
                if(self->orderedKeys){PostMessageW(self->orderedKeys,WM_CLOSE,0,0);self->orderedKeys=nullptr;}
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
    if(ModalOpen())return;
    auto lifetime=shared_from_this(); // App may release its last reference below.
    finishing=true;
    CapyLifecycle("join_renderer");
    if(renderer.joinable()) renderer.join();
    CapyLifecycle("detach_surface");
    panel.as<ISwapChainPanelNative>()->SetSwapChain(nullptr);
    auto retired=host;host=0;
    renderer=std::jthread([lifetime,retired]{
        init_apartment(apartment_type::multi_threaded);
        CapyLifecycle("destroy_host");
        capy_destroy(retired);
        CapyLifecycle("host_destroyed");
        uninit_apartment();
        lifetime->dispatcher.TryEnqueue([lifetime]{lifetime->CloseViews();});
    });
}
void CanvasWindow::CloseViews() {
    if(renderer.joinable())renderer.join();
    // XAML controls and their retained bindings must be released while this
    // window still owns a live XAML context, not later from App destruction.
    settings.reset();documents.reset();workspaceDialogs.reset();canvasSize.reset();imageSize.reset();workspaceStorage.reset();workspaceManager.reset();header.reset();workspace.reset();
    textFocus.revoke();localization.reset();
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

void CanvasWindow::UpdateNavigationCursor() {
    if(!inputSource)return;
    using namespace Microsoft::UI::Input;
    InputCursor next{nullptr};
    auto mode=navigationCursor.load();
    switch(mode){
    case NavigationCursor::Pan:next=InputSystemCursor::Create(InputSystemCursorShape::SizeAll);break;
    case NavigationCursor::Zoom:next=zoomInCursor;break;
    case NavigationCursor::ZoomOut:next=zoomOutCursor;break;
    case NavigationCursor::Rotate:next=InputSystemCursor::Create(InputSystemCursorShape::Cross);break;
    default:break;
    }
    inputSource.Cursor(next);
    dispatcher.TryEnqueue([weak=weak_from_this(),mode,next]{
        auto self=weak.lock();
        if(!self||self->closing||self->closed||self->navigationCursor.load()!=mode)return;
        self->panel.as<IUIElementProtected>().ProtectedCursor(next);
        if(self->menuOpen.load()||self->dialogOpen.load())return;
        POINT point{};
        if(!GetCursorPos(&point)||GetAncestor(WindowFromPoint(point),GA_ROOT)!=self->Handle())return;
        if(auto capture=GetCapture();capture&&capture!=self->Handle())return;
        auto xaml=self->root.XamlRoot();if(!xaml)return;
        auto coordinates=xaml.CoordinateConverter();if(!coordinates)return;
        auto hits=VisualTreeHelper::FindElementsInHostCoordinates(coordinates.ConvertScreenToLocal(Windows::Graphics::PointInt32{point.x,point.y}),self->root,false).First();
        if(!hits.HasCurrent())return;
        DependencyObject hovered=hits.Current();
        while(hovered&&hovered!=self->canvasFocus)hovered=VisualTreeHelper::GetParent(hovered);
        if(hovered)if(auto source=Microsoft::UI::Input::InputPointerSource::GetForIsland(xaml.ContentIsland()))source.Cursor(next);
    });
}
void CanvasWindow::Publish(std::string snapshot,CapySnapshotInfo const& info) {
    bool full=info.full!=0;
    auto value=[&](CapySnapshotRange range){return snapshot.substr(range.offset,range.length);};
    std::optional<std::string> camera;
    if(info.camera.length)camera="{\"camera\":"+value(info.camera)+"}";
    auto localizationBytes=value(info.localization);
    auto glass=info.glass.length?"{\"windows_glass\":"+value(info.glass)+"}":"";
    // Explicit local test evidence. This can include user state and is never
    // enabled by ordinary launches.
    if(GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)){
        auto model=Windows::Data::Json::JsonObject::Parse(to_hstring(snapshot));
        auto identity="{\"process_id\":"+std::to_string(GetCurrentProcessId())+
            ",\"window_id\":"+std::to_string(windowId)+",\"presented\":"+std::to_string(presentationTrace.required)+
            ",\"presentation_epoch\":"+std::to_string(presentationTrace.epoch);
        if(full)tracedModel=model;
        else if(info.layout&&tracedModel.HasKey(L"state"))WorkspaceView::MergeLayout(tracedModel,model);
        bool glassChanged=info.glass.length&&CapyUi::object(tracedModel,L"windows_glass").Stringify()!=CapyUi::object(model,L"windows_glass").Stringify();
        if(glassChanged)tracedModel.Insert(L"windows_glass",model.GetNamedValue(L"windows_glass"));
        if(full||info.layout||glassChanged)TraceState("ui-state",identity+",\"model\":"+to_string(tracedModel.Stringify())+"}");
        if(full||model.HasKey(L"command_search")){
            auto search=full?CapyUi::object(model,L"state").GetNamedValue(L"command_search",Windows::Data::Json::JsonValue::CreateNullValue()):model.GetNamedValue(L"command_search");
            TraceState("search-state",identity+",\"search\":"+to_string(search.Stringify())+"}");
        }
        auto view=full?CapyUi::object(CapyUi::object(model,L"state"),L"camera"):CapyUi::object(model,L"camera");
        if(view.Size())TraceState("camera-state",identity+",\"camera\":"+to_string(view.Stringify())+"}");
    }
    if(full){
        auto cursorModel=info.navigation_cursor.length?Windows::Data::Json::JsonObject::Parse(to_hstring("{\"navigation_cursor\":"+value(info.navigation_cursor)+"}")):Windows::Data::Json::JsonObject{};
        auto mode=CapyUi::str(cursorModel,L"navigation_cursor");
        auto cursor=mode==L"pan"?NavigationCursor::Pan:mode==L"zoom"?NavigationCursor::Zoom:
            mode==L"zoom_out"?NavigationCursor::ZoomOut:mode==L"rotate"?NavigationCursor::Rotate:NavigationCursor::None;
        if(navigationCursor.exchange(cursor)!=cursor&&inputDispatcher)inputDispatcher.TryEnqueue([weak=weak_from_this()]{
            if(auto self=weak.lock())self->UpdateNavigationCursor();
        });
    }
    {
        std::lock_guard lock(snapshots->mutex);
        snapshots->pending.Push(std::move(snapshot),full,info.workspace!=0,std::move(camera),info.search!=0,std::move(localizationBytes),info.layout!=0,std::move(glass));
    }
    QueueSnapshots();
}
void CanvasWindow::QueueSnapshots() {
    std::optional<CanvasSnapshotMailbox::Batch> batch;
    {
        std::lock_guard lock(snapshots->mutex);batch=snapshots->pending.BeginDecode();
    }
    if(batch)DecodeSnapshots(snapshots,dispatcher,weak_from_this(),std::move(*batch));
}
winrt::fire_and_forget CanvasWindow::DecodeSnapshots(std::shared_ptr<SnapshotState> state,
    Microsoft::UI::Dispatching::DispatcherQueue queue,std::weak_ptr<CanvasWindow> owner,CanvasSnapshotMailbox::Batch raw) {
    auto sequence=raw.sequence;
    PreparedSnapshots batch;
    std::string failure;
    try {
        co_await winrt::resume_background();
        auto parse=[](std::string const& bytes,bool patch=false){
            if(bytes.empty())return Windows::Data::Json::JsonObject{nullptr};
            auto value=Windows::Data::Json::JsonObject::Parse(to_hstring(bytes));
            if(patch&&value.HasKey(L"windows_glass"))value.Remove(L"windows_glass");
            return value;
        };
        batch={parse(raw.full),parse(raw.layout,true),parse(raw.workspace,true),parse(raw.camera,true),parse(raw.search,true),parse(raw.localization),parse(raw.glass)};
    }catch(hresult_error const& error){failure=to_string(error.message());}
     catch(std::exception const& error){failure=error.what();}
    raw={};
    bool failed=!failure.empty();
    if(failed){std::lock_guard lock(state->mutex);state->Close();}
    bool posted=false;
    try{posted=queue.TryEnqueue([state,owner,batch=std::move(batch),failure=std::move(failure)]()mutable{
        {
            std::lock_guard lock(state->mutex);
            state->pending.CompleteDecode();
            if(state->pending.Closed()&&failure.empty())return;
        }
        auto self=owner.lock();if(!self||self->closing)return;
        if(!failure.empty()){
            self->FailPresentation(std::move(failure));return;
        }
        self->QueueSnapshots();self->ApplyPending(std::move(batch));
    });}catch(hresult_error const& error){OutputDebugStringW(error.message().c_str());}
    if(!posted){std::lock_guard lock(state->mutex);state->Close();co_return;}
    if(failed)co_return;
    std::deque<std::function<void()>> replies;
    {
        std::lock_guard lock(state->mutex);
        state->pending.Posted(sequence);
        while(!state->replies.empty()&&state->pending.IsPosted(state->replies.front().first)){
            replies.push_back(std::move(state->replies.front().second));state->replies.pop_front();
        }
    }
    try{for(auto& reply:replies)reply();}
    catch(hresult_error const& error){failure=to_string(error.message());}
    catch(std::exception const& error){failure=error.what();}
    if(!failure.empty())queue.TryEnqueue([owner,failure=std::move(failure)]{
        if(auto self=owner.lock())self->FailPresentation(failure);
    });
}
void CanvasWindow::FailPresentation(std::string message) {
    {std::lock_guard lock(snapshots->mutex);snapshots->Close();}
    {std::lock_guard lock(mutex);transportFailed=true;}
    wake.notify_all();space.notify_all();Fail(std::move(message));
}
void CanvasWindow::ApplyPending(PreparedSnapshots batch) {
    if(closing)return;
    try {
        if(batch.full){
            if(batch.localization)batch.full.Insert(L"localization",batch.localization);
            ApplyModel(batch.full);
        }
        for(auto const& patch:{batch.layout,batch.workspace,batch.camera,batch.search,batch.glass})
            if(!closing&&patch)workspace->Apply(patch);
    } catch(hresult_error const& error) {
        char code[16];std::snprintf(code,sizeof code," [0x%08X]",uint32_t(error.code().value));
        Fail(to_string(error.message())+code);
    }
}

void CanvasWindow::ApplyDialogs() {
    if(closing){
        // Let the coroutine release its final dialog reference before tearing
        // down XAML. Completion callbacks may run before ShowAsync unwinds.
        dispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock())self->Finish();});
        return;
    }
    if(applyingDialogs||!settings||!documents||!workspaceDialogs||!canvasSize||!imageSize||!workspaceStorage||!workspaceManager||!lastModel.Size())return;
    applyingDialogs=true;
    struct Reset{bool& flag;~Reset(){flag=false;}}reset{applyingDialogs};
    if(settings->IsOpen()||!ModalOpen())settings->Apply(lastModel);
    workspaceDialogs->Apply(lastModel,!workspaceDialogs->IsOpen()&&ModalOpen());
    for(auto const& dialog:{&canvasSize,&imageSize})(*dialog)->Apply(lastModel,!(*dialog)->IsOpen()&&ModalOpen());
    documents->Apply(lastModel,!documents->IsOpen()&&ModalOpen());
    workspaceStorage->Apply(lastModel,bootstrap,!workspaceStorage->IsOpen()&&ModalOpen());
    workspaceManager->Apply(lastModel,!workspaceManager->IsOpen()&&ModalOpen());
    UpdatePopup();
}
bool CanvasWindow::ModalOpen()const {
    return (settings&&settings->IsOpen())||(documents&&documents->IsOpen())||(workspaceDialogs&&workspaceDialogs->IsOpen())||
        (canvasSize&&canvasSize->IsOpen())||(imageSize&&imageSize->IsOpen())||
        (workspaceStorage&&workspaceStorage->IsOpen())||(workspaceManager&&workspaceManager->IsOpen());
}
void CanvasWindow::Popup(bool open) {
    headerPopupOpen=open;UpdatePopup();
}
void CanvasWindow::UpdatePopup() {
    if(closing||closed)return;
    auto storage=CapyUi::object(lastModel,L"windows_workspace");
    bool unavailable=storage.Size()&&(!CapyUi::flag(storage,L"ready")||CapyUi::flag(storage,L"busy")||CapyUi::flag(storage,L"owner_lost")||CapyUi::flag(storage,L"closing"));
    unavailable|=CapyUi::flag(CapyUi::object(lastModel,L"windows_settings_close"),L"requested");
    bool blocked=unavailable||ModalOpen();
    bool retire=dialogOpen.exchange(blocked)!=blocked&&blocked;
    if(retire){
        if(header)header->CancelNativeContacts();
        if(workspace)workspace->CancelNativeContacts();
    }
    canvasFocus.IsEnabled(!blocked);
    if(workspace)workspace->Root().IsHitTestVisible(!unavailable);
    if(header){header->Root().IsHitTestVisible(!unavailable);header->SetBlocked(blocked);}
    if(retire){
        heldKeys.clear();sentModifiers.store(0);Send(R"({"type":"blur"})",CanvasCommandKind::Input);
    }
    bool open=headerPopupOpen||workspacePopupOpen||screenPopupOpen||blocked;
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
    auto source=e.OriginalSource().try_as<DependencyObject>();
    while(source&&source!=panel&&source!=root)source=VisualTreeHelper::GetParent(source);
    if(!point.IsInContact()&&(leave||source!=panel)&&inputDispatcher){
        inputDispatcher.TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock();self&&!self->closing)self->SendIndependent(CanvasCommand{CanvasCommandKind::Input,R"({"type":"cursor_leave"})"});});
    }
    {
        std::lock_guard lock(mutex);
        if(closing||!host||rendererDone.load())return;
        pendingHover=leave;
    }
    wake.notify_one();
}
void CanvasWindow::Fullscreen() {
    if(closing||closed)return;
    auto app=window.AppWindow();
    bool entering=app.Presenter().Kind()!=AppWindowPresenterKind::FullScreen;
    BOOL disabled=TRUE;auto result=DwmSetWindowAttribute(Handle(),DWMWA_TRANSITIONS_FORCEDISABLED,&disabled,sizeof(disabled));
    TraceState("fullscreen-transition",std::string("{\"fullscreen\":")+(entering?"true":"false")+
        ",\"disable_hresult\":"+std::to_string(result)+"}");
    auto restore=[weak=weak_from_this()]{if(auto self=weak.lock();self&&!self->closing&&!self->closed&&
        self->window.AppWindow().Presenter().Kind()!=AppWindowPresenterKind::FullScreen){
        BOOL disabled=FALSE;auto result=DwmSetWindowAttribute(self->Handle(),DWMWA_TRANSITIONS_FORCEDISABLED,&disabled,sizeof(disabled));
        self->TraceState("fullscreen-restored","{\"enable_hresult\":"+std::to_string(result)+"}");
    }};
    try{
        app.SetPresenter(entering?AppWindowPresenterKind::FullScreen:AppWindowPresenterKind::Overlapped);
        Resize();
    }catch(...){restore();throw;}
    if(!entering&&!dispatcher.TryEnqueue(Microsoft::UI::Dispatching::DispatcherQueuePriority::Low,restore))restore();
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
    if(!closing&&!closed)Send(R"({"operation":"input","input":{"type":"refresh_switcher"}})",CanvasCommandKind::Workspace);
}
void CanvasWindow::ApplyModel(Windows::Data::Json::JsonObject const& model) {
    using namespace CapyUi;
    auto presentation=object(model,L"localization");
    if(presentation.Size()){
        bootstrap=object(presentation,L"bootstrap");auto tag=to_string(str(bootstrap,L"active_tag"));
        localization=NativeLocalization(capy_localization_for_tag(tag.c_str()),capy_localization_free);
        root.Language(str(bootstrap,L"active_tag"));
        AutomationProperties::SetName(canvasFocus,str(bootstrap,L"drawing_canvas"));
        AutomationProperties::SetHelpText(canvasFocus,str(bootstrap,L"drawing_canvas_help"));
        AutomationProperties::SetName(root,str(bootstrap,L"drawing_workspace"));
    }
    auto focus=settings&&settings->IsOpen()?FocusManager::GetFocusedElement(root.XamlRoot()).try_as<Control>():nullptr;
    if(focus)if(auto owner=ItemsControl::ItemsControlFromItemContainer(focus))focus=owner;
    if(!workspace->Apply(model))return;
    lastModel=model;
    auto suspended=flag(model,L"windows_rendering_suspended");
    auto workspaceReady=flag(object(model,L"state"),L"workspace_ready");
    workspace->Root().Visibility(suspended||!workspaceReady?Visibility::Collapsed:Visibility::Visible);
    panel.Visibility(suspended?Visibility::Collapsed:Visibility::Visible);
    auto state=object(model,L"state");auto theme=str(state,L"theme",L"dark");
    {std::lock_guard lock(mutex);penSources.Select(to_string(object(model,L"windows_tabs").GetNamedValue(L"selected",JsonValue::CreateNullValue()).Stringify()+L"/"+object(state,L"document_file").GetNamedValue(L"epoch",JsonValue::CreateNullValue()).Stringify()));}
    if(surfaceState.CanAttach(uint64_t(num(model,L"windows_gpu_generation")),flag(model,L"windows_surface_ready"))){
        check_hresult(panel.as<ISwapChainPanelNative2>()->SetSwapChainHandle(surfaceHandle));surfaceState.DidAttach();
    }
    if(suspended||!surfaceState.Attached())root.Background(SolidColorBrush(color(str(object(state,L"palette"),L"bg",L"#323232"))));
    else root.Background(nullptr);
    status.VerticalAlignment(suspended?VerticalAlignment::Center:VerticalAlignment::Bottom);
    status.TextAlignment(suspended?TextAlignment::Center:TextAlignment::Left);
    status.TextWrapping(suspended?TextWrapping::Wrap:TextWrapping::NoWrap);
    status.Margin(suspended?Thickness{24,24,24,24}:Thickness{0,0,0,40});
    SendLaunchFiles(model);
    if(auto sessionWindows=uint32_t(num(object(model,L"windows_recovery"),L"windows"));sessionWindows&&!sessionWindowsOpened&&createWindow){
        sessionWindowsOpened=true;
        window.DispatcherQueue().TryEnqueue([weak=weak_from_this(),sessionWindows]{
            if(auto self=weak.lock();self&&!self->closing)for(uint32_t index=0;index<sessionWindows;++index)self->createWindow();
        });
    }
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
    if(flag(object(model,L"windows_tabs"),L"window_ready")&&flag(object(state,L"document_file"),L"close_ready")&&(!storage.Size()||flag(storage,L"closed"))
        &&(!preferencesClose.Size()||flag(preferencesClose,L"ready"))
        &&(!object(model,L"windows_recovery").Size()||flag(object(model,L"windows_recovery"),L"ready"))){Stop();return;}
    if(!statusFailed){
        auto message=str(model,L"error");
        if(message.empty())message=str(state,L"host_error");
        if(message.empty())message=str(object(model,L"windows_recovery"),L"error");
        if(message.empty()&&str(object(model,L"windows_document"),L"type")==L"workflow_busy")message=str(bootstrap,L"preparing_document");
        if(message.empty())message=str(object(model,L"windows_filter_load"),L"error");
        if(message.empty()&&!flag(model,L"brush_ready"))message=str(bootstrap,L"preparing_brush");
        if(message.empty()&&flag(object(model,L"windows_filter_load"),L"pending"))message=str(bootstrap,L"loading_filters");
        if(message.empty())message=str(object(model,L"windows_proof"),L"text");
        status.Text(message);status.Visibility(message.empty()?Visibility::Collapsed:Visibility::Visible);
    }
    ApplyScreen(model);
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
    if(tabs.Size())window.Title((flag(object(state,L"document_file"),L"modified")?hstring(L"• "):hstring())+str(tabs.GetObjectAt(0),L"title")+L" · "+str(bootstrap,L"app_name"));
    header->Apply(model);ApplyDialogs();
    header->Root().Visibility(workspaceReady?Visibility::Visible:Visibility::Collapsed);
    // Workspace theme replacement must not take focus from retained Preferences.
    if(retheme&&focus)dispatcher.TryEnqueue(Microsoft::UI::Dispatching::DispatcherQueuePriority::Low,
        [weak=weak_from_this(),target=make_weak(focus)]{
            if(auto self=weak.lock();self&&!self->closing&&self->settings->IsOpen())
                if(auto control=target.get();control&&control.IsLoaded())control.Focus(FocusState::Programmatic);
        });
}

void CanvasWindow::ApplyScreen(Windows::Data::Json::JsonObject const& model){
    using namespace CapyUi;
    auto state=object(model,L"state"),screen=object(state,L"screen"),chip=object(screen,L"chip"),details=object(screen,L"details");
    auto catalog=object(object(model,L"localization"),L"catalog");auto updated=object(object(catalog,L"native_copy"),L"color");if(updated.Size())screenCopy=updated;if(catalog.Size())screenFontSize=num(catalog,L"text_size_pt",11)*4/3;
    auto key=screen.Stringify()+screenCopy.Stringify()+str(state,L"theme")+to_hstring(screenFontSize);if(screenKey==key)return;screenKey=key;
    auto colorCopy=screenCopy;screenChip.FontSize(screenFontSize);screenClipped.FontSize(screenFontSize);
    for(auto text:{screenTitle,screenHeadline,screenBody})text.FontSize(screenFontSize);
    auto foreground=SolidColorBrush(color(str(object(state,L"palette"),L"text",L"#fafafb")));
    auto warning=SolidColorBrush(color(str(state,L"theme")==L"dark"?L"#e5a50a":L"#9c5700"));
    screenChip.Visibility(chip.Size()?Visibility::Visible:Visibility::Collapsed);
    if(!chip.Size()||!details.Size()){if(screenPopupOpen)screenPopup.Hide();return;}
    StackPanel face;face.Orientation(Orientation::Horizontal);face.Spacing(6);
    if(flag(chip,L"warning")){auto mark=icon(L"warning",str(state,L"theme"));face.Children().Append(mark);}
    TextBlock label;label.Text(str(chip,L"label"));face.Children().Append(label);screenChip.Content(face);
    auto caption=str(colorCopy,L"screen_details");AutomationProperties::SetName(screenChip,caption);ToolTipService::SetToolTip(screenChip,box_value(caption));
    screenTitle.Text(str(details,L"title"));screenHeadline.Text(str(details,L"headline"));screenHeadline.Foreground(flag(details,L"warning")?warning:foreground);
    screenBody.Text(str(details,L"body"));screenBody.Visibility(screenBody.Text().empty()?Visibility::Collapsed:Visibility::Visible);
    auto clipped=details.GetNamedValue(L"show_clipped",JsonValue::CreateNullValue());updatingScreen=true;
    screenClipped.Visibility(clipped.ValueType()==JsonValueType::Boolean?Visibility::Visible:Visibility::Collapsed);screenClipped.IsChecked(clipped.ValueType()==JsonValueType::Boolean&&clipped.GetBoolean());
    screenClipped.Content(box_value(str(colorCopy,L"highlight_clipped_colors")));updatingScreen=false;
}
