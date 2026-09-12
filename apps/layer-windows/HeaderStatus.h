#pragma once
#include "UiControls.h"
#include <chrono>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

// Native clock/power observations stay on the UI dispatcher, outside painting.
// Visibility follows the shared preference; hidden status does not run a timer.
class HeaderStatus {
    struct Impl : std::enable_shared_from_this<Impl> {
        std::shared_ptr<CapyUi::WorkspaceData> data;
        std::function<void()> changed;
        winrt::Microsoft::UI::Xaml::Controls::StackPanel root;
        winrt::Microsoft::UI::Xaml::Controls::Border clockTile,batteryTile,shell,charge,cap;
        winrt::Microsoft::UI::Xaml::Controls::TextBlock clock,percent;
        winrt::Microsoft::UI::Xaml::Controls::Grid battery;
        winrt::Microsoft::UI::Xaml::Shapes::Path bolt{nullptr};
        winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
        bool visible=false;
        ~Impl(){if(timer)timer.Stop();}
        void init(){
            using namespace CapyUi;
            root.Orientation(Orientation::Horizontal);root.Height(36);
            AutomationProperties::SetAutomationId(root,L"system-status");
            AutomationProperties::SetName(root,L"Battery and clock");
            clock=label(data,L"");clock.VerticalAlignment(VerticalAlignment::Center);
            AutomationProperties::SetAutomationId(clock,L"system-clock");
            clockTile.Child(clock);clockTile.Height(36);clockTile.Padding({6,0,6,0});
            root.Children().Append(clockTile);
            batteryTile.Width(36);batteryTile.Height(36);
            battery.Width(26);battery.Height(14);battery.VerticalAlignment(VerticalAlignment::Center);
            battery.HorizontalAlignment(HorizontalAlignment::Center);
            shell.Width(22);shell.Height(14);shell.HorizontalAlignment(HorizontalAlignment::Left);shell.CornerRadius({4});
            charge.Height(14);charge.HorizontalAlignment(HorizontalAlignment::Left);charge.CornerRadius({4});
            shell.Child(charge);battery.Children().Append(shell);
            percent=label(data,L"",true);percent.Width(22);percent.LineHeight(14);
            percent.HorizontalAlignment(HorizontalAlignment::Left);percent.TextAlignment(TextAlignment::Center);
            percent.VerticalAlignment(VerticalAlignment::Center);battery.Children().Append(percent);
            cap.Width(2);cap.Height(6);cap.CornerRadius({1});cap.HorizontalAlignment(HorizontalAlignment::Left);
            cap.VerticalAlignment(VerticalAlignment::Top);cap.Margin({23,4,0,0});battery.Children().Append(cap);
            bolt=Markup::XamlReader::Load(L"<Path xmlns='http://schemas.microsoft.com/winfx/2006/xaml/presentation' Data='M5 1 .5 7h2.7L2 11l5-6.1H4.7L6.1 1Z' StrokeLineJoin='Round' StrokeThickness='1.75' />").as<Shapes::Path>();
            bolt.Width(8);bolt.Height(12);bolt.HorizontalAlignment(HorizontalAlignment::Left);
            bolt.VerticalAlignment(VerticalAlignment::Top);bolt.Margin({18,1,0,0});battery.Children().Append(bolt);
            batteryTile.Child(battery);root.Children().Append(batteryTile);
            AutomationProperties::SetAutomationId(batteryTile,L"system-battery");
            AutomationProperties::SetAccessibilityView(shell,Automation::Peers::AccessibilityView::Raw);
            AutomationProperties::SetAccessibilityView(percent,Automation::Peers::AccessibilityView::Raw);
            root.Visibility(Visibility::Collapsed);
            timer=root.DispatcherQueue().CreateTimer();timer.IsRepeating(false);
            timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
            root.SizeChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&self->changed)self->changed();});
        }
        void refresh(){
            using namespace CapyUi;
            if(!visible){timer.Stop();return;}
            wchar_t text[128]{};
            if(GetTimeFormatEx(LOCALE_NAME_USER_DEFAULT,TIME_NOSECONDS,nullptr,nullptr,text,128)){
                if(clock.Text()!=text)clock.Text(text);
            }
            SYSTEM_POWER_STATUS power{};
            bool known=GetSystemPowerStatus(&power)&&!(power.BatteryFlag&128)&&power.BatteryLifePercent<=100;
            batteryTile.Visibility(known?Visibility::Visible:Visibility::Collapsed);
            if(known){
                int level=power.BatteryLifePercent;bool charging=(power.BatteryFlag&8)!=0,low=level<=15&&!charging;
                bool light=data->theme()==L"light";
                auto track=color(charging?L"#c4c9cf":light?L"#707479":L"#a3a8b0");
                auto ink=color(charging?L"#13251a":light?L"#ffffff":L"#202226");
                auto paint=color(charging?L"#91b89d":low?(light?L"#a15d59":L"#bc9996"):light?L"#3f4246":L"#e5e7eb");
                shell.Background(fill(track));cap.Background(fill(track));charge.Background(fill(paint));charge.Width(22.*level/100.);
                percent.Text(to_hstring(level));percent.FontSize(level==100?10:11);percent.Foreground(fill(ink));
                cap.Visibility(charging?Visibility::Collapsed:Visibility::Visible);bolt.Visibility(charging?Visibility::Visible:Visibility::Collapsed);
                bolt.Fill(fill(light?ink:color(L"#e5e7eb")));bolt.Stroke(fill(light?track:color(L"#202226")));
                auto description=L"Battery "+to_hstring(level)+L"%"+(charging?L", charging":low?L", low":L"");
                AutomationProperties::SetName(batteryTile,description);ToolTipService::SetToolTip(batteryTile,box_value(description));
            }
            clock.Foreground(data->brush(L"text"));
            SYSTEMTIME now{};GetLocalTime(&now);
            auto untilMinute=60000-int(now.wSecond)*1000-int(now.wMilliseconds);
            timer.Interval(std::chrono::milliseconds(std::max(20,std::min(15000,untilMinute+20))));
            timer.Start();
        }
        void apply(bool fullscreen,bool hidden,double gap){
            using namespace CapyUi;
            auto policy=str(object(data->state,L"settings"),L"show_clock",L"fullscreen");
            bool next=!hidden&&(policy==L"always"||(policy==L"fullscreen"&&fullscreen));
            root.Spacing(gap);root.Visibility(next?Visibility::Visible:Visibility::Collapsed);
            if(next!=visible){visible=next;refresh();}
        }
    };
    std::shared_ptr<Impl> impl;
public:
    HeaderStatus(std::shared_ptr<CapyUi::WorkspaceData> data,std::function<void()> changed):impl(std::make_shared<Impl>()){
        impl->data=std::move(data);impl->changed=std::move(changed);impl->init();
    }
    auto Root()const{return impl->root;}
    void Apply(bool fullscreen,bool hidden,double gap){impl->apply(fullscreen,hidden,gap);}
};
