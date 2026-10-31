#include "pch.h"
#include "ChoiceFeedback.h"
#include "UiControls.h"
#include "WorkspaceShadow.h"
#include <limits>
#include <winrt/Microsoft.UI.Text.h>
#include <winrt/Microsoft.UI.Xaml.Documents.h>
#include <winrt/Windows.UI.ViewManagement.h>

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
    bool rowSuffix=false;
    std::optional<Windows::UI::Text::FontWeight> captionWeight;
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
        for(auto const& value:resources)value.brush.Color(value.kind==Kind::Hover?(rowSuffix?base:over(base,ink,20)):value.kind==Kind::Pressed?(rowSuffix?base:over(base,ink,41)):value.kind==Kind::Base?base:value.kind==Kind::Ink?ink:Windows::UI::Color{});
        double opacity=rowSuffix||control.IsEnabled()||control.Opacity()<=0?1.:std::min(1.,(92./255)/control.Opacity());
        for(auto name:{L"ContentPresenter",L"PlaceholderTextBlock",L"HeaderContentPresenter",L"DropDownGlyph"})if(auto child=part(control,name))child.Opacity(opacity);
    }
    void presentWeights(){
        auto control=owner.get();if(!control||!captionWeight)return;
        for(uint32_t i=0;i<control.Items().Size();++i)if(auto item=control.Items().GetAt(i).try_as<ComboBoxItem>())if(auto text=item.Content().try_as<TextBlock>()){
            auto weight=!control.IsDropDownOpen()&&int32_t(i)==control.SelectedIndex()?*captionWeight:Windows::UI::Text::FontWeights::Normal();
            if(text.FontWeight().Weight!=weight.Weight)text.FontWeight(weight);
        }
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
        control.Loaded([self](auto&&,auto&&){self->present();self->presentWeights();});control.DropDownOpened([self](auto&&,auto&&){self->presentWeights();self->presentItems();if(auto owner=self->owner.get())owner.DispatcherQueue().TryEnqueue([self]{self->presentItems();});});
        control.DropDownClosed([self](auto&&,auto&&){self->presentWeights();});control.SelectionChanged([self](auto&&,auto&&){self->presentWeights();});
        control.Items().VectorChanged([self](auto&&,auto&&){self->presentWeights();});present();
    }
};
void naturalDialogLayout(ContentDialog const& dialog){
    auto resources=dialog.Resources();auto enabled=box_value(L"CapyDialogLayout");if(!resources.HasKey(enabled))return;
    auto commands=part(dialog,L"CommandSpace").try_as<Grid>();if(!commands)return;
    static thread_local Windows::UI::ViewManagement::UISettings settings;
    hstring key=to_hstring(settings.TextScaleFactor())+L"|"+dialog.PrimaryButtonText()+L"|"+dialog.SecondaryButtonText()+L"|"+dialog.CloseButtonText()+L"|";
    for(auto name:{L"PrimaryButton",L"SecondaryButton",L"CloseButton"})if(auto button=part(dialog,name).try_as<Button>())
        key=key+to_hstring(int(button.Visibility()))+L":"+to_hstring(Grid::GetColumn(button))+L":"+to_hstring(button.FontSize())+L"|";
    auto previous=box_value(L"CapyDialogLayoutKey");if(resources.HasKey(previous)&&unbox_value<hstring>(resources.Lookup(previous))==key)return;
    resources.Insert(previous,box_value(key));commands.HorizontalAlignment(HorizontalAlignment::Right);commands.Padding({20,16,20,20});
    std::vector<double> widths(commands.ColumnDefinitions().Size());std::vector<uint32_t> occupied;
    for(auto name:{L"PrimaryButton",L"SecondaryButton",L"CloseButton"})if(auto button=part(dialog,name).try_as<Button>();button&&button.Visibility()==Visibility::Visible){
        button.MinWidth(0);button.Measure({std::numeric_limits<float>::infinity(),std::numeric_limits<float>::infinity()});
        auto column=uint32_t(Grid::GetColumn(button));if(column<widths.size()){widths[column]=button.DesiredSize().Width;occupied.push_back(column);}
    }
    std::sort(occupied.begin(),occupied.end());
    for(size_t i=1;i<occupied.size();++i)if(occupied[i]>occupied[i-1]+1)widths[occupied[i-1]+1]=8;
    for(uint32_t i=0;i<widths.size();++i){
        auto column=commands.ColumnDefinitions().GetAt(i);column.MinWidth(0);column.MaxWidth(widths[i]);
        column.Width(std::find(occupied.begin(),occupied.end(),i)!=occupied.end()?GridLength{1,GridUnitType::Star}:GridLength{widths[i],GridUnitType::Pixel});
    }
}
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
    naturalDialogLayout(dialog);
}
}
SolidColorBrush CapyUi::fieldBackground(std::shared_ptr<WorkspaceData> const& data,ControlSurface surface){
    return surface==ControlSurface::Transparent?clear():surface==ControlSurface::Panel||surface==ControlSurface::Card?data->brush(L"input"):data->tint(L"text",surface==ControlSurface::Drawing?18:26);
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
    put(L"ToolTipBorderBrush",transparent);
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
    previous=key;
}
bool CapyUi::selectIndex(ComboBox const& control,int32_t index){
    auto next=index>=0&&uint32_t(index)<control.Items().Size()?index:-1;
    if(control.SelectedIndex()==next)return false;control.SelectedIndex(next);return true;
}
void CapyUi::choiceColors(std::shared_ptr<WorkspaceData> const& data,ComboBox const& control,ControlSurface surface,ChoiceFeedbackMode mode,std::optional<Windows::UI::Text::FontWeight> captionWeight){
    control.Background(fieldBackground(data,mode==ChoiceFeedbackMode::RowSuffix?ControlSurface::Transparent:surface));control.Foreground(data->brush(L"text"));
    control.BorderThickness({0});control.CornerRadius(data->controlCorner());control.MinWidth(0);
    control.MinHeight(surface==ControlSurface::Dialog||surface==ControlSurface::Drawing?34:data->controlHeight());
    double padding=data->panelMetric(L"field_padding");control.Padding({padding,0,padding,0});
    control.FontFamily(FontFamily(L"Segoe UI"));control.FontSize(data->textSize());control.FontWeight(Windows::UI::Text::FontWeights::Normal());
    auto key=box_value(L"CapyChoiceFeedback");if(control.Resources().HasKey(key))return;
    edgeTag(control,L"edge_reset_contact",S(L"pickup"));
    control.Resources().Insert(key,box_value(true));auto state=std::make_shared<ChoiceState>();state->rowSuffix=mode==ChoiceFeedbackMode::RowSuffix;state->captionWeight=captionWeight;state->init(control);
    auto open=std::make_shared<bool>(false);auto context=std::weak_ptr<WorkspaceData>(data);
    auto report=[context,open](bool value){if(std::exchange(*open,value)!=value)if(auto data=context.lock())data->popup(value);};
    control.DropDownOpened([report](auto&&,auto&&){report(true);});control.DropDownClosed([report](auto&&,auto&&){report(false);});control.Unloaded([report](auto&&,auto&&){report(false);});
}
void CapyUi::fieldColors(std::shared_ptr<WorkspaceData> const& data,TextBox const& control,ControlSurface surface){
    auto background=fieldBackground(data,surface);control.Background(background);control.Foreground(data->brush(L"text"));
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
ToggleSwitch CapyUi::compactSwitch(){
    ToggleSwitch control;control.MinWidth(0);control.OnContent(box_value(L""));control.OffContent(box_value(L""));
    control.Resources().Insert(box_value(L"ToggleSwitchThemeMinWidth"),box_value(0.));
    for(auto name:{L"ToggleSwitchPreContentMargin",L"ToggleSwitchPostContentMargin"})control.Resources().Insert(box_value(name),box_value(0.));
    auto present=[weak=make_weak(control)]{
        auto control=weak.get();if(!control)return;control.ApplyTemplate();
        for(auto name:{L"OnContentPresenter",L"OffContentPresenter",L"HeaderContentPresenter"})if(auto content=part(control,name))content.Visibility(Visibility::Collapsed);
        if(auto area=part(control,L"SwitchAreaGrid"))if(auto grid=VisualTreeHelper::GetParent(area).try_as<Grid>())
            for(uint32_t i=1;i<grid.ColumnDefinitions().Size();++i){auto column=grid.ColumnDefinitions().GetAt(i);column.Width({0,GridUnitType::Pixel});column.MaxWidth(0);}
    };
    control.Loaded([present](auto&&,auto&&){present();});return control;
}
Button CapyUi::choiceRow(std::shared_ptr<WorkspaceData> const& data,UIElement const& content,ComboBox const& control){
    auto row=button(data,L"",[weak=make_weak(control)]{if(auto control=weak.get();control&&control.IsEnabled())control.IsDropDownOpen(true);});
    row.HorizontalAlignment(HorizontalAlignment::Stretch);row.HorizontalContentAlignment(HorizontalAlignment::Stretch);
    row.Padding({0});row.MinHeight(0);row.CornerRadius({0});row.Content(content);buttonFeedback(row,data->brush(L"text"),8,20);
    auto name=[owner=make_weak(row),source=make_weak(control)]{if(auto row=owner.get())if(auto control=source.get())AutomationProperties::SetName(row,AutomationProperties::GetName(control));};
    control.RegisterPropertyChangedCallback(AutomationProperties::NameProperty(),[name](auto&&,auto&&){name();});name();return row;
}
void CapyUi::dialogLayout(std::shared_ptr<WorkspaceData> const& data,ContentDialog const& dialog){
    auto key=box_value(L"CapyDialogLayout");if(!dialog.Resources().HasKey(key)){
        dialog.Resources().Insert(key,box_value(true));dialog.Resources().Insert(box_value(L"ContentDialogPadding"),box_value(Thickness{20,20,20,20}));
        dialog.LayoutUpdated([weak=make_weak(dialog)](auto&&,auto&&){if(auto dialog=weak.get())naturalDialogLayout(dialog);});
    }
    dialogButtonColors(data,dialog);
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

namespace {
struct EntryBaseline:std::enable_shared_from_this<EntryBaseline>{
    weak_ref<TextBox> entry;
    weak_ref<TextBlock> reference;
    Grid host;
    RichEditBox native;
    TextBlock sample;
    Windows::UI::ViewManagement::UISettings settings;
    Windows::UI::ViewManagement::UISettings::TextScaleFactorChanged_revoker scaleChanged;
    bool dirty=true,queued=false,presenting=false;
    template<class Target,class Source> static void font(Target const& target,Source const& source){
        target.FontFamily(source.FontFamily());target.FontSize(source.FontSize());target.FontWeight(source.FontWeight());
        target.FontStyle(source.FontStyle());target.FontStretch(source.FontStretch());target.CharacterSpacing(source.CharacterSpacing());
        target.Language(source.Language());target.FlowDirection(source.FlowDirection());target.IsTextScaleFactorEnabled(source.IsTextScaleFactorEnabled());
        target.UseLayoutRounding(source.UseLayoutRounding());
    }
    void invalidate(){
        if(presenting)return;
        dirty=true;if(queued)return;
        if(auto owner=entry.get()){
            queued=true;auto weak=weak_from_this();
            if(!owner.DispatcherQueue().TryEnqueue([weak]{if(auto self=weak.lock()){self->queued=false;self->present();}}))queued=false;
        }
    }
    void present(){
        auto owner=entry.get();if(!owner||!dirty||presenting||!owner.IsLoaded())return;
        auto parent=owner.Parent().try_as<Panel>();if(!parent)return;
        if(host.Parent()!=parent){
            if(auto previous=host.Parent().try_as<Panel>()){uint32_t index;if(previous.Children().IndexOf(host,index))previous.Children().RemoveAt(index);}
            parent.Children().Append(host);
        }
        if(!host.IsLoaded()||!native.IsLoaded()||!sample.IsLoaded()||parent.ActualWidth()<=0)return;
        presenting=true;struct Reset{bool& value;~Reset(){value=false;}}reset{presenting};
        auto source=reference.get();font(native,owner);
        hstring text=owner.Text();double optical=0;
        if(source){
            font(sample,source);sample.LineHeight(source.LineHeight());sample.LineStackingStrategy(source.LineStackingStrategy());sample.TextLineBounds(source.TextLineBounds());
            text=owner.Visibility()==Visibility::Visible?owner.Text():source.Text();
            optical=source.RenderTransform()?source.RenderTransform().TransformPoint({0,0}).Y:0;
            sample.Text(source.Text().empty()?hstring(L"0"):source.Text());
        }else{font(sample,owner);sample.Text(text.empty()?hstring(L"0"):text);opticalText(sample,owner.FontSize());optical=sample.RenderTransform().TransformPoint({0,0}).Y;}
        auto available=Windows::Foundation::Size{std::numeric_limits<float>::infinity(),std::numeric_limits<float>::infinity()};
        native.MaxWidth(parent.ActualWidth());sample.MaxWidth(parent.ActualWidth());
        sample.Measure(available);auto measured=sample.DesiredSize();sample.Arrange({0,0,measured.Width,measured.Height});
        auto character=sample.ContentStart().GetCharacterRect(Documents::LogicalDirection::Forward);
        double height=std::isfinite(owner.Height())?owner.Height():owner.ActualHeight();if(height<=0)return;
        native.Height(height);native.Document().SetText(Microsoft::UI::Text::TextSetOptions::None,text.empty()?hstring(L"0"):text);
        native.Measure(available);auto extent=native.DesiredSize();native.Arrange({0,0,extent.Width,extent.Height});
        Windows::Foundation::Point top,baseline,bottom;
        auto options=Microsoft::UI::Text::PointOptions::Start|Microsoft::UI::Text::PointOptions::ClientCoordinates|Microsoft::UI::Text::PointOptions::AllowOffClient;
        auto range=native.Document().GetRange(0,1);
        range.GetPoint(Microsoft::UI::Text::HorizontalCharacterAlignment::Left,Microsoft::UI::Text::VerticalCharacterAlignment::Top,options,top);
        range.GetPoint(Microsoft::UI::Text::HorizontalCharacterAlignment::Left,Microsoft::UI::Text::VerticalCharacterAlignment::Baseline,options,baseline);
        range.GetPoint(Microsoft::UI::Text::HorizontalCharacterAlignment::Left,Microsoft::UI::Text::VerticalCharacterAlignment::Bottom,options,bottom);
        double line=bottom.Y-top.Y,shift=character.Y+sample.BaselineOffset()-measured.Height/2+optical-((baseline.Y-top.Y)-line/2);
        if(measured.Height<=0||line<=0||!std::isfinite(shift))return;
        if(source){
            height=std::max({height,double(measured.Height),line+2*std::abs(shift)});
            if(owner.Height()!=height)owner.Height(height);
            if(auto slot=parent.try_as<Grid>();slot&&slot.MinHeight()<height)slot.MinHeight(height);
        }
        double inset=std::max(0.,(height-line)/2);shift=std::clamp(shift,-inset,inset);
        auto padding=owner.Padding(),border=owner.BorderThickness();auto next=Thickness{padding.Left,std::max(0.,inset+shift-border.Top),padding.Right,std::max(0.,inset-shift-border.Bottom)};
        if(padding!=next)owner.Padding(next);dirty=false;
    }
    void init(TextBox const& owner,TextBlock const& target){
        entry=make_weak(owner);if(target)reference=make_weak(target);
        host.Opacity(0);host.Width(0);host.Height(0);host.IsHitTestVisible(false);
        AutomationProperties::SetAccessibilityView(host,Automation::Peers::AccessibilityView::Raw);
        for(auto element:{native.as<FrameworkElement>(),sample.as<FrameworkElement>()}){element.IsHitTestVisible(false);AutomationProperties::SetAccessibilityView(element,Automation::Peers::AccessibilityView::Raw);host.Children().Append(element);}
        native.IsTabStop(false);native.MinWidth(0);native.MinHeight(0);native.BorderThickness({0,0,0,0});native.Padding({0,0,0,0});native.AcceptsReturn(false);native.TextWrapping(TextWrapping::NoWrap);
        sample.TextWrapping(TextWrapping::NoWrap);
        auto self=shared_from_this();auto weak=weak_from_this();
        scaleChanged=settings.TextScaleFactorChanged(auto_revoke,[queue=owner.DispatcherQueue(),weak](auto&&,auto&&){queue.TryEnqueue([weak]{if(auto self=weak.lock())self->invalidate();});});
        owner.Loaded([self](auto&&,auto&&){self->invalidate();});owner.SizeChanged([self](auto&&,auto&&){self->invalidate();});owner.TextChanged([self](auto&&,auto&&){self->invalidate();});
        host.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->invalidate();});
        owner.LayoutUpdated([self](auto&&,auto&&){if(self->dirty)self->present();});
        for(auto property:{Control::FontFamilyProperty(),Control::FontSizeProperty(),Control::FontWeightProperty(),Control::FontStyleProperty(),Control::FontStretchProperty(),Control::CharacterSpacingProperty(),FrameworkElement::LanguageProperty(),FrameworkElement::FlowDirectionProperty(),UIElement::UseLayoutRoundingProperty(),Control::IsTextScaleFactorEnabledProperty(),UIElement::VisibilityProperty()})owner.RegisterPropertyChangedCallback(property,[self](auto&&,auto&&){self->invalidate();});
        if(target){
            target.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->invalidate();});
            for(auto property:{TextBlock::TextProperty(),TextBlock::FontFamilyProperty(),TextBlock::FontSizeProperty(),TextBlock::FontWeightProperty(),TextBlock::FontStyleProperty(),TextBlock::FontStretchProperty(),TextBlock::CharacterSpacingProperty(),TextBlock::LineHeightProperty(),TextBlock::LineStackingStrategyProperty(),TextBlock::TextLineBoundsProperty(),FrameworkElement::LanguageProperty(),FrameworkElement::FlowDirectionProperty(),UIElement::UseLayoutRoundingProperty(),TextBlock::IsTextScaleFactorEnabledProperty(),UIElement::RenderTransformProperty()})target.RegisterPropertyChangedCallback(property,[weak](auto&&,auto&&){if(auto self=weak.lock())self->invalidate();});
        }
        invalidate();
    }
};
}
void CapyUi::entryBaseline(TextBox const& entry,TextBlock const& reference){
    auto key=box_value(L"CapyEntryBaseline");if(entry.Resources().HasKey(key))return;
    entry.Resources().Insert(key,box_value(true));auto state=std::make_shared<EntryBaseline>();state->init(entry,reference);
}
