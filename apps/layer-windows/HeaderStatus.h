#pragma once
#include "UiControls.h"
#include <chrono>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

// Native clock/power observations stay on the UI dispatcher, outside painting.
// Workspace items decide visibility; hidden status does not run a timer.
class HeaderStatus {
    struct Impl : std::enable_shared_from_this<Impl> {
        std::shared_ptr<CapyUi::WorkspaceData> data;
        std::function<void()> changed;
        winrt::Microsoft::UI::Xaml::Controls::Border clockTile,batteryTile,shell,charge,cap;
        winrt::Microsoft::UI::Xaml::Controls::TextBlock clock,percent;
        winrt::Microsoft::UI::Xaml::Controls::Grid battery;
        winrt::Microsoft::UI::Xaml::Shapes::Path bolt{nullptr};
        winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
        bool visible=false,placeholder=false,editable=false,knownBattery=false;
        ~Impl(){if(timer)timer.Stop();}
        void init(){
            using namespace CapyUi;
            clock=label(data,L"");clock.VerticalAlignment(VerticalAlignment::Center);
            AutomationProperties::SetAutomationId(clock,L"system-clock");
            clockTile.Child(clock);clockTile.Height(36);clockTile.Padding({8,0,8,0});

            batteryTile.Width(36);batteryTile.Height(36);
            for(auto tile:{clockTile,batteryTile})tile.Background(headerSurface(data));
            clockTile.CornerRadius({18*.54,18*.54,18*.54,18*.54});batteryTile.CornerRadius({18*.54,18*.54,18*.54,18*.54});
            battery.Width(26);battery.Height(14);battery.VerticalAlignment(VerticalAlignment::Center);
            battery.HorizontalAlignment(HorizontalAlignment::Center);
            shell.Width(22);shell.Height(14);shell.HorizontalAlignment(HorizontalAlignment::Left);shell.CornerRadius({4,4,4,4});
            charge.Height(14);charge.HorizontalAlignment(HorizontalAlignment::Left);charge.CornerRadius({4,4,4,4});
            shell.Child(charge);battery.Children().Append(shell);
            percent=label(data,L"",true);percent.Width(22);percent.LineHeight(14);
            percent.HorizontalAlignment(HorizontalAlignment::Left);percent.TextAlignment(TextAlignment::Center);
            percent.VerticalAlignment(VerticalAlignment::Center);battery.Children().Append(percent);
            cap.Width(2);cap.Height(6);cap.CornerRadius({1,1,1,1});cap.HorizontalAlignment(HorizontalAlignment::Left);
            cap.VerticalAlignment(VerticalAlignment::Top);cap.Margin({23,4,0,0});battery.Children().Append(cap);
            bolt=Markup::XamlReader::Load(L"<Path xmlns='http://schemas.microsoft.com/winfx/2006/xaml/presentation' Data='M5 1 .5 7h2.7L2 11l5-6.1H4.7L6.1 1Z' StrokeLineJoin='Round' StrokeThickness='1.75' />").as<Shapes::Path>();
            bolt.Width(8);bolt.Height(12);bolt.HorizontalAlignment(HorizontalAlignment::Left);
            bolt.VerticalAlignment(VerticalAlignment::Top);bolt.Margin({18,1,0,0});battery.Children().Append(bolt);
            batteryTile.Child(battery);
            AutomationProperties::SetAutomationId(batteryTile,L"system-battery");
            AutomationProperties::SetAccessibilityView(shell,Automation::Peers::AccessibilityView::Raw);
            AutomationProperties::SetAccessibilityView(percent,Automation::Peers::AccessibilityView::Raw);
            timer=clockTile.DispatcherQueue().CreateTimer();timer.IsRepeating(false);
            timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
            clockTile.SizeChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&self->changed)self->changed();});
        }
        void refresh(){
            using namespace CapyUi;
            if(!visible){timer.Stop();return;}
            wchar_t text[128]{};
            if(placeholder){clock.Text(L"Clock");}else if(GetTimeFormatEx(LOCALE_NAME_USER_DEFAULT,TIME_NOSECONDS,nullptr,nullptr,text,128)){
                if(clock.Text()!=text)clock.Text(text);
            }
            SYSTEM_POWER_STATUS power{};
            bool known=GetSystemPowerStatus(&power)&&!(power.BatteryFlag&128)&&power.BatteryLifePercent<=100;
            if(knownBattery!=known){knownBattery=known;if(changed)changed();}
            batteryTile.Visibility(known||editable?Visibility::Visible:Visibility::Collapsed);
            if(known||editable){
                int level=known?power.BatteryLifePercent:0;bool charging=known&&(power.BatteryFlag&8)!=0,low=known&&level<=15&&!charging;
                bool light=data->theme()==L"light";
                auto track=color(charging?L"#c4c9cf":light?L"#707479":L"#a3a8b0");
                auto ink=color(charging?L"#13251a":light?L"#ffffff":L"#202226");
                auto paint=color(charging?L"#91b89d":low?(light?L"#a15d59":L"#bc9996"):light?L"#3f4246":L"#e5e7eb");
                shell.Background(fill(track));cap.Background(fill(track));charge.Background(fill(paint));charge.Width(22.*level/100.);
                percent.Text(known?to_hstring(level):L"?");percent.FontSize(level==100?10:11);percent.Foreground(fill(ink));
                cap.Visibility(charging?Visibility::Collapsed:Visibility::Visible);bolt.Visibility(charging?Visibility::Visible:Visibility::Collapsed);
                bolt.Fill(fill(light?ink:color(L"#e5e7eb")));bolt.Stroke(fill(light?track:color(L"#202226")));
                auto description=known?L"Battery "+to_hstring(level)+L"%"+(charging?L", charging":low?L", low":L""):L"Battery unavailable";
                AutomationProperties::SetName(batteryTile,description);tooltip(batteryTile,description);
            }
            clock.Foreground(data->brush(L"text"));
            SYSTEMTIME now{};GetLocalTime(&now);
            auto untilMinute=60000-int(now.wSecond)*1000-int(now.wMilliseconds);
            timer.Interval(std::chrono::milliseconds(std::max(20,std::min(15000,untilMinute+20))));
            if(!placeholder)timer.Start();else timer.Stop();
        }
        void apply(bool fullscreen,bool hidden,bool editing){
            using namespace CapyUi;
            bool next=!hidden&&(fullscreen||editing),nextPlaceholder=editing&&!fullscreen;
            clockTile.Visibility(next?Visibility::Visible:Visibility::Collapsed);
            if(next!=visible||placeholder!=nextPlaceholder||editable!=editing){visible=next;placeholder=nextPlaceholder;editable=editing;refresh();}
            if(!next)batteryTile.Visibility(Visibility::Collapsed);
        }
    };
    std::shared_ptr<Impl> impl;
public:
    HeaderStatus(std::shared_ptr<CapyUi::WorkspaceData> data,std::function<void()> changed):impl(std::make_shared<Impl>()){
        impl->data=std::move(data);impl->changed=std::move(changed);impl->init();
    }
    auto Clock()const{return impl->clockTile;}
    auto Battery()const{return impl->batteryTile;}
    bool HasBattery()const{return impl->knownBattery;}
    void Apply(bool fullscreen,bool hidden,bool editing){impl->apply(fullscreen,hidden,editing);}
};
