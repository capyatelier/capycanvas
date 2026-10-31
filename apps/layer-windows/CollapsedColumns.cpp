#include "pch.h"
#include "CollapsedColumns.h"
#include "ChromeVisibility.h"
#include "WorkspaceGeometry.h"
#include "PanelScroll.h"
#include <set>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

using namespace CapyUi;
namespace {
struct Column:std::enable_shared_from_this<Column>{
    std::shared_ptr<WorkspaceData> data;
    std::shared_ptr<WorkspaceGestures> gestures;
    std::function<void()> changed;
    std::shared_ptr<ChromeVisibility> fade=std::make_shared<ChromeVisibility>();
    Canvas workspace{nullptr},frame,icons;
    Shapes::Path surface;
    Border background,grip;
    ScrollView scroll;
    uint32_t id=0;
    J geometry;
    std::map<std::wstring,Button> buttons;
    std::map<std::wstring,Bindings> paintIcons;
    std::map<uint32_t,Border> dividers;
    bool gpu=false;
    double reported=0;
    bool applying=false;
    uint32_t pendingScroll=0;
    std::array<float,4> corners{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius},sourceRadii{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
    std::array<float,6> surfaceShape{};
    void shape(float width,float height){
        std::array<float,6> next{width,height,corners[0],corners[1],corners[2],corners[3]};
        if(next==surfaceShape)return;
        surfaceShape=next;surface.Data(squircleRectangle(width,height,corners));
        background.CornerRadius({corners[0]*CornerFit,corners[1]*CornerFit,corners[2]*CornerFit,corners[3]*CornerFit});
    }
    void init(){
        auto weak=weak_from_this();
        background.Background(clear());background.CornerRadius({SurfaceRadius*CornerFit,SurfaceRadius*CornerFit,SurfaceRadius*CornerFit,SurfaceRadius*CornerFit});background.Child(frame);
        surface.Fill(data->glass(L"panel"));surface.IsHitTestVisible(false);frame.Children().Append(surface);
        Canvas::SetZIndex(background,160);
        AutomationProperties::SetAutomationId(background,L"collapsed-column-"+to_hstring(id));
        icons.Background(clear());scroll.Content(icons);scroll.ContentOrientation(ScrollingContentOrientation::Vertical);panelScrolling(scroll,icons,data);
        scroll.HorizontalScrollMode(ScrollingScrollMode::Disabled);
        scroll.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
        scroll.VerticalScrollMode(ScrollingScrollMode::Enabled);scroll.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
        scroll.ViewChanged([weak](auto&&,auto&&){if(auto self=weak.lock()){
            auto offset=self->scroll.VerticalOffset();
            if(!self->applying&&std::abs(offset-self->reported)>.25){
                self->reported=offset;++self->pendingScroll;
                self->data->dispatch(O({{L"type",S(L"measure_column_scroll")},{L"column",N(self->id)},{L"offset",N(offset)}}),[weak]{
                    if(auto self=weak.lock())--self->pendingScroll;
                });
            }
        }});
        frame.Children().Append(scroll);
        grip.Background(clear());grip.Child(panelGrip(data->theme(),true));
        auto item=O({{L"kind",S(L"column")},{L"column",N(id)}});
        gestures->Source(background,{},item,true);edgeTag(background,L"edge_surface",B(true));
        gestures->Source(grip,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item);
        AutomationProperties::SetAutomationId(grip,L"column-grip-"+to_hstring(id));
        copyName(data,grip,data->copyDelivery(L"move_column"));frame.Children().Append(grip);workspace.Children().Append(background);
    }
    J local(J const& bounds,J const& parent,double offset=0)const{
        auto result=J::Parse(bounds.Stringify());result.Insert(L"x",N(num(bounds,L"x")-num(parent,L"x")));
        result.Insert(L"y",N(num(bounds,L"y")-num(parent,L"y")+offset));return result;
    }
    void apply(J const& next){
        applying=true;geometry=next;
        auto bounds=rectangle(pixelBounds(rectangle(object(geometry,L"bounds")),workspace)),content=object(geometry,L"content");placeOnPixelGrid(background,bounds,workspace);
        J source;for(auto value:data->drawerSources){auto anchor=object(value.GetObject(),L"anchor");
            if(str(anchor,L"kind")==L"column"&&num(anchor,L"column",-1)==id)source=value.GetObject();}
        corners={SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
        if(source.Size()){auto square=sourceCorners(data,source,bounds);for(int i=0;i<4;++i)if(square[i])corners[i]=0;}
        sourceRadii=corners;auto open=object(geometry,L"open");
        auto facing=str(source,L"direction"),joined=str(object(source,L"anchor"),L"origin");
        shape(float(num(bounds,L"width")),float(num(bounds,L"height")));
        place(grip,local(object(geometry,L"grip"),bounds));place(scroll,local(content,bounds));
        double offset=num(geometry,L"scroll_offset");
        hstring origin;
        for(auto value:array(object(data->state,L"customization"),L"column_drawers")){
            auto anchor=object(value.GetObject(),L"anchor");
            if(num(anchor,L"column") == id)origin=str(anchor,L"origin");
        }
        std::set<uint32_t> currentDividers;
        std::set<std::wstring> current;
        for(auto value:array(geometry,L"groups")){
            auto group=value.GetObject();
            auto separator=object(group,L"divider");
            if(num(separator,L"height")>0){
                auto key=uint32_t(num(group,L"group"));currentDividers.insert(key);
                auto [line,added]=dividers.try_emplace(key);
                if(added){line->second=dividerLine(data,true);icons.Children().Append(line->second);
                    AutomationProperties::SetAutomationId(line->second,L"column-divider-"+to_hstring(key));}
                auto at=local(separator,content,offset);
                at.SetNamedValue(L"x",N(num(at,L"x")+(num(at,L"width")-DividerLength)/2));at.SetNamedValue(L"width",N(DividerLength));
                place(line->second,at);
            }
            for(auto iconValue:array(group,L"icons")){
                auto tile=iconValue.GetObject();auto panelId=str(tile,L"panel");std::wstring key=panelId.c_str();current.insert(key);
                auto panel=find(array(data->model,L"panels"),L"id",panelId);
                auto found=buttons.find(key);
                if(found==buttons.end()){
                    auto pick=button(data,str(panel,L"title"),[weak=weak_from_this(),panelId]{
                        if(auto self=weak.lock();self&&!self->gestures->SuppressClick())for(auto value:array(self->geometry,L"groups")){
                            auto group=value.GetObject();
                            for(auto iconValue:array(group,L"icons"))if(str(iconValue.GetObject(),L"panel")==panelId){
                                self->data->dispatch(O({{L"type",S(L"customize")},{L"action",O({{L"type",S(L"toggle_column_drawer")},
                                    {L"group",N(num(group,L"group"))},{L"panel",S(panelId)}})}}));return;
                            }
                        }
                    });
                    pick.Padding({0,0,0,0});pick.Content(panelIcon(data,str(panel,L"icon"),paintIcons[key]));
                    auto target=O({{L"kind",S(L"panel")},{L"panel",S(panelId)}});
                    gestures->Source(pick,O({{L"type",S(L"drag_workspace")},{L"item",target}}),target,false,{},WorkspaceGestures::Pickup::Hold);
                    AutomationProperties::SetAutomationId(pick,L"column-icon-"+panelId);
                    tooltip(pick,str(panel,L"title"));
                    icons.Children().Append(pick);found=buttons.emplace(key,pick).first;
                }
                auto pick=found->second;place(pick,local(object(tile,L"bounds"),content,offset));
                AutomationProperties::SetName(pick,str(panel,L"title"));tooltip(pick,str(panel,L"title"));
                bool active=(open.Size()&&str(group,L"active")==panelId)||origin==panelId;
                pick.Background(active?selected(data):clear());buttonFeedback(pick,data->brush(L"text"),20,41,nullptr,active);
                auto sourceFacing=!facing.empty()&&joined==panelId?facing:hstring{};
                pick.CornerRadius(facingCorners(SurfaceRadius*CornerFit,sourceFacing));
                AutomationProperties::SetItemStatus(pick,active?data->caption(L"search",L"selected"):L"");
            }
        }
        for(auto it=buttons.begin();it!=buttons.end();)if(!current.contains(it->first)){
            uint32_t index=0;if(!gestures->DeferRemoval(it->second,shared_from_this())&&icons.Children().IndexOf(it->second,index))icons.Children().RemoveAt(index);paintIcons.erase(it->first);it=buttons.erase(it);
        }else ++it;
        for(auto const& [key,updates]:paintIcons)for(auto const& update:updates)update();
        for(auto it=dividers.begin();it!=dividers.end();)if(!currentDividers.contains(it->first)){
            uint32_t index=0;if(!gestures->DeferRemoval(it->second,shared_from_this())&&icons.Children().IndexOf(it->second,index))icons.Children().RemoveAt(index);it=dividers.erase(it);
        }else ++it;
        icons.Width(num(content,L"width"));icons.Height(num(content,L"height")+num(geometry,L"scroll_max"));
        if(!pendingScroll){reported=offset;if(std::abs(scroll.VerticalOffset()-offset)>.25)scrollVertical(scroll,offset);}
        applying=false;
    }
    void visible(bool shown){fade->apply(background,true,!shown,[weak=weak_from_this()]{if(auto self=weak.lock();self&&self->changed)self->changed();});}
    J geometryMeasure()const{
        auto box=rectangle(pixelBounds(rectangle(object(geometry,L"bounds")),workspace));if(num(box,L"width")<=0||num(box,L"height")<=0||background.Opacity()<=0)return J{};
        A measured;
        for(auto value:array(geometry,L"groups"))for(auto icon:array(value.GetObject(),L"icons")){
            auto item=icon.GetObject();A pair;pair.Append(S(str(item,L"panel")));pair.Append(object(item,L"bounds"));measured.Append(pair);}
        return O({{L"column",N(id)},{L"bounds",box},{L"content",object(geometry,L"content")},{L"icons",measured}});
    }
    void present(J const& frame,bool ready){
        J column;for(auto value:array(frame,L"columns"))if(num(value.GetObject(),L"column")==id){column=value.GetObject();break;}
        auto radii=array(column,L"radii");corners=sourceRadii;
        if(radii.Size()==4)for(uint32_t i=0;i<4;++i)corners[i]=std::min(corners[i],float(radii.GetNumberAt(i)));
        for(auto value:array(column,L"icons")){auto pair=value.GetArray();auto found=buttons.find(std::wstring(pair.GetStringAt(0)));if(found==buttons.end())continue;
            auto r=pair.GetArrayAt(1);found->second.CornerRadius({r.GetNumberAt(0)*CornerFit,r.GetNumberAt(1)*CornerFit,r.GetNumberAt(2)*CornerFit,r.GetNumberAt(3)*CornerFit});}
        gpu=ready;
        surface.Opacity(gpu?0:1);
        auto snapped=pixelBounds(rectangle(object(geometry,L"bounds")),workspace);shape(snapped.Width,snapped.Height);
    }
    void collectGlass(A& regions,UIElement const& reference)const{
        if(!gpu)appendGlass(regions,background,reference,{corners[0],corners[1],corners[2],corners[3]},true);
    }
    void remove(){
        uint32_t index=0;if(!gestures->DeferRemoval(background,shared_from_this())&&workspace.Children().IndexOf(background,index))workspace.Children().RemoveAt(index);
    }
};
}
struct CollapsedColumns::Impl{
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr};
    std::shared_ptr<WorkspaceGestures> gestures;
    std::map<uint32_t,std::shared_ptr<Column>> columns;
    std::function<void()> changed;
    void reset(){for(auto const& [id,column]:columns)column->remove();columns.clear();}
    void apply(){
        std::set<uint32_t> current;
        for(auto value:array(object(data->model,L"layout"),L"collapsed")){
            auto geometry=value.GetObject();uint32_t id=uint32_t(num(geometry,L"id"));current.insert(id);
            auto [it,added]=columns.try_emplace(id);
            if(added){auto column=std::make_shared<Column>();column->data=data;column->workspace=root;column->gestures=gestures;column->changed=changed;column->id=id;column->init();it->second=column;}
            it->second->apply(geometry);it->second->visible(!flag(data->model,L"chrome_hidden"));
        }
        for(auto it=columns.begin();it!=columns.end();)if(!current.contains(it->first)){it->second->remove();it=columns.erase(it);}else ++it;
    }
};
CollapsedColumns::CollapsedColumns(std::shared_ptr<WorkspaceData> data,Canvas root,std::shared_ptr<WorkspaceGestures> gestures,std::function<void()> changed):
    impl(std::make_shared<Impl>()){impl->data=std::move(data);impl->root=root;impl->gestures=std::move(gestures);impl->changed=std::move(changed);}
CollapsedColumns::~CollapsedColumns(){impl->reset();}
void CollapsedColumns::Apply(){impl->apply();}
A CollapsedColumns::Geometry()const{A result;for(auto const& [id,column]:impl->columns){auto value=column->geometryMeasure();if(value.Size())result.Append(value);}return result;}
void CollapsedColumns::ForEachBounds(UIElement const& reference,std::function<void(uint32_t,Rect)> const& visit)const{
    for(auto const& [id,column]:impl->columns){
        if(!column->background.IsLoaded()||column->background.Visibility()!=Visibility::Visible||column->background.Opacity()<=0)continue;
        auto bounds=allocatedBounds(column->background,reference);if(bounds.Width>0&&bounds.Height>0)visit(id,bounds);
    }
}
void CollapsedColumns::PresentBridges(J const& frame,bool resizing){for(auto const& [id,column]:impl->columns)column->present(frame,!resizing&&impl->data->chromeFrames->Column(id));}
double CollapsedColumns::Opacity(uint32_t column)const{auto found=impl->columns.find(column);return found==impl->columns.end()?0:found->second->background.Opacity();}
void CollapsedColumns::AppendGlass(A& regions,UIElement const& reference,std::vector<uint32_t> const& publishedColumns)const{for(auto const& [id,column]:impl->columns){
    if(column->background.IsLoaded()&&column->background.Visibility()==Visibility::Visible)impl->data->chromeFrames->Track(column->background);
    if(std::find(publishedColumns.begin(),publishedColumns.end(),id)==publishedColumns.end())column->collectGlass(regions,reference);
}}
