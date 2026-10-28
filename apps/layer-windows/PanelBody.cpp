#include "pch.h"
#include "PanelBody.h"
#include "WorkspaceGeometry.h"
#include "ColorView.h"
#include "ToolView.h"
#include "EffectControls.h"
#include "LayersView.h"
#include "PalettesView.h"
#include "StatsView.h"
#include "ScopesView.h"
#include "ProofPanel.h"
#include "WorkspaceQuery.h"
#include "ColorPair.h"
#include "NativeMenus.h"
#include "PanelScroll.h"

using namespace CapyUi;
namespace {
bool shows(J const& panel,wchar_t const* control){return flag(find(array(panel,L"controls"),L"control",control),L"visible_in_panel");}
}
FrameworkElement CapyUi::panelIcon(std::shared_ptr<WorkspaceData> const& data,hstring const& name,Bindings& bindings,double size){
    if(name!=L"colors")return icon(name,data->theme(),size);
    auto pair=std::make_shared<ColorPair>(data);
    auto update=[data=data,pair,size]{pair->Update(data,size);};update();
    bindings.emplace_back(std::move(update));return pair->root;
}
PanelBody::PanelBody(std::shared_ptr<WorkspaceData> source,J const& panel,J const& geometry,
    std::function<void()> layoutChanged,std::shared_ptr<WorkspaceGestures> const& gestures,bool scrollable,std::function<J()> tools):data(std::move(source)){
        auto tileGeometry=object(geometry,L"tiles");
        if(str(panel,L"id")==L"navigator"&&shows(panel,L"navigator")){
            navigator=std::make_unique<NavigatorView>(data,std::move(layoutChanged));
            auto view=navigator->Root();root=view;
        }else if(str(panel,L"id")==L"adjustments"&&shows(panel,L"adjustments")){
            auto view=FiltersPanel(data,bindings,&contentHeight,&scrollMetrics,flag(geometry,L"split_filters"));root=view;
        }else if(str(panel,L"id")==L"filter_types"&&shows(panel,L"filter_types")){
            root=FilterTypesPanel(data,bindings);
        }else if(str(panel,L"id")==L"layers"){
            auto view=LayersPanel(data,bindings,&contentHeight,&scrollMetrics,&layers);root=view;
        }else if(str(panel,L"id")==L"palettes"){
            root=PalettesPanel(data,bindings,&contentHeight,&scrollMetrics);
        }else if(tileGeometry.Size()){
            Canvas tiles;tiles.Background(clear());auto views=array(panel,L"tiles");auto rects=array(tileGeometry,L"tiles");
            bool vertical=str(geometry,L"axis",L"vertical")==L"vertical";
            for(uint32_t i=0;i<std::min(views.Size(),rects.Size());i++){
                auto tile=views.GetObjectAt(i);double id=num(tile,L"id");auto panelId=str(panel,L"id");
                auto kind=str(object(tile,L"control"),L"kind");
                auto item=O({{L"kind",S(L"tile")},{L"panel",S(panelId)},{L"tile",N(id)}});
                auto attach=[&](FrameworkElement const& element){
                    tileElements.emplace(uint32_t(id),element);tileOrder.push_back(uint32_t(id));
                    if(gestures)gestures->Source(element,O({{L"type",S(L"tile_drag")},{L"item",item}}),item,false,{},WorkspaceGestures::Pickup::Hold);
                    edgeTag(element,L"edge_contact",S(flag(tile,L"tears_bubble")?L"input":L"free"));
                };
                if(object(tile,L"component").Size()){
                    auto component=std::make_shared<ToolbarComponent>(data,panel,tile,tileGeometry,gestures);
                    auto element=component->Root();auto bounds=rects.GetObjectAt(i);
                    tileElements.emplace(uint32_t(id),element);tileOrder.push_back(uint32_t(id));components.emplace(uint32_t(id),component);
                    place(element,bounds);component->Layout(bounds,vertical);tiles.Children().Append(element);
                    bindings.emplace_back([data=data,component,panelId,id]{
                        auto current=findId(array(find(array(data->model,L"panels"),L"id",panelId),L"tiles"),id);
                        if(object(current,L"component").Size())component->Update(current);
                    });
                    continue;
                }
                if(kind==L"divider"){
                    auto bounds=rects.GetObjectAt(i);auto slot=button(data,str(tile,L"label"),[]{});place(slot,bounds);
                    auto line=dividerLine(data,num(bounds,L"width")>num(bounds,L"height"));
                    slot.Content(line);dividers.emplace(uint32_t(id),line);tiles.Children().Append(slot);attach(slot);
                    tileControls.emplace(uint32_t(id),slot);
                    AutomationProperties::SetAutomationId(slot,L"tile-"+panelId+L"-"+to_hstring(uint32_t(id)));
                    continue;
                }
                bool picker=pickerControl(object(tile,L"control"));
                auto presses=std::make_shared<DoublePress>();
                auto pick=button(data,str(tile,L"label"),[data=data,id,panelId,presses,weak=std::weak_ptr<WorkspaceGestures>(gestures)]{
                    if(auto gestures=weak.lock();gestures&&gestures->SuppressClick()){presses->reset();return;}
                    if(presses->second()){
                        auto current=findId(array(find(array(data->model,L"panels"),L"id",panelId),L"tiles"),id);
                        auto control=object(current,L"resolved_control");
                        if(flag(current,L"double_click")){
                            data->dispatch(O({{L"type",S(L"double_click_tool")},{L"control",control}}));return;
                        }
                        if(pickerControl(control)){
                            data->dispatch(O({{L"type",S(L"color_picker")},{L"action",O({{L"kind",S(L"settings")},
                                {L"anchor",O({{L"kind",S(L"tile")},{L"panel",S(panelId)},{L"tile",N(id)}})}})}}));return;
                        }
                    }
                    data->dispatch(O({{L"type",S(L"activate_tile")},{L"panel",S(panelId)},{L"tile",N(id)}}));
                });
                presses->listen(pick);
                // A disabled command remains disabled and accessible as such;
                // its surrounding tile still accepts customization gestures.
                Border slot;slot.Background(clear());auto tileContent=explainable(pick);
                auto variants=ToolGroupMarker(data);
                tileContent.Children().Append(variants);slot.Child(tileContent);attach(slot);
                tileControls.emplace(uint32_t(id),pick);
                double radius=num(tileGeometry,L"tile_corner_radius",SurfaceRadius);
                {double r=radius*CornerFit;pick.CornerRadius({r,r,r,r});}
                AutomationProperties::SetAutomationId(slot,L"tile-hit-"+panelId+L"-"+to_hstring(uint32_t(id)));
                auto tileIcon=icon(str(tile,L"icon",L"brush"),data->theme(),num(tileGeometry,L"tile_icon_size",16));
                auto tileIconName=std::make_shared<hstring>(str(tile,L"icon",L"brush"));pick.Content(tileIcon);
                tooltip(tileContent,str(tile,L"tooltip"));place(slot,rects.GetObjectAt(i));tiles.Children().Append(slot);
                AutomationProperties::SetAutomationId(pick,L"tile-"+panelId+L"-"+to_hstring(uint32_t(id)));
                if(kind==L"color"||kind==L"opacity")anchors.insert_or_assign(kind==L"color"?L"brush_color":L"brush_opacity",pick);
                if(*tileIconName==L"colors")pick.Content(panelIcon(data,*tileIconName,bindings,num(tileGeometry,L"tile_icon_size",16)));
                if(num(tileGeometry,L"tile_label_lines")>0){
                    auto image=pick.Content();pick.Content(nullptr);
                    Grid content;ColumnDefinition mark;mark.Width({36,GridUnitType::Pixel});content.ColumnDefinitions().Append(mark);
                    ColumnDefinition caption;caption.Width({1,GridUnitType::Star});content.ColumnDefinitions().Append(caption);
                    ContentPresenter glyph;glyph.Content(image);glyph.HorizontalAlignment(HorizontalAlignment::Center);
                    glyph.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(glyph);
                    auto text=label(data,str(tile,L"label"),flag(tileGeometry,L"tile_label_bold"));text.TextWrapping(TextWrapping::Wrap);
                    text.MaxLines(int(num(tileGeometry,L"tile_label_lines")));text.TextTrimming(TextTrimming::CharacterEllipsis);text.Margin({0,0,4,0});
                    text.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(text,1);content.Children().Append(text);
                    bindings.emplace_back([data=data,text,panelId,id]{auto current=findId(array(find(array(data->model,L"panels"),L"id",panelId),L"tiles"),id);text.Text(str(current,L"label"));text.Margin({0,0,flag(current,L"has_variants")?16.:4.,0});});
                    pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);pick.Content(content);
                }
                bindings.emplace_back([data=data,pick,slot,tileContent,variants,panelId,id,picker,tileIcon,tileIconName,radius]{
                    auto currentPanel=find(array(data->model,L"panels"),L"id",panelId);
                    auto current=findId(array(currentPanel,L"tiles"),id);
                    auto role=flag(current,L"tears_bubble")?L"input":L"free";
                    if(str(slot.Tag().as<J>(),L"edge_contact")!=role)edgeTag(slot,L"edge_contact",S(role));
                    if(auto name=str(current,L"icon",L"brush");name!=*tileIconName){
                        *tileIconName=name;tileIcon.Source(icon(name,data->theme()).Source());
                    }
                    variants.Visibility(flag(current,L"has_variants")?Visibility::Visible:Visibility::Collapsed);
                    AutomationProperties::SetName(pick,str(current,L"label"));
                    bool enabled=flag(current,L"enabled");auto hint=str(current,L"tooltip");
                    explain(tileContent,pick,enabled,picker?pickerTooltip(hint):hint,str(current,L"disabled_reason"));pick.Opacity(enabled?1.:.36);
                    auto facing=drawerFacing(data,O({{L"kind",S(L"tile")},{L"panel",S(panelId)},{L"tile",N(id)}}));
                    pick.Background(flag(current,L"selected")?selected(data):!facing.empty()?data->glass(L"open_tile"):clear());
                    buttonFeedback(pick,data->brush(L"text"),20,41,nullptr,flag(current,L"selected"));
                    pick.CornerRadius(facingCorners(radius*CornerFit,facing));
                });
            }
            for(uint32_t i=0;i<tileOrder.size()&&i<rects.Size();++i){
                auto box=rects.GetObjectAt(i);
                if(num(box,L"width")<=0||num(box,L"height")<=0)tileElements.at(tileOrder[i]).Visibility(Visibility::Collapsed);
            }
            auto grip=object(tileGeometry,L"grip");
            if(grip.Size()&&gestures){
                auto caption=[source=std::weak_ptr<WorkspaceData>(data),id=str(panel,L"id")]{
                    auto current=source.lock();if(!current)return hstring();
                    auto currentPanel=find(array(current->model,L"panels"),L"id",id);
                    return current->caption(O({{L"type",S(L"move_panel")},{L"title",S(str(currentPanel,L"title"))}}));
                };
                auto handle=button(data,LocalizedCopy(caption(),caption),[]{});
                bindings.emplace_back([handle,caption]{AutomationProperties::SetName(handle,caption());});
                place(handle,grip);tileGrip=handle;
                tileGripMark=panelGrip(data->theme(),num(grip,L"width")>num(grip,L"height"));handle.Content(tileGripMark);
                auto item=O({{L"kind",S(L"panel")},{L"panel",S(str(panel,L"id"))}});
                gestures->Source(handle,O({{L"type",S(L"drag_workspace")},{L"item",item}}),
                    O({{L"kind",S(L"ribbon")},{L"panel",S(str(panel,L"id"))}}),true);
                AutomationProperties::SetAutomationId(handle,L"ribbon-grip-"+str(panel,L"id"));tiles.Children().Append(handle);
            }
            root=tiles;
        }else{
            StackPanel content;content.Background(clear());content.Spacing(data->panelMetric(L"group_spacing"));auto panelId=str(panel,L"id");
            bool scope=panelId==L"histogram"||panelId==L"waveform";
            double inset=data->panelMetric(L"inset");
            contentHeight=[content](double width){content.Measure({float(width),INFINITY});return content.DesiredSize().Height;};
            content.Padding(Thickness{inset,inset,inset,inset});
            if(panelId==L"proof"){content.VerticalAlignment(VerticalAlignment::Top);content.Children().Append(ProofPanel(data,bindings));}
            FrameworkElement fittedControl{nullptr};std::function<double(double)> fittedHeight;size_t visibleControls=0;
            for(auto value:array(panel,L"controls"))if(flag(value.GetObject(),L"visible_in_panel"))visibleControls++;
            auto brushValue=[data=data](wchar_t const* key){return num(object(data->state,L"brush"),key);};
            for(auto value:array(panel,L"controls")){
                auto control=value.GetObject();if(!flag(control,L"visible_in_panel"))continue;
                auto kind=str(control,L"control");
                if(kind==L"brushes"||kind==L"brush_sets"||kind==L"sculpt_sets"||kind==L"tools")content.Children().Append(ToolSetPanel(data,bindings,kind,tools));
                else if(kind==L"size_presets")content.Children().Append(BrushSizePanel(data,bindings));
                else if(kind==L"tool_settings")content.Children().Append(ToolSettingsPanel(data,bindings));
                else if(kind==L"color_wheel"){
                    auto picker=ColorPanel(data,bindings,visibleControls==1);
                    if(visibleControls==1){fittedControl=picker;fittedHeight=[data=data,picker](double width){auto root=picker.XamlRoot();return ColorPanelNaturalHeight(data,width,root?root.RasterizationScale():1.);};}
                    else content.Children().Append(picker);
                }
                else if(kind==L"properties")content.Children().Append(PropertiesPanel(data,bindings));
                else if(kind==L"stats")content.Children().Append(StatsPanel(data,bindings));
                else if(kind==L"histogram"||kind==L"waveform"){
                    auto view=CapyScopes::ScopePanel(data,bindings,kind==L"waveform",visibleControls==1?&fittedHeight:nullptr);
                    if(visibleControls==1)fittedControl=view;else content.Children().Append(view);
                }
                else if(kind==L"brush_size"||kind==L"brush_opacity"){
                    bool size=kind==L"brush_size";
                    NumberPresentation presentation;presentation.reset=[data=data,size]{data->dispatch(O({{L"type",S(L"reset_tool_setting")},{L"id",S(size?L"size":L"opacity")}}));};
                    content.Children().Append(number(data,size?L"Brush size":L"Brush opacity",
                        object(data->catalog,size?L"brush_size":L"opacity"),
                        [brushValue,size]{return brushValue(size?L"diameter":L"opacity");},
                        [data=data,size](double value){data->dispatch(O({{L"type",S(size?L"set_brush_size":L"set_brush_opacity")},{L"value",N(value)}}));},bindings,nullptr,false,L"",false,presentation));
                }else if(kind==L"brush_color"){
                    content.Children().Append(ColorPanel(data,bindings));
                }else if(kind==L"layer_opacity"){
                    content.Children().Append(number(data,L"Opacity",object(data->catalog,L"layer_opacity"),
                        [data=data]{for(auto value:array(data->state,L"layers")){auto layer=value.GetObject();if(flag(layer,L"selected"))return num(layer,L"opacity");}return 1.;},
                        [data=data](double value){data->dispatch(O({{L"type",S(L"set_layer_opacity")},{L"opacity",N(value)}}));},bindings));
                }else if(kind==L"layer_actions"){
                    StackPanel actions;actions.Orientation(Orientation::Horizontal);actions.Spacing(4);
                    for(auto commandValue:array(data->catalog,L"layer_commands")){
                        auto id=commandValue.GetString();auto command=find(array(data->state,L"commands"),L"id",id);
                        auto pick=button(data,str(command,L"label"),[data=data,id]{data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(id)}}));});
                        pick.Width(data->panelMetric(L"control_height"));pick.Height(data->panelMetric(L"control_height"));pick.Padding({0});pick.Content(icon(str(command,L"icon"),data->theme()));actions.Children().Append(pick);
                        bindings.emplace_back([data=data,id,pick]{pick.IsEnabled(flag(find(array(data->state,L"commands"),L"id",id),L"enabled"));});
                    }content.Children().Append(actions);
                }
            }
            if(fittedControl){
                Grid fitted;fitted.Padding({inset,inset,inset,inset});fitted.Children().Append(fittedControl);root=fitted;
                contentHeight=[fittedHeight,inset](double width){return fittedHeight(std::max(0.,width-2*inset))+2*inset;};
            }else if(!scrollable)root=content;
            else if(scope||str(panel,L"id")==L"proof"){StackPanel natural;natural.Children().Append(content);root=natural;}
            else{
                scrollMetrics=[] {return O({{L"fixed_height",N(0)},{L"unit_height",N(0)}});};
                ScrollView scroll;scroll.Content(content);scroll.HorizontalScrollMode(ScrollingScrollMode::Disabled);
                scroll.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
                scroll.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);mouseScrollbars(scroll);scroll.ContentOrientation(ScrollingContentOrientation::Vertical);panelScrolling(scroll,content,data);
                scroll.ViewChanged([changed=std::move(layoutChanged)](auto&&,auto&&){if(changed)changed();});
                AutomationProperties::SetAutomationId(scroll,L"panel-scroll-"+str(panel,L"id"));
                root=scroll;
            }
        }
    if(gestures)gestures->Source(root,{},O({{L"kind",S(tileGeometry.Size()?L"ribbon":L"panel")},{L"panel",S(str(panel,L"id"))}}));
    if(navigator)scrollMetrics=[this]{return O({{L"fixed_height",N(navigator->FixedHeight())},{L"unit_height",N(0)}});};
    if(!scrollMetrics&&contentHeight&&str(panel,L"id")!=L"color"&&str(panel,L"id")!=L"proof"
        &&str(panel,L"id")!=L"histogram"&&str(panel,L"id")!=L"waveform")
        scrollMetrics=[]{return O({{L"fixed_height",N(0)},{L"unit_height",N(0)}});};
    if(!scrollable){
        auto id=str(panel,L"id");
        if((id==L"filter_types"&&shows(panel,L"filter_types"))||(id==L"adjustments"&&flag(geometry,L"split_filters")))root.Height(440);
        else if((id==L"layers"&&shows(panel,L"layers"))||(id==L"adjustments"&&shows(panel,L"adjustments")))root.Height(480);
        else if(navigator)root.Height(navigator->NaturalHeight(num(object(geometry,L"bounds"),L"width",272)));
    }
}
void PanelBody::Layout(J const& geometry){
    auto layout=object(geometry,L"tiles");auto bounds=array(layout,L"tiles");
    bool vertical=str(geometry,L"axis",L"vertical")==L"vertical";
    for(uint32_t i=0;i<tileOrder.size();++i){
        auto element=tileElements.at(tileOrder[i]);
        auto box=i<bounds.Size()?bounds.GetObjectAt(i):J{};
        bool shown=num(box,L"width")>0&&num(box,L"height")>0;
        element.Visibility(shown?Visibility::Visible:Visibility::Collapsed);
        if(!shown)continue;
        place(element,box);
        if(auto found=components.find(tileOrder[i]);found!=components.end())found->second->Layout(box,vertical);
        if(auto found=dividers.find(tileOrder[i]);found!=dividers.end())orientDivider(found->second,num(box,L"width")>num(box,L"height"));
    }
    if(tileGrip){
        auto grip=object(layout,L"grip");tileGrip.Visibility(grip.Size()?Visibility::Visible:Visibility::Collapsed);
        if(grip.Size()){place(tileGrip,grip);orientGrip(tileGripMark,num(grip,L"width")>num(grip,L"height"));}
    }
}
double PanelBody::ContentHeight(double width)const{
    if(width<=0){
        if(!root.IsLoaded()||root.Visibility()!=Visibility::Visible)return -1;
        width=root.ActualWidth();
    }
    if(!std::isfinite(width)||width<=0)return -1;
    if(navigator)return navigator->NaturalHeight(width);
    return contentHeight?contentHeight(width):-1;
}
void PanelBody::Apply(bool visible){
    if(navigator)navigator->Apply(visible);
    for(auto const& binding:bindings)binding();
}
