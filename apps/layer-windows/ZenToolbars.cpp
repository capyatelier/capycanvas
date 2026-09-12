#include "pch.h"
#include "ZenToolbars.h"
#include "WorkspaceGeometry.h"
using namespace CapyUi;
ZenToolbars::ZenToolbars(std::shared_ptr<WorkspaceData> data,Canvas const& root,std::shared_ptr<WorkspaceGestures> gestures):
    data(std::move(data)),root(root),gestures(std::move(gestures)){}
void ZenToolbars::Reset(){
    for(auto const& [key,section]:sections){uint32_t index;if(root.Children().IndexOf(section.frame,index))root.Children().RemoveAt(index);}
    sections.clear();
}
void ZenToolbars::Apply(){
    std::vector<std::wstring> visible;
    if(flag(data->model,L"partial_zen"))for(auto value:array(object(data->model,L"zen_toolbars"),L"sections")){
        auto section=value.GetObject();auto panelId=str(section,L"panel");A tiles,rects;
        auto panel=J::Parse(find(array(data->model,L"panels"),L"id",panelId).Stringify());
        for(auto pairValue:array(section,L"tiles")){
            auto pair=pairValue.GetArray();
            if(pair.Size()!=2)continue;
            auto tile=findId(array(panel,L"tiles"),pair.GetNumberAt(0));
            if(!tile.Size())continue;
            tiles.Append(tile);rects.Append(pair.GetObjectAt(1));
        }
        if(!tiles.Size())continue;
        std::wstring key=std::wstring(panelId)+L"-"+std::to_wstring(uint32_t(num(tiles.GetObjectAt(0),L"id")));
        visible.push_back(key);
        panel.Insert(L"tiles",tiles);panel.Insert(L"tile_style",S(str(section,L"style")));
        auto geometry=O({{L"bounds",object(section,L"bounds")},{L"tiles",O({{L"tiles",rects}})}});
        auto [it,added]=sections.try_emplace(key);auto& native=it->second;
        if(added){
            native.frame.Background(data->brush(L"panel"));native.frame.CornerRadius({6,6,6,6});
            Canvas::SetZIndex(native.frame,150);root.Children().Append(native.frame);
            AutomationProperties::SetAutomationId(native.frame,L"zen-section-"+hstring(key));
        }
        auto signature=panelStructure(panel).Stringify();
        if(signature!=native.signature){
            native.signature=signature;
            native.body=std::make_unique<PanelBody>(data,panel,geometry,[] {},gestures);
            native.frame.Child(native.body->Root());
            for(auto const& [id,element]:native.body->tileElements)
                AutomationProperties::SetAutomationId(element,L"zen-tile-"+panelId+L"-"+to_hstring(id));
        }
        place(native.frame,object(section,L"bounds"));
        native.body->Layout(geometry);native.body->Apply(true);
        AutomationProperties::SetItemStatus(native.frame,section.Stringify());
    }
    for(auto it=sections.begin();it!=sections.end();){
        if(std::find(visible.begin(),visible.end(),it->first)==visible.end()){
            uint32_t index;if(root.Children().IndexOf(it->second.frame,index))root.Children().RemoveAt(index);
            it=sections.erase(it);
        }else ++it;
    }
}
FrameworkElement ZenToolbars::Anchor(std::wstring const& control)const{
    for(auto const& [key,section]:sections){
        auto found=section.body->anchors.find(control);
        if(found!=section.body->anchors.end())return found->second;
    }
    return nullptr;
}
