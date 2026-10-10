#include "pch.h"
#include "UiControls.h"
#include <limits>

namespace CapyUi {
namespace {
struct NumericCaption {hstring title,language,errorLanguage;J labels,errorReason;};
struct NumberState {
    double value=0;bool editing=false,dragging=false,formatting=false,gesture=false;
    bool scrubbing=false;std::optional<uint32_t> pointer;double origin=0,originY=0;
    hstring identity;
    hstring measuredText;double measuredWidth=-1;
    std::function<J(J const&,double,J const&)> resolve;
};
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
            std::unique_ptr<char,decltype(&capy_string_free)> raw(capy_numeric_labels(data->localization.get(),input.c_str()),capy_string_free);
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
    auto finish=[local,phase](hstring const& name){if(local->gesture){local->gesture=false;phase(name,local->value);}};
    auto showText=[local,spec,presented](wchar_t const* field){return presented?presented():str(local->resolve(spec,local->value,O({{L"type",S(L"format")}})),field);};
    bool ranged=str(spec,L"kind")==L"slider",preference=presentation.preference;
    bool panel=ranged&&!preference&&!valueOnly&&!inlineTrack;
    double valueHeight=preference||panel?34.:(ranged?24.:32.),stepSize=panel?16.:ranged&&!preference?24.:32.;
    StackPanel root;root.Spacing(0);
    auto numberId=identifier.empty()?title:identifier;
    AutomationProperties::SetAutomationId(root,L"number-root-"+numberId);
    AutomationProperties::SetName(root,title);
    Grid header;header.UseLayoutRounding(false);header.ColumnSpacing(6);header.MinHeight(valueHeight);ColumnDefinition left;left.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(left);
    ColumnDefinition right;right.Width({panel?88.:1.,panel?GridUnitType::Pixel:GridUnitType::Auto});header.ColumnDefinitions().Append(right);
    if(panel){header.Height(36);header.MinHeight(36);}
    auto text=label(data,title);text.Margin(Thickness{preference?0.:6.,0,0,0});text.VerticalAlignment(VerticalAlignment::Center);
    text.LineHeight(20);text.TextTrimming(TextTrimming::CharacterEllipsis);
    tooltip(text,title);
    StackPanel labels;labels.UseLayoutRounding(false);labels.VerticalAlignment(VerticalAlignment::Center);labels.Children().Append(text);
    if(panel){labels.VerticalAlignment(VerticalAlignment::Top);text.Height(20);}
    weak_ref<TextBlock> detailView;
    if(!presentation.description.empty()||presentation.descriptionText){
        auto detail=label(data,presentation.description);detailView=make_weak(detail);detail.FontSize(data->textSize()/1.2);
        detail.LineHeight(detail.FontSize()*1.25);detail.TextWrapping(TextWrapping::Wrap);detail.Opacity(.55);
        labels.Children().Append(detail);
    }
    header.Children().Append(labels);
    TextBox entry;entry.UseLayoutRounding(false);entry.Width(ranged?72:60);entry.MinWidth(0);entry.MinHeight(0);entry.Height(valueHeight);entry.Padding(Thickness{preference?9.:6.,(valueHeight-20)/2,preference?9.:6.,(valueHeight-20)/2});
    entry.VerticalAlignment(VerticalAlignment::Center);entry.VerticalContentAlignment(VerticalAlignment::Center);
    inheritLanguage(entry,data);entry.FontSize(data->textSize());entry.FontFamily(FontFamily(L"Segoe UI"));entry.Foreground(data->brush(L"text"));
    entry.Background(data->brush(L"input"));entry.BorderThickness(Thickness{0});entry.CornerRadius({6,6,6,6});
    entry.Resources().Insert(box_value(L"TextControlBackgroundDisabled"),clear());
    auto disabledText=clear(),hiddenText=clear();
    entry.Resources().Insert(box_value(L"TextControlForegroundDisabled"),disabledText);
    bindings.emplace_back([data,ranged,disabledText]{
        auto ink=color(str(object(data->state,L"palette"),L"text"));if(ranged)ink.A=92;disabledText.Color(ink);
    });
    entry.TextAlignment(TextAlignment::Right);Grid::SetColumn(entry,1);
    weak_ref<TextBlock> readout;
    Button valueSurface{nullptr};
    if(ranged&&!valueOnly){
        // Read mode has the same text extent as the shared plain value. Keep
        // native text editing/accessibility while avoiding a hidden caret gutter.
        auto display=label(data,L"");display.UseLayoutRounding(false);display.LineHeight(20);
        display.TextAlignment(TextAlignment::Right);display.VerticalAlignment(VerticalAlignment::Center);
        display.Margin({preference?9.:6.,0,preference?9.:6.,0});display.IsHitTestVisible(false);
        AutomationProperties::SetAccessibilityView(display,Automation::Peers::AccessibilityView::Raw);
        readout=make_weak(display);
        entry.Resources().Insert(box_value(L"TextControlForegroundDisabled"),hiddenText);
        Grid valueBox;valueBox.UseLayoutRounding(false);Grid::SetColumn(valueBox,1);Grid::SetColumn(entry,0);
        valueBox.Children().Append(entry);
        if(panel){
            valueBox.Width(80);valueBox.HorizontalAlignment(HorizontalAlignment::Right);entry.Width(80);
            valueSurface=button(data,L"",[]{});valueSurface.Content(display);valueSurface.Padding({0});valueSurface.Height(34);
            valueSurface.IsTabStop(false);AutomationProperties::SetAccessibilityView(valueSurface,Automation::Peers::AccessibilityView::Raw);
            valueBox.Children().Append(valueSurface);
        }else valueBox.Children().Append(display);
        header.Children().Append(valueBox);
        entry.IsEnabledChanged([readout](auto&&,DependencyPropertyChangedEventArgs const& args){
            if(auto view=readout.get())view.Opacity(unbox_value<bool>(args.NewValue())?1.:.36);
        });
        entry.Loaded([readout](Windows::Foundation::IInspectable const& sender,auto&&){
            if(auto view=readout.get())view.Opacity(sender.as<TextBox>().IsEnabled()?1.:.36);
        });
    }else header.Children().Append(entry);
    AutomationProperties::SetName(entry,str(numericLabels,L"edit"));if(!identifier.empty())AutomationProperties::SetAutomationId(entry,identifier);
    Slider slider;
    auto presentCaption=[source=std::weak_ptr<WorkspaceData>(data),numericCopy,currentTitle,caption,description=presentation.descriptionText,root=make_weak(root),text=make_weak(text),detail=detailView,entry=make_weak(entry),slider=make_weak(slider)]{
        auto data=source.lock();auto control=entry.get();if(!data||!control)return false;
        auto title=currentTitle();auto labels=numericCopy();
        AutomationProperties::SetName(control,str(labels,L"edit"));
        if(auto view=root.get())AutomationProperties::SetName(view,title);
        if(auto view=text.get()){view.Text(title);tooltip(view,title);}
        if(auto view=detail.get();view&&description){auto descriptionText=description();view.Text(descriptionText);view.Visibility(descriptionText.empty()?Visibility::Collapsed:Visibility::Visible);}
        if(auto view=slider.get())AutomationProperties::SetName(view,title);
        if(caption->errorReason.Size()&&caption->errorLanguage!=data->language()){showNumericError(control,data->caption(O({{L"type",S(L"numeric_error")},{L"reason",caption->errorReason}})));caption->errorLanguage=data->language();}
        return true;
    };
    data->copyView(presentCaption);bindings.emplace_back([presentCaption]{presentCaption();});
    auto measureText=[data,local](hstring const& value){
        // Routine model updates retain the measured extent of unchanged text.
        if(local->measuredWidth>=0&&local->measuredText==value)return local->measuredWidth;
        auto measure=label(data,value,false,false);measure.UseLayoutRounding(false);
        measure.Measure({std::numeric_limits<float>::infinity(),32});
        local->measuredText=value;local->measuredWidth=double(measure.DesiredSize().Width);return local->measuredWidth;
    };
    auto surface=make_weak(valueSurface);
    auto setText=[data,local,measureText,readout,surface,hiddenText,ranged,valueOnly,inlineTrack,preference,panel,weak=make_weak(entry)](hstring const& value){
        bool previous=std::exchange(local->formatting,true);
        struct Reset{bool& value;bool previous;~Reset(){value=previous;}} reset{local->formatting,previous};
        if(auto control=weak.get()){
            if(control.Text()!=value)control.Text(value);
            if(auto display=readout.get()){
                bool reading=control.FocusState()==FocusState::Unfocused&&!local->editing;
                if(display.Text()!=value)display.Text(value);
                display.Visibility(reading?Visibility::Visible:Visibility::Collapsed);
                if(auto target=surface.get())target.Visibility(reading?Visibility::Visible:Visibility::Collapsed);
                control.Foreground(reading?hiddenText:data->brush(L"text"));
            }
            if(ranged&&!valueOnly&&!inlineTrack){
                auto width=panel?80.:control.FocusState()==FocusState::Unfocused?measureText(value)+(preference?18:12):92.;
                if(std::abs(control.Width()-width)>.01)control.Width(width);
            }
        }
    };
    entry.TextChanging([data,local,readout,surface,inlineTrack,panel](Windows::Foundation::IInspectable const& sender,auto&&){
        // Covers typing, paste, accessibility and IME edits, including after Enter.
        if(data->updating||local->formatting)return;
        local->editing=true;
        // Accessibility can set text before focusing the field. Show that draft
        // immediately and preserve it when focus subsequently enters.
        if(auto display=readout.get()){
            display.Visibility(Visibility::Collapsed);auto control=sender.as<TextBox>();
            if(auto target=surface.get())target.Visibility(Visibility::Collapsed);
            control.Foreground(data->brush(L"text"));if(!inlineTrack)control.Width(panel?80:92);
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
    // The stock outer thumb has a negative margin and remains visible even at
    // zero size. Keep the native range interaction with a transparent thumb.
    if(!preference){
        for(auto key:{L"SliderOuterThumbBackground",L"SliderOuterThumbBackgroundPointerOver",L"SliderOuterThumbBackgroundPressed",L"SliderThumbBorderBrush"})
            slider.Resources().Insert(box_value(key),clear());
        for(auto key:{L"SliderHorizontalThumbWidth",L"SliderHorizontalThumbHeight",L"SliderInnerThumbWidth",L"SliderInnerThumbHeight"})
            slider.Resources().Insert(box_value(key),box_value(0.));
    }
    AutomationProperties::SetName(slider,title);if(!identifier.empty())AutomationProperties::SetAutomationId(slider,identifier+L"-slider");
    auto palette=object(data->state,L"palette");
    auto panelColor=color(str(palette,L"panel")),textColor=color(str(palette,L"text"));
    auto track=fill({255,uint8_t((int(panelColor.R)+textColor.R)/2),
        uint8_t((int(panelColor.G)+textColor.G)/2),uint8_t((int(panelColor.B)+textColor.B)/2)});
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
    if(panel){
        auto restore=[local,spec,set,setText,finish]{
            bool active=std::exchange(local->scrubbing,false);local->pointer.reset();local->dragging=false;
            if(active){bool transaction=local->gesture;finish(L"cancel");local->value=local->origin;if(!transaction)set(local->value);}
            setText(str(local->resolve(spec,local->value,O({{L"type",S(L"format")}})),L"text"));
        };
        auto move=[local,spec,set,setText,phase](double y){
            double pixels=local->originY-y;
            auto dpi=GetDpiForSystem();double slop=std::max(2.,double(GetSystemMetricsForDpi(SM_CYDRAG,dpi))*96./std::max(96u,dpi));
            if(!local->scrubbing&&std::abs(pixels)<=slop)return;
            if(!std::exchange(local->scrubbing,true)&&phase){local->gesture=true;phase(L"down",local->origin);}
            auto next=local->resolve(spec,local->origin,O({{L"type",S(L"scrub")},{L"origin",N(local->origin)},{L"pixels",N(pixels)}}));
            local->value=num(next,L"value");setText(str(next,L"scrub_text"));set(local->value);
        };
        valueSurface.PointerPressed([local,commit,weak=make_weak(valueSurface)](auto&&,PointerRoutedEventArgs const& e){
            if(local->pointer||!commit(false))return;auto target=weak.get();if(!target||!target.IsEnabled())return;
            if(e.GetCurrentPoint(target).Properties().IsCanceled())return;
            local->pointer=e.Pointer().PointerId();local->origin=local->value;local->originY=e.GetCurrentPoint(nullptr).Position().Y;
            if(!target.CapturePointer(e.Pointer())){local->pointer.reset();return;}
            local->dragging=true;target.Focus(FocusState::Pointer);e.Handled(true);
        });
        valueSurface.PointerMoved([local,move,restore,identity=presentation.identity](auto&&,PointerRoutedEventArgs const& e){
            if(local->pointer!=e.Pointer().PointerId())return;
            if(e.GetCurrentPoint(nullptr).Properties().IsCanceled()||(identity&&local->identity!=identity())){restore();return;}
            move(e.GetCurrentPoint(nullptr).Position().Y);e.Handled(true);
        });
        valueSurface.PointerReleased([local,move,finish,setText,showText,restore,weak=make_weak(entry)](auto&&,PointerRoutedEventArgs const& e){
            if(local->pointer!=e.Pointer().PointerId())return;
            if(e.GetCurrentPoint(nullptr).Properties().IsCanceled()){restore();return;}
            bool scrub=local->scrubbing;if(scrub)move(e.GetCurrentPoint(nullptr).Position().Y);
            local->pointer.reset();local->dragging=false;local->scrubbing=false;
            if(scrub){finish(L"up");setText(showText(L"text"));}else if(auto entry=weak.get())entry.Focus(FocusState::Pointer);
            e.Handled(true);
        });
        valueSurface.PointerCanceled([restore](auto&&,auto&&){restore();});
        valueSurface.PointerCaptureLost([local,restore](auto&&,auto&&){if(local->pointer)restore();});
        valueSurface.KeyDown([local,restore](auto&&,KeyRoutedEventArgs const& e){if(e.Key()==Windows::System::VirtualKey::Escape&&local->pointer){restore();e.Handled(true);}});
        text.DoubleTapped([local,spec,set,commit,reset=presentation.reset](auto&&,DoubleTappedRoutedEventArgs const& e){
            commit(true);
            if(reset)reset();else if(spec.HasKey(L"default_value")&&spec.GetNamedValue(L"default_value").ValueType()==Windows::Data::Json::JsonValueType::Number){
                auto next=local->resolve(spec,local->value,O({{L"type",S(L"expression")},{L"text",S(L"")}}));local->value=num(next,L"value");set(local->value);
            }e.Handled(true);
        });
    }
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
    slider.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([local,phase](auto&&,auto&&){
        local->dragging=true;if(phase&&!local->gesture){local->gesture=true;phase(L"down",local->value);}
    })),true);
    slider.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler(
        [local,finish](auto&&,auto&&){local->dragging=false;finish(L"up");})),true);
    slider.PointerCaptureLost([local,finish](auto&&,auto&&){local->dragging=false;finish(L"up");});
    slider.PointerCanceled([local,finish](auto&&,auto&&){local->dragging=false;finish(L"cancel");});
    slider.ValueChanged([data,local,caption,spec,set,setText,phase,weak=make_weak(entry)](auto&&,Primitives::RangeBaseValueChangedEventArgs const& e){
        if(data->updating||local->formatting)return;
        auto next=local->resolve(spec,local->value,O({{L"type",S(L"position")},{L"position",N(e.NewValue())}}));
        local->value=num(next,L"value");local->editing=false;
        if(auto entry=weak.get()){
            showNumericError(entry,L"");caption->errorReason=J{};
            setText(str(next,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));
        }
        if(phase&&!local->gesture)Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread().TryEnqueue([local,set,value=local->value]{if(!local->gesture)set(value);});
        else set(local->value);
    });
    bindings.emplace_back([data,track]{
        auto palette=object(data->state,L"palette");
        auto panel=color(str(palette,L"panel")),ink=color(str(palette,L"text"));
        track.Color({255,uint8_t((int(panel.R)+ink.R)/2),uint8_t((int(panel.G)+ink.G)/2),uint8_t((int(panel.B)+ink.B)/2)});
    });
    bindings.emplace_back([data,local,caption,spec,get,entry,slider,setText,presented,finish,identity=presentation.identity]{
        if(identity && local->identity!=identity()){
            finish(L"cancel");
            local->identity=identity();local->editing=false;local->dragging=false;local->pointer.reset();local->scrubbing=false;
            showNumericError(entry,L"");caption->errorReason=J{};
        }
        if(local->editing||local->dragging)return;
        bool previous=std::exchange(local->formatting,true);
        struct Reset{bool& value;bool previous;~Reset(){value=previous;}} reset{local->formatting,previous};
        local->value=get();auto shown=local->resolve(spec,local->value,O({{L"type",S(L"format")}}));
        setText(presented?presented():str(shown,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));
        entry.Background(entry.FocusState()==FocusState::Unfocused?clear():data->brush(L"input"));
        slider.Value(num(shown,L"fill"));
    });
    root.Unloaded([local,finish](auto&&,auto&&){finish(L"cancel");local->pointer.reset();local->scrubbing=false;local->dragging=false;});
    if(inlineTrack){
        // Compact layer controls retain the same shared value/expression rules
        // and target guards as full numeric controls.
        header.Children().RemoveAt(0);header.Children().InsertAt(0,slider);
        header.ColumnSpacing(4);header.Height(24);
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
        root.Children().Append(entry);return root;
    }
    if(panel){
        slider.Margin({36,0,88,0});slider.VerticalAlignment(VerticalAlignment::Bottom);Grid::SetColumnSpan(slider,2);
        header.Children().Append(slider);root.Children().Append(header);return root;
    }
    StackPanel spin;spin.Orientation(Orientation::Horizontal);spin.Spacing(0);
    if(!ranged){
        header.Children().RemoveAt(1);spin.Children().Append(entry);
        Border frame;frame.Child(spin);frame.Background(data->brush(L"input"));frame.CornerRadius({6,6,6,6});
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
    Grid::SetColumn(slider,1);trackRow.Children().Append(slider);
    root.Children().Append(header);if(ranged)root.Children().Append(trackRow);return root;
}
}
