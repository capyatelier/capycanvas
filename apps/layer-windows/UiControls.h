#pragma once
#include "pch.h"
#include "FilterPreviews.h"
#include "LayerThumbnails.h"
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
#include <optional>
#include <vector>
#include <type_traits>

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
struct StrokeRecording;
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
// Use the same trailing grip asset, orientation and inset as GTK/Android.
inline void orientGrip(FrameworkElement const& grip,bool vertical){
    auto transform=grip.RenderTransform().try_as<CompositeTransform>();
    if(!transform){transform=CompositeTransform();grip.RenderTransform(transform);}
    transform.Rotation(vertical?90.:0.);
    transform.TranslateX(vertical?0.:-1.6);transform.TranslateY(vertical?-1.6:0.);
}
inline Image panelGrip(hstring const& theme,bool vertical=false){
    auto result=icon(L"grip",theme);result.Opacity(.65);
    result.HorizontalAlignment(HorizontalAlignment::Center);result.VerticalAlignment(VerticalAlignment::Center);
    result.RenderTransformOrigin({.5f,.5f});orientGrip(result,vertical);return result;
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
    std::shared_ptr<LayerThumbnailCache> thumbnails;
    PreviewTransport query;
    uint64_t windowId=0;bool glassSurfaces=false;A drawerSources;
    std::shared_ptr<StrokeRecording> strokes;
    std::function<void(bool)> popupChanged;
    int popupCount=0;
    void popup(bool open){popupCount=std::max(0,popupCount+(open?1:-1));if(popupChanged)popupChanged(popupCount>0);}
    std::function<void(std::string)> send,document,input;
    J chrome=O({{L"held",B(false)},{L"dragging",B(false)},{L"popup_open",B(false)}});
    bool externalPopup=false;
    bool updating=false;
    std::map<uint64_t,std::function<bool()>> transients;uint64_t nextTransient=0;
    uint64_t transient(std::function<bool()> close){transients.emplace(++nextTransient,std::move(close));return nextTransient;}
    bool dismissTransients(){
        bool closed=false;auto current=transients;
        for(auto& [id,close]:current)closed=close()||closed;
        return closed;
    }
    J colorPreview;
    std::map<uint64_t,std::function<void()>> colorViews;uint64_t nextColorView=0,colorFields=0;bool colorQueued=false;
    uint64_t colorView(std::function<void()> refresh){colorViews.emplace(++nextColorView,std::move(refresh));return nextColorView;}
    static bool previewing(J const& preview){
        return object(preview,L"picker").GetNamedValue(L"preview",JsonValue::CreateNullValue()).ValueType()!=JsonValueType::Null;
    }
    mutable std::map<std::wstring,SolidColorBrush> paletteBrushes;
    mutable std::map<std::pair<std::wstring,uint8_t>,SolidColorBrush> tintBrushes;
    mutable std::map<std::wstring,SolidColorBrush> glassBrushes;
    Windows::UI::Color glassColor(wchar_t const* role)const{
        auto value=array(object(object(state,L"palette"),L"glass"),role);
        if(value.Size()!=4)return color(str(object(state,L"palette"),role,L"#414141"));
        auto channel=[&](uint32_t i){return uint8_t(std::lround(std::clamp(value.GetNumberAt(i),0.,1.)*255));};
        return Windows::UI::Color{channel(3),channel(0),channel(1),channel(2)};
    }
    void refreshPalette(){
        auto palette=object(state,L"palette");
        for(auto const& [role,brush]:paletteBrushes)brush.Color(color(str(palette,role.c_str(),L"#414141")));
        for(auto const& [role,brush]:glassBrushes)brush.Color(glassColor(role.c_str()));
        for(auto const& [key,brush]:tintBrushes){auto value=color(str(palette,key.first.c_str(),L"#414141"));value.A=key.second;brush.Color(value);}
    }
    void dispatch(J const& action) const {send(to_string(action.Stringify()));}
    void dispatchDocument(J const& action,hstring const& epoch) const {
        dispatch(O({{L"windows_epoch",S(epoch)},{L"action",action}}));
    }
    hstring theme()const{return str(state,L"theme",L"dark");}
    SolidColorBrush brush(wchar_t const* role)const{
        auto found=paletteBrushes.find(role);if(found!=paletteBrushes.end())return found->second;
        return paletteBrushes.emplace(role,fill(color(str(object(state,L"palette"),role,L"#414141")))).first->second;
    }
    SolidColorBrush glass(wchar_t const* role)const{
        auto found=glassBrushes.find(role);if(found!=glassBrushes.end())return found->second;
        return glassBrushes.emplace(role,fill(glassColor(role))).first->second;
    }
    bool transparent()const{return str(object(object(state,L"palette"),L"glass"),L"transparency",L"off")!=L"off";}
    SolidColorBrush tint(wchar_t const* role,uint8_t alpha)const{
        auto key=std::make_pair(std::wstring(role),alpha);
        if(auto found=tintBrushes.find(key);found!=tintBrushes.end())return found->second;
        auto value=color(str(object(state,L"palette"),role,L"#414141"));value.A=alpha;
        return tintBrushes.emplace(key,fill(value)).first->second;
    }
    double textSize()const{return num(catalog,L"text_size_pt",11)*96./72.;}
};
inline SolidColorBrush buttonBackground(std::shared_ptr<WorkspaceData> const& data){return data->tint(L"button",13);}
inline SolidColorBrush headerSurface(std::shared_ptr<WorkspaceData> const& data){return data->glass(L"chip");}
inline SolidColorBrush selected(std::shared_ptr<WorkspaceData> const& data){return data->glassSurfaces?data->glass(L"selection"):data->brush(L"selection");}
inline J displayColors(J const& state){
    auto masked=object(object(object(state,L"layer_tools"),L"mask_editing"),L"colors");
    return masked.Size()?masked:object(state,L"colors");
}
inline SolidColorBrush accent(std::shared_ptr<WorkspaceData> const& data){return data->brush(L"accent");}
inline TextBlock label(std::shared_ptr<WorkspaceData> const& data,hstring const& text,bool bold=false){
    TextBlock result;result.Text(text);result.FontSize(data->textSize());
    result.FontFamily(FontFamily(L"Segoe UI"));result.Foreground(data->brush(L"text"));
    result.LineHeight(18);result.LineStackingStrategy(LineStackingStrategy::BlockLineHeight);
    if(bold)result.FontWeight(Windows::UI::Text::FontWeights::Bold());
    return result;
}
template<typename T>
inline void buttonColors(std::shared_ptr<WorkspaceData> const& data,T const& result){
    hstring prefix=std::is_same_v<T,Primitives::ToggleButton>?L"ToggleButton":L"Button";
    result.Resources().Insert(box_value(prefix+L"BackgroundPointerOver"),data->tint(L"text",20));
    result.Resources().Insert(box_value(prefix+L"BackgroundPressed"),data->tint(L"text",41));
    result.Resources().Insert(box_value(prefix+L"BackgroundDisabled"),clear());
    result.Resources().Insert(box_value(prefix+L"ForegroundDisabled"),data->tint(L"text",92));
    if constexpr(std::is_same_v<T,Primitives::ToggleButton>)
        for(auto role:{L"ToggleButtonBackgroundChecked",L"ToggleButtonBackgroundCheckedPointerOver",L"ToggleButtonBackgroundCheckedPressed"})
            result.Resources().Insert(box_value(role),selected(data));
}
template<typename T=Button>
inline T button(std::shared_ptr<WorkspaceData> const& data,hstring const& text,std::function<void()> action){
    T result;result.Content(box_value(text));result.FontSize(data->textSize());
    result.FontFamily(FontFamily(L"Segoe UI"));result.Foreground(data->brush(L"text"));
    result.FontWeight(Windows::UI::Text::FontWeights::Bold());
    result.MinWidth(0);result.MinHeight(0);result.Padding(Thickness{0});
    result.BorderThickness(Thickness{0});result.CornerRadius(CornerRadius{6,6,6,6});
    result.Background(clear());AutomationProperties::SetName(result,text);
    buttonColors(data,result);
    result.Click([action=std::move(action)](auto&&,auto&&){action();});
    return result;
}
inline bool& touchContact(){thread_local bool touch=false;return touch;}
inline void tooltip(DependencyObject const& target,hstring const& text){
    if(auto current=ToolTipService::GetToolTip(target).try_as<ToolTip>()){
        if(unbox_value_or<hstring>(current.Content(),L"")!=text)current.Content(box_value(text));
        return;
    }
    ToolTip tip;tip.Content(box_value(text));
    tip.Opened([](winrt::Windows::Foundation::IInspectable const& sender,auto&&){if(touchContact())sender.as<ToolTip>().IsOpen(false);});
    ToolTipService::SetToolTip(target,tip);
}
inline bool pickerControl(J const& control){
    auto kind=str(control,L"kind");return kind==L"color_picker"||(kind==L"command"&&str(control,L"command")==L"eyedropper");
}
inline hstring pickerTooltip(hstring const& tooltip){return tooltip+L" · Double-press for options";}
struct DoublePress : std::enable_shared_from_this<DoublePress> {
    using Press=std::pair<uint64_t,Microsoft::UI::Input::PointerDeviceType>;
    std::optional<Press> pressed,last;
    void listen(UIElement const& target){
        target.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak=weak_from_this()](auto&&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock())self->pressed=Press{e.GetCurrentPoint(nullptr).Timestamp(),e.Pointer().PointerDeviceType()};
        })),true);
    }
    bool second(){
        auto now=std::exchange(pressed,std::nullopt);auto previous=std::exchange(last,now);
        bool twice=now&&previous&&now->second==previous->second&&now->first-previous->first<=uint64_t(GetDoubleClickTime())*1000;
        if(twice)last.reset();
        return twice;
    }
    void reset(){pressed.reset();last.reset();}
};
struct NumberPresentation {
    bool preference=false;hstring description;std::function<hstring()> identity;
    std::vector<hstring> widthSamples;std::function<J(J const&,double,J const&)> resolve;
};
StackPanel number(std::shared_ptr<WorkspaceData> const& data,hstring const& title,J const& spec,
    std::function<double()> get,std::function<void(double)> set,Bindings& bindings,Bindings* commits=nullptr,bool valueOnly=false,hstring const& identifier=L"",bool inlineTrack=false,NumberPresentation const& presentation={});
}
