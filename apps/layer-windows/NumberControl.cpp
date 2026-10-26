#include "pch.h"
#include "UiControls.h"
#include "PointerGesture.h"
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>
#include <winrt/Microsoft.UI.Xaml.Automation.Provider.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <limits>
#include <numbers>

namespace CapyUi {
namespace {
namespace Peers=Microsoft::UI::Xaml::Automation::Peers;
namespace Provider=Microsoft::UI::Xaml::Automation::Provider;
struct NumericCaption {hstring title,language,errorLanguage;J labels,errorReason;};
struct NumberState {
    double value=0;bool editing=false,dragging=false,formatting=false,gesture=false,sliderKeyboard=false;
    bool scrubbing=false,turning=false,fine=false;std::optional<uint32_t> pointer,sliderPointer;double origin=0,originY=0;
    double scrubOrigin=0,scrubBase=0,scrubPixels=0,dialShown=0,sliderOrigin=0;
    uint64_t contactGeneration=0;
    hstring identity;
    hstring measuredText;double measuredWidth=-1,measuredHeight=0;
    std::function<J(J const&,double,J const&)> resolve;
};
struct DialRange {
    std::function<double()> value;std::function<hstring()> text;std::function<void(J const&)> apply;
    double scale=1,minimum=0,maximum=0,step=0;
};
struct DialText:implements<DialText,Provider::IValueProvider> {
    weak_ref<Control> owner;std::shared_ptr<DialRange> range;
    DialText(weak_ref<Control> control,std::shared_ptr<DialRange> source):owner(std::move(control)),range(std::move(source)){}
    hstring Value(){return range->text();}
    bool IsReadOnly(){auto control=owner.get();return !control||!control.IsEnabled();}
    void SetValue(hstring const& text){if(!IsReadOnly())range->apply(O({{L"type",S(L"expression")},{L"text",S(text)}}));}
};
struct DialPeer:Peers::FrameworkElementAutomationPeerT<DialPeer,Provider::IRangeValueProvider> {
    weak_ref<Control> owner;std::shared_ptr<DialRange> range;Provider::IValueProvider text;
    DialPeer(Control const& control,std::shared_ptr<DialRange> const& source):FrameworkElementAutomationPeerT<DialPeer,Provider::IRangeValueProvider>(control),
        owner(make_weak(control)),range(source),text(make<DialText>(make_weak(control),source)){}
    Peers::AutomationControlType GetAutomationControlTypeCore(){return Peers::AutomationControlType::Slider;}
    hstring GetClassNameCore(){return L"Dial";}
    Windows::Foundation::IInspectable GetPatternCore(Peers::PatternInterface pattern){
        if(pattern==Peers::PatternInterface::RangeValue)return *this;
        if(pattern==Peers::PatternInterface::Value)return text;
        return FrameworkElementAutomationPeerT<DialPeer,Provider::IRangeValueProvider>::GetPatternCore(pattern);
    }
    double Value(){return range->value()*range->scale;}
    double Minimum(){return range->minimum*range->scale;}
    double Maximum(){return range->maximum*range->scale;}
    double SmallChange(){return range->step*range->scale;}
    double LargeChange(){return SmallChange();}
    bool IsReadOnly(){auto control=owner.get();return !control||!control.IsEnabled();}
    void SetValue(double value){if(!IsReadOnly())range->apply(O({{L"type",S(L"value")},{L"value",N(value/range->scale)}}));}
};
struct Dial:ContentControlT<Dial> {
    std::shared_ptr<DialRange> range=std::make_shared<DialRange>();
    Peers::AutomationPeer OnCreateAutomationPeer(){return make<DialPeer>(*this,range);}
};
bool shifted(PointerRoutedEventArgs const& e){
    using Windows::System::VirtualKeyModifiers;return (e.KeyModifiers()&VirtualKeyModifiers::Shift)==VirtualKeyModifiers::Shift;
}
void showNumericError(TextBox const& entry,hstring const& text){
    AutomationProperties::SetHelpText(entry,text);
    if(text.empty()){entry.BorderThickness({0});ToolTipService::SetToolTip(entry,nullptr);}
    else {entry.BorderThickness({1,1,1,1});entry.BorderBrush(fill({255,221,85,85}));tooltip(entry,text);}
}
}
StackPanel number(std::shared_ptr<WorkspaceData> const& data,hstring const& title,J const& spec,
    std::function<double()> get,std::function<void(double)> set,Bindings& bindings,Bindings* commits,bool valueOnly,hstring const& identifier,bool inlineTrack,NumberPresentation const& presentation,NumericAdmissions* admissions){
    auto currentTitle=presentation.title?presentation.title:std::function<hstring()>([title]{return title;});
    auto caption=std::make_shared<NumericCaption>();
    auto numericCopy=[source=std::weak_ptr<WorkspaceData>(data),currentTitle,caption]{
        auto data=source.lock();if(!data)return J{};auto title=currentTitle();auto language=data->language();
        if(!caption->labels.Size()||caption->title!=title||caption->language!=language){
            auto input=to_string(O({{L"label",S(title)}}).Stringify());
            NativeString raw(capy_numeric_labels(data->localization.get(),input.c_str()),capy_string_free);
            if(!raw)throw hresult_error(E_OUTOFMEMORY);auto labels=J::Parse(to_hstring(raw.get()));
            if(labels.HasKey(L"error"))throw hresult_invalid_argument(str(labels,L"error"));
            caption->title=title;caption->language=language;caption->labels=std::move(labels);
        }
        return caption->labels;
    };
    auto numericLabels=numericCopy();
    auto local=std::make_shared<NumberState>();local->value=get();
    if(presentation.identity)local->identity=presentation.identity();
    local->resolve=presentation.resolve?presentation.resolve:decltype(local->resolve)([data](J const& spec,double value,J const& operation){return numeric(data->localization.get(),spec,value,operation);});
    auto phase=presentation.phase;auto presented=presentation.text;
    auto setPosition=presentation.position;
    auto finish=[local,phase](hstring const& name){if(local->gesture){local->gesture=false;phase(name,local->value);}};
    auto showText=[local,spec,presented](wchar_t const* field){return presented?presented():str(local->resolve(spec,local->value,O({{L"type",S(L"format")}})),field);};
    bool preference=presentation.preference,compact=presentation.compact&&!preference;
    bool dial=str(spec,L"kind")==L"dial"&&!preference&&!valueOnly&&!inlineTrack,ranged=str(spec,L"kind")==L"slider"||dial;
    bool panel=ranged&&!preference&&!valueOnly&&!inlineTrack;
    double valueHeight=compact?data->controlHeight():preference?34.:(ranged?24.:32.),stepSize=panel?16.:compact?data->panelMetric(L"control_height"):ranged&&!preference?24.:32.;
    StackPanel root;root.Spacing(0);
    auto numberId=identifier.empty()?title:identifier;
    AutomationProperties::SetAutomationId(root,L"number-root-"+numberId);
    AutomationProperties::SetName(root,title);
    Grid header;header.UseLayoutRounding(false);header.ColumnSpacing(panel?0.:compact?data->panelMetric(L"control_spacing"):6);header.MinHeight(valueHeight);ColumnDefinition left;left.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(left);
    ColumnDefinition right;right.Width({panel?76.:1.,panel?GridUnitType::Pixel:GridUnitType::Auto});header.ColumnDefinitions().Append(right);
    if(panel){
        header.MinHeight(data->panelMetric(L"slider_height"));
        RowDefinition captionRow;captionRow.Height({1,GridUnitType::Auto});header.RowDefinitions().Append(captionRow);
        RowDefinition trackRow;trackRow.Height({stepSize,GridUnitType::Pixel});header.RowDefinitions().Append(trackRow);
    }
    auto text=label(data,title);text.Margin(Thickness{preference||compact?0.:6.,0,0,0});text.VerticalAlignment(VerticalAlignment::Center);
    AutomationProperties::SetAutomationId(text,L"number-caption-"+numberId);
    text.LineHeight(20);text.TextTrimming(TextTrimming::CharacterEllipsis);
    tooltip(text,title);
    StackPanel labels;labels.UseLayoutRounding(false);labels.VerticalAlignment(VerticalAlignment::Center);labels.Children().Append(text);
    if(panel){labels.VerticalAlignment(VerticalAlignment::Top);text.MinHeight(20);}
    double dialSize=data->panelMetric(L"slider_height")-2*data->panelMetric(L"control_spacing");
    if(dial)labels.Margin({0,0,dialSize+data->panelMetric(L"control_spacing"),0});
    weak_ref<TextBlock> detailView;
    if(!presentation.description.empty()||presentation.descriptionText){
        auto detail=label(data,presentation.description);detailView=make_weak(detail);detail.FontSize(data->textSize()/1.2);
        detail.LineHeight(detail.FontSize()*1.25);detail.TextWrapping(TextWrapping::Wrap);detail.Opacity(.55);
        labels.Children().Append(detail);
    }
    header.Children().Append(labels);
    TextBox entry;entry.UseLayoutRounding(false);entry.Width(panel?68:ranged?72:60);entry.MinWidth(0);entry.MinHeight(0);entry.Height(valueHeight);
    entry.VerticalAlignment(panel?VerticalAlignment::Bottom:VerticalAlignment::Center);
    inheritLanguage(entry,data);entry.FontSize(data->textSize());entry.FontFamily(FontFamily(L"Segoe UI"));entry.Foreground(data->brush(L"text"));
    entry.Background(data->brush(L"input"));entry.BorderThickness(Thickness{0});entry.CornerRadius(data->controlCorner());
    entry.Resources().Insert(box_value(L"TextControlBackgroundDisabled"),clear());
    auto hiddenText=clear();
    entry.Resources().Insert(box_value(L"TextControlForegroundDisabled"),hiddenText);
    entry.TextAlignment(TextAlignment::Right);Grid::SetColumn(entry,1);
    weak_ref<TextBlock> readout;
    weak_ref<Grid> panelValueBox;
    ContentControl valueSurface{nullptr};
    Grid valueBox;
    {
        // Read mode has the same text extent as the shared plain value. Keep
        // native text editing/accessibility while avoiding a hidden caret gutter.
        auto display=label(data,L"");display.UseLayoutRounding(false);display.LineHeight(20);
        display.TextAlignment(valueOnly?TextAlignment::Center:TextAlignment::Right);display.VerticalAlignment(VerticalAlignment::Center);
        display.Margin({preference?9.:6.,0,preference?9.:6.,0});display.IsHitTestVisible(false);
        AutomationProperties::SetAccessibilityView(display,Automation::Peers::AccessibilityView::Raw);
        readout=make_weak(display);
        valueBox.UseLayoutRounding(false);Grid::SetColumn(valueBox,1);Grid::SetColumn(entry,0);
        valueBox.Children().Append(entry);
        if(panel){
            valueBox.Width(68);valueBox.Height(valueHeight);
            entry.HorizontalAlignment(HorizontalAlignment::Right);panelValueBox=make_weak(valueBox);
        }
        {
            Border hit;hit.Background(clear());hit.ManipulationMode(ManipulationModes::None);hit.Child(display);
            valueSurface=ContentControl();valueSurface.Content(hit);valueSurface.Padding({0});valueSurface.Height(valueHeight);
            valueSurface.ManipulationMode(ManipulationModes::None);
            valueSurface.IsTabStop(true);valueSurface.HorizontalContentAlignment(HorizontalAlignment::Stretch);valueSurface.VerticalContentAlignment(VerticalAlignment::Stretch);
            AutomationProperties::SetAccessibilityView(valueSurface,Automation::Peers::AccessibilityView::Raw);
            valueBox.Children().Append(valueSurface);
        }
        if(panel){
            Canvas overlay;Grid::SetColumn(overlay,1);Grid::SetRowSpan(overlay,2);overlay.Children().Append(valueBox);
            auto placeValue=[canvas=make_weak(overlay),box=make_weak(valueBox)](auto&&,SizeChangedEventArgs const&){
                if(auto overlay=canvas.get())if(auto value=box.get()){
                    Canvas::SetLeft(value,overlay.ActualWidth()-value.ActualWidth());
                    Canvas::SetTop(value,overlay.ActualHeight()-value.ActualHeight());
                }
            };
            overlay.SizeChanged(placeValue);valueBox.SizeChanged(placeValue);
            header.Children().Append(overlay);
        }else header.Children().Append(valueBox);
        entry.IsEnabledChanged([readout](auto&&,DependencyPropertyChangedEventArgs const& args){
            if(auto view=readout.get())view.Opacity(unbox_value<bool>(args.NewValue())?1.:.36);
        });
        entry.Loaded([readout](Windows::Foundation::IInspectable const& sender,auto&&){
            if(auto view=readout.get())view.Opacity(sender.as<TextBox>().IsEnabled()?1.:.36);
        });
    }
    AutomationProperties::SetName(entry,str(numericLabels,L"edit"));if(!identifier.empty())AutomationProperties::SetAutomationId(entry,identifier);
    auto palette=object(data->state,L"palette");
    auto panelColor=color(str(palette,L"panel")),textColor=color(str(palette,L"text"));
    auto track=fill({255,uint8_t((int(panelColor.R)+textColor.R)/2),
        uint8_t((int(panelColor.G)+textColor.G)/2),uint8_t((int(panelColor.B)+textColor.B)/2)});
    ContentControl dialView{nullptr};weak_ref<ContentControl> dialWeak;std::shared_ptr<DialRange> dialRange;
    TranslateTransform dialDot{nullptr};
    if(dial){
        auto implementation=make_self<Dial>();dialRange=implementation->range;dialView=implementation.as<ContentControl>();dialWeak=make_weak(dialView);
        dialRange->scale=num(spec,L"scale",1);dialRange->minimum=num(spec,L"min");dialRange->maximum=num(spec,L"max");dialRange->step=num(spec,L"step");
        Grid face;face.Background(clear());face.ManipulationMode(ManipulationModes::None);
        Shapes::Ellipse disc;disc.Fill(data->brush(L"input"));disc.IsHitTestVisible(false);face.Children().Append(disc);
        Shapes::Ellipse ring;ring.Stroke(data->tint(L"text",153));ring.StrokeThickness(2);ring.IsHitTestVisible(false);ring.Visibility(Visibility::Collapsed);face.Children().Append(ring);
        Shapes::Ellipse dot;dot.Width(dialSize*.22);dot.Height(dialSize*.22);dot.Fill(track);dot.IsHitTestVisible(false);
        dot.HorizontalAlignment(HorizontalAlignment::Center);dot.VerticalAlignment(VerticalAlignment::Center);
        dialDot=TranslateTransform();dot.RenderTransform(dialDot);face.Children().Append(dot);
        dialView.Content(face);dialView.Width(dialSize);dialView.Height(dialSize);dialView.Padding({0});
        dialView.HorizontalContentAlignment(HorizontalAlignment::Stretch);dialView.VerticalContentAlignment(VerticalAlignment::Stretch);
        dialView.HorizontalAlignment(HorizontalAlignment::Right);dialView.VerticalAlignment(VerticalAlignment::Center);dialView.Margin({0,0,76,0});
        dialView.IsTabStop(true);dialView.UseSystemFocusVisuals(false);dialView.ManipulationMode(ManipulationModes::None);
        Grid::SetRowSpan(dialView,2);Grid::SetColumnSpan(dialView,2);
        AutomationProperties::SetName(dialView,title);AutomationProperties::SetAutomationId(dialView,numberId+L"-dial");tooltip(dialView,title);
        dialView.GotFocus([dialWeak,ring=make_weak(ring)](auto&&,auto&&){
            if(auto view=dialWeak.get())if(auto focus=ring.get())focus.Visibility(view.FocusState()==FocusState::Keyboard?Visibility::Visible:Visibility::Collapsed);
        });
        dialView.LostFocus([ring=make_weak(ring)](auto&&,auto&&){if(auto focus=ring.get())focus.Visibility(Visibility::Collapsed);});
        header.Children().Append(dialView);
    }
    Slider slider;
    slider.PreviewKeyDown([local](auto&&,auto&&){local->sliderKeyboard=true;});
    slider.KeyUp([local](auto&&,auto&&){local->sliderKeyboard=false;});
    slider.LostFocus([local](auto&&,auto&&){local->sliderKeyboard=false;});
    Grid sliderFrame;Border sliderInput;
    sliderInput.Background(clear());sliderInput.ManipulationMode(ManipulationModes::None);
    AutomationProperties::SetAccessibilityView(sliderInput,Peers::AccessibilityView::Raw);
    if(!preference){slider.IsHitTestVisible(false);sliderFrame.Children().Append(slider);sliderFrame.Children().Append(sliderInput);}
    FrameworkElement sliderHost=preference?slider.as<FrameworkElement>():sliderFrame.as<FrameworkElement>();
    UIElement sliderContact=preference?slider.as<UIElement>():sliderInput.as<UIElement>();
    auto presentCaption=[source=std::weak_ptr<WorkspaceData>(data),numericCopy,currentTitle,caption,description=presentation.descriptionText,root=make_weak(root),text=make_weak(text),detail=detailView,entry=make_weak(entry),slider=make_weak(slider),dialWeak]{
        auto data=source.lock();auto control=entry.get();if(!data||!control)return false;
        auto title=currentTitle();auto labels=numericCopy();
        AutomationProperties::SetName(control,str(labels,L"edit"));
        if(auto view=root.get())AutomationProperties::SetName(view,title);
        if(auto view=text.get()){view.Text(title);tooltip(view,title);}
        if(auto view=detail.get();view&&description){auto descriptionText=description();view.Text(descriptionText);view.Visibility(descriptionText.empty()?Visibility::Collapsed:Visibility::Visible);}
        if(auto view=slider.get())AutomationProperties::SetName(view,title);
        if(auto view=dialWeak.get()){AutomationProperties::SetName(view,title);tooltip(view,title);}
        if(caption->errorReason.Size()&&caption->errorLanguage!=data->language()){showNumericError(control,data->caption(O({{L"type",S(L"numeric_error")},{L"reason",caption->errorReason}})));caption->errorLanguage=data->language();}
        return true;
    };
    data->copyView(presentCaption);bindings.emplace_back([presentCaption]{presentCaption();});
    auto measureText=[data,local](hstring const& value){
        // Routine model updates retain the measured extent of unchanged text.
        if(local->measuredWidth>=0&&local->measuredText==value)return local->measuredWidth;
        auto measure=label(data,value,false,false);measure.UseLayoutRounding(false);measure.LineHeight(0);
        measure.Measure({std::numeric_limits<float>::infinity(),std::numeric_limits<float>::infinity()});
        auto extent=measure.DesiredSize();local->measuredText=value;
        local->measuredWidth=extent.Width;local->measuredHeight=extent.Height;return local->measuredWidth;
    };
    measureText(L"0");
    double textInset=std::max(0.,(valueHeight-local->measuredHeight)/2);
    entry.Padding({preference?9.:6.,textInset,preference?9.:6.,textInset});
    auto surface=make_weak(valueSurface);
    auto setText=[data,local,measureText,readout,panelValueBox,surface,hiddenText,ranged,valueOnly,inlineTrack,preference,panel,dialWeak,dialDot,dialSize,scale=num(spec,L"scale",1),weak=make_weak(entry)](hstring const& value){
        bool previous=std::exchange(local->formatting,true);
        struct Reset{bool& value;bool previous;~Reset(){value=previous;}} reset{local->formatting,previous};
        if(dialDot){
            double angle=local->value*scale*std::numbers::pi/180;
            dialDot.X(std::cos(angle)*dialSize*.275);dialDot.Y(std::sin(angle)*dialSize*.275);
            double shown=std::exchange(local->dialShown,local->value);
            if(auto view=dialWeak.get();view&&shown!=local->value&&Peers::AutomationPeer::ListenerExists(Peers::AutomationEvents::PropertyChanged))
                if(auto peer=Peers::FrameworkElementAutomationPeer::FromElement(view))
                    peer.RaisePropertyChangedEvent(Automation::RangeValuePatternIdentifiers::ValueProperty(),box_value(shown*scale),box_value(local->value*scale));
        }
        if(auto control=weak.get()){
            if(control.Text()!=value)control.Text(value);
            if(auto display=readout.get()){
                bool reading=control.FocusState()==FocusState::Unfocused&&!local->editing;
                if(auto box=panelValueBox.get())box.Width(reading?measureText(value)+12:68);
                if(display.Text()!=value)display.Text(value);
                display.Visibility(reading?Visibility::Visible:Visibility::Collapsed);
                if(auto target=surface.get())target.Visibility(reading?Visibility::Visible:Visibility::Collapsed);
                control.Foreground(reading?hiddenText:data->brush(L"text"));
            }
            if(inlineTrack)control.Width(std::max(control.Width(),measureText(value)+control.Padding().Left+control.Padding().Right));
            if(ranged&&!valueOnly&&!inlineTrack){
                auto width=panel?68.:control.FocusState()==FocusState::Unfocused?measureText(value)+(preference?18:12):92.;
                if(std::abs(control.Width()-width)>.01)control.Width(width);
            }
        }
    };
    entry.TextChanging([data,local,readout,panelValueBox,surface,inlineTrack,panel](Windows::Foundation::IInspectable const& sender,auto&&){
        // Covers typing, paste, accessibility and IME edits, including after Enter.
        if(data->updating||local->formatting)return;
        local->editing=true;
        // Accessibility can set text before focusing the field. Show that draft
        // immediately and preserve it when focus subsequently enters.
        if(auto display=readout.get()){
            display.Visibility(Visibility::Collapsed);auto control=sender.as<TextBox>();
            if(auto box=panelValueBox.get())box.Width(68);
            if(auto target=surface.get())target.Visibility(Visibility::Collapsed);
            control.Foreground(data->brush(L"text"));if(!inlineTrack)control.Width(panel?68:92);
        }
    });
    slider.Minimum(0);slider.Maximum(1);slider.StepFrequency(0.001);slider.MinHeight(0);slider.Height(stepSize);
    // The adjacent field displays shared units; the default thumb tooltip
    // exposes only normalized 0..1 positions.
    slider.IsThumbToolTipEnabled(false);
    slider.Resources().Insert(box_value(L"SliderHorizontalHeight"),box_value(stepSize));
    slider.Resources().Insert(box_value(L"SliderTrackThemeHeight"),box_value(4.));
    slider.Resources().Insert(box_value(L"SliderPreContentMargin"),box_value((stepSize-4)/2));
    slider.Resources().Insert(box_value(L"SliderPostContentMargin"),box_value((stepSize-4)/2));
    if(!preference){
        for(auto key:{L"SliderOuterThumbBackground",L"SliderOuterThumbBackgroundPointerOver",L"SliderOuterThumbBackgroundPressed",L"SliderThumbBorderBrush"})
            slider.Resources().Insert(box_value(key),clear());
        for(auto key:{L"SliderHorizontalThumbWidth",L"SliderHorizontalThumbHeight",L"SliderInnerThumbWidth",L"SliderInnerThumbHeight"})
            slider.Resources().Insert(box_value(key),box_value(0.));
    }
    AutomationProperties::SetName(slider,title);if(!identifier.empty())AutomationProperties::SetAutomationId(slider,identifier+L"-slider");
    for(auto key:{L"SliderTrackValueFill",L"SliderTrackValueFillPointerOver",L"SliderTrackValueFillPressed",L"SliderTrackValueFillDisabled"})
        slider.Resources().Insert(box_value(key),preference?accent(data):track);
    for(auto key:{L"SliderThumbBackground",L"SliderThumbBackgroundPointerOver",L"SliderThumbBackgroundPressed"})
        slider.Resources().Insert(box_value(key),preference?accent(data):data->brush(L"thumb"));
    for(auto key:{L"SliderTrackFill",L"SliderTrackFillPointerOver",L"SliderTrackFillPressed",L"SliderTrackFillDisabled"})
        slider.Resources().Insert(box_value(key),data->brush(L"input"));
    auto commit=[data,local,caption,spec,get,set,setText,presented,identity=presentation.identity,weak=make_weak(entry)](bool cancel){
        auto entry=weak.get();if(!cancel&&entry&&textComposing(entry))return false;
        if(!entry||!local->editing)return true;
        if(identity && local->identity!=identity()){
            local->identity=identity();local->value=get();cancel=true;
        }
        if(!cancel&&presented&&entry.Text()==presented()){local->editing=false;showNumericError(entry,L"");caption->errorReason=J{};return true;}
        try{
            auto next=local->resolve(spec,local->value,cancel?O({{L"type",S(L"format")}}):
                O({{L"type",S(L"expression")},{L"text",S(entry.Text())}}));
            bool changed=local->value!=num(next,L"value");
            local->value=num(next,L"value");local->editing=false;
            setText(cancel&&presented?presented():str(next,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));showNumericError(entry,L"");caption->errorReason=J{};
            if(!cancel&&changed)set(local->value);
            return true;
        }catch(NumericFailure const& error){
            caption->errorReason=error.reason;caption->errorLanguage=data->language();
            showNumericError(entry,error.message());return false;
        }catch(hresult_error const& error){
            caption->errorReason=J{};showNumericError(entry,error.message());
            return false;
        }
    };
    if(commits)commits->emplace_back([commit]{commit(false);});
    if(admissions)admissions->emplace_back(commit);
    {
        auto surfaceY=[weak=make_weak(valueSurface)](PointerRoutedEventArgs const& e){return e.GetCurrentPoint(weak.get().XamlRoot().Content().as<UIElement>()).Position().Y;};
        auto restore=[data,local,spec,set,setText,finish,weak=make_weak(valueSurface),dialWeak]{
            bool active=std::exchange(local->scrubbing,false);active=std::exchange(local->turning,false)||active;
            ++local->contactGeneration;if(local->pointer)data->pointerCancels.Forget(*local->pointer,local.get());
            local->pointer.reset();local->dragging=false;
            if(auto target=weak.get())target.ReleasePointerCaptures();
            if(auto target=dialWeak.get())target.ReleasePointerCaptures();
            if(active){bool transaction=local->gesture;finish(L"cancel");local->value=local->origin;if(!transaction)set(local->value);}
            setText(str(local->resolve(spec,local->value,O({{L"type",S(L"format")}})),L"text"));
        };
        auto move=[local,spec,set,setPosition,setText,phase,panel](double y,bool fine){
            double pixels=local->originY-y;
            if(panel&&fine!=local->fine){local->fine=fine;local->scrubOrigin=local->value;local->scrubBase=local->scrubPixels;}
            local->scrubPixels=pixels;
            auto dpi=GetDpiForSystem();double slop=std::max(2.,double(GetSystemMetricsForDpi(SM_CYDRAG,dpi))*96./std::max(96u,dpi));
            if(!local->scrubbing&&std::abs(pixels)<=slop)return;
            if(!std::exchange(local->scrubbing,true)&&phase){local->gesture=true;phase(L"down",local->origin);}
            auto operation=panel?O({{L"type",S(L"scrub")},{L"origin",N(local->scrubOrigin)},{L"pixels",N(pixels-local->scrubBase)},{L"fine",B(fine)}}):O({{L"type",S(L"position")},{L"position",N(local->scrubOrigin+pixels/200.)}});
            auto next=local->resolve(spec,local->value,operation);
            if(!panel&&setPosition)setPosition(num(next,L"value"));
            else{local->value=num(next,L"value");setText(str(next,L"text"));set(local->value);}
        };
        valueSurface.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([data,local,commit,restore,surfaceY,panel,spec,editor=make_weak(entry),weak=make_weak(valueSurface)](auto&&,PointerRoutedEventArgs const& e){
            if(data->claimBubbleContact(e))return;
            if(local->pointer||!PointerGesture::PrimaryContact(e.GetCurrentPoint(nullptr)))return;
            auto entry=editor.get();if(!entry||!entry.IsEnabled())return;
            if(!panel&&e.GetCurrentPoint(nullptr).PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse){entry.Focus(FocusState::Pointer);e.Handled(true);return;}
            if(!commit(false))return;
            auto target=weak.get();if(!target||!target.IsEnabled())return;
            local->pointer=e.Pointer().PointerId();local->origin=local->value;local->scrubOrigin=panel?local->value:num(local->resolve(spec,local->value,O({{L"type",S(L"format")}})),L"fill");local->originY=surfaceY(e);
            local->fine=shifted(e);local->scrubBase=local->scrubPixels=0;
            if(!target.CapturePointer(e.Pointer())){local->pointer.reset();return;}
            ++local->contactGeneration;data->pointerCancels.Observe(*local->pointer,local.get(),restore);
            local->dragging=true;target.Focus(FocusState::Pointer);e.Handled(true);
        })),true);
        valueSurface.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([local,move,restore,surfaceY,identity=presentation.identity](auto&&,PointerRoutedEventArgs const& e){
            if(local->pointer!=e.Pointer().PointerId())return;
            if(e.GetCurrentPoint(nullptr).Properties().IsCanceled()||(identity&&local->identity!=identity())){restore();return;}
            move(surfaceY(e),shifted(e));e.Handled(true);
        })),true);
        valueSurface.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([data,local,move,finish,setText,showText,restore,surfaceY,weak=make_weak(entry),surface=make_weak(valueSurface)](auto&&,PointerRoutedEventArgs const& e){
            if(local->pointer!=e.Pointer().PointerId())return;
            if(e.GetCurrentPoint(nullptr).Properties().IsCanceled()){restore();return;}
            bool scrub=local->scrubbing;if(scrub)move(surfaceY(e),shifted(e));
            ++local->contactGeneration;data->pointerCancels.Forget(*local->pointer,local.get());
            local->pointer.reset();local->dragging=false;local->scrubbing=false;
            if(auto target=surface.get())target.ReleasePointerCaptures();
            if(scrub){finish(L"up");setText(showText(L"text"));}else if(auto entry=weak.get())entry.Focus(FocusState::Pointer);
            e.Handled(true);
        })),true);
        auto cancel=[local,restore](auto&&,PointerRoutedEventArgs const& e){if(local->pointer==e.Pointer().PointerId())restore();};
        valueSurface.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler(cancel)),true);
        valueSurface.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler(cancel)),true);
        root.KeyDown([local,restore](auto&&,KeyRoutedEventArgs const& e){if(e.Key()==Windows::System::VirtualKey::Escape&&local->pointer){restore();e.Handled(true);}});
        valueSurface.PreviewKeyDown([local,weak=make_weak(entry)](auto&&,KeyRoutedEventArgs const& e){
            if(!local->pointer&&(e.Key()==Windows::System::VirtualKey::Enter||e.Key()==Windows::System::VirtualKey::Space)){
                if(auto entry=weak.get())entry.Focus(FocusState::Keyboard);e.Handled(true);
            }
        });
        auto resetValue=[local,spec,set,commit,reset=presentation.reset]{
            commit(true);
            if(reset)reset();else if(spec.HasKey(L"default_value")&&spec.GetNamedValue(L"default_value").ValueType()==Windows::Data::Json::JsonValueType::Number){
                auto next=local->resolve(spec,local->value,O({{L"type",S(L"expression")},{L"text",S(L"")}}));local->value=num(next,L"value");set(local->value);
            }
        };
        text.DoubleTapped([resetValue](auto&&,DoubleTappedRoutedEventArgs const& e){resetValue();e.Handled(true);});
        if(dial){
            auto turn=[local,spec,set,setText,weak=make_weak(entry)](J const& operation){
                auto next=local->resolve(spec,local->value,operation);bool changed=num(next,L"value")!=local->value;
                local->value=num(next,L"value");
                if(auto entry=weak.get())setText(str(next,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));
                if(changed)set(local->value);
            };
            auto point=[dialWeak](PointerRoutedEventArgs const& e){
                auto view=dialWeak.get();auto at=e.GetCurrentPoint(view).Position();double width=view.ActualWidth(),height=view.ActualHeight();
                return O({{L"type",S(L"dial")},{L"x",N(at.X-width/2)},{L"y",N(at.Y-height/2)},{L"radius",N(std::min(width,height)/2)}});
            };
            dialRange->value=[local]{return local->value;};
            dialRange->text=[local,spec]{return str(local->resolve(spec,local->value,O({{L"type",S(L"format")}})),L"text");};
            dialRange->apply=[local,commit,turn](J const& operation){if(!local->pointer&&commit(false))turn(operation);};
            dialView.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([data,local,commit,phase,turn,point,resetValue,restore,dialWeak,pressed=std::make_shared<uint64_t>()](auto&&,PointerRoutedEventArgs const& e){
                if(data->claimBubbleContact(e))return;
                auto view=dialWeak.get();if(!view||local->pointer||!view.IsEnabled())return;
                auto contact=e.GetCurrentPoint(view);
                if(!PointerGesture::PrimaryContact(contact))return;
                bool repeated=contact.Timestamp()-std::exchange(*pressed,contact.Timestamp())<=uint64_t(GetDoubleClickTime())*1000;
                if(repeated){*pressed=0;view.Focus(FocusState::Pointer);resetValue();e.Handled(true);return;}
                if(!commit(false)||!view.CapturePointer(e.Pointer()))return;
                view.Focus(FocusState::Pointer);
                local->pointer=contact.PointerId();local->origin=local->value;local->dragging=true;local->turning=true;
                ++local->contactGeneration;data->pointerCancels.Observe(*local->pointer,local.get(),restore);
                if(phase){local->gesture=true;phase(L"down",local->value);}
                turn(point(e));e.Handled(true);
            })),true);
            dialView.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([local,restore,turn,point,identity=presentation.identity](auto&&,PointerRoutedEventArgs const& e){
                if(local->pointer!=e.Pointer().PointerId()||!local->turning)return;
                if(e.GetCurrentPoint(nullptr).Properties().IsCanceled()||(identity&&local->identity!=identity())){restore();return;}
                turn(point(e));e.Handled(true);
            })),true);
            dialView.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([data,local,restore,turn,point,finish,dialWeak](auto&&,PointerRoutedEventArgs const& e){
                if(local->pointer!=e.Pointer().PointerId()||!local->turning)return;
                if(e.GetCurrentPoint(nullptr).Properties().IsCanceled()){restore();return;}
                turn(point(e));++local->contactGeneration;data->pointerCancels.Forget(*local->pointer,local.get());local->pointer.reset();local->dragging=false;local->turning=false;
                if(auto view=dialWeak.get())view.ReleasePointerCaptures();
                finish(L"up");e.Handled(true);
            })),true);
            auto release=[local,restore](auto&&,PointerRoutedEventArgs const& e){if(local->turning&&local->pointer==e.Pointer().PointerId())restore();};
            dialView.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler(release)),true);
            dialView.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler(release)),true);
            dialView.KeyDown([local,commit,phase,finish,turn,resetValue](auto&&,KeyRoutedEventArgs const& e){
                using Windows::System::VirtualKey;auto key=e.Key();
                if(local->pointer)return;
                if(key==VirtualKey::Escape&&local->gesture){finish(L"cancel");e.Handled(true);return;}
                if(key==VirtualKey::Home){resetValue();e.Handled(true);return;}
                int amount=(GetKeyState(VK_SHIFT)&0x8000)?15:1;
                int steps=key==VirtualKey::PageUp?45:key==VirtualKey::PageDown?-45:key==VirtualKey::Right||key==VirtualKey::Up?amount:key==VirtualKey::Left||key==VirtualKey::Down?-amount:0;
                if(!steps)return;
                e.Handled(true);if(!commit(false))return;
                if(phase&&!local->gesture){local->gesture=true;phase(L"down",local->value);}
                turn(O({{L"type",S(L"step")},{L"steps",N(steps)}}));
            });
            dialView.KeyUp([local,finish](auto&&,KeyRoutedEventArgs const& e){
                using Windows::System::VirtualKey;auto key=e.Key();
                if(!local->pointer&&(key==VirtualKey::Right||key==VirtualKey::Up||key==VirtualKey::Left||key==VirtualKey::Down||key==VirtualKey::PageUp||key==VirtualKey::PageDown))finish(L"up");
            });
            dialView.PointerWheelChanged([local,commit,turn](auto&&,PointerRoutedEventArgs const& e){
                if(local->pointer||e.GetCurrentPoint(nullptr).Properties().IsHorizontalMouseWheel()||!commit(false))return;
                turn(O({{L"type",S(L"step")},{L"steps",N(e.GetCurrentPoint(nullptr).Properties().MouseWheelDelta()/120.)}}));e.Handled(true);
            });
            dialView.LostFocus([local,finish](auto&&,auto&&){if(!local->pointer)finish(L"up");});
        }
    }
    header.AddHandler(UIElement::PointerWheelChangedEvent(),box_value(PointerEventHandler([local,spec,set,setText,commit,editor=make_weak(entry),surface=make_weak(valueSurface)](auto&&,PointerRoutedEventArgs const& e){
        auto entry=editor.get();auto readout=surface.get();if(!entry||!entry.IsEnabled()||local->pointer||e.GetCurrentPoint(nullptr).Properties().IsHorizontalMouseWheel())return;
        bool inside=false;for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node))if(node==entry||node==readout){inside=true;break;}
        if(!inside)return;e.Handled(true);if(!commit(false))return;
        auto next=local->resolve(spec,local->value,O({{L"type",S(L"step")},{L"steps",N(e.GetCurrentPoint(nullptr).Properties().MouseWheelDelta()/120.)}}));
        local->value=num(next,L"value");setText(str(next,L"text"));set(local->value);e.Handled(true);
    })),true);
    entry.GotFocus([data,local,showText,setText](Windows::Foundation::IInspectable const& sender,RoutedEventArgs const&){
        auto entry=sender.as<TextBox>();entry.Background(data->brush(L"input"));
        if(!local->editing)setText(showText(L"edit"));
    });
    // LosingFocus is synchronous; close and target-change commands must follow
    // the draft commit, rather than race the later LostFocus notification.
    entry.LosingFocus([commit,finish](auto&&,auto&&){finish(L"up");commit(false);});
    entry.LostFocus([commit,local,showText,setText](Windows::Foundation::IInspectable const& sender,RoutedEventArgs const&){
        auto entry=sender.as<TextBox>();commit(false);entry.Background(clear());
        if(!local->editing)setText(showText(L"text"));
    });
    entry.KeyDown([commit,local,finish](auto&&,KeyRoutedEventArgs const& e){
        if(composingKey(e))return;
        if(e.Key()==Windows::System::VirtualKey::Enter){commit(false);e.Handled(true);}
        else if(e.Key()==Windows::System::VirtualKey::Escape&&local->gesture){finish(L"cancel");e.Handled(true);}
        else if(e.Key()==Windows::System::VirtualKey::Escape&&local->editing){commit(true);e.Handled(true);}
    });
    // TextBox consumes some arrow keys before the bubbling KeyDown event.
    // Numeric spin steps must take precedence over its caret navigation.
    if(!ranged)entry.PreviewKeyDown([commit,local,spec,set,phase](auto&&,KeyRoutedEventArgs const& e){
        if(composingKey(e))return;
        if(e.Key()!=Windows::System::VirtualKey::Up&&e.Key()!=Windows::System::VirtualKey::Down)return;
        e.Handled(true);if(!commit(false))return;
        auto next=local->resolve(spec,local->value,O({{L"type",S(L"step")},{L"steps",N(e.Key()==Windows::System::VirtualKey::Up?1:-1)}}));
        local->value=num(next,L"value");
        if(phase&&!local->gesture){local->gesture=true;phase(L"down",local->value);}else set(local->value);
    });
    if(!ranged&&phase)entry.AddHandler(UIElement::KeyUpEvent(),box_value(KeyEventHandler([finish](auto&&,KeyRoutedEventArgs const& e){
        if(e.Key()==Windows::System::VirtualKey::Up||e.Key()==Windows::System::VirtualKey::Down)finish(L"up");
    })),true);
    auto completeSlider=[data,local,finish,set,setText,spec,weak=make_weak(slider),surface=make_weak(sliderContact)](bool cancel){
        if(!local->sliderPointer)return;
        ++local->contactGeneration;data->pointerCancels.Forget(*local->sliderPointer,local.get());local->sliderPointer.reset();
        bool transaction=local->gesture;local->dragging=false;finish(cancel?L"cancel":L"up");
        if(auto target=surface.get())target.ReleasePointerCaptures();
        if(cancel){
            local->value=local->sliderOrigin;if(!transaction)set(local->value);auto shown=local->resolve(spec,local->value,O({{L"type",S(L"format")}}));setText(str(shown,L"text"));
            if(auto slider=weak.get()){bool formatting=std::exchange(local->formatting,true);slider.Value(num(shown,L"fill"));local->formatting=formatting;}
        }
    };
    auto moveSlider=[weak=make_weak(slider),surface=make_weak(sliderContact)](PointerRoutedEventArgs const& e){
        auto slider=weak.get();auto target=surface.get().try_as<FrameworkElement>();if(!slider||!target||target.ActualWidth()<=0)return;
        double fill=std::clamp(double(e.GetCurrentPoint(target).Position().X)/target.ActualWidth(),0.,1.);
        if(slider.IsDirectionReversed()!=(slider.FlowDirection()==FlowDirection::RightToLeft))fill=1-fill;
        slider.Value(fill);
    };
    sliderContact.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([data,local,phase,get,commit,completeSlider,moveSlider,preference,weak=make_weak(slider)](auto&& sender,PointerRoutedEventArgs const& e){
        local->sliderKeyboard=false;
        if(data->claimBubbleContact(e))return;
        auto slider=weak.get();auto target=sender.template as<UIElement>();
        if(!slider||!slider.IsEnabled()||local->sliderPointer||!PointerGesture::PrimaryContact(e.GetCurrentPoint(nullptr)))return;
        if(!preference){
            if(!commit(false)){e.Handled(true);return;}
            slider.Focus(FocusState::Pointer);
            if(!target.CapturePointer(e.Pointer()))return;
        }
        local->sliderPointer=e.Pointer().PointerId();
        ++local->contactGeneration;data->pointerCancels.Observe(*local->sliderPointer,local.get(),[completeSlider]{completeSlider(true);});
        local->sliderOrigin=preference?get():local->value;
        local->dragging=true;if(phase&&!local->gesture){local->gesture=true;phase(L"down",local->value);}
        if(!preference){moveSlider(e);e.Handled(true);}
    })),true);
    if(!preference)sliderContact.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([local,moveSlider,completeSlider](auto&&,PointerRoutedEventArgs const& e){if(local->sliderPointer==e.Pointer().PointerId()){
        if(e.GetCurrentPoint(nullptr).Properties().IsCanceled())completeSlider(true);else moveSlider(e);e.Handled(true);
    }})),true);
    PointerGesture::OnCompleted(sliderContact,[local,completeSlider,moveSlider,preference](PointerRoutedEventArgs const& e,bool cancel){
        if(local->sliderPointer==e.Pointer().PointerId()){if(!cancel&&!preference)moveSlider(e);completeSlider(cancel);if(!preference)e.Handled(true);}
    },PointerCaptureLoss::CompleteLift);
    slider.ValueChanged([data,local,caption,spec,set,setPosition,setText,phase,range=make_weak(slider),weak=make_weak(entry)](auto&&,Primitives::RangeBaseValueChangedEventArgs const& e){
        if(data->updating||local->formatting)return;
        auto next=local->resolve(spec,local->value,O({{L"type",S(L"position")},{L"position",N(e.NewValue())}}));
        if(setPosition&&!local->sliderKeyboard){
            auto value=num(next,L"value");
            auto shown=local->resolve(spec,local->value,O({{L"type",S(L"format")}}));
            if(auto slider=range.get()){local->formatting=true;slider.Value(num(shown,L"fill"));local->formatting=false;}
            if(value!=local->value)setPosition(value);
            return;
        }
        if(num(next,L"value")==local->value)return;
        local->value=num(next,L"value");local->editing=false;
        if(auto entry=weak.get()){
            showNumericError(entry,L"");caption->errorReason=J{};
            setText(str(next,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));
        }
        if(phase&&!local->gesture)Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread().TryEnqueue([local,set,value=local->value,generation=local->contactGeneration]{if(generation==local->contactGeneration&&!local->gesture)set(value);});
        else set(local->value);
    });
    bindings.emplace_back([data,track]{
        auto palette=object(data->state,L"palette");
        auto panel=color(str(palette,L"panel")),ink=color(str(palette,L"text"));
        track.Color({255,uint8_t((int(panel.R)+ink.R)/2),uint8_t((int(panel.G)+ink.G)/2),uint8_t((int(panel.B)+ink.B)/2)});
    });
    bindings.emplace_back([data,local,caption,spec,get,entry,slider,setText,setPosition,presented,finish,surface=make_weak(sliderContact),identity=presentation.identity]{
        if(identity && local->identity!=identity()){
            ++local->contactGeneration;if(local->sliderPointer)data->pointerCancels.Forget(*local->sliderPointer,local.get());if(local->pointer)data->pointerCancels.Forget(*local->pointer,local.get());
            local->sliderPointer.reset();
            if(auto target=surface.get())target.ReleasePointerCaptures();
            finish(L"cancel");
            local->identity=identity();local->editing=false;local->dragging=false;local->pointer.reset();local->scrubbing=false;local->turning=false;
            showNumericError(entry,L"");caption->errorReason=J{};
        }
        if(local->editing||(local->dragging&&!setPosition))return;
        bool previous=std::exchange(local->formatting,true);
        struct Reset{bool& value;bool previous;~Reset(){value=previous;}} reset{local->formatting,previous};
        local->value=get();auto shown=local->resolve(spec,local->value,O({{L"type",S(L"format")}}));
        setText(presented?presented():str(shown,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));
        entry.Background(entry.FocusState()==FocusState::Unfocused?clear():data->brush(L"input"));
        slider.Value(num(shown,L"fill"));
    });
    root.Unloaded([data,local,finish,surface=make_weak(sliderContact)](auto&&,auto&&){++local->contactGeneration;if(local->sliderPointer)data->pointerCancels.Forget(*local->sliderPointer,local.get());if(local->pointer)data->pointerCancels.Forget(*local->pointer,local.get());local->sliderPointer.reset();if(auto target=surface.get())target.ReleasePointerCaptures();finish(L"cancel");local->pointer.reset();local->scrubbing=false;local->turning=false;local->dragging=false;});
    if(inlineTrack){
        // Compact layer controls retain the same shared value/expression rules
        // and target guards as full numeric controls.
        header.Children().RemoveAt(0);header.Children().InsertAt(0,sliderHost);
        header.ColumnSpacing(4);header.Height(data->controlHeight());
        auto low=str(local->resolve(spec,num(spec,L"min"),O({{L"type",S(L"format")}})),L"text");
        auto high=str(local->resolve(spec,num(spec,L"max"),O({{L"type",S(L"format")}})),L"text");
        std::wstring measure(low.size()>high.size()?low.c_str():high.c_str());
        for(auto const& sample:presentation.widthSamples)if(sample.size()>measure.size())measure=sample.c_str();
        for(auto& ch:measure)if(ch>=L'0'&&ch<=L'9')ch=L'8';
        entry.Width(measureText(hstring(measure))+12);entry.MinWidth(0);
        slider.MinWidth(0);root.Children().Append(header);return root;
    }
    if(valueOnly){
        header.Children().RemoveAt(1);entry.ClearValue(FrameworkElement::WidthProperty());entry.MinWidth(0);
        entry.HorizontalAlignment(HorizontalAlignment::Stretch);entry.TextAlignment(TextAlignment::Center);
        root.Children().Append(valueBox);return root;
    }
    if(panel){
        sliderHost.Margin({36,0,76,0});sliderHost.VerticalAlignment(VerticalAlignment::Bottom);Grid::SetColumnSpan(sliderHost,2);Grid::SetRow(sliderHost,1);
        if(!dial)header.Children().Append(sliderHost);
        root.Children().Append(header);return root;
    }
    StackPanel spin;spin.Orientation(Orientation::Horizontal);spin.Spacing(0);
    if(!ranged){
        if(compact){
            auto low=str(local->resolve(spec,num(spec,L"min"),O({{L"type",S(L"format")}})),L"text");
            auto high=str(local->resolve(spec,num(spec,L"max"),O({{L"type",S(L"format")}})),L"text");
            std::wstring measure(low.size()>high.size()?low.c_str():high.c_str());
            for(auto const& sample:presentation.widthSamples)if(sample.size()>measure.size())measure=sample.c_str();
            for(auto& ch:measure)if(ch>=L'0'&&ch<=L'9')ch=L'8';
            entry.Width(std::max(24.,measureText(hstring(measure)))+2*data->panelMetric(L"field_padding"));
        }
        header.Children().RemoveAt(1);spin.Children().Append(valueBox);
        Border frame;frame.Child(spin);frame.Background(data->brush(L"input"));frame.CornerRadius(data->controlCorner());
        Grid::SetColumn(frame,1);header.Children().Append(frame);
    }
    Grid trackRow;trackRow.ColumnSpacing(6);
    for(int i=0;i<3;i++){ColumnDefinition column;column.Width({i==1?1.:stepSize,i==1?GridUnitType::Star:GridUnitType::Pixel});trackRow.ColumnDefinitions().Append(column);}
    for(int direction:{-1,1}){
        auto step=button(data,str(numericLabels,direction<0?L"decrease":L"increase"),[local,spec,set,commit,direction]{
            if(!commit(false))return;
            auto next=local->resolve(spec,local->value,O({{L"type",S(L"step")},{L"steps",N(direction)}}));
            local->value=num(next,L"value");set(local->value);
        });
        step.Width(stepSize);step.Height(stepSize);step.Content(icon(direction<0?L"minus":L"plus",data->theme()));
        auto presentStep=[numericCopy,weak=make_weak(step),direction]{
            auto view=weak.get();if(!view)return false;auto labels=numericCopy();
            AutomationProperties::SetName(view,str(labels,direction<0?L"decrease":L"increase"));return true;
        };
        data->copyView(presentStep);bindings.emplace_back([presentStep]{presentStep();});
        AutomationProperties::SetAutomationId(step,numberId+(direction<0?L"-decrease":L"-increase"));
        step.IsEnabledChanged([](Windows::Foundation::IInspectable const& sender,DependencyPropertyChangedEventArgs const& args){
            sender.as<Button>().Opacity(unbox_value<bool>(args.NewValue())?1.:.36);
        });
        step.Loaded([](Windows::Foundation::IInspectable const& sender,auto&&){
            auto control=sender.as<Button>();control.Opacity(control.IsEnabled()?1.:.36);
        });
        if(ranged){Grid::SetColumn(step,direction<0?0:2);trackRow.Children().Append(step);}else spin.Children().Append(step);
        bindings.emplace_back([local,spec,step,direction]{
            bool enabled=direction<0?local->value>num(spec,L"min"):local->value<num(spec,L"max");
            step.IsEnabled(enabled);step.Opacity(step.IsEnabled()?1.:.36);
        });
    }
    Grid::SetColumn(sliderHost,1);trackRow.Children().Append(sliderHost);
    root.Children().Append(header);if(ranged)root.Children().Append(trackRow);return root;
}
}
