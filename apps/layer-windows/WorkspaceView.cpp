#include "pch.h"
#include "WorkspaceView.h"
#include "PanelBody.h"
#include "WorkspaceDrawers.h"
#include "CollapsedColumns.h"
#include "WorkspaceGestures.h"
#include "ColorView.h"
#include "ToolView.h"
#include "NavigatorView.h"
#include "EffectControls.h"
#include "LayersView.h"
#include "NativeMenus.h"
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
struct WorkspaceView::Impl : std::enable_shared_from_this<Impl> {
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    Canvas root;
    std::shared_ptr<WorkspaceGestures> gestures;
    std::unique_ptr<WorkspaceDrawers> drawers;
    std::unique_ptr<CollapsedColumns> collapsed;
    std::map<std::wstring,Border> handles;
    Dispatch overviews;
    hstring lastOverviews;
    struct Group {Border border;std::wstring key;std::unique_ptr<PanelBody> body;};
    std::map<uint32_t,Group> groups;
    hstring previousTheme,previousPalette;
    Flyout popup{nullptr};
    std::wstring popupControl;
    std::shared_ptr<uint64_t> popupGeneration=std::make_shared<uint64_t>(0);
    Bindings popupBindings;
    TextBlock camera;
    Impl(Dispatch send,J catalog,Dispatch report,PreviewTransport previews,std::function<void(bool)> popupChanged,Dispatch document,Dispatch input):overviews(std::move(report)){
        data->input=std::move(input);gestures=std::make_shared<WorkspaceGestures>(data,root);
        data->document=std::move(document);
        data->popupChanged=std::move(popupChanged);
        data->thumbnails=CreateLayerThumbnailCache(previews);
        data->query=previews;data->previews=CreateFilterPreviewCache(std::move(previews));
        data->send=std::move(send);data->catalog=catalog;
        AutomationProperties::SetName(root,L"Drawing workspace");
        camera.FontSize(num(catalog,L"text_size_pt",11)*96./72.);
        camera.IsHitTestVisible(false);
        AutomationProperties::SetAutomationId(camera,L"canvas-camera");
    }
    void init(){
        collapsed=std::make_unique<CollapsedColumns>(data,root,gestures);
        drawers=std::make_unique<WorkspaceDrawers>(data,root,gestures,[weak=weak_from_this()]{if(auto self=weak.lock())self->publishOverviews();});
    }
    void build(Group& group,J const& geometry,J const& panel){
        group.body.reset();
        auto groupItem=O({{L"kind",S(L"group")},{L"group",N(num(geometry,L"id"))}});
        group.border.Background(data->brush(L"panel"));group.border.CornerRadius(CornerRadius{8,8,8,8});
        Grid frame;
        RowDefinition tabRow;tabRow.Height({flag(geometry,L"tabs_visible")?36.:0.,GridUnitType::Pixel});
        frame.RowDefinitions().Append(tabRow);
        RowDefinition bodyRow;bodyRow.Height({1,GridUnitType::Star});frame.RowDefinitions().Append(bodyRow);
        if(flag(geometry,L"tabs_visible")){
            StackPanel tabs;tabs.Orientation(Orientation::Horizontal);tabs.Background(data->brush(L"tabbar"));
            uint32_t index=0;
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
                auto item=O({{L"kind",S(L"panel")},{L"panel",S(id)}});
                gestures->Source(tab,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item,false,
                    O({{L"group",N(num(geometry,L"id"))},{L"index",N(index++)},{L"panel",S(id)}}));
                AutomationProperties::SetAutomationId(tab,L"panel-tab-"+id);
                tabs.Children().Append(tab);
            }
            ScrollViewer tabScroll;tabScroll.Content(tabs);tabScroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Hidden);
            tabScroll.HorizontalScrollMode(ScrollMode::Enabled);tabScroll.VerticalScrollMode(ScrollMode::Disabled);
            tabScroll.Background(data->brush(L"tabbar"));
            gestures->Source(tabScroll,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem,true);
            frame.Children().Append(tabScroll);
        }
        group.body=std::make_unique<PanelBody>(data,panel,geometry,
            [weak=weak_from_this()]{if(auto self=weak.lock())self->publishOverviews();},gestures);
        auto body=group.body->Root();Grid::SetRow(body,1);frame.Children().Append(body);
        if(group.body->navigator)group.border.Background(clear());
        auto grip=object(geometry,L"footer_grip");
        if(grip.Size()){
            Canvas overlay;Grid::SetRow(overlay,1);
            Border handle;handle.Background(clear());place(handle,grip);
            Border mark;mark.Width(16);mark.Height(2);mark.Background(data->brush(L"settings_secondary"));mark.Opacity(.4);
            mark.HorizontalAlignment(HorizontalAlignment::Center);mark.VerticalAlignment(VerticalAlignment::Center);handle.Child(mark);
            auto item=O({{L"kind",S(L"panel")},{L"panel",S(str(panel,L"id"))}});
            gestures->Source(handle,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item,true);
            AutomationProperties::SetName(handle,L"Move "+str(panel,L"title"));
            overlay.Children().Append(handle);frame.Children().Append(overlay);
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
            FrameworkElement anchor{nullptr};
            for(auto const& [id,group]:groups){
                if(group.border.Visibility()!=Visibility::Visible||!group.body)continue;
                auto found=group.body->anchors.find(next);
                if(found!=group.body->anchors.end()){anchor=found->second;break;}
            }
            if(!anchor&&drawers)anchor=drawers->Anchor(next);
            if(!anchor)return;
            StackPanel content;content.Width(280);content.Spacing(12);
            content.Children().Append(label(data,next==L"brush_color"?L"Brush color":L"Brush opacity",true));
            if(next==L"brush_opacity"){
                content.Children().Append(number(data,L"Opacity",object(data->catalog,L"opacity"),
                    [data=data]{return num(object(data->state,L"brush"),L"opacity");},
                    [data=data](double value){data->dispatch(O({{L"type",S(L"set_brush_opacity")},{L"value",N(value)}}));},popupBindings));
            }else if(next==L"brush_color"){
                content.Children().Append(ColorPanel(data,popupBindings));
            }
            popup=Flyout();popup.Content(content);TrackPopup(popup,data);
            popup.Closed([data=data,generation=popupGeneration,current=*popupGeneration](auto&&,auto&&){
                if(*generation==current)data->dispatch(O({{L"type",S(L"customize")},
                    {L"action",O({{L"type",S(L"close_control")}})}}));
            });
            for(auto const& bind:popupBindings)bind();
            popup.ShowAt(anchor);
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
            drawers->Reset();collapsed->Reset();root.Children().Clear();groups.clear();handles.clear();previousTheme=theme;previousPalette=palette;root.Children().Append(camera);
        }
        root.RequestedTheme(theme==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        auto layout=object(snapshot,L"layout");
        std::vector<uint32_t> visible;int order=0;
        std::vector<std::wstring> handleIds;
        for(auto value:array(layout,L"groups")){
            auto geometry=value.GetObject();uint32_t id=uint32_t(num(geometry,L"id"));visible.push_back(id);
            auto panel=find(array(snapshot,L"panels"),L"id",str(geometry,L"active"));
            auto [it,added]=groups.try_emplace(id);auto& group=it->second;
            if(added)root.Children().Append(group.border);
            // Geometry and structure may rebuild this group. Value-only updates
            // below keep its native focus, slider capture and scroll position.
            auto structure=J::Parse(geometry.Stringify());
            if(structure.HasKey(L"resize_handles"))structure.Remove(L"resize_handles");
            auto size=object(structure,L"bounds");size.Remove(L"x");size.Remove(L"y");
            structure.Insert(L"bounds",size);
            if(str(panel,L"id")==L"navigator"||str(panel,L"id")==L"properties"||str(panel,L"id")==L"adjustments"||str(panel,L"id")==L"layers")structure.Remove(L"bounds");
            J signature=O({{L"geometry",structure},{L"controls",array(panel,L"controls")},
                {L"style",S(str(panel,L"tile_style"))}});
            A tileKeys;for(auto item:array(panel,L"tiles")){
                auto tile=item.GetObject();tileKeys.Append(O({{L"id",N(num(tile,L"id"))},{L"control",object(tile,L"control")}}));
            }signature.Insert(L"tiles",tileKeys);
            A headers;for(auto member:array(geometry,L"panels")){
                auto model=find(array(snapshot,L"panels"),L"id",member.GetString());
                headers.Append(O({{L"title",S(str(model,L"title"))},{L"icon",S(str(model,L"icon"))},{L"tab",object(model,L"tab")}}));
            }signature.Insert(L"headers",headers);
            std::wstring key=signature.Stringify().c_str();
            if(group.key!=key){group.key=std::move(key);build(group,geometry,panel);}
            place(group.border,object(geometry,L"bounds"));
            bool hidden=flag(snapshot,L"chrome_hidden")&&(!flag(geometry,L"floating")||flag(snapshot,L"hide_floating_panels"));
            group.border.Visibility(hidden?Visibility::Collapsed:Visibility::Visible);
            int z=flag(geometry,L"floating")?100+2*order:0;++order;
            Canvas::SetZIndex(group.border,z);
            if(!hidden)for(auto handleValue:array(geometry,L"resize_handles")){
                auto handle=handleValue.GetObject();auto handleKey=L"floating-"+std::to_wstring(id)+L"-"+std::wstring(str(handle,L"edge"));
                resizeHandle(handleKey,object(handle,L"bounds"),O({{L"type",S(L"resize_floating")},
                    {L"group",N(id)},{L"edge",S(str(handle,L"edge"))}}),z+1);handleIds.push_back(handleKey);
            }
            group.body->Apply(!hidden);
        }
        for(auto it=groups.begin();it!=groups.end();){
            if(std::find(visible.begin(),visible.end(),it->first)==visible.end()){
                uint32_t index;if(root.Children().IndexOf(it->second.border,index))root.Children().RemoveAt(index);
                it=groups.erase(it);
            }else ++it;
        }
        if(!flag(snapshot,L"chrome_hidden"))for(auto value:array(layout,L"dividers")){
            auto divider=value.GetObject();auto key=L"divider-"+std::to_wstring(uint32_t(num(divider,L"id")));
            resizeHandle(key,object(divider,L"bounds"),O({{L"type",S(L"drag_divider")},{L"id",N(num(divider,L"id"))}}),10,
                flag(divider,L"band")&&str(divider,L"axis")==L"horizontal");
            handleIds.push_back(key);
        }
        for(auto it=handles.begin();it!=handles.end();){
            if(std::find(handleIds.begin(),handleIds.end(),it->first)==handleIds.end()){
                uint32_t index;if(root.Children().IndexOf(it->second,index))root.Children().RemoveAt(index);
                it=handles.erase(it);
            }else ++it;
        }
        collapsed->Apply();drawers->Apply();gestures->Refresh();
        camera.Foreground(data->brush(L"text"));place(camera,object(layout,L"status"));
        camera.TextAlignment(TextAlignment::Right);updateCamera(object(data->state,L"camera"));
        updatePopup();publishOverviews();
    }
    void resizeHandle(std::wstring const& key,J const& bounds,J const& action,int z,bool resetColumn=false){
        auto [it,added]=handles.try_emplace(key);
        auto handle=it->second;
        if(added){handle.Background(clear());root.Children().Append(handle);}
        place(handle,bounds);Canvas::SetZIndex(handle,z);gestures->Source(handle,action,{},resetColumn);
        AutomationProperties::SetAutomationId(handle,hstring(key));AutomationProperties::SetName(handle,L"Resize panel");
        AutomationProperties::SetHelpText(handle,resetColumn?L"Drag to resize the column. Double-click to restore its default width.":L"Drag to resize the panel.");
    }
    void publishOverviews(){
        A slots;
        Windows::Foundation::Rect clip{0,0,float(root.ActualWidth()),float(root.ActualHeight())};
        for(auto const& [id,group]:groups){
            if(!group.body||!group.body->navigator||group.border.Visibility()!=Visibility::Visible)continue;
            auto slot=group.body->navigator->Placement(root,clip,Canvas::GetZIndex(group.border));
            if(slot.Size())slots.Append(slot);
        }
        if(drawers)drawers->AppendOverviews(slots);
        auto json=slots.Stringify();
        if(json!=lastOverviews){lastOverviews=json;overviews(to_string(json));}
    }
    void updateCamera(J const& view){
        if(view.Size())camera.Text(to_hstring(int(std::round(num(view,L"zoom",1)*100)))+L"% · "+
            to_hstring(int(std::round(num(view,L"rotation")*180/3.141592653589793)))+L"°");
    }
};
WorkspaceView::WorkspaceView(Dispatch send,Json catalog,Dispatch overviews,PreviewTransport previews,std::function<void(bool)> popupChanged,Dispatch document,Dispatch input):impl(std::make_shared<Impl>(std::move(send),catalog,std::move(overviews),std::move(previews),std::move(popupChanged),std::move(document),std::move(input))){impl->init();}
WorkspaceView::~WorkspaceView()=default;
Canvas WorkspaceView::Root()const{return impl->root;}
void WorkspaceView::Apply(Json const& snapshot){impl->apply(snapshot);}
WorkspaceView::Json WorkspaceView::ChromeFacts(bool popupOpen){impl->data->externalPopup=popupOpen;return J::Parse(impl->data->chrome.Stringify());}
bool WorkspaceView::CancelGesture(){return impl->gestures->Cancel();}
