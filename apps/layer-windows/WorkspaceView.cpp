#include "pch.h"
#include "WorkspaceView.h"
#include "ColorView.h"
#include "UiControls.h"
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
using namespace CapyUi;
struct WorkspaceView::Impl {
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    Canvas root;
    struct Group {Border border;std::wstring key;Bindings bindings;};
    std::map<uint32_t,Group> groups;
    hstring previousTheme,previousPalette;
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
                else if(kind==L"color_wheel")content.Children().Append(ColorPanel(data,bindings));
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
                content.Children().Append(ColorPanel(data,popupBindings));
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
        data->refreshPalette();
        auto theme=data->theme(),palette=object(data->state,L"palette").Stringify();
        if(theme!=previousTheme||palette!=previousPalette){
            root.Children().Clear();groups.clear();previousTheme=theme;previousPalette=palette;root.Children().Append(camera);
        }
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
