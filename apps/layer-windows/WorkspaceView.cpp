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
#include "PanelConfiguration.h"
#include "WorkspaceExpansion.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceTabSurface.h"
#include "WorkspacePublication.h"
#include "OverviewOcclusion.h"
#include "WorkspaceShadow.h"
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
    struct Measurement {double tab=36,content=320;J scroll;};
    std::map<std::wstring,Measurement> measurements;
    struct Offscreen {std::wstring key;std::shared_ptr<WorkspaceData> data;std::unique_ptr<PanelBody> body;uint64_t publication=0;};
    std::map<std::wstring,Offscreen> offscreen;
    Canvas measureHost;
    hstring lastMeasurements;
    CompositionTarget::Rendering_revoker measurementFrame;
    std::array<float,3> titlebar{};
    hstring lastTitlebar;
    bool presenting=false;
    WorkspacePublication publication;
    std::optional<uint32_t> movingGroup;
    bool trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    uint64_t fullUpdates=0,motionUpdates=0;
    J workspaceUpdate;
    hstring lastPresentation;
    OverviewOcclusion overviewOcclusion;
    std::map<std::wstring,ContentControl> handles;
    Dispatch overviews;
    hstring lastOverviews;
    struct Group {
        Border frame,border,configurationFrame;
        std::shared_ptr<ChromeVisibility> fade=std::make_shared<ChromeVisibility>();
        WorkspaceShadow shadow;
        hstring shadowKey;
        Canvas content;
        Grid layout{nullptr},header{nullptr},bodyViewport{nullptr};
        StackPanel tabLabels{nullptr};
        Grid activeTab{nullptr};
        std::vector<AutomaticTab> automatic;
        Bindings paintIcons;
        ScrollView tabScroll{nullptr};
        ScrollView bodyScroll{nullptr};
        Microsoft::UI::Xaml::Shapes::Path background;
        WorkspaceTabSurface strip;
        std::array<float,4> corners{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
        Border footer{nullptr};
        std::wstring key;
        hstring tabKey;
        J geometry,presented;
        Windows::Foundation::Point offset{};
        TranslateTransform translation;
        RectangleGeometry clip;
        hstring backgroundKey;
        std::unique_ptr<PanelBody> body;
        std::map<std::wstring,Grid> tabs;
        std::unique_ptr<PanelConfiguration> configuration;
        bool hidden=false,gpu=false;
        int order=0;
    };
    std::map<uint32_t,Group> groups;
    hstring panelBridgeKey,panelBridgeContext,lastPanelSurfaces;
    J panelBridgeFrame;
    uint64_t panelBridgeRevision=0;
    std::shared_ptr<CommandSearchPopup> commandSearch=std::make_shared<CommandSearchPopup>();
    std::shared_ptr<CanvasActionBar> canvasBar=std::make_shared<CanvasActionBar>();
    std::shared_ptr<CanvasNotice> notice=std::make_shared<CanvasNotice>();
    std::shared_ptr<ColorStrip> colorStrip=std::make_shared<ColorStrip>();
    std::set<uint32_t> handledColorRequests;
    std::array<std::shared_ptr<PreviewPanel>,2> previewPanels{std::make_shared<PreviewPanel>(),std::make_shared<PreviewPanel>()};
    std::shared_ptr<ResponseCalibration> pressure=std::make_shared<ResponseCalibration>();
    double cameraRevision=-1;
    hstring previousTheme,previousPalette;
    Flyout popup{nullptr};
    std::wstring popupControl;
    std::shared_ptr<uint64_t> popupGeneration=std::make_shared<uint64_t>(0);
    Bindings popupBindings;
    std::shared_ptr<ZoomReadout> zoom=std::make_shared<ZoomReadout>();
    Grid cameraSlot;
    Border cameraSurface;std::function<void()> glassChanged;std::map<std::wstring,double> tabWidths;
    Impl(Dispatch send,J catalog,NativeLocalization localization,Dispatch report,PreviewTransport previews,std::function<void(bool)> popupChanged,Dispatch document,Dispatch input,EdgeTransport edges):overviews(std::move(report)){
        data->input=std::move(input);data->edges=std::move(edges);gestures=std::make_shared<WorkspaceGestures>(data,root);
        data->document=std::move(document);
        data->popupChanged=std::move(popupChanged);
        data->thumbnails=CreateLayerThumbnailCache(previews);
        data->query=previews;data->previews=CreateFilterPreviewCache(std::move(previews));
        data->localization=localization;data->send=std::move(send);data->catalog=catalog;data->glassSurfaces=true;
        AutomationProperties::SetName(root,str(object(data->catalog,L"bootstrap"),L"drawing_workspace"));
    }
    void init(){
        data->strokes=std::make_shared<StrokeRecording>();data->strokes->data=data;data->strokes->start();
        data->colorsPresented=[weak=weak_from_this()]{if(auto self=weak.lock())self->tracePresentation();};
        zoom->data=data;zoom->init();
        auto surface=cameraSurface;surface.Child(zoom->root);surface.Background(headerSurface(data));
        surface.SizeChanged([button=zoom->root](auto&& sender,SizeChangedEventArgs const& e){
            double r=e.NewSize().Height*.5*CornerFit;sender.template as<Border>().CornerRadius({r,r,r,r});button.CornerRadius({r,r,r,r});
        });
        surface.HorizontalAlignment(HorizontalAlignment::Right);surface.VerticalAlignment(VerticalAlignment::Bottom);surface.Margin({4,0,4,0});
        cameraSlot.Children().Append(surface);
        canvasBar->data=data;
        canvasBar->changed=[weak=weak_from_this()]{if(auto self=weak.lock()){self->gestures->ChromeChanged();self->placePreviews();if(self->glassChanged)self->glassChanged();}};
        canvasBar->init(root);
        notice->data=data;notice->init(root);
        pressure->data=data;pressure->init(root);
        colorStrip->data=data;colorStrip->init(root);data->colorStrip=colorStrip;
        for(auto [panel,kind]:{std::pair{previewPanels[0],PreviewPanel::SelectionRefine},std::pair{previewPanels[1],PreviewPanel::FrequencySeparation}}){
            panel->data=data;panel->kind=kind;panel->moved=[weak=weak_from_this()]{if(auto self=weak.lock())self->placeNotice();};panel->init(root);
        }
        collapsed=std::make_unique<CollapsedColumns>(data,root,gestures,[weak=weak_from_this()]{if(auto self=weak.lock();self&&self->glassChanged)self->glassChanged();});
        commandSearch->data=data;commandSearch->changed=[weak=weak_from_this()]{if(auto self=weak.lock();self&&self->glassChanged)self->glassChanged();};commandSearch->init(root);
        drawers=std::make_unique<WorkspaceDrawers>(data,root,gestures,[weak=weak_from_this()]{if(auto self=weak.lock())self->queueMeasurements();});
        expansion=std::make_unique<WorkspaceExpansion>(data,root,gestures,[weak=weak_from_this()]{if(auto self=weak.lock())self->present();});
        bubbles=std::make_unique<WorkspaceBubbles>(data,root,[weak=weak_from_this()]{if(auto self=weak.lock();self&&self->glassChanged)self->glassChanged();});
        measureHost.IsHitTestVisible(false);measureHost.Opacity(0);Canvas::SetLeft(measureHost,-100000);root.Children().Append(measureHost);
        root.LayoutUpdated([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->measured();});
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
    void removeGroup(Group const& group){
        uint32_t index;if(root.Children().IndexOf(group.frame,index))root.Children().RemoveAt(index);
        if(root.Children().IndexOf(group.shadow.Root(),index))root.Children().RemoveAt(index);
    }
    void build(Group& group,J const& geometry,J const& panel){
        group.body.reset();group.footer=nullptr;group.backgroundKey=L"";
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
        frame.RowDefinitions().GetAt(0).Height({flag(geometry,L"tabs_visible")?36.:0.,GridUnitType::Pixel});
        frame.RowDefinitions().GetAt(2).Height({num(object(geometry,L"footer_grip"),L"height"),GridUnitType::Pixel});
        while(frame.Children().Size()>1)frame.Children().RemoveAtEnd();
        A members;for(auto value:array(geometry,L"panels")){
            auto model=find(array(data->model,L"panels"),L"id",value.GetString());
            members.Append(O({{L"id",value},{L"icon",S(str(model,L"icon"))}}));
        }
        auto tabKey=O({{L"members",members},{L"style",S(str(object(data->model,L"windows_tab_styles"),to_hstring(uint32_t(num(geometry,L"id"))).c_str()))}}).Stringify();
        if(group.tabKey!=tabKey){
            group.tabKey=tabKey;group.tabs.clear();group.activeTab=nullptr;group.automatic.clear();group.paintIcons.clear();
            auto tabs=group.tabLabels;tabs.Children().Clear();
            uint32_t index=0;bool fitted=automaticTabs(data,num(geometry,L"id"));
            for(auto value:array(geometry,L"panels")){
                auto id=value.GetString();auto model=find(array(data->model,L"panels"),L"id",id);
                auto tab=button(data,str(model,L"title"),[data=data,id,groupId=num(geometry,L"id")]{
                    data->dispatch(O({{L"type",S(L"select_panel_tab")},{L"group",N(groupId)},{L"panel",S(id)}}));
                });tab.UseLayoutRounding(false);tab.Height(36);tab.MinWidth(36);tab.Padding(Thickness{8,4,8,4});tab.CornerRadius(CornerRadius{6,6,0,0});
                StackPanel content;content.Orientation(Orientation::Horizontal);content.Spacing(6);
                auto tabStyle=object(model,L"tab");
                content.Children().Append(panelIcon(data,str(model,L"icon"),group.paintIcons));
                {
                    auto name=label(data,str(model,L"title"),true);name.Visibility(flag(tabStyle,L"show_name")?Visibility::Visible:Visibility::Collapsed);content.Children().Append(name);
                    if(fitted)group.automatic.push_back({tab,name,std::wstring(id)+L"\n"+std::wstring(str(model,L"title"))+L"\n"+std::to_wstring(data->textSize())});
                }
                tab.Content(content);
                auto item=O({{L"kind",S(L"panel")},{L"panel",S(id)}});
                gestures->Source(tab,O({{L"type",S(L"drag_workspace")},{L"item",item}}),item,false,
                    O({{L"group",N(num(geometry,L"id"))},{L"index",N(index++)},{L"panel",S(id)}}));
                AutomationProperties::SetAutomationId(tab,L"panel-tab-"+id);
                auto shell=panelTabShell(tab,nullptr,data->tint(L"text",20));
                {
                    auto cut=[weak=weak_from_this(),id=uint32_t(num(geometry,L"id"))]{if(auto self=weak.lock())self->cutStrip(id);};
                    shell.SizeChanged([cut](auto&&,auto&&){cut();});
                    shell.RegisterPropertyChangedCallback(UIElement::OpacityProperty(),[cut](auto&&,auto&&){cut();});
                }
                group.tabs.emplace(std::wstring(id),shell);tabs.Children().Append(shell);
            }
            if(!group.tabScroll){
                group.tabScroll=ScrollView();auto tabScroll=group.tabScroll;tabScroll.Content(tabs);tabScroll.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
                tabScroll.HorizontalScrollMode(ScrollingScrollMode::Enabled);tabScroll.VerticalScrollMode(ScrollingScrollMode::Disabled);
                tabScroll.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
                tabScroll.ContentOrientation(ScrollingContentOrientation::Horizontal);
                tabScroll.IgnoredInputKinds(ScrollingInputKinds::Touch|ScrollingInputKinds::Pen);
                tabScroll.Background(clear());tabScroll.CornerRadius({});
                gestures->Source(tabScroll,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem,true);
                tabScroll.SizeChanged([weak=weak_from_this(),id=uint32_t(num(geometry,L"id"))](auto&&,auto&&){if(auto self=weak.lock()){self->fitTabs(id);self->cutStrip(id);}});
                tabScroll.ViewChanged([weak=weak_from_this(),id=uint32_t(num(geometry,L"id"))](auto&&,auto&&){if(auto self=weak.lock())self->cutStrip(id);});
                tabs.SizeChanged([weak=weak_from_this(),id=uint32_t(num(geometry,L"id"))](auto&&,auto&&){if(auto self=weak.lock())self->cutStrip(id);});
                group.header=Grid();auto header=group.header;header.Background(clear());
                ColumnDefinition labels;labels.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(labels);
                ColumnDefinition trailing;trailing.Width({20,GridUnitType::Pixel});header.ColumnDefinitions().Append(trailing);
                header.Children().Append(tabScroll);
                Border handle;handle.Background(clear());handle.Width(20);handle.Height(36);
                handle.HorizontalAlignment(HorizontalAlignment::Left);handle.Child(panelGrip(data->theme()));
                gestures->Source(handle,O({{L"type",S(L"drag_workspace")},{L"item",groupItem}}),groupItem,true);
                AutomationProperties::SetAutomationId(handle,L"group-grip-"+to_hstring(uint32_t(num(geometry,L"id"))));
                copyName(data,handle,data->copyDelivery(L"move_group"));
                auto help=data->copyDelivery(L"drag_panel");
                AutomationProperties::SetHelpText(handle,help);
                data->copyView([weak=make_weak(handle),resolve=help.current]{if(auto view=weak.get()){AutomationProperties::SetHelpText(view,resolve());return true;}return false;});
                Grid::SetColumn(handle,1);header.Children().Append(handle);frame.Children().Append(header);
            }
        }
        group.header.Visibility(flag(geometry,L"tabs_visible")?Visibility::Visible:Visibility::Collapsed);
        group.activeTab=nullptr;
        bool fitted=automaticTabs(data,num(geometry,L"id"));
        for(auto const& [id,shell]:group.tabs){
            auto tab=shell.Children().GetAt(1).as<Button>();
            auto model=find(array(data->model,L"panels"),L"id",hstring(id));auto presentation=object(model,L"tab");
            auto label=tab.Content().as<StackPanel>();
            label.Children().GetAt(0).Visibility(fitted||flag(presentation,L"show_icon")?Visibility::Visible:Visibility::Collapsed);
            if(!fitted)label.Children().GetAt(1).Visibility(flag(presentation,L"show_name")?Visibility::Visible:Visibility::Collapsed);
            bool selected=hstring(id)==str(geometry,L"active");
            selectPanelTabShell(shell,selected,nullptr,presentation);
            if(selected)group.activeTab=shell;
        }
        if(group.tabScroll&&group.tabScroll.ActualWidth()>0)fitAutomaticTabs(data,group.automatic,group.tabScroll.ActualWidth(),tabWidths);
        group.body=std::make_unique<PanelBody>(data,panel,geometry,
            [weak=weak_from_this()]{if(auto self=weak.lock())self->measured();},gestures);
        auto body=group.body->Root();
        group.bodyScroll=nullptr;group.bodyViewport=nullptr;
        if(object(geometry,L"tiles").Size()){
            Grid::SetRow(body,1);frame.Children().Append(body);
        }else{
            group.bodyScroll=body.try_as<ScrollView>();
            if(!group.bodyScroll){
                group.bodyViewport=Grid();group.bodyViewport.Children().Append(body);
                group.bodyScroll=ScrollView();group.bodyScroll.Content(group.bodyViewport);group.bodyScroll.HorizontalScrollMode(ScrollingScrollMode::Disabled);
                group.bodyScroll.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
                group.bodyScroll.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);mouseScrollbars(group.bodyScroll);
                group.bodyScroll.ViewChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->measured();});
            }
            AutomationProperties::SetAutomationId(group.bodyScroll,L"panel-body-scroll-"+to_hstring(uint32_t(num(geometry,L"id"))));
            AutomationProperties::SetAccessibilityView(group.bodyScroll,Automation::Peers::AccessibilityView::Control);
            Grid::SetRow(group.bodyScroll,1);frame.Children().Append(group.bodyScroll);
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
    bool apply(J const& snapshot){
        auto previousGpu=num(data->model,L"windows_gpu_generation");
        bool relocalize=data->adoptLocalization(snapshot);
        if(relocalize){measureHost.Children().Clear();offscreen.clear();AutomationProperties::SetName(root,str(object(data->catalog,L"bootstrap"),L"drawing_workspace"));}
        bool full=snapshot.HasKey(L"state");
        auto update=object(snapshot,L"workspace_update");
        bool reflow=!full&&snapshot.HasKey(L"layout");
        // Workspace motion can also carry a camera. It is never a camera-only
        // fallback, and it must reference the content already retained here.
        if(full||update.Size()){
            if(!publication.Accept(full,revision(update,L"revision"),revision(update,L"model_revision"),revision(update,L"content_revision"),reflow))return false;
            workspaceUpdate=update;
        }
        if(!full&&!reflow&&snapshot.HasKey(L"windows_glass")){
            auto previous=num(object(data->model,L"windows_glass"),L"panel_revision",-1);
            data->model.Insert(L"windows_glass",snapshot.GetNamedValue(L"windows_glass"));
            if(previous!=num(object(data->model,L"windows_glass"),L"panel_revision",-1)){
                presentPanelBridges();if(glassChanged)glassChanged();
            }
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
        auto bridgeContext=object(model,L"layout").Stringify()+object(full?object(model,L"state"):data->state,L"document_file").GetNamedValue(L"epoch",JsonValue::CreateNullValue()).Stringify();
        if(bridgeContext!=panelBridgeContext){
            panelBridgeContext=bridgeContext;panelBridgeKey=L"";panelBridgeFrame=J{};++panelBridgeRevision;
        }
        if(full){
        if(previousGpu>0&&num(model,L"windows_gpu_generation")!=previousGpu){
            data->thumbnails=CreateLayerThumbnailCache(data->query);
            data->previews=CreateFilterPreviewCache(data->query);
        }
        data->model=snapshot;data->state=object(model,L"state");data->colorPreview=object(model,L"color_preview");
        RefreshFilterPreviewContext(data->previews,object(data->state,L"document_file").GetNamedValue(L"epoch").Stringify());
        // Adoption and layout restoration discard transient host measurements.
        // Reconcile the new model even when retained controls have identical sizes.
        lastMeasurements=L"";
        data->refreshPalette();
        auto theme=data->theme(),palette=object(data->state,L"palette").Stringify();
        if(theme!=previousTheme||palette!=previousPalette){
            panelBridgeKey=L"";panelBridgeFrame=J{};++panelBridgeRevision;
            expansion->Reset();drawers->Reset();collapsed->Reset();root.Children().Clear();groups.clear();handles.clear();previousTheme=theme;previousPalette=palette;root.Children().Append(cameraSlot);canvasBar->attach();notice->attach();colorStrip->attach();for(auto const& panel:previewPanels)panel->attach();
            measureHost.Children().Clear();offscreen.clear();root.Children().Append(measureHost);
        }
        root.RequestedTheme(theme==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        }
        auto layout=object(model,L"layout");
        std::vector<uint32_t> visible;int order=0;
        std::vector<std::wstring> handleIds;
        for(auto value:array(layout,L"groups")){
            auto geometry=value.GetObject();uint32_t id=uint32_t(num(geometry,L"id"));visible.push_back(id);
            auto panel=find(array(model,L"panels"),L"id",str(geometry,L"active"));
            auto [it,added]=groups.try_emplace(id);auto& group=it->second;
            if(added){
                group.frame.Child(group.content);group.frame.Background(clear());group.frame.CornerRadius({SurfaceRadius*CornerFit,SurfaceRadius*CornerFit,SurfaceRadius*CornerFit,SurfaceRadius*CornerFit});
                group.frame.RenderTransform(group.translation);group.shadow.Root().RenderTransform(TranslateTransform());
                group.content.Clip(group.clip);
                group.background.IsHitTestVisible(false);group.background.Fill(data->brush(L"panel"));
                group.content.Children().Append(group.background);group.content.Children().Append(group.strip.Root());group.content.Children().Append(group.border);
                group.configurationFrame.Background(clear());group.content.Children().Append(group.configurationFrame);
                root.Children().Append(group.shadow.Root());root.Children().Append(group.frame);
                AutomationProperties::SetAutomationId(group.frame,L"workspace-group-"+to_hstring(id));
                edgeTag(group.frame,L"edge_surface",B(true));
            }
            AutomationProperties::SetName(group.frame,str(panel,L"title"));
            group.geometry=geometry;group.offset={};
            if(float fit=groupRadius(geometry)*CornerFit;group.frame.CornerRadius().TopLeft!=fit){group.frame.CornerRadius({fit,fit,fit,fit});group.border.CornerRadius({fit,fit,fit,fit});}
            // Geometry-only changes retain controls, focus, capture and scroll.
            if(full||added||!group.body){
            auto structure=J::Parse(geometry.Stringify());
            for(auto field:{L"bounds",L"resize_handles",L"tiles",L"footer_grip",L"min_body_height",L"zen_kept"})if(structure.HasKey(field))structure.Remove(field);
            if(auto tiles=object(geometry,L"tiles");tiles.Size())structure.Insert(L"tile_style",S(str(tiles,L"tile_style")));
            structure.Insert(L"footer",B(object(geometry,L"footer_grip").Size()!=0));
            J signature=O({{L"geometry",structure},{L"panel",panelStructure(panel)}});
            A headers;for(auto member:array(geometry,L"panels")){
                auto panelModel=find(array(model,L"panels"),L"id",member.GetString());
                headers.Append(O({{L"id",S(str(panelModel,L"id"))},{L"icon",S(str(panelModel,L"icon"))},{L"tab",object(panelModel,L"tab")}}));
            }signature.Insert(L"headers",headers);signature.Insert(L"tab_style",S(str(object(model,L"windows_tab_styles"),to_hstring(id).c_str())));
            std::wstring key=signature.Stringify().c_str();
            if(group.key!=key||!group.body){
                try{build(group,geometry,panel);group.key=std::move(key);}
                catch(...){removeGroup(group);groups.erase(it);throw;}
            }
            for(auto const& [panelId,shell]:group.tabs){
                auto tab=shell.Children().GetAt(1).as<Button>();
                auto panelModel=find(array(model,L"panels"),L"id",hstring(panelId));auto title=str(panelModel,L"title");
                AutomationProperties::SetName(tab,title);tooltip(tab,title);
                if(auto content=tab.Content().try_as<Panel>())for(auto child:content.Children())if(auto label=child.try_as<TextBlock>())label.Text(title);
                for(auto& automatic:group.automatic)if(automatic.tab==tab)automatic.key=panelId+L"\n"+std::wstring(title)+L"\n"+std::to_wstring(data->textSize());
            }
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
                removeGroup(it->second);
                it=groups.erase(it);
            }else ++it;
        }
        if(!flag(model,L"chrome_hidden"))for(auto value:array(layout,L"dividers")){
            auto divider=value.GetObject();if(flag(divider,L"fixed"))continue;auto key=L"divider-"+std::to_wstring(uint32_t(num(divider,L"id")));
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
        if(full)updateConfiguration();
        if(full)commandSearch->Apply(data->state);
        canvasBar->Dragging(object(update,L"drag").Size()!=0);if(full)canvasBar->Apply(data->state);
        for(auto const& panel:previewPanels){panel->Place(layout);if(full)panel->Publish(data->state);}
        if(full){pressure->attach();pressure->Publish();}
        notice->Place(layout);if(full){notice->Publish(data->state);data->queueColors();}
        expansion->Apply(configurationHeight());present();
        collapsed->Apply();drawers->Apply();bubbles->Apply();gestures->Refresh();
        auto status=object(layout,L"status");place(cameraSlot,status);
        cameraSlot.Visibility(num(status,L"height")>0?Visibility::Visible:Visibility::Collapsed);
        zoom->text.Foreground(data->brush(L"text"));
        updateCamera(object(data->state,L"camera"));
        updatePopup();queueMeasurements();
        applyMotion(object(update,L"drag"));tracePresentation();if(full)colorRequests();return true;
    }

    Rect placement(Group const& group)const{
        auto bounds=rectangle(object(group.presented.Size()?group.presented:group.geometry,L"bounds"));
        bounds.X+=group.offset.X;bounds.Y+=group.offset.Y;return bounds;
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
            auto from=rectangle(object(group.geometry,L"bounds")),to=rectangle(object(motion,L"bounds"));
            Point offset{to.X-from.X,to.Y-from.Y};
            moved|=offset.X!=group.offset.X||offset.Y!=group.offset.Y;
            group.offset=offset;placeMotion(*next,group);
        }
        gestures->Present(drag);
        if(moved)queueMeasurements();
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
            positions.Append(O({{L"id",N(id)},{L"bounds",rectangle(bounds)},{L"radii",radii},{L"gpu_background",B(group.gpu)},{L"body_scroll",body}}));
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
        visit(root);
        A grips;
        for(auto const& [id,handle]:handles){
            if(!handle.IsLoaded())continue;
            auto bounds=handle.TransformToVisual(root).TransformBounds(
                {0,0,float(handle.ActualWidth()),float(handle.ActualHeight())});
            grips.Append(O({{L"id",S(hstring(id))},{L"bounds",rectangle(bounds)}}));
        }
        auto value=O({{L"revision",N(double(publication.Revision()))},
            {L"model_revision",N(double(publication.ModelRevision()))},
            {L"full_updates",N(double(fullUpdates))},{L"motion_updates",N(double(motionUpdates))},
            {L"color_field_pixels",N(double(data->colorFieldPixels))},
            {L"color_preview",object(data->colorPreview,L"picker").GetNamedValue(L"preview",JsonValue::CreateNullValue())},
            {L"workspace_update",workspaceUpdate},{L"groups",positions},{L"handles",grips},{L"elements",elements},
            {L"panel_bridge_frame",panelBridgeFrame},{L"panel_revision",N(double(panelBridgeRevision))},
            {L"presented_panel_revision",object(data->model,L"windows_glass").GetNamedValue(L"panel_revision",JsonValue::CreateNullValue())},
            {L"overviews",lastOverviews.empty()?A{}:A::Parse(lastOverviews)}}).Stringify();
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
    void measurePanels(){
        if(data->updating||!root.IsLoaded()||root.ActualWidth()<=0)return;
        auto accepted=array(data->model,L"panel_measurements");std::vector<std::wstring> live;
        for(auto value:array(data->model,L"panels")){
            auto id=str(value.GetObject(),L"id");std::wstring key(id);live.push_back(key);
            if(!measurements.contains(key)){
                auto previous=find(accepted,L"panel",id);
                measurements.emplace(key,Measurement{num(previous,L"tab_width",36),num(previous,L"content_height",320),object(previous,L"scroll")});
            }
        }
        for(auto it=measurements.begin();it!=measurements.end();)
            if(std::find(live.begin(),live.end(),it->first)==live.end())it=measurements.erase(it);else ++it;
        auto stable=[](double value){return std::round(std::clamp(value,0.,999999.)*64.)/64.;};
        for(auto const& [id,group]:groups){
            if(group.hidden||group.presented.Size()||!group.frame.IsLoaded())continue;
            for(auto const& [panel,shell]:group.tabs)if(auto tab=shell.Children().GetAt(1).as<Button>();tab.IsLoaded()&&tab.DesiredSize().Width>0){
                double width=tab.DesiredSize().Width;
                for(auto const& automatic:group.automatic)if(automatic.tab==tab)
                    if(auto cached=tabWidths.find(automatic.key);cached!=tabWidths.end()){width=cached->second;break;}
                measurements[panel].tab=stable(width);
            }
            measurements[std::wstring(str(group.geometry,L"active"))].scroll=group.body->ScrollMetrics();
            double height=group.body->ContentHeight();
            if(std::isfinite(height)&&height>=0)measurements[std::wstring(str(group.geometry,L"active"))].content=stable(height);
        }
        std::set<std::wstring> keep;
        for(auto const& [id,group]:groups){
            if(group.hidden||group.presented.Size())continue;
            double width=num(object(group.geometry,L"bounds"),L"width");auto active=str(group.geometry,L"active");
            for(auto value:array(group.geometry,L"panels")){
                auto panelId=value.GetString();if(panelId==active)continue;
                auto panel=find(array(data->model,L"panels"),L"id",panelId);if(!panel.Size()||array(panel,L"tiles").Size())continue;
                std::wstring name(panelId);keep.insert(name);auto& copy=offscreen[name];
                if(copy.data){copy.data->state=data->state;copy.data->model=data->model;}
                if(!copy.body||copy.publication!=fullUpdates){
                auto key=std::wstring(panelStructure(panel).Stringify());
                if(copy.key!=key||!copy.body){
                    auto inert=std::make_shared<WorkspaceData>(*data);
                    inert->send=[](std::string){};inert->document=[](std::string){};inert->input=[](std::string){};
                    inert->query=nullptr;inert->previews.reset();inert->thumbnails.reset();inert->scopes.reset();
                    inert->popupChanged=nullptr;inert->takeBubble=nullptr;inert->bubbleContact=nullptr;inert->pointerCancels.Clear();inert->transients.clear();inert->colorViews.clear();inert->copyViews.clear();
                    auto body=std::make_unique<PanelBody>(inert,panel,O({{L"bounds",O({{L"width",N(width)}})}}),[]{},nullptr,false);
                    body->Root().Width(width);body->Apply(true);measureHost.Children().Append(body->Root());
                    if(copy.body){uint32_t at=0;if(measureHost.Children().IndexOf(copy.body->Root(),at))measureHost.Children().RemoveAt(at);}
                    copy.data=inert;copy.body=std::move(body);copy.key=key;
                }else copy.body->Apply(true);
                copy.publication=fullUpdates;
                }
                if(copy.body->Root().Width()!=width)copy.body->Root().Width(width);
                double height=copy.body->ContentHeight();
                if(std::isfinite(height)&&height>=0&&measurements.contains(name)){
                    measurements[name].content=stable(height);measurements[name].scroll=copy.body->ScrollMetrics();
                }
            }
        }
        for(auto it=offscreen.begin();it!=offscreen.end();){
            if(keep.contains(it->first)){++it;continue;}
            uint32_t at=0;if(it->second.body&&measureHost.Children().IndexOf(it->second.body->Root(),at))measureHost.Children().RemoveAt(at);
            it=offscreen.erase(it);
        }
        if(drawers)for(auto value:drawers->PanelMeasurements()){
            auto item=value.GetObject();auto panel=std::wstring(str(item,L"panel"));
            if(measurements.contains(panel)){measurements[panel].content=stable(num(item,L"content_height"));measurements[panel].scroll=object(item,L"scroll");}
        }
        A report;bool same=accepted.Size()==measurements.size();
        for(auto const& [panel,size]:measurements){
            auto previous=find(accepted,L"panel",hstring(panel));
            same=same&&previous.Size()&&num(previous,L"tab_width")==size.tab&&num(previous,L"content_height")==size.content&&object(previous,L"scroll").Stringify()==size.scroll.Stringify();
            report.Append(O({{L"panel",S(hstring(panel))},{L"tab_width",N(size.tab)},{L"content_height",N(size.content)},{L"scroll",size.scroll.Size()?V(size.scroll):JsonValue::CreateNullValue()}}));
        }
        if(same){lastMeasurements=L"";return;}
        auto json=report.Stringify();if(json==lastMeasurements)return;lastMeasurements=json;
        data->dispatch(O({{L"type",S(L"measure_panels")},{L"measurements",report}}));
    }
    void queueMeasurements(){
        if(measurementFrame)return;
        measurementFrame=CompositionTarget::Rendering(auto_revoke,[weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock()){
                self->measurementFrame.revoke();
                if(!self->root.IsLoaded())return;
                self->reportTitlebar();self->measurePanels();
                self->expansion->Apply(self->configurationHeight());self->publishOverviews();self->presentBackgrounds();self->tracePresentation();
                if(!self->popup&&!str(object(self->data->state,L"customization"),L"control").empty())self->updatePopup();
            }
        });
    }
    void measured(){
        if(!presenting&&!data->updating&&root.IsLoaded())queueMeasurements();
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
            place(group.frame,bounds);placeMotion(id,group);place(group.border,preview);
            group.content.Width(box.Width);group.content.Height(box.Height);
            group.clip.Rect({0,0,box.Width,box.Height});
            group.fade->apply(group.frame,!group.hidden,[weak=weak_from_this(),id,frame=make_weak(group.frame),shadow=make_weak(group.shadow.Root())]{auto self=weak.lock();auto view=frame.get();if(!self||!view)return;auto group=self->groups.find(id);if(group==self->groups.end()||group->second.frame!=view)return;if(auto surface=shadow.get())surface.Opacity(group->second.gpu?0:view.Opacity());if(view.Opacity()==0&&group->second.body)group->second.body->Apply(false);self->queueMeasurements();if(self->glassChanged)self->glassChanged();});
            if(group.hidden&&group.frame.Opacity()==0&&group.body)group.body->Apply(false);
            Canvas::SetZIndex(group.frame,active?1000:group.order);
            group.shadow.Layout(box,active?1000:group.order,group.frame.Visibility()==Visibility::Visible);group.shadow.Root().Opacity(group.gpu?0:group.frame.Opacity());
            auto geometry=group.geometry;
            if(active){
                geometry=J::Parse(group.geometry.Stringify());geometry.Insert(L"bounds",preview);
                if(expanded.HasKey(L"tiles"))geometry.Insert(L"tiles",object(expanded,L"tiles"));
            }
            double bodyHeight=std::max(0.,double(previewBox.Height)-(flag(geometry,L"tabs_visible")?36.:0.)
                -num(object(geometry,L"footer_grip"),L"height"));
            double minimum=object(geometry,L"tiles").Size()?0.:num(geometry,L"min_body_height");
            if(group.bodyViewport)group.bodyViewport.Height(std::max(bodyHeight,minimum));
            if(group.bodyScroll){
                group.bodyScroll.Height(bodyHeight);
                if(auto content=group.bodyScroll.Content().try_as<FrameworkElement>())content.MinHeight(minimum);
            }
            group.body->Layout(geometry);
            if(group.footer){
                auto grip=rectangle(object(group.geometry,L"footer_grip"));
                grip.X=0;grip.Y=0;grip.Width=previewBox.Width;
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
        queueMeasurements();
    }
    void appendGroupOverviews(A& slots,Group const& group){
        if(!(group.body&&group.body->navigator)&&!(group.presented.Size()&&group.configuration))return;
        if(!group.frame.IsLoaded()||group.frame.Visibility()!=Visibility::Visible||group.frame.Opacity()<=0)return;
        auto start=slots.Size();auto clip=visibleBounds(group.frame,root);int order=visualOrder(root,group.frame);
        if(group.body&&group.body->navigator){
            auto visible=intersect(clip,visibleBounds(group.body->navigator->Root(),root));
            auto slot=group.body->navigator->Placement(root,visible,order);
            if(slot.Size())slots.Append(slot);
        }
        if(group.presented.Size()&&group.configuration)group.configuration->AppendOverviews(slots,root,clip,order);
        for(uint32_t i=start;i<slots.Size();++i)slots.GetObjectAt(i).Insert(L"opacity",N(group.frame.Opacity()));
    }
    static float groupRadius(J const& geometry){
        auto tiles=object(geometry,L"tiles");
        return tiles.Size()&&!flag(geometry,L"tabs_visible")?float(num(tiles,L"tile_corner_radius",SurfaceRadius)):SurfaceRadius;
    }
    bool panelBridgesReady()const{return panelBridgeFrame.Size()&&num(object(data->model,L"windows_glass"),L"panel_revision",-1)==double(panelBridgeRevision);}
    bool panelBridgeGroup(uint32_t id)const{
        if(!panelBridgesReady())return false;
        for(auto value:array(panelBridgeFrame,L"bridges"))for(auto receiver:array(value.GetArray().GetObjectAt(2),L"receivers"))if(num(receiver.GetObject(),L"group")==id)return true;
        return false;
    }
    void measurePanelBridges(){
        if(!root.IsLoaded()||root.ActualWidth()<=0||root.ActualHeight()<=0)return;
        A allocated;
        for(auto const& [id,group]:groups){
            if(group.hidden||group.presented.Size()||group.frame.Opacity()<=0)continue;
            auto bounds=allocatedBounds(group.frame,root);if(bounds.Width<=0||bounds.Height<=0)continue;
            A pair;pair.Append(N(id));pair.Append(rectangle(bounds));allocated.Append(pair);
        }
        A viewport;viewport.Append(N(root.ActualWidth()));viewport.Append(N(root.ActualHeight()));
        auto request=O({{L"type",S(L"panel_bridges")},{L"viewport",viewport},
            {L"measure",O({{L"groups",allocated},{L"columns",collapsed->Measure(root)}})}});
        auto key=request.Stringify()+panelBridgeContext;
        if(key==panelBridgeKey)return;
        panelBridgeKey=key;panelBridgeFrame=J{};++panelBridgeRevision;
        if(!QueryWorkspace(data->query,request,[weak=weak_from_this(),key](J reply){
            auto self=weak.lock();if(!self||self->panelBridgeKey!=key)return;
            self->panelBridgeFrame=object(reply,L"result");++self->panelBridgeRevision;
            self->queueMeasurements();self->tracePresentation();if(self->glassChanged)self->glassChanged();
        }))panelBridgeKey=L"";
    }
    static A panelColor(Windows::UI::Color color,double opacity=1){
        A result;for(auto value:{color.R/255.,color.G/255.,color.B/255.,color.A/255.*opacity})result.Append(N(value));return result;
    }
    A panelSurfaces(UIElement const& reference)const{
        A surfaces;auto offset=root.TransformToVisual(reference).TransformPoint({0,0});
        for(auto value:array(panelBridgeFrame,L"columns")){
            auto column=value.GetObject();double id=num(column,L"column");A bridges;
            for(auto bridgeValue:array(panelBridgeFrame,L"bridges")){auto bridge=bridgeValue.GetArray();if(bridge.GetNumberAt(0)==id)bridges.Append(bridge.GetObjectAt(2));}
            if(!bridges.Size())continue;
            A receivers;
            auto expected=array(bridges.GetObjectAt(0),L"receivers");
            for(auto receiverValue:expected){auto receiver=receiverValue.GetObject();auto found=groups.find(uint32_t(num(receiver,L"group")));if(found==groups.end())continue;
                auto const& group=found->second;if(group.hidden||group.presented.Size()||!group.frame.IsLoaded())continue;
                auto rect=[](Rect bounds){A values;for(auto v:{bounds.X,bounds.Y,bounds.Width,bounds.Height})values.Append(N(v));return values;};
                bool tabbed=flag(group.geometry,L"tabs_visible");
                receivers.Append(O({{L"group",N(num(receiver,L"group"))},
                    {L"tab_bar",rect(tabbed?allocatedBounds(group.header,root):Rect{})},
                    {L"active_tab",rect(tabbed?allocatedBounds(group.activeTab,root):Rect{})},
                    {L"tab_clip",rect(tabbed?allocatedBounds(group.tabScroll,root):Rect{})},
                    {L"color",panelColor(data->glass(L"panel").Color(),group.frame.Opacity())},
                    {L"strip_color",panelColor(data->glass(L"strip").Color(),group.frame.Opacity())}}));
            }
            if(receivers.Size()!=expected.Size())continue;
            auto b=rectangle(object(column,L"bounds"));A sidebar,translation;
            for(auto v:{b.X,b.Y,b.Width,b.Height})sidebar.Append(N(v));translation.Append(N(offset.X));translation.Append(N(offset.Y));
            surfaces.Append(O({{L"offset",translation},{L"sidebar",sidebar},{L"radii",array(column,L"radii")},
                {L"color",panelColor(data->glass(L"panel").Color(),collapsed->Opacity(uint32_t(id)))},{L"bridges",bridges},{L"groups",receivers}}));
        }
        return surfaces;
    }
    void presentBackgrounds(){
        measurePanelBridges();presentPanelBridges();
        auto tabs=array(data->state,L"tabs");auto document=tabs.Size()?tabs.GetObjectAt(0):J{};
        for(auto& [id,group]:groups){
            if(!group.frame.IsLoaded()||group.hidden)continue;
            A slots;appendGroupOverviews(slots,group);
            auto bounds=rectangle(placement(group));
            bool expanded=group.presented.Size()!=0,tabbed=flag(group.geometry,L"tabs_visible");
            J source;for(auto value:data->drawerSources){auto anchor=object(value.GetObject(),L"anchor");
                if(str(anchor,L"kind")==L"tile"&&str(anchor,L"panel")==str(group.geometry,L"active"))source=value.GetObject();}
            float radius=groupRadius(group.geometry);
            std::array<float,4> corners{radius,radius,radius,radius};
            if(source.Size()&&!expanded){auto square=sourceCorners(data,source,bounds);for(int i=0;i<4;++i)if(square[i])corners[i]=0;}
            for(auto value:array(panelBridgeFrame,L"groups")){auto geometry=value.GetObject();if(num(geometry,L"group")!=id)continue;
                auto radii=array(geometry,L"radii");for(uint32_t i=0;i<4;++i)corners[i]=float(radii.GetNumberAt(i));break;}
            A radii;for(auto r:corners)radii.Append(N(r));
            auto key=O({{L"expansion",group.presented},{L"tabbed",B(tabbed)},{L"source",source},{L"transparent",B(data->transparent())},{L"bounds",bounds},{L"slots",slots},
                {L"radius",N(radius)},{L"corners",radii},{L"width",N(num(document,L"width"))},{L"height",N(num(document,L"height"))}}).Stringify();
            if(key==group.backgroundKey)continue;group.backgroundKey=key;
            group.corners=tabbed&&!expanded?std::array<float,4>{SurfaceRadius,SurfaceRadius,corners[2],corners[3]}:corners;
            GeometryGroup shape;shape.FillRule(FillRule::EvenOdd);
            float width=float(num(bounds,L"width")),height=float(num(bounds,L"height"));
            auto outline=expanded?expansionShape(group.presented):tabbed?squircleRectangle(width,std::max(0.f,height-36),{0,0,corners[2],corners[3]}):squircleRectangle(width,height,corners);
            cutStrip(group);
            if(tabbed&&!expanded){TranslateTransform body;body.Y(36);outline.Transform(body);}
            group.background.Fill(expanded?data->brush(L"panel"):data->glass(source.Size()?L"source":L"panel"));
            if(group.header)group.header.Background(expanded?data->brush(L"tabbar"):clear());
            if(group.activeTab)for(auto layer:group.activeTab.Children())if(auto canvas=layer.try_as<Canvas>())for(auto item:canvas.Children())if(auto path=item.try_as<Microsoft::UI::Xaml::Shapes::Path>())
                path.Fill(expanded?Brush(data->brush(L"panel")):Brush(nullptr));
            auto shadowKey=O({{L"expansion",group.presented},{L"radius",N(radius)},{L"width",bounds.GetNamedValue(L"width")},{L"height",bounds.GetNamedValue(L"height")}}).Stringify();
            if(shadowKey!=group.shadowKey){
                group.shadowKey=shadowKey;
                // WinUI geometries have one owner; retain a separate mask outline.
                auto mask=expanded?expansionShape(group.presented):squircleRectangle(float(num(bounds,L"width")),float(num(bounds,L"height")),{radius,radius,radius,radius});
                group.shadow.Shape(mask,float(num(bounds,L"width")),float(num(bounds,L"height")),expanded?36.f:12.f,expanded?8.f:2.f,expanded?.4f:.16f);
                if(expanded)group.shadow.Uncut();else group.shadow.Cut({radius,radius,radius,radius});
            }
            shape.Children().Append(outline);
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
    void presentPanelBridges(){
        collapsed->PresentBridges(panelBridgeFrame,panelBridgesReady());
        for(auto& [id,group]:groups){
            if(!group.frame.IsLoaded()||group.hidden)continue;
            group.gpu=panelBridgeGroup(id)&&!group.presented.Size();
            group.background.Opacity(group.gpu?0:1);group.strip.Root().Opacity(group.gpu?0:1);group.shadow.Root().Opacity(group.gpu?0:group.frame.Opacity());
        }
    }
    void cutStrip(uint32_t id){if(auto found=groups.find(id);found!=groups.end())cutStrip(found->second);}
    void cutStrip(Group& group){
        std::vector<Point> band,active;float width=placement(group).Width;
        if(flag(group.geometry,L"tabs_visible")&&!group.presented.Size()&&width>0){
            band=squircleOutline(width,36,{SurfaceRadius,SurfaceRadius,0,0});
            if(auto tab=group.activeTab;tab&&tab.Opacity()>0&&tab.ActualWidth()>0){
                auto view=group.tabScroll.TransformToVisual(group.content).TransformPoint({0,0});
                float x=view.X+group.tabLabels.ActualOffset().x+tab.ActualOffset().x-float(group.tabScroll.HorizontalOffset())-6;
                auto outline=panelTabOutline(float(tab.ActualWidth()),float(tab.ActualHeight()));
                for(auto& point:outline){point.X+=x;point.Y+=view.Y;}
                active=clipColumns(outline,view.X,view.X+float(group.tabScroll.ActualWidth()));
            }
        }
        group.strip.Present(width,std::move(band),std::move(active),data->glass(L"strip").Color(),data->glass(L"panel").Color());
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
        if(str(action,L"type")==L"drag_divider"){
            using namespace winrt::Microsoft::UI::Input;
            auto shape=num(bounds,L"width")<num(bounds,L"height")?
                InputSystemCursorShape::SizeWestEast:InputSystemCursorShape::SizeNorthSouth;
            handle.as<IUIElementProtected>().ProtectedCursor(InputSystemCursor::Create(shape));
        }
        AutomationProperties::SetAutomationId(handle,hstring(key));
        AutomationProperties::SetAutomationControlType(handle,Automation::Peers::AutomationControlType::Thumb);
    }
    void fitTabs(uint32_t id){if(auto found=groups.find(id);found!=groups.end()&&found->second.tabScroll)fitAutomaticTabs(data,found->second.automatic,found->second.tabScroll.ActualWidth(),tabWidths);}
    void publishOverviews(){
        A slots;
        for(auto const& [id,group]:groups)appendGroupOverviews(slots,group);
        if(drawers)drawers->AppendOverviews(slots);
        if(drawers){
            auto sources=drawers->Sources();
            if(sources.Stringify()!=data->drawerSources.Stringify()){
                data->drawerSources=sources;
                for(auto& [id,group]:groups){group.backgroundKey=L"";if(group.body)group.body->Apply(!group.hidden);}
                collapsed->Apply();
            }
        }
        auto tabs=array(data->state,L"tabs");overviewOcclusion.Apply(root,slots,tabs.Size()?tabs.GetObjectAt(0):J{});
        auto json=slots.Stringify();
        if(json!=lastOverviews){lastOverviews=json;overviews(to_string(json));}
        if(glassChanged)glassChanged();
    }
    A glass(UIElement const& reference,A& connections,A& arcs,A& colors,A& panels,uint64_t& panelRevision){
        panels=panelSurfaces(reference);auto encoded=panels.Stringify();
        if(encoded!=lastPanelSurfaces){lastPanelSurfaces=encoded;++panelBridgeRevision;presentPanelBridges();}
        panelRevision=panelBridgeRevision;
        A regions;
        for(auto const& [id,group]:groups)if(!group.gpu&&group.frame.Visibility()==Visibility::Visible&&!group.presented.Size())appendGlass(regions,group.frame,reference,{group.corners[0],group.corners[1],group.corners[2],group.corners[3]},true);
        collapsed->AppendGlass(regions,reference);drawers->AppendGlass(regions,connections,reference);
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
WorkspaceView::WorkspaceView(Dispatch send,Json catalog,NativeLocalization localization,Dispatch overviews,PreviewTransport previews,std::function<void(bool)> popupChanged,Dispatch document,Dispatch input,EdgeTransport edges):impl(std::make_shared<Impl>(std::move(send),catalog,localization,std::move(overviews),std::move(previews),std::move(popupChanged),std::move(document),std::move(input),std::move(edges))){impl->init();}
WorkspaceView::~WorkspaceView()=default;
Canvas WorkspaceView::Root()const{return impl->root;}
bool WorkspaceView::Apply(Json const& snapshot){return impl->apply(snapshot);}
WorkspaceView::Json WorkspaceView::ChromeFacts(bool popupOpen){impl->data->externalPopup=popupOpen;return J::Parse(impl->data->chrome.Stringify());}
void WorkspaceView::CancelPreviews(){for(auto const& panel:impl->previewPanels)panel->Cancel();}
bool WorkspaceView::CancelGesture(){
    bool closed=impl->data->dismissTransients();
    closed=impl->bubbles->Cancel()||closed;
    return impl->gestures->Cancel()||closed;
}
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
JsonArray WorkspaceView::Glass(UIElement const& reference,JsonArray& connections,JsonArray& arcs,JsonArray& colors,JsonArray& panels,uint64_t& panelRevision)const{return impl->glass(reference,connections,arcs,colors,panels,panelRevision);}
void WorkspaceView::SetTitlebarInsets(float left,float right,float height){
    std::array<float,3> value{left,right,height};
    if(value!=impl->titlebar){impl->titlebar=value;impl->lastTitlebar=L"";impl->reportTitlebar();}
}

void WorkspaceView::MergeLayout(Json const& model,Json const& patch){
    using namespace CapyUi;
    for(auto field:{L"layout",L"panel_measurements",L"workspace_update",L"windows_tab_styles",L"windows_glass"})
        if(patch.HasKey(field))model.Insert(field,patch.GetNamedValue(field));
    auto state=object(model,L"state"),layout=object(object(state,L"workspace"),L"layout");
    for(auto const& item:object(patch,L"workspace_layout"))layout.Insert(item.Key(),item.Value());
    if(patch.HasKey(L"camera"))state.Insert(L"camera",patch.GetNamedValue(L"camera"));
}
