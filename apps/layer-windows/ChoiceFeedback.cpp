#include "pch.h"
#include "ChoiceFeedback.h"
#include "UiControls.h"

using namespace CapyUi;
namespace {
FrameworkElement part(Control const& control,hstring const& name){
    if(!VisualTreeHelper::GetChildrenCount(control))return nullptr;
    auto root=VisualTreeHelper::GetChild(control,0).try_as<FrameworkElement>();
    return root?root.FindName(name).try_as<FrameworkElement>():nullptr;
}
Windows::UI::Color over(Windows::UI::Color base,Windows::UI::Color ink,uint8_t amount){
    double a=ink.A/255.*amount/255.,b=base.A/255.*(1-a),alpha=a+b;
    auto channel=[&](uint8_t bg,uint8_t fg){return uint8_t(alpha>0?std::clamp(std::lround((bg*b+fg*a)/alpha),0l,255l):0);};
    return {uint8_t(std::lround(alpha*255)),channel(base.R,ink.R),channel(base.G,ink.G),channel(base.B,ink.B)};
}
struct ChoiceState:std::enable_shared_from_this<ChoiceState>{
    enum class Kind{Hover,Pressed,Base,Ink,Clear};
    struct Resource{SolidColorBrush brush;Kind kind;};
    weak_ref<ComboBox> owner;
    SolidColorBrush background{nullptr},foreground{nullptr};int64_t backgroundChanged=0,foregroundChanged=0;
    std::vector<Resource> resources;
    ~ChoiceState(){
        if(background)background.UnregisterPropertyChangedCallback(SolidColorBrush::ColorProperty(),backgroundChanged);
        if(foreground)foreground.UnregisterPropertyChangedCallback(SolidColorBrush::ColorProperty(),foregroundChanged);
    }
    void watch(SolidColorBrush const& next,SolidColorBrush& source,int64_t& token){
        if(source==next)return;
        if(source)source.UnregisterPropertyChangedCallback(SolidColorBrush::ColorProperty(),token);
        source=next;
        if(source)token=source.RegisterPropertyChangedCallback(SolidColorBrush::ColorProperty(),[weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->present();});
    }
    void present(){
        auto control=owner.get();if(!control)return;
        watch(control.Background().try_as<SolidColorBrush>(),background,backgroundChanged);
        watch(control.Foreground().try_as<SolidColorBrush>(),foreground,foregroundChanged);
        auto base=background?background.Color():Windows::UI::Color{},ink=foreground?foreground.Color():Windows::UI::Color{};
        for(auto const& value:resources)value.brush.Color(value.kind==Kind::Hover?over(base,ink,20):value.kind==Kind::Pressed?over(base,ink,41):value.kind==Kind::Base?base:value.kind==Kind::Ink?ink:Windows::UI::Color{});
        double opacity=control.IsEnabled()||control.Opacity()<=0?1.:std::min(1.,(92./255)/control.Opacity());
        for(auto name:{L"ContentPresenter",L"PlaceholderTextBlock",L"HeaderContentPresenter",L"DropDownGlyph"})if(auto child=part(control,name))child.Opacity(opacity);
    }
    void init(ComboBox const& control){
        owner=make_weak(control);
        auto add=[&](wchar_t const* key,Kind kind){SolidColorBrush brush;resources.push_back({brush,kind});control.Resources().Insert(box_value(hstring(key)),brush);};
        add(L"ComboBoxBackgroundPointerOver",Kind::Hover);add(L"ComboBoxBackgroundPressed",Kind::Pressed);add(L"ComboBoxBackgroundDisabled",Kind::Base);
        add(L"ComboBoxBackgroundFocused",Kind::Clear);
        for(auto key:{L"ComboBoxForegroundPointerOver",L"ComboBoxForegroundPressed",L"ComboBoxForegroundDisabled",L"ComboBoxForegroundFocused",L"ComboBoxForegroundFocusedPressed",L"ComboBoxPlaceHolderForegroundPointerOver",L"ComboBoxPlaceHolderForegroundPressed",L"ComboBoxPlaceHolderForegroundDisabled",L"ComboBoxPlaceHolderForegroundFocused",L"ComboBoxHeaderForegroundDisabled",L"ComboBoxDropDownGlyphForeground",L"ComboBoxDropDownGlyphForegroundDisabled",L"ComboBoxDropDownGlyphForegroundFocused",L"ComboBoxDropDownGlyphForegroundFocusedPressed"})add(key,Kind::Ink);
        for(auto key:{L"ComboBoxBorderBrushPointerOver",L"ComboBoxBorderBrushPressed",L"ComboBoxBorderBrushDisabled"})add(key,Kind::Clear);
        auto self=shared_from_this();
        for(auto property:{Control::BackgroundProperty(),Control::ForegroundProperty(),Control::IsEnabledProperty(),UIElement::OpacityProperty()})
            control.RegisterPropertyChangedCallback(property,[self](auto&&,auto&&){self->present();});
        control.Loaded([self](auto&&,auto&&){self->present();});present();
    }
};
void footer(std::shared_ptr<WorkspaceData> const& data,ContentDialog const& dialog){
    auto defaultButton=dialog.DefaultButton();
    for(auto const& [name,role]:{std::pair{L"PrimaryButton",ContentDialogButton::Primary},std::pair{L"SecondaryButton",ContentDialogButton::Secondary},std::pair{L"CloseButton",ContentDialogButton::Close}}){
        auto button=part(dialog,name).try_as<Button>();if(!button)continue;
        bool primary=defaultButton==role;auto ink=data->brush(primary?L"accent_foreground":L"text");
        button.Background(data->brush(primary?L"accent":L"control_fill"));button.Foreground(ink);button.BorderThickness({0});button.CornerRadius(data->controlCorner());
        buttonFeedback(button,ink);button.ApplyTemplate();feedback::state(button)->present();
    }
}
}
void CapyUi::choiceColors(std::shared_ptr<WorkspaceData> const& data,ComboBox const& control,bool drawing){
    control.Background(drawing?data->tint(L"text",18):data->brush(L"input"));control.Foreground(data->brush(L"text"));
    control.BorderThickness({0});control.CornerRadius(data->controlCorner());
    auto key=box_value(L"CapyChoiceFeedback");if(control.Resources().HasKey(key))return;
    control.Resources().Insert(key,box_value(true));auto state=std::make_shared<ChoiceState>();state->init(control);
}
void CapyUi::dialogButtonColors(std::shared_ptr<WorkspaceData> const& data,ContentDialog const& dialog){
    auto key=box_value(L"CapyDialogFeedback");
    if(!dialog.Resources().HasKey(key)){
        dialog.Resources().Insert(key,box_value(true));
        auto present=[weak=make_weak(dialog),source=std::weak_ptr<WorkspaceData>(data)]{auto owner=weak.get();auto data=source.lock();if(owner&&data)footer(data,owner);};
        dialog.Loaded([present](auto&&,auto&&){present();});dialog.Opened([present](auto&&,auto&&){present();});
        for(auto property:{ContentDialog::DefaultButtonProperty(),ContentDialog::PrimaryButtonTextProperty(),ContentDialog::SecondaryButtonTextProperty(),ContentDialog::CloseButtonTextProperty()})
            dialog.RegisterPropertyChangedCallback(property,[present](auto&&,auto&&){present();});
    }
    footer(data,dialog);
}
