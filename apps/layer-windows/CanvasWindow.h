#pragma once
#include "pch.h"
#include "native/include/capy_windows.h"
#include <atomic>
#include <condition_variable>
#include <deque>
#include <memory>
#include <mutex>
#include <thread>
#include <variant>
#include <vector>

class CanvasWindow : public std::enable_shared_from_this<CanvasWindow> {
public:
    void Open();
    ~CanvasWindow();
private:
    struct Size { uint32_t width=1, height=1; float scale=1; };
    struct Command { bool input=false; std::string json; };
    using Work = std::variant<std::vector<CapyPointer>, Command>;
    winrt::Microsoft::UI::Xaml::Window window;
    winrt::Microsoft::UI::Xaml::Controls::SwapChainPanel panel;
    winrt::Microsoft::UI::Xaml::Controls::TextBlock status;
    winrt::Microsoft::UI::Dispatching::DispatcherQueue dispatcher{nullptr};
    winrt::Microsoft::UI::Dispatching::DispatcherQueueController inputController{nullptr};
    winrt::Microsoft::UI::Dispatching::DispatcherQueue inputDispatcher{nullptr};
    winrt::Microsoft::UI::Input::InputPointerSource inputSource{nullptr}; // input thread only
    std::atomic<float> inputScale{1};
    bool inputDone=false;
    CapyHost* host=nullptr;
    std::jthread renderer;
    std::mutex mutex;
    std::condition_variable wake;
    std::deque<Work> work;
    Size desired;
    bool closing=false, closed=false, resize=false, paused=false;
    std::atomic<bool> rendererDone{false};
    std::atomic<uint64_t> revision{0};
    uint64_t sequence=0;
    void Start();
    void Resize();
    void ApplyResize();
    void Run();
    void Stop();
    void Finish();
    void Fail(std::string message);
    void Send(std::string json, bool input=false);
    void Replay(bool pan); // Explicit smoke-test fixture, not OS input evidence.
    void StartInput();
    void Pointer(winrt::Microsoft::UI::Input::PointerEventArgs const&, uint32_t phase);
};
