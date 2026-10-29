#include "pch.h"
#include "ChoiceFeedback.h"
#include "UiControls.h"
#include "WorkspaceShadow.h"

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
    void presentItems(){
        auto control=owner.get();if(!control)return;
        for(uint32_t i=0;i<control.Items().Size();++i){
            auto item=control.ContainerFromIndex(i).try_as<ComboBoxItem>();if(!item)item=control.Items().GetAt(i).try_as<ComboBoxItem>();
            if(!item)continue;item.ApplyTemplate();
            if(VisualTreeHelper::GetChildrenCount(item))if(auto root=VisualTreeHelper::GetChild(item,0).try_as<FrameworkElement>())root.Margin({0});
        }
    }
    void init(ComboBox const& control){
        owner=make_weak(control);
        auto add=[&](wchar_t const* key,Kind kind){SolidColorBrush brush;resources.push_back({brush,kind});control.Resources().Insert(box_value(hstring(key)),brush);};
        add(L"ComboBoxBackgroundPointerOver",Kind::Hover);add(L"ComboBoxBackgroundPressed",Kind::Pressed);add(L"ComboBoxBackgroundDisabled",Kind::Base);
        add(L"ComboBoxBackgroundFocused",Kind::Base);
        for(auto key:{L"ComboBoxForegroundPointerOver",L"ComboBoxForegroundPressed",L"ComboBoxForegroundDisabled",L"ComboBoxForegroundFocused",L"ComboBoxForegroundFocusedPressed",L"ComboBoxPlaceHolderForegroundPointerOver",L"ComboBoxPlaceHolderForegroundPressed",L"ComboBoxPlaceHolderForegroundDisabled",L"ComboBoxPlaceHolderForegroundFocused",L"ComboBoxHeaderForegroundDisabled",L"ComboBoxDropDownGlyphForeground",L"ComboBoxDropDownGlyphForegroundDisabled",L"ComboBoxDropDownGlyphForegroundFocused",L"ComboBoxDropDownGlyphForegroundFocusedPressed"})add(key,Kind::Ink);
        for(auto key:{L"ComboBoxBorderBrushPointerOver",L"ComboBoxBorderBrushPressed",L"ComboBoxBorderBrushDisabled"})add(key,Kind::Clear);
        auto self=shared_from_this();
        for(auto property:{Control::BackgroundProperty(),Control::ForegroundProperty(),Control::IsEnabledProperty(),UIElement::OpacityProperty()})
            control.RegisterPropertyChangedCallback(property,[self](auto&&,auto&&){self->present();});
        control.Loaded([self](auto&&,auto&&){self->present();});control.DropDownOpened([self](auto&&,auto&&){self->presentItems();if(auto owner=self->owner.get())owner.DispatcherQueue().TryEnqueue([self]{self->presentItems();});});present();
    }
};
void footer(std::shared_ptr<WorkspaceData> const& data,ContentDialog const& dialog){
    auto defaultButton=dialog.DefaultButton();
    for(auto const& [name,role]:{std::pair{L"PrimaryButton",ContentDialogButton::Primary},std::pair{L"SecondaryButton",ContentDialogButton::Secondary},std::pair{L"CloseButton",ContentDialogButton::Close}}){
        auto button=part(dialog,name).try_as<Button>();if(!button)continue;
        bool primary=defaultButton==role;auto ink=data->brush(primary?L"accent_foreground":L"text");
        button.Background(primary?data->brush(L"accent"):buttonBackground(data));button.MinHeight(34);button.Padding({16,0,16,0});button.FontSize(data->textSize());button.FontFamily(FontFamily(L"Segoe UI"));button.FontWeight(Windows::UI::Text::FontWeights::Medium());button.Foreground(ink);button.BorderThickness({0});button.CornerRadius(data->controlCorner());
        buttonFeedback(button,ink);button.FocusVisualPrimaryBrush(data->tint(L"accent",128));button.FocusVisualSecondaryBrush(clear());button.ApplyTemplate();
        if(auto presenter=part(button,L"ContentPresenter"))opticalText(presenter,button.FontSize());
        feedback::state(button)->present();
    }
}
}
namespace {
SolidColorBrush fieldFill(std::shared_ptr<WorkspaceData> const& data,ControlSurface surface){
    return surface==ControlSurface::Panel||surface==ControlSurface::Card?data->brush(L"input"):data->tint(L"text",surface==ControlSurface::Drawing?18:26);
}
}
ResourceDictionary CapyUi::paletteResources(){static thread_local ResourceDictionary resources;return resources;}
void CapyUi::refreshControlPalette(WorkspaceData& data){
    static thread_local hstring previous;auto palette=object(data.state,L"palette");if(!palette.Size())return;
    auto key=palette.Stringify();if(key==previous)return;
    auto resources=data.controlResources;static thread_local std::map<std::wstring,SolidColorBrush> ownedBrushes;
    auto put=[&](wchar_t const* name,auto const& value){
        auto key=box_value(hstring(name));
        if(auto next=value.template try_as<SolidColorBrush>()){
            auto found=ownedBrushes.find(name);
            if(found!=ownedBrushes.end()){found->second.Color(next.Color());return;}
            auto owned=fill(next.Color());ownedBrushes.emplace(name,owned);resources.Insert(key,owned);return;
        }
        resources.Insert(key,value);
    };
    auto text=data.brush(L"text"),accent=data.brush(L"accent"),selection=data.transparent()?data.glass(L"selection"):data.brush(L"selection");
    auto transparent=clear(),hover=data.tint(L"text",20),pressed=data.tint(L"text",41);
    put(L"SystemAccentColor",box_value(accent.Color()));
    for(auto name:{L"AccentFillColorDefaultBrush",L"AccentFillColorSecondaryBrush",L"AccentFillColorTertiaryBrush",L"CheckBoxCheckBackgroundFillChecked",L"CheckBoxCheckBackgroundFillCheckedPointerOver",L"CheckBoxCheckBackgroundFillCheckedPressed",L"ToggleSwitchFillOn",L"ToggleSwitchFillOnPointerOver",L"ToggleSwitchFillOnPressed",L"RadioButtonOuterEllipseFillChecked",L"RadioButtonOuterEllipseStrokeChecked"})put(name,accent);
    for(auto name:{L"ComboBoxDropDownBackground",L"FlyoutPresenterBackground",L"MenuFlyoutPresenterBackground"})put(name,data.brush(L"panel"));
    for(auto name:{L"ComboBoxDropDownBorderBrush",L"FlyoutPresenterBorderBrush",L"MenuFlyoutPresenterBorderBrush"})put(name,data.line(L"popover_border"));
    for(auto name:{L"ComboBoxItemBackground",L"MenuFlyoutItemBackground",L"ListViewItemBackground",L"ComboBoxItemPillFillBrush",L"ListViewItemSelectionIndicatorBrush",L"TextControlBorderBrush",L"TextControlBorderBrushPointerOver",L"TextControlBorderBrushFocused",L"TextControlBorderBrushDisabled"})put(name,transparent);
    for(auto name:{L"ComboBoxItemBackgroundPointerOver",L"MenuFlyoutItemBackgroundPointerOver",L"ListViewItemBackgroundPointerOver"})put(name,hover);
    for(auto name:{L"ComboBoxItemBackgroundPressed",L"ComboBoxItemBackgroundSelectedPressed",L"MenuFlyoutItemBackgroundPressed",L"ListViewItemBackgroundPressed",L"ListViewItemBackgroundSelectedPressed"})put(name,pressed);
    for(auto name:{L"ComboBoxItemBackgroundSelected",L"ComboBoxItemBackgroundSelectedUnfocused",L"ComboBoxItemBackgroundSelectedPointerOver",L"ListViewItemBackgroundSelected",L"ListViewItemBackgroundSelectedPointerOver"})put(name,selection);
    for(auto name:{L"ComboBoxItemForeground",L"ComboBoxItemForegroundPointerOver",L"ComboBoxItemForegroundPressed",L"ComboBoxItemForegroundSelected",L"ComboBoxItemForegroundSelectedUnfocused",L"ComboBoxItemForegroundSelectedPointerOver",L"ComboBoxItemForegroundSelectedPressed",L"MenuFlyoutItemForeground",L"MenuFlyoutItemForegroundPointerOver",L"MenuFlyoutItemForegroundPressed",L"TextControlForeground",L"TextControlForegroundPointerOver",L"TextControlForegroundFocused",L"HyperlinkButtonForeground",L"HyperlinkButtonForegroundPointerOver",L"HyperlinkButtonForegroundPressed"})put(name,text);
    for(auto name:{L"TextControlBackground",L"TextControlBackgroundPointerOver",L"TextControlBackgroundFocused",L"TextControlBackgroundDisabled"})put(name,data.brush(L"input"));
    for(auto name:{L"ToggleSwitchFillOff",L"ToggleSwitchFillOffPointerOver",L"ToggleSwitchFillOffPressed"})put(name,data.tint(L"text",51));
    for(auto name:{L"CheckBoxCheckGlyphForegroundChecked",L"CheckBoxCheckGlyphForegroundCheckedPointerOver",L"CheckBoxCheckGlyphForegroundCheckedPressed"})put(name,data.brush(L"accent_foreground"));
    for(auto name:{L"CheckBoxCheckBackgroundStrokeUnchecked",L"CheckBoxCheckBackgroundStrokeUncheckedPointerOver",L"CheckBoxCheckBackgroundStrokeUncheckedPressed",L"RadioButtonOuterEllipseStroke"})put(name,data.tint(L"text",128));
    put(L"ToolTipBackground",fill({204,0,0,6}));put(L"ToolTipForeground",fill({255,255,255,255}));put(L"ToolTipBorderBrush",transparent);
    put(L"ContentDialogBackground",data.brush(L"settings"));put(L"ContentDialogBorderBrush",data.line(L"settings",L"border"));put(L"ContentDialogSmokeFill",fill({82,0,0,0}));put(L"ContentDialogTopOverlay",transparent);
    put(L"SystemControlFocusVisualPrimaryBrush",data.tint(L"accent",128));put(L"SystemControlFocusVisualSecondaryBrush",transparent);
    put(L"FocusVisualPrimaryBrush",data.tint(L"accent",128));put(L"FocusVisualSecondaryBrush",transparent);
    put(L"ComboBoxItemThemePadding",box_value(Thickness{data.panelMetric(L"field_padding"),0,data.panelMetric(L"field_padding"),0}));
    put(L"ComboBoxItemThemeMinHeight",box_value(data.controlHeight()));
    put(L"ComboBoxItemThemeTouchPadding",box_value(Thickness{data.panelMetric(L"field_padding"),0,data.panelMetric(L"field_padding"),0}));
    put(L"ComboBoxItemThemeGameControllerPadding",box_value(Thickness{data.panelMetric(L"field_padding"),0,data.panelMetric(L"field_padding"),0}));
    put(L"ListViewItemSelectionIndicatorVisualEnabled",box_value(false));
    put(L"CheckBoxSize",box_value(16.));put(L"CheckBoxCornerRadius",box_value(CornerRadius{4,4,4,4}));
    put(L"TextControlBorderThemeThicknessFocused",box_value(Thickness{0}));put(L"TextControlBorderThemeThickness",box_value(Thickness{0}));
    put(L"ToolTipCornerRadius",box_value(CornerRadius{9,9,9,9}));put(L"ToolTipContentThemePadding",box_value(Thickness{10,6,10,6}));
    previous=key;
}
bool CapyUi::selectIndex(ComboBox const& control,int32_t index){
    auto next=index>=0&&uint32_t(index)<control.Items().Size()?index:-1;
    if(control.SelectedIndex()==next)return false;control.SelectedIndex(next);return true;
}
void CapyUi::choiceColors(std::shared_ptr<WorkspaceData> const& data,ComboBox const& control,ControlSurface surface){
    control.Background(fieldFill(data,surface));control.Foreground(data->brush(L"text"));
    control.BorderThickness({0});control.CornerRadius(data->controlCorner());control.MinWidth(0);
    control.MinHeight(surface==ControlSurface::Dialog||surface==ControlSurface::Drawing?34:data->controlHeight());
    double padding=data->panelMetric(L"field_padding");control.Padding({padding,0,padding,0});
    control.FontFamily(FontFamily(L"Segoe UI"));control.FontSize(data->textSize());control.FontWeight(Windows::UI::Text::FontWeights::Normal());
    auto key=box_value(L"CapyChoiceFeedback");if(control.Resources().HasKey(key))return;
    edgeTag(control,L"edge_reset_contact",S(L"pickup"));
    control.Resources().Insert(key,box_value(true));auto state=std::make_shared<ChoiceState>();state->init(control);
    auto open=std::make_shared<bool>(false);auto context=std::weak_ptr<WorkspaceData>(data);
    auto report=[context,open](bool value){if(std::exchange(*open,value)!=value)if(auto data=context.lock())data->popup(value);};
    control.DropDownOpened([report](auto&&,auto&&){report(true);});control.DropDownClosed([report](auto&&,auto&&){report(false);});control.Unloaded([report](auto&&,auto&&){report(false);});
}
void CapyUi::fieldColors(std::shared_ptr<WorkspaceData> const& data,TextBox const& control,ControlSurface surface){
    auto background=fieldFill(data,surface);control.Background(background);control.Foreground(data->brush(L"text"));
    control.FontFamily(FontFamily(L"Segoe UI"));control.FontSize(data->textSize());control.CornerRadius(data->controlCorner());
    auto resources=control.Resources();for(auto name:{L"TextControlBackground",L"TextControlBackgroundPointerOver",L"TextControlBackgroundFocused",L"TextControlBackgroundDisabled"})resources.Insert(box_value(name),background);
    auto key=box_value(L"CapyFieldBorder");if(!resources.HasKey(key))resources.Insert(key,clear());
    auto border=resources.Lookup(key);control.BorderBrush(border.as<Brush>());control.BorderThickness({1,1,1,1});
    for(auto name:{L"TextControlBorderBrush",L"TextControlBorderBrushPointerOver",L"TextControlBorderBrushFocused",L"TextControlBorderBrushDisabled"})resources.Insert(box_value(name),border);
    for(auto name:{L"TextControlBorderThemeThickness",L"TextControlBorderThemeThicknessFocused"})resources.Insert(box_value(name),box_value(Thickness{1,1,1,1}));
}
void CapyUi::clipRounded(FrameworkElement const& element,CornerRadius const& radius){
    auto present=[weak=make_weak(element),radius]{if(auto element=weak.get())ClipSurface(element,float(element.ActualWidth()),float(element.ActualHeight()),{float(radius.TopLeft),float(radius.TopRight),float(radius.BottomRight),float(radius.BottomLeft)},false);};
    element.SizeChanged([present](auto&&,auto&&){present();});element.Loaded([present](auto&&,auto&&){present();});present();
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
