#pragma once
#include "pch.h"
#include "WorkspaceView.h"
#include "HeaderView.h"
#include "SettingsView.h"
#include "DocumentView.h"
#include "CanvasWorkBuffer.h"
#include "native/include/capy_windows.h"
#include <atomic>
#include <condition_variable>
#include <deque>
#include <memory>
#include <mutex>
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
    void Open();
    ~CanvasWindow();
private:
    struct Size { uint32_t width=1, height=1; float scale=1; };
    winrt::Microsoft::UI::Xaml::Window window;
    winrt::Microsoft::UI::Xaml::Controls::SwapChainPanel panel;
    winrt::Microsoft::UI::Xaml::Controls::ContentControl canvasFocus;
    winrt::Microsoft::UI::Xaml::Controls::TextBlock status;
    winrt::Microsoft::UI::Dispatching::DispatcherQueue dispatcher{nullptr};
    winrt::Microsoft::UI::Dispatching::DispatcherQueueController inputController{nullptr};
    winrt::Microsoft::UI::Dispatching::DispatcherQueue inputDispatcher{nullptr};
    winrt::Microsoft::UI::Input::InputPointerSource inputSource{nullptr}; // input thread only
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
    winrt::Windows::Data::Json::JsonObject lastModel;
    bool applyingDialogs=false,headerPopupOpen=false;
    std::atomic<bool> menuOpen{false},dialogOpen{false};
    struct Hover {float x,y;bool leave,touch;};
    std::optional<Hover> pendingHover;
    std::unordered_set<uint64_t> consumedContacts; // render thread
    winrt::Microsoft::UI::Xaml::Controls::StackPanel toolbar;
    winrt::Microsoft::UI::Xaml::Controls::Grid root;
    std::string pendingFull, pendingCamera;
    bool snapshotPosted=false;
    void Publish(std::string snapshot, bool full);
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
    bool transportFailed=false;
    bool statusFailed=false; // UI thread: readiness must not hide a reported error.
    Size desired;
    bool closing=false, closed=false, finishing=false, resize=false, paused=false, servicesReady=false;
    std::atomic<bool> rendererDone{false};
    uint64_t sequence=0;
    void Start();
    void Resize();
    void ApplyResize();
    void Run();
    void Stop();
    void Finish();
    void Fail(std::string message);
    void Send(std::string json, bool input=false, bool document=false);
    bool SendIndependent(CanvasWork item);
    void Key(winrt::Microsoft::UI::Xaml::Input::KeyRoutedEventArgs const&, bool pressed);
    void Wheel(winrt::Microsoft::UI::Input::PointerEventArgs const&);
    void Replay(bool pan, bool backlog=false); // Explicit smoke-test fixture, not OS input evidence.
    void StartInput();
    void Pointer(winrt::Microsoft::UI::Input::PointerEventArgs const&, uint32_t phase);
};
