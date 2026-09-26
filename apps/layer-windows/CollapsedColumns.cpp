#include "pch.h"
#include "CollapsedColumns.h"
#include "WorkspaceGeometry.h"
#include <set>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

using namespace CapyUi;
namespace {
struct Column:std::enable_shared_from_this<Column>{
    std::shared_ptr<WorkspaceData> data;
    std::shared_ptr<WorkspaceGestures> gestures;
    Canvas workspace{nullptr},frame,icons;
    Shapes::Path surface;
    Border background,grip;
    ScrollViewer scroll;
    uint32_t id=0;
    J geometry;
    std::map<std::wstring,Button> buttons;
    std::map<uint32_t,Border> dividers;
    struct Connection {Shapes::Path path;hstring geometry;};
    std::map<std::wstring,Connection> connections;
    double reported=0;
    bool applying=false;
    std::array<float,4> corners{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
    void init(){
        auto weak=weak_from_this();
        background.Background(clear());background.CornerRadius({SurfaceRadius*CornerFit,SurfaceRadius*CornerFit,SurfaceRadius*CornerFit,SurfaceRadius*CornerFit});background.Child(frame);
        surface.Fill(data->glass(L"panel"));surface.IsHitTestVisible(false);frame.Children().Append(surface);
        Canvas::SetZIndex(background,160);
        AutomationProperties::SetAutomationId(background,L"collapsed-column-"+to_hstring(id));
        scroll.Content(icons);scroll.HorizontalScrollMode(ScrollMode::Disabled);
        scroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);
        scroll.VerticalScrollMode(ScrollMode::Enabled);scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Hidden);
        scroll.ViewChanged([weak](auto&&,auto&&){if(auto self=weak.lock()){
            auto offset=self->scroll.VerticalOffset();
            if(!self->applying&&std::abs(offset-self->reported)>.25){
                self->reported=offset;self->data->dispatch(O({{L"type",S(L"measure_column_scroll")},{L"column",N(self->id)},{L"offset",N(offset)}}));
            }
        }});
        frame.Children().Append(scroll);
        grip.Background(clear());grip.Child(panelGrip(data->theme(),true));
        auto item=O({{L"kind",S(L"column")},{L"column",N(id)}});
        gestures->Source(background,{},item,true);
        gestures->Source(grip,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item);
        AutomationProperties::SetAutomationId(grip,L"column-grip-"+to_hstring(id));
        AutomationProperties::SetName(grip,L"Move column");frame.Children().Append(grip);workspace.Children().Append(background);
    }
    J local(J const& bounds,J const& parent,double offset=0)const{
        auto result=J::Parse(bounds.Stringify());result.Insert(L"x",N(num(bounds,L"x")-num(parent,L"x")));
        result.Insert(L"y",N(num(bounds,L"y")-num(parent,L"y")+offset));return result;
    }
    void apply(J const& next){
        applying=true;geometry=next;
        auto bounds=object(geometry,L"bounds"),content=object(geometry,L"content");place(background,bounds);
        J source;for(auto value:data->drawerSources){auto anchor=object(value.GetObject(),L"anchor");
            if(str(anchor,L"kind")==L"column"&&num(anchor,L"column",-1)==id)source=value.GetObject();}
        corners={SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
        if(source.Size()){auto square=sourceCorners(source,bounds);for(int i=0;i<4;++i)if(square[i])corners[i]=0;}
        auto open=object(geometry,L"open");auto openDirection=str(open,L"direction");
        std::set<std::wstring> openSources;
        for(auto value:array(open,L"connections"))openSources.insert(std::wstring(value.GetArray().GetStringAt(0)));
        if(!openDirection.empty())for(auto value:array(geometry,L"groups"))for(auto iconValue:array(value.GetObject(),L"icons")){
            auto tile=iconValue.GetObject();if(!openSources.contains(std::wstring(str(tile,L"panel"))))continue;
            auto square=sourceCorners(O({{L"bounds",object(tile,L"bounds")},{L"direction",S(openDirection)}}),bounds);
            for(int i=0;i<4;++i)if(square[i])corners[i]=0;
        }
        auto facing=str(source,L"direction"),joined=str(object(source,L"anchor"),L"origin");
        surface.Data(squircleRectangle(float(num(bounds,L"width")),float(num(bounds,L"height")),corners));
        background.CornerRadius({corners[0]*CornerFit,corners[1]*CornerFit,corners[2]*CornerFit,corners[3]*CornerFit});
        place(grip,local(object(geometry,L"grip"),bounds));place(scroll,local(content,bounds));
        double offset=0;
        for(auto entry:array(object(object(data->state,L"workspace"),L"layout"),L"column_scroll")){
            auto pair=entry.GetArray();if(pair.Size()==2&&pair.GetNumberAt(0)==id)offset=pair.GetNumberAt(1);
        }
        hstring origin;
        for(auto value:array(object(data->state,L"customization"),L"column_drawers")){
            auto anchor=object(value.GetObject(),L"anchor");
            if(num(anchor,L"column") == id)origin=str(anchor,L"origin");
        }
        std::set<uint32_t> currentDividers;
        std::set<std::wstring> current;double bottom=num(content,L"height");
        for(auto value:array(geometry,L"groups")){
            auto group=value.GetObject();auto groupBounds=object(group,L"bounds");
            auto separator=object(group,L"divider");
            if(num(separator,L"height")>0){
                auto key=uint32_t(num(group,L"group"));currentDividers.insert(key);
                auto [line,added]=dividers.try_emplace(key);
                if(added){line->second.Background(data->brush(L"tabbar"));line->second.IsHitTestVisible(false);icons.Children().Append(line->second);
                    AutomationProperties::SetAutomationId(line->second,L"column-divider-"+to_hstring(key));}
                place(line->second,local(separator,content,offset));
            }
            bottom=std::max(bottom,num(groupBounds,L"y")+num(groupBounds,L"height")-num(content,L"y")+offset);
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
                    pick.Padding({0,0,0,0});pick.Content(icon(str(panel,L"icon"),data->theme()));
                    auto target=O({{L"kind",S(L"panel")},{L"panel",S(panelId)}});
                    gestures->Source(pick,O({{L"type",S(L"drag_workspace")},{L"item",target}}),target,false,{},WorkspaceGestures::Pickup::Hold);
                    AutomationProperties::SetAutomationId(pick,L"column-icon-"+panelId);
                    tooltip(pick,str(panel,L"title"));
                    icons.Children().Append(pick);found=buttons.emplace(key,pick).first;
                }
                auto pick=found->second;place(pick,local(object(tile,L"bounds"),content,offset));
                AutomationProperties::SetName(pick,str(panel,L"title"));tooltip(pick,str(panel,L"title"));
                bool active=(open.Size()&&str(group,L"active")==panelId)||origin==panelId;
                pick.Background(active?selected(data):clear());
                auto sourceFacing=!facing.empty()&&joined==panelId?facing:openSources.contains(key)?openDirection:hstring{};
                pick.CornerRadius(facingCorners(SurfaceRadius*CornerFit,sourceFacing));
                AutomationProperties::SetItemStatus(pick,active?L"Selected":L"");
            }
        }
        for(auto it=buttons.begin();it!=buttons.end();)if(!current.contains(it->first)){
            uint32_t index;if(icons.Children().IndexOf(it->second,index))icons.Children().RemoveAt(index);it=buttons.erase(it);
        }else ++it;
        for(auto it=dividers.begin();it!=dividers.end();)if(!currentDividers.contains(it->first)){
            uint32_t index;if(icons.Children().IndexOf(it->second,index))icons.Children().RemoveAt(index);it=dividers.erase(it);
        }else ++it;
        std::set<std::wstring> currentConnections;
        for(auto value:array(open,L"connections")){
            auto pair=value.GetArray();auto panel=pair.GetStringAt(0);auto link=pair.GetObjectAt(1);
            std::wstring key(panel);currentConnections.insert(key);
            auto [it,added]=connections.try_emplace(key);auto& connection=it->second;
            if(added){
                connection.path.Fill(data->glass(L"panel"));connection.path.IsHitTestVisible(false);
                Canvas::SetZIndex(connection.path,159);workspace.Children().Append(connection.path);
                AutomationProperties::SetAutomationId(connection.path,L"column-connection-"+to_hstring(id)+L"-"+panel);
            }
            auto encoded=link.Stringify();
            if(encoded!=connection.geometry){connection.geometry=encoded;place(connection.path,object(link,L"bounds"));connection.path.Data(drawerBridge(link));}
        }
        for(auto it=connections.begin();it!=connections.end();)if(!currentConnections.contains(it->first)){
            uint32_t index;if(workspace.Children().IndexOf(it->second.path,index))workspace.Children().RemoveAt(index);it=connections.erase(it);
        }else ++it;
        icons.Width(num(content,L"width"));icons.Height(bottom);
        reported=offset;
        if(std::abs(scroll.VerticalOffset()-offset)>.25)scroll.ChangeView(nullptr,offset,nullptr,true);
        applying=false;
    }
    void collectGlass(A& regions,A& links,UIElement const& reference)const{
        if(!appendGlass(regions,background,reference,{corners[0],corners[1],corners[2],corners[3]},true))return;
        for(auto value:array(object(geometry,L"open"),L"connections"))appendConnection(links,value.GetArray().GetObjectAt(1),workspace,reference);
    }
    void remove(){
        uint32_t index;if(workspace.Children().IndexOf(background,index))workspace.Children().RemoveAt(index);
        for(auto const& [key,connection]:connections)if(workspace.Children().IndexOf(connection.path,index))workspace.Children().RemoveAt(index);
    }
};
}
struct CollapsedColumns::Impl{
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr};
    std::shared_ptr<WorkspaceGestures> gestures;
    std::map<uint32_t,std::shared_ptr<Column>> columns;
    void reset(){for(auto const& [id,column]:columns)column->remove();columns.clear();}
    void apply(){
        std::set<uint32_t> current;
        if(!flag(data->model,L"chrome_hidden"))for(auto value:array(object(data->model,L"layout"),L"collapsed")){
            auto geometry=value.GetObject();uint32_t id=uint32_t(num(geometry,L"id"));current.insert(id);
            auto [it,added]=columns.try_emplace(id);
            if(added){auto column=std::make_shared<Column>();column->data=data;column->workspace=root;column->gestures=gestures;column->id=id;column->init();it->second=column;}
            it->second->apply(geometry);
        }
        for(auto it=columns.begin();it!=columns.end();)if(!current.contains(it->first)){it->second->remove();it=columns.erase(it);}else ++it;
    }
};
CollapsedColumns::CollapsedColumns(std::shared_ptr<WorkspaceData> data,Canvas root,std::shared_ptr<WorkspaceGestures> gestures):
    impl(std::make_shared<Impl>()){impl->data=std::move(data);impl->root=root;impl->gestures=std::move(gestures);}
CollapsedColumns::~CollapsedColumns(){impl->reset();}
void CollapsedColumns::Apply(){impl->apply();}
void CollapsedColumns::Reset(){impl->reset();}
void CollapsedColumns::AppendGlass(A& regions,A& connections,UIElement const& reference)const{for(auto const& [id,column]:impl->columns)column->collectGlass(regions,connections,reference);}
