#pragma once
#include "pch.h"
#include "PointerGesture.h"
#include <map>
#include <memory>
#include <optional>

namespace CapyUi {
namespace feedback {
using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Media;
using namespace Microsoft::UI::Xaml::Input;
struct State;
struct Contacts {
    std::map<uint32_t,std::weak_ptr<State>> owners;
    void end(PointerRoutedEventArgs const& event,PointerCompletion completion);
};
inline std::shared_ptr<Contacts> contacts(UIElement const& root){
    if(!root)return nullptr;
    static thread_local std::map<void*,std::weak_ptr<Contacts>> values;
    auto key=get_abi(root);auto found=values.find(key);
    if(found!=values.end())if(auto value=found->second.lock())return value;
    std::erase_if(values,[](auto const& pair){return pair.second.expired();});
    auto value=std::make_shared<Contacts>();values[key]=value;
    for(auto const& [event,completion]:{std::pair{UIElement::PointerReleasedEvent(),PointerCompletion::Released},std::pair{UIElement::PointerCanceledEvent(),PointerCompletion::Canceled},std::pair{UIElement::PointerCaptureLostEvent(),PointerCompletion::CaptureLost}})
        root.AddHandler(event,box_value(PointerEventHandler([value,completion](auto&&,PointerRoutedEventArgs const& e){value->end(e,completion);})),true);
    return value;
}
inline ControlTemplate buttonTemplate(){
    static thread_local auto value=Markup::XamlReader::Load(
        LR"(<ControlTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" xmlns:p="using:Microsoft.UI.Xaml.Controls.Primitives" TargetType="p:ButtonBase"><Grid x:Name="CapyButtonFeedback"><Border Background="{TemplateBinding Background}" BorderBrush="{TemplateBinding BorderBrush}" BorderThickness="{TemplateBinding BorderThickness}" CornerRadius="{TemplateBinding CornerRadius}"/><Border IsHitTestVisible="False" CornerRadius="{TemplateBinding CornerRadius}"/><ContentPresenter x:Name="ContentPresenter" Content="{TemplateBinding Content}" ContentTemplate="{TemplateBinding ContentTemplate}" Padding="{TemplateBinding Padding}" HorizontalContentAlignment="{TemplateBinding HorizontalContentAlignment}" VerticalContentAlignment="{TemplateBinding VerticalContentAlignment}" Foreground="{TemplateBinding Foreground}"/></Grid></ControlTemplate>)").as<ControlTemplate>();
    return value;
}
struct State:std::enable_shared_from_this<State> {
    weak_ref<Primitives::ButtonBase> owner;
    SolidColorBrush overlay;
    SolidColorBrush ink{nullptr};int64_t inkChanged=0;
    std::shared_ptr<Contacts> input;
    std::optional<uint32_t> pointer;
    bool inside=false,hoverAllowed=true,ignoreNativePress=false;
    uint8_t hover=20,press=41;
    Brush hoverSurface{nullptr},checkedSurface{nullptr};
    bool fixed=false,external=false;
    ~State(){if(ink)ink.UnregisterPropertyChangedCallback(SolidColorBrush::ColorProperty(),inkChanged);}
    void source(SolidColorBrush const& next){
        if(ink==next)return;
        if(ink)ink.UnregisterPropertyChangedCallback(SolidColorBrush::ColorProperty(),inkChanged);
        ink=next;
        if(ink)inkChanged=ink.RegisterPropertyChangedCallback(SolidColorBrush::ColorProperty(),[weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->present();});
    }
    bool hit(PointerRoutedEventArgs const& event)const{
        auto button=owner.get();return button&&!PointerGesture::Outside(button,event);
    }
    void end(bool clearHover){
        if(pointer&&input)input->owners.erase(*pointer);
        pointer.reset();inside=false;if(clearHover)hoverAllowed=false;
        if(auto button=owner.get())ignoreNativePress=button.IsPressed();present();
    }
    void complete(PointerRoutedEventArgs const& event,PointerCompletion completion){
        if(pointer!=event.Pointer().PointerId())return;
        auto point=event.GetCurrentPoint(nullptr);
        end(CanceledCompletion(completion,point.Properties().IsCanceled(),point.IsInContact(),PointerCaptureLoss::CompleteLift)
            ||point.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch||!hit(event));
    }
    void present(){
        auto button=owner.get();if(!button)return;
        if(!button.IsEnabled()&&pointer){end(true);return;}
        bool pressed=button.IsEnabled()&&(pointer?inside:!ignoreNativePress&&button.IsPressed());
        bool over=button.IsEnabled()&&hoverAllowed&&button.IsPointerOver();
        auto color=ink?ink.Color():Windows::UI::Color{};color.A=fixed?0:pressed?press:over?hover:0;overlay.Color(color);
        if(button.Template()!=buttonTemplate()||!VisualTreeHelper::GetChildrenCount(button))return;
        auto root=VisualTreeHelper::GetChild(button,0).try_as<Grid>();
        if(!root||root.Name()!=L"CapyButtonFeedback"||root.Children().Size()!=3)return;
        auto background=button.Background();
        if(auto toggle=button.try_as<Primitives::ToggleButton>();toggle&&checkedSurface&&toggle.IsChecked()&&toggle.IsChecked().Value())background=checkedSurface;
        if(over&&!pressed&&hoverSurface)background=hoverSurface;
        root.Children().GetAt(0).as<Border>().Background(background);
        root.Children().GetAt(1).as<Border>().Background(external?Brush(nullptr):overlay);
        root.Children().GetAt(2).as<ContentPresenter>().Opacity(button.IsEnabled()||button.Opacity()<=0?1.:std::min(1.,(92./255)/button.Opacity()));
    }
};
inline void Contacts::end(PointerRoutedEventArgs const& event,PointerCompletion completion){
    auto found=owners.find(event.Pointer().PointerId());if(found==owners.end())return;
    auto state=found->second.lock();owners.erase(found);if(!state)return;
    state->complete(event,completion);
}
inline std::shared_ptr<State> state(Primitives::ButtonBase const& button){
    static thread_local std::map<void*,std::weak_ptr<State>> values;
    auto key=get_abi(button);auto found=values.find(key);
    if(found!=values.end())if(auto state=found->second.lock())return state;
    if(values.size()%64==0)std::erase_if(values,[](auto const& pair){return pair.second.expired();});
    auto state=std::make_shared<State>();state->owner=make_weak(button);values[key]=state;
    button.Template(buttonTemplate());button.UseSystemFocusVisuals(true);
    auto changed=[state](DependencyObject const&,DependencyProperty const& property){if(property==Primitives::ButtonBase::IsPressedProperty())if(auto owner=state->owner.get();owner&&!owner.IsPressed())state->ignoreNativePress=false;state->present();};
    for(auto property:{Primitives::ButtonBase::IsPointerOverProperty(),Primitives::ButtonBase::IsPressedProperty(),Control::IsEnabledProperty(),Control::BackgroundProperty(),UIElement::OpacityProperty()})
        button.RegisterPropertyChangedCallback(property,changed);
    if(button.try_as<Primitives::ToggleButton>())button.RegisterPropertyChangedCallback(Primitives::ToggleButton::IsCheckedProperty(),changed);
    button.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([state](auto&&,PointerRoutedEventArgs const& e){
        auto button=state->owner.get();if(!button||!button.IsEnabled())return;auto point=e.GetCurrentPoint(button);
        if(!PointerGesture::PrimaryContact(point))return;
        if(state->pointer)state->end(true);state->pointer=point.PointerId();state->inside=state->hit(e);state->hoverAllowed=point.PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Touch;
        if(state->input)state->input->owners[*state->pointer]=state;state->present();
    })),true);
    for(auto const& event:{UIElement::PointerEnteredEvent(),UIElement::PointerMovedEvent(),UIElement::PointerExitedEvent()})
        button.AddHandler(event,box_value(PointerEventHandler([state](auto&&,PointerRoutedEventArgs const& e){
            auto point=e.GetCurrentPoint(nullptr);bool inside=state->hit(e);
            if(state->pointer==point.PointerId())state->inside=inside;
            state->hoverAllowed=inside&&point.PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Touch;state->present();
        })),true);
    for(auto const& [event,completion]:{std::pair{UIElement::PointerReleasedEvent(),PointerCompletion::Released},std::pair{UIElement::PointerCanceledEvent(),PointerCompletion::Canceled},std::pair{UIElement::PointerCaptureLostEvent(),PointerCompletion::CaptureLost}})
        button.AddHandler(event,box_value(PointerEventHandler([state,completion](auto&&,PointerRoutedEventArgs const& e){state->complete(e,completion);})),true);
    button.LostFocus([state](auto&&,auto&&){if(state->pointer)state->end(true);});
    button.Loaded([state](auto&&,auto&&){if(auto owner=state->owner.get();owner&&owner.XamlRoot())state->input=contacts(owner.XamlRoot().Content());state->present();});
    button.Unloaded([state](auto&&,auto&&){state->end(true);auto color=state->overlay.Color();color.A=0;state->overlay.Color(color);state->input.reset();});
    if(button.IsLoaded()&&button.XamlRoot())state->input=contacts(button.XamlRoot().Content());
    return state;
}
}
inline void buttonFeedback(winrt::Microsoft::UI::Xaml::Controls::Primitives::ButtonBase const& button,winrt::Microsoft::UI::Xaml::Media::SolidColorBrush const& ink,
    uint8_t hover=20,uint8_t press=41,winrt::Microsoft::UI::Xaml::Media::Brush const& hoverSurface=nullptr,bool fixed=false,
    winrt::Microsoft::UI::Xaml::Media::Brush const& checkedSurface=nullptr){
    auto state=feedback::state(button);state->source(ink);state->hover=hover;state->press=press;
    state->hoverSurface=hoverSurface;state->fixed=fixed;state->checkedSurface=checkedSurface;state->present();
}
inline winrt::Microsoft::UI::Xaml::Media::SolidColorBrush buttonFeedbackBrush(winrt::Microsoft::UI::Xaml::Controls::Primitives::ButtonBase const& button,
    winrt::Microsoft::UI::Xaml::Media::SolidColorBrush const& ink,uint8_t hover=20,uint8_t press=41){
    buttonFeedback(button,ink,hover,press);auto state=feedback::state(button);state->external=true;state->present();return state->overlay;
}
}
