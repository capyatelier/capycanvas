#include "pch.h"
#include "ButtonFeedback.h"
#include "SdkFeedbackFixture.h"
#include "NavigatorLayoutFixture.h"
#include "ScrollIndicatorsFixture.h"
#include <winrt/Microsoft.UI.Xaml.Automation.h>
#include <chrono>
#include <string_view>
#include <vector>
using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Media;
namespace {
std::shared_ptr<NavigatorLayoutFixture> navigatorFixture;
struct App:ApplicationT<App,Markup::IXamlMetadataProvider>{
    XamlTypeInfo::XamlControlsXamlMetaDataProvider metadata;
    Window window{nullptr};Grid root;ContentDialog dialog{nullptr};Flyout flyout;
    std::vector<Button> probes;unsigned clicks=0;
    std::shared_ptr<SdkFeedbackFixture> sdk;
    std::shared_ptr<ScrollIndicatorsFixture> indicators;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    SolidColorBrush ink{Windows::UI::Color{255,30,30,30}};
    ControlContext context;
    explicit App(ControlContext source):context(std::move(source)){}
    Markup::IXamlType GetXamlType(Windows::UI::Xaml::Interop::TypeName const& type){return metadata.GetXamlType(type);}
    Markup::IXamlType GetXamlType(hstring const& name){return metadata.GetXamlType(name);}
    com_array<Markup::XmlnsDefinition> GetXmlnsDefinitions(){return metadata.GetXmlnsDefinitions();}
    Button probe(hstring const& id){
        Button button;button.Width(220);button.Height(64);button.Content(box_value(id));
        button.Background(SolidColorBrush(Windows::UI::Color{255,237,237,237}));button.Foreground(ink);
        Automation::AutomationProperties::SetAutomationId(button,id);CapyUi::buttonFeedback(button,ink);
        button.Click([this](auto&&,auto&&){++clicks;});probes.push_back(button);return button;
    }
    void status(){
        for(auto const& button:probes){auto state=CapyUi::feedback::state(button);
            Automation::AutomationProperties::SetItemStatus(button,
                L"contact="+to_hstring(bool(state->pointer))+L";alpha="+to_hstring(state->overlay.Color().A)+L";clicks="+to_hstring(clicks));
        }
    }
    fire_and_forget showDialog(){
        dialog=ContentDialog();dialog.XamlRoot(root.XamlRoot());dialog.Title(box_value(L"Button feedback"));
        StackPanel body;body.Spacing(24);body.Children().Append(probe(L"feedback-dialog"));
        Button close;close.Content(box_value(L"Close"));Automation::AutomationProperties::SetAutomationId(close,L"feedback-close-dialog");
        close.Click([this](auto&&,auto&&){dialog.Hide();});body.Children().Append(close);dialog.Content(body);co_await dialog.ShowAsync();
    }
    void OnLaunched(LaunchActivatedEventArgs const&){
        Resources().MergedDictionaries().Append(XamlControlsResources());Resources().MergedDictionaries().Append(CapyUi::paletteResources());
        if(GetEnvironmentVariableW(L"CAPY_SCROLL_INDICATORS",nullptr,0)){indicators=std::make_shared<ScrollIndicatorsFixture>();indicators->open();return;}
        if(GetEnvironmentVariableW(L"CAPY_NAVIGATOR_LAYOUT",nullptr,0)){navigatorFixture=std::make_shared<NavigatorLayoutFixture>(context);navigatorFixture->open();return;}
        if(GetEnvironmentVariableW(L"CAPY_FEEDBACK_SDK",nullptr,0)){sdk=std::make_shared<SdkFeedbackFixture>();sdk->open();return;}
        wchar_t theme[16]{};GetEnvironmentVariableW(L"CAPY_FEEDBACK_THEME",theme,16);
        root.RequestedTheme(std::wstring_view(theme)==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        root.Background(SolidColorBrush(Windows::UI::Color{255,237,237,237}));
        StackPanel body;body.Width(460);body.Spacing(28);body.Margin({80,80,0,0});
        body.HorizontalAlignment(HorizontalAlignment::Left);body.VerticalAlignment(VerticalAlignment::Top);
        body.Children().Append(probe(L"feedback-root"));
        Button modal;modal.Content(box_value(L"Show dialog"));Automation::AutomationProperties::SetAutomationId(modal,L"feedback-show-dialog");
        modal.Click([this](auto&&,auto&&){showDialog();});body.Children().Append(modal);
        StackPanel popup;popup.Spacing(24);popup.Children().Append(probe(L"feedback-flyout"));
        Button hide;hide.Content(box_value(L"Close"));Automation::AutomationProperties::SetAutomationId(hide,L"feedback-hide-flyout");
        hide.Click([this](auto&&,auto&&){flyout.Hide();});popup.Children().Append(hide);flyout.Content(popup);
        Button anchor;anchor.Content(box_value(L"Show flyout"));anchor.Flyout(flyout);
        Automation::AutomationProperties::SetAutomationId(anchor,L"feedback-show-flyout");body.Children().Append(anchor);
        root.Children().Append(body);window=Window();window.Content(root);window.Activate();
        timer=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread().CreateTimer();
        timer.Interval(std::chrono::milliseconds(30));timer.Tick([this](auto&&,auto&&){status();});timer.Start();
    }
};
}
int WINAPI wWinMain(HINSTANCE,HINSTANCE,PWSTR,int){
    ControlContext context;if(GetEnvironmentVariableW(L"CAPY_NAVIGATOR_LAYOUT",nullptr,0))context=fixtureContext(L"CAPY_NAVIGATOR_LAYOUT");
    init_apartment(apartment_type::single_threaded);Application::Start([context](auto&&){make<App>(context);});return navigatorFixture?navigatorFixture->write():0;
}
