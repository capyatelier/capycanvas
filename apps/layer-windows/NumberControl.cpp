#include "pch.h"
#include "UiControls.h"
#include <limits>

namespace CapyUi {
namespace {
struct NumberState {
    double value=0;bool editing=false,dragging=false,formatting=false;
    hstring measuredText;double measuredWidth=-1;
};
}
StackPanel number(std::shared_ptr<WorkspaceData> const& data,hstring const& title,J const& spec,
    std::function<double()> get,std::function<void(double)> set,Bindings& bindings,Bindings* commits,bool valueOnly,hstring const& identifier,bool inlineTrack,NumberPresentation const& presentation){
    auto local=std::make_shared<NumberState>();local->value=get();
    bool ranged=str(spec,L"kind")==L"slider",preference=presentation.preference;
    double valueHeight=preference?34.:(ranged?24.:32.),stepSize=ranged&&!preference?24.:32.;
    StackPanel root;root.Spacing(0);
    auto numberId=identifier.empty()?title:identifier;
    AutomationProperties::SetAutomationId(root,L"number-root-"+numberId);
    AutomationProperties::SetName(root,title+L" numeric control");
    Grid header;header.UseLayoutRounding(false);header.ColumnSpacing(6);header.MinHeight(valueHeight);ColumnDefinition left;left.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(left);
    ColumnDefinition right;right.Width({1,GridUnitType::Auto});header.ColumnDefinitions().Append(right);
    auto text=label(data,title);text.Margin(Thickness{preference?0.:6.,0,0,0});text.VerticalAlignment(VerticalAlignment::Center);
    text.LineHeight(20);text.TextTrimming(TextTrimming::CharacterEllipsis);
    ToolTipService::SetToolTip(text,box_value(title));
    StackPanel labels;labels.UseLayoutRounding(false);labels.VerticalAlignment(VerticalAlignment::Center);labels.Children().Append(text);
    if(!presentation.description.empty()){
        auto detail=label(data,presentation.description);detail.FontSize(data->textSize()/1.2);
        detail.LineHeight(detail.FontSize()*1.25);detail.TextWrapping(TextWrapping::Wrap);detail.Opacity(.55);
        labels.Children().Append(detail);
    }
    header.Children().Append(labels);
    TextBox entry;entry.UseLayoutRounding(false);entry.Width(ranged?72:60);entry.MinWidth(0);entry.MinHeight(0);entry.Height(valueHeight);entry.Padding(Thickness{preference?9.:6.,(valueHeight-20)/2,preference?9.:6.,(valueHeight-20)/2});
    entry.VerticalAlignment(VerticalAlignment::Center);entry.VerticalContentAlignment(VerticalAlignment::Center);
    entry.FontSize(data->textSize());entry.FontFamily(FontFamily(L"Segoe UI"));entry.Foreground(data->brush(L"text"));
    entry.Background(data->brush(L"input"));entry.BorderThickness(Thickness{0});entry.CornerRadius({6,6,6,6});
    entry.Resources().Insert(box_value(L"TextControlBackgroundDisabled"),clear());
    auto disabledText=clear(),hiddenText=clear();
    entry.Resources().Insert(box_value(L"TextControlForegroundDisabled"),disabledText);
    bindings.emplace_back([data,ranged,disabledText]{
        auto ink=color(str(object(data->state,L"palette"),L"text"));if(ranged)ink.A=92;disabledText.Color(ink);
    });
    entry.TextAlignment(TextAlignment::Right);Grid::SetColumn(entry,1);
    weak_ref<TextBlock> readout;
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
        valueBox.Children().Append(entry);valueBox.Children().Append(display);header.Children().Append(valueBox);
        entry.IsEnabledChanged([readout](auto&&,DependencyPropertyChangedEventArgs const& args){
            if(auto view=readout.get())view.Opacity(unbox_value<bool>(args.NewValue())?1.:.36);
        });
        entry.Loaded([readout](Windows::Foundation::IInspectable const& sender,auto&&){
            if(auto view=readout.get())view.Opacity(sender.as<TextBox>().IsEnabled()?1.:.36);
        });
    }else header.Children().Append(entry);
    AutomationProperties::SetName(entry,title);if(!identifier.empty())AutomationProperties::SetAutomationId(entry,identifier);
    auto measureText=[data,local](hstring const& value){
        // Routine model updates retain the measured extent of unchanged text.
        if(local->measuredWidth>=0&&local->measuredText==value)return local->measuredWidth;
        auto measure=label(data,value);measure.UseLayoutRounding(false);
        measure.Measure({std::numeric_limits<float>::infinity(),32});
        local->measuredText=value;local->measuredWidth=double(measure.DesiredSize().Width);return local->measuredWidth;
    };
    auto setText=[data,local,measureText,readout,hiddenText,ranged,valueOnly,inlineTrack,preference,weak=make_weak(entry)](hstring const& value){
        bool previous=std::exchange(local->formatting,true);
        struct Reset{bool& value;bool previous;~Reset(){value=previous;}} reset{local->formatting,previous};
        if(auto control=weak.get()){
            if(control.Text()!=value)control.Text(value);
            if(auto display=readout.get()){
                bool reading=control.FocusState()==FocusState::Unfocused&&!local->editing;
                if(display.Text()!=value)display.Text(value);
                display.Visibility(reading?Visibility::Visible:Visibility::Collapsed);
                control.Foreground(reading?hiddenText:data->brush(L"text"));
            }
            if(ranged&&!valueOnly&&!inlineTrack){
                auto width=control.FocusState()==FocusState::Unfocused?measureText(value)+(preference?18:12):92.;
                if(std::abs(control.Width()-width)>.01)control.Width(width);
            }
        }
    };
    entry.TextChanging([data,local,readout,inlineTrack](Windows::Foundation::IInspectable const& sender,auto&&){
        // Covers typing, paste, accessibility and IME edits, including after Enter.
        if(data->updating||local->formatting)return;
        local->editing=true;
        // Accessibility can set text before focusing the field. Show that draft
        // immediately and preserve it when focus subsequently enters.
        if(auto display=readout.get()){
            display.Visibility(Visibility::Collapsed);auto control=sender.as<TextBox>();
            control.Foreground(data->brush(L"text"));if(!inlineTrack)control.Width(92);
        }
    });
    Slider slider;slider.Minimum(0);slider.Maximum(1);slider.StepFrequency(0.001);slider.MinHeight(0);slider.Height(stepSize);
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
    AutomationProperties::SetName(slider,title+L" slider");if(!identifier.empty())AutomationProperties::SetAutomationId(slider,identifier+L"-slider");
    auto palette=object(data->state,L"palette");
    auto panelColor=color(str(palette,L"panel")),textColor=color(str(palette,L"text"));
    auto track=fill({255,uint8_t((int(panelColor.R)+textColor.R)/2),
        uint8_t((int(panelColor.G)+textColor.G)/2),uint8_t((int(panelColor.B)+textColor.B)/2)});
    for(auto key:{L"SliderTrackValueFill",L"SliderTrackValueFillPointerOver",L"SliderTrackValueFillPressed",L"SliderTrackValueFillDisabled"})
        slider.Resources().Insert(box_value(key),preference?fill({255,53,132,228}):track);
    for(auto key:{L"SliderThumbBackground",L"SliderThumbBackgroundPointerOver",L"SliderThumbBackgroundPressed"})
        slider.Resources().Insert(box_value(key),preference?fill({255,53,132,228}):data->brush(L"thumb"));
    for(auto key:{L"SliderTrackFill",L"SliderTrackFillPointerOver",L"SliderTrackFillPressed",L"SliderTrackFillDisabled"})
        slider.Resources().Insert(box_value(key),data->brush(L"input"));
    auto commit=[data,local,spec,set,setText,weak=make_weak(entry)](bool cancel){
        auto entry=weak.get();if(!entry||!local->editing)return;
        try{
            auto next=numeric(spec,local->value,cancel?O({{L"type",S(L"format")}}):
                O({{L"type",S(L"expression")},{L"text",S(entry.Text())}}));
            bool changed=local->value!=num(next,L"value");
            local->value=num(next,L"value");local->editing=false;
            setText(str(next,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));entry.BorderThickness(Thickness{0});
            ToolTipService::SetToolTip(entry,nullptr);
            if(!cancel&&changed)set(local->value);
        }catch(hresult_error const& error){
            entry.BorderThickness(Thickness{1,1,1,1});entry.BorderBrush(fill({255,221,85,85}));
            ToolTipService::SetToolTip(entry,box_value(error.message()));
        }
    };
    if(commits)commits->emplace_back([commit]{commit(false);});
    entry.GotFocus([data,local,spec,setText](Windows::Foundation::IInspectable const& sender,RoutedEventArgs const&){
        auto entry=sender.as<TextBox>();entry.Background(data->brush(L"input"));
        if(!local->editing){setText(str(numeric(spec,local->value,O({{L"type",S(L"format")}})),L"edit"));}
    });
    // LosingFocus is synchronous; close and target-change commands must follow
    // the draft commit, rather than race the later LostFocus notification.
    entry.LosingFocus([commit](auto&&,auto&&){commit(false);});
    entry.LostFocus([commit,local,spec,setText](Windows::Foundation::IInspectable const& sender,RoutedEventArgs const&){
        auto entry=sender.as<TextBox>();commit(false);entry.Background(clear());
        if(!local->editing)setText(str(numeric(spec,local->value,O({{L"type",S(L"format")}})),L"text"));
    });
    entry.KeyDown([commit](auto&&,KeyRoutedEventArgs const& e){
        if(e.Key()==Windows::System::VirtualKey::Enter){commit(false);e.Handled(true);}
        else if(e.Key()==Windows::System::VirtualKey::Escape){commit(true);e.Handled(true);}
    });
    // TextBox consumes some arrow keys before the bubbling KeyDown event.
    // Numeric spin steps must take precedence over its caret navigation.
    if(!ranged)entry.PreviewKeyDown([commit,local,spec,set](auto&&,KeyRoutedEventArgs const& e){
        if(e.Key()!=Windows::System::VirtualKey::Up&&e.Key()!=Windows::System::VirtualKey::Down)return;
        e.Handled(true);commit(false);if(local->editing)return;
        auto next=numeric(spec,local->value,O({{L"type",S(L"step")},{L"steps",N(e.Key()==Windows::System::VirtualKey::Up?1:-1)}}));
        local->value=num(next,L"value");set(local->value);
    });
    slider.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler(
        [local](auto&&,auto&&){local->dragging=true;})),true);
    slider.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler(
        [local](auto&&,auto&&){local->dragging=false;})),true);
    slider.PointerCaptureLost([local](auto&&,auto&&){local->dragging=false;});
    slider.ValueChanged([data,local,spec,set,setText,weak=make_weak(entry)](auto&&,Primitives::RangeBaseValueChangedEventArgs const& e){
        if(data->updating)return;
        auto next=numeric(spec,local->value,O({{L"type",S(L"position")},{L"position",N(e.NewValue())}}));
        local->value=num(next,L"value");local->editing=false;
        if(auto entry=weak.get()){
            entry.BorderThickness({0});ToolTipService::SetToolTip(entry,nullptr);
            setText(str(next,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));
        }
        set(local->value);
    });
    bindings.emplace_back([data,track]{
        auto palette=object(data->state,L"palette");
        auto panel=color(str(palette,L"panel")),ink=color(str(palette,L"text"));
        track.Color({255,uint8_t((int(panel.R)+ink.R)/2),uint8_t((int(panel.G)+ink.G)/2),uint8_t((int(panel.B)+ink.B)/2)});
    });
    bindings.emplace_back([data,local,spec,get,entry,slider,setText]{
        if(local->editing||local->dragging)return;
        local->value=get();auto shown=numeric(spec,local->value,O({{L"type",S(L"format")}}));
        setText(str(shown,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));
        entry.Background(entry.FocusState()==FocusState::Unfocused?clear():data->brush(L"input"));
        slider.Value(num(shown,L"fill"));
    });
    if(inlineTrack){
        // Compact layer controls retain the same shared value/expression rules
        // and target guards as full numeric controls.
        header.Children().RemoveAt(0);header.Children().InsertAt(0,slider);
        header.ColumnSpacing(4);header.Height(24);
        auto low=str(numeric(spec,num(spec,L"min"),O({{L"type",S(L"format")}})),L"text");
        auto high=str(numeric(spec,num(spec,L"max"),O({{L"type",S(L"format")}})),L"text");
        std::wstring measure(low.size()>high.size()?low.c_str():high.c_str());
        for(auto& ch:measure)if(ch>=L'0'&&ch<=L'9')ch=L'8';
        entry.Width(measureText(hstring(measure))+12);entry.MinWidth(0);
        slider.MinWidth(0);root.Children().Append(header);return root;
    }
    if(valueOnly){
        header.Children().RemoveAt(1);entry.ClearValue(FrameworkElement::WidthProperty());entry.MinWidth(0);
        entry.HorizontalAlignment(HorizontalAlignment::Stretch);entry.TextAlignment(TextAlignment::Center);
        root.Children().Append(entry);return root;
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
        auto step=button(data,(direction<0?L"Decrease ":L"Increase ")+title,[local,spec,set,commit,direction]{
            commit(false);if(local->editing)return;
            auto next=numeric(spec,local->value,O({{L"type",S(L"step")},{L"steps",N(direction)}}));
            local->value=num(next,L"value");set(local->value);
        });
        step.Width(stepSize);step.Height(stepSize);step.Content(icon(direction<0?L"minus":L"plus",data->theme()));
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
