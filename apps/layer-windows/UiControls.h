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
    std::function<void(bool)> popupChanged;
    int popupCount=0;
    void popup(bool open){popupCount=std::max(0,popupCount+(open?1:-1));if(popupChanged)popupChanged(popupCount>0);}
    std::function<void(std::string)> send,document,input;
    J chrome=O({{L"held",B(false)},{L"dragging",B(false)},{L"popup_open",B(false)}});
    bool externalPopup=false;
    bool updating=false;
    mutable std::map<std::wstring,SolidColorBrush> paletteBrushes;
    void refreshPalette(){
        auto palette=object(state,L"palette");
        for(auto const& [role,brush]:paletteBrushes)brush.Color(color(str(palette,role.c_str(),L"#414141")));
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
    double textSize()const{return num(catalog,L"text_size_pt",11)*96./72.;}
};
// Shared button color is a tint; native Android/Web apply 13/255 opacity.
inline SolidColorBrush buttonBackground(std::shared_ptr<WorkspaceData> const& data){
    auto tint=color(str(object(data->state,L"palette"),L"button"));tint.A=13;return fill(tint);
}
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
struct NumberPresentation {bool preference=false;hstring description;};
StackPanel number(std::shared_ptr<WorkspaceData> const& data,hstring const& title,J const& spec,
    std::function<double()> get,std::function<void(double)> set,Bindings& bindings,Bindings* commits=nullptr,bool valueOnly=false,hstring const& identifier=L"",bool inlineTrack=false,NumberPresentation const& presentation={});
}
