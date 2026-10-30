#include "pch.h"
#include "WorkspaceView.h"
#include "ChromeVisibility.h"
#include "StrokeRecording.h"
#include "CommandSearch.h"
#include "CanvasActionBar.h"
#include "PreviewPanel.h"
#include "PressureCalibration.h"
#include "ZoomReadout.h"
#include "PanelBody.h"
#include "PanelScroll.h"
#include "PanelConfiguration.h"
#include "WorkspaceExpansion.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceTabSurface.h"
#include "WorkspacePublication.h"
#include "OverviewOcclusion.h"
#include "WorkspaceShadow.h"
#include "WorkspaceSurface.h"
#include "PanelBridgeSurface.h"
#include "WorkspaceDrawers.h"
#include "WorkspaceBubbles.h"
#include <set>
#include "CollapsedColumns.h"
#include "WorkspaceGestures.h"
#include "ColorView.h"
#include "ColorEditor.h"
#include "ColorStrip.h"
#include "ToolView.h"
#include "NavigatorView.h"
#include "EffectControls.h"
#include "LayersView.h"
#include "NativeMenus.h"
#include "UiControls.h"
#include "ScopesView.h"
#include "NativeHandles.h"
#include <winrt/Microsoft.UI.Xaml.Automation.h>
#include <winrt/Microsoft.UI.Xaml.Media.Imaging.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Windows.UI.Text.h>
#include <algorithm>
#include <cmath>
#include <filesystem>
#include <map>
#include <optional>
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
    Canvas root,utilities;
    std::shared_ptr<WorkspaceGestures> gestures;
    std::unique_ptr<WorkspaceDrawers> drawers;
    std::unique_ptr<WorkspaceBubbles> bubbles;
    std::unique_ptr<CollapsedColumns> collapsed;
    std::unique_ptr<WorkspaceExpansion> expansion;
    struct Offscreen {std::wstring key;std::shared_ptr<WorkspaceData> data;std::unique_ptr<PanelBody> body;};
    std::map<std::wstring,Offscreen> offscreen;
    struct Body {
        hstring key;std::shared_ptr<PanelBody> view;Panel host{nullptr};
        void Detach(){
            if(host){uint32_t at;if(host.Children().IndexOf(view->Root(),at))host.Children().RemoveAt(at);host=nullptr;}
        }
    };
    std::map<std::wstring,Body> bodies;
    Canvas measureHost;
    ContentControl measureGate;
    hstring lastMeasurements;
    bool measurementsPending=false,measuring=false;
    CompositionTarget::Rendered_revoker resizeTintFrame;
    CompositionTarget::Rendering_revoker resizeTraceFrame;
    std::array<float,3> titlebar{};
    hstring lastTitlebar;
    bool presenting=false;
    WorkspacePublication publication;
    std::optional<uint32_t> movingGroup;
    bool trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    uint64_t fullUpdates=0,motionUpdates=0;
    struct ResizeFrame {int64_t qpc;float width,height,appliedWidth,appliedHeight;bool valid,mismatch;uint32_t panels,columns,outside;};
    struct ResizeBound {int64_t qpc;uint32_t group;Rect bounds;bool outside,column;};
    std::vector<ResizeFrame> resizeFrames;
    std::vector<ResizeBound> resizeBounds;
    uint64_t resizeFrameOverflow=0;
    uint64_t resizeBoundsOverflow=0;
    uint64_t surfaceFrames=0,surfaceGroups=0,surfaceMismatches=0,tabFrames=0,escapedLabels=0;
    double maximumSurfaceOffset=0;
    J firstSurfaceMismatch;
    J workspaceUpdate;
    hstring lastPresentation;
    OverviewOcclusion overviewOcclusion;
    std::map<std::wstring,ContentControl> handles;
    struct Group {
        Border frame,border,configurationFrame;
        std::shared_ptr<ChromeVisibility> fade=std::make_shared<ChromeVisibility>();
        WorkspaceShadow shadow;
        hstring shadowKey;
        Canvas content,surfaceHost;
        Grid layout{nullptr},header{nullptr},bodyViewport{nullptr};
        StackPanel tabLabels{nullptr};
        Grid activeTab{nullptr};
        std::vector<AutomaticTab> automatic;
        Bindings paintIcons;
        ScrollView tabScroll{nullptr};
        ScrollView bodyScroll{nullptr};
        WorkspaceSurface background;
        WorkspaceTabSurface strip;
        std::array<float,4> corners{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
        Border footer{nullptr};
        std::wstring key;
        hstring tabKey,tabsPresentationKey;
        J geometry,presented;
        Windows::Foundation::Point offset{};
        TranslateTransform translation;
        RectangleGeometry clip;
        hstring backgroundKey;
        std::shared_ptr<PanelBody> body;
        std::map<std::wstring,Grid> tabs;
        std::unique_ptr<PanelConfiguration> configuration;
        bool hidden=false,gpu=false;
        ContourCommands bridge;
        hstring bridgeKey;
        int order=0;
    };
    std::map<uint32_t,Group> groups;
    hstring panelBridgeKey;
    NativeLayoutInput layoutInput,resolvedInput,metricInput;
    J panelMetricContext;hstring panelMetricKey;
    J resolvedLayout;
    float viewportWidth=0,viewportHeight=0,resolvedWidth=0,resolvedHeight=0;
    uint64_t panelLayoutRevision=0,panelQueryLayoutRevision=0;
    J panelBridgeFrame;
    uint64_t panelBridgeRevision=0;
    bool windowResizing=false,resizeHandback=false;
    int64_t nativeSizingStartQpc=0,nativeSizingEndQpc=0;
    bool nativeResize()const{return windowResizing||resizeHandback;}
    std::shared_ptr<CommandSearchPopup> commandSearch=std::make_shared<CommandSearchPopup>();
    std::shared_ptr<CanvasActionBar> canvasBar=std::make_shared<CanvasActionBar>();
    std::shared_ptr<CanvasNotice> notice=std::make_shared<CanvasNotice>();
    std::shared_ptr<ColorStrip> colorStrip=std::make_shared<ColorStrip>();
    std::set<uint32_t> handledColorRequests;
    std::array<std::shared_ptr<PreviewPanel>,2> previewPanels{std::make_shared<PreviewPanel>(),std::make_shared<PreviewPanel>()};
    std::shared_ptr<ResponseCalibration> pressure=std::make_shared<ResponseCalibration>();
    double cameraRevision=-1;
    hstring previousTheme,previousPalette,paintedTheme;std::vector<uint32_t> publishedBridgeGroups,publishedBridgeColumns;
    Flyout popup{nullptr};
    std::wstring popupControl;
    std::shared_ptr<uint64_t> popupGeneration=std::make_shared<uint64_t>(0);
    Bindings popupBindings;
    std::shared_ptr<ZoomReadout> zoom=std::make_shared<ZoomReadout>();
    Grid cameraSlot;
    Border cameraSurface;std::function<void()> glassChanged;std::map<std::wstring,double> tabWidths;
    Impl(Dispatch send,J catalog,NativeLocalization localization,PreviewTransport previews,std::function<void(bool)> popupChanged,Dispatch document,Dispatch input,EdgeTransport edges){
        data->input=std::move(input);data->edges=std::move(edges);gestures=std::make_shared<WorkspaceGestures>(data,root);
        data->document=std::move(document);
        data->popupChanged=std::move(popupChanged);
        data->thumbnails=CreateLayerThumbnailCache(previews);
        data->query=previews;data->previews=CreateFilterPreviewCache(std::move(previews));
        data->localization=localization;data->send=std::move(send);data->catalog=catalog;data->glassSurfaces=true;
        AutomationProperties::SetName(root,str(object(data->catalog,L"bootstrap"),L"drawing_workspace"));
    }
    void init(){
        data->chromeFrames->root=make_weak(root.as<FrameworkElement>());
        if(trace)resizeTraceFrame=CompositionTarget::Rendering(auto_revoke,[weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock()){
                self->traceSurfaces();
                if(!self->windowResizing)return;
                LARGE_INTEGER now{};QueryPerformanceCounter(&now);
                auto viewport=array(object(self->data->model,L"layout"),L"viewport");
                float width=float(self->root.ActualWidth()),height=float(self->root.ActualHeight());
                bool valid=viewport.Size()==2;
                float appliedWidth=valid?float(viewport.GetNumberAt(0)):0,appliedHeight=valid?float(viewport.GetNumberAt(1)):0;
                bool mismatch=!valid||std::abs(width-appliedWidth)>.01f||std::abs(height-appliedHeight)>.01f;
                uint32_t panels=0,columns=0,outside=0;
                for(auto const& [id,group]:self->groups){
                    if(group.hidden||flag(group.geometry,L"floating")||!group.frame.IsLoaded()||group.frame.Visibility()!=Visibility::Visible||group.frame.Opacity()<=0)continue;
                    auto bounds=allocatedBounds(group.frame,self->root);
                    bool escaped=bounds.X<-.01f||bounds.Y<-.01f||bounds.X+bounds.Width>width+.01f||bounds.Y+bounds.Height>height+.01f;
                    ++panels;if(escaped)++outside;
                    if(self->resizeBounds.size()<32768)self->resizeBounds.push_back({now.QuadPart,id,bounds,escaped,false});else ++self->resizeBoundsOverflow;
                }
                self->collapsed->ForEachBounds(self->root,[&](uint32_t id,Rect bounds){
                    bool escaped=bounds.X<-.01f||bounds.Y<-.01f||bounds.X+bounds.Width>width+.01f||bounds.Y+bounds.Height>height+.01f;
                    ++columns;if(escaped)++outside;
                    if(self->resizeBounds.size()<32768)self->resizeBounds.push_back({now.QuadPart,id,bounds,escaped,true});else ++self->resizeBoundsOverflow;
                });
                if(self->resizeFrames.size()<4096)self->resizeFrames.push_back({now.QuadPart,width,height,appliedWidth,appliedHeight,valid,mismatch,panels,columns,outside});else ++self->resizeFrameOverflow;
            }
        });
        data->strokes=std::make_shared<StrokeRecording>();data->strokes->data=data;data->strokes->start();
        data->colorsPresented=[weak=weak_from_this()]{if(auto self=weak.lock())self->tracePresentation();};
        zoom->data=data;zoom->init();
        auto surface=cameraSurface;surface.Child(zoom->root);surface.Background(headerSurface(data));
        surface.CornerRadius(zoom->root.CornerRadius());
        surface.HorizontalAlignment(HorizontalAlignment::Right);surface.VerticalAlignment(VerticalAlignment::Bottom);surface.Margin({4,0,4,0});
        cameraSlot.Children().Append(surface);
        root.Children().Append(cameraSlot);
        canvasBar->data=data;
        canvasBar->changed=[weak=weak_from_this()]{if(auto self=weak.lock()){self->gestures->ChromeChanged();self->placePreviews();self->queueMeasurements();}};
        canvasBar->init(root);
        notice->data=data;notice->init(root);
        pressure->data=data;pressure->init(root);
        colorStrip->data=data;colorStrip->init(root);data->colorStrip=colorStrip;
        for(auto [panel,kind]:{std::pair{previewPanels[0],PreviewPanel::SelectionRefine},std::pair{previewPanels[1],PreviewPanel::FrequencySeparation}}){
            panel->data=data;panel->kind=kind;panel->moved=[weak=weak_from_this()]{if(auto self=weak.lock())self->placeNotice();};panel->init(root);
        }
        collapsed=std::make_unique<CollapsedColumns>(data,root,gestures,[weak=weak_from_this()]{if(auto self=weak.lock())self->queueMeasurements();});
        commandSearch->data=data;commandSearch->changed=[weak=weak_from_this()]{if(auto self=weak.lock())self->queueMeasurements();};commandSearch->init(root);
        drawers=std::make_unique<WorkspaceDrawers>(data,root,gestures,[weak=weak_from_this()]{if(auto self=weak.lock()){self->publishDrawerSources();self->queueMeasurements();}});
        expansion=std::make_unique<WorkspaceExpansion>(data,root,gestures,[weak=weak_from_this()]{if(auto self=weak.lock())self->present();});
        bubbles=std::make_unique<WorkspaceBubbles>(data,root,[weak=weak_from_this()]{if(auto self=weak.lock())self->queueMeasurements();});
        measureGate.Content(measureHost);measureGate.IsEnabled(false);measureGate.IsTabStop(false);measureGate.IsHitTestVisible(false);measureGate.Visibility(Visibility::Collapsed);Canvas::SetLeft(measureGate,-100000);root.Children().Append(measureGate);
        root.LayoutUpdated([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->layoutUpdated();});
        root.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->queueMeasurements();});
    }
    void colorRequests(){
        if(!root.XamlRoot())return;
        auto requests=array(data->state,L"requests");
        std::erase_if(handledColorRequests,[&](uint32_t id){return !findId(requests,id).Size();});
        for(auto value:requests){
            auto request=value.GetObject();auto kind=object(request,L"kind");auto id=uint32_t(num(request,L"id"));
            if(str(kind,L"type")!=L"color_slot"||!handledColorRequests.insert(id).second)continue;
            auto slot=str(kind,L"slot");
            EditColor(data,root,O({{L"slot",S(slot)}}),false,[data=data,slot](J color,std::optional<double> stops){
                auto action=O({{L"op",S(stops?L"set_slot_intensity":L"set_slot")},{L"slot",S(slot)},{L"color",color}});
                if(stops)action.Insert(L"stops",N(*stops));
                data->dispatch(O({{L"type",S(L"color")},{L"action",action}}));
            });
            data->dispatch(O({{L"type",S(L"complete_request")},{L"id",N(id)},{L"error",JsonValue::CreateNullValue()}}));
        }
    }
    void removeGroup(Group const& group,std::shared_ptr<void> owner={}){
        uint32_t index=0;
        if(!gestures->DeferRemoval(group.frame,owner)&&root.Children().IndexOf(group.frame,index))root.Children().RemoveAt(index);
        if(!gestures->DeferRemoval(group.shadow.Root(),owner)&&root.Children().IndexOf(group.shadow.Root(),index))root.Children().RemoveAt(index);
    }
    void buildTabs(Group& group,J const& geometry){
        auto groupItem=O({{L"kind",S(L"group")},{L"group",N(num(geometry,L"id"))}});
        float fit=groupRadius(geometry)*CornerFit;group.border.Background(clear());group.border.CornerRadius(CornerRadius{fit,fit,fit,fit});
        if(!group.layout){
            group.layout=Grid();
            group.layout.RowDefinitions().Append(RowDefinition());
            RowDefinition bodyRow;bodyRow.Height({1,GridUnitType::Star});group.layout.RowDefinitions().Append(bodyRow);
            group.layout.RowDefinitions().Append(RowDefinition());
            group.border.Child(group.layout);
            group.tabLabels=StackPanel();group.tabLabels.Orientation(Orientation::Horizontal);
            group.tabLabels.Background(clear());
        }
        auto frame=group.layout;
        frame.RowDefinitions().GetAt(0).Height({flag(geometry,L"tabs_visible")?data->panelMetric(L"tab_bar_height"):0.,GridUnitType::Pixel});
        frame.RowDefinitions().GetAt(2).Height({num(object(geometry,L"footer_grip"),L"height"),GridUnitType::Pixel});
        A members;for(auto const& id:panelTabIds(array(geometry,L"panels"))){
            auto model=find(array(data->model,L"panels"),L"id",id);
            members.Append(O({{L"id",S(id)},{L"icon",S(str(model,L"icon"))}}));
        }
        auto tabKey=O({{L"members",members},{L"style",S(str(object(data->model,L"windows_tab_styles"),to_hstring(uint32_t(num(geometry,L"id"))).c_str()))}}).Stringify();
        if(group.tabKey!=tabKey){
            group.tabKey=tabKey;group.tabs.clear();group.activeTab=nullptr;group.automatic.clear();group.paintIcons.clear();
            auto tabs=group.tabLabels;tabs.Children().Clear();
            uint32_t index=0;bool fitted=automaticTabs(data,num(geometry,L"id"));
            for(auto value:array(geometry,L"panels")){
                auto id=value.GetString();auto model=find(array(data->model,L"panels"),L"id",id);
                auto tabStyle=object(model,L"tab");
                auto tab=panelTabButton(data,str(model,L"title"),panelIcon(data,str(model,L"icon"),group.paintIcons),tabStyle,[data=data,id,groupId=num(geometry,L"id")]{
                    data->dispatch(O({{L"type",S(L"select_panel_tab")},{L"group",N(groupId)},{L"panel",S(id)}}));
                });
                auto name=tab.Content().as<StackPanel>().Children().GetAt(1).as<TextBlock>();
                if(fitted)group.automatic.push_back({tab,name,std::wstring(id)+L"\n"+std::wstring(str(model,L"title"))+L"\n"+std::to_wstring(data->textSize())});
                auto item=O({{L"kind",S(L"panel")},{L"panel",S(id)}});
                gestures->Source(tab,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item,false,
                    O({{L"group",N(num(geometry,L"id"))},{L"index",N(index++)},{L"panel",S(id)}}));
                AutomationProperties::SetAutomationId(tab,L"panel-tab-"+id);
                auto shell=panelTabShell(tab,nullptr,data->tint(L"text",20));
                {
                    auto cut=[weak=weak_from_this(),id=uint32_t(num(geometry,L"id"))]{if(auto self=weak.lock()){self->cutStrip(id);self->queueMeasurements();}};
                    shell.SizeChanged([cut](auto&&,auto&&){cut();});
                    shell.RegisterPropertyChangedCallback(UIElement::OpacityProperty(),[cut](auto&&,auto&&){cut();});
                }
                group.tabs.emplace(std::wstring(id),shell);tabs.Children().Append(shell);
            }
            if(!group.tabScroll){
                group.tabScroll=ScrollView();auto tabScroll=group.tabScroll;Border handle;group.header=panelHeader(data,tabs,tabScroll,handle);auto header=group.header;
                gestures->Source(tabScroll,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem,true,{},WorkspaceGestures::Pickup::Sideways);
                tabScroll.SizeChanged([weak=weak_from_this(),id=uint32_t(num(geometry,L"id"))](auto&&,auto&&){if(auto self=weak.lock()){self->fitTabs(id);self->cutStrip(id);}});
                tabScroll.ViewChanged([weak=weak_from_this(),id=uint32_t(num(geometry,L"id"))](auto&&,auto&&){if(auto self=weak.lock())self->cutStrip(id);});
                tabs.SizeChanged([weak=weak_from_this(),id=uint32_t(num(geometry,L"id"))](auto&&,auto&&){if(auto self=weak.lock())self->cutStrip(id);});
                gestures->Source(handle,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem,true);
                AutomationProperties::SetAutomationId(handle,L"group-grip-"+to_hstring(uint32_t(num(geometry,L"id"))));
                copyName(data,handle,data->copyDelivery(L"move_group"));
                auto help=data->copyDelivery(L"drag_panel");
                AutomationProperties::SetHelpText(handle,help);
                data->copyView([weak=make_weak(handle),resolve=help.current]{if(auto view=weak.get()){AutomationProperties::SetHelpText(view,resolve());return true;}return false;});
                frame.Children().Append(header);
            }
        }
        group.header.Visibility(flag(geometry,L"tabs_visible")?Visibility::Visible:Visibility::Collapsed);
        group.activeTab=nullptr;
        bool fitted=automaticTabs(data,num(geometry,L"id"));
        for(auto const& [id,shell]:group.tabs){
            auto tab=shell.Children().GetAt(1).as<Button>();
            auto model=find(array(data->model,L"panels"),L"id",hstring(id));auto presentation=object(model,L"tab");
            auto title=str(model,L"title");AutomationProperties::SetName(tab,title);tooltip(tab,title);
            auto label=tab.Content().as<StackPanel>();label.Children().GetAt(1).as<TextBlock>().Text(title);
            for(auto& automatic:group.automatic)if(automatic.tab==tab)automatic.key=id+L"\n"+std::wstring(title)+L"\n"+std::to_wstring(data->textSize());
            label.Children().GetAt(0).Visibility(fitted||flag(presentation,L"show_icon")?Visibility::Visible:Visibility::Collapsed);
            if(!fitted)label.Children().GetAt(1).Visibility(flag(presentation,L"show_name")?Visibility::Visible:Visibility::Collapsed);
            bool selected=hstring(id)==str(geometry,L"active");
            selectPanelTabShell(shell,selected,nullptr,presentation);
            if(selected)group.activeTab=shell;
        }
        orderPanelTabs(group.tabLabels,array(geometry,L"panels"),group.tabs,group.automatic);
        if(group.tabScroll&&group.tabScroll.ActualWidth()>0)fitAutomaticTabs(data,group.automatic,group.tabScroll.ActualWidth(),tabWidths);
    }
    void build(Group& group,J const& geometry,J const& panel){
        group.body.reset();group.footer=nullptr;group.backgroundKey=L"";
        buildTabs(group,geometry);
        auto frame=group.layout;
        while(frame.Children().Size()>1)frame.Children().RemoveAtEnd();
        auto create=[&]{return std::make_shared<PanelBody>(data,panel,geometry,
            [weak=weak_from_this()]{if(auto self=weak.lock())self->measured();},gestures);};
        Body* retained=nullptr;
        if(object(geometry,L"tiles").Size())group.body=create();
        else{
            auto key=O({{L"panel",panelStructure(panel)},{L"metrics",S(panelMetricKey)}}).Stringify();
            auto& cached=bodies[std::wstring(str(panel,L"id"))];
            if(!cached.view||cached.key!=key||gestures->Retains(cached.view->Root())){cached.view=create();cached.key=key;cached.host=nullptr;}
            else cached.Detach();
            group.body=cached.view;retained=&cached;
        }
        auto body=group.body->Root();
        group.bodyScroll=nullptr;group.bodyViewport=nullptr;
        if(object(geometry,L"tiles").Size()){
            Grid::SetRow(body,1);frame.Children().Append(body);
        }else{
            group.bodyScroll=body.try_as<ScrollView>();
            if(!group.bodyScroll){
                group.bodyViewport=Grid();group.bodyViewport.Children().Append(body);
                if(retained)retained->host=group.bodyViewport;
                group.bodyViewport.Background(clear());group.bodyScroll=ScrollView();group.bodyScroll.Content(group.bodyViewport);group.bodyScroll.ContentOrientation(body.try_as<StackPanel>()?ScrollingContentOrientation::Vertical:ScrollingContentOrientation::None);panelScrolling(group.bodyScroll,group.bodyViewport,data);group.bodyScroll.HorizontalScrollMode(ScrollingScrollMode::Disabled);
                group.bodyScroll.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
                group.bodyScroll.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);mouseScrollbars(group.bodyScroll);
                group.bodyScroll.ViewChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->measured();});
            }
            AutomationProperties::SetAutomationId(group.bodyScroll,L"panel-body-scroll-"+to_hstring(uint32_t(num(geometry,L"id"))));
            AutomationProperties::SetAccessibilityView(group.bodyScroll,Automation::Peers::AccessibilityView::Control);
            Grid::SetRow(group.bodyScroll,1);frame.Children().Append(group.bodyScroll);
            if(retained&&!group.bodyViewport)retained->host=frame;
        }
        auto grip=object(geometry,L"footer_grip");
        if(grip.Size()){
            Canvas overlay;Grid::SetRow(overlay,2);
            Border handle;group.footer=handle;handle.Background(clear());place(handle,grip);
            handle.Child(panelGrip(data->theme(),true));
            auto item=O({{L"kind",S(L"panel")},{L"panel",S(str(panel,L"id"))}});
            gestures->Source(handle,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item,true);
            auto caption=[source=std::weak_ptr<WorkspaceData>(data),id=str(panel,L"id")]{
                auto current=source.lock();if(!current)return hstring();
                auto currentPanel=find(array(current->model,L"panels"),L"id",id);
                return current->caption(O({{L"type",S(L"move_panel")},{L"title",S(str(currentPanel,L"title"))}}));
            };
            copyName(data,handle,LocalizedCopy(caption(),caption));
            AutomationProperties::SetAutomationId(handle,L"panel-footer-grip-"+str(panel,L"id"));
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
            if(!anchor&&drawers)anchor=drawers->Anchor(next);
            if(!anchor)return;
            StackPanel content;content.Width(280);content.Spacing(12);
            content.Children().Append(label(data,next==L"brush_color"?L"Brush color":L"Brush opacity",true));
            if(next==L"brush_opacity"){
                NumberPresentation presentation;presentation.reset=[data=data]{data->dispatch(O({{L"type",S(L"reset_tool_setting")},{L"id",S(L"opacity")}}));};
                content.Children().Append(number(data,L"Opacity",object(data->catalog,L"opacity"),
                    [data=data]{return num(object(data->state,L"brush"),L"opacity");},
                    [data=data](double value){data->dispatch(O({{L"type",S(L"set_brush_opacity")},{L"value",N(value)}}));},popupBindings,nullptr,false,L"",false,presentation));
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
    static uint64_t revision(J const& update,wchar_t const* field){
        double value=num(update,field,-1);
        if(!std::isfinite(value)||value<0||value>9007199254740991.||std::floor(value)!=value)
            throw hresult_invalid_argument(L"Invalid workspace publication revision");
        return uint64_t(value);
    }
    J metricContext(){
        auto scale=root.XamlRoot()?root.XamlRoot().RasterizationScale():1.;
        auto textScale=Windows::UI::ViewManagement::UISettings().TextScaleFactor();
        auto style=L"winui:"+data->language()+L":"+to_hstring(data->textSize());
        auto key=O({{L"scale",N(scale)},{L"text_scale",N(textScale)},{L"style",S(style)}}).Stringify();
        if(key!=panelMetricKey){
            static std::atomic<uint64_t> generation{0};
            panelMetricKey=key;auto panelMetricGeneration=++generation;
            panelMetricContext=O({{L"generation",N(double(panelMetricGeneration))},{L"scale",N(scale)},{L"text_scale",N(textScale)},{L"style",S(style)}});
        }
        return panelMetricContext;
    }
    bool resolveLayout(){
        if(!layoutInput||viewportWidth<=0||viewportHeight<=0)return false;
        auto previous=panelMetricKey;auto context=metricContext();
        if(layoutInput.get()!=metricInput.get()||previous!=panelMetricKey){
            auto batch=O({{L"context",context},{L"reports",A{}}});
            NativeLayoutInput measured(capy_layout_input_measure(layoutInput.get(),to_string(batch.Stringify()).c_str()));
            if(!measured)throw hresult_invalid_argument(L"Invalid panel metric context");
            layoutInput=std::move(measured);metricInput=layoutInput;
        }
        if(layoutInput.get()==resolvedInput.get()&&viewportWidth==resolvedWidth&&viewportHeight==resolvedHeight){
            data->model.Insert(L"layout",resolvedLayout);return false;
        }
        auto result=capy_layout_resolve(layoutInput.get(),viewportWidth,viewportHeight);
        NativeString reply(result,capy_string_free);J packet;
        if(!reply||!J::TryParse(to_hstring(reply.get()),packet)||!object(packet,L"result").Size())
            throw hresult_invalid_argument(L"Workspace layout resolution failed: "+str(packet,L"error"));
        resolvedLayout=object(packet,L"result");resolvedInput=layoutInput;resolvedWidth=viewportWidth;resolvedHeight=viewportHeight;
        data->model.Insert(L"layout",resolvedLayout);++panelLayoutRevision;
        return true;
    }
    void resize(float width,float height){
        if(!std::isfinite(width)||!std::isfinite(height)||width<=0||height<=0)return;
        viewportWidth=width;viewportHeight=height;
        if(!resolveLayout())return;
        data->updating=true;struct Reset {bool& value;~Reset(){value=false;}} reset{data->updating};
        applyLayout(data->model,false);data->updating=false;presentBackgrounds();
    }
    void applyGlass(double previous){
        auto value=num(object(data->model,L"windows_glass"),L"chrome_revision",-1);if(previous==value)return;
        if(value<0)data->chromeFrames->Reset();else data->chromeFrames->Present(uint64_t(value));
        presentBackgrounds();drawers->PresentChrome();
        overviewOcclusion.Apply(root,data->chromeFrames->presented.overviews,data->chromeFrames->presented.document);
        tracePresentation();
    }
    bool applyFields(J const& model,bool scopes){
        auto update=object(model,L"workspace_update");
        if(!publication.Accept(true,revision(update,L"revision"),revision(update,L"model_revision"),revision(update,L"content_revision")))return false;
        workspaceUpdate=update;
        auto previousGlass=num(object(data->model,L"windows_glass"),L"chrome_revision",-1);
        model.Insert(L"layout",data->model.GetNamedValue(L"layout"));
        data->model=model;data->state=object(model,L"state");
        if(scopes){
            CapyScopes::Refresh(data);
            for(auto& [id,copy]:offscreen)if(copy.data){
                copy.data->model=model;copy.data->state=data->state;CapyScopes::Refresh(copy.data);
            }
            queueMeasurements();
        }
        applyGlass(previousGlass);
        tracePresentation();return true;
    }
    bool apply(J const& snapshot,NativeLayoutInput preparedInput){
        auto previousGpu=num(data->model,L"windows_gpu_generation");
        bool relocalize=data->adoptLocalization(snapshot);
        if(relocalize){measureHost.Children().Clear();offscreen.clear();AutomationProperties::SetName(root,str(object(data->catalog,L"bootstrap"),L"drawing_workspace"));}
        bool full=snapshot.HasKey(L"state");
        auto update=object(snapshot,L"workspace_update");
        bool reflow=!full&&snapshot.HasKey(L"layout");
        if((full||reflow)&&!preparedInput)throw hresult_invalid_argument(L"Missing prepared workspace layout input");
        // Workspace motion can also carry a camera. It is never a camera-only
        // fallback, and it must reference the content already retained here.
        if(full||update.Size()){
            if(!publication.Accept(full,revision(update,L"revision"),revision(update,L"model_revision"),revision(update,L"content_revision"),reflow))return false;
            workspaceUpdate=update;
        }
        if(!full&&!reflow&&snapshot.HasKey(L"windows_glass")){
            auto previous=num(object(data->model,L"windows_glass"),L"chrome_revision",-1);
            data->model.Insert(L"windows_glass",snapshot.GetNamedValue(L"windows_glass"));
            applyGlass(previous);
            if(snapshot.Size()==1){tracePresentation();return true;}
        }
        if(!full&&snapshot.HasKey(L"command_search")){
            data->state.Insert(L"command_search",snapshot.GetNamedValue(L"command_search"));commandSearch->Apply(data->state);
            return true;
        }
        if(!full&&!reflow){
            if(snapshot.HasKey(L"color_preview")){data->colorPreview=object(snapshot,L"color_preview");data->queueColors();}
            else if(WorkspaceData::previewing(data->colorPreview)){data->colorPreview=J{};data->queueColors();}
            if(update.Size()){++motionUpdates;applyMotion(object(update,L"drag"));canvasBar->Dragging(object(update,L"drag").Size()!=0);}
            if(snapshot.HasKey(L"camera")){
                auto cameraPatch=object(snapshot,L"camera");
                data->state.Insert(L"camera",cameraPatch);updateCamera(cameraPatch);colorStrip->present();
            }
            tracePresentation();return true;
        }
        if(full)++fullUpdates;
        movingGroup.reset();
        data->updating=true;
        struct Reset {bool& value;~Reset(){value=false;}} reset{data->updating};
        if(reflow)WorkspaceView::MergeLayout(data->model,snapshot);
        auto model=reflow?data->model:snapshot;
        layoutInput=std::move(preparedInput);
        if(full){
        if(previousGpu>0&&num(model,L"windows_gpu_generation")!=previousGpu){
            data->chromeFrames->Reset();publishedBridgeGroups.clear();publishedBridgeColumns.clear();
            data->thumbnails=CreateLayerThumbnailCache(data->query);
            data->previews=CreateFilterPreviewCache(data->query);
        }
        data->model=snapshot;data->state=object(model,L"state");data->colorPreview=object(model,L"color_preview");
        RefreshFilterPreviewContext(data->previews,object(data->state,L"document_file").GetNamedValue(L"epoch").Stringify());
        lastMeasurements=L"";
        data->refreshPalette();
        auto theme=data->theme(),palette=object(data->state,L"palette").Stringify();
        if(theme!=previousTheme||palette!=previousPalette){
            panelBridgeKey=L"";
            for(auto& [id,group]:groups)group.backgroundKey=L"";
            previousTheme=theme;previousPalette=palette;
        }
        root.RequestedTheme(theme==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        }
        if(viewportWidth<=0||viewportHeight<=0){
            auto viewport=array(object(model,L"layout"),L"viewport");
            if(viewport.Size()==2){viewportWidth=float(viewport.GetNumberAt(0));viewportHeight=float(viewport.GetNumberAt(1));}
        }
        resolveLayout();applyLayout(data->model,full);if(full&&needsPanelMeasurements())preparePanels();data->updating=false;presentBackgrounds();tracePresentation();if(full)colorRequests();return true;
    }
    bool applyLayout(J const& model,bool full){
        bool rebuilt=false;
        auto update=workspaceUpdate;
        auto layout=object(model,L"layout");
        std::vector<uint32_t> visible;int order=0;
        std::vector<std::wstring> handleIds;
        for(auto value:array(layout,L"groups")){
            auto geometry=value.GetObject();uint32_t id=uint32_t(num(geometry,L"id"));visible.push_back(id);
            auto panel=find(array(model,L"panels"),L"id",str(geometry,L"active"));
            auto [it,added]=groups.try_emplace(id);auto& group=it->second;
            if(added){
                group.frame.UseLayoutRounding(false);group.frame.Child(group.surfaceHost);group.frame.Background(clear());group.frame.CornerRadius({SurfaceRadius*CornerFit,SurfaceRadius*CornerFit,SurfaceRadius*CornerFit,SurfaceRadius*CornerFit});
                group.frame.RenderTransform(group.translation);group.shadow.Root().RenderTransform(TranslateTransform());
                group.content.Clip(group.clip);
                group.surfaceHost.Children().Append(group.background.Root());group.surfaceHost.Children().Append(group.strip.Root());group.surfaceHost.Children().Append(group.content);group.content.Children().Append(group.border);
                group.configurationFrame.Background(clear());group.content.Children().Append(group.configurationFrame);
                root.Children().Append(group.shadow.Root());root.Children().Append(group.frame);
                AutomationProperties::SetAutomationId(group.frame,L"workspace-group-"+to_hstring(id));
                edgeTag(group.frame,L"edge_surface",B(true));
            }
            AutomationProperties::SetName(group.frame,str(panel,L"title"));
            group.geometry=geometry;group.offset={};
            // Geometry-only changes retain controls, focus, capture and scroll.
            if(full||added||!group.body){
            auto structure=J::Parse(geometry.Stringify());
            for(auto field:{L"bounds",L"resize_handles",L"tiles",L"footer_grip",L"min_body_height",L"zen_kept",L"scrolls",L"panels"})if(structure.HasKey(field))structure.Remove(field);
            if(auto tiles=object(geometry,L"tiles");tiles.Size())structure.Insert(L"tile_style",S(str(tiles,L"tile_style")));
            structure.Insert(L"footer",B(object(geometry,L"footer_grip").Size()!=0));
            J signature=O({{L"geometry",structure},{L"panel",panelStructure(panel)},{L"metrics",S(panelMetricKey)}});
            A headers;for(auto member:array(geometry,L"panels")){
                auto model=find(array(data->model,L"panels"),L"id",member.GetString());
                headers.Append(O({{L"id",member},{L"title",S(str(model,L"title"))},{L"icon",S(str(model,L"icon"))},{L"tab",object(model,L"tab")}}));
            }
            auto tabsPresentationKey=O({{L"headers",headers},{L"style",S(str(object(data->model,L"windows_tab_styles"),to_hstring(id).c_str()))},
                {L"text_size",N(data->textSize())}}).Stringify();
            std::wstring key=signature.Stringify().c_str();
            if(group.key!=key||!group.body){
                try{build(group,geometry,panel);rebuilt=true;group.key=std::move(key);group.tabsPresentationKey=tabsPresentationKey;}
                catch(...){removeGroup(group);groups.erase(it);throw;}
            }else if(group.tabsPresentationKey!=tabsPresentationKey){buildTabs(group,geometry);rebuilt=true;group.tabsPresentationKey=tabsPresentationKey;}
            for(auto const& update:group.paintIcons)update();
            }
            bool hidden=flag(model,L"chrome_hidden")&&!flag(geometry,L"floating")&&!flag(geometry,L"zen_kept");
            group.hidden=hidden;
            int z=flag(geometry,L"floating")?100+2*order:0;++order;
            group.order=z;
            if(!hidden)for(auto handleValue:array(geometry,L"resize_handles")){
                auto handle=handleValue.GetObject();auto handleKey=L"floating-"+std::to_wstring(id)+L"-"+std::wstring(str(handle,L"edge"));
                resizeHandle(handleKey,object(handle,L"bounds"),O({{L"type",S(L"resize_floating")},
                    {L"group",N(id)},{L"edge",S(str(handle,L"edge"))}}),z+1);handleIds.push_back(handleKey);
            }
            if(full||added)group.body->Apply(!hidden||(group.frame.Visibility()==Visibility::Visible&&group.frame.Opacity()>0));
        }
        for(auto it=groups.begin();it!=groups.end();){
            if(std::find(visible.begin(),visible.end(),it->first)==visible.end()){
                auto current=it++;
                auto retired=std::make_shared<decltype(groups)::node_type>(groups.extract(current));
                removeGroup(retired->mapped(),retired);
            }else ++it;
        }
        if(!flag(model,L"chrome_hidden"))for(auto value:array(layout,L"dividers")){
            auto divider=value.GetObject();if(flag(divider,L"fixed"))continue;auto key=L"divider-"+std::to_wstring(uint32_t(num(divider,L"id")));
            resizeHandle(key,object(divider,L"bounds"),O({{L"type",S(L"drag_divider")},{L"id",N(num(divider,L"id"))}}),10,
                str(divider,L"axis")==L"horizontal"&&!str(divider,L"owner_side").empty());
            handleIds.push_back(key);
        }
        for(auto it=handles.begin();it!=handles.end();){
            if(std::find(handleIds.begin(),handleIds.end(),it->first)==handleIds.end()){
                uint32_t index=0;if(!gestures->DeferRemoval(it->second)&&root.Children().IndexOf(it->second,index))root.Children().RemoveAt(index);
                it=handles.erase(it);
            }else ++it;
        }
        if(full)updateConfiguration();
        if(full)commandSearch->Apply(data->state);
        canvasBar->Dragging(object(update,L"drag").Size()!=0);if(full)canvasBar->Apply(data->state);
        for(auto const& panel:previewPanels){panel->Place(layout);if(full)panel->Publish(data->state);}
        if(full){pressure->attach();pressure->Publish();}
        notice->Place(layout);if(full){notice->Publish(data->state);data->queueColors();}
        collapsed->Apply();expansion->Apply(configurationHeight());present();
        drawers->Apply(layoutInput,viewportWidth,viewportHeight);bubbles->Apply();gestures->Refresh();
        auto status=object(layout,L"status");place(cameraSlot,status);
        cameraSlot.Visibility(num(status,L"height")>0?Visibility::Visible:Visibility::Collapsed);
        zoom->text.Foreground(data->brush(L"text"));
        updateCamera(object(data->state,L"camera"));
        updatePopup();queueMeasurements();
        if(paintedTheme!=data->theme()){paintedTheme=data->theme();std::set<void*> seen;refreshAssetIconTheme(root,paintedTheme,seen);}
        applyMotion(object(update,L"drag"));
        return rebuilt;
    }

    Rect placement(Group const& group)const{
        auto bounds=rectangle(object(group.presented.Size()?group.presented:group.geometry,L"bounds"));
        bounds.X+=group.offset.X;bounds.Y+=group.offset.Y;return pixelBounds(bounds,root);
    }
    void placeMotion(uint32_t id,Group const& group){
        // Native render transforms preserve control resolution and move hit/clip
        // coordinates without invalidating the retained panel's layout.
        group.translation.X(group.offset.X);group.translation.Y(group.offset.Y);
        auto shadowTransform=group.shadow.Root().RenderTransform().as<TranslateTransform>();
        shadowTransform.X(group.offset.X);shadowTransform.Y(group.offset.Y);
        // Resize grips are siblings of the frame. Apply the same absolute
        // offset so their hit rectangles stay attached to the native panel.
        for(auto value:array(group.geometry,L"resize_handles")){
            auto handle=value.GetObject();
            auto key=L"floating-"+std::to_wstring(id)+L"-"+std::wstring(str(handle,L"edge"));
            auto found=handles.find(key);if(found==handles.end())continue;
            auto transform=found->second.RenderTransform().as<TranslateTransform>();
            transform.X(group.offset.X);transform.Y(group.offset.Y);
        }
    }
    void applyMotion(J const& drag){
        bool previousPresenting=std::exchange(presenting,true),moved=false;
        struct Reset{bool& flag;bool previous;~Reset(){flag=previous;}} reset{presenting,previousPresenting};
        auto motion=object(drag,L"group");
        std::optional<uint32_t> next;
        if(motion.Size())next=uint32_t(num(motion,L"id"));
        if(movingGroup&&movingGroup!=next)if(auto found=groups.find(*movingGroup);found!=groups.end()){
            found->second.offset={};placeMotion(found->first,found->second);moved=true;
        }
        movingGroup=next;
        if(next)if(auto found=groups.find(*next);found!=groups.end()){
            auto& group=found->second;
            auto from=pixelBounds(rectangle(object(group.geometry,L"bounds")),root),to=pixelBounds(rectangle(object(motion,L"bounds")),root);
            Point offset{to.X-from.X,to.Y-from.Y};
            moved|=offset.X!=group.offset.X||offset.Y!=group.offset.Y;
            group.offset=offset;placeMotion(*next,group);
        }
        gestures->Present(drag);bool tabsMoved=drawers->PresentTabs();
        for(auto& [id,group]:groups){auto [offset,animate]=gestures->TabOffset(id);tabsMoved=group.strip.TranslateTab(offset,animate)||tabsMoved;}
        if(moved||tabsMoved)queueMeasurements();
    }
    void traceSurfaces(){
        if(!root.IsLoaded())return;
        bool mismatch=false,tabbed=false,escaped=false;
        double scale=root.XamlRoot()?root.XamlRoot().RasterizationScale():1;
        ++surfaceFrames;
        for(auto const& [id,group]:groups){
            if(group.hidden||!group.frame.IsLoaded()||group.frame.Visibility()!=Visibility::Visible||group.frame.Opacity()<=0)continue;
            auto bounds=allocatedBounds(group.frame,root);
            if(bounds.Width<=0||bounds.Height<=0)continue;
            auto size=group.background.Size();
            Rect fill{bounds.X,bounds.Y,size.Width,size.Height+float(Canvas::GetTop(group.background.Root()))};
            if(group.gpu)for(auto value:array(panelBridgeFrame,L"groups"))if(num(value.GetObject(),L"group")==id){fill=rectangle(object(value.GetObject(),L"bounds"));break;}
            double offset=scale*std::max({std::abs(bounds.X-fill.X),std::abs(bounds.Y-fill.Y),std::abs(bounds.Width-fill.Width),std::abs(bounds.Height-fill.Height)});
            ++surfaceGroups;maximumSurfaceOffset=std::max(maximumSurfaceOffset,offset);
            if(offset>.01){
                mismatch=true;
                if(!firstSurfaceMismatch.Size())firstSurfaceMismatch=O({{L"group",N(id)},{L"frame",rectangle(bounds)},{L"fill",rectangle(fill)},{L"offset_px",N(offset)}});
            }
            for(auto const& [name,shell]:group.tabs){
                if(!shell.IsLoaded()||shell.Opacity()<=0||shell.Visibility()!=Visibility::Visible)continue;
                auto tab=shell.Children().GetAt(1).as<Button>();auto box=allocatedBounds(tab,root);
                if(box.Width<=0||box.Height<=0)continue;
                tabbed=true;
                std::function<void(DependencyObject const&)> visit=[&](DependencyObject const& node){
                    if(auto element=node.try_as<FrameworkElement>();element&&(element.Visibility()!=Visibility::Visible||element.Opacity()<=0))return;
                    if(auto text=node.try_as<TextBlock>()){
                        auto label=allocatedBounds(text,root);
                        escaped|=label.Width>0&&label.Height>0&&(label.X<box.X-.01f||label.Y<box.Y-.01f||label.X+label.Width>box.X+box.Width+.01f||label.Y+label.Height>box.Y+box.Height+.01f);
                    }
                    for(int i=0;i<VisualTreeHelper::GetChildrenCount(node);++i)visit(VisualTreeHelper::GetChild(node,i));
                };
                visit(tab);
            }
        }
        if(mismatch)++surfaceMismatches;
        if(tabbed)++tabFrames;
        if(escaped)++escapedLabels;
    }
    void tracePresentation(){
        if(!trace)return;
        A positions;
        for(auto const& [id,group]:groups){
            if(!group.frame.IsLoaded())continue;
            auto bounds=group.frame.TransformToVisual(root).TransformBounds(
                {0,0,float(group.frame.ActualWidth()),float(group.frame.ActualHeight())});
            A radii;for(auto radius:group.corners)radii.Append(N(radius));
            J body;
            if(group.bodyScroll){
                body=O({{L"bounds",rectangle(allocatedBounds(group.bodyScroll,root))},
                    {L"viewport_height",N(group.bodyScroll.ViewportHeight())},{L"extent_height",N(group.bodyScroll.ExtentHeight())},
                    {L"vertical_offset",N(group.bodyScroll.VerticalOffset())}});
            }
            positions.Append(O({{L"id",N(id)},{L"bounds",rectangle(bounds)},{L"radii",radii},{L"gpu_background",B(group.gpu)},{L"native_bridge_background",B(nativeResize()&&!group.bridge.empty())},{L"body_scroll",body}}));
        }
        // Automation bounds can include invisible template focus decorations or
        // omit a container's padding. Trace the actual arranged rectangles too;
        // this is opt-in evidence and never changes layout or hit testing.
        A elements;
        std::function<void(DependencyObject const&)> visit=[&](DependencyObject const& node){
            if(auto item=node.try_as<FrameworkElement>()){
                if(item.Visibility()!=Visibility::Visible)return;
                auto id=AutomationProperties::GetAutomationId(item);
                auto key=std::wstring_view(id);
                if((key.starts_with(L"layer-")||key.starts_with(L"column-")||key.starts_with(L"collapsed-column-")
                    ||key.starts_with(L"tool-")||key.starts_with(L"number-")||key.starts_with(L"property-")||key.starts_with(L"properties-")
                    ||key.starts_with(L"toolbar-")||key.starts_with(L"component-slider-")||key.starts_with(L"slider-")
                    ||key.starts_with(L"filter-")||key.starts_with(L"histogram-")||key.starts_with(L"waveform-")||key.starts_with(L"navigator-")
                    ||key.starts_with(L"palette-")||key.starts_with(L"palettes-")||key.starts_with(L"proof-panel-")
                    ||key.starts_with(L"panel-body-scroll-")||key.starts_with(L"panel-scroll-"))
                    &&item.IsLoaded()&&item.ActualWidth()>0&&item.ActualHeight()>0){
                    auto transform=item.TransformToVisual(root);auto size=item.RenderSize();
                    auto bounds=transform.TransformBounds({0,0,size.Width,size.Height});
                    auto contentBounds=transform.TransformBounds({0,0,float(item.ActualWidth()),float(item.ActualHeight())});
                    elements.Append(O({{L"id",S(id)},{L"bounds",rectangle(bounds)},{L"actual_bounds",rectangle(contentBounds)}}));
                }
            }
            for(int i=0;i<VisualTreeHelper::GetChildrenCount(node);i++)visit(VisualTreeHelper::GetChild(node,i));
        };
        if(!windowResizing)visit(root);
        A grips;
        for(auto const& [id,handle]:handles){
            if(!handle.IsLoaded())continue;
            auto bounds=handle.TransformToVisual(root).TransformBounds(
                {0,0,float(handle.ActualWidth()),float(handle.ActualHeight())});
            grips.Append(O({{L"id",S(hstring(id))},{L"bounds",rectangle(bounds)}}));
        }
        A resizeSamples;
        if(!windowResizing)for(auto const& frame:resizeFrames)resizeSamples.Append(O({
            {L"qpc",N(double(frame.qpc))},{L"width",N(frame.width)},{L"height",N(frame.height)},
            {L"applied_width",N(frame.appliedWidth)},{L"applied_height",N(frame.appliedHeight)},
            {L"valid",B(frame.valid)},{L"mismatch",B(frame.mismatch)},{L"panels",N(frame.panels)},{L"columns",N(frame.columns)},{L"outside",N(frame.outside)}}));
        A boundSamples;
        if(!windowResizing){
            for(auto const& bound:resizeBounds)boundSamples.Append(O({{L"qpc",N(double(bound.qpc))},{L"group",N(bound.group)},{L"kind",S(bound.column?L"column":L"group")},{L"bounds",rectangle(bound.bounds)},{L"outside",B(bound.outside)}}));
        }
        LARGE_INTEGER snapshotQpc{};QueryPerformanceCounter(&snapshotQpc);
        auto value=O({{L"revision",N(double(publication.Revision()))},
            {L"model_revision",N(double(publication.ModelRevision()))},
            {L"full_updates",N(double(fullUpdates))},{L"motion_updates",N(double(motionUpdates))},
            {L"shaders_ready",B(flag(data->model,L"shaders_ready"))},{L"checkpoint_camera",object(object(data->model,L"windows_recovery"),L"camera")},
            {L"color_field_pixels",N(double(data->colorFieldPixels))},
            {L"color_preview",object(data->colorPreview,L"picker").GetNamedValue(L"preview",JsonValue::CreateNullValue())},
            {L"workspace_update",workspaceUpdate},{L"groups",positions},{L"handles",grips},{L"elements",elements},
            {L"window_resizing",B(windowResizing)},{L"native_resize_pending",B(resizeHandback)},
            {L"snapshot_qpc",N(double(snapshotQpc.QuadPart))},{L"native_sizing_start_qpc",N(double(nativeSizingStartQpc))},{L"native_sizing_end_qpc",N(double(nativeSizingEndQpc))},
            {L"resize_layout_frames",O({{L"count",N(double(resizeFrames.size()+resizeFrameOverflow))},{L"overflow",N(double(resizeFrameOverflow))},{L"tolerance",N(.01)},{L"samples",resizeSamples}})},
            {L"resize_panel_bounds",O({{L"count",N(double(resizeBounds.size()+resizeBoundsOverflow))},{L"overflow",N(double(resizeBoundsOverflow))},{L"samples",boundSamples}})},
            {L"surface_frames",O({{L"frames",N(double(surfaceFrames))},{L"groups",N(double(surfaceGroups))},{L"mismatched_frames",N(double(surfaceMismatches))},
                {L"maximum_offset_px",N(maximumSurfaceOffset)},{L"tab_frames",N(double(tabFrames))},{L"escaped_label_frames",N(double(escapedLabels))},{L"first_mismatch",firstSurfaceMismatch}})},
            {L"panel_bridge_frame",panelBridgeFrame},{L"chrome_revision",N(double(data->chromeFrames->sentRevision))},
            {L"presented_chrome_revision",object(data->model,L"windows_glass").GetNamedValue(L"chrome_revision",JsonValue::CreateNullValue())},
            {L"overviews",data->chromeFrames->presented.overviews}}).Stringify();
        if(value!=lastPresentation){lastPresentation=value;AutomationProperties::SetItemStatus(root,value);}
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
    A measurementRequests()const{return array(object(data->model,L"layout"),L"measurement_requests");}
    bool needsPanelMeasurements()const{
        for(auto value:measurementRequests()){
            auto request=value.GetObject();bool visible=false;
            for(auto const& [id,group]:groups)if(!group.hidden&&!group.presented.Size())
                for(auto panel:array(group.geometry,L"panels"))if(panel.GetString()==str(request,L"panel"))visible=true;
            if(!visible)continue;
            if(!flag(request,L"tab_measured")||!flag(request,L"body_measured"))return true;
        }
        return false;
    }
    A collectPanelMeasurements(bool preparing=false){
        A reports;
        if((data->updating&&!preparing)||!root.IsLoaded()||root.ActualWidth()<=0)return reports;
        auto requests=measurementRequests();auto context=metricContext();
        for(auto it=offscreen.begin();it!=offscreen.end();){
            if(find(array(data->model,L"panels"),L"id",hstring(it->first)).Size()){++it;continue;}
            uint32_t at=0;if(it->second.body&&measureHost.Children().IndexOf(it->second.body->Root(),at))measureHost.Children().RemoveAt(at);
            it=offscreen.erase(it);
        }
        auto stable=[](double value){return std::round(std::clamp(value,0.,999999.)*64.)/64.;};
        for(auto const& [id,group]:groups){
            if(group.hidden||group.presented.Size()||(!preparing&&!group.frame.IsLoaded()))continue;
            auto active=str(group.geometry,L"active");
            for(auto value:array(group.geometry,L"panels")){
                auto panelId=value.GetString();auto request=find(requests,L"panel",panelId);
                if(!request.Size()||(flag(request,L"tab_measured")&&flag(request,L"body_measured")))continue;
                auto panel=find(array(data->model,L"panels"),L"id",panelId);if(!panel.Size())continue;
                V tabWidth=JsonValue::CreateNullValue(),body=JsonValue::CreateNullValue();
                if(!flag(request,L"tab_measured")){
                    auto found=group.tabs.find(std::wstring(panelId));
                    if(found!=group.tabs.end()){
                        auto tab=found->second.Children().GetAt(1).as<Button>();
                        tab.Measure({INFINITY,INFINITY});double width=tab.DesiredSize().Width;
                        for(auto const& automatic:group.automatic)if(automatic.tab==tab)
                            if(auto cached=tabWidths.find(automatic.key);cached!=tabWidths.end()){width=cached->second;break;}
                        if(width>0)tabWidth=N(stable(width));
                    }
                }
                if(!flag(request,L"body_measured")){
                    double width=num(request,L"width"),actualWidth=width;double height=-1;J scroll;
                    if(array(panel,L"tiles").Size())height=0;
                    else if(panelId==active){
                        actualWidth=group.body->Root().ActualWidth();
                        height=group.body->ContentHeight(actualWidth);scroll=group.body->ScrollMetrics();
                    }
                    else {
                        auto& copy=offscreen[std::wstring(panelId)];
                        auto key=std::wstring(str(request,L"body"))+std::wstring(panelMetricKey);
                        if(!copy.body||copy.key!=key){
                            if(copy.body){uint32_t at=0;if(measureHost.Children().IndexOf(copy.body->Root(),at))measureHost.Children().RemoveAt(at);}
                            auto inert=std::make_shared<WorkspaceData>(*data);inert->updating=false;
                            inert->send=[](std::string){};inert->document=[](std::string){};inert->input=[](std::string){};
                            inert->query=nullptr;inert->previews.reset();inert->thumbnails.reset();inert->scopes.reset();
                            inert->popupChanged=nullptr;inert->takeBubble=nullptr;inert->bubbleContact=nullptr;inert->edgeHolding=nullptr;inert->cancelEdge=nullptr;inert->pointerCancels.Clear();inert->transients.clear();inert->colorViews.clear();inert->copyViews.clear();
                            copy.body=std::make_unique<PanelBody>(inert,panel,O({{L"bounds",O({{L"width",N(width)}})}}),[]{},nullptr,false);
                            copy.data=inert;copy.key=key;copy.body->Root().Width(width);copy.body->Apply(true);measureHost.Children().Append(copy.body->Root());
                        }
                        if(copy.data){copy.data->state=data->state;copy.data->model=data->model;}
                        if(!flag(request,L"cacheable"))copy.body->Apply(true);
                        copy.body->Root().Width(width);
                        copy.body->Root().Measure({float(width),INFINITY});
                        copy.body->Root().Arrange({0,0,float(width),copy.body->Root().DesiredSize().Height});
                        actualWidth=copy.body->Root().ActualWidth();
                        height=copy.body->ContentHeight(actualWidth);scroll=copy.body->ScrollMetrics();
                    }
                    if(std::isfinite(height)&&height>=0)body=O({{L"actual_width",N(stable(actualWidth))},{L"content_height",N(stable(height))},
                        {L"scroll",scroll.Size()?V(scroll):JsonValue::CreateNullValue()}});
                }
                if(tabWidth.ValueType()!=JsonValueType::Null||body.ValueType()!=JsonValueType::Null)
                    reports.Append(O({{L"request",request},{L"context",context},{L"tab_width",tabWidth},{L"body",body}}));
            }
        }
        return reports;
    }
    J measurementBatch(A const& reports){return O({{L"context",metricContext()},{L"reports",reports}});}
    void publishPanelMeasurements(A const& reports){
        if(!reports.Size())return;
        auto batch=measurementBatch(reports);auto json=batch.Stringify();if(json==lastMeasurements)return;lastMeasurements=json;
        data->dispatch(O({{L"type",S(L"measure_panels")},{L"measurements",batch}}));
    }
    void measurePanels(){publishPanelMeasurements(collectPanelMeasurements());}
    void projectPanelMeasurements(A const& reports){
        auto batch=measurementBatch(reports);
        NativeLayoutInput measured(capy_layout_input_measure(layoutInput.get(),to_string(batch.Stringify()).c_str()));
        if(!measured)throw hresult_invalid_argument();
        layoutInput=std::move(measured);metricInput=layoutInput;
        resolveLayout();applyLayout(data->model,false);root.UpdateLayout();
    }
    void preparePanels(){
        if(nativeResize()||object(workspaceUpdate,L"drag").Size()||!root.IsLoaded()||root.ActualWidth()<=0||!needsPanelMeasurements())return;
        root.UpdateLayout();A reports;
        for(int pass=0;pass<2&&needsPanelMeasurements();++pass){
            auto measured=collectPanelMeasurements(true);if(!measured.Size())break;
            projectPanelMeasurements(measured);
            for(auto value:measured){
                auto report=value.GetObject();uint32_t replace=reports.Size();
                for(uint32_t at=0;at<reports.Size();++at)if(str(object(reports.GetObjectAt(at),L"request"),L"panel")==str(object(report,L"request"),L"panel")){replace=at;break;}
                if(replace<reports.Size())reports.SetAt(replace,report);else reports.Append(report);
            }
        }
        publishPanelMeasurements(reports);
    }
    void queueMeasurements(){
        measurementsPending=true;
        if(root.IsLoaded()&&glassChanged)glassChanged();
    }
    void layoutUpdated(){
        if(!measurementsPending||measuring||presenting||data->updating||!root.IsLoaded())return;
        measuring=true;measurementsPending=false;
        struct Reset{bool& value;~Reset(){value=false;}} reset{measuring};
        reportTitlebar();measurePanels();expansion->Apply(configurationHeight());
        presentBackgrounds();tracePresentation();
        if(!popup&&!str(object(data->state,L"customization"),L"control").empty())updatePopup();
    }
    void measured(){
        if(!presenting&&!data->updating&&root.IsLoaded()){presentBackgrounds();queueMeasurements();}
    }
    void present(){
        if(presenting)return;presenting=true;
        struct Reset{bool& flag;~Reset(){flag=false;}} reset{presenting};
        auto expanded=expansion->Geometry();
        for(auto& [id,group]:groups){
            bool active=expanded.Size()&&num(expanded,L"group")==id&&group.configuration;
            group.presented=active?expanded:J{};
            auto bounds=object(active?expanded:group.geometry,L"bounds");auto box=pixelBounds(rectangle(bounds),root);
            auto preview=active?object(expanded,L"preview"):rectangle({0,0,box.Width,box.Height});
            auto previewBox=pixelBounds(rectangle(preview),root);
            placeOnPixelGrid(group.frame,bounds,root);placeMotion(id,group);placeOnPixelGrid(group.border,preview,root);
            group.content.Width(box.Width);group.content.Height(box.Height);
            group.clip.Rect({0,0,box.Width,box.Height});
            group.fade->apply(group.frame,true,group.hidden,[weak=weak_from_this(),id,frame=make_weak(group.frame),shadow=make_weak(group.shadow.Root())]{auto self=weak.lock();auto view=frame.get();if(!self||!view)return;auto group=self->groups.find(id);if(group==self->groups.end()||group->second.frame!=view)return;if(auto surface=shadow.get())surface.Opacity(group->second.gpu||!group->second.bridge.empty()?0:view.Opacity());if(view.Opacity()==0&&group->second.body)group->second.body->Apply(false);self->queueMeasurements();});
            if(group.hidden&&group.frame.Opacity()==0&&group.body)group.body->Apply(false);
            Canvas::SetZIndex(group.frame,active?1000:group.order);
            group.shadow.Layout(box,active?1000:group.order,group.frame.Visibility()==Visibility::Visible);group.shadow.Root().Opacity(group.gpu||!group.bridge.empty()?0:group.frame.Opacity());
            auto geometry=group.geometry;
            if(active){
                geometry=J::Parse(group.geometry.Stringify());geometry.Insert(L"bounds",preview);
                if(expanded.HasKey(L"tiles"))geometry.Insert(L"tiles",object(expanded,L"tiles"));
            }
            double bodyHeight=std::max(0.,double(previewBox.Height)-(flag(geometry,L"tabs_visible")?data->panelMetric(L"tab_bar_height"):0.)
                -num(object(geometry,L"footer_grip"),L"height"));
            double minimum=object(geometry,L"tiles").Size()?0.:num(geometry,L"min_body_height");
            if(group.bodyScroll){
                group.bodyScroll.Height(bodyHeight);
                if(auto content=group.bodyScroll.Content().try_as<FrameworkElement>())content.MinHeight(minimum);
            }
            group.body->Root().Width(previewBox.Width);group.body->Layout(geometry);
            if(group.footer){
                auto grip=rectangle(object(group.geometry,L"footer_grip"));
                grip.X=0;grip.Y=0;grip.Width=previewBox.Width;
                place(group.footer,rectangle(grip));
            }
            if(active){
                auto configuration=object(expanded,L"configuration");auto configBox=pixelBounds(rectangle(configuration),root);
                bool left=configBox.X<previewBox.X;bool tabs=configBox.Y>0;
                auto radius=groupRadius(group.geometry)*CornerFit;group.border.CornerRadius(left?CornerRadius{tabs?radius:0.,radius,radius,0}:CornerRadius{radius,tabs?radius:0.,0,radius});
                group.configurationFrame.CornerRadius(left?CornerRadius{radius,0,0,radius}:CornerRadius{0,radius,radius,0});
                placeOnPixelGrid(group.configurationFrame,configuration,root);
                group.configurationFrame.Opacity(1);group.configurationFrame.IsHitTestVisible(!group.hidden);
                group.configuration->SetVisible(!group.hidden);
                AutomationProperties::SetItemStatus(group.configuration->Root(),expanded.Stringify());
            }else{
                // Measure full natural content before the first shared geometry reply.
                place(group.configurationFrame,rectangle({box.Width,0,std::min(float(data->panelMetric(L"configuration_width")),float(root.ActualWidth())),box.Height}));
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
        if(!data->updating)presentBackgrounds();queueMeasurements();
    }
    void appendGroupOverviews(A& slots,Group const& group){
        if(!(group.body&&group.body->navigator)&&!(group.presented.Size()&&group.configuration))return;
        if(!group.frame.IsLoaded()||group.frame.Visibility()!=Visibility::Visible||group.frame.Opacity()<=0)return;
        auto start=slots.Size();auto clip=visibleBounds(group.frame,root);int order=visualOrder(root,group.frame);
        if(group.body&&group.body->navigator){
            auto visible=intersect(clip,visibleBounds(group.body->navigator->Root(),root));
            auto slot=group.body->navigator->Placement(root,visible,order,true);
            if(slot.Size())slots.Append(slot);
        }
        if(group.presented.Size()&&group.configuration)group.configuration->AppendOverviews(slots,root,clip,order);
        for(uint32_t i=start;i<slots.Size();++i)slots.GetObjectAt(i).Insert(L"opacity",N(group.frame.Opacity()));
    }
    static float groupRadius(J const& geometry){
        auto tiles=object(geometry,L"tiles");
        return tiles.Size()&&!flag(geometry,L"tabs_visible")?float(num(tiles,L"tile_corner_radius",SurfaceRadius)):SurfaceRadius;
    }
    bool chromeReady()const{return data->chromeFrames->sentRevision&&data->chromeFrames->revision==data->chromeFrames->sentRevision;}
    bool panelBridgeGroup(uint32_t id)const{return data->chromeFrames->Group(id);}
    void measurePanelBridges(){
        if(!layoutInput||!root.IsLoaded()||viewportWidth<=0||viewportHeight<=0)return;
        A allocated;
        for(auto const& [id,group]:groups){
            if(group.hidden||group.presented.Size()||group.frame.Opacity()<=0)continue;
            auto bounds=placement(group);if(bounds.Width<=0||bounds.Height<=0)continue;
            A pair;pair.Append(N(id));pair.Append(rectangle(bounds));allocated.Append(pair);
        }
        auto key=O({{L"groups",allocated},{L"columns",collapsed->Geometry()}}).Stringify();
        if(key==panelBridgeKey&&panelQueryLayoutRevision==panelLayoutRevision)return;
        auto request=to_string(key);
        auto result=capy_panel_bridges(layoutInput.get(),viewportWidth,viewportHeight,request.c_str());
        NativeString reply(result,capy_string_free);
        J packet;if(reply)J::TryParse(to_hstring(reply.get()),packet);
        auto frame=object(packet,L"result");
        if(!frame.Size()){
            panelBridgeKey=L"";
            OutputDebugStringW((L"Capy panel bridge projection failed: "+str(packet,L"error")+L"\n").c_str());
            return;
        }
        panelBridgeKey=key;panelQueryLayoutRevision=panelLayoutRevision;
        if(frame.Stringify()!=panelBridgeFrame.Stringify()){panelBridgeFrame=frame;++panelBridgeRevision;}
    }
    A panelColor(Windows::UI::Color color,double opacity=1)const{
        if(windowResizing&&!resizeTintFrame)color={0,0,0,0};
        A result;for(auto value:{color.R/255.,color.G/255.,color.B/255.,color.A/255.*opacity})result.Append(N(value));return result;
    }
    A panelSurfaces(UIElement const& reference){
        publishedBridgeGroups.clear();publishedBridgeColumns.clear();publishedBridgeGroups.reserve(groups.size());publishedBridgeColumns.reserve(array(panelBridgeFrame,L"columns").Size());
        A surfaces;auto offset=root.TransformToVisual(reference).TransformPoint({0,0});
        for(auto value:array(panelBridgeFrame,L"columns")){
            auto column=value.GetObject();double id=num(column,L"column");A bridges;
            for(auto bridgeValue:array(panelBridgeFrame,L"bridges")){auto bridge=bridgeValue.GetArray();if(bridge.GetNumberAt(0)==id)bridges.Append(bridge.GetObjectAt(2));}
            if(!bridges.Size())continue;
            A receivers;
            auto expected=array(bridges.GetObjectAt(0),L"receivers");
            for(auto receiverValue:expected){auto receiver=receiverValue.GetObject();auto found=groups.find(uint32_t(num(receiver,L"group")));if(found==groups.end())continue;
                auto const& group=found->second;if(group.hidden||group.presented.Size()||!group.frame.IsLoaded()||group.frame.Visibility()!=Visibility::Visible||group.frame.Opacity()<=0)continue;
                auto rect=[](Rect bounds){A values;for(auto v:{bounds.X,bounds.Y,bounds.Width,bounds.Height})values.Append(N(v));return values;};
                bool tabbed=flag(group.geometry,L"tabs_visible");auto bounds=placement(group);
                auto slot=[&](FrameworkElement const& item){if(!tabbed)return Rect{};auto result=allocatedBounds(item,group.frame);result.X+=bounds.X;result.Y+=bounds.Y;return result;};
                auto bar=tabbed?Rect{bounds.X,bounds.Y,bounds.Width,float(data->panelMetric(L"tab_bar_height"))}:Rect{};
                auto activeTab=tabbed?gestures->TabBounds(uint32_t(num(receiver,L"group")),group.frame).value_or(allocatedBounds(group.activeTab,group.frame)):Rect{};
                if(tabbed){activeTab.X+=bounds.X;activeTab.Y+=bounds.Y;}activeTab.X+=gestures->TabOffset(uint32_t(num(receiver,L"group"))).first;
                receivers.Append(O({{L"group",N(num(receiver,L"group"))},
                    {L"tab_bar",rect(bar)},
                    {L"active_tab",rect(activeTab)},
                    {L"tab_clip",rect(slot(group.tabScroll))},
                    {L"color",panelColor(data->glass(L"panel").Color(),group.frame.Opacity())},
                    {L"strip_color",panelColor(data->glass(L"strip").Color(),group.frame.Opacity())}}));
            }
            if(receivers.Size()!=expected.Size())continue;
            publishedBridgeColumns.push_back(uint32_t(id));for(auto item:receivers)publishedBridgeGroups.push_back(uint32_t(num(item.GetObject(),L"group")));
            auto b=rectangle(object(column,L"bounds"));A sidebar,translation;
            for(auto v:{b.X,b.Y,b.Width,b.Height})sidebar.Append(N(v));translation.Append(N(offset.X));translation.Append(N(offset.Y));
            surfaces.Append(O({{L"offset",translation},{L"sidebar",sidebar},{L"radii",array(column,L"radii")},
                {L"color",panelColor(data->glass(L"panel").Color(),collapsed->Opacity(uint32_t(id)))},{L"bridges",bridges},{L"groups",receivers}}));
        }
        return surfaces;
    }
    void presentBackgrounds(){
        measurePanelBridges();presentPanelBridges();
        auto document=data->chromeFrames->presented.document;
        for(auto& [id,group]:groups){
            if(group.hidden)continue;
            A slots;for(auto value:data->chromeFrames->presented.overviews)if(num(value.GetObject(),L"order")>=visualOrder(root,group.frame)&&num(value.GetObject(),L"opacity",1)>0)slots.Append(value);
            bool connected=nativeResize()&&!group.bridge.empty();
            bool expanded=group.presented.Size()!=0,tabbed=flag(group.geometry,L"tabs_visible");
            auto bounds=rectangle(placement(group));
            J source;for(auto value:data->drawerSources){auto anchor=object(value.GetObject(),L"anchor");
                if(str(anchor,L"kind")==L"tile"&&str(anchor,L"panel")==str(group.geometry,L"active"))source=value.GetObject();}
            float radius=groupRadius(group.geometry);
            std::array<float,4> corners{radius,radius,radius,radius};
            if(source.Size()&&!expanded){auto square=sourceCorners(data,source,bounds);for(int i=0;i<4;++i)if(square[i])corners[i]=0;}
            for(auto value:array(panelBridgeFrame,L"groups")){auto geometry=value.GetObject();if(num(geometry,L"group")!=id)continue;
                auto radii=array(geometry,L"radii");for(uint32_t i=0;i<4;++i)corners[i]=std::min(corners[i],float(radii.GetNumberAt(i)));break;}
            A radii;for(auto r:corners)radii.Append(N(r));
            auto key=O({{L"expansion",group.presented},{L"tabbed",B(tabbed)},{L"source",source},{L"transparent",B(data->transparent())},{L"bounds",bounds},{L"slots",slots},
                {L"glass",object(object(data->state,L"palette"),L"glass")},{L"scale",N(root.XamlRoot()?root.XamlRoot().RasterizationScale():1)},
                {L"radius",N(radius)},{L"corners",radii},{L"bridge",S(group.bridgeKey)},{L"native_resize",B(nativeResize())},{L"width",N(num(document,L"width"))},{L"height",N(num(document,L"height"))}}).Stringify();
            if(key==group.backgroundKey)continue;group.backgroundKey=key;
            group.corners=corners;CornerRadius rounded{corners[0]*CornerFit,corners[1]*CornerFit,corners[2]*CornerFit,corners[3]*CornerFit};group.frame.CornerRadius(nativeResize()?CornerRadius{}:rounded);if(!expanded)group.border.CornerRadius(rounded);
            float width=float(num(bounds,L"width")),height=float(num(bounds,L"height"));
            GeometryGroup shape{nullptr};
            if(expanded){shape=GeometryGroup();shape.FillRule(FillRule::EvenOdd);shape.Children().Append(expansionShape(group.presented));}
            cutStrip(group);
            float top=tabbed&&!expanded&&!connected?float(data->panelMetric(L"tab_bar_height")):0;
            Canvas::SetTop(group.background.Root(),top);
            auto color=(expanded?data->brush(L"panel"):data->glass(source.Size()?L"source":L"panel")).Color();
            if(group.header)group.header.Background(expanded?data->brush(L"tabbar"):clear());
            if(group.activeTab)for(auto layer:group.activeTab.Children())if(auto canvas=layer.try_as<Canvas>())for(auto item:canvas.Children())if(auto path=item.try_as<Microsoft::UI::Xaml::Shapes::Path>())
                path.Fill(expanded?Brush(data->brush(L"panel")):Brush(nullptr));
            auto shadowKey=O({{L"expansion",group.presented},{L"corners",radii},{L"radius",N(radius)},{L"width",bounds.GetNamedValue(L"width")},{L"height",bounds.GetNamedValue(L"height")}}).Stringify();
            if(shadowKey!=group.shadowKey){
                group.shadowKey=shadowKey;
                auto style=panelShadowStyle(data,expanded);
                if(expanded){group.shadow.Shape(expansionShape(group.presented),width,height,style.blur,style.offset,style.opacity);group.shadow.Uncut();}
                else group.shadow.Rounded(width,height,corners,style.blur,style.offset,style.opacity);
            }
            std::vector<Rect> holes;
            for(auto value:slots){
                auto slot=value.GetObject();auto area=array(slot,L"bounds"),clip=array(slot,L"clip");float image[4]{};
                if(area.Size()!=4||clip.Size()!=4||!capy_navigator_image(float(area.GetNumberAt(2)),float(area.GetNumberAt(3)),
                    uint32_t(num(document,L"width")),uint32_t(num(document,L"height")),image))continue;
                Rect hole{float(area.GetNumberAt(0))+image[0],float(area.GetNumberAt(1))+image[1],image[2],image[3]};
                hole=intersect(hole,{float(clip.GetNumberAt(0)),float(clip.GetNumberAt(1)),float(clip.GetNumberAt(2)),float(clip.GetNumberAt(3))});
                if(hole.Width<=0||hole.Height<=0)continue;
                hole.X-=float(num(bounds,L"x"));hole.Y-=float(num(bounds,L"y"));
                if(expanded){RectangleGeometry cutout;cutout.Rect(hole);shape.Children().Append(cutout);}
                else{hole.Y-=top;holes.push_back(hole);}
            }
            if(expanded)group.background.Shape(shape,color);
            else if(connected){
                if(tabbed)holes.push_back({0,0,width,float(data->panelMetric(L"tab_bar_height"))});
                group.background.Contour(width,height,group.bridge,color,std::move(holes));
            }else{
                auto bodyRadii=tabbed?std::array<float,4>{0,0,corners[2],corners[3]}:corners;
                group.background.Rounded(width,std::max(0.f,height-top),bodyRadii,color,std::move(holes));
            }
        }
    }
    void presentPanelBridges(){
        if(resizeHandback&&!windowResizing&&chromeReady()){
            resizeHandback=false;for(auto& [id,group]:groups)group.backgroundKey=L"";
        }
        collapsed->PresentBridges(panelBridgeFrame,nativeResize());
        for(auto& [id,group]:groups){
            J path;
            if(nativeResize()&&!group.hidden&&!group.presented.Size())for(auto value:array(panelBridgeFrame,L"paths"))
                if(num(value.GetObject(),L"group")==id){path=value.GetObject();break;}
            auto key=path.Stringify();
            if(key!=group.bridgeKey){group.bridgeKey=key;group.bridge=bridgeCommands(array(path,L"commands"));group.backgroundKey=L"";}
            group.gpu=!nativeResize()&&!group.presented.Size()&&panelBridgeGroup(id);
            group.background.Root().Opacity(group.gpu?0:1);group.strip.Root().Opacity(group.gpu?0:1);
            group.shadow.Root().Opacity(group.gpu||!group.bridge.empty()?0:group.frame.Opacity());
        }
    }
    void cutStrip(uint32_t id){if(auto found=groups.find(id);found!=groups.end())cutStrip(found->second);}
    void cutStrip(Group& group){
        auto [offset,animate]=gestures->TabOffset(uint32_t(num(group.geometry,L"id")));group.strip.TranslateTab(offset,animate);
        std::vector<Point> band,active;float width=placement(group).Width,clipX=0,clipWidth=width;
        if(flag(group.geometry,L"tabs_visible")&&!group.presented.Size()&&width>0){
            band=squircleOutline(width,float(data->panelMetric(L"tab_bar_height")),{group.corners[0],group.corners[1],0,0});
            if(auto tab=group.activeTab;tab&&tab.ActualWidth()>0){
                auto view=group.tabScroll.TransformToVisual(group.content).TransformPoint({0,0});
                auto bounds=gestures->TabBounds(uint32_t(num(group.geometry,L"id")),group.content).value_or(allocatedBounds(tab,group.content));
                auto outline=panelTabOutline(bounds.Width,bounds.Height);
                for(auto& point:outline){point.X+=bounds.X-6;point.Y+=bounds.Y;}
                active=std::move(outline);clipX=view.X;clipWidth=float(group.tabScroll.ActualWidth());
            }
        }
        auto strip=data->glass(L"strip").Color(),panel=data->glass(L"panel").Color();
        group.strip.Present(width,float(data->panelMetric(L"tab_bar_height")),std::move(band),std::move(active),strip,panel,clipX,clipWidth);
    }
    void resizeHandle(std::wstring const& key,J const& bounds,J const& action,int z,bool resetColumn=false){
        auto [it,added]=handles.try_emplace(key);
        auto handle=it->second;
        if(added){
            Border fill;fill.Background(clear());handle.Content(fill);handle.Padding({0,0,0,0});handle.BorderThickness({0,0,0,0});
            handle.HorizontalContentAlignment(HorizontalAlignment::Stretch);handle.VerticalContentAlignment(VerticalAlignment::Stretch);
            handle.IsTabStop(true);handle.UseSystemFocusVisuals(true);
            handle.RenderTransform(TranslateTransform());root.Children().Append(handle);
            copyName(data,handle,data->copyDelivery(L"resize_dock"));
        }
        auto transform=handle.RenderTransform().as<TranslateTransform>();transform.X(0);transform.Y(0);
        place(handle,bounds);Canvas::SetZIndex(handle,z);gestures->Source(handle,action,{},resetColumn);
        if(auto shape=resizeCursor(action,object(data->model,L"layout"))){
            using namespace winrt::Microsoft::UI::Input;
            handle.as<IUIElementProtected>().ProtectedCursor(InputSystemCursor::Create(*shape));
        }
        AutomationProperties::SetAutomationId(handle,hstring(key));
        AutomationProperties::SetAutomationControlType(handle,Automation::Peers::AutomationControlType::Thumb);
    }
    void fitTabs(uint32_t id){if(auto found=groups.find(id);found!=groups.end()&&found->second.tabScroll)fitAutomaticTabs(data,found->second.automatic,found->second.tabScroll.ActualWidth(),tabWidths);}
    void publishDrawerSources(){
        if(drawers){
            auto sources=drawers->Sources();
            if(sources.Stringify()!=data->drawerSources.Stringify()){
                data->drawerSources=sources;
                for(auto& [id,group]:groups){group.backgroundKey=L"";if(group.body)group.body->Apply(!group.hidden);}
                collapsed->Apply();
                queueMeasurements();
            }
        }
    }
    A glass(UIElement const& reference,A& connections,A& arcs,A& colors,A& panels,A& overviews){
        auto& frame=data->chromeFrames->current;frame=ChromeFrames::Frame{};
        auto tabs=array(data->state,L"tabs");frame.document=tabs.Size()?tabs.GetObjectAt(0):J{};
        for(auto const& [id,group]:groups)if(!group.hidden&&group.frame.IsLoaded()&&group.frame.Visibility()==Visibility::Visible){
            data->chromeFrames->Track(group.frame);data->chromeFrames->Track(group.shadow.Root());appendGroupOverviews(frame.overviews,group);
        }
        if(drawers)drawers->AppendOverviews(frame.overviews);
        auto transform=root.TransformToVisual(reference);
        for(auto value:frame.overviews){auto slot=value.GetObject();auto copy=J::Parse(slot.Stringify());
            for(auto name:{L"bounds",L"clip"}){auto a=array(slot,name);auto b=transform.TransformBounds({float(a.GetNumberAt(0)),float(a.GetNumberAt(1)),float(a.GetNumberAt(2)),float(a.GetNumberAt(3))});
                A coordinates;for(auto v:{b.X,b.Y,b.Width,b.Height})coordinates.Append(N(v));copy.Insert(name,coordinates);
            }overviews.Append(copy);
        }
        panels=panelSurfaces(reference);
        if(!windowResizing||resizeTintFrame){frame.groups.insert(publishedBridgeGroups.begin(),publishedBridgeGroups.end());frame.columns.insert(publishedBridgeColumns.begin(),publishedBridgeColumns.end());}
        presentPanelBridges();
        A regions;
        for(auto const& [id,group]:groups)if(std::find(publishedBridgeGroups.begin(),publishedBridgeGroups.end(),id)==publishedBridgeGroups.end()&&group.frame.Visibility()==Visibility::Visible&&!group.presented.Size())appendGlass(regions,group.frame,reference,{group.corners[0],group.corners[1],group.corners[2],group.corners[3]},true);
        collapsed->AppendGlass(regions,reference,publishedBridgeColumns);drawers->AppendGlass(regions,connections,reference);
        appendGlass(regions,cameraSurface,reference,cornerRadii(cameraSurface.CornerRadius()));
        canvasBar->AppendGlass(regions,reference);notice->AppendGlass(regions,reference);commandSearch->AppendGlass(regions,reference);
        bubbles->Glass(regions,arcs,colors,reference);
        return regions;
    }
    void placePreviews(){
        auto bar=object(data->chrome,L"canvas_bar");
        for(auto const& panel:previewPanels)panel->Bar(bar);
        placeNotice();
    }
    void placeNotice(){
        for(auto const& panel:previewPanels)if(auto bounds=panel->Bounds();bounds.Size()){notice->Bar(bounds);return;}
        notice->Bar(object(data->chrome,L"canvas_bar"));
    }
    void updateCamera(J const& view){
        if(double revision=num(view,L"revision",-1);revision!=cameraRevision){cameraRevision=revision;canvasBar->Defer();}
        zoom->Update(view);
    }
};
WorkspaceView::WorkspaceView(Dispatch send,Json catalog,NativeLocalization localization,PreviewTransport previews,std::function<void(bool)> popupChanged,Dispatch document,Dispatch input,EdgeTransport edges):impl(std::make_shared<Impl>(std::move(send),catalog,localization,std::move(previews),std::move(popupChanged),std::move(document),std::move(input),std::move(edges))){impl->init();}
WorkspaceView::~WorkspaceView(){impl->gestures->Cancel();impl->data->chromeFrames->Close();}
void WorkspaceView::SetCommit(std::function<void(std::string,bool,std::function<void()>)> commit){impl->data->commit=std::move(commit);}
Canvas WorkspaceView::Root()const{return impl->root;}
bool WorkspaceView::Apply(Json const& snapshot,NativeLayoutInput input){return impl->apply(snapshot,std::move(input));}
bool WorkspaceView::ApplyFields(Json const& snapshot,bool scopes){return impl->applyFields(snapshot,scopes);}
void WorkspaceView::Resize(float width,float height){impl->resize(width,height);}
void WorkspaceView::SetWindowResizing(bool value){
    if(impl->windowResizing==value)return;
    if(impl->trace){
        LARGE_INTEGER now{};QueryPerformanceCounter(&now);
        if(value){impl->nativeSizingStartQpc=now.QuadPart;impl->nativeSizingEndQpc=0;}
        else impl->nativeSizingEndQpc=now.QuadPart;
    }
    if(value&&impl->trace){
        impl->resizeFrames.clear();impl->resizeFrames.reserve(4096);impl->resizeFrameOverflow=0;
        impl->resizeBounds.clear();impl->resizeBounds.reserve(32768);impl->resizeBoundsOverflow=0;
    }
    impl->windowResizing=value;impl->resizeHandback=!value;
    impl->resizeTintFrame.revoke();
    if(value)impl->resizeTintFrame=CompositionTarget::Rendered(auto_revoke,[weak=std::weak_ptr<Impl>(impl)](auto&&,auto&&){
        if(auto self=weak.lock()){self->resizeTintFrame.revoke();self->queueMeasurements();}
    });
    else impl->data->chromeFrames->sentRevision=0;
    ++impl->panelBridgeRevision;
    for(auto& [id,group]:impl->groups)group.backgroundKey=L"";
    impl->presentBackgrounds();impl->tracePresentation();impl->queueMeasurements();
}
WorkspaceView::Json WorkspaceView::ChromeFacts(bool popupOpen){impl->data->externalPopup=popupOpen;return J::Parse(impl->data->chrome.Stringify());}
void WorkspaceView::CancelPreviews(){for(auto const& panel:impl->previewPanels)panel->Cancel();}
bool WorkspaceView::CancelGesture(){
    bool closed=impl->data->dismissTransients();
    closed=impl->bubbles->Cancel()||closed;
    return impl->gestures->Cancel()||closed;
}
bool WorkspaceView::CancelNativeContacts(){return impl->data->pointerCancels.CancelAll();}
void WorkspaceView::SetWindowId(uint64_t id){impl->data->windowId=id;}
void WorkspaceView::SetGlassChanged(std::function<void()> changed){impl->glassChanged=std::move(changed);}
void WorkspaceView::CanvasContact(bool active){impl->canvasBar->Contact(active);impl->bubbles->Wake();if(active){impl->notice->Hide();hideRevealedTooltip();}}
void WorkspaceView::AttachOverlays(UIElement const& root,std::function<void()> cancelHeader){
    auto weak=std::weak_ptr<Impl>(impl);root.as<Panel>().Children().Append(impl->utilities);Canvas::SetZIndex(impl->utilities,1100);impl->pressure->attach(impl->utilities);
    auto sync=[weak](auto&&,auto&&){if(auto self=weak.lock()){self->utilities.Visibility(self->root.Visibility());self->utilities.IsHitTestVisible(self->root.IsHitTestVisible());}};
    impl->root.RegisterPropertyChangedCallback(UIElement::VisibilityProperty(),sync);impl->root.RegisterPropertyChangedCallback(UIElement::IsHitTestVisibleProperty(),sync);sync(nullptr,nullptr);
    impl->bubbles->Attach(root,[weak,cancelHeader]{if(auto self=weak.lock())self->gestures->Cancel();cancelHeader();});
}
JsonArray WorkspaceView::DrawerSources()const{return impl->data->drawerSources;}
JsonArray WorkspaceView::OverlayInputRegions(UIElement const& reference)const{auto result=impl->bubbles->InputRegions(reference);for(auto value:impl->pressure->inputRegions(reference))result.Append(value);return result;}
JsonArray WorkspaceView::Glass(UIElement const& reference,JsonArray& connections,JsonArray& arcs,JsonArray& colors,JsonArray& panels,JsonArray& overviews)const{return impl->glass(reference,connections,arcs,colors,panels,overviews);}
void WorkspaceView::MeasureChrome(){impl->layoutUpdated();}
void WorkspaceView::ChromeSent(uint64_t revision){if(impl->data->chromeFrames->Sent(revision)||impl->resizeHandback){impl->presentBackgrounds();impl->drawers->PresentChrome();impl->overviewOcclusion.Apply(impl->root,impl->data->chromeFrames->presented.overviews,impl->data->chromeFrames->presented.document);}}
void WorkspaceView::SetTitlebarInsets(float left,float right,float height){
    std::array<float,3> value{left,right,height};
    if(value!=impl->titlebar){impl->titlebar=value;impl->lastTitlebar=L"";impl->reportTitlebar();}
}

void WorkspaceView::MergeLayout(Json const& model,Json const& patch){
    using namespace CapyUi;
    for(auto field:{L"layout",L"layout_input",L"panel_measurements",L"workspace_update",L"windows_tab_styles",L"windows_glass"})
        if(patch.HasKey(field))model.Insert(field,patch.GetNamedValue(field));
    auto state=object(model,L"state"),layout=object(object(state,L"workspace"),L"layout");
    for(auto const& item:object(patch,L"workspace_layout"))layout.Insert(item.Key(),item.Value());
    if(patch.HasKey(L"camera"))state.Insert(L"camera",patch.GetNamedValue(L"camera"));
}
