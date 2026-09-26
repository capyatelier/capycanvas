#pragma once
#include "pch.h"
#include "WorkspaceView.h"
#include "HeaderView.h"
#include "SettingsView.h"
#include "DocumentView.h"
#include "WorkspaceDialogs.h"
#include "SelectionDialog.h"
#include "WorkspaceStorageView.h"
#include "WorkspaceManagerView.h"
#include "CanvasWorkBuffer.h"
#include "CanvasLatencyTrace.h"
#include "CanvasSnapshotMailbox.h"
#include "FilterPreviews.h"
#include "native/include/capy_windows.h"
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
    CanvasWindow(std::function<void()> createWindow,std::function<void(uint64_t)> onClosed,bool primary,
        std::function<void(uint64_t)> preferencesChanged);
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
    std::vector<std::wstring> launchFiles;
    void SendLaunchFiles(winrt::Windows::Data::Json::JsonObject const& model);
    struct Size { uint32_t width=1, height=1; float scale=1; };
    winrt::Microsoft::UI::Xaml::Window window;
    uint64_t const windowId;
    bool const primaryWindow;
    std::function<void()> createWindow;
    std::function<void(uint64_t)> onClosed,workspacePreferencesChanged;
    std::optional<uint64_t> workspacePreferencesRevision;
    void TraceState(char const* kind,std::string const& value)const;
    winrt::Microsoft::UI::Xaml::Controls::SwapChainPanel panel;
    winrt::Microsoft::UI::Xaml::Controls::ContentControl canvasFocus;
    winrt::Microsoft::UI::Xaml::Controls::TextBlock status;
    winrt::Microsoft::UI::Dispatching::DispatcherQueue dispatcher{nullptr};
    winrt::Windows::UI::ViewManagement::UISettings uiSettings;
    winrt::event_token colorValues{};
    std::string SystemTheme();
    winrt::Microsoft::UI::Dispatching::DispatcherQueueController inputController{nullptr};
    winrt::Microsoft::UI::Dispatching::DispatcherQueue inputDispatcher{nullptr};
    winrt::Microsoft::UI::Input::InputPointerSource inputSource{nullptr}; // input thread only
    std::atomic<bool> panCursor{false};
    winrt::Microsoft::UI::Input::PointerPredictor pointerPredictor{nullptr}; // input thread only
    winrt::Microsoft::UI::Input::GestureRecognizer pickerHold{nullptr};
    std::optional<uint32_t> holdContact;
    std::set<uint32_t> contacts;
    // Captured together under mutex; never pair a new DPI with an old camera.
    float inputScale=1;
    uint64_t revision=0;
    std::unordered_map<uint32_t,std::wstring> heldKeys; // UI thread
    bool inputDone=false;
    CapyHost* host=nullptr;
    std::unique_ptr<WorkspaceView> workspace;
    std::unique_ptr<HeaderView> header;
    std::unique_ptr<SettingsView> settings;
    std::unique_ptr<DocumentView> documents;
    std::unique_ptr<WorkspaceDialogs> workspaceDialogs;
    std::unique_ptr<SelectionDialog> selectionDialog;
    std::unique_ptr<WorkspaceStorageView> workspaceStorage;
    std::unique_ptr<WorkspaceManagerView> workspaceManager;
    winrt::Windows::Data::Json::JsonObject lastModel;
    std::string lastGlass;
    std::wstring workspaceOwnerProperty;
    HWND workspaceOwnerWindow=nullptr;
    bool applyingDialogs=false,headerPopupOpen=false,workspacePopupOpen=false;
    std::atomic<bool> menuOpen{false},dialogOpen{false};
    std::atomic<uint32_t> sentModifiers{0};
    struct Hover {float x,y;bool leave,touch;};
    std::optional<Hover> pendingHover;
    std::unordered_set<uint64_t> consumedContacts; // render thread
    winrt::Microsoft::UI::Xaml::Controls::StackPanel toolbar;
    winrt::Microsoft::UI::Xaml::Controls::Grid root;
    CanvasSnapshotMailbox snapshots;
    bool snapshotPosted=false;
    void Publish(std::string snapshot, winrt::Windows::Data::Json::JsonObject const& model);
    void ApplyPending();
    void ApplyModel(winrt::Windows::Data::Json::JsonObject const&);
    void Popup(bool open);
    void UpdatePopup();
    void ApplyDialogs();
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
    std::vector<winrt::Windows::Graphics::RectInt32> captionRegions,captionInputRegions;
    bool captionRegionsValid=false;
    winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer captionRetry{nullptr};
    bool closing=false, closed=false, finishing=false, resize=false, paused=false, servicesReady=false;
    std::atomic<bool> rendererDone{false};
    uint64_t sequence=0;
    void Start();
    void Resize();
    void PublishGlass();
    void ApplyResize();
    bool RecoverGpu();
    void SaveAfterGpuFailure(std::string const& reason);
    void ResetSurface();
    int DispatchWork(CanvasWork const&,bool retiring);
    void Run();
    void Stop();
    void Finish();
    void Fail(std::string message);
    void Send(std::string json, CanvasCommandKind kind=CanvasCommandKind::Action);
    bool SendIndependent(CanvasWork item);
    void Key(winrt::Microsoft::UI::Xaml::Input::KeyRoutedEventArgs const&, bool pressed);
    bool SyncContactModifiers(winrt::Windows::System::VirtualKeyModifiers held);
    void Wheel(winrt::Microsoft::UI::Input::PointerEventArgs const&);
    // Explicit smoke fixtures; only the input dispatcher touches replayTime.
    enum class ReplayKind { Stroke, Pan, Backlog, Pen, PenBegin, PenEnd };
    uint64_t replayTime=0;
    void Replay(ReplayKind);
    void StartInput();
    void Pointer(winrt::Microsoft::UI::Input::PointerEventArgs const&, uint32_t phase);
    void PickerHold(winrt::Microsoft::UI::Input::PointerEventArgs const&, uint32_t phase);
    void CancelPickerHold();
    float PickerOffset(float scale)const;
};
