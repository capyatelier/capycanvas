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
                L"contact="+to_hstring(bool(state->pointer))+L";alpha="+to_hstring(state->overlay.Color().A)+L";clicks="+to_hstring(clicks)+L";content_opacity="+to_hstring(VisualTreeHelper::GetChildrenCount(button)?VisualTreeHelper::GetChild(button,0).as<Grid>().Children().GetAt(2).as<ContentPresenter>().Opacity():-1)+L";opacity="+to_hstring(button.Opacity()));
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
        auto data=std::make_shared<CapyUi::WorkspaceData>();data->state=CapyUi::O({{L"palette",CapyUi::O({{L"text",CapyUi::S(L"#1e1e1e")},{L"accent",CapyUi::S(L"#3584e4")}})}});
        StackPanel disabled;disabled.Orientation(Orientation::Horizontal);disabled.Spacing(20);
        for(auto custom:{false,true}){auto button=probe(custom?L"feedback-disabled-custom":L"feedback-disabled-default");button.Height(20);if(custom)CapyUi::buttonColors(data,button,20,nullptr,.45);button.Opacity(.55);button.IsEnabled(false);disabled.Children().Append(button);}body.Children().Append(disabled);
        auto wrapped=CapyUi::wrapPanel(40,32,4);
        for(int i=0;i<5;++i){Button item;item.MinWidth(0);item.HorizontalAlignment(HorizontalAlignment::Left);item.Content(box_value(L"x"));wrapped.Children().Append(item);}
        wrapped.Measure({88,std::numeric_limits<float>::infinity()});auto narrow=wrapped.DesiredSize().Height;wrapped.Measure({160,std::numeric_limits<float>::infinity()});
        if(wrapped.DesiredSize().Height<=0||narrow<=wrapped.DesiredSize().Height)throw hresult_error(E_FAIL,L"Offscreen wrap panel did not measure its rows from the available width");
        wrapped.Arrange({0,0,160,wrapped.DesiredSize().Height});
        for(auto item:wrapped.Children())if(std::abs(item.as<FrameworkElement>().ActualWidth()-40)>.01)throw hresult_error(E_FAIL,L"Wrapped button did not fill its declared cell width");
        auto natural=CapyUi::wrapPanel(0,26,4);for(auto width:{50.,80.}){Border cell;cell.Width(width);natural.Children().Append(cell);}
        natural.Measure({std::numeric_limits<float>::infinity(),std::numeric_limits<float>::infinity()});auto wide=natural.DesiredSize();natural.Measure({90,std::numeric_limits<float>::infinity()});
        if(wide.Width<130||natural.DesiredSize().Height<=wide.Height)throw hresult_error(E_FAIL,L"Natural wrap panel lost its caption width or wrapped-row height");
        Button modal;modal.Content(box_value(L"Show dialog"));Automation::AutomationProperties::SetAutomationId(modal,L"feedback-show-dialog");
        modal.Click([this](auto&&,auto&&){showDialog();});body.Children().Append(modal);
        StackPanel popup;popup.Spacing(24);popup.Children().Append(probe(L"feedback-flyout"));
        Button hide;hide.Content(box_value(L"Close"));Automation::AutomationProperties::SetAutomationId(hide,L"feedback-hide-flyout");
        hide.Click([this](auto&&,auto&&){flyout.Hide();});popup.Children().Append(hide);flyout.Content(popup);
        Button anchor;anchor.Content(box_value(L"Show flyout"));anchor.Flyout(flyout);
        Automation::AutomationProperties::SetAutomationId(anchor,L"feedback-show-flyout");body.Children().Append(anchor);
        root.Children().Append(body);window=Window();window.Content(root);window.AppWindow().Move({40,40});window.AppWindow().Resize({1200,1100});window.Activate();
        timer=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread().CreateTimer();
        timer.Interval(std::chrono::milliseconds(30));timer.Tick([this](auto&&,auto&&){status();});timer.Start();
    }
};
}
int WINAPI wWinMain(HINSTANCE,HINSTANCE,PWSTR,int){
    ControlContext context;if(GetEnvironmentVariableW(L"CAPY_NAVIGATOR_LAYOUT",nullptr,0))context=fixtureContext(L"CAPY_NAVIGATOR_LAYOUT");
    init_apartment(apartment_type::single_threaded);Application::Start([context](auto&&){make<App>(context);});return navigatorFixture?navigatorFixture->write():0;
}
