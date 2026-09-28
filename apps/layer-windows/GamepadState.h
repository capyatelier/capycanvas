#pragma once
#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <functional>
#include <string>

struct GamepadSample {
    uint32_t buttons=0;
    double leftTrigger=0,rightTrigger=0,leftX=0,leftY=0,rightY=0;
};

class GamepadState {
public:
    using Clock=std::chrono::steady_clock;
    static constexpr auto RepeatDelay=std::chrono::milliseconds(500),RepeatEvery=std::chrono::milliseconds(50);
    static constexpr std::array<char const*,14> Buttons{"gamepad_a","gamepad_b","gamepad_x","gamepad_y","gamepad_l1","gamepad_r1",
        "gamepad_select","gamepad_start","gamepad_l3","gamepad_r3","gamepad_up","gamepad_down","gamepad_left","gamepad_right"};
    explicit GamepadState(std::function<void(std::string)> send):send(std::move(send)){}
    void Read(GamepadSample const& sample,Clock::time_point now){
        for(size_t i=0;i<Buttons.size();++i)button(i,Buttons[i],(sample.buttons>>i)&1u,now);
        trigger(14,"gamepad_l2",sample.leftTrigger,now);trigger(15,"gamepad_r2",sample.rightTrigger,now);
        stick({rounded(sample.leftX),rounded(-sample.leftY),rounded(sample.rightY)});
    }
    void Release(Clock::time_point now){Read(GamepadSample{},now);}
private:
    struct Held{bool down=false;Clock::time_point next;};
    std::function<void(std::string)> send;
    std::array<Held,16> held{};
    std::array<float,3> axes{};
    void key(char const* name,bool pressed,bool repeat){
        send(std::string(R"({"type":"key","key":")")+name+R"(","pressed":)"+(pressed?"true":"false")+R"(,"repeat":)"+(repeat?"true":"false")+
            R"(,"editing":false,"modifiers":{"command":false,"shift":false,"alt":false}})");
    }
    void button(size_t index,char const* name,bool down,Clock::time_point now){
        auto& state=held[index];
        if(down&&!state.down){state={true,now+RepeatDelay};key(name,true,false);}
        else if(down&&now>=state.next){state.next=now+RepeatEvery;key(name,true,true);}
        else if(!down&&state.down){state.down=false;key(name,false,false);}
    }
    void trigger(size_t index,char const* name,double value,Clock::time_point now){button(index,name,held[index].down?value>.3:value>=.5,now);}
    static float rounded(double value){return float(std::round(std::clamp(std::isfinite(value)?value:0.,-1.,1.)*100)/100)+0.f;}
    void stick(std::array<float,3> next){
        if(next==axes)return;
        axes=next;char json[128];
        std::snprintf(json,sizeof json,R"({"type":"axes","pan":[%.2f,%.2f],"zoom":%.2f})",next[0],next[1],next[2]);
        send(json);
    }
};
