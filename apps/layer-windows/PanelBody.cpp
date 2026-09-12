#include "pch.h"
#include "PanelBody.h"
#include "ColorView.h"
#include "ToolView.h"
#include "EffectControls.h"
#include "LayersView.h"
#include "StatsView.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

using namespace CapyUi;
Grid PanelBody::sizes(double width){
        Grid grid;int columns=width<130?2:width<174?3:4;
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
PanelBody::PanelBody(std::shared_ptr<WorkspaceData> source,J const& panel,J const& geometry,
    std::function<void()> layoutChanged,std::shared_ptr<WorkspaceGestures> const& gestures,bool scrollable):data(std::move(source)){
        auto tileGeometry=object(geometry,L"tiles");
        if(str(panel,L"id")==L"navigator"){
            navigator=std::make_unique<NavigatorView>(data,std::move(layoutChanged));
            auto view=navigator->Root();root=view;
        }else if(str(panel,L"id")==L"adjustments"){
            auto view=FiltersPanel(data,bindings);root=view;
        }else if(str(panel,L"id")==L"layers"){
            auto view=LayersPanel(data,bindings);root=view;
        }else if(tileGeometry.Size()){
            Canvas tiles;auto views=array(panel,L"tiles");auto rects=array(tileGeometry,L"tiles");
            for(uint32_t i=0;i<std::min(views.Size(),rects.Size());i++){
                auto tile=views.GetObjectAt(i);double id=num(tile,L"id");auto panelId=str(panel,L"id");
                auto kind=str(object(tile,L"control"),L"kind");
                auto item=O({{L"kind",S(L"tile")},{L"panel",S(panelId)},{L"tile",N(id)}});
                auto attach=[&](FrameworkElement const& element){
                    tileElements.emplace(uint32_t(id),element);
                    if(gestures)gestures->Source(element,O({{L"type",S(L"tile_drag")},{L"item",item}}),item);
                };
                if(kind==L"divider"){
                    auto bounds=rects.GetObjectAt(i);Border slot;slot.Background(clear());place(slot,bounds);
                    Border line;line.Background(data->brush(L"settings_secondary"));line.Opacity(.3);
                    bool horizontal=num(bounds,L"width")>num(bounds,L"height");
                    line.Width(horizontal?num(bounds,L"width")*.7:1);
                    line.Height(horizontal?1:num(bounds,L"height")*.7);
                    line.HorizontalAlignment(HorizontalAlignment::Center);line.VerticalAlignment(VerticalAlignment::Center);
                    slot.Child(line);tiles.Children().Append(slot);attach(slot);
                    AutomationProperties::SetAutomationId(slot,L"tile-"+panelId+L"-"+to_hstring(uint32_t(id)));
                    continue;
                }
                auto pick=button(data,str(tile,L"label"),[data=data,id,panelId]{
                    data->dispatch(O({{L"type",S(L"activate_tile")},{L"panel",S(panelId)},{L"tile",N(id)}}));
                });
                attach(pick);
                pick.Content(icon(str(tile,L"icon",L"brush"),data->theme(),num(panel,L"tile_icon_size",16)));
                ToolTipService::SetToolTip(pick,box_value(str(tile,L"tooltip")));place(pick,rects.GetObjectAt(i));tiles.Children().Append(pick);
                AutomationProperties::SetAutomationId(pick,L"tile-"+panelId+L"-"+to_hstring(uint32_t(id)));
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
                if(num(panel,L"tile_label_lines")>0){
                    auto image=pick.Content();pick.Content(nullptr);
                    Grid content;ColumnDefinition mark;mark.Width({36,GridUnitType::Pixel});content.ColumnDefinitions().Append(mark);
                    ColumnDefinition caption;caption.Width({1,GridUnitType::Star});content.ColumnDefinitions().Append(caption);
                    ContentPresenter glyph;glyph.Content(image);glyph.HorizontalAlignment(HorizontalAlignment::Center);
                    glyph.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(glyph);
                    auto text=label(data,str(tile,L"label"),flag(panel,L"tile_label_bold"));text.TextWrapping(TextWrapping::Wrap);
                    text.MaxLines(int(num(panel,L"tile_label_lines")));text.TextTrimming(TextTrimming::CharacterEllipsis);text.Margin({0,0,4,0});
                    text.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(text,1);content.Children().Append(text);
                    pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);pick.Content(content);
                }
                bindings.emplace_back([data=data,pick,panelId,id]{
                    auto currentPanel=find(array(data->model,L"panels"),L"id",panelId);
                    auto current=findId(array(currentPanel,L"tiles"),id);
                    pick.IsEnabled(flag(current,L"enabled"));pick.Background(flag(current,L"selected")?selected():clear());
                    ToolTipService::SetToolTip(pick,box_value(str(current,L"tooltip")));
                });
            }
            auto grip=object(tileGeometry,L"grip");
            if(grip.Size()&&gestures){
                auto handle=button(data,L"Move "+str(panel,L"title"),[]{});
                place(handle,grip);
                Border mark;mark.Width(16);mark.Height(2);mark.Background(data->brush(L"settings_secondary"));mark.Opacity(.4);
                mark.HorizontalAlignment(HorizontalAlignment::Center);mark.VerticalAlignment(VerticalAlignment::Center);handle.Content(mark);
                auto item=O({{L"kind",S(L"panel")},{L"panel",S(str(panel,L"id"))}});
                gestures->Source(handle,O({{L"type",S(L"drag_workspace")},{L"item",item}}),
                    O({{L"kind",S(L"ribbon")},{L"panel",S(str(panel,L"id"))}}),true);
                AutomationProperties::SetAutomationId(handle,L"ribbon-grip-"+str(panel,L"id"));tiles.Children().Append(handle);
            }
            root=tiles;
        }else{
            StackPanel content;content.Spacing(12);double inset=str(panel,L"id")==L"stats"?6:8;
            content.Padding(Thickness{inset,inset,inset,inset});
            auto brushValue=[data=data](wchar_t const* key){return num(object(data->state,L"brush"),key);};
            for(auto value:array(panel,L"controls")){
                auto control=value.GetObject();if(!flag(control,L"visible_in_panel"))continue;
                auto kind=str(control,L"control");
                if(kind==L"brushes")content.Children().Append(ToolSetPanel(data,bindings));
                else if(kind==L"size_presets")content.Children().Append(sizes(num(object(geometry,L"bounds"),L"width")-16));
                else if(kind==L"tool_settings")content.Children().Append(ToolSettingsPanel(data,bindings));
                else if(kind==L"color_wheel")content.Children().Append(ColorPanel(data,bindings));
                else if(kind==L"properties")content.Children().Append(PropertiesPanel(data,bindings));
                else if(kind==L"stats")content.Children().Append(StatsPanel(data,bindings));
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
            if(!scrollable)root=content;
            else if(str(panel,L"id")==L"tool_settings"||str(panel,L"id")==L"properties"){
                ScrollView scroll;scroll.Content(content);scroll.HorizontalScrollMode(ScrollingScrollMode::Disabled);
                scroll.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
                scroll.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);
                root=scroll;
            }else{
                ScrollViewer scroll;scroll.Content(content);scroll.HorizontalScrollMode(ScrollMode::Disabled);
                scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);root=scroll;
            }
        }
    if(!scrollable){
        auto id=str(panel,L"id");
        if(id==L"layers"||id==L"adjustments")root.Height(480);
        else if(id==L"navigator")root.Height(272);
    }
}
void PanelBody::Apply(bool visible){
    if(navigator)navigator->Apply(visible);
    for(auto const& binding:bindings)binding();
}