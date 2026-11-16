#pragma once
#include "pch.h"
#include "CanvasModelUpdate.h"
#include "NativeJson.h"
#include <winrt/Windows.ApplicationModel.DataTransfer.h>
#include "WorkspaceView.h"
#include "HeaderView.h"
#include "SettingsView.h"
#include "DocumentView.h"
#include "WorkspaceDialogs.h"
#include "SizeDialog.h"
#include "WorkspaceStorageView.h"
#include "WorkspaceManagerView.h"
#include "CanvasWorkBuffer.h"
#include "CanvasSurfaceState.h"
#include <winrt/Microsoft.UI.Xaml.Media.Animation.h>
#include "CanvasContactGate.h"
#include "CanvasLatencyTrace.h"
#include "CanvasSnapshotMailbox.h"
#include "FilterPreviews.h"
#include "GamepadInput.h"
#include "NativeHandles.h"
#include "PenSourceCache.h"
#include <winrt/Windows.Devices.Haptics.h>
#include "PointerModality.h"
#include <atomic>
#include <condition_variable>
#include <deque>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <thread>
#include <variant>
#include <vector>
#include <unordered_map>
#include <unordered_set>
#include <optional>

// Opt-in, local lifecycle diagnostics; never used on the presentation hot path.
void CapyLifecycle(char const* event);

namespace CapyUi {struct WorkspaceData;}
struct OrderedTraceFile;

class CanvasWindow : public std::enable_shared_from_this<CanvasWindow> {
public:
    CanvasWindow(std::function<void()> createWindow,std::function<void(uint64_t)> onClosed,
        std::function<void(uint64_t)> preferencesChanged,std::function<void()> testDeviceLoss,std::function<size_t(uint64_t)> otherWindows);
    void TestDeviceLoss(std::string group){Send(std::move(group),CanvasCommandKind::DeviceLoss);}
    void RefreshWorkspaceSwitcher();
    uint64_t Id()const{return windowId;}
    HWND Handle()const;
    void Open();
    void OpenFiles(std::vector<std::wstring> paths);
    void Present();
    bool Closing()const{return closing||closed;}
    ~CanvasWindow();
private:
    CanvasLatencyTrace latencyTrace{GetEnvironmentVariableW(L"CAPY_LATENCY_TRACE",nullptr,0)!=0};
    PresentationTrace presentationTrace{GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0};
    bool brushReady=false;
    bool TracePresentation(bool frame,bool dirty,bool repeat=false);
    std::shared_ptr<OrderedTraceFile> textInputTrace;uint64_t textInputPresses=0;
    void TraceTextInput(bool pressed);
    std::atomic<bool> workspaceTraceQueued{false};
    void QueueWorkspaceTrace();
    std::vector<std::wstring> launchFiles;
    void SendLaunchFiles(winrt::Windows::Data::Json::JsonObject const& model);
    struct Size { uint32_t width=1, height=1; float scale=1; };
    winrt::Microsoft::UI::Xaml::Window window;
    uint64_t const windowId;
    HWND nativeWindow=nullptr;
    std::function<void()> createWindow,testDeviceLoss;
    std::function<void(uint64_t)> onClosed,workspacePreferencesChanged;
    std::function<size_t(uint64_t)> otherWindows;
    std::optional<uint64_t> workspacePreferencesRevision;
    bool sessionWindowsOpened=false,discoveryReady=false;
    void TraceState(char const* kind,std::string const& value)const;
    winrt::Microsoft::UI::Xaml::Controls::SwapChainPanel panel;
    void* surfaceHandle=nullptr;
    CanvasSurfaceState surfaceState;
    winrt::Microsoft::UI::Xaml::Controls::ContentControl canvasFocus;
    winrt::Microsoft::UI::Xaml::Controls::TextBlock status;
    winrt::Microsoft::UI::Xaml::Controls::Grid launchCover;
    winrt::Microsoft::UI::Xaml::Controls::Image launchMark;
    winrt::Microsoft::UI::Xaml::Controls::TextBlock launchCaption;
    winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer launchCaptionTimer{nullptr};
    winrt::Microsoft::UI::Xaml::Media::Animation::Storyboard launchFade{nullptr},launchCaptionFade{nullptr};
    CanvasLaunchTiming launchTiming;
    CanvasLaunchPaint launchPaint;
    winrt::Microsoft::UI::Xaml::Media::CompositionTarget::Rendered_revoker launchFrame;
    bool launchLoaded=false;
    void CreateLaunchCover();
    void LayoutLaunchCover();
    void ArmLaunchCaption();
    void ApplyLaunchCover(winrt::Windows::Data::Json::JsonObject const& model);
    void HideLaunchCover(bool animate);
    void QueueLaunchReveal();
    void CancelLaunchReveal();
    static winrt::fire_and_forget AwaitLaunchPresent(std::weak_ptr<CanvasWindow>,
        winrt::Microsoft::UI::Dispatching::DispatcherQueue,CanvasLaunchPaint::Receipt);

    std::shared_ptr<CapyUi::WorkspaceData> screenData;
    winrt::Microsoft::UI::Xaml::Controls::Button screenChip;
    winrt::Microsoft::UI::Xaml::Controls::Flyout screenPopup;
    winrt::Microsoft::UI::Xaml::Controls::TextBlock screenTitle,screenHeadline,screenBody;
    winrt::Microsoft::UI::Xaml::Controls::CheckBox screenClipped;
    winrt::Windows::Data::Json::JsonObject screenCopy;
    winrt::hstring screenKey;
    double screenFontSize=14.;
    bool screenPopupOpen=false,updatingScreen=false;
    void ApplyScreen(winrt::Windows::Data::Json::JsonObject const& model);
    winrt::Microsoft::UI::Dispatching::DispatcherQueue dispatcher{nullptr};
    winrt::Windows::UI::ViewManagement::UISettings uiSettings;
    winrt::event_token colorValues{},motionValues{};
    winrt::Microsoft::UI::Xaml::Input::FocusManager::GettingFocus_revoker textFocus;
    std::string SystemTheme();
    std::string SystemMotion();
    winrt::Microsoft::UI::Dispatching::DispatcherQueueController inputController{nullptr};
    winrt::Microsoft::UI::Dispatching::DispatcherQueue inputDispatcher{nullptr};
    winrt::Microsoft::UI::Input::InputPointerSource inputSource{nullptr}; // input thread only
    enum class NavigationCursor { None, Pan, Zoom, ZoomOut, Rotate, Resize };
    std::atomic<NavigationCursor> navigationCursor{NavigationCursor::None};
    std::atomic<bool> fillResizeCursor{false};
    winrt::Windows::Devices::Haptics::SimpleHapticsController fillHaptics{nullptr};
    uint64_t fillTransaction=0,fillTick=0;
    winrt::Microsoft::UI::Input::InputCursor zoomInCursor{nullptr},zoomOutCursor{nullptr};
    winrt::Microsoft::UI::Input::PointerPredictor pointerPredictor{nullptr}; // input thread only
    struct CanvasTouchGesture {
        winrt::Microsoft::UI::Input::GestureRecognizer recognizer{nullptr};
        std::optional<uint32_t> contact;
        std::set<uint32_t> contacts;
    } canvasTouch,xamlCanvasTouch;
    std::optional<uint32_t> holdContact;
    std::set<uint32_t> contacts;
    std::set<uint32_t> xamlContacts;
    // Captured together under mutex; never pair a new DPI with an old camera.
    float inputScale=1;
    CapyView inputView{};
    std::unordered_map<uint32_t,std::pair<std::wstring,std::wstring>> heldKeys; // UI thread
    std::unordered_set<uint32_t> penButtons; // input thread
    PenSourceCache penSources;
    HWND orderedKeys=nullptr; // input thread
    bool orderingKeys=false; // input thread
    std::unordered_set<uint32_t> rawKeys; // key reader thread
    std::atomic<int32_t> keyBalance{0};
    std::vector<CanvasWork> heldWork; // input thread
    winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer holdTimer{nullptr}; // input thread
    std::unique_ptr<GamepadInput> gamepad;
    bool inputDone=false;
    CapyHost  host=0;
    std::unique_ptr<WorkspaceView> workspace;
    std::unique_ptr<HeaderView> header;
    std::unique_ptr<SettingsView> settings;
    std::unique_ptr<DocumentView> documents;
    std::unique_ptr<WorkspaceDialogs> workspaceDialogs;
    std::unique_ptr<SizeDialog> canvasSize,imageSize;
    std::unique_ptr<WorkspaceStorageView> workspaceStorage;
    std::unique_ptr<WorkspaceManagerView> workspaceManager;
    winrt::Windows::Data::Json::JsonObject lastModel;
    winrt::Windows::Data::Json::JsonObject tracedModel;
    CapyUi::CameraTrace cameraTrace;
    std::string lastGlass;
    uint64_t chromeRevision=0;
    std::wstring workspaceOwnerProperty;
    HWND workspaceOwnerWindow=nullptr;
    bool applyingDialogs=false,headerPopupOpen=false,workspacePopupOpen=false;
    std::atomic<bool> menuOpen{false},dialogOpen{false};
    std::shared_ptr<CapyUi::PointerModality> scrollbarInput;
    std::atomic<uint32_t> sentModifiers{0};
    std::optional<bool> pendingHover;
    enum class HoverRoute:uint8_t{None,Xaml,Independent};
    HoverRoute hoverRoute=HoverRoute::None;
    CanvasContactGate canvasContacts; // render thread
    winrt::Microsoft::UI::Xaml::Controls::StackPanel toolbar;
    winrt::Microsoft::UI::Xaml::Controls::Grid root;
    struct SnapshotState {
        std::mutex mutex;
        CanvasSnapshotMailbox pending;
        CanvasQuerySlots queries;
        std::deque<std::pair<uint64_t,std::function<void()>>> replies;
        std::string layoutInputBytes;
        NativeLayoutInput layoutInput;
        bool layoutInputFull=false;
        NativeModelUpdates modelUpdates;
        CanvasModelUpdate models;
        uint32_t unappliedSections=0;
        CanvasModelChanges unappliedChanges;
        winrt::Windows::Data::Json::JsonObject pendingLocalization{nullptr};
        void Close(){pending.Close();replies.clear();}
        void Reply(std::function<void()> deliver){
            {
                std::lock_guard lock(mutex);
                if(pending.Closed())return;
                auto publication=pending.Sequence();
                if(!pending.IsPosted(publication)){replies.emplace_back(publication,std::move(deliver));return;}
            }
            deliver();
        }
    };
    struct PreparedSnapshots {
        winrt::Windows::Data::Json::JsonObject full{nullptr},layout{nullptr},workspace{nullptr},camera{nullptr},search{nullptr},localization{nullptr},glass{nullptr};
        NativeLayoutInput fullInput,reflowInput;
        uint32_t sections=CanvasAllSections;
        CanvasModelChanges changes;
    };
    std::shared_ptr<SnapshotState> snapshots=std::make_shared<SnapshotState>();
    winrt::Microsoft::UI::Xaml::Media::CompositionTarget::Rendered_revoker glassFrame;
    struct NavigatorWork {CapyNavigatorViewport viewport{};bool remove=false;std::function<void(NativeSurfaceAttachment)> reply;};
    struct NavigatorWatch {std::function<void(NativeSurfaceAttachment)> reply;CapyNavigatorSurfaceInfo published{};};
    std::map<uint64_t,NavigatorWork> navigatorWork;
    std::map<uint64_t,NavigatorWatch> navigatorWatches;
    bool navigatorDirty=false;
    void PresentNavigators();
    void UpdateNavigationCursor();
    void Publish(std::string snapshot, CapySnapshotInfo const& info);
    std::vector<std::pair<std::function<void(int32_t)>,int32_t>> completedCommands;
    void CompleteCommands();
    void QueueSnapshots();
    static winrt::fire_and_forget DecodeSnapshots(std::shared_ptr<SnapshotState>,
        winrt::Microsoft::UI::Dispatching::DispatcherQueue,std::weak_ptr<CanvasWindow>,CanvasSnapshotMailbox::Batch);
    bool ApplyPending(PreparedSnapshots);
    void FailPresentation(std::string);
    bool ApplyModel(winrt::Windows::Data::Json::JsonObject const&,NativeLayoutInput,uint32_t sections=CanvasAllSections,CanvasModelChanges const& changes={true});
    CanvasModelInputs windowInputs,captionInputs;
    void Popup(bool open);
    void UpdatePopup();
    void ApplyDialogs(CanvasModelChanges const* changes=nullptr);
    bool ModalOpen()const;
    void RequestClose();
    void ChromeMotion(winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs const&,bool leave=false);
    void Fullscreen();
    std::jthread renderer;
    std::mutex mutex;
    std::condition_variable wake;
    std::condition_variable space;
    CanvasWorkBuffer work;
    CanvasQueryQueue previewWork;
    bool RequestPreviews(CanvasQueryKind,std::string,PreviewReply);
    bool transportFailed=false;
    std::string surfaceError; // mutex: UI surface-reset result.
    bool inputStopped=false; // mutex: admitted work remains owned by the worker.
    bool statusFailed=false; // UI thread: readiness must not hide a reported error.
    Size desired;
    std::vector<winrt::Windows::Graphics::RectInt32> captionRegions,captionInputRegions,bubbleInputRegions;
    bool captionRegionsValid=false,nativeSizing=false;
    winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer captionRetry{nullptr};
    bool closing=false, closed=false, finishing=false, resize=false, paused=false, servicesReady=false;
    std::atomic<bool> rendererDone{false};
    uint64_t sequence=0;
    void Start();
    bool StartPrepared(CapyLaunch );
    bool launchReady=false,launchSucceeded=false;
    NativeLocalization localization;
    winrt::Windows::Data::Json::JsonObject bootstrap;
    void Resize();
    void PublishGlass();
    void CollectGlass();
    static LRESULT CALLBACK WindowMessage(HWND window,UINT message,WPARAM wparam,LPARAM lparam,UINT_PTR id,DWORD_PTR data);
    bool RecoverGpu();
    void SaveAfterGpuFailure(std::string const& reason);
    void ResetSurface();
    int DispatchWork(CanvasWork const&,bool retiring);
    void Run();
    void Stop();
    void Finish();
    void CloseViews();
    void Fail(std::string message);
    void Send(std::string json, CanvasCommandKind kind=CanvasCommandKind::Action, std::function<void(int32_t)> reply={});
    bool SendIndependent(CanvasWork item);
    void Key(winrt::Microsoft::UI::Xaml::Input::KeyRoutedEventArgs const&, bool pressed);
    bool CancelGesture();
    bool SyncContactModifiers(winrt::Windows::System::VirtualKeyModifiers held);
    void OrderKeys(bool contact);
    void RawKeysHeld();
    void RawKey(RAWKEYBOARD const& key);
    void ReleaseKeyHold();
    bool FlushHeld();
    bool Enqueue(CanvasWork item);
    bool ClaimsDeviceKey(std::wstring const& name)const;
    bool PenButtons(winrt::Microsoft::UI::Input::PointerPoint const& point,uint32_t phase);
    bool PenButtons(uint32_t id,bool primary);
    void Wheel(winrt::Microsoft::UI::Input::PointerEventArgs const&);
    void ObservePointerModality(winrt::Microsoft::UI::Input::PointerPoint const&);
    CanvasScroll ScrollSample(winrt::Windows::Foundation::Point,int32_t,bool,winrt::Windows::System::VirtualKeyModifiers);
    // Explicit smoke fixtures; only the input dispatcher touches replayTime.
    enum class ReplayKind { Stroke, Pan, Backlog, Pen, PenBegin, PenEnd, Eraser };
    uint64_t replayTime=0;
    void Replay(ReplayKind,std::string point={});
    winrt::Windows::Foundation::IAsyncOperation<winrt::Windows::ApplicationModel::DataTransfer::DataPackageOperation> DropFiles(winrt::Windows::ApplicationModel::DataTransfer::DataPackageView content,winrt::Windows::Foundation::Point point);
    winrt::fire_and_forget TestFileDrop(winrt::Microsoft::UI::Xaml::Controls::TextBox arguments);
    void StartInput();
    void LeaveHover(HoverRoute route);
    void Pointer(winrt::Microsoft::UI::Input::PointerEventArgs const&, uint32_t phase);
    using PointerPoints=winrt::Windows::Foundation::Collections::IVector<winrt::Microsoft::UI::Input::PointerPoint>;
    std::vector<CapyPointer> PointerSamples(winrt::Microsoft::UI::Input::PointerPoint const&,PointerPoints const&,uint32_t phase,bool predict);
    void Pointer(std::vector<CapyPointer>,winrt::Windows::System::VirtualKeyModifiers,bool pen,bool barrel,uint64_t arrival);
    void XamlPointer(winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs const&,uint32_t phase);
    void CanvasTouch(winrt::Microsoft::UI::Input::PointerEventArgs const&, uint32_t phase);
    void CanvasTouch(CanvasTouchGesture&,winrt::Microsoft::UI::Input::PointerPoint const&,PointerPoints const&,uint32_t phase);
    void StartCanvasTouch(CanvasTouchGesture&);
    void CancelCanvasTouch(CanvasTouchGesture&);
    void TrackContact(uint32_t id,uint32_t phase,bool touch);
    float PickerOffset(float scale)const;
};
