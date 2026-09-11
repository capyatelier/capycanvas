#include "pch.h"
#include "WorkspaceView.h"
#include "native/include/capy_windows.h"
#include <winrt/Microsoft.UI.Xaml.Automation.h>
#include <winrt/Microsoft.UI.Xaml.Media.Imaging.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Windows.UI.Text.h>
#include <algorithm>
#include <cmath>
#include <filesystem>
#include <map>
#include <vector>

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
namespace {
V S(hstring const& value){return JsonValue::CreateStringValue(value);}
V N(double value){return JsonValue::CreateNumberValue(value);}
V B(bool value){return JsonValue::CreateBooleanValue(value);}
J O(std::initializer_list<std::pair<wchar_t const*,V>> fields){
    J value;for(auto const& [key,item]:fields)value.Insert(key,item);return value;
}
J object(J const& value,wchar_t const* key){
    auto item=value.GetNamedValue(key,JsonValue::CreateNullValue());
    return item.ValueType()==JsonValueType::Object?item.GetObject():J{};
}
A array(J const& value,wchar_t const* key){
    auto item=value.GetNamedValue(key,JsonValue::CreateNullValue());
    return item.ValueType()==JsonValueType::Array?item.GetArray():A{};
}
hstring str(J const& value,wchar_t const* key,hstring fallback=L""){
    auto item=value.GetNamedValue(key,JsonValue::CreateNullValue());
    return item.ValueType()==JsonValueType::String?item.GetString():fallback;
}
double num(J const& value,wchar_t const* key,double fallback=0){
    return value.GetNamedNumber(key,fallback);
}
bool flag(J const& value,wchar_t const* key,bool fallback=false){return value.GetNamedBoolean(key,fallback);}
J find(A const& list,wchar_t const* key,hstring const& id){
    for(auto value:list){auto row=value.GetObject();if(str(row,key)==id)return row;}return J{};
}
J findId(A const& list,double id){
    for(auto value:list){auto row=value.GetObject();if(num(row,L"id")==id)return row;}return J{};
}
Windows::UI::Color color(hstring const& hex){
    auto text=to_string(hex);
    unsigned long rgb=text.size()==7?std::stoul(text.substr(1),nullptr,16):0;
    return {255,uint8_t(rgb>>16),uint8_t(rgb>>8),uint8_t(rgb)};
}
SolidColorBrush fill(Windows::UI::Color value){return SolidColorBrush(value);}
SolidColorBrush clear(){return fill({0,0,0,0});}
SolidColorBrush selected(){return fill({56,53,132,228});}
void place(FrameworkElement const& element,J const& rect){
    Canvas::SetLeft(element,num(rect,L"x"));Canvas::SetTop(element,num(rect,L"y"));
    element.Width(num(rect,L"width"));element.Height(num(rect,L"height"));
}
Windows::Foundation::Uri asset(std::wstring const& relative){
    wchar_t executable[32768];auto length=GetModuleFileNameW(nullptr,executable,32768);
    auto path=std::filesystem::path(std::wstring(executable,length)).parent_path()/L"Assets"/relative;
    return Windows::Foundation::Uri(L"file:///"+path.generic_wstring());
}
Image icon(hstring name,hstring theme,double size=16){
    std::wstring file=name.c_str();
    if(!file.starts_with(L"layer-"))file=L"layer-"+file;
    if(!file.ends_with(L"-symbolic"))file+=L"-symbolic";
    Image result;result.Width(size);result.Height(size);
    result.Source(Imaging::SvgImageSource(asset(L"icons/"+std::wstring(theme.c_str())+L"/"+file+L".svg")));
    result.IsHitTestVisible(false);return result;
}
J numeric(J const& spec,double value,J const& operation){
    auto json=to_string(O({{L"control",spec},{L"value",N(value)},{L"operation",operation}}).Stringify());
    std::unique_ptr<char,decltype(&capy_string_free)> result(capy_number(json.c_str()),capy_string_free);
    if(!result)throw hresult_invalid_argument(to_hstring(capy_error()));
    return J::Parse(to_hstring(result.get()));
}
using Bindings=std::vector<std::function<void()>>;
struct WorkspaceData {
    J state,catalog,model;
    WorkspaceView::Dispatch send;
    bool updating=false;
    void dispatch(J const& action) const {send(to_string(action.Stringify()));}
    hstring theme()const{return str(state,L"theme",L"dark");}
    SolidColorBrush brush(wchar_t const* role)const{return fill(color(str(object(state,L"palette"),role,L"#414141")));}
    double textSize()const{return num(catalog,L"text_size_pt",11)*96./72.;}
};
TextBlock label(std::shared_ptr<WorkspaceData> const& data,hstring const& text,bool bold=false){
    TextBlock result;result.Text(text);result.FontSize(data->textSize());
    result.FontFamily(FontFamily(L"Segoe UI"));result.Foreground(data->brush(L"text"));
    result.LineHeight(18);result.LineStackingStrategy(LineStackingStrategy::BlockLineHeight);
    if(bold)result.FontWeight(Windows::UI::Text::FontWeights::Bold());
    return result;
}
Button button(std::shared_ptr<WorkspaceData> const& data,hstring const& text,std::function<void()> action){
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
struct NumberState {double value=0;bool editing=false,dragging=false;};
StackPanel number(std::shared_ptr<WorkspaceData> const& data,hstring const& title,J const& spec,
    std::function<double()> get,std::function<void(double)> set,Bindings& bindings){
    auto local=std::make_shared<NumberState>();local->value=get();
    StackPanel root;root.Spacing(0);
    Grid header;ColumnDefinition left;left.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(left);
    ColumnDefinition right;right.Width({1,GridUnitType::Auto});header.ColumnDefinitions().Append(right);
    auto text=label(data,title);text.Margin(Thickness{6,0,6,0});text.VerticalAlignment(VerticalAlignment::Center);
    header.Children().Append(text);
    TextBox entry;entry.Width(72);entry.MinHeight(24);entry.Height(24);entry.Padding(Thickness{6,0,6,0});
    entry.FontSize(data->textSize());entry.Background(data->brush(L"input"));entry.BorderThickness(Thickness{0});
    entry.TextAlignment(TextAlignment::Right);Grid::SetColumn(entry,1);header.Children().Append(entry);
    AutomationProperties::SetName(entry,title);
    Slider slider;slider.Minimum(0);slider.Maximum(1);slider.StepFrequency(0.001);slider.MinHeight(0);slider.Height(24);
    slider.Resources().Insert(box_value(L"SliderHorizontalHeight"),box_value(24.));
    for(auto key:{L"SliderHorizontalThumbWidth",L"SliderHorizontalThumbHeight",L"SliderInnerThumbWidth",L"SliderInnerThumbHeight"})
        slider.Resources().Insert(box_value(key),box_value(0.));
    AutomationProperties::SetName(slider,title+L" slider");
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
    auto commit=[data,local,spec,set,weak=make_weak(entry)](bool cancel){
        auto entry=weak.get();if(!entry||!local->editing)return;
        try{
            auto next=numeric(spec,local->value,cancel?O({{L"type",S(L"format")}}):
                O({{L"type",S(L"expression")},{L"text",S(entry.Text())}}));
            local->value=num(next,L"value");local->editing=false;
            entry.Text(str(next,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));entry.BorderThickness(Thickness{0});
            if(!cancel)set(local->value);
        }catch(hresult_error const& error){
            entry.BorderThickness(Thickness{1,1,1,1});entry.BorderBrush(fill({255,221,85,85}));
            ToolTipService::SetToolTip(entry,box_value(error.message()));
        }
    };
    entry.GotFocus([data,local,spec](Windows::Foundation::IInspectable const& sender,RoutedEventArgs const&){
        auto entry=sender.as<TextBox>();local->editing=true;entry.Background(data->brush(L"input"));
        entry.Text(str(numeric(spec,local->value,O({{L"type",S(L"format")}})),L"edit"));
    });
    entry.LostFocus([commit,local,spec](Windows::Foundation::IInspectable const& sender,RoutedEventArgs const&){
        auto entry=sender.as<TextBox>();commit(false);entry.Background(clear());
        if(!local->editing)entry.Text(str(numeric(spec,local->value,O({{L"type",S(L"format")}})),L"text"));
    });
    entry.KeyDown([commit,local](auto&&,KeyRoutedEventArgs const& e){
        if(e.Key()==Windows::System::VirtualKey::Enter){local->editing=true;commit(false);e.Handled(true);}
        else if(e.Key()==Windows::System::VirtualKey::Escape){commit(true);e.Handled(true);}
        else local->editing=true;
    });
    slider.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler(
        [local](auto&&,auto&&){local->dragging=true;})),true);
    slider.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler(
        [local](auto&&,auto&&){local->dragging=false;})),true);
    slider.PointerCaptureLost([local](auto&&,auto&&){local->dragging=false;});
    slider.ValueChanged([data,local,spec,set,entry](auto&&,Primitives::RangeBaseValueChangedEventArgs const& e){
        if(data->updating)return;
        auto next=numeric(spec,local->value,O({{L"type",S(L"position")},{L"position",N(e.NewValue())}}));
        local->value=num(next,L"value");if(!local->editing)entry.Text(str(next,L"edit"));set(local->value);
    });
    bindings.emplace_back([data,local,spec,get,entry,slider]{
        if(local->editing||local->dragging)return;
        local->value=get();auto shown=numeric(spec,local->value,O({{L"type",S(L"format")}}));
        entry.Text(str(shown,entry.FocusState()==FocusState::Unfocused?L"text":L"edit"));
        entry.Background(entry.FocusState()==FocusState::Unfocused?clear():data->brush(L"input"));
        slider.Value(num(shown,L"fill"));
    });
    Grid trackRow;trackRow.ColumnSpacing(6);
    for(int i=0;i<3;i++){ColumnDefinition column;column.Width({i==1?1.:24.,i==1?GridUnitType::Star:GridUnitType::Pixel});trackRow.ColumnDefinitions().Append(column);}
    for(int direction:{-1,1}){
        auto step=button(data,(direction<0?L"Decrease ":L"Increase ")+title,[local,spec,set,commit,direction]{
            commit(false);if(local->editing)return;
            auto next=numeric(spec,local->value,O({{L"type",S(L"step")},{L"steps",N(direction)}}));
            local->value=num(next,L"value");set(local->value);
        });
        step.Width(24);step.Height(24);step.Content(icon(direction<0?L"minus":L"plus",data->theme()));
        Grid::SetColumn(step,direction<0?0:2);trackRow.Children().Append(step);
        bindings.emplace_back([local,spec,step,direction]{step.IsEnabled(direction<0?local->value>num(spec,L"min"):local->value<num(spec,L"max"));});
    }
    Grid::SetColumn(slider,1);trackRow.Children().Append(slider);
    root.Children().Append(header);root.Children().Append(trackRow);return root;
}
}

struct WorkspaceView::Impl {
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    Canvas root;
    struct Group {Border border;std::wstring key;Bindings bindings;};
    std::map<uint32_t,Group> groups;
    hstring previousTheme;
    std::map<std::wstring,FrameworkElement> anchors;
    Flyout popup{nullptr};
    std::wstring popupControl;
    std::shared_ptr<uint64_t> popupGeneration=std::make_shared<uint64_t>(0);
    Bindings popupBindings;
    TextBlock camera;
    Impl(Dispatch send,J catalog){
        data->send=std::move(send);data->catalog=catalog;
        AutomationProperties::SetName(root,L"Drawing workspace");
        camera.FontSize(num(catalog,L"text_size_pt",11)*96./72.);
        camera.IsHitTestVisible(false);
    }
    StackPanel brushes(Bindings& bindings){
        StackPanel list;list.Spacing(2);
        for(auto value:array(data->catalog,L"brush_categories")){
            auto category=value.GetObject();
            auto heading=label(data,str(category,L"label"),true);heading.Opacity(.55);
            heading.Margin(Thickness{8,8,8,8});list.Children().Append(heading);
            for(auto choiceValue:array(category,L"brushes")){
                auto choice=choiceValue.GetObject();double id=num(choice,L"id");
                auto pick=button(data,str(choice,L"label"),[data=data,id]{
                    data->dispatch(O({{L"type",S(L"select_brush")},{L"id",N(id)}}));
                });
                pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
                pick.Padding(Thickness{6,3,6,3});
                StackPanel content;
                Image preview;preview.Height(40);preview.Stretch(Stretch::Fill);
                preview.Source(Imaging::BitmapImage(asset(L"brush-previews/"+std::to_wstring(int(id))+L"-"+std::wstring(data->theme().c_str())+L".png")));
                content.Children().Append(preview);
                auto title=label(data,str(choice,L"label"),true);title.TextAlignment(TextAlignment::Right);
                content.Children().Append(title);pick.Content(content);list.Children().Append(pick);
                bindings.emplace_back([data=data,id,pick]{
                    pick.Background(num(object(data->state,L"brush"),L"preset")==id?selected():clear());
                });
            }
        }return list;
    }
    Grid sizes(double width,Bindings& bindings){
        Grid grid;int columns=std::max(1,int(width/44));
        for(int i=0;i<columns;i++){ColumnDefinition column;column.Width({1,GridUnitType::Star});grid.ColumnDefinitions().Append(column);}
        auto choices=array(data->catalog,L"brush_sizes");
        for(uint32_t i=0;i<choices.Size();i++){
            int row=int(i)/columns;if(i%columns==0){RowDefinition def;def.Height({1,GridUnitType::Auto});grid.RowDefinitions().Append(def);}
            double value=choices.GetNumberAt(i);
            auto pick=button(data,to_hstring(int(value))+L" px",[data=data,value]{
                data->dispatch(O({{L"type",S(L"set_brush_size")},{L"value",N(value)}}));
            });
            pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.Margin(Thickness{1,2,1,2});pick.Padding(Thickness{2,2,2,2});
            StackPanel content;content.Spacing(4);
            Grid dotBox;dotBox.Height(28);
            Microsoft::UI::Xaml::Shapes::Ellipse dot;double diameter=std::min(27.,2.+std::sqrt(value)*1.2);
            dot.Width(diameter);dot.Height(diameter);dot.Fill(data->brush(L"text"));
            dotBox.Children().Append(dot);content.Children().Append(dotBox);
            auto text=label(data,to_hstring(int(value)));text.TextAlignment(TextAlignment::Center);content.Children().Append(text);
            pick.Content(content);Grid::SetColumn(pick,int(i)%columns);Grid::SetRow(pick,row);grid.Children().Append(pick);
            bindings.emplace_back([data=data,pick,value]{
                pick.Background(num(object(data->state,L"brush"),L"diameter")==value?selected():clear());
            });
        }return grid;
    }
    StackPanel layers(Bindings& bindings){
        StackPanel rows;rows.Spacing(2);
        for(auto value:array(data->state,L"layers")){
            auto layer=value.GetObject();double id=num(layer,L"id");
            Grid row;
            ColumnDefinition eyeColumn;eyeColumn.Width({28,GridUnitType::Pixel});row.ColumnDefinitions().Append(eyeColumn);
            ColumnDefinition nameColumn;nameColumn.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(nameColumn);
            auto eye=button(data,L"Layer visibility",[data=data,id]{
                auto current=findId(array(data->state,L"layers"),id);
                data->dispatch(O({{L"type",S(L"set_layer_visibility")},{L"id",N(id)},{L"visible",B(!flag(current,L"visible"))}}));
            });eye.Width(28);eye.Height(40);row.Children().Append(eye);
            auto pick=button(data,str(layer,L"label"),[data=data,id]{
                data->dispatch(O({{L"type",S(L"select_layer")},{L"id",N(id)}}));
            });pick.Height(40);pick.FontWeight(Windows::UI::Text::FontWeights::Normal());pick.HorizontalAlignment(HorizontalAlignment::Stretch);
            pick.HorizontalContentAlignment(HorizontalAlignment::Left);pick.Padding(Thickness{6,0,6,0});
            pick.Margin(Thickness{num(layer,L"depth")*12,0,0,0});
            Grid::SetColumn(pick,1);row.Children().Append(pick);rows.Children().Append(row);
            bindings.emplace_back([data=data,id,pick,eye]{
                auto current=findId(array(data->state,L"layers"),id);
                pick.Background(flag(current,L"selected")?selected():clear());
                auto shown=flag(current,L"visible")?L"eye":L"eye-hidden";
                if(unbox_value_or<hstring>(eye.Tag(),L"")!=shown){
                    eye.Content(icon(shown,data->theme()));eye.Tag(box_value(shown));
                }
            });
        }return rows;
    }
    void build(Group& group,J const& geometry,J const& panel){
        auto& bindings=group.bindings;bindings.clear();
        group.border.Background(data->brush(L"panel"));group.border.CornerRadius(CornerRadius{8,8,8,8});
        Grid frame;
        RowDefinition tabRow;tabRow.Height({flag(geometry,L"tabs_visible")?36.:0.,GridUnitType::Pixel});
        frame.RowDefinitions().Append(tabRow);
        RowDefinition bodyRow;bodyRow.Height({1,GridUnitType::Star});frame.RowDefinitions().Append(bodyRow);
        if(flag(geometry,L"tabs_visible")){
            StackPanel tabs;tabs.Orientation(Orientation::Horizontal);tabs.Background(data->brush(L"tabbar"));
            for(auto value:array(geometry,L"panels")){
                auto id=value.GetString();auto model=find(array(data->model,L"panels"),L"id",id);
                auto tab=button(data,str(model,L"title"),[data=data,id,groupId=num(geometry,L"id")]{
                    data->dispatch(O({{L"type",S(L"select_panel_tab")},{L"group",N(groupId)},{L"panel",S(id)}}));
                });tab.Height(36);tab.Padding(Thickness{8,4,8,4});tab.CornerRadius(CornerRadius{6,6,0,0});
                StackPanel content;content.Orientation(Orientation::Horizontal);content.Spacing(6);
                auto tabStyle=object(model,L"tab");
                if(flag(tabStyle,L"show_icon"))content.Children().Append(icon(str(model,L"icon"),data->theme()));
                if(flag(tabStyle,L"show_name"))content.Children().Append(label(data,str(model,L"title"),true));
                tab.Content(content);if(id==str(geometry,L"active"))tab.Background(data->brush(L"panel"));
                tabs.Children().Append(tab);
            }
            ScrollViewer tabScroll;tabScroll.Content(tabs);tabScroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Hidden);
            tabScroll.HorizontalScrollMode(ScrollMode::Enabled);tabScroll.VerticalScrollMode(ScrollMode::Disabled);
            frame.Children().Append(tabScroll);
        }
        auto tileGeometry=object(geometry,L"tiles");
        if(tileGeometry.Size()){
            Canvas tiles;auto views=array(panel,L"tiles");auto rects=array(tileGeometry,L"tiles");
            for(uint32_t i=0;i<std::min(views.Size(),rects.Size());i++){
                auto tile=views.GetObjectAt(i);double id=num(tile,L"id");auto panelId=str(panel,L"id");
                auto pick=button(data,str(tile,L"label"),[data=data,id,panelId]{
                    data->dispatch(O({{L"type",S(L"activate_tile")},{L"panel",S(panelId)},{L"tile",N(id)}}));
                });
                pick.Content(icon(str(tile,L"icon",L"brush"),data->theme(),str(panel,L"tile_style")==L"large"?32:16));
                ToolTipService::SetToolTip(pick,box_value(str(tile,L"tooltip")));place(pick,rects.GetObjectAt(i));tiles.Children().Append(pick);
                auto kind=str(object(tile,L"control"),L"kind");
                if(kind==L"color"||kind==L"opacity")anchors.insert_or_assign(kind==L"color"?L"brush_color":L"brush_opacity",pick);
                if(kind==L"color"){
                    Microsoft::UI::Xaml::Shapes::Ellipse swatch;
                    swatch.Width(14);swatch.Height(14);swatch.Stroke(data->brush(L"text"));swatch.StrokeThickness(1.5);
                    pick.Content(swatch);
                    bindings.emplace_back([data=data,swatch]{
                        auto rgba=array(object(data->state,L"brush"),L"color");
                        if(rgba.Size()==4)swatch.Fill(fill({255,uint8_t(std::round(rgba.GetNumberAt(0)*255)),
                            uint8_t(std::round(rgba.GetNumberAt(1)*255)),uint8_t(std::round(rgba.GetNumberAt(2)*255))}));
                    });
                }
                bindings.emplace_back([data=data,pick,panelId,id]{
                    auto currentPanel=find(array(data->model,L"panels"),L"id",panelId);
                    auto current=findId(array(currentPanel,L"tiles"),id);
                    pick.IsEnabled(flag(current,L"enabled"));pick.Background(flag(current,L"selected")?selected():clear());
                });
            }
            Grid::SetRow(tiles,1);frame.Children().Append(tiles);
        }else{
            StackPanel content;content.Spacing(12);content.Padding(Thickness{8,8,8,8});
            auto brushValue=[data=data](wchar_t const* key){return num(object(data->state,L"brush"),key);};
            for(auto value:array(panel,L"controls")){
                auto control=value.GetObject();if(!flag(control,L"visible_in_panel"))continue;
                auto kind=str(control,L"control");
                if(kind==L"brushes")content.Children().Append(brushes(bindings));
                else if(kind==L"size_presets")content.Children().Append(sizes(num(object(geometry,L"bounds"),L"width")-16,bindings));
                else if(kind==L"layers")content.Children().Append(layers(bindings));
                else if(kind==L"brush_size"||kind==L"brush_opacity"){
                    bool size=kind==L"brush_size";
                    content.Children().Append(number(data,size?L"Brush size":L"Brush opacity",
                        object(data->catalog,size?L"brush_size":L"opacity"),
                        [brushValue,size]{return brushValue(size?L"diameter":L"opacity");},
                        [data=data,size](double value){data->dispatch(O({{L"type",S(size?L"set_brush_size":L"set_brush_opacity")},{L"value",N(value)}}));},bindings));
                }else if(kind==L"layer_opacity"){
                    content.Children().Append(number(data,L"Opacity",object(data->catalog,L"layer_opacity"),
                        [data=data]{for(auto value:array(data->state,L"layers")){auto layer=value.GetObject();if(flag(layer,L"selected"))return num(layer,L"opacity");}return 1.;},
                        [data=data](double value){data->dispatch(O({{L"type",S(L"set_layer_opacity")},{L"opacity",N(value)}}));},bindings));
                }else if(kind==L"layer_actions"){
                    StackPanel actions;actions.Orientation(Orientation::Horizontal);actions.Spacing(4);
                    for(auto commandValue:array(data->catalog,L"layer_commands")){
                        auto id=commandValue.GetString();auto command=find(array(data->state,L"commands"),L"id",id);
                        auto pick=button(data,str(command,L"label"),[data=data,id]{data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(id)}}));});
                        pick.Width(28);pick.Height(28);pick.Content(icon(str(command,L"icon"),data->theme()));actions.Children().Append(pick);
                        bindings.emplace_back([data=data,id,pick]{pick.IsEnabled(flag(find(array(data->state,L"commands"),L"id",id),L"enabled"));});
                    }content.Children().Append(actions);
                }
            }
            ScrollViewer scroll;scroll.Content(content);scroll.HorizontalScrollMode(ScrollMode::Disabled);
            scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);Grid::SetRow(scroll,1);frame.Children().Append(scroll);
        }
        group.border.Child(frame);
    }
    void updatePopup(){
        auto control=str(object(data->state,L"customization"),L"control");
        std::wstring next=control.c_str();
        if(next!=popupControl){
            ++*popupGeneration;
            if(popup)popup.Hide();
            popup=nullptr;popupBindings.clear();popupControl=next;
            auto anchor=anchors.find(next);
            if(anchor==anchors.end())return;
            StackPanel content;content.Width(280);content.Spacing(12);
            content.Children().Append(label(data,next==L"brush_color"?L"Brush color":L"Brush opacity",true));
            if(next==L"brush_opacity"){
                content.Children().Append(number(data,L"Opacity",object(data->catalog,L"opacity"),
                    [data=data]{return num(object(data->state,L"brush"),L"opacity");},
                    [data=data](double value){data->dispatch(O({{L"type",S(L"set_brush_opacity")},{L"value",N(value)}}));},popupBindings));
            }else if(next==L"brush_color"){
                ColorPicker picker;picker.IsAlphaEnabled(true);picker.IsAlphaSliderVisible(true);picker.IsHexInputVisible(true);
                picker.ColorChanged([data=data](ColorPicker const&,ColorChangedEventArgs const& e){
                    if(data->updating)return;
                    auto color=e.NewColor();A rgba;
                    for(auto channel:{color.R,color.G,color.B,color.A})rgba.Append(N(double(channel)/255));
                    data->dispatch(O({{L"type",S(L"set_color")},{L"rgba",rgba}}));
                });
                popupBindings.emplace_back([data=data,picker]{
                    auto rgba=array(object(data->state,L"brush"),L"color");if(rgba.Size()!=4)return;
                    Windows::UI::Color nextColor{uint8_t(std::round(rgba.GetNumberAt(3)*255)),
                        uint8_t(std::round(rgba.GetNumberAt(0)*255)),uint8_t(std::round(rgba.GetNumberAt(1)*255)),
                        uint8_t(std::round(rgba.GetNumberAt(2)*255))};
                    if(picker.Color()!=nextColor)picker.Color(nextColor);
                });content.Children().Append(picker);
            }
            popup=Flyout();popup.Content(content);
            popup.Closed([data=data,generation=popupGeneration,current=*popupGeneration](auto&&,auto&&){
                if(*generation==current)data->dispatch(O({{L"type",S(L"customize")},
                    {L"action",O({{L"type",S(L"close_control")}})}}));
            });
            for(auto const& bind:popupBindings)bind();
            popup.ShowAt(anchor->second);
        }else for(auto const& bind:popupBindings)bind();
    }
    void apply(J const& snapshot){
        if(!snapshot.HasKey(L"state")){
            auto cameraPatch=object(snapshot,L"camera");
            data->state.Insert(L"camera",cameraPatch);updateCamera(cameraPatch);return;
        }
        data->updating=true;
        struct Reset {bool& value;~Reset(){value=false;}} reset{data->updating};
        data->model=snapshot;data->state=object(snapshot,L"state");
        auto theme=data->theme();
        if(theme!=previousTheme){root.Children().Clear();groups.clear();previousTheme=theme;root.Children().Append(camera);}
        root.RequestedTheme(theme==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        auto layout=object(snapshot,L"layout");
        std::vector<uint32_t> visible;
        for(auto value:array(layout,L"groups")){
            auto geometry=value.GetObject();uint32_t id=uint32_t(num(geometry,L"id"));visible.push_back(id);
            auto panel=find(array(snapshot,L"panels"),L"id",str(geometry,L"active"));
            auto [it,added]=groups.try_emplace(id);auto& group=it->second;
            if(added)root.Children().Append(group.border);
            // Geometry and structure may rebuild this group. Value-only updates
            // below keep its native focus, slider capture and scroll position.
            J signature=O({{L"geometry",geometry},{L"controls",array(panel,L"controls")},
                {L"style",S(str(panel,L"tile_style"))}});
            A tileKeys;for(auto item:array(panel,L"tiles")){
                auto tile=item.GetObject();tileKeys.Append(O({{L"id",N(num(tile,L"id"))},{L"control",object(tile,L"control")}}));
            }signature.Insert(L"tiles",tileKeys);
            if(str(panel,L"id")==L"layers"){
                A keys;for(auto item:array(data->state,L"layers")){auto layer=item.GetObject();
                    keys.Append(O({{L"id",N(num(layer,L"id"))},{L"label",S(str(layer,L"label"))},{L"depth",N(num(layer,L"depth"))}}));}
                signature.Insert(L"layers",keys);
            }
            std::wstring key=signature.Stringify().c_str();
            if(group.key!=key){group.key=std::move(key);build(group,geometry,panel);}
            place(group.border,object(geometry,L"bounds"));
            bool hidden=flag(snapshot,L"chrome_hidden")&&(!flag(geometry,L"floating")||flag(snapshot,L"hide_floating_panels"));
            group.border.Visibility(hidden?Visibility::Collapsed:Visibility::Visible);
            for(auto const& bind:group.bindings)bind();
        }
        for(auto it=groups.begin();it!=groups.end();){
            if(std::find(visible.begin(),visible.end(),it->first)==visible.end()){
                uint32_t index;if(root.Children().IndexOf(it->second.border,index))root.Children().RemoveAt(index);
                it=groups.erase(it);
            }else ++it;
        }
        camera.Foreground(data->brush(L"text"));place(camera,object(layout,L"status"));
        camera.TextAlignment(TextAlignment::Right);updateCamera(object(data->state,L"camera"));
        updatePopup();
    }
    void updateCamera(J const& view){
        if(view.Size())camera.Text(to_hstring(int(std::round(num(view,L"zoom",1)*100)))+L"% · "+
            to_hstring(int(std::round(num(view,L"rotation")*180/3.141592653589793)))+L"°");
    }
};
WorkspaceView::WorkspaceView(Dispatch send,Json catalog):impl(std::make_unique<Impl>(std::move(send),catalog)){}
WorkspaceView::~WorkspaceView()=default;
Canvas WorkspaceView::Root()const{return impl->root;}
void WorkspaceView::Apply(Json const& snapshot){impl->apply(snapshot);}
