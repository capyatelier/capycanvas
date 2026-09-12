#include "pch.h"
#include "CollapsedColumns.h"
#include "WorkspaceGeometry.h"
#include <set>

using namespace CapyUi;
namespace {
struct Column:std::enable_shared_from_this<Column>{
    std::shared_ptr<WorkspaceData> data;
    std::shared_ptr<WorkspaceGestures> gestures;
    Canvas workspace{nullptr},frame,icons;
    Border background,grip;
    ScrollViewer scroll;
    Button expand{nullptr};
    uint32_t id=0;
    J geometry;
    std::map<std::wstring,Button> buttons;
    hstring structure;
    double reported=0;
    bool applying=false;
    void init(){
        auto weak=weak_from_this();
        background.Background(data->brush(L"panel"));background.CornerRadius({8,8,8,8});background.Child(frame);
        Canvas::SetZIndex(background,160);
        AutomationProperties::SetAutomationId(background,L"collapsed-column-"+to_hstring(id));
        expand=button(data,L"Expand column",[data=data,id=id]{
            data->dispatch(O({{L"type",S(L"customize")},{L"action",O({{L"type",S(L"set_column_collapsed")},
                {L"group",N(id)},{L"collapsed",B(false)}})}}));
        });
        expand.Padding({0,0,0,0});expand.Content(icon(L"column-expand",data->theme()));
        AutomationProperties::SetAutomationId(expand,L"expand-column-"+to_hstring(id));
        frame.Children().Append(expand);
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
        grip.Background(clear());grip.Child(panelGrip(data->theme()));
        auto item=O({{L"kind",S(L"column")},{L"column",N(id)}});
        gestures->Source(grip,O({{L"type",S(L"drag_workspace")},{L"item",item}}));
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
        place(expand,local(object(geometry,L"expand"),bounds));place(grip,local(object(geometry,L"grip"),bounds));place(scroll,local(content,bounds));
        double offset=0;
        for(auto entry:array(object(object(data->state,L"workspace"),L"layout"),L"column_scroll")){
            auto pair=entry.GetArray();if(pair.Size()==2&&pair.GetNumberAt(0)==id)offset=pair.GetNumberAt(1);
        }
        std::set<std::wstring> current;double bottom=num(content,L"height");
        for(auto value:array(geometry,L"groups")){
            auto group=value.GetObject();auto groupBounds=object(group,L"bounds");
            bottom=std::max(bottom,num(groupBounds,L"y")+num(groupBounds,L"height")-num(content,L"y")+offset);
            for(auto iconValue:array(group,L"icons")){
                auto tile=iconValue.GetObject();auto panelId=str(tile,L"panel");std::wstring key=panelId.c_str();current.insert(key);
                auto panel=find(array(data->model,L"panels"),L"id",panelId);
                auto found=buttons.find(key);
                if(found==buttons.end()){
                    auto pick=button(data,str(panel,L"title"),[weak=weak_from_this(),panelId]{
                        if(auto self=weak.lock())for(auto value:array(self->geometry,L"groups")){
                            auto group=value.GetObject();
                            for(auto iconValue:array(group,L"icons"))if(str(iconValue.GetObject(),L"panel")==panelId){
                                self->data->dispatch(O({{L"type",S(L"customize")},{L"action",O({{L"type",S(L"toggle_column_drawer")},
                                    {L"group",N(num(group,L"group"))},{L"panel",S(panelId)}})}}));return;
                            }
                        }
                    });
                    pick.Padding({0,0,0,0});pick.Content(icon(str(panel,L"icon"),data->theme()));
                    auto target=O({{L"kind",S(L"panel")},{L"panel",S(panelId)}});
                    gestures->Source(pick,J{},target);
                    AutomationProperties::SetAutomationId(pick,L"column-icon-"+panelId);
                    ToolTipService::SetToolTip(pick,box_value(str(panel,L"title")));
                    icons.Children().Append(pick);found=buttons.emplace(key,pick).first;
                }
                auto pick=found->second;place(pick,local(object(tile,L"bounds"),content,offset));
                AutomationProperties::SetName(pick,str(panel,L"title"));ToolTipService::SetToolTip(pick,box_value(str(panel,L"title")));
                pick.Background(str(group,L"active")==panelId?selected():clear());
            }
        }
        for(auto it=buttons.begin();it!=buttons.end();)if(!current.contains(it->first)){
            uint32_t index;if(icons.Children().IndexOf(it->second,index))icons.Children().RemoveAt(index);it=buttons.erase(it);
        }else ++it;
        icons.Width(num(content,L"width"));icons.Height(bottom);
        reported=offset;
        if(std::abs(scroll.VerticalOffset()-offset)>.25)scroll.ChangeView(nullptr,offset,nullptr,true);
        applying=false;
    }
    void remove(){
        uint32_t index;if(workspace.Children().IndexOf(background,index))workspace.Children().RemoveAt(index);
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
