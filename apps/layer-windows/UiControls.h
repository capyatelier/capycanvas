#pragma once
#include "pch.h"
#include "FilterPreviews.h"
#include "native/include/capy_windows.h"
#include <winrt/Microsoft.UI.Xaml.Automation.h>
#include <winrt/Microsoft.UI.Xaml.Media.Imaging.h>
#include <winrt/Windows.UI.Text.h>
#include <algorithm>
#include <cmath>
#include <filesystem>
#include <functional>
#include <memory>
#include <map>
#include <vector>

namespace CapyUi {
using namespace winrt;
using namespace Windows::Data::Json;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Media;
using namespace Microsoft::UI::Xaml::Input;
using Microsoft::UI::Xaml::Automation::AutomationProperties;
using J=JsonObject;
using A=JsonArray;
using V=IJsonValue;
}
namespace CapyUi {
inline V S(hstring const& value){return JsonValue::CreateStringValue(value);}
inline V N(double value){return JsonValue::CreateNumberValue(value);}
inline V B(bool value){return JsonValue::CreateBooleanValue(value);}
inline J O(std::initializer_list<std::pair<wchar_t const*,V>> fields){
    J value;for(auto const& [key,item]:fields)value.Insert(key,item);return value;
}
inline J object(J const& value,wchar_t const* key){
    auto item=value.GetNamedValue(key,JsonValue::CreateNullValue());
    return item.ValueType()==JsonValueType::Object?item.GetObject():J{};
}
inline A array(J const& value,wchar_t const* key){
    auto item=value.GetNamedValue(key,JsonValue::CreateNullValue());
    return item.ValueType()==JsonValueType::Array?item.GetArray():A{};
}
inline hstring str(J const& value,wchar_t const* key,hstring fallback=L""){
    auto item=value.GetNamedValue(key,JsonValue::CreateNullValue());
    return item.ValueType()==JsonValueType::String?item.GetString():fallback;
}
inline double num(J const& value,wchar_t const* key,double fallback=0){
    return value.GetNamedNumber(key,fallback);
}
inline bool flag(J const& value,wchar_t const* key,bool fallback=false){return value.GetNamedBoolean(key,fallback);}
inline J find(A const& list,wchar_t const* key,hstring const& id){
    for(auto value:list){auto row=value.GetObject();if(str(row,key)==id)return row;}return J{};
}
inline J findId(A const& list,double id){
    for(auto value:list){auto row=value.GetObject();if(num(row,L"id")==id)return row;}return J{};
}
inline Windows::UI::Color color(hstring const& hex){
    auto text=to_string(hex);
    unsigned long rgb=text.size()==7?std::stoul(text.substr(1),nullptr,16):0;
    return {255,uint8_t(rgb>>16),uint8_t(rgb>>8),uint8_t(rgb)};
}
inline SolidColorBrush fill(Windows::UI::Color value){return SolidColorBrush(value);}
inline SolidColorBrush clear(){return fill({0,0,0,0});}
inline SolidColorBrush selected(){return fill({56,53,132,228});}
inline void place(FrameworkElement const& element,J const& rect){
    Canvas::SetLeft(element,num(rect,L"x"));Canvas::SetTop(element,num(rect,L"y"));
    element.Width(num(rect,L"width"));element.Height(num(rect,L"height"));
}
inline Windows::Foundation::Uri asset(std::wstring const& relative){
    wchar_t executable[32768];auto length=GetModuleFileNameW(nullptr,executable,32768);
    auto path=std::filesystem::path(std::wstring(executable,length)).parent_path()/L"Assets"/relative;
    return Windows::Foundation::Uri(L"file:///"+path.generic_wstring());
}
inline Image icon(hstring name,hstring theme,double size=16){
    std::wstring file=name.c_str();
    if(!file.starts_with(L"layer-"))file=L"layer-"+file;
    if(!file.ends_with(L"-symbolic"))file+=L"-symbolic";
    Image result;result.Width(size);result.Height(size);
    result.Source(Imaging::SvgImageSource(asset(L"icons/"+std::wstring(theme.c_str())+L"/"+file+L".svg")));
    result.IsHitTestVisible(false);return result;
}
inline J numeric(J const& spec,double value,J const& operation){
    auto json=to_string(O({{L"control",spec},{L"value",N(value)},{L"operation",operation}}).Stringify());
    std::unique_ptr<char,decltype(&capy_string_free)> result(capy_number(json.c_str()),capy_string_free);
    if(!result)throw hresult_invalid_argument(to_hstring(capy_error()));
    return J::Parse(to_hstring(result.get()));
}
using Bindings=std::vector<std::function<void()>>;
struct WorkspaceData {
    J state,catalog,model;
    std::shared_ptr<FilterPreviewCache> previews;
    std::function<void(std::string)> send;
    bool updating=false;
    mutable std::map<std::wstring,SolidColorBrush> paletteBrushes;
    void refreshPalette(){
        auto palette=object(state,L"palette");
        for(auto const& [role,brush]:paletteBrushes)brush.Color(color(str(palette,role.c_str(),L"#414141")));
    }
    void dispatch(J const& action) const {send(to_string(action.Stringify()));}
    hstring theme()const{return str(state,L"theme",L"dark");}
    SolidColorBrush brush(wchar_t const* role)const{
        auto found=paletteBrushes.find(role);if(found!=paletteBrushes.end())return found->second;
        return paletteBrushes.emplace(role,fill(color(str(object(state,L"palette"),role,L"#414141")))).first->second;
    }
    double textSize()const{return num(catalog,L"text_size_pt",11)*96./72.;}
};
inline TextBlock label(std::shared_ptr<WorkspaceData> const& data,hstring const& text,bool bold=false){
    TextBlock result;result.Text(text);result.FontSize(data->textSize());
    result.FontFamily(FontFamily(L"Segoe UI"));result.Foreground(data->brush(L"text"));
    result.LineHeight(18);result.LineStackingStrategy(LineStackingStrategy::BlockLineHeight);
    if(bold)result.FontWeight(Windows::UI::Text::FontWeights::Bold());
    return result;
}
inline Button button(std::shared_ptr<WorkspaceData> const& data,hstring const& text,std::function<void()> action){
    Button result;result.Content(box_value(text));result.FontSize(data->textSize());
    result.FontFamily(FontFamily(L"Segoe UI"));result.Foreground(data->brush(L"text"));
    result.FontWeight(Windows::UI::Text::FontWeights::Bold());
    result.MinWidth(0);result.MinHeight(0);result.Padding(Thickness{0});
    result.BorderThickness(Thickness{0});result.CornerRadius(CornerRadius{6,6,6,6});
    result.Background(clear());AutomationProperties::SetName(result,text);
    auto ink=color(str(object(data->state,L"palette"),L"text"));
    auto hover=ink;hover.A=20;auto pressed=ink;pressed.A=41;auto disabled=ink;disabled.A=92;
    result.Resources().Insert(box_value(L"ButtonBackgroundPointerOver"),fill(hover));
    result.Resources().Insert(box_value(L"ButtonBackgroundPressed"),fill(pressed));
    result.Resources().Insert(box_value(L"ButtonBackgroundDisabled"),clear());
    result.Resources().Insert(box_value(L"ButtonForegroundDisabled"),fill(disabled));
    result.Click([action=std::move(action)](auto&&,auto&&){action();});
    return result;
}
struct NumberState {double value=0;bool editing=false,dragging=false,formatting=false;};
inline StackPanel number(std::shared_ptr<WorkspaceData> const& data,hstring const& title,J const& spec,
    std::function<double()> get,std::function<void(double)> set,Bindings& bindings,Bindings* commits=nullptr,bool valueOnly=false,hstring const& identifier=L""){
    auto local=std::make_shared<NumberState>();local->value=get();
    StackPanel root;root.Spacing(0);
    Grid header;ColumnDefinition left;left.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(left);
    ColumnDefinition right;right.Width({1,GridUnitType::Auto});header.ColumnDefinitions().Append(right);
    auto text=label(data,title);text.Margin(Thickness{6,0,6,0});text.VerticalAlignment(VerticalAlignment::Center);
    header.Children().Append(text);
    TextBox entry;entry.Width(72);entry.MinHeight(24);entry.Height(24);entry.Padding(Thickness{6,0,6,0});
    entry.FontSize(data->textSize());entry.Background(data->brush(L"input"));entry.BorderThickness(Thickness{0});
    entry.TextAlignment(TextAlignment::Right);Grid::SetColumn(entry,1);header.Children().Append(entry);
    AutomationProperties::SetName(entry,title);if(!identifier.empty())AutomationProperties::SetAutomationId(entry,identifier);
    auto setText=[local,weak=make_weak(entry)](hstring const& value){
        bool previous=std::exchange(local->formatting,true);
        struct Reset{bool& value;bool previous;~Reset(){value=previous;}} reset{local->formatting,previous};
        if(auto control=weak.get();control&&control.Text()!=value)control.Text(value);
    };
    entry.TextChanging([data,local](auto&&,auto&&){
        // Covers typing, paste, accessibility and IME edits, including after Enter.
        if(!data->updating&&!local->formatting)local->editing=true;
    });
    Slider slider;slider.Minimum(0);slider.Maximum(1);slider.StepFrequency(0.001);slider.MinHeight(0);slider.Height(24);
    // The adjacent field displays shared units; the default thumb tooltip
    // exposes only normalized 0..1 positions.
    slider.IsThumbToolTipEnabled(false);
    slider.Resources().Insert(box_value(L"SliderHorizontalHeight"),box_value(24.));
    for(auto key:{L"SliderHorizontalThumbWidth",L"SliderHorizontalThumbHeight",L"SliderInnerThumbWidth",L"SliderInnerThumbHeight"})
        slider.Resources().Insert(box_value(key),box_value(0.));
    AutomationProperties::SetName(slider,title+L" slider");if(!identifier.empty())AutomationProperties::SetAutomationId(slider,identifier+L"-slider");
    auto palette=object(data->state,L"palette");
    auto panelColor=color(str(palette,L"panel")),textColor=color(str(palette,L"text"));
    auto track=fill({255,uint8_t((int(panelColor.R)+textColor.R)/2),
        uint8_t((int(panelColor.G)+textColor.G)/2),uint8_t((int(panelColor.B)+textColor.B)/2)});
    for(auto key:{L"SliderTrackValueFill",L"SliderTrackValueFillPointerOver",L"SliderTrackValueFillPressed"})
        slider.Resources().Insert(box_value(key),track);
    for(auto key:{L"SliderThumbBackground",L"SliderThumbBackgroundPointerOver",L"SliderThumbBackgroundPressed"})
        slider.Resources().Insert(box_value(key),data->brush(L"thumb"));
    for(auto key:{L"SliderTrackFill",L"SliderTrackFillPointerOver",L"SliderTrackFillPressed"})
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
        if(!local->editing)setText(str(numeric(spec,local->value,O({{L"type",S(L"format")}})),L"edit"));
    });
    entry.LostFocus([commit,local,spec,setText](Windows::Foundation::IInspectable const& sender,RoutedEventArgs const&){
        auto entry=sender.as<TextBox>();commit(false);entry.Background(clear());
        if(!local->editing)setText(str(numeric(spec,local->value,O({{L"type",S(L"format")}})),L"text"));
    });
    entry.KeyDown([commit,local](auto&&,KeyRoutedEventArgs const& e){
        if(e.Key()==Windows::System::VirtualKey::Enter){commit(false);e.Handled(true);}
        else if(e.Key()==Windows::System::VirtualKey::Escape){commit(true);e.Handled(true);}
    });
    slider.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler(
        [local](auto&&,auto&&){local->dragging=true;})),true);
    slider.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler(
        [local](auto&&,auto&&){local->dragging=false;})),true);
    slider.PointerCaptureLost([local](auto&&,auto&&){local->dragging=false;});
    slider.ValueChanged([data,local,spec,set,setText](auto&&,Primitives::RangeBaseValueChangedEventArgs const& e){
        if(data->updating)return;
        auto next=numeric(spec,local->value,O({{L"type",S(L"position")},{L"position",N(e.NewValue())}}));
        local->value=num(next,L"value");if(!local->editing)setText(str(next,L"edit"));set(local->value);
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
    if(valueOnly){
        header.Children().RemoveAt(1);entry.ClearValue(FrameworkElement::WidthProperty());entry.MinWidth(0);
        entry.HorizontalAlignment(HorizontalAlignment::Stretch);entry.TextAlignment(TextAlignment::Center);
        root.Children().Append(entry);return root;
    }
    bool ranged=str(spec,L"kind")==L"slider";
    StackPanel spin;spin.Orientation(Orientation::Horizontal);spin.Spacing(6);
    if(!ranged){header.Children().RemoveAt(1);spin.Children().Append(entry);Grid::SetColumn(spin,1);header.Children().Append(spin);}
    Grid trackRow;trackRow.ColumnSpacing(6);
    for(int i=0;i<3;i++){ColumnDefinition column;column.Width({i==1?1.:24.,i==1?GridUnitType::Star:GridUnitType::Pixel});trackRow.ColumnDefinitions().Append(column);}
    for(int direction:{-1,1}){
        auto step=button(data,(direction<0?L"Decrease ":L"Increase ")+title,[local,spec,set,commit,direction]{
            commit(false);if(local->editing)return;
            auto next=numeric(spec,local->value,O({{L"type",S(L"step")},{L"steps",N(direction)}}));
            local->value=num(next,L"value");set(local->value);
        });
        step.Width(24);step.Height(24);step.Content(icon(direction<0?L"minus":L"plus",data->theme()));
        if(ranged){Grid::SetColumn(step,direction<0?0:2);trackRow.Children().Append(step);}else spin.Children().Append(step);
        bindings.emplace_back([local,spec,step,direction]{step.IsEnabled(direction<0?local->value>num(spec,L"min"):local->value<num(spec,L"max"));});
    }
    Grid::SetColumn(slider,1);trackRow.Children().Append(slider);
    root.Children().Append(header);if(ranged)root.Children().Append(trackRow);return root;
}
}
