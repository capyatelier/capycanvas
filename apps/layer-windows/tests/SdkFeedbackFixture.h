#pragma once
#include "UiControls.h"
#include "ChoiceFeedback.h"
#include <winrt/Microsoft.UI.Xaml.Automation.h>
#include <chrono>
struct SdkFeedbackFixture {
    using J=CapyUi::J;
    std::shared_ptr<CapyUi::WorkspaceData> data=std::make_shared<CapyUi::WorkspaceData>();
    winrt::Microsoft::UI::Xaml::Window window{nullptr};
    winrt::Microsoft::UI::Xaml::Controls::StackPanel root;
    winrt::Microsoft::UI::Xaml::Controls::ComboBox choice,modalChoice;
    winrt::Microsoft::UI::Xaml::Controls::ContentDialog dialog{nullptr};
    winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::map<void*,winrt::Microsoft::UI::Xaml::Controls::ControlTemplate> templates;
    std::map<void*,std::vector<std::pair<winrt::hstring,winrt::Windows::Foundation::IInspectable>>> resources;
    bool dark=false,drawing=false,disabled=false,appPaletteOwned=false,appPaletteStable=false;unsigned primary=0,secondary=0,selections=0;
    static winrt::Microsoft::UI::Xaml::FrameworkElement part(winrt::Microsoft::UI::Xaml::Controls::Control const& control,wchar_t const* name){
        using namespace winrt::Microsoft::UI::Xaml;
        if(!Media::VisualTreeHelper::GetChildrenCount(control))return nullptr;
        auto child=Media::VisualTreeHelper::GetChild(control,0).try_as<FrameworkElement>();return child?child.FindName(name).try_as<FrameworkElement>():nullptr;
    }
    static CapyUi::A rgba(winrt::Microsoft::UI::Xaml::Media::Brush const& brush){
        CapyUi::A result;auto solid=brush.try_as<winrt::Microsoft::UI::Xaml::Media::SolidColorBrush>();auto c=solid?solid.Color():winrt::Windows::UI::Color{};
        for(auto value:{c.A,c.R,c.G,c.B})result.Append(CapyUi::N(value));return result;
    }
    winrt::Microsoft::UI::Xaml::Controls::Button action(wchar_t const* id,wchar_t const* text,std::function<void()> call){
        using namespace winrt::Microsoft::UI::Xaml;Controls::Button button;button.Content(winrt::box_value(text));Automation::AutomationProperties::SetAutomationId(button,id);
        button.Click([call](auto&&,auto&&){call();});return button;
    }
    void palette(){
        using namespace CapyUi;using namespace winrt::Microsoft::UI::Xaml;
        auto colors=O({{L"input",S(dark?L"#263445":L"#e8eef4")},{L"text",S(dark?L"#e1ebf5":L"#24384c")},{L"settings",S(dark?L"#182432":L"#f4f4f4")},{L"control_fill",S(dark?L"#344354":L"#dde3e9")},{L"accent",S(dark?L"#5486b9":L"#2a6db0")},{L"accent_foreground",S(dark?L"#101820":L"#f8fcff")}});
        A selection;for(double value:{.3,.4,.5,.6})selection.Append(N(value));
        colors.Insert(L"selection",S(dark?L"#516078":L"#b8c3d0"));colors.Insert(L"glass",O({{L"selection",selection},{L"transparency",S(L"low")}}));
        data->state=O({{L"theme",S(dark?L"dark":L"light")},{L"palette",colors}});data->refreshPalette();
        appPaletteOwned=false;for(auto const& entry:data->controlResources)if(unbox_value<hstring>(entry.Key())==L"ComboBoxItemForeground")appPaletteOwned=true;
        auto chosen=data->controlResources.Lookup(box_value(L"ComboBoxItemBackgroundSelected")).as<Media::SolidColorBrush>().Color();
        WorkspaceData consumer;consumer.state=data->state;consumer.catalog=data->catalog;consumer.glassSurfaces=!data->glassSurfaces;consumer.refreshPalette();
        appPaletteStable=data->controlResources.Lookup(box_value(L"ComboBoxItemBackgroundSelected")).as<Media::SolidColorBrush>().Color()==chosen;
        root.RequestedTheme(dark?ElementTheme::Dark:ElementTheme::Light);root.Background(data->brush(L"settings"));
        for(auto control:{choice,modalChoice}){CapyUi::choiceColors(data,control,drawing?ControlSurface::Drawing:ControlSurface::Panel);control.IsEnabled(!disabled);}
        if(dialog){dialog.RequestedTheme(root.RequestedTheme());dialog.Background(data->brush(L"settings"));dialog.IsPrimaryButtonEnabled(!disabled);dialog.IsSecondaryButtonEnabled(!disabled);CapyUi::dialogButtonColors(data,dialog);}
    }
    void actions(winrt::Microsoft::UI::Xaml::Controls::StackPanel const& panel,winrt::hstring const& prefix){
        panel.Children().Append(action((prefix+L"theme").c_str(),L"Change theme",[this]{dark=!dark;palette();}));
        panel.Children().Append(action((prefix+L"role").c_str(),L"Change surface",[this]{drawing=!drawing;palette();}));
        panel.Children().Append(action((prefix+L"disable").c_str(),L"Disable controls",[this]{disabled=!disabled;palette();}));
    }
    void initChoice(winrt::Microsoft::UI::Xaml::Controls::ComboBox const& control,wchar_t const* id){
        using namespace winrt::Microsoft::UI::Xaml;control.Width(280);control.MinHeight(40);
        Automation::AutomationProperties::SetAutomationId(control,id);for(auto text:{L"One",L"Two",L"Three"})control.Items().Append(winrt::box_value(text));control.SelectedIndex(0);
        control.SelectionChanged([this](auto&&,auto&&){++selections;});
    }
    winrt::fire_and_forget show(){
        using namespace winrt::Microsoft::UI::Xaml;
        if(!dialog){dialog=Controls::ContentDialog();dialog.XamlRoot(root.XamlRoot());dialog.Title(winrt::box_value(L"SDK feedback"));dialog.PrimaryButtonText(L"Apply");dialog.SecondaryButtonText(L"Other");dialog.CloseButtonText(L"Close");dialog.DefaultButton(Controls::ContentDialogButton::Primary);
            Controls::StackPanel body;body.Spacing(12);body.Children().Append(modalChoice);Controls::TextBox input;input.PlaceholderText(L"Keyboard default");Automation::AutomationProperties::SetAutomationId(input,L"sdk-dialog-input");body.Children().Append(input);actions(body,L"sdk-dialog-");
            body.Children().Append(action(L"sdk-default",L"Change default",[this]{bool alternate=dialog.DefaultButton()==Controls::ContentDialogButton::Primary;dialog.PrimaryButtonText(alternate?L"Apply now":L"Apply");dialog.SecondaryButtonText(alternate?L"Other now":L"Other");dialog.CloseButtonText(alternate?L"Close now":L"Close");dialog.DefaultButton(alternate?Controls::ContentDialogButton::Secondary:Controls::ContentDialogButton::Primary);}));dialog.Content(body);
            dialog.PrimaryButtonClick([this](auto&&,Controls::ContentDialogButtonClickEventArgs const& event){++primary;event.Cancel(true);});dialog.SecondaryButtonClick([this](auto&&,Controls::ContentDialogButtonClickEventArgs const& event){++secondary;event.Cancel(true);});}
        palette();co_await dialog.ShowAsync();
    }
    void status(){
        using namespace CapyUi;using namespace winrt::Microsoft::UI::Xaml;
        for(auto control:{choice,modalChoice}){
            auto background=part(control,L"Background").try_as<Controls::Border>();if(!background)continue;auto key=winrt::get_abi(control);
            if(!templates.contains(key)){templates.emplace(key,control.Template());for(auto entry:control.Resources())if(entry.Value().try_as<Media::SolidColorBrush>())resources[key].emplace_back(winrt::unbox_value<winrt::hstring>(entry.Key()),entry.Value());}
            bool stable=true;for(auto const& [name,value]:resources[key])stable&=control.Resources().HasKey(winrt::box_value(name))&&control.Resources().Lookup(winrt::box_value(name))==value;
            winrt::hstring common,focus;auto layout=Media::VisualTreeHelper::GetChild(control,0).as<FrameworkElement>();for(auto group:VisualStateManager::GetVisualStateGroups(layout))if(group.CurrentState()){if(group.Name()==L"CommonStates")common=group.CurrentState().Name();if(group.Name()==L"FocusStates")focus=group.CurrentState().Name();}
            auto glyph=part(control,L"DropDownGlyph").try_as<Controls::IconElement>();auto presenter=part(control,L"ContentPresenter");
            auto value=O({{L"app_palette_owned",B(appPaletteOwned)},{L"app_palette_stable",B(appPaletteStable)},{L"app_foreground",rgba(data->controlResources.Lookup(box_value(L"ComboBoxItemForeground")).as<Media::Brush>())},{L"common",S(common)},{L"focus",S(focus)},{L"enabled",B(control.IsEnabled())},{L"open",B(control.IsDropDownOpen())},{L"selected",N(control.SelectedIndex())},{L"selections",N(selections)},{L"template_retained",B(control.Template()==templates[key]&&layout.Name()==L"LayoutRoot")},{L"resources_retained",B(stable)},{L"base",rgba(control.Background())},{L"ink",rgba(control.Foreground())},{L"background",rgba(background.Background())},{L"glyph",glyph?rgba(glyph.Foreground()):A{}},{L"content_opacity",N(presenter?presenter.Opacity():-1)},{L"dark",B(dark)},{L"drawing",B(drawing)},{L"primary",N(primary)},{L"secondary",N(secondary)}});
            Automation::AutomationProperties::SetItemStatus(control,value.Stringify());
        }
        if(dialog)for(auto const& [name,id]:{std::pair{L"PrimaryButton",L"sdk-footer-primary"},std::pair{L"SecondaryButton",L"sdk-footer-secondary"},std::pair{L"CloseButton",L"sdk-footer-close"}}){
            auto button=part(dialog,name).try_as<Controls::Button>();if(!button)continue;Automation::AutomationProperties::SetAutomationId(button,id);bool bound=button.Template()==feedback::buttonTemplate();auto state=bound?feedback::state(button):nullptr;auto content=part(button,L"ContentPresenter");
            auto caption=std::wstring_view(name)==L"PrimaryButton"?dialog.PrimaryButtonText():std::wstring_view(name)==L"SecondaryButton"?dialog.SecondaryButtonText():dialog.CloseButtonText();
            auto value=O({{L"expected_caption",S(caption)},{L"contact",B(state&&bool(state->pointer))},{L"alpha",N(state?int(state->overlay.Color().A):-1)},{L"enabled",B(button.IsEnabled())},{L"background",rgba(button.Background())},{L"ink",rgba(button.Foreground())},{L"content_opacity",N(content?content.Opacity():-1)},{L"template",B(bound)},{L"default",N(int(dialog.DefaultButton()))},{L"primary",N(primary)},{L"secondary",N(secondary)}});Automation::AutomationProperties::SetItemStatus(button,value.Stringify());
        }
    }
    void open(){
        using namespace CapyUi;using namespace winrt::Microsoft::UI::Xaml;
        wchar_t theme[16]{};GetEnvironmentVariableW(L"CAPY_FEEDBACK_THEME",theme,16);dark=std::wstring_view(theme)==L"dark";
        data->catalog=O({{L"panel_metrics",O({{L"control_radius",N(6)},{L"control_height",N(26)},{L"field_padding",N(6)}})}});initChoice(choice,L"sdk-choice-root");initChoice(modalChoice,L"sdk-choice-dialog");root.Spacing(14);root.Padding({48,48,48,48});root.Children().Append(choice);root.Children().Append(action(L"sdk-show",L"Show dialog",[this]{show();}));actions(root,L"sdk-root-");palette();
        window=Window();window.Content(root);window.AppWindow().Move({40,40});window.AppWindow().Resize({1000,1000});window.Activate();
        timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(30));timer.Tick([this](auto&&,auto&&){status();});timer.Start();
    }
};
