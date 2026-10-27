#pragma once
#include "pch.h"
#include "BitmapUpload.h"
#include "FilterPreviews.h"
#include "LayerThumbnails.h"
#include "PointerGesture.h"
#include "NativeHandles.h"
#include <winrt/Microsoft.UI.Xaml.Automation.h>
#include <winrt/Microsoft.UI.Xaml.Media.Imaging.h>
#include <winrt/Windows.UI.Text.h>
#include <algorithm>
#include <cmath>
#include <filesystem>
#include <functional>
#include <unordered_map>
#include <memory>
#include <map>
#include <optional>
#include <vector>
#include <type_traits>

namespace CapyUi {
constexpr float CornerFit=.54f;
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
struct ScopeFeed;
struct ColorStrip;
struct PaperPreviewCache;
void captureTextComposition(TextBox const& entry,std::function<void(bool)> localizationInput={});
bool textComposing(DependencyObject element);
bool focusedTextComposing(XamlRoot const& root);
bool composingKey(KeyRoutedEventArgs const& event);
template<class Handler>
inline std::pair<event_token,event_token> ShortcutKeys(UIElement const& element,Handler handler){
    auto preview=element.PreviewKeyDown([handler](auto&& sender,KeyRoutedEventArgs const& e){
        if(e.Key()!=Windows::System::VirtualKey::Escape)handler(sender,e);
    });
    auto bubble=element.KeyDown([handler](auto&& sender,KeyRoutedEventArgs const& e){
        if(e.Key()==Windows::System::VirtualKey::Escape)handler(sender,e);
    });
    return {preview,bubble};
}
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
    auto item=value.GetNamedValue(key,JsonValue::CreateNullValue());
    return item.ValueType()==JsonValueType::Number?item.GetNumber():fallback;
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
inline std::filesystem::path assetPath(std::wstring const& relative){
    wchar_t executable[32768];auto length=GetModuleFileNameW(nullptr,executable,32768);
    return std::filesystem::path(std::wstring(executable,length)).parent_path()/L"Assets"/relative;
}
inline Windows::Foundation::Uri asset(std::wstring const& relative){
    return Windows::Foundation::Uri(L"file:///"+assetPath(relative).generic_wstring());
}
inline Microsoft::UI::Xaml::Media::ImageSource bundledImage(std::wstring const& relative,std::function<void()> ready={}){
    return BitmapUpload::Bundled(assetPath(relative),std::move(ready));
}
struct MouseScrollbars {
    struct Entry { weak_ref<DependencyObject> node; std::function<void(bool)> show; };
    std::vector<Entry> entries;
    bool mouse=false;
    HHOOK hook=nullptr;
    static MouseScrollbars& current(){static thread_local MouseScrollbars value;return value;}
    void input(bool next){
        if(mouse==next)return;mouse=next;
        std::erase_if(entries,[](auto const& entry){return !entry.node.get();});
        for(auto const& entry:entries)entry.show(mouse);
    }
    static LRESULT CALLBACK message(int code,WPARAM action,LPARAM payload){
        auto& self=current();
        if(code>=0&&action==PM_REMOVE){
            auto event=reinterpret_cast<MSG*>(payload);
            if(event->message==WM_POINTERDOWN||event->message==WM_POINTERUPDATE||event->message==WM_POINTERENTER){
                POINTER_INPUT_TYPE type;
                if(GetPointerType(GET_POINTERID_WPARAM(event->wParam),&type))self.input(type==PT_MOUSE);
            }else if(event->message==WM_MOUSEMOVE||event->message==WM_LBUTTONDOWN||event->message==WM_MOUSEWHEEL)
                self.input((ULONG_PTR(GetMessageExtraInfo())&0xFFFFFF00)!=0xFF515700);
        }
        return CallNextHookEx(self.hook,code,action,payload);
    }
    MouseScrollbars(){hook=SetWindowsHookExW(WH_GETMESSAGE,message,nullptr,GetCurrentThreadId());}
    ~MouseScrollbars(){if(hook)UnhookWindowsHookEx(hook);}
};
inline void mouseScrollbars(ScrollView const& scroll){
    auto horizontal=scroll.HorizontalScrollBarVisibility(),vertical=scroll.VerticalScrollBarVisibility();
    auto& mode=MouseScrollbars::current();auto weak=make_weak(scroll);
    auto show=[weak,horizontal,vertical](bool mouse){if(auto value=weak.get()){
        value.HorizontalScrollBarVisibility(mouse?horizontal:ScrollingScrollBarVisibility::Hidden);
        value.VerticalScrollBarVisibility(mouse?vertical:ScrollingScrollBarVisibility::Hidden);
    }};
    mode.entries.push_back({make_weak(scroll.as<DependencyObject>()),show});show(mode.mouse);
}
inline void mouseScrollbars(ScrollViewer const& scroll){
    auto horizontal=scroll.HorizontalScrollBarVisibility(),vertical=scroll.VerticalScrollBarVisibility();
    auto& mode=MouseScrollbars::current();auto weak=make_weak(scroll);
    auto show=[weak,horizontal,vertical](bool mouse){if(auto value=weak.get()){
        value.HorizontalScrollBarVisibility(mouse||horizontal==ScrollBarVisibility::Disabled?horizontal:ScrollBarVisibility::Hidden);
        value.VerticalScrollBarVisibility(mouse||vertical==ScrollBarVisibility::Disabled?vertical:ScrollBarVisibility::Hidden);
    }};
    mode.entries.push_back({make_weak(scroll.as<DependencyObject>()),show});show(mode.mouse);
}
inline void scrollVertical(ScrollView const& scroll,double offset){
    static thread_local ScrollingScrollOptions options{ScrollingAnimationMode::Disabled,ScrollingSnapPointsMode::Ignore};scroll.ScrollTo(scroll.HorizontalOffset(),offset,options);
}
inline void dragCursor(UIElement const& element,bool touch,bool dragging,bool held=false){
    using namespace Microsoft::UI::Input;
    auto target=element.as<IUIElementProtected>();
    if(touch||(!dragging&&!held)){target.ProtectedCursor(nullptr);return;}
    target.ProtectedCursor(InputSystemCursor::Create(dragging?InputSystemCursorShape::SizeAll:InputSystemCursorShape::Hand));
}
inline std::optional<Microsoft::UI::Input::InputSystemCursorShape> resizeCursor(J const& action,J const& layout){
    using Shape=Microsoft::UI::Input::InputSystemCursorShape;
    auto type=str(action,L"type");
    if(type==L"drag_divider"){
        auto divider=findId(array(layout,L"dividers"),num(action,L"id"));
        if(divider.Size())return str(divider,L"axis")==L"horizontal"?Shape::SizeWestEast:Shape::SizeNorthSouth;
    }else if(type==L"resize_floating"){
        auto edge=str(action,L"edge");
        if(edge==L"left"||edge==L"right")return Shape::SizeWestEast;
        if(edge==L"top"||edge==L"bottom")return Shape::SizeNorthSouth;
        if(edge==L"top_left"||edge==L"bottom_right")return Shape::SizeNorthwestSoutheast;
        if(edge==L"top_right"||edge==L"bottom_left")return Shape::SizeNortheastSouthwest;
    }
    return {};
}
inline void edgeTag(FrameworkElement const& element,wchar_t const* key,V const& value){
    auto previous=element.Tag().try_as<J>();auto tag=previous?J::Parse(previous.Stringify()):J{};
    tag.Insert(key,value);element.Tag(tag);
}
inline void setIcon(Image const& result,hstring name,hstring theme,double size=16){
    std::wstring file=name.c_str();
    if(!file.starts_with(L"layer-"))file=L"layer-"+file;
    if(!file.ends_with(L"-symbolic"))file+=L"-symbolic";
    result.Width(size);result.Height(size);result.IsHitTestVisible(false);
    auto uri=asset(L"icons/"+std::wstring(theme.c_str())+L"/"+file+L".svg");
    auto source=result.Source().try_as<Imaging::SvgImageSource>();
    if(!source||!source.UriSource()||source.UriSource().AbsoluteUri()!=uri.AbsoluteUri())result.Source(Imaging::SvgImageSource(uri));
}
inline Image icon(hstring name,hstring theme,double size=16){
    Image result;setIcon(result,name,theme,size);return result;
}
// Use the same trailing grip asset, orientation and inset as GTK/Android.
inline void orientGrip(FrameworkElement const& grip,bool vertical){
    auto transform=grip.RenderTransform().try_as<CompositeTransform>();
    if(!transform){transform=CompositeTransform();grip.RenderTransform(transform);}
    transform.Rotation(vertical?90.:0.);
    transform.TranslateX(vertical?0.:-2.);transform.TranslateY(vertical?-2.:0.);
}
inline Image panelGrip(hstring const& theme,bool vertical=false){
    auto result=icon(L"grip",theme);result.Opacity(.65);
    result.HorizontalAlignment(HorizontalAlignment::Center);result.VerticalAlignment(VerticalAlignment::Center);
    result.RenderTransformOrigin({.5f,.5f});orientGrip(result,vertical);return result;
}
inline J exportDraft(CapyLocalization  localization,J const& request){
    auto input=to_string(request.Stringify());NativeString raw(capy_export_draft(localization,input.c_str()),capy_string_free);
    if(!raw)throw hresult_error(E_OUTOFMEMORY);return J::Parse(to_hstring(raw.get()));
}
struct NumericFailure : hresult_invalid_argument {
    J reason;
    NumericFailure(hstring const& message,J value):hresult_invalid_argument(message),reason(std::move(value)){}
};
inline J numeric(CapyLocalization  localization,J const& spec,double value,J const& operation){
    auto json=to_string(O({{L"control",spec},{L"value",N(value)},{L"operation",operation}}).Stringify());
    NativeString result(capy_number(localization,json.c_str()),capy_string_free);
    if(!result)throw hresult_invalid_argument(to_hstring(NativeString(capy_error()).get()));
    auto response=J::Parse(to_hstring(result.get()));
    if(response.HasKey(L"error_reason"))throw NumericFailure(str(response,L"error"),object(response,L"error_reason"));
    return response;
}
using Bindings=std::vector<std::function<void()>>;
using NumericAdmissions=std::vector<std::function<bool(bool)>>;
struct LocalizedCopy : hstring {
    std::function<hstring()> current;
    LocalizedCopy(hstring value,std::function<hstring()> resolve):hstring(value),current(std::move(resolve)){}
};
struct WorkspaceData : std::enable_shared_from_this<WorkspaceData> {
    NativeLocalization localization;
    uint64_t localizationGeneration=uint64_t(-1);
    std::vector<std::function<bool()>> copyViews;
    size_t copyCleanup=64;
    void copyView(std::function<bool()> update){
        if(copyViews.size()>=copyCleanup){std::erase_if(copyViews,[](auto const& refresh){return !refresh();});copyCleanup=std::max(size_t(64),copyViews.size()*2);}
        copyViews.emplace_back(std::move(update));
    }
    bool adoptLocalization(J const& snapshot){
        auto presentation=object(snapshot,L"localization");
        if(!presentation.Size())return false;
        auto generation=uint64_t(num(presentation,L"generation"));
        if(localizationGeneration==generation)return false;
        auto nextCatalog=object(presentation,L"catalog"),bootstrap=object(presentation,L"bootstrap");
        auto tag=to_string(str(bootstrap,L"active_tag"));
        NativeLocalization next(capy_localization_for_tag(tag.c_str()),capy_localization_free);
        if(!next)throw hresult_invalid_argument(L"Invalid localization presentation");
        if(snapshot.HasKey(L"state")){model=snapshot;state=object(snapshot,L"state");}
        nextCatalog.Insert(L"bootstrap",bootstrap);catalog=nextCatalog;localization=std::move(next);localizationGeneration=generation;
        std::erase_if(copyViews,[](auto const& update){return !update();});
        return true;
    }
    J state,catalog,model;
    std::shared_ptr<FilterPreviewCache> previews;
    std::shared_ptr<LayerThumbnailCache> thumbnails;
    std::shared_ptr<PaperPreviewCache> papers;
    PreviewTransport query;
    std::function<void(J,Windows::Foundation::Point,std::function<bool()>,std::function<void()>)> takeBubble;
    uint64_t windowId=0;bool glassSurfaces=false;A drawerSources;
    std::shared_ptr<StrokeRecording> strokes;
    std::shared_ptr<ScopeFeed> scopes;
    std::shared_ptr<ColorStrip> colorStrip;
    std::function<void(bool)> popupChanged;
    int popupCount=0;
    void popup(bool open){popupCount=std::max(0,popupCount+(open?1:-1));if(popupChanged)popupChanged(popupCount>0);}
    std::function<void(std::string)> send,document,input;
    std::function<void(std::string,std::function<void(int32_t)>)> edges;
    std::function<bool(uint32_t)> edgeHolding,cancelEdge;
    J chrome=O({{L"held",B(false)},{L"dragging",B(false)},{L"popup_open",B(false)}});
    bool externalPopup=false;
    bool updating=false;
    PointerCancelRegistry pointerCancels;
    std::function<bool(PointerRoutedEventArgs const&)> bubbleContact;
    bool claimBubbleContact(PointerRoutedEventArgs const& event){return bubbleContact&&bubbleContact(event);}
    std::map<uint64_t,std::function<bool()>> transients;uint64_t nextTransient=0;
    uint64_t transient(std::function<bool()> close){transients.emplace(++nextTransient,std::move(close));return nextTransient;}
    bool dismissTransients(){
        bool closed=false;auto current=transients;
        for(auto& [id,close]:current)closed=close()||closed;
        return closed;
    }
    J colorPreview;
    std::map<uint64_t,std::function<void()>> colorViews;uint64_t nextColorView=0,colorFieldPixels=0;bool colorQueued=false;
    std::function<void()> colorsPresented;
    uint64_t colorView(std::function<void()> refresh){colorViews.emplace(++nextColorView,std::move(refresh));return nextColorView;}
    void queueColors(){
        if(std::exchange(colorQueued,true))return;
        Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread().TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock()){
            self->colorQueued=false;auto views=self->colorViews;
            for(auto const& [id,refresh]:views)refresh();
            if(self->colorsPresented)self->colorsPresented();
        }});
    }
    static bool previewing(J const& preview){
        return object(preview,L"picker").GetNamedValue(L"preview",JsonValue::CreateNullValue()).ValueType()!=JsonValueType::Null;
    }
    mutable std::map<std::wstring,SolidColorBrush> paletteBrushes;
    mutable std::map<std::pair<std::wstring,uint8_t>,SolidColorBrush> tintBrushes;
    mutable std::map<std::wstring,SolidColorBrush> glassBrushes;
    mutable std::map<std::pair<std::wstring,std::wstring>,SolidColorBrush> lineBrushes;
    static Windows::UI::Color unitColor(A const& value){
        auto channel=[&](uint32_t i){return uint8_t(std::lround(std::clamp(value.GetNumberAt(i),0.,1.)*255));};
        return Windows::UI::Color{channel(3),channel(0),channel(1),channel(2)};
    }
    Windows::UI::Color paletteColor(wchar_t const* role)const{
        auto palette=object(state,L"palette");auto value=array(palette,role);
        return value.Size()==4?unitColor(value):color(str(palette,role,L"#414141"));
    }
    Windows::UI::Color glassColor(wchar_t const* role)const{
        auto value=array(object(object(state,L"palette"),L"glass"),role);
        if(value.Size()!=4)return color(str(object(state,L"palette"),role,L"#414141"));
        return unitColor(value);
    }
    Windows::UI::Color lineColor(std::wstring const& surface,std::wstring const& role)const{
        auto lines=object(object(state,L"palette"),L"lines");
        auto value=array(surface.empty()?lines:object(lines,surface.c_str()),role.c_str());
        return value.Size()==4?unitColor(value):Windows::UI::Color{};
    }
    void refreshPalette(){
        auto palette=object(state,L"palette");
        for(auto const& [role,brush]:paletteBrushes)brush.Color(paletteColor(role.c_str()));
        for(auto const& [role,brush]:glassBrushes)brush.Color(glassColor(role.c_str()));
        for(auto const& [key,brush]:lineBrushes)brush.Color(lineColor(key.first,key.second));
        for(auto const& [key,brush]:tintBrushes){auto value=color(str(palette,key.first.c_str(),L"#414141"));value.A=key.second;brush.Color(value);}
    }
    hstring caption(J const& request)const{
        auto source=to_string(request.Stringify());
        NativeString raw(capy_native_caption(localization.get(),source.c_str()),capy_string_free);
        if(!raw)throw hresult_error(E_OUTOFMEMORY);
        auto result=J::Parse(to_hstring(raw.get()));
        if(result.HasKey(L"error"))throw hresult_invalid_argument(str(result,L"error"));
        return str(result,L"text");
    }
    LocalizedCopy copyCaption(J const& request)const{
        auto resolve=[weak=weak_from_this(),request]{if(auto data=weak.lock())return data->caption(request);return hstring();};return {resolve(),resolve};
    }
    hstring caption(wchar_t const* group,wchar_t const* key)const{return str(object(object(catalog,L"native_copy"),group),key);}
    LocalizedCopy copyCaption(wchar_t const* group,wchar_t const* key)const{
        auto resolve=[weak=weak_from_this(),group=std::wstring(group),key=std::wstring(key)]{
            if(auto data=weak.lock())return str(object(object(data->catalog,L"native_copy"),group.c_str()),key.c_str());return hstring();
        };
        return {resolve(),resolve};
    }
    hstring common(wchar_t const* key)const{return str(object(object(catalog,L"bootstrap"),L"common"),key);}
    LocalizedCopy copyCommon(wchar_t const* key)const{
        auto resolve=[weak=weak_from_this(),key=std::wstring(key)]{
            if(auto data=weak.lock())return str(object(object(data->catalog,L"bootstrap"),L"common"),key.c_str());return hstring();
        };
        return {resolve(),resolve};
    }
    hstring delivery(wchar_t const* key)const{return str(object(catalog,L"delivery"),key);}
    LocalizedCopy copyDelivery(wchar_t const* key)const{
        auto resolve=[weak=weak_from_this(),key=std::wstring(key)]{if(auto data=weak.lock())return data->delivery(key.c_str());return hstring();};
        return {resolve(),resolve};
    }
    void dispatch(J const& action) const {send(to_string(action.Stringify()));}
    void dispatchDocument(J const& action,hstring const& epoch) const {
        dispatch(O({{L"windows_epoch",S(epoch)},{L"action",action}}));
    }
    hstring language()const{return str(object(catalog,L"bootstrap"),L"active_tag");}
    hstring theme()const{return str(state,L"theme",L"dark");}
    SolidColorBrush brush(wchar_t const* role)const{
        auto found=paletteBrushes.find(role);if(found!=paletteBrushes.end())return found->second;
        return paletteBrushes.emplace(role,fill(paletteColor(role))).first->second;
    }
    SolidColorBrush glass(wchar_t const* role)const{
        auto found=glassBrushes.find(role);if(found!=glassBrushes.end())return found->second;
        return glassBrushes.emplace(role,fill(glassColor(role))).first->second;
    }
    SolidColorBrush line(wchar_t const* surface,wchar_t const* role)const{
        auto key=std::make_pair(std::wstring(surface),std::wstring(role));
        if(auto found=lineBrushes.find(key);found!=lineBrushes.end())return found->second;
        return lineBrushes.emplace(key,fill(lineColor(key.first,key.second))).first->second;
    }
    SolidColorBrush line(wchar_t const* role)const{return line(L"",role);}
    bool transparent()const{return str(object(object(state,L"palette"),L"glass"),L"transparency",L"off")!=L"off";}
    SolidColorBrush tint(wchar_t const* role,uint8_t alpha)const{
        auto key=std::make_pair(std::wstring(role),alpha);
        if(auto found=tintBrushes.find(key);found!=tintBrushes.end())return found->second;
        auto value=color(str(object(state,L"palette"),role,L"#414141"));value.A=alpha;
        return tintBrushes.emplace(key,fill(value)).first->second;
    }
    double textSize()const{return num(catalog,L"text_size_pt",11)*96./72.;}
    double panelMetric(wchar_t const* name)const{return num(object(catalog,L"panel_metrics"),name);}
    double controlHeight()const{return std::max(panelMetric(L"control_height"),std::ceil(textSize()*Windows::UI::ViewManagement::UISettings().TextScaleFactor()*1.3));}
    CornerRadius controlCorner()const{auto r=panelMetric(L"control_radius")*CornerFit;return {r,r,r,r};}
};
inline void inheritLanguage(FrameworkElement const& element,std::shared_ptr<WorkspaceData> const& data) {
    auto language=data->language();if(!language.empty())element.Language(language);
    data->copyView([weak=make_weak(element),source=std::weak_ptr<WorkspaceData>(data)]{
        auto owner=weak.get();auto data=source.lock();if(!owner||!data)return false;owner.Language(data->language());return true;
    });
}
FrameworkElement panelIcon(std::shared_ptr<WorkspaceData> const& data,hstring const& name,Bindings& bindings,double size=16);
inline SolidColorBrush buttonBackground(std::shared_ptr<WorkspaceData> const& data){return data->tint(L"button",13);}
inline SolidColorBrush headerSurface(std::shared_ptr<WorkspaceData> const& data){return data->glass(L"chip");}
inline SolidColorBrush selected(std::shared_ptr<WorkspaceData> const& data){return data->glassSurfaces?data->glass(L"selection"):data->brush(L"selection");}
inline J displayColors(J const& state){
    auto masked=object(object(object(state,L"layer_tools"),L"mask_editing"),L"colors");
    return masked.Size()?masked:object(state,L"colors");
}
inline SolidColorBrush accent(std::shared_ptr<WorkspaceData> const& data){return data->brush(L"accent");}
inline void dialogSurface(std::shared_ptr<WorkspaceData> const& data,ContentDialog const& dialog){
    auto theme=data->theme()==L"dark"?ElementTheme::Dark:ElementTheme::Light;
    if(dialog.RequestedTheme()!=theme)dialog.RequestedTheme(theme);
    auto background=data->brush(L"settings");if(dialog.Background()!=background)dialog.Background(background);
    static thread_local auto overlay=box_value(L"ContentDialogTopOverlay");
    if(!dialog.Resources().HasKey(overlay))dialog.Resources().Insert(overlay,clear());
}
inline TextBlock label(std::shared_ptr<WorkspaceData> const& data,hstring const& text,bool bold=false,bool retained=true){
    TextBlock result;result.Text(text);result.FontSize(data->textSize());
    if(retained)inheritLanguage(result,data);else result.Language(data->language());result.FontFamily(FontFamily(L"Segoe UI"));result.Foreground(data->brush(L"text"));
    result.LineHeight(18);result.LineStackingStrategy(LineStackingStrategy::MaxHeight);
    if(bold)result.FontWeight(Windows::UI::Text::FontWeights::Bold());
    return result;
}
inline CheckBox compactCheck(std::shared_ptr<WorkspaceData> const& data,winrt::Windows::Foundation::IInspectable const& content){
    CheckBox check;check.Content(content);check.MinWidth(0);check.MinHeight(data->controlHeight());
    check.Padding({data->panelMetric(L"control_spacing"),0,0,0});check.VerticalContentAlignment(VerticalAlignment::Center);
    return check;
}
inline void twoLines(TextBlock const& text){
    text.TextWrapping(TextWrapping::Wrap);text.MaxLines(2);text.TextTrimming(TextTrimming::CharacterEllipsis);
}
inline TextBlock label(std::shared_ptr<WorkspaceData> const& data,LocalizedCopy const& text,bool bold=false){
    auto result=label(data,static_cast<hstring const&>(text),bold);
    data->copyView([weak=make_weak(result),resolve=text.current]{if(auto view=weak.get()){view.Text(resolve());return true;}return false;});
    return result;
}
inline void comboOptionText(ComboBoxItem const& option,hstring const& text){
    auto content=option.Tag().try_as<TextBlock>();
    if(!content){
        content=option.Content().try_as<TextBlock>();
        if(!content){content=TextBlock();content.TextTrimming(TextTrimming::CharacterEllipsis);option.Content(content);}
        option.Tag(content);
    }
    if(content.Text()!=text)content.Text(text);AutomationProperties::SetName(option,text);
}
inline ComboBoxItem comboOption(ComboBox const& control,hstring const& text,hstring const& id=L""){
    ComboBoxItem option;comboOptionText(option,text);if(!id.empty())AutomationProperties::SetAutomationId(option,id);
    control.Items().Append(option);return option;
}
inline void comboOptionText(ComboBox const& control,uint32_t index,hstring const& text){
    comboOptionText(control.Items().GetAt(index).as<ComboBoxItem>(),text);
}
template<typename T>
inline void buttonColors(std::shared_ptr<WorkspaceData> const& data,T const& result,uint8_t hover=20){
    hstring prefix=std::is_same_v<T,Primitives::ToggleButton>?L"ToggleButton":L"Button";
    result.Resources().Insert(box_value(prefix+L"BackgroundPointerOver"),data->tint(L"text",hover));
    result.Resources().Insert(box_value(prefix+L"BackgroundPressed"),data->tint(L"text",41));
    result.Resources().Insert(box_value(prefix+L"BackgroundDisabled"),clear());
    result.Resources().Insert(box_value(prefix+L"ForegroundDisabled"),data->tint(L"text",92));
    if constexpr(std::is_same_v<T,Primitives::ToggleButton>)
        for(auto role:{L"ToggleButtonBackgroundChecked",L"ToggleButtonBackgroundCheckedPointerOver",L"ToggleButtonBackgroundCheckedPressed"})
            result.Resources().Insert(box_value(role),selected(data));
}
inline void headerButtonColors(std::shared_ptr<WorkspaceData> const& data,Button const& item){
    item.UseLayoutRounding(false);
    item.Background(clear());item.BorderBrush(clear());item.BackgroundSizing(BackgroundSizing::InnerBorderEdge);
    for(auto role:{L"ButtonBorderBrush",L"ButtonBorderBrushPointerOver",L"ButtonBorderBrushPressed",L"ButtonBorderBrushDisabled"})
        item.Resources().Insert(box_value(role),clear());
    buttonColors(data,item,26);
}
template<typename T=Button>
inline T button(std::shared_ptr<WorkspaceData> const& data,hstring const& text,std::function<void()> action){
    T result;result.Content(box_value(text));result.FontSize(data->textSize());
    inheritLanguage(result,data);result.FontFamily(FontFamily(L"Segoe UI"));result.Foreground(data->brush(L"text"));
    result.FontWeight(Windows::UI::Text::FontWeights::Bold());
    result.MinWidth(0);result.MinHeight(0);result.Padding(Thickness{0});
    result.BorderThickness(Thickness{0});result.CornerRadius(data->controlCorner());
    result.Background(clear());AutomationProperties::SetName(result,text);
    buttonColors(data,result);
    result.Click([action=std::move(action)](auto&&,auto&&){action();});
    return result;
}
template<typename T=Button>
inline T button(std::shared_ptr<WorkspaceData> const& data,LocalizedCopy const& text,std::function<void()> action){
    auto result=button<T>(data,static_cast<hstring const&>(text),std::move(action));
    data->copyView([weak=make_weak(result),resolve=text.current,shown=static_cast<hstring const&>(text)]() mutable {
        auto view=weak.get();if(!view)return false;
        auto next=resolve();
        if(auto content=view.Content().try_as<Windows::Foundation::IReference<hstring>>();content&&content.Value()==shown)view.Content(box_value(next));
        AutomationProperties::SetName(view,next);shown=next;return true;
    });
    return result;
}
inline bool& touchContact(){thread_local bool touch=false;return touch;}
struct TooltipOwner {weak_ref<DependencyObject> owner;ToolTip tip{nullptr};};
inline std::unordered_map<void*,TooltipOwner>& tooltipOwners(){thread_local std::unordered_map<void*,TooltipOwner> owners;return owners;}
inline void setTouchContact(bool touch){
    if(touchContact()==touch)return;
    touchContact()=touch;
    std::erase_if(tooltipOwners(),[touch](auto const& entry){
        auto owner=entry.second.owner.get();if(!owner)return true;
        if(touch){entry.second.tip.IsOpen(false);ToolTipService::SetToolTip(owner,nullptr);}
        else ToolTipService::SetToolTip(owner,entry.second.tip);
        return false;
    });
}
inline void tooltip(DependencyObject const& target,hstring const& text){
    auto& owners=tooltipOwners();
    if(auto found=owners.find(get_abi(target));found!=owners.end()&&found->second.owner.get()==target){
        if(unbox_value_or<hstring>(found->second.tip.Content(),L"")!=text)found->second.tip.Content(box_value(text));
        if(!touchContact())ToolTipService::SetToolTip(target,found->second.tip);
        return;
    }
    if(owners.size()>=1024)std::erase_if(owners,[](auto const& entry){return !entry.second.owner.get();});
    ToolTip tip;tip.Content(box_value(text));
    if(!touchContact())ToolTipService::SetToolTip(target,tip);
    owners.insert_or_assign(get_abi(target),TooltipOwner{make_weak(target),tip});
}
inline void copyName(std::shared_ptr<WorkspaceData> const& data,FrameworkElement const& target,LocalizedCopy const& text,bool tip=false){
    auto present=[tip](FrameworkElement const& view,hstring const& value){AutomationProperties::SetName(view,value);if(tip)tooltip(view,value);};
    present(target,text);
    data->copyView([weak=make_weak(target),resolve=text.current,present]{if(auto view=weak.get()){present(view,resolve());return true;}return false;});
}
struct TooltipReveal {weak_ref<FrameworkElement> owner;ToolTip tip{nullptr};Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};};
inline TooltipReveal& tooltipReveal(){thread_local TooltipReveal reveal;return reveal;}
inline void hideRevealedTooltip(){
    auto& reveal=tooltipReveal();if(!reveal.tip)return;
    if(reveal.timer)reveal.timer.Stop();
    reveal.tip.IsOpen(false);
    if(auto owner=reveal.owner.get();owner&&touchContact())ToolTipService::SetToolTip(owner,nullptr);
    reveal.tip=nullptr;reveal.owner={};
}
inline void revealTooltip(FrameworkElement const& target){
    hideRevealedTooltip();
    DependencyObject key=target;
    auto found=tooltipOwners().find(get_abi(key));
    if(found==tooltipOwners().end()||found->second.owner.get()!=target)return;
    auto& reveal=tooltipReveal();reveal.owner=make_weak(target);reveal.tip=found->second.tip;
    ToolTipService::SetToolTip(target,reveal.tip);reveal.tip.IsOpen(true);
    if(!reveal.timer){
        reveal.timer=target.DispatcherQueue().CreateTimer();reveal.timer.IsRepeating(false);reveal.timer.Interval(std::chrono::milliseconds(4000));
        reveal.timer.Tick([](auto&&,auto&&){hideRevealedTooltip();});
    }
    reveal.timer.Start();
}
struct ChoiceGrid {
    Grid grid;std::vector<Button> cells;
    void update(std::shared_ptr<WorkspaceData> const& data,J const& spec)const{
        AutomationProperties::SetName(grid,str(spec,L"label"));tooltip(grid,str(spec,L"label"));
        auto items=array(spec,L"items");
        for(uint32_t i=0;i<std::min<uint32_t>(items.Size(),uint32_t(cells.size()));++i){
            auto item=items.GetObjectAt(i);bool chosen=flag(item,L"selected");
            cells[i].Background(chosen?selected(data):data->brush(L"control_fill"));cells[i].Content().as<UIElement>().Opacity(chosen?1.:.45);
            AutomationProperties::SetName(cells[i],str(item,L"label"));tooltip(cells[i],str(item,L"label"));
            AutomationProperties::SetItemStatus(cells[i],chosen?data->caption(L"search",L"selected"):L"");
        }
    }
};
inline ChoiceGrid choiceGrid(std::shared_ptr<WorkspaceData> const& data,J const& spec,hstring const& id,std::function<void(J)> const& send){
    ChoiceGrid result;auto items=array(spec,L"items");auto columns=std::max<uint32_t>(1,uint32_t(num(spec,L"columns")));
    for(uint32_t i=0;i<columns;++i){ColumnDefinition column;column.Width({18,GridUnitType::Pixel});result.grid.ColumnDefinitions().Append(column);}
    for(uint32_t i=0;i<(items.Size()+columns-1)/columns;++i){RowDefinition row;row.Height({18,GridUnitType::Pixel});result.grid.RowDefinitions().Append(row);}
    AutomationProperties::SetAutomationId(result.grid,id);
    for(uint32_t i=0;i<items.Size();++i){
        auto action=object(items.GetObjectAt(i),L"action");
        auto cell=button(data,L"",[send,action]{send(action);});cell.Width(18);cell.Height(18);
        cell.Content(icon(str(items.GetObjectAt(i),L"icon"),data->theme(),6));
        Grid::SetColumn(cell,int(i%columns));Grid::SetRow(cell,int(i/columns));AutomationProperties::SetAutomationId(cell,id+L"-"+to_hstring(i));
        result.grid.Children().Append(cell);result.cells.push_back(cell);
    }
    result.update(data,spec);return result;
}
inline Grid explainable(Control const& control){
    Grid host;host.Background(clear());host.Children().Append(control);
    control.HorizontalAlignment(HorizontalAlignment::Stretch);control.VerticalAlignment(VerticalAlignment::Stretch);
    host.Tapped([inner=make_weak(control)](auto&& sender,TappedRoutedEventArgs const&){
        if(auto control=inner.get();control&&!control.IsEnabled())revealTooltip(sender.template as<FrameworkElement>());
    });
    return host;
}
inline void explain(Grid const& host,Control const& control,bool enabled,hstring const& text,hstring const& reason){
    control.IsEnabled(enabled);control.IsHitTestVisible(enabled);
    tooltip(host,enabled||reason.empty()?text:reason);
    AutomationProperties::SetHelpText(control,enabled?L"":reason);
}
inline Control explained(FrameworkElement const& host){return host.as<Grid>().Children().GetAt(0).as<Control>();}
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
    bool preference=false;hstring description;std::function<hstring()> identity;std::function<void()> reset;
    std::vector<hstring> widthSamples;std::function<J(J const&,double,J const&)> resolve;std::function<hstring()> title,text,descriptionText;
    std::function<void(hstring const&,double)> phase;
    std::function<void(double)> position;
    bool compact=true;
};
StackPanel number(std::shared_ptr<WorkspaceData> const& data,hstring const& title,J const& spec,
    std::function<double()> get,std::function<void(double)> set,Bindings& bindings,Bindings* commits=nullptr,bool valueOnly=false,hstring const& identifier=L"",bool inlineTrack=false,NumberPresentation const& presentation={},NumericAdmissions* admissions=nullptr);
inline TextBox numberEntry(UIElement const& element){
    if(auto text=element.try_as<TextBox>())return text;
    if(auto border=element.try_as<Border>())return border.Child()?numberEntry(border.Child()):nullptr;
    if(auto panel=element.try_as<Panel>())for(auto const& child:panel.Children())if(auto text=numberEntry(child))return text;
    return nullptr;
}
}
