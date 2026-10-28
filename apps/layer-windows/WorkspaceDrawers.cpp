#include "pch.h"
#include "WorkspaceDrawers.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceShadow.h"
#include "WorkspaceQuery.h"
#include "LayerInspector.h"
#include "PanelScroll.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Windows.UI.ViewManagement.h>
#include <chrono>
#include <set>
#include <cmath>

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
    J model,geometry,from,presentedSource;
    hstring modelKey,layoutKey,columnsKey,backgroundKey,tabsKey;
    std::map<std::wstring,J> panels;
    struct Body {hstring key;bool splitFilters=false;std::unique_ptr<PanelBody> view;};
    std::map<std::wstring,Body> bodies;
    struct Column {Grid frame;Grid header{nullptr};std::map<std::wstring,Grid> tabs;ScrollView scroll;StackPanel stack;std::vector<std::wstring> panels;std::vector<AutomaticTab> automatic;Bindings paintIcons;};
    std::map<std::wstring,double> tabWidths;
    std::vector<Column> columns;
    std::map<std::wstring,J> toolbarLayouts;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::vector<double> heights;
    std::unique_ptr<LayerInspector> inspector;
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
        frame.Background(clear());frame.Visibility(Visibility::Collapsed);frame.Child(content);
        background.Fill(data->glass(L"panel"));background.IsHitTestVisible(false);content.Children().Append(background);
        bridge.Fill(data->glass(L"panel"));bridge.Visibility(Visibility::Collapsed);
        Canvas::SetZIndex(frame,order());Canvas::SetZIndex(bridge,order());
        AutomationProperties::SetAutomationId(frame,id==L"tool"?L"tool-drawer":hstring(L"column-drawer-"+id));
        AutomationProperties::SetName(frame,id==L"tool"?L"Tool drawer":L"Column drawer");
        workspace.Children().Append(shadow.Root());workspace.Children().Append(bridge);workspace.Children().Append(frame);
        timer=frame.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(8));
        timer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->drive();});
        frame.LayoutUpdated([weak](auto&&,auto&&){if(auto self=weak.lock())self->measured();});
        frame.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->timer.Stop();});
        frame.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->wake();});
    }
    void wake(){if(disposed)return;dirty=true;if(!timer.IsRunning())timer.Start();}
    void apply(J const& current,hstring const& layout){
        if(disposed)return;
        auto next=current.Size()?current.Stringify():hstring{};
        bool changedModel=next!=modelKey;
        if(changedModel){
            bool pageChange=inspector&&current.HasKey(L"layer_properties")&&model.HasKey(L"layer_properties")&&flag(current,L"layer_properties")!=flag(model,L"layer_properties");
            from=object(geometry,L"placement");started=Clock::now();animating=true;
            modelKey=next;closing=!current.Size();if(!closing)model=current;
            ++generation;heights.assign(array(model,L"columns").Size(),0.);wake();
            if(pageChange){
                inspector->startPosition=inspector->position;inspector->targetPosition=flag(model,L"layer_properties")?1.:0.;inspector->started=started;inspector->animating=true;
                if(flag(model,L"layer_properties")){auto layer=bodies.find(L"layers");if(layer!=bodies.end()&&layer->second.view&&layer->second.view->layers)inspector->anchor=layer->second.view->layers->scrollAnchor();}
                animating=false;
            }
        }
        if(layout!=layoutKey){layoutKey=layout;wake();}

        for(auto value:array(data->model,L"panels")){auto panel=value.GetObject();panels[std::wstring(str(panel,L"id"))]=panel;}
        rebuild();
        if(changedModel&&!closing)for(auto& column:columns)if(column.header){
            auto strip=column.header.Children().GetAt(0).as<ScrollView>();
            if(orderPanelTabs(strip.Content().as<StackPanel>(),array(object(model,L"tabs"),L"panels"),column.tabs,column.automatic))
                fitAutomaticTabs(data,column.automatic,strip.ActualWidth(),tabWidths);
        }
        for(auto const& column:columns)for(auto const& update:column.paintIcons)update();
        for(auto& [panel,body]:bodies)if(body.view)body.view->Apply(active(panel));
        if(inspector&&model.HasKey(L"layer_properties")){inspector->properties->Apply(flag(model,L"layer_properties"));refreshInspector();if(inspectorSizingKey()!=inspector->sizingKey)wake();}
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
        if(inspector&&model.HasKey(L"layer_properties")&&!closing&&!inspector->animating&&inspector->position==0&&inspector->anchor.Size()){
            auto layer=bodies.at(L"layers").view->layers;
            if(layer&&layer->restoreAnchor(inspector->anchor))inspector->anchor=J{};else dirty=true;
        }
        double pagePosition=inspector?inspector->position:0.,pageProgress=1.;bool pageComplete=false;
        bool motion=Windows::UI::ViewManagement::UISettings().AnimationsEnabled();
        if(inspector&&model.HasKey(L"layer_properties")&&!closing){
            auto layer=bodies.find(L"layers");
            if(layer!=bodies.end()&&layer->second.view){
                auto request=inspectorSizingRequest();auto key=inspectorSizingKey(request);
                if(key!=inspector->sizingKey){
                    busy=true;auto serial=generation;
                    if(!QueryWorkspace(data->query,request,[weak=weak_from_this(),serial,key](J reply){if(auto self=weak.lock()){
                        self->busy=false;if(self->disposed||serial!=self->generation||!self->inspector)return;
                        self->inspector->sizingKey=key;self->inspector->sizes=object(reply,L"result");self->wake();
                    }}))busy=false;
                    return;
                }
            }
            if(inspector->animating){
                pageProgress=motion?std::clamp(std::chrono::duration<double>(Clock::now()-inspector->started).count()/.2,0.,1.):1.;
                pagePosition=inspector->startPosition+(inspector->targetPosition-inspector->startPosition)*(1.-std::pow(1.-pageProgress,3));
                pageComplete=pageProgress>=1.;dirty=true;
            }
            auto sizes=array(inspector->sizes,L"heights");
            if(sizes.Size()==2)heights={sizes.GetNumberAt(0)*(1.-pagePosition)+sizes.GetNumberAt(1)*pagePosition};
        }
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
                                self->toolbarLayouts[std::wstring(key)]=result;self->rebuild();self->wake();
                            }
                        }
                    }
                }))busy=false;
                return;
            }
        }
        if(!dirty&&!animating&&!(inspector&&inspector->animating)){dirty=false;timer.Stop();return;}
        float progress=animating&&motion?std::clamp(std::chrono::duration<float>(Clock::now()-started).count()/.2f,0.f,1.f):1.f;
        auto endpoints=inspector?array(inspector->sizes,L"placements"):A{};
        J endpoint;
        if(endpoints.Size()==2){auto value=endpoints.GetAt(flag(model,L"layer_properties")?1:0);if(value.ValueType()==JsonValueType::Object)endpoint=value.GetObject();}
        if(inspector&&model.HasKey(L"layer_properties")&&((!closing&&endpoint.Size())||closing&&from.Size())){
            auto end=closing?from:endpoint;
            auto fraction=inspector->animating?pageProgress:progress;
            auto request=O({{L"from",from.Size()?V(from):JsonValue::CreateNullValue()},{L"to",end},{L"progress",N(1.-std::pow(1.-fraction,3))},{L"closing",B(closing)}});
            NativeString raw(capy_drawer_frame(data->localization.get(),to_string(request.Stringify()).c_str()),capy_string_free);
            if(!raw)throw hresult_invalid_argument(L"Drawer frame failed");
            geometry=J::Parse(to_hstring(raw.get()));inspector->position=pagePosition;
            if(pageComplete)inspector->animating=false;
            if(progress>=1)animating=false;
            dirty=false;rebuild();draw();changed();
            if(closing&&progress>=1){timer.Stop();closed();}
            return;
        }
        A measured;for(auto height:heights)measured.Append(N(height));
        auto request=O({{L"type",S(L"drawer")},{L"column",id==L"tool"?JsonValue::CreateNullValue():N(std::stoul(id))},
            {L"heights",measured},{L"progress",N(progress)},{L"from",from.Size()?V(from):JsonValue::CreateNullValue()},{L"closing",B(closing)}});
        auto serial=generation;busy=true;dirty=false;
        if(!QueryWorkspace(data->query,request,[weak=weak_from_this(),serial,progress,pagePosition,pageComplete](J reply){
            if(auto self=weak.lock()){
                self->busy=false;if(self->disposed||serial!=self->generation)return;
                self->geometry=object(reply,L"result");
                if(self->inspector){self->inspector->position=pagePosition;if(pageComplete)self->inspector->animating=false;}
                if(progress>=1)self->animating=false;
                self->rebuild();
                if(!self->closing&&self->measureHeights()){self->wake();return;}
                self->draw();self->changed();
                if(self->closing&&progress>=1){self->timer.Stop();self->closed();}
            }
        })){busy=false;dirty=true;}
    }
    void rebuild(){
        auto placements=array(object(geometry,L"placement"),L"columns"),models=array(model,L"columns");
        if(!placements.Size()||placements.Size()!=models.Size())return;
        bool splitFilters=false;
        for(auto members:models)for(auto member:members.GetArray())if(member.GetString()==L"filter_types")splitFilters=true;
        A signature;
        for(uint32_t i=0;i<models.Size();++i){
            double width=num(placements.GetObjectAt(i),L"width");A items;
            for(auto value:models.GetArrayAt(i)){
                auto found=panels.find(std::wstring(value.GetString()));if(found==panels.end())continue;
                auto panel=found->second;auto key=toolbarKey(panel,width);
                items.Append(O({{L"panel",panelStructure(panel)},{L"ready",B(!array(panel,L"tiles").Size()||(bodies.contains(std::wstring(value.GetString()))&&bodies.at(std::wstring(value.GetString())).key==panelStructure(panel).Stringify())||toolbarLayouts.contains(std::wstring(key)))}}));
            }
            signature.Append(items);
        }
        auto tabs=object(model,L"tabs"),tabStructure=J::Parse(tabs.Stringify());if(tabStructure.HasKey(L"active"))tabStructure.Remove(L"active");
        A headers,ids;for(auto const& id:panelTabIds(array(tabs,L"panels"))){
            ids.Append(S(id));
            auto panel=panels.find(std::wstring(id));if(panel==panels.end())continue;
            headers.Append(O({{L"title",S(str(panel->second,L"title"))},{L"icon",S(str(panel->second,L"icon"))}}));
        }
        if(tabStructure.Size())tabStructure.Insert(L"panels",ids);
        auto nextTabsKey=O({{L"tabs",tabStructure},{L"headers",headers},{L"style",S(str(object(data->model,L"windows_tab_styles"),to_hstring(uint32_t(num(tabs,L"group"))).c_str()))}}).Stringify();
        auto key=O({{L"columns",signature},{L"tabs",S(nextTabsKey)}}).Stringify();
        if(key==columnsKey){placeColumns();return;}
        columnsKey=key;
        if(inspector)inspector->pages.Children().Clear();
        if(!model.HasKey(L"layer_properties"))inspector.reset();
        auto previous=std::move(columns);columns.clear();
        for(auto& column:previous){
            column.stack.Children().Clear();column.scroll.Content(nullptr);column.panels.clear();
            if(tabsKey!=nextTabsKey){column.frame.Children().Clear();column.header=nullptr;column.tabs.clear();column.automatic.clear();column.paintIcons.clear();}
            else while(column.frame.Children().Size()>(column.header?1u:0u))column.frame.Children().RemoveAtEnd();
        }
        tabsKey=nextTabsKey;std::set<std::wstring> keep;auto weak=weak_from_this();
        for(uint32_t i=0;i<models.Size();++i){
            bool retained=i<previous.size();Column column=retained?std::move(previous[i]):Column{};double width=num(placements.GetObjectAt(i),L"width");
            if(!retained){
                RowDefinition tabRow;tabRow.Height({tabs.Size()?36.:0.,GridUnitType::Pixel});column.frame.RowDefinitions().Append(tabRow);
                RowDefinition bodyRow;bodyRow.Height({1,GridUnitType::Star});column.frame.RowDefinitions().Append(bodyRow);
                column.stack.Background(clear());column.scroll.ContentOrientation(ScrollingContentOrientation::Vertical);panelScrolling(column.scroll,column.stack,data);column.scroll.VerticalScrollMode(ScrollingScrollMode::Enabled);
                column.scroll.HorizontalScrollMode(ScrollingScrollMode::Disabled);column.scroll.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
                column.scroll.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);mouseScrollbars(column.scroll);
            }else column.frame.RowDefinitions().GetAt(0).Height({tabs.Size()?36.:0.,GridUnitType::Pixel});
            if(tabs.Size()&&!column.header){
                auto group=num(tabs,L"group");
                auto groupItem=O({{L"kind",S(L"group")},{L"group",N(group)}});
                Grid header;header.Background(clear());
                ColumnDefinition tabColumn;tabColumn.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(tabColumn);
                ColumnDefinition gripColumn;gripColumn.Width({28,GridUnitType::Pixel});header.ColumnDefinitions().Append(gripColumn);
                StackPanel row;row.Orientation(Orientation::Horizontal);row.Background(clear());
                uint32_t index=0;
                for(auto value:array(tabs,L"panels")){
                    auto panelId=value.GetString();auto found=panels.find(std::wstring(panelId));if(found==panels.end())continue;
                    auto panel=found->second;
                    auto pick=button(data,str(panel,L"title"),[data=data,group=num(tabs,L"group"),panelId]{
                        data->dispatch(O({{L"type",S(L"select_panel_tab")},{L"group",N(group)},{L"panel",S(panelId)}}));
                    });
                    pick.Height(36);pick.MinWidth(36);pick.Padding({8,4,8,4});pick.CornerRadius({6,6,0,0});
                    StackPanel labelRow;labelRow.Orientation(Orientation::Horizontal);labelRow.Spacing(6);
                    auto presentation=object(panel,L"tab");bool fitted=automaticTabs(data,group);
                    labelRow.Children().Append(panelIcon(data,str(panel,L"icon"),column.paintIcons));
                    {
                        auto name=label(data,str(panel,L"title"),true);name.Visibility(flag(presentation,L"show_name")?Visibility::Visible:Visibility::Collapsed);labelRow.Children().Append(name);
                        if(fitted)column.automatic.push_back({pick,name,std::wstring(panelId)+L"\n"+std::wstring(str(panel,L"title"))+L"\n"+std::to_wstring(data->textSize())});
                    }
                    pick.Content(labelRow);
                    AutomationProperties::SetAutomationId(pick,L"drawer-tab-"+panelId);
                    auto item=O({{L"kind",S(L"panel")},{L"panel",S(panelId)}});
                    gestures->Source(pick,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item,false,
                        O({{L"group",N(group)},{L"index",N(index++)},{L"panel",S(panelId)}}));
                    auto shell=panelTabShell(pick,nullptr,data->tint(L"text",20));column.tabs.emplace(std::wstring(panelId),shell);row.Children().Append(shell);
                }
                ScrollView strip;strip.Content(row);strip.Background(clear());
                strip.SizeChanged([weak,index=columns.size()](auto&& sender,auto&&){if(auto self=weak.lock();self&&index<self->columns.size())fitAutomaticTabs(self->data,self->columns[index].automatic,sender.template as<ScrollView>().ActualWidth(),self->tabWidths);});
                strip.HorizontalScrollMode(ScrollingScrollMode::Enabled);strip.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
                strip.VerticalScrollMode(ScrollingScrollMode::Disabled);strip.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
                strip.ContentOrientation(ScrollingContentOrientation::Horizontal);strip.IgnoredInputKinds(ScrollingInputKinds::Touch|ScrollingInputKinds::Pen);header.Children().Append(strip);
                gestures->Source(strip,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem,false,{},WorkspaceGestures::Pickup::Sideways);
                Border grip;grip.Background(clear());grip.Width(20);grip.Height(36);
                grip.HorizontalAlignment(HorizontalAlignment::Left);grip.Child(panelGrip(data->theme()));
                gestures->Source(grip,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem);
                AutomationProperties::SetAutomationId(grip,L"drawer-grip-"+to_hstring(uint32_t(group)));
                AutomationProperties::SetName(grip,data->delivery(L"move_group"));
                AutomationProperties::SetHelpText(grip,data->delivery(L"drag_panel"));
                Grid::SetColumn(grip,1);header.Children().Append(grip);
                column.header=header;column.frame.Children().Append(header);
            }
            column.stack.Spacing(6);column.stack.VerticalAlignment(VerticalAlignment::Top);
            for(auto value:models.GetArrayAt(i)){
                std::wstring panelId=value.GetString().c_str();auto found=panels.find(panelId);if(found==panels.end())continue;
                auto panel=found->second;auto bodyKey=toolbarKey(panel,width);auto toolbar=toolbarLayouts.find(std::wstring(bodyKey));
                if(array(panel,L"tiles").Size()&&toolbar==toolbarLayouts.end()&&(!bodies.contains(panelId)||bodies.at(panelId).key!=panelStructure(panel).Stringify()))continue;
                auto& body=bodies[panelId];keep.insert(panelId);column.panels.push_back(panelId);
                bodyKey=panelStructure(panel).Stringify();
                if(!body.view||body.key!=bodyKey||body.splitFilters!=splitFilters){
                    auto placement=O({{L"bounds",O({{L"width",N(width)}})}});
                    placement.Insert(L"split_filters",B(splitFilters));
                    if(toolbar!=toolbarLayouts.end())placement.Insert(L"tiles",toolbar->second);
                    body.view=std::make_unique<PanelBody>(data,panel,placement,[weak]{if(auto self=weak.lock())self->measured();},gestures,false,[weak]{if(auto self=weak.lock();self&&self->id==L"tool")return object(self->model,L"tool_set");return J{};});
                    body.key=bodyKey;body.splitFilters=splitFilters;
                    if(toolbar!=toolbarLayouts.end())body.view->Root().Height(std::max(36.,num(toolbar->second,L"content_height")));
                }
                AutomationProperties::SetAutomationId(body.view->Root(),L"drawer-panel-"+hstring(panelId));
                if(AutomationProperties::GetName(body.view->Root()).empty())
                    AutomationProperties::SetName(body.view->Root(),str(panel,L"title"));
                body.view->Root().Width(width);body.view->Apply(true);column.stack.Children().Append(body.view->Root());
            }
            AutomationProperties::SetAutomationId(column.scroll,hstring(L"drawer-slot-"+id+L"-"+std::to_wstring(i)));
            if(model.HasKey(L"layer_properties")){
                column.stack.Children().Clear();
                if(!inspector)initInspector();
                inspector->pages.Children().Append(bodies.at(L"layers").view->Root());
                inspector->pages.Children().Append(inspector->properties->Root());
                Grid::SetRow(inspector->root,1);column.frame.Children().Append(inspector->root);
            }else{
                column.scroll.Content(column.stack);
                Grid::SetRow(column.scroll,1);column.frame.Children().Append(column.scroll);
            }
            if(!retained)content.Children().Append(column.frame);columns.push_back(std::move(column));
        }
        for(uint32_t i=models.Size();i<previous.size();++i)remove(content,previous[i].frame);
        // Retain inactive tab bodies, including their native control state.
        for(auto& [panel,body]:bodies)if(body.view&&!keep.contains(panel))body.view->Apply(false);
        placeColumns();wake();
    }
    void placeColumns(){
        auto tabs=object(model,L"tabs");bool fitted=automaticTabs(data,num(tabs,L"group"));
        for(auto& column:columns)for(auto const& [id,shell]:column.tabs){
            auto tab=shell.Children().GetAt(1).as<Button>();
            auto panel=panels.find(id);if(panel==panels.end())continue;auto presentation=object(panel->second,L"tab");
            auto labels=tab.Content().as<StackPanel>();labels.Children().GetAt(0).Visibility(fitted||flag(presentation,L"show_icon")?Visibility::Visible:Visibility::Collapsed);
            if(!fitted)labels.Children().GetAt(1).Visibility(flag(presentation,L"show_name")?Visibility::Visible:Visibility::Collapsed);
            selectPanelTabShell(shell,hstring(id)==str(tabs,L"active"),nullptr,presentation);
        }
        auto placements=array(object(geometry,L"placement"),L"columns");
        for(uint32_t i=0;i<std::min(uint32_t(columns.size()),placements.Size());++i){
            auto placement=placements.GetObjectAt(i);auto& column=columns[i];place(column.frame,placement);
            double width=num(placement,L"width");
            for(auto const& panelId:column.panels){
                auto& body=bodies.at(panelId);auto panel=panels.at(panelId);body.view->Root().Width(width);
                auto toolbar=toolbarLayouts.find(std::wstring(toolbarKey(panel,width)));
                if(toolbar!=toolbarLayouts.end()){
                    body.view->Layout(O({{L"tiles",toolbar->second}}));
                    body.view->Root().Height(std::max(36.,num(toolbar->second,L"content_height")));
                }else if(panelId==L"layers"||panelId==L"adjustments"||body.view->navigator){
                    body.view->Root().Height(body.view->navigator?272.:480.);
                }else{
                    auto height=body.view->ContentHeight(width);
                    if(height>=0)body.view->Root().Height(height);
                }
                if(panelId==L"layers"){
                    auto inset=model.HasKey(L"layer_properties")?0.:data->panelMetric(L"inset");
                    body.view->Root().Margin({0,inset,0,inset});
                }
            }
            if(inspector&&model.HasKey(L"layer_properties")){
                inspector->place(bodies.at(L"layers").view->Root(),width,num(placement,L"height"),num(inspector->sizes,L"footer_height",49.),flag(model,L"layer_properties"));
            }
        }
    }

    void draw(){
        auto placement=object(geometry,L"placement");auto bounds=object(placement,L"bounds");
        frame.Visibility(bounds.Size()?Visibility::Visible:Visibility::Collapsed);
        shadow.Layout(rectangle(bounds),order(),bounds.Size()!=0);
        if(!bounds.Size()){presentedSource=J{};bridge.Visibility(Visibility::Collapsed);return;}
        place(frame,bounds);
        RectangleGeometry clip;clip.Rect({0,0,float(num(bounds,L"width")),float(num(bounds,L"height"))});content.Clip(clip);
        auto connection=object(geometry,L"connection");
        bridge.Visibility(connection.Size()?Visibility::Visible:Visibility::Collapsed);
        if(connection.Size()){place(bridge,object(connection,L"bounds"));bridge.Data(drawerBridge(connection));}
        // A local, inspectable geometry record; no documents or profile paths.
        AutomationProperties::SetItemStatus(frame,geometry.Stringify());
        paintBackground();
        auto anchor=object(model,L"anchor");
        presentedSource=anchor.Size()&&connection.Size()?O({{L"anchor",anchor},{L"direction",S(str(placement,L"direction"))},{L"bounds",object(placement,L"anchor")}}):J{};
    }
    bool measureHeights(){
        auto placements=array(object(geometry,L"placement"),L"columns");
        if(columns.empty()||columns.size()!=placements.Size()||columns.size()!=array(model,L"columns").Size()||inspector)return false;
        double tabHeight=object(model,L"tabs").Size()?36.:0.;
        std::vector<double> next;
        for(uint32_t i=0;i<columns.size();++i){
            auto width=num(placements.GetObjectAt(i),L"width");
            if(width<=0)return false;
            columns[i].stack.Measure({float(width),INFINITY});
            next.push_back(columns[i].stack.DesiredSize().Height+tabHeight);
        }
        bool differs=next.size()!=heights.size();
        if(!differs)for(size_t i=0;i<next.size();++i)if(std::abs(next[i]-heights[i])>.5){differs=true;break;}
        if(differs)heights=std::move(next);
        return differs;
    }
    void measured(){
        // A mode switch can await its first placement with old native slots
        // still arranged. Never report those heights against the new model.
        if(disposed||layingOut||!frame.IsLoaded()||columns.empty()||columns.size()!=array(model,L"columns").Size())return;
        layingOut=true;
        if(inspector&&model.HasKey(L"layer_properties")){
            if(!inspector->animating&&inspector->position==0&&inspector->anchor.Size())wake();
            if(inspectorSizingKey()!=inspector->sizingKey)wake();
            paintBackground();changed();layingOut=false;return;
        }
        if(measureHeights())wake();
        paintBackground();changed();layingOut=false;
    }
    void initInspector(){
        inspector=std::make_unique<LayerInspector>(data,panels.at(L"properties"),gestures,
            [weak=weak_from_this()]{if(auto self=weak.lock())self->wake();},
            [data=data]{data->dispatch(O({{L"type",S(L"customize")},{L"action",O({{L"type",S(L"toggle_layer_drawer_properties")}})}}));});
        inspector->position=flag(model,L"layer_properties")?1.:0.;refreshInspector();
    }
    void refreshInspector(){inspector->refresh(flag(model,L"layer_properties"));}
    J inspectorSizingRequest()const{
        auto layer=bodies.find(L"layers");if(!inspector||layer==bodies.end()||!layer->second.view)return J{};
        return inspector->sizingRequest(*layer->second.view);
    }
    hstring inspectorSizingKey(J const& request)const{return request.Stringify()+L":"+to_hstring(array(data->state,L"layers").Size())+L":"+layoutKey+L":"+object(model,L"anchor").Stringify();}
    hstring inspectorSizingKey()const{return inspectorSizingKey(inspectorSizingRequest());}
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
        std::array<float,4> radii{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
        for(uint32_t i=0;i<std::min(4u,corners.Size());++i)if(corners.GetBooleanAt(i))radii[i]=0;
        frame.CornerRadius({radii[0]*CornerFit,radii[1]*CornerFit,radii[2]*CornerFit,radii[3]*CornerFit});
        auto outline=squircleRectangle(width,height,radii);
        auto link=object(object(geometry,L"connection"),L"bounds");
        auto silhouette=O({{L"width",N(width)},{L"height",N(height)},{L"corners",corners},{L"link",link},{L"origin",bounds}}).Stringify();
        if(silhouette!=shadowKey){
            shadowKey=silhouette;shadow.Shape(squircleRectangle(width,height,radii),width,height,21,5,1.f/3);
            std::vector<Rect> bridgeArea;
            if(link.Size())bridgeArea.push_back({float(num(link,L"x")-num(bounds,L"x")),float(num(link,L"y")-num(bounds,L"y")),float(num(link,L"width")),float(num(link,L"height"))});
            shadow.Cut(radii,std::move(bridgeArea));
        }
        GeometryGroup shape;shape.FillRule(FillRule::EvenOdd);shape.Children().Append(outline);
        for(auto hole:holes){RectangleGeometry region;region.Rect(rectangle(hole.GetObject()));shape.Children().Append(region);}
        background.Data(shape);
    }
    void collectGlass(A& regions,A& links,UIElement const& reference)const{
        auto design=cornerRadii(frame.CornerRadius());for(auto& radius:design)radius/=CornerFit;
        if(disposed||!appendGlass(regions,frame,reference,design,true))return;
        appendConnection(links,object(geometry,L"connection"),workspace,reference);
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
    std::optional<uint64_t> context;
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
        auto next=uint64_t(num(data->state,L"toolbar_context_generation"));
        if(context!=next){context=next;reset();}
        auto customization=object(data->state,L"customization");std::map<std::wstring,J> models;
        if(!flag(data->model,L"chrome_hidden"))for(auto value:array(customization,L"column_drawers")){
            auto model=value.GetObject();models[std::to_wstring(uint32_t(num(object(model,L"anchor"),L"column")))]=model;
        }
        auto tool=object(customization,L"drawer");if(tool.Size())models[L"tool"]=tool;
        if(models.empty()&&drawers.empty()&&!lastFacts.empty())return;
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
A WorkspaceDrawers::Sources()const{
    A result;
    for(auto const& [id,drawer]:impl->drawers){
        if(!drawer->disposed&&drawer->presentedSource.Size())result.Append(drawer->presentedSource);
    }
    return result;
}
void WorkspaceDrawers::AppendGlass(A& regions,A& connections,UIElement const& reference)const{for(auto const& [id,drawer]:impl->drawers)drawer->collectGlass(regions,connections,reference);}
FrameworkElement WorkspaceDrawers::Anchor(std::wstring const& control)const{
    for(auto const& [id,drawer]:impl->drawers)if(!drawer->closing)for(auto const& column:drawer->columns)for(auto const& panel:column.panels){
        auto const& body=drawer->bodies.at(panel).view;if(!body)continue;
        if(auto found=body->anchors.find(control);found!=body->anchors.end())return found->second;
    }
    return nullptr;
}

A WorkspaceDrawers::PanelMeasurements()const{
    A result;
    for(auto const& [id,drawer]:impl->drawers)if(!drawer->closing&&!drawer->disposed&&drawer->frame.IsLoaded())
        for(auto const& column:drawer->columns)for(auto const& panel:column.panels){
            auto const& body=drawer->bodies.at(panel).view;if(!body||!body->Root().IsLoaded())continue;
            auto height=body->ContentHeight();if(!std::isfinite(height)||height<0)continue;
            auto scroll=body->ScrollMetrics();result.Append(O({{L"panel",S(hstring(panel))},
                {L"content_height",N(height)},{L"scroll",scroll.Size()?V(scroll):JsonValue::CreateNullValue()}}));
        }
    return result;
}
