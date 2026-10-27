#pragma once
#include "pch.h"
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
#include "CanvasContactGate.h"
#include "CanvasLatencyTrace.h"
#include "CanvasSnapshotMailbox.h"
#include "FilterPreviews.h"
#include "GamepadInput.h"
#include "NativeHandles.h"
#include "PenSourceCache.h"
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

class CanvasWindow : public std::enable_shared_from_this<CanvasWindow> {
public:
    CanvasWindow(std::function<void()> createWindow,std::function<void(uint64_t)> onClosed,
        std::function<void(uint64_t)> preferencesChanged,std::function<void()> testDeviceLoss);
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
    void TracePresentation(bool frame,bool dirty,bool repeat=false);
    std::vector<std::wstring> launchFiles;
    void SendLaunchFiles(winrt::Windows::Data::Json::JsonObject const& model);
    struct Size { uint32_t width=1, height=1; float scale=1; };
    winrt::Microsoft::UI::Xaml::Window window;
    uint64_t const windowId;
    HWND nativeWindow=nullptr;
    std::function<void()> createWindow,testDeviceLoss;
    std::function<void(uint64_t)> onClosed,workspacePreferencesChanged;
    std::optional<uint64_t> workspacePreferencesRevision;
    bool sessionWindowsOpened=false;
    void TraceState(char const* kind,std::string const& value)const;
    winrt::Microsoft::UI::Xaml::Controls::SwapChainPanel panel;
    void* surfaceHandle=nullptr;
    CanvasSurfaceState surfaceState;
    winrt::Microsoft::UI::Xaml::Controls::ContentControl canvasFocus;
    winrt::Microsoft::UI::Xaml::Controls::TextBlock status;
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
    winrt::event_token colorValues{};
    winrt::Microsoft::UI::Xaml::Input::FocusManager::GettingFocus_revoker textFocus;
    std::string SystemTheme();
    winrt::Microsoft::UI::Dispatching::DispatcherQueueController inputController{nullptr};
    winrt::Microsoft::UI::Dispatching::DispatcherQueue inputDispatcher{nullptr};
    winrt::Microsoft::UI::Input::InputPointerSource inputSource{nullptr}; // input thread only
    enum class NavigationCursor { None, Pan, Zoom, ZoomOut, Rotate };
    std::atomic<NavigationCursor> navigationCursor{NavigationCursor::None};
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
    uint64_t revision=0;
    std::unordered_map<uint32_t,std::wstring> heldKeys; // UI thread
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
    std::string lastGlass;
    std::wstring workspaceOwnerProperty;
    HWND workspaceOwnerWindow=nullptr;
    bool applyingDialogs=false,headerPopupOpen=false,workspacePopupOpen=false;
    std::atomic<bool> menuOpen{false},dialogOpen{false};
    std::shared_ptr<CapyUi::PointerModality> scrollbarInput;
    std::atomic<uint32_t> sentModifiers{0};
    std::optional<bool> pendingHover;
    CanvasContactGate canvasContacts; // render thread
    winrt::Microsoft::UI::Xaml::Controls::StackPanel toolbar;
    winrt::Microsoft::UI::Xaml::Controls::Grid root;
    struct SnapshotState {
        std::mutex mutex;
        CanvasSnapshotMailbox pending;
        CanvasQuerySlots queries;
        std::deque<std::pair<uint64_t,std::function<void()>>> replies;
        void Close(){pending.Close();replies.clear();}
    };
    struct PreparedSnapshots {
        winrt::Windows::Data::Json::JsonObject full{nullptr},layout{nullptr},workspace{nullptr},camera{nullptr},search{nullptr},localization{nullptr},glass{nullptr};
        std::string fullLayoutBytes,layoutBytes;
    };
    std::shared_ptr<SnapshotState> snapshots=std::make_shared<SnapshotState>();
    bool glassPosted=false;
    void UpdateNavigationCursor();
    void Publish(std::string snapshot, CapySnapshotInfo const& info);
    void QueueSnapshots();
    static winrt::fire_and_forget DecodeSnapshots(std::shared_ptr<SnapshotState>,
        winrt::Microsoft::UI::Dispatching::DispatcherQueue,std::weak_ptr<CanvasWindow>,CanvasSnapshotMailbox::Batch);
    void ApplyPending(PreparedSnapshots);
    void FailPresentation(std::string);
    void ApplyModel(winrt::Windows::Data::Json::JsonObject const&,std::string layoutBytes={});
    void Popup(bool open);
    void UpdatePopup();
    void ApplyDialogs();
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
    bool captionRegionsValid=false;
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
    enum class ReplayKind { Stroke, Pan, Backlog, Pen, PenBegin, PenEnd };
    uint64_t replayTime=0;
    void Replay(ReplayKind);
    void StartInput();
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
