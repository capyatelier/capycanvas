#include "pch.h"
#include "WorkspaceDrawers.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceShadow.h"
#include "WorkspaceQuery.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <chrono>
#include <set>

using namespace CapyUi;
namespace {
using Clock=std::chrono::steady_clock;
void remove(Canvas const& root,UIElement const& element){
    uint32_t index;if(root.Children().IndexOf(element,index))root.Children().RemoveAt(index);
}
Rect coordinates(A const& value){
    if(value.Size()!=4)return {};
    return {float(value.GetNumberAt(0)),float(value.GetNumberAt(1)),float(value.GetNumberAt(2)),float(value.GetNumberAt(3))};
}
struct Drawer:std::enable_shared_from_this<Drawer>{
    std::shared_ptr<WorkspaceData> data;
    Canvas workspace{nullptr},content;
    Border frame;
    WorkspaceShadow shadow;
    hstring shadowKey;
    Shapes::Path background,bridge;
    std::shared_ptr<WorkspaceGestures> gestures;
    std::function<void()> changed,closed;
    std::wstring id;
    J model,geometry,from;
    hstring modelKey,layoutKey,columnsKey,backgroundKey;
    std::map<std::wstring,J> panels;
    struct Body {hstring key;std::unique_ptr<PanelBody> view;};
    std::map<std::wstring,Body> bodies;
    struct Column {Grid frame;ScrollViewer scroll;StackPanel stack;std::vector<std::wstring> panels;};
    std::vector<Column> columns;
    std::map<std::wstring,J> toolbarLayouts;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::vector<double> heights;
    Clock::time_point started;
    bool closing=false,animating=false,dirty=true,busy=false,layingOut=false,disposed=false;
    uint64_t generation=0;
    int order()const{return id==L"tool"?220:200;}
    ~Drawer(){dispose();}
    void dispose(){
        if(disposed)return;disposed=true;++generation;
        if(timer)timer.Stop();
        remove(workspace,frame);remove(workspace,bridge);remove(workspace,shadow.Root());
    }
    void init(){
        auto weak=weak_from_this();
        frame.Background(clear());frame.Child(content);
        background.Fill(data->brush(L"panel"));background.IsHitTestVisible(false);content.Children().Append(background);
        bridge.Fill(data->brush(L"panel"));bridge.Visibility(Visibility::Collapsed);
        Canvas::SetZIndex(frame,order());Canvas::SetZIndex(bridge,order());
        AutomationProperties::SetAutomationId(frame,id==L"tool"?L"tool-drawer":hstring(L"column-drawer-"+id));
        AutomationProperties::SetName(frame,id==L"tool"?L"Tool drawer":L"Column drawer");
        workspace.Children().Append(shadow.Root());workspace.Children().Append(bridge);workspace.Children().Append(frame);
        timer=frame.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(16));
        timer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->drive();});
        frame.LayoutUpdated([weak](auto&&,auto&&){if(auto self=weak.lock())self->measured();});
        frame.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->timer.Stop();});
        frame.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->wake();});
    }
    void wake(){if(disposed)return;dirty=true;if(!timer.IsRunning())timer.Start();}
    void apply(J const& current,hstring const& layout){
        if(disposed)return;
        auto next=current.Size()?current.Stringify():hstring{};
        if(next!=modelKey){
            from=object(geometry,L"placement");started=Clock::now();animating=true;
            modelKey=next;closing=!current.Size();if(!closing)model=current;
            ++generation;heights.assign(array(model,L"columns").Size(),0.);wake();
        }
        if(layout!=layoutKey){layoutKey=layout;wake();}
        for(auto value:array(data->model,L"panels")){auto panel=value.GetObject();panels[std::wstring(str(panel,L"id"))]=panel;}
        rebuild();
        for(auto& [panel,body]:bodies)if(body.view)body.view->Apply(active(panel));
        frame.IsHitTestVisible(!closing);bridge.IsHitTestVisible(!closing);
    }
    bool active(std::wstring const& panel)const{
        for(auto const& column:columns)if(std::find(column.panels.begin(),column.panels.end(),panel)!=column.panels.end())return true;
        return false;
    }
    hstring toolbarKey(J const& panel,double width)const{
        return O({{L"panel",panelStructure(panel)},{L"width",N(width)}}).Stringify();
    }
    void drive(){
        if(disposed){timer.Stop();return;}
        if(busy)return;
        if(closing&&!object(geometry,L"placement").Size()){timer.Stop();closed();return;}
        // A tile body needs the shared wrapping geometry before native measurement.
        auto placements=array(object(geometry,L"placement"),L"columns"),models=array(model,L"columns");
        if(!closing)for(uint32_t i=0;i<std::min(placements.Size(),models.Size());++i){
            auto width=num(placements.GetObjectAt(i),L"width");
            for(auto value:models.GetArrayAt(i)){
                auto found=panels.find(std::wstring(value.GetString()));if(found==panels.end())continue;
                auto panel=found->second;if(!array(panel,L"tiles").Size())continue;
                auto key=toolbarKey(panel,width);
                if(toolbarLayouts.contains(std::wstring(key)))continue;
                busy=true;auto serial=generation;
                auto request=O({{L"type",S(L"drawer_toolbar")},{L"panel",S(str(panel,L"id"))},{L"width",N(width)},{L"height",N(800)}});
                if(!QueryWorkspace(data->query,request,[weak=weak_from_this(),serial,key](J reply){
                    if(auto self=weak.lock()){
                        self->busy=false;if(self->disposed)return;
                        if(serial==self->generation){
                            auto result=object(reply,L"result");
                            if(result.Size()){
                                if(self->toolbarLayouts.size()>=32)self->toolbarLayouts.erase(self->toolbarLayouts.begin());
                                self->toolbarLayouts[std::wstring(key)]=result;self->columnsKey=L"";self->rebuild();self->wake();
                            }
                        }
                    }
                }))busy=false;
                return;
            }
        }
        if(!dirty&&!animating){timer.Stop();return;}
        float progress=animating?std::clamp(std::chrono::duration<float>(Clock::now()-started).count()/.2f,0.f,1.f):1.f;
        A measured;for(auto height:heights)measured.Append(N(height));
        auto request=O({{L"type",S(L"drawer")},{L"column",id==L"tool"?JsonValue::CreateNullValue():N(std::stoul(id))},
            {L"heights",measured},{L"progress",N(progress)},{L"from",from.Size()?V(from):JsonValue::CreateNullValue()},{L"closing",B(closing)}});
        auto serial=generation;busy=true;dirty=false;
        if(!QueryWorkspace(data->query,request,[weak=weak_from_this(),serial,progress](J reply){
            if(auto self=weak.lock()){
                self->busy=false;if(self->disposed||serial!=self->generation)return;
                self->geometry=object(reply,L"result");
                if(progress>=1)self->animating=false;
                self->draw();self->rebuild();self->changed();
                if(self->closing&&progress>=1){self->timer.Stop();self->closed();}
            }
        })){busy=false;dirty=true;}
    }
    void rebuild(){
        auto placements=array(object(geometry,L"placement"),L"columns"),models=array(model,L"columns");
        if(!placements.Size()||placements.Size()!=models.Size())return;
        A signature;
        for(uint32_t i=0;i<models.Size();++i){
            double width=num(placements.GetObjectAt(i),L"width");A items;
            for(auto value:models.GetArrayAt(i)){
                auto found=panels.find(std::wstring(value.GetString()));if(found==panels.end())continue;
                auto panel=found->second;auto key=toolbarKey(panel,width);
                items.Append(O({{L"key",S(key)},{L"ready",B(!array(panel,L"tiles").Size()||toolbarLayouts.contains(std::wstring(key)))}}));
            }
            signature.Append(items);
        }
        A headers;for(auto value:array(object(model,L"tabs"),L"panels")){
            auto panel=panels.find(std::wstring(value.GetString()));if(panel==panels.end())continue;
            headers.Append(O({{L"title",S(str(panel->second,L"title"))},{L"icon",S(str(panel->second,L"icon"))}}));
        }
        auto key=O({{L"columns",signature},{L"tabs",object(model,L"tabs")},{L"headers",headers}}).Stringify();
        if(key==columnsKey){placeColumns();return;}
        columnsKey=key;
        // Detach before reusing a retained body in another native parent.
        for(auto& column:columns){column.stack.Children().Clear();remove(content,column.frame);}
        columns.clear();
        std::set<std::wstring> keep;
        auto tabs=object(model,L"tabs");auto weak=weak_from_this();
        for(uint32_t i=0;i<models.Size();++i){
            Column column;double width=num(placements.GetObjectAt(i),L"width");
            RowDefinition tabRow;tabRow.Height({tabs.Size()?36.:0.,GridUnitType::Pixel});column.frame.RowDefinitions().Append(tabRow);
            RowDefinition bodyRow;bodyRow.Height({1,GridUnitType::Star});column.frame.RowDefinitions().Append(bodyRow);
            if(tabs.Size()){
                auto group=num(tabs,L"group");
                auto groupItem=O({{L"kind",S(L"group")},{L"group",N(group)}});
                Grid header;header.Background(data->brush(L"tabbar"));
                ColumnDefinition tabColumn;tabColumn.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(tabColumn);
                ColumnDefinition gripColumn;gripColumn.Width({28,GridUnitType::Pixel});header.ColumnDefinitions().Append(gripColumn);
                StackPanel row;row.Orientation(Orientation::Horizontal);row.Background(data->brush(L"tabbar"));
                uint32_t index=0;
                for(auto value:array(tabs,L"panels")){
                    auto panelId=value.GetString();auto found=panels.find(std::wstring(panelId));if(found==panels.end())continue;
                    auto panel=found->second;bool selected=panelId==str(tabs,L"active");
                    auto pick=button(data,str(panel,L"title"),[data=data,group=num(tabs,L"group"),panelId]{
                        data->dispatch(O({{L"type",S(L"select_panel_tab")},{L"group",N(group)},{L"panel",S(panelId)}}));
                    });
                    pick.Height(36);pick.MinWidth(36);pick.Padding({8,4,8,4});pick.CornerRadius({6,6,0,0});
                    StackPanel labelRow;labelRow.Orientation(Orientation::Horizontal);labelRow.Spacing(6);
                    auto presentation=object(panel,L"tab");
                    if(flag(presentation,L"show_icon"))labelRow.Children().Append(icon(str(panel,L"icon"),data->theme()));
                    if(flag(presentation,L"show_name"))labelRow.Children().Append(label(data,str(panel,L"title"),true));
                    pick.Content(labelRow);
                    AutomationProperties::SetAutomationId(pick,L"drawer-tab-"+panelId);
                    auto item=O({{L"kind",S(L"panel")},{L"panel",S(panelId)}});
                    gestures->Source(pick,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item,false,
                        O({{L"group",N(group)},{L"index",N(index++)},{L"panel",S(panelId)}}));
                    row.Children().Append(panelTabShell(data,pick,selected));
                }
                ScrollViewer strip;strip.Content(row);strip.Background(data->brush(L"tabbar"));
                strip.HorizontalScrollMode(ScrollMode::Enabled);strip.HorizontalScrollBarVisibility(ScrollBarVisibility::Hidden);
                strip.VerticalScrollMode(ScrollMode::Disabled);header.Children().Append(strip);
                gestures->Source(strip,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem);
                Border grip;grip.Background(clear());grip.Width(20);grip.Height(36);
                grip.HorizontalAlignment(HorizontalAlignment::Left);grip.Child(panelGrip(data->theme()));
                gestures->Source(grip,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem);
                AutomationProperties::SetAutomationId(grip,L"drawer-grip-"+to_hstring(uint32_t(group)));
                AutomationProperties::SetName(grip,L"Move panel group");
                AutomationProperties::SetHelpText(grip,L"Drag to move every panel in this drawer.");
                Grid::SetColumn(grip,1);header.Children().Append(grip);
                column.frame.Children().Append(header);
            }
            column.stack.Spacing(6);column.stack.VerticalAlignment(VerticalAlignment::Top);
            for(auto value:models.GetArrayAt(i)){
                std::wstring panelId=value.GetString().c_str();auto found=panels.find(panelId);if(found==panels.end())continue;
                auto panel=found->second;auto bodyKey=toolbarKey(panel,width);auto toolbar=toolbarLayouts.find(std::wstring(bodyKey));
                if(array(panel,L"tiles").Size()&&toolbar==toolbarLayouts.end())continue;
                auto& body=bodies[panelId];keep.insert(panelId);column.panels.push_back(panelId);
                if(!body.view||body.key!=bodyKey){
                    auto placement=O({{L"bounds",O({{L"width",N(width)}})}});
                    if(toolbar!=toolbarLayouts.end())placement.Insert(L"tiles",toolbar->second);
                    body.view=std::make_unique<PanelBody>(data,panel,placement,[weak]{if(auto self=weak.lock())self->measured();},gestures,false);
                    body.key=bodyKey;
                    if(toolbar!=toolbarLayouts.end())body.view->Root().Height(std::max(36.,num(toolbar->second,L"content_height")));
                }
                body.view->Root().Width(width);body.view->Apply(true);column.stack.Children().Append(body.view->Root());
            }
            column.scroll.Content(column.stack);column.scroll.HorizontalScrollMode(ScrollMode::Disabled);
            column.scroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);column.scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);
            Grid::SetRow(column.scroll,1);column.frame.Children().Append(column.scroll);
            content.Children().Append(column.frame);columns.push_back(std::move(column));
        }
        // Retain inactive tab bodies, including their native control state.
        for(auto& [panel,body]:bodies)if(body.view&&!keep.contains(panel))body.view->Apply(false);
        placeColumns();wake();
    }
    void placeColumns(){
        auto placements=array(object(geometry,L"placement"),L"columns");
        for(uint32_t i=0;i<std::min(uint32_t(columns.size()),placements.Size());++i)place(columns[i].frame,placements.GetObjectAt(i));
    }
    void draw(){
        auto placement=object(geometry,L"placement");auto bounds=object(placement,L"bounds");
        frame.Visibility(bounds.Size()?Visibility::Visible:Visibility::Collapsed);
        shadow.Layout(rectangle(bounds),order(),bounds.Size()!=0);
        if(!bounds.Size()){bridge.Visibility(Visibility::Collapsed);return;}
        place(frame,bounds);
        RectangleGeometry clip;clip.Rect({0,0,float(num(bounds,L"width")),float(num(bounds,L"height"))});content.Clip(clip);
        auto connection=object(geometry,L"connection");
        bridge.Visibility(connection.Size()?Visibility::Visible:Visibility::Collapsed);
        if(connection.Size()){place(bridge,object(connection,L"bounds"));bridge.Data(drawerBridge(connection));}
        // A local, inspectable geometry record; no documents or profile paths.
        AutomationProperties::SetItemStatus(frame,geometry.Stringify());
        paintBackground();
    }
    void measured(){
        if(disposed||layingOut||!frame.IsLoaded()||columns.empty())return;
        layingOut=true;
        double tabHeight=object(model,L"tabs").Size()?36.:0.;
        std::vector<double> next;for(auto const& column:columns)next.push_back(column.stack.ActualHeight()+tabHeight);
        bool differs=next.size()!=heights.size();
        if(!differs)for(size_t i=0;i<next.size();++i)if(std::abs(next[i]-heights[i])>.5){differs=true;break;}
        if(differs){heights=std::move(next);wake();}
        paintBackground();changed();layingOut=false;
    }
    void paintBackground(){
        if(!frame.IsLoaded())return;
        auto bounds=object(object(geometry,L"placement"),L"bounds");float width=float(num(bounds,L"width")),height=float(num(bounds,L"height"));
        if(width<=0||height<=0)return;
        A holes;auto tabs=array(data->state,L"tabs");auto document=tabs.Size()?tabs.GetObjectAt(0):J{};
        for(auto const& column:columns)for(auto const& panel:column.panels){
            auto const& body=bodies.at(panel).view;if(!body||!body->navigator)continue;
            auto clip=visibleBounds(column.scroll,content);
            auto slot=body->navigator->Placement(content,clip,order());if(!slot.Size())continue;
            auto nav=coordinates(array(slot,L"bounds"));float image[4]{};
            if(!capy_navigator_image(nav.Width,nav.Height,uint32_t(num(document,L"width")),uint32_t(num(document,L"height")),image))continue;
            auto hole=intersect(intersect({nav.X+image[0],nav.Y+image[1],image[2],image[3]},coordinates(array(slot,L"clip"))),{0,0,width,height});
            if(hole.Width>0&&hole.Height>0)holes.Append(rectangle(hole));
        }
        auto corners=array(object(geometry,L"connection"),L"square_corners");
        auto key=O({{L"width",N(width)},{L"height",N(height)},{L"corners",corners},{L"holes",holes}}).Stringify();
        if(key==backgroundKey)return;backgroundKey=key;
        std::array<float,4> radii{8,8,8,8};
        for(uint32_t i=0;i<std::min(4u,corners.Size());++i)if(corners.GetBooleanAt(i))radii[i]=0;
        frame.CornerRadius({radii[0],radii[1],radii[2],radii[3]});
        auto outline=roundedRectangle(width,height,radii);
        auto silhouette=O({{L"width",N(width)},{L"height",N(height)},{L"corners",corners}}).Stringify();
        if(silhouette!=shadowKey){shadowKey=silhouette;shadow.Shape(roundedRectangle(width,height,radii),width,height,21,5,1.f/3);}
        GeometryGroup shape;shape.FillRule(FillRule::EvenOdd);shape.Children().Append(outline);
        for(auto hole:holes){RectangleGeometry region;region.Rect(rectangle(hole.GetObject()));shape.Children().Append(region);}
        background.Data(shape);
    }
    void appendOverviews(A& slots)const{
        if(disposed||!frame.IsLoaded())return;
        for(auto const& column:columns)for(auto const& panel:column.panels){
            auto const& body=bodies.at(panel).view;
            if(!body||!body->navigator)continue;
            auto slot=body->navigator->Placement(workspace,visibleBounds(column.scroll,workspace),visualOrder(workspace,frame));
            if(slot.Size())slots.Append(slot);
        }
    }
    void appendBounds(A& measurements)const{
        if(id==L"tool"||disposed||closing)return;
        auto bounds=visibleBounds(frame,workspace);
        if(bounds.Width>0&&bounds.Height>0)measurements.Append(O({
            {L"group",N(num(object(model,L"anchor"),L"group"))},{L"bounds",rectangle(bounds)}}));
    }
    void appendTiles(A& tiles)const{
        if(id==L"tool"||disposed||closing)return;
        for(auto const& column:columns)for(auto const& panel:column.panels){
            auto const& body=bodies.at(panel).view;if(!body)continue;
            for(auto const& [tile,element]:body->tileElements){
                auto bounds=visibleBounds(element,workspace);if(bounds.Width<=0||bounds.Height<=0)continue;
                tiles.Append(O({{L"column",N(std::stoul(id))},{L"anchor",O({{L"panel",S(hstring(panel))},{L"tile",N(tile)}})},
                    {L"bounds",rectangle(bounds)}}));
            }
        }
    }
};
}
struct WorkspaceDrawers::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr};
    std::shared_ptr<WorkspaceGestures> gestures;
    std::function<void()> changed;
    std::map<std::wstring,std::shared_ptr<Drawer>> drawers;
    hstring lastTiles=L"[]",lastBounds=L"[]",lastFacts;
    bool publishing=false,resetting=false;
    void publish(){
        if(publishing||resetting)return;publishing=true;
        A measuredBounds;for(auto const& [id,drawer]:drawers)drawer->appendBounds(measuredBounds);
        auto placed=measuredBounds.Stringify();
        if(placed!=lastBounds){
            lastBounds=placed;data->dispatch(O({{L"type",S(L"measure_column_drawers")},{L"measurements",measuredBounds}}));
        }
        A tiles;for(auto const& [id,drawer]:drawers)drawer->appendTiles(tiles);
        auto measurements=tiles.Stringify();
        if(measurements!=lastTiles){
            lastTiles=measurements;data->dispatch(O({{L"type",S(L"measure_drawer_tiles")},{L"measurements",tiles}}));
            if(auto it=drawers.find(L"tool");it!=drawers.end())it->second->wake();
        }
        auto tool=drawers.find(L"tool");J geometry=tool==drawers.end()?J{}:tool->second->geometry;
        auto bounds=object(object(geometry,L"placement"),L"bounds"),connection=object(object(geometry,L"connection"),L"bounds");
        auto facts=O({{L"content_drawer",bounds.Size()?V(bounds):JsonValue::CreateNullValue()},
            {L"drawer_connection",connection.Size()?V(connection):JsonValue::CreateNullValue()}});
        if(facts.Stringify()!=lastFacts){
            lastFacts=facts.Stringify();for(auto const& fact:facts)data->chrome.Insert(fact.Key(),fact.Value());
            gestures->ChromeChanged();
        }
        changed();publishing=false;
    }
    void apply(){
        auto customization=object(data->state,L"customization");std::map<std::wstring,J> models;
        for(auto value:array(customization,L"column_drawers")){
            auto model=value.GetObject();models[std::to_wstring(uint32_t(num(object(model,L"anchor"),L"column")))]=model;
        }
        auto tool=object(customization,L"drawer");if(tool.Size())models[L"tool"]=tool;
        for(auto const& [id,model]:models)if(!drawers.contains(id)){
            auto drawer=std::make_shared<Drawer>();drawer->data=data;drawer->workspace=root;drawer->gestures=gestures;drawer->id=id;
            auto weak=weak_from_this();auto weakDrawer=std::weak_ptr<Drawer>(drawer);
            drawer->changed=[weak]{if(auto self=weak.lock())self->publish();};
            drawer->closed=[weak,weakDrawer,id]{
                if(auto self=weak.lock()){
                    auto found=self->drawers.find(id);
                    if(found!=self->drawers.end()&&found->second==weakDrawer.lock()){found->second->dispose();self->drawers.erase(found);self->publish();}
                }
            };
            drawers[id]=drawer;drawer->init();
        }
        auto layout=object(data->model,L"layout").Stringify();
        for(auto const& [id,drawer]:drawers){auto model=models.find(id);drawer->apply(model==models.end()?J{}:model->second,layout);}
        publish();
    }
    void reset(){
        resetting=true;for(auto const& [id,drawer]:drawers)drawer->dispose();drawers.clear();resetting=false;publish();
    }
};
WorkspaceDrawers::WorkspaceDrawers(std::shared_ptr<WorkspaceData> data,Canvas root,std::shared_ptr<WorkspaceGestures> gestures,std::function<void()> changed):
    impl(std::make_shared<Impl>()){impl->data=std::move(data);impl->root=root;impl->gestures=std::move(gestures);impl->changed=std::move(changed);}
WorkspaceDrawers::~WorkspaceDrawers(){impl->reset();}
void WorkspaceDrawers::Apply(){impl->apply();}
void WorkspaceDrawers::Reset(){impl->reset();}
void WorkspaceDrawers::AppendOverviews(A& slots)const{for(auto const& [id,drawer]:impl->drawers)drawer->appendOverviews(slots);}
FrameworkElement WorkspaceDrawers::Anchor(std::wstring const& control)const{
    for(auto const& [id,drawer]:impl->drawers)if(!drawer->closing)for(auto const& column:drawer->columns)for(auto const& panel:column.panels){
        auto const& body=drawer->bodies.at(panel).view;if(!body)continue;
        if(auto found=body->anchors.find(control);found!=body->anchors.end())return found->second;
    }
    return nullptr;
}
