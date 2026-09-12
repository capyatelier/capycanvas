#include "pch.h"
#include "WorkspaceView.h"
#include "PanelBody.h"
#include "PanelConfiguration.h"
#include "WorkspaceExpansion.h"
#include "ZenToolbars.h"
#include "WorkspaceGeometry.h"
#include "OverviewOcclusion.h"
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
    std::unique_ptr<WorkspaceExpansion> expansion;
    std::unique_ptr<ZenToolbars> zen;
    struct Measurement {double tab=36,content=320;};
    std::map<std::wstring,Measurement> measurements;
    hstring lastMeasurements;
    bool measurementQueued=false;
    std::array<float,3> titlebar{};
    hstring lastTitlebar;
    bool presenting=false;
    OverviewOcclusion overviewOcclusion;
    std::map<std::wstring,Border> handles;
    Dispatch overviews;
    hstring lastOverviews;
    struct Group {
        Border frame,border,configurationFrame;
        Canvas content;
        Grid layout{nullptr},header{nullptr};
        StackPanel tabLabels{nullptr};
        ScrollViewer tabScroll{nullptr};
        Microsoft::UI::Xaml::Shapes::Path background;
        Border footer{nullptr};
        std::wstring key;
        J geometry,presented;
        hstring backgroundKey;
        std::unique_ptr<PanelBody> body;
        std::map<std::wstring,Button> tabs;
        std::unique_ptr<PanelConfiguration> configuration;
        bool hidden=false;
        int order=0;
    };
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
        zen=std::make_unique<ZenToolbars>(data,root,gestures);
        collapsed=std::make_unique<CollapsedColumns>(data,root,gestures);
        drawers=std::make_unique<WorkspaceDrawers>(data,root,gestures,[weak=weak_from_this()]{if(auto self=weak.lock())self->publishOverviews();});
        expansion=std::make_unique<WorkspaceExpansion>(data,root,gestures,[weak=weak_from_this()]{if(auto self=weak.lock())self->present();});
        root.LayoutUpdated([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->measured();});
    }
    void build(Group& group,J const& geometry,J const& panel){
        group.body.reset();group.footer=nullptr;group.tabs.clear();
        auto groupItem=O({{L"kind",S(L"group")},{L"group",N(num(geometry,L"id"))}});
        group.border.Background(data->brush(L"panel"));group.border.CornerRadius(CornerRadius{8,8,8,8});
        if(!group.layout){
            group.layout=Grid();
            group.layout.RowDefinitions().Append(RowDefinition());
            RowDefinition bodyRow;bodyRow.Height({1,GridUnitType::Star});group.layout.RowDefinitions().Append(bodyRow);
            group.border.Child(group.layout);
            group.tabLabels=StackPanel();group.tabLabels.Orientation(Orientation::Horizontal);
            group.tabLabels.Background(data->brush(L"tabbar"));
        }
        auto frame=group.layout;
        frame.RowDefinitions().GetAt(0).Height({flag(geometry,L"tabs_visible")?36.:0.,GridUnitType::Pixel});
        // Keep the header ScrollViewer and its manipulation content alive while
        // a panel changes group; only the body and tab buttons are replaced.
        while(frame.Children().Size()>1)frame.Children().RemoveAtEnd();
        {
            auto tabs=group.tabLabels;tabs.Children().Clear();
            uint32_t index=0;
            for(auto value:array(geometry,L"panels")){
                auto id=value.GetString();auto model=find(array(data->model,L"panels"),L"id",id);
                auto tab=button(data,str(model,L"title"),[data=data,id,groupId=num(geometry,L"id")]{
                    data->dispatch(O({{L"type",S(L"select_panel_tab")},{L"group",N(groupId)},{L"panel",S(id)}}));
                });tab.Height(36);tab.MinWidth(36);tab.Padding(Thickness{8,4,8,4});tab.CornerRadius(CornerRadius{6,6,0,0});
                StackPanel content;content.Orientation(Orientation::Horizontal);content.Spacing(6);
                auto tabStyle=object(model,L"tab");
                if(flag(tabStyle,L"show_icon"))content.Children().Append(icon(str(model,L"icon"),data->theme()));
                if(flag(tabStyle,L"show_name"))content.Children().Append(label(data,str(model,L"title"),true));
                tab.Content(content);bool active=id==str(geometry,L"active");
                auto item=O({{L"kind",S(L"panel")},{L"panel",S(id)}});
                gestures->Source(tab,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item,false,
                    O({{L"group",N(num(geometry,L"id"))},{L"index",N(index++)},{L"panel",S(id)}}));
                AutomationProperties::SetAutomationId(tab,L"panel-tab-"+id);
                group.tabs.emplace(std::wstring(id),tab);tabs.Children().Append(panelTabShell(data,tab,active));
            }
            if(!group.tabScroll){
                group.tabScroll=ScrollViewer();auto tabScroll=group.tabScroll;tabScroll.Content(tabs);tabScroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Hidden);
                tabScroll.HorizontalScrollMode(ScrollMode::Enabled);tabScroll.VerticalScrollMode(ScrollMode::Disabled);
                tabScroll.Background(data->brush(L"tabbar"));
                gestures->Source(tabScroll,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem,true);
                group.header=Grid();auto header=group.header;header.Background(data->brush(L"tabbar"));
                ColumnDefinition labels;labels.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(labels);
                ColumnDefinition trailing;trailing.Width({28,GridUnitType::Pixel});header.ColumnDefinitions().Append(trailing);
                header.Children().Append(tabScroll);
                Border handle;handle.Background(clear());handle.Width(20);handle.Height(36);
                handle.HorizontalAlignment(HorizontalAlignment::Left);handle.Child(panelGrip(data->theme()));
                gestures->Source(handle,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem,true);
                AutomationProperties::SetAutomationId(handle,L"group-grip-"+to_hstring(uint32_t(num(geometry,L"id"))));
                AutomationProperties::SetName(handle,L"Move panel group");
                AutomationProperties::SetHelpText(handle,L"Drag to move every panel in this group.");
                Grid::SetColumn(handle,1);header.Children().Append(handle);frame.Children().Append(header);
            }
            group.header.Visibility(flag(geometry,L"tabs_visible")?Visibility::Visible:Visibility::Collapsed);
        }
        group.body=std::make_unique<PanelBody>(data,panel,geometry,
            [weak=weak_from_this()]{if(auto self=weak.lock())self->measured();},gestures);
        auto body=group.body->Root();Grid::SetRow(body,1);frame.Children().Append(body);
        if(group.body->navigator)group.border.Background(clear());
        auto grip=object(geometry,L"footer_grip");
        if(grip.Size()){
            Canvas overlay;Grid::SetRow(overlay,1);
            Border handle;group.footer=handle;handle.Background(clear());place(handle,grip);
            handle.Child(panelGrip(data->theme(),true));
            auto item=O({{L"kind",S(L"panel")},{L"panel",S(str(panel,L"id"))}});
            gestures->Source(handle,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item,true);
            AutomationProperties::SetName(handle,L"Move "+str(panel,L"title"));
            overlay.Children().Append(handle);frame.Children().Append(overlay);
        }

    }
    void updatePopup(){
        auto control=str(object(data->state,L"customization"),L"control");
        std::wstring next=control.c_str();
        if(next!=popupControl||(!next.empty()&&!popup)){
            ++*popupGeneration;
            if(popup)popup.Hide();
            popup=nullptr;popupBindings.clear();popupControl=next;
            FrameworkElement anchor{nullptr};
            for(auto const& [id,group]:groups)if(group.presented.Size()&&group.configuration&&!group.hidden){
                anchor=group.configuration->Anchor(next);if(anchor)break;
            }
            for(auto const& [id,group]:groups){
                if(anchor)break;
                if(group.hidden||!group.body)continue;
                auto found=group.body->anchors.find(next);
                if(found!=group.body->anchors.end()){anchor=found->second;break;}
            }
            if(!anchor&&zen)anchor=zen->Anchor(next);
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
            expansion->Reset();zen->Reset();drawers->Reset();collapsed->Reset();root.Children().Clear();groups.clear();handles.clear();previousTheme=theme;previousPalette=palette;root.Children().Append(camera);
        }
        root.RequestedTheme(theme==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        auto layout=object(snapshot,L"layout");
        std::vector<uint32_t> visible;int order=0;
        std::vector<std::wstring> handleIds;
        for(auto value:array(layout,L"groups")){
            auto geometry=value.GetObject();uint32_t id=uint32_t(num(geometry,L"id"));visible.push_back(id);
            auto panel=find(array(snapshot,L"panels"),L"id",str(geometry,L"active"));
            auto [it,added]=groups.try_emplace(id);auto& group=it->second;
            if(added){
                group.frame.Child(group.content);group.frame.Background(clear());group.frame.CornerRadius({8,8,8,8});
                group.background.IsHitTestVisible(false);group.background.Fill(data->brush(L"panel"));
                group.content.Children().Append(group.background);group.content.Children().Append(group.border);
                group.configurationFrame.Background(clear());group.content.Children().Append(group.configurationFrame);
                root.Children().Append(group.frame);
                AutomationProperties::SetAutomationId(group.frame,L"workspace-group-"+to_hstring(id));
            }
            group.geometry=geometry;
            // Geometry-only changes retain controls, focus, capture and scroll.
            auto structure=J::Parse(geometry.Stringify());
            for(auto field:{L"bounds",L"resize_handles",L"tiles",L"footer_grip"})if(structure.HasKey(field))structure.Remove(field);
            structure.Insert(L"footer",B(object(geometry,L"footer_grip").Size()!=0));
            J signature=O({{L"geometry",structure},{L"panel",panelStructure(panel)}});
            A headers;for(auto member:array(geometry,L"panels")){
                auto model=find(array(snapshot,L"panels"),L"id",member.GetString());
                headers.Append(O({{L"title",S(str(model,L"title"))},{L"icon",S(str(model,L"icon"))},{L"tab",object(model,L"tab")}}));
            }signature.Insert(L"headers",headers);
            std::wstring key=signature.Stringify().c_str();
            if(group.key!=key){group.key=std::move(key);build(group,geometry,panel);}
            bool hidden=flag(snapshot,L"chrome_hidden")&&(!flag(geometry,L"floating")||flag(snapshot,L"hide_floating_panels"));
            group.hidden=hidden;
            int z=flag(geometry,L"floating")?100+2*order:0;++order;
            group.order=z;
            if(!hidden)for(auto handleValue:array(geometry,L"resize_handles")){
                auto handle=handleValue.GetObject();auto handleKey=L"floating-"+std::to_wstring(id)+L"-"+std::wstring(str(handle,L"edge"));
                resizeHandle(handleKey,object(handle,L"bounds"),O({{L"type",S(L"resize_floating")},
                    {L"group",N(id)},{L"edge",S(str(handle,L"edge"))}}),z+1);handleIds.push_back(handleKey);
            }
            group.body->Apply(!hidden);
        }
        for(auto it=groups.begin();it!=groups.end();){
            if(std::find(visible.begin(),visible.end(),it->first)==visible.end()){
                uint32_t index;if(root.Children().IndexOf(it->second.frame,index))root.Children().RemoveAt(index);
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
        updateConfiguration();
        expansion->Apply(configurationHeight());present();
        zen->Apply();collapsed->Apply();drawers->Apply();gestures->Refresh();
        camera.Foreground(data->brush(L"text"));place(camera,object(layout,L"status"));
        camera.TextAlignment(TextAlignment::Right);updateCamera(object(data->state,L"camera"));
        updatePopup();publishOverviews();reportTitlebar();
    }

    double configurationHeight()const{
        auto panel=str(object(data->state,L"customization"),L"expanded");
        if(panel.empty())panel=expansion->Panel();
        for(auto const& [id,group]:groups)if(group.configuration&&group.configuration->Panel()==panel)
            return group.configuration->ContentHeight();
        return 0;
    }
    void updateConfiguration(){
        auto wanted=str(object(data->state,L"customization"),L"expanded");
        auto active=expansion->Geometry();
        for(auto& [id,group]:groups){
            if(!wanted.empty()&&str(group.geometry,L"active")==wanted){
                auto panel=find(array(data->model,L"panels"),L"id",wanted);
                if(!group.configuration||group.configuration->Panel()!=wanted){
                    group.configurationFrame.Child(nullptr);
                    group.configuration=std::make_unique<PanelConfiguration>(data,panel,
                        [weak=weak_from_this()]{if(auto self=weak.lock())self->measured();});
                    group.configurationFrame.Child(group.configuration->Root());
                }
                group.configuration->Apply(panel,!group.hidden&&group.presented.Size()!=0);
            }else if(group.configuration&&active.Size()&&num(active,L"group")==id){
                auto panel=find(array(data->model,L"panels"),L"id",group.configuration->Panel());
                group.configuration->Apply(panel,!group.hidden);
            }else{
                group.configurationFrame.Child(nullptr);group.configuration.reset();
            }
        }
    }
    void reportTitlebar(){
        if(!root.IsLoaded())return;
        auto accepted=array(data->model,L"titlebar_insets");A value;bool same=accepted.Size()==3;
        for(uint32_t i=0;i<3;++i){value.Append(N(titlebar[i]));same=same&&accepted.GetNumberAt(i)==titlebar[i];}
        if(same){lastTitlebar=L"";return;}
        auto json=value.Stringify();if(json==lastTitlebar)return;lastTitlebar=json;
        data->dispatch(O({{L"type",S(L"measure_titlebar")},{L"insets",value}}));
    }
    void measurePanels(){
        if(data->updating||!root.IsLoaded()||root.ActualWidth()<=0)return;
        auto accepted=array(data->model,L"panel_measurements");std::vector<std::wstring> live;
        for(auto value:array(data->model,L"panels")){
            auto id=str(value.GetObject(),L"id");std::wstring key(id);live.push_back(key);
            if(!measurements.contains(key)){
                auto previous=find(accepted,L"panel",id);
                measurements.emplace(key,Measurement{num(previous,L"tab_width",36),num(previous,L"content_height",320)});
            }
        }
        for(auto it=measurements.begin();it!=measurements.end();)
            if(std::find(live.begin(),live.end(),it->first)==live.end())it=measurements.erase(it);else ++it;
        auto stable=[](double value){return std::round(std::clamp(value,0.,999999.)*64.)/64.;};
        for(auto const& [id,group]:groups){
            if(group.hidden||group.presented.Size()||!group.frame.IsLoaded())continue;
            for(auto const& [panel,tab]:group.tabs)if(tab.IsLoaded()&&tab.DesiredSize().Width>0)
                measurements[panel].tab=stable(tab.DesiredSize().Width);
            double height=group.body->ContentHeight();
            if(std::isfinite(height)&&height>=0)measurements[std::wstring(str(group.geometry,L"active"))].content=stable(height);
        }
        A report;bool same=accepted.Size()==measurements.size();
        for(auto const& [panel,size]:measurements){
            auto previous=find(accepted,L"panel",hstring(panel));
            same=same&&previous.Size()&&num(previous,L"tab_width")==size.tab&&num(previous,L"content_height")==size.content;
            report.Append(O({{L"panel",S(hstring(panel))},{L"tab_width",N(size.tab)},{L"content_height",N(size.content)}}));
        }
        if(same){lastMeasurements=L"";return;}
        auto json=report.Stringify();if(json==lastMeasurements)return;lastMeasurements=json;
        data->dispatch(O({{L"type",S(L"measure_panels")},{L"measurements",report}}));
    }
    void measured(){
        if(presenting||data->updating||!root.IsLoaded())return;
        // Coalesce layout notifications after native arrange. Measuring the
        // scroll extents never realizes the entire virtualized Layers list.
        if(!measurementQueued){
            measurementQueued=true;
            if(!root.DispatcherQueue().TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock()){
                self->measurementQueued=false;self->reportTitlebar();self->measurePanels();
            }}))measurementQueued=false;
        }
        expansion->Apply(configurationHeight());backgrounds();publishOverviews();
        if(!popup&&!str(object(data->state,L"customization"),L"control").empty())updatePopup();
    }
    void present(){
        if(presenting)return;presenting=true;
        struct Reset{bool& flag;~Reset(){flag=false;}} reset{presenting};
        auto expanded=expansion->Geometry();
        for(auto& [id,group]:groups){
            bool active=expanded.Size()&&num(expanded,L"group")==id&&group.configuration;
            group.presented=active?expanded:J{};
            auto bounds=object(active?expanded:group.geometry,L"bounds");auto box=rectangle(bounds);
            auto preview=active?object(expanded,L"preview"):rectangle({0,0,box.Width,box.Height});
            auto previewBox=rectangle(preview);
            place(group.frame,bounds);place(group.border,preview);
            group.content.Width(box.Width);group.content.Height(box.Height);
            RectangleGeometry clip;clip.Rect({0,0,box.Width,box.Height});group.content.Clip(clip);
            group.frame.Visibility(group.hidden?Visibility::Collapsed:Visibility::Visible);
            Canvas::SetZIndex(group.frame,active?1000:group.order);
            auto geometry=J::Parse(group.geometry.Stringify());geometry.Insert(L"bounds",preview);
            if(active&&expanded.HasKey(L"tiles"))geometry.Insert(L"tiles",object(expanded,L"tiles"));
            group.body->Layout(geometry);
            if(group.footer){
                auto grip=rectangle(object(group.geometry,L"footer_grip"));
                grip.Y=std::max(0.f,previewBox.Height-(flag(group.geometry,L"tabs_visible")?36.f:0.f)-grip.Height);
                place(group.footer,rectangle(grip));
            }
            if(active){
                auto configuration=object(expanded,L"configuration");auto configBox=rectangle(configuration);
                bool left=configBox.X<previewBox.X;bool tabs=configBox.Y>0;
                group.border.CornerRadius(left?CornerRadius{tabs?8.:0.,8,8,0}:CornerRadius{8,tabs?8.:0.,0,8});
                group.configurationFrame.CornerRadius(left?CornerRadius{8,0,0,8}:CornerRadius{0,8,8,0});
                place(group.configurationFrame,configuration);
                group.configurationFrame.Opacity(1);group.configurationFrame.IsHitTestVisible(!group.hidden);
                group.configuration->SetVisible(!group.hidden);
                AutomationProperties::SetItemStatus(group.configuration->Root(),expanded.Stringify());
            }else{
                group.border.CornerRadius({8,8,8,8});
                // Measure full natural content before the first shared geometry reply.
                place(group.configurationFrame,rectangle({box.Width,0,std::min(380.f,float(root.ActualWidth())),box.Height}));
                group.configurationFrame.Opacity(0);group.configurationFrame.IsHitTestVisible(false);
                if(group.configuration){group.configuration->SetVisible(false);AutomationProperties::SetItemStatus(group.configuration->Root(),L"");}
            }
            AutomationProperties::SetItemStatus(group.frame,active?expanded.Stringify():L"");
            auto prefix=L"floating-"+std::to_wstring(id)+L"-";
            for(auto const& [key,handle]:handles)if(key.starts_with(prefix))
                handle.Visibility(active||group.hidden?Visibility::Collapsed:Visibility::Visible);
        }
        if(!expanded.Size()&&str(object(data->state,L"customization"),L"expanded").empty()){
            for(auto& [id,group]:groups){group.configurationFrame.Child(nullptr);group.configuration.reset();}
        }
        backgrounds();publishOverviews();
    }
    void appendGroupOverviews(A& slots,Group const& group){
        if(group.hidden||!group.frame.IsLoaded())return;
        auto clip=visibleBounds(group.frame,root);int order=visualOrder(root,group.frame);
        if(group.body&&group.body->navigator){
            auto visible=intersect(clip,visibleBounds(group.body->navigator->Root(),root));
            auto slot=group.body->navigator->Placement(root,visible,order);
            if(slot.Size())slots.Append(slot);
        }
        if(group.presented.Size()&&group.configuration)group.configuration->AppendOverviews(slots,root,clip,order);
    }
    void backgrounds(){
        auto tabs=array(data->state,L"tabs");auto document=tabs.Size()?tabs.GetObjectAt(0):J{};
        for(auto& [id,group]:groups){
            if(!group.frame.IsLoaded()||group.hidden)continue;
            A slots;appendGroupOverviews(slots,group);
            auto bounds=object(group.presented.Size()?group.presented:group.geometry,L"bounds");
            auto key=O({{L"expansion",group.presented},{L"bounds",bounds},{L"slots",slots},
                {L"width",N(num(document,L"width"))},{L"height",N(num(document,L"height"))}}).Stringify();
            if(key==group.backgroundKey)continue;group.backgroundKey=key;
            GeometryGroup shape;shape.FillRule(FillRule::EvenOdd);
            shape.Children().Append(group.presented.Size()?expansionShape(group.presented):
                roundedRectangle(float(num(bounds,L"width")),float(num(bounds,L"height")),{8,8,8,8}));
            for(auto value:slots){
                auto slot=value.GetObject();auto area=array(slot,L"bounds"),clip=array(slot,L"clip");float image[4]{};
                if(area.Size()!=4||clip.Size()!=4||!capy_navigator_image(float(area.GetNumberAt(2)),float(area.GetNumberAt(3)),
                    uint32_t(num(document,L"width")),uint32_t(num(document,L"height")),image))continue;
                Rect hole{float(area.GetNumberAt(0))+image[0],float(area.GetNumberAt(1))+image[1],image[2],image[3]};
                hole=intersect(hole,{float(clip.GetNumberAt(0)),float(clip.GetNumberAt(1)),float(clip.GetNumberAt(2)),float(clip.GetNumberAt(3))});
                if(hole.Width<=0||hole.Height<=0)continue;
                hole.X-=float(num(bounds,L"x"));hole.Y-=float(num(bounds,L"y"));
                RectangleGeometry cutout;cutout.Rect(hole);shape.Children().Append(cutout);
            }
            group.background.Data(shape);
        }
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
        for(auto const& [id,group]:groups)appendGroupOverviews(slots,group);
        if(drawers)drawers->AppendOverviews(slots);
        auto tabs=array(data->state,L"tabs");overviewOcclusion.Apply(root,slots,tabs.Size()?tabs.GetObjectAt(0):J{});
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
void WorkspaceView::SetTitlebarInsets(float left,float right,float height){
    std::array<float,3> value{left,right,height};
    if(value!=impl->titlebar){impl->titlebar=value;impl->lastTitlebar=L"";impl->reportTitlebar();}
}
