#pragma once
#include "pch.h"
#include "GamepadState.h"
#include <winrt/Windows.Gaming.Input.h>
#include <condition_variable>
#include <mutex>
#include <thread>

class GamepadInput {
public:
    explicit GamepadInput(std::function<void(std::string)> send):state(std::move(send)){}
    ~GamepadInput(){
        {std::lock_guard lock(mutex);stopping=true;}
        wake.notify_all();
        if(worker.joinable())worker.join();
    }
    void Active(bool value){
        {std::lock_guard lock(mutex);active=value;if(value&&!worker.joinable())worker=std::thread([this]{run();});}
        wake.notify_all();
    }
private:
    GamepadState state;
    std::mutex mutex;
    std::condition_variable wake;
    bool active=false,stopping=false;
    std::thread worker;

    static GamepadSample sample(winrt::Windows::Gaming::Input::GamepadReading const& reading){
        using B=winrt::Windows::Gaming::Input::GamepadButtons;
        static constexpr std::array<B,14> order{B::A,B::B,B::X,B::Y,B::LeftShoulder,B::RightShoulder,B::View,B::Menu,
            B::LeftThumbstick,B::RightThumbstick,B::DPadUp,B::DPadDown,B::DPadLeft,B::DPadRight};
        GamepadSample result{0,reading.LeftTrigger,reading.RightTrigger,reading.LeftThumbstickX,reading.LeftThumbstickY,reading.RightThumbstickY};
        for(size_t i=0;i<order.size();++i)if((reading.Buttons&order[i])==order[i])result.buttons|=1u<<i;
        return result;
    }
    void run(){
        winrt::init_apartment(winrt::apartment_type::multi_threaded);
        std::unique_lock lock(mutex);
        while(!stopping){
            if(!active){
                lock.unlock();state.Release(GamepadState::Clock::now());lock.lock();
                wake.wait(lock,[&]{return stopping||active;});continue;
            }
            lock.unlock();
            bool connected=false;
            try{
                auto pads=winrt::Windows::Gaming::Input::Gamepad::Gamepads();
                if(pads.Size()){connected=true;state.Read(sample(pads.GetAt(0).GetCurrentReading()),GamepadState::Clock::now());}
                else state.Release(GamepadState::Clock::now());
            }catch(winrt::hresult_error const&){state.Release(GamepadState::Clock::now());}
            lock.lock();
            wake.wait_for(lock,std::chrono::milliseconds(connected?16:500),[&]{return stopping||!active;});
        }
        lock.unlock();state.Release(GamepadState::Clock::now());
    }
};
