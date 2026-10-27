#pragma once
#include "ControlContext.h"
#include "NavigatorView.h"
#include <chrono>
#include <fstream>
#include <limits>
struct NavigatorLayoutFixture:std::enable_shared_from_this<NavigatorLayoutFixture>{
    ControlContext context;
    std::shared_ptr<CapyUi::WorkspaceData> data=std::make_shared<CapyUi::WorkspaceData>();
    std::unique_ptr<NavigatorView> view;
    winrt::Microsoft::UI::Xaml::Window window{nullptr};
    winrt::Microsoft::UI::Xaml::Controls::StackPanel host;
    winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer timeout{nullptr};
    CapyUi::A samples;
    unsigned phase=0,events=0,callbacks=0;
    bool pending=false,finished=false,passed=false;
    std::string error;
    explicit NavigatorLayoutFixture(ControlContext source):context(std::move(source)){}
    static winrt::Microsoft::UI::Xaml::FrameworkElement find(winrt::Microsoft::UI::Xaml::DependencyObject const& parent,winrt::hstring const& id){
        using namespace winrt::Microsoft::UI::Xaml;
        if(auto element=parent.try_as<FrameworkElement>();element&&Automation::AutomationProperties::GetAutomationId(element)==id)return element;
        for(int i=0;i<Media::VisualTreeHelper::GetChildrenCount(parent);++i)if(auto found=find(Media::VisualTreeHelper::GetChild(parent,i),id))return found;
        return nullptr;
    }
    static CapyUi::A rectangle(winrt::Windows::Foundation::Rect bounds){
        CapyUi::A result;for(double value:{bounds.X,bounds.Y,bounds.Width,bounds.Height})result.Append(CapyUi::N(value));return result;
    }
    void finish(bool success,std::string message={}){
        if(finished)return;finished=true;passed=success;error=std::move(message);
        if(timeout)timeout.Stop();
        host.DispatcherQueue().TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock()){self->window.Close();winrt::Microsoft::UI::Xaml::Application::Current().Exit();}});
    }
    void size(){
        auto root=view->Root();
        root.Width(phase==2?241.+1./3:242.);
        root.Height(phase==0?std::numeric_limits<double>::quiet_NaN():phase==1?82.:118.+2./3);
    }
    void observe(wchar_t const* event){
        using namespace CapyUi;using namespace winrt::Microsoft::UI::Xaml;
        ++events;if(finished||pending)return;
        auto root=view->Root(),overview=find(root,L"navigator-overview");
        if(!root.IsLoaded()||!overview||root.ActualWidth()<=0||root.ActualHeight()<=0||overview.ActualWidth()<=0||overview.ActualHeight()<=0)return;
        double targetWidth=phase==2?241.+1./3:242.,targetHeight=phase==0?view->NaturalHeight(targetWidth):phase==1?82.:118.+2./3;
        if(std::abs(root.ActualWidth()-targetWidth)>.75||(phase&&std::abs(root.ActualHeight()-targetHeight)>.75))return;
        auto bounds=overview.TransformToVisual(root).TransformBounds({0,0,float(overview.ActualWidth()),float(overview.ActualHeight())});
        double inset=data->panelMetric(L"inset"),gap=data->panelMetric(L"control_spacing"),height=data->panelMetric(L"control_height");
        double natural=std::max(0.,root.ActualWidth()-2*inset)*capy_navigator_aspect(2005,1504);
        double expected=std::clamp(root.ActualHeight()-view->FixedHeight(),0.,natural);
        bool valid=std::abs(root.ActualHeight()-targetHeight)<=.75&&std::abs(bounds.X-inset)<=.75&&std::abs(bounds.Y-inset)<=.75
            &&std::abs(bounds.Width-(root.ActualWidth()-2*inset))<=.75&&std::abs(bounds.Height-expected)<=.75;
        A buttons;
        for(auto id:{L"zoom_out",L"zoom_in",L"rotate_left",L"rotate_right",L"flip_horizontal",L"flip_vertical"}){
            auto button=find(root,L"navigator-"+winrt::hstring(id));if(!button||button.ActualHeight()<=0)return;
            auto box=button.TransformToVisual(root).TransformBounds({0,0,float(button.ActualWidth()),float(button.ActualHeight())});
            valid=valid&&std::abs(box.Y-(inset+expected+gap))<=.75&&std::abs(box.Height-height)<=.75
                &&box.X>=inset-.75&&box.X+box.Width<=root.ActualWidth()-inset+.75&&box.Y+box.Height<=root.ActualHeight()-inset+.75;
            buttons.Append(rectangle(box));
        }
        samples.Append(O({{L"phase",N(phase)},{L"event",S(event)},{L"event_index",N(events)},{L"root_width",N(root.ActualWidth())},{L"root_height",N(root.ActualHeight())},{L"expected_root_height",N(targetHeight)},
            {L"overview",rectangle(bounds)},{L"expected_image_height",N(expected)},{L"actions",buttons},{L"passed",B(valid)}}));
        if(!valid){finish(false,"First allocated Navigator geometry does not fit");return;}
        if(phase==2){finish(true);return;}
        pending=true;
        if(!host.DispatcherQueue().TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock()){++self->phase;self->pending=false;self->size();}}))finish(false,"Phase enqueue rejected");
    }
    void open(){
        using namespace CapyUi;using namespace winrt::Microsoft::UI::Xaml;
        data->localization=context.localization;data->catalog=O({{L"bootstrap",J::Parse(winrt::to_hstring(context.bootstrap))},{L"panel_metrics",O({{L"inset",N(8)},{L"control_spacing",N(4)},{L"control_height",N(26)},{L"control_radius",N(6)}})}});
        A ids,commands;for(auto id:{L"zoom_out",L"zoom_in",L"rotate_left",L"rotate_right",L"flip_horizontal",L"flip_vertical"}){ids.Append(S(id));commands.Append(O({{L"id",S(id)},{L"label",S(id)},{L"enabled",B(true)}}));}
        data->catalog.Insert(L"navigator_commands",ids);A tabs;tabs.Append(O({{L"width",N(2005)},{L"height",N(1504)}}));
        data->state=O({{L"theme",S(L"dark")},{L"palette",O({{L"bg",S(L"#323232")},{L"text",S(L"#eeeeee")}})},{L"tabs",tabs},{L"commands",commands}});data->send=[](auto&&){};
        view=std::make_unique<NavigatorView>(data,[weak=weak_from_this()]{if(auto self=weak.lock())++self->callbacks;});
        auto root=view->Root();root.HorizontalAlignment(HorizontalAlignment::Left);root.VerticalAlignment(VerticalAlignment::Top);size();
        root.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->observe(L"Loaded");});
        root.LayoutUpdated([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->observe(L"LayoutUpdated");});
        host.HorizontalAlignment(HorizontalAlignment::Left);host.VerticalAlignment(VerticalAlignment::Top);host.Margin({32,32,0,0});host.Children().Append(root);
        window=Window();window.Content(host);window.AppWindow().Move({40,40});window.AppWindow().Resize({800,700});window.Activate();
        timeout=host.DispatcherQueue().CreateTimer();timeout.Interval(std::chrono::seconds(10));timeout.IsRepeating(false);timeout.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->finish(false,"First-allocation event timed out");});timeout.Start();
    }
    int write()const{
        using namespace CapyUi;
        wchar_t path[32768]{};auto length=GetEnvironmentVariableW(L"CAPY_NAVIGATOR_LAYOUT",path,32768);if(!length||length>=32768)return 1;
        std::ofstream output{std::filesystem::path(path)};output<<winrt::to_string(O({{L"passed",B(passed)},{L"error",S(winrt::to_hstring(error))},{L"samples",samples},{L"events",N(events)},{L"geometry_callbacks",N(callbacks)}}).Stringify());
        return output&&passed?0:1;
    }
};
