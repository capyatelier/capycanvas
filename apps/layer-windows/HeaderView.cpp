#include "pch.h"
#include "HeaderView.h"
#include "HeaderInput.h"
#include "HeaderStatus.h"
#include "DrawingTabs.h"
#include "NativeMenus.h"
#include "WorkspaceQuery.h"
#include "WorkspaceGeometry.h"
#include "ColorPair.h"
#include <chrono>
#include <set>
#include <limits>
#include <winrt/Microsoft.UI.Xaml.Media.Animation.h>

using namespace CapyUi;
namespace {
J invoke(hstring const& command){return O({{L"type",S(L"invoke")},{L"command",S(command)}});}
J edit(J const& action){return O({{L"type",S(L"customize")},{L"action",O({{L"type",S(L"header")},{L"action",action}})}});}
Windows::UI::Color blend(Windows::UI::Color bg,Windows::UI::Color ink,float amount){
    float alpha=bg.A*(1-amount)+ink.A*amount;
    auto channel=[&](uint8_t a,uint8_t b){return uint8_t(alpha>0?std::lround((a*bg.A*(1-amount)+b*ink.A*amount)/alpha):0);};
    return {uint8_t(std::lround(alpha)),channel(bg.R,ink.R),channel(bg.G,ink.G),channel(bg.B,ink.B)};
}
void style(Button const& item,std::shared_ptr<WorkspaceData> const& data,bool surface=true){
    item.Height(36);item.Padding({6,0,6,0});item.UseLayoutRounding(false);
    if(surface){
        auto bg=headerSurface(data).Color();auto ink=color(str(object(data->state,L"palette"),L"text",L"#fafafb"));
        item.Background(headerSurface(data));
        item.Resources().Insert(box_value(L"ButtonBackgroundPointerOver"),fill(blend(bg,ink,.10f)));
        item.Resources().Insert(box_value(L"ButtonBackgroundPressed"),fill(blend(bg,ink,.16f)));
        item.Resources().Insert(box_value(L"ButtonBackgroundDisabled"),headerSurface(data));
    }else{
        item.Background(clear());item.BorderBrush(clear());item.BackgroundSizing(BackgroundSizing::InnerBorderEdge);
        for(auto role:{L"ButtonBorderBrush",L"ButtonBorderBrushPointerOver",L"ButtonBorderBrushPressed",L"ButtonBorderBrushDisabled"})
            item.Resources().Insert(box_value(role),clear());
        item.Resources().Insert(box_value(L"ButtonBackgroundPointerOver"),data->tint(L"text",26));
        item.Resources().Insert(box_value(L"ButtonBackgroundPressed"),data->tint(L"text",41));
        item.Resources().Insert(box_value(L"ButtonBackgroundDisabled"),clear());
    }
}
}
struct HeaderView::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    std::function<void()> changed,fullscreen,newWindow;
    Grid root;
    Canvas canvas,bankContent;
    Border background,bank,ghost;
    ScrollViewer bankScroll;
    std::unique_ptr<HeaderInput> input;
    std::unique_ptr<HeaderStatus> systemStatus;
    struct Item{
        Border frame,outline;Grid content;Button editor{nullptr};FrameworkElement view{nullptr};
        Image grip{nullptr};J entry;hstring key,iconKey;std::shared_ptr<CapyUi::ColorPair> colors;
    };
    std::map<uint32_t,Item> items;std::vector<Border> bars;
    std::vector<FrameworkElement> bankParts;
    StackPanel editorControls,sizeChoices,menuLabels,switches;
    std::vector<Button> menus;
    std::vector<std::pair<Button,hstring>> sizes;
    CheckBox footer;
    Button primary,menuOverflow,workspaceOverflow,zen,settings,recovery;
    Grid menuGroup,workspaceGroup;Border menuCapsule;
    ScrollViewer switcher;
    Border document;
    std::shared_ptr<DrawingTabs> drawings;
    std::vector<std::pair<Primitives::ToggleButton,hstring>> workspaces;
    hstring switchActive,theme,palette,bankKey,geometryKey,desiredKey,lastMeasurement,traceKey;
    J geometry,view,configuration;
    std::vector<Button> overflow;
    std::vector<Border> zones;
    bool built=false,editing=false,hidden=false,fullscreenActive=false,applying=false,resolvingLink=false,queryBusy=false;
    bool scheduled=false,trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    float leftInset=0,rightInset=0;
    double tile=36,iconSize=20,height=48,totalHeight=48,menuWidth=0,switchWidth=36;
    uint32_t focusedItem=0;
    std::set<uint32_t> handledRequests;
    Microsoft::UI::Dispatching::DispatcherQueueTimer requestTimer{nullptr},geometryTimer{nullptr};
    ~Impl(){if(requestTimer)requestTimer.Stop();if(geometryTimer)geometryTimer.Stop();}
    void complete(uint32_t id,V error=JsonValue::CreateNullValue()){
        data->dispatch(O({{L"type",S(L"complete_request")},{L"id",N(id)},{L"error",error}}));
    }
    static fire_and_forget launchLink(std::weak_ptr<Impl> weak,uint32_t id,hstring url){
        V error=JsonValue::CreateNullValue();
        try{
            if(!co_await Windows::System::Launcher::LaunchUriAsync(Windows::Foundation::Uri(url)))
                error=S(L"Windows could not open the application link.");
        }catch(hresult_error const& failure){
            error=S(L"Windows could not open the application link ("+to_hstring(failure.code().value)+L").");
        }
        if(auto self=weak.lock()){self->resolvingLink=false;self->complete(id,error);}
    }
    void requests(){
        auto requests=array(data->state,L"requests");std::set<uint32_t> present;bool retry=false;
        for(auto value:requests)present.insert(uint32_t(num(value.GetObject(),L"id")));
        for(auto it=handledRequests.begin();it!=handledRequests.end();)
            if(!present.contains(*it))it=handledRequests.erase(it);else ++it;
        for(auto value:requests){
            auto request=value.GetObject();auto id=uint32_t(num(request,L"id"));
            if(handledRequests.contains(id))continue;
            auto kind=object(request,L"kind");auto type=str(kind,L"type");
            if(type==L"drawings"&&drawings){
                handledRequests.insert(id);showDrawings();complete(id);
            }else if(type==L"set_fullscreen"){
                handledRequests.insert(id);
                try{if(flag(kind,L"fullscreen")!=fullscreenActive)fullscreen();complete(id);}
                catch(hresult_error const& failure){complete(id,S(L"Windows could not change full screen ("+to_hstring(failure.code().value)+L")."));}
            }else if(type==L"new_window"){
                // Opening/activating a window can re-enter the UI dispatcher.
                handledRequests.insert(id);
                try{newWindow();complete(id);}
                catch(hresult_error const& failure){complete(id,S(L"Windows could not create a window ("+to_hstring(failure.code().value)+L")."));}
                catch(std::exception const&){complete(id,S(L"Windows could not create a window."));}
            }else if(type==L"open_link"&&!resolvingLink){
                resolvingLink=true;
                bool queued=QueryWorkspace(data->query,O({{L"type",S(L"application_link")},{L"link",S(str(kind,L"link"))}}),
                    [weak=weak_from_this(),id](J reply){
                        if(auto self=weak.lock()){
                            auto url=str(reply,L"result");
                            if(url.empty()){self->resolvingLink=false;self->complete(id,S(L"The application link is unavailable."));return;}
                            launchLink(weak,id,url);
                        }
                    });
                if(queued)handledRequests.insert(id);else{resolvingLink=false;retry=true;}
            }
        }
        if(retry)requestTimer.Start();else requestTimer.Stop();
    }
    void applyWorkspaces() {
        auto storage=object(data->model,L"windows_workspace");
        auto values=array(storage,L"switcher_display");auto active=str(storage,L"id");
        auto previousFirst=workspaces.empty()?hstring{}:workspaces.front().second;
        std::vector<std::pair<Primitives::ToggleButton,hstring>> next;
        auto chosen=data->glass(L"switcher_selection"),hover=data->brush(L"header_selection_hover");
        for(auto value:values){
            auto choice=value.GetObject();auto id=str(choice,L"id");
            auto found=std::find_if(workspaces.begin(),workspaces.end(),[&](auto const& p){return p.second==id;});
            if(found==workspaces.end()){
                Primitives::ToggleButton item;item.UseLayoutRounding(false);item.MinWidth(0);item.MinHeight(0);item.Height(26);item.Padding({8,0,8,0});
                item.BorderThickness({0});item.CornerRadius({15,15,15,15});item.FontSize(data->textSize());
                // Chrome resolves the shared CSS medium weight to Segoe UI Semibold.
                item.FontFamily(FontFamily(L"Segoe UI"));item.FontWeight(Windows::UI::Text::FontWeights::SemiBold());
                item.Foreground(data->brush(L"text"));item.Background(fill({0,0,0,0}));
                item.Resources().Insert(box_value(L"ToggleButtonBackgroundPointerOver"),data->tint(L"text",20));
                item.Resources().Insert(box_value(L"ToggleButtonBackgroundPressed"),data->tint(L"text",41));
                item.Resources().Insert(box_value(L"ToggleButtonBackgroundChecked"),chosen);
                item.Resources().Insert(box_value(L"ToggleButtonBackgroundCheckedPointerOver"),hover);
                item.Resources().Insert(box_value(L"ToggleButtonBackgroundCheckedPressed"),hover);
                item.Resources().Insert(box_value(L"ToggleButtonForegroundChecked"),data->brush(L"text"));
                AutomationProperties::SetAutomationId(item,L"workspace-switch-"+str(choice,L"key"));
                auto label=CapyUi::label(data,L"");label.UseLayoutRounding(false);label.FontWeight(Windows::UI::Text::FontWeights::SemiBold());
                label.TextTrimming(TextTrimming::CharacterEllipsis);label.MaxWidth(110);item.Content(label);
                item.Click([weak=weak_from_this(),id](auto&&,auto&&){
                    if(auto self=weak.lock()){
                        // Toggle state always reflects adoption, including focus-only and failed switches.
                        self->applyWorkspaces();
                        self->data->dispatch(O({{L"type",S(L"workspace_manager")},
                            {L"command",O({{L"type",S(L"switch")},{L"id",S(id)}})}}));
                    }
                });
                switches.Children().Append(item);workspaces.emplace_back(item,id);found=std::prev(workspaces.end());
            }
            auto const& item=found->first;auto name=str(choice,L"name");
            auto content=item.Content().as<TextBlock>();
            if(content.Text()!=name){
                content.Text(name);
                auto measure=CapyUi::label(data,name);measure.FontWeight(Windows::UI::Text::FontWeights::SemiBold());
                measure.UseLayoutRounding(false);measure.Measure({std::numeric_limits<float>::infinity(),36});
                item.Tag(box_value(double(measure.DesiredSize().Width)));
            }
            next.emplace_back(item,id);
            content.Foreground(data->brush(L"text"));item.IsChecked(id==active);item.IsEnabled(flag(storage,L"can_switch"));
            item.Background(id==active?chosen:clear());
            item.Foreground(data->brush(L"text"));
            AutomationProperties::SetName(item,name);ToolTipService::SetToolTip(item,box_value(L"Switch to "+name+L" workspace"));
        }
        auto children=switches.Children();
        for(uint32_t i=0;i<next.size();++i){
            auto item=next[i].first;
            if(i<children.Size()&&children.GetAt(i)==item)continue;
            uint32_t from=0;if(children.IndexOf(item,from))children.RemoveAt(from);
            children.InsertAt(i,item);
        }
        while(children.Size()>next.size())children.RemoveAtEnd();
        workspaces=std::move(next);
        if(!workspaces.empty()&&workspaces.front().second==active&&(active!=switchActive||previousFirst!=active))switcher.ChangeView(0.,nullptr,nullptr,true);
        switchActive=active;
        switcher.Visibility(values.Size()?Visibility::Visible:Visibility::Collapsed);
    }
    double textWidth(hstring const& text,bool bold=false,bool semibold=false)const{
        auto value=label(data,text,bold);value.UseLayoutRounding(false);if(semibold)value.FontWeight(Windows::UI::Text::FontWeights::SemiBold());
        value.Measure({std::numeric_limits<float>::infinity(),60});return value.DesiredSize().Width;
    }
    void schedule(){
        if(scheduled)return;scheduled=true;
        root.DispatcherQueue().TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock()){
            self->scheduled=false;self->reflow();
        }});
    }
    void init(){
        root.VerticalAlignment(VerticalAlignment::Top);root.Height(48);root.UseLayoutRounding(false);
        canvas.UseLayoutRounding(false);root.Children().Append(canvas);
        AutomationProperties::SetName(root,L"Application header");
        AutomationProperties::SetAutomationId(canvas,L"title-bar");
        input=std::make_unique<HeaderInput>(data,canvas,[weak=weak_from_this()]{if(auto self=weak.lock())self->present();});
        input->Source(canvas,O({{L"kind",S(L"background")}}),L"Title bar");
        root.SizeChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->schedule();});
        root.LayoutUpdated([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){
            self->evidence();if(self->changed)self->changed();
        }});
        requestTimer=root.DispatcherQueue().CreateTimer();requestTimer.Interval(std::chrono::milliseconds(200));
        requestTimer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->requests();});
        geometryTimer=root.DispatcherQueue().CreateTimer();geometryTimer.Interval(std::chrono::milliseconds(16));
        geometryTimer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->reflow();});
        root.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){self->requests();self->schedule();}});
        root.Unloaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){
            self->requestTimer.Stop();self->geometryTimer.Stop();
        }});
    }
    MenuFlyout menu(std::function<void(Windows::Foundation::Collections::IVector<MenuFlyoutItemBase>)> populate){
        MenuFlyout result;TrackPopup(result,data);
        result.Opening([populate](Windows::Foundation::IInspectable const& sender,auto&&){
            auto value=sender.as<MenuFlyout>();value.Items().Clear();populate(value.Items());
        });return result;
    }
    void fillMenu(Windows::Foundation::Collections::IVector<MenuFlyoutItemBase> const& target,J const& model){
        NativeMenuItems(target,array(model,L"sections"),data,[data=data](J action){data->dispatch(action);});
    }
    Button command(hstring const& id){
        auto item=button(data,id,[data=data,id]{data->dispatch(invoke(id));});style(item,data,false);item.Padding({0});return item;
    }
    void build(){
        input->Cancel();canvas.Children().Clear();items.clear();bars.clear();bankParts.clear();workspaces.clear();menus.clear();sizes.clear();overflow.clear();zones.clear();
        bankKey=geometryKey=desiredKey=lastMeasurement=L"";systemStatus.reset();
        background=Border();background.Background(clear());canvas.Children().Append(background);
        menuLabels=StackPanel();menuLabels.Orientation(Orientation::Horizontal);menuLabels.Spacing(2);
        menuLabels.UseLayoutRounding(false);menuWidth=8;
        for(auto value:array(data->model,L"application_menus")){
            auto spec=value.GetObject();auto id=str(spec,L"id");auto item=button(data,str(spec,L"label"),[]{});style(item,data,false);item.Padding({8,0,8,0});
            item.Height(26);item.CornerRadius({13,13,13,13});item.FontWeight(Windows::UI::Text::FontWeights::SemiBold());
            item.Width(textWidth(str(spec,L"label"),false,true)+16);menuWidth+=item.Width()+(menus.empty()?0:2);
            AutomationProperties::SetAutomationId(item,L"application-menu-"+id);
            item.Flyout(menu([weak=weak_from_this(),id](auto target){if(auto self=weak.lock())
                self->fillMenu(target,object(find(array(self->data->model,L"application_menus"),L"id",id),L"model"));
            }));
            menuLabels.Children().Append(item);menus.push_back(item);
        }
        auto primaryMenu=[weak=weak_from_this()](auto target){if(auto self=weak.lock())self->fillMenu(target,object(self->view,L"primary_menu"));};
        primary=button(data,L"Main Menu",[]{});style(primary,data,false);primary.Padding({0});primary.Flyout(menu(primaryMenu));
        AutomationProperties::SetAutomationId(primary,L"application-primary-menu");
        menuOverflow=button(data,L"Menus",[]{});style(menuOverflow,data,false);menuOverflow.Padding({0});
        AutomationProperties::SetAutomationId(menuOverflow,L"application-menus");
        menuOverflow.Flyout(menu([weak=weak_from_this()](auto target){if(auto self=weak.lock()){
            for(auto value:array(self->data->model,L"application_menus")){
                auto spec=value.GetObject();MenuFlyoutSubItem item;item.Text(str(spec,L"label"));item.FontSize(self->data->textSize());
                AutomationProperties::SetAutomationId(item,L"application-menu-"+str(spec,L"id"));self->fillMenu(item.Items(),object(spec,L"model"));target.Append(item);
            }
        }}));
        menuCapsule=Border();menuCapsule.Height(36);menuCapsule.Padding({5,5,5,5});menuCapsule.CornerRadius({18,18,18,18});menuCapsule.Background(headerSurface(data));
        menuCapsule.VerticalAlignment(VerticalAlignment::Center);menuCapsule.Child(menuLabels);
        menuGroup=Grid();menuGroup.VerticalAlignment(VerticalAlignment::Center);menuGroup.Children().Append(menuCapsule);menuGroup.Children().Append(menuOverflow);
        zen=command(L"zen_mode");settings=command(L"settings");
        AutomationProperties::SetAutomationId(zen,L"zen-button");AutomationProperties::SetAutomationId(settings,L"settings-button");
        switches=StackPanel();switches.Orientation(Orientation::Horizontal);switches.Spacing(2);switches.UseLayoutRounding(false);
        switcher=ScrollViewer();switcher.UseLayoutRounding(false);switcher.Content(switches);switcher.Height(36);
        switcher.HorizontalScrollMode(ScrollMode::Enabled);switcher.VerticalScrollMode(ScrollMode::Disabled);
        switcher.HorizontalScrollBarVisibility(ScrollBarVisibility::Hidden);switcher.VerticalScrollBarVisibility(ScrollBarVisibility::Disabled);
        switcher.ZoomMode(ZoomMode::Disabled);switcher.IsTabStop(false);switcher.Padding({5,5,5,5});switcher.CornerRadius({18,18,18,18});switcher.BorderThickness({0});
        switcher.Background(data->glass(L"switcher"));
        AutomationProperties::SetAutomationId(switcher,L"workspace-switcher");AutomationProperties::SetName(switcher,L"Task workspaces");
        workspaceOverflow=button(data,L"Workspaces",[]{});style(workspaceOverflow,data,false);workspaceOverflow.Padding({0});
        AutomationProperties::SetAutomationId(workspaceOverflow,L"header-workspace-menu");
        workspaceOverflow.Flyout(menu([weak=weak_from_this()](auto target){if(auto self=weak.lock()){
            auto storage=object(self->data->model,L"windows_workspace");
            for(auto value:array(storage,L"switcher_display")){
                auto choice=value.GetObject();auto id=str(choice,L"id");ToggleMenuFlyoutItem item;item.Text(str(choice,L"name"));
                item.IsChecked(id==str(storage,L"id"));item.IsEnabled(flag(storage,L"can_switch"));
                item.Click([data=self->data,id](auto&&,auto&&){data->dispatch(O({{L"type",S(L"workspace_manager")},{L"command",O({{L"type",S(L"switch")},{L"id",S(id)}})}}));});
                target.Append(item);
            }
        }}));
        workspaceGroup=Grid();workspaceGroup.VerticalAlignment(VerticalAlignment::Center);workspaceGroup.Children().Append(switcher);workspaceGroup.Children().Append(workspaceOverflow);
        drawings=std::make_shared<DrawingTabs>();drawings->data=data;drawings->init();
        document=Border();document.Child(drawings->root);document.Background(clear());
        AutomationProperties::SetAutomationId(document,L"document-title");AutomationProperties::SetName(document,L"Drawings");
        systemStatus=std::make_unique<HeaderStatus>(data,[weak=weak_from_this()]{if(auto self=weak.lock())self->schedule();});
        bank=Border();bank.Background(data->brush(L"panel"));bank.CornerRadius({8,8,8,8});bank.Padding({6,6,6,6});bankContent=Canvas();bankScroll=ScrollViewer();
        bankScroll.Content(bankContent);bankScroll.HorizontalScrollMode(ScrollMode::Disabled);bankScroll.VerticalScrollMode(ScrollMode::Enabled);
        bankScroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);bankScroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);
        bank.Child(bankScroll);canvas.Children().Append(bank);Canvas::SetZIndex(bank,20);
        AutomationProperties::SetAutomationId(bank,L"header-editor");AutomationProperties::SetName(bank,L"Customize Title Bar");
        ghost=Border();ghost.IsHitTestVisible(false);ghost.Background(data->brush(L"panel"));ghost.CornerRadius({6,6,6,6});ghost.BorderThickness({1,1,1,1});
        canvas.Children().Append(ghost);Canvas::SetZIndex(ghost,40);
        for(int zone=0;zone<3;++zone){
            Border outline;outline.IsHitTestVisible(false);outline.BorderThickness({1,1,1,1});outline.CornerRadius({6,6,6,6});canvas.Children().Append(outline);zones.push_back(outline);
            auto more=button(data,L"More title bar items",[]{});style(more,data);more.Padding({0});
            AutomationProperties::SetAutomationId(more,L"header-overflow-"+to_hstring(zone));
            more.Flyout(menu([weak=weak_from_this(),zone](auto target){if(auto self=weak.lock()){
                auto hidden=array(self->geometry,L"hidden");if(uint32_t(zone)>=hidden.Size())return;
                for(auto value:hidden.GetArrayAt(zone)){
                    auto id=uint32_t(value.GetNumber());auto spec=findId(array(self->view,L"items"),id);MenuFlyoutItem row;row.Text(str(spec,L"label"));
                    AutomationProperties::SetAutomationId(row,L"header-overflow-item-"+to_hstring(id));
                    row.Click([weak,id,zone](auto&&,auto&&){if(auto self=weak.lock()){
                        if(self->editing)self->input->Select(id);else self->activate(id,self->overflow[zone]);
                    }});target.Append(row);
                }
            }}));
            canvas.Children().Append(more);Canvas::SetZIndex(more,10);overflow.push_back(more);
        }
        recovery=button(data,L"Main Menu",[]{});style(recovery,data);recovery.Padding({0});recovery.Flyout(menu(primaryMenu));
        AutomationProperties::SetAutomationId(recovery,L"header-recovery-menu");canvas.Children().Append(recovery);Canvas::SetZIndex(recovery,10);
        built=true;
    }
    FrameworkElement control(uint32_t id,J const& item){
        auto kind=str(item,L"kind");
        if(kind==L"capy")return zen;if(kind==L"settings")return settings;if(kind==L"menu")return primary;
        if(kind==L"menu_labels")return menuGroup;if(kind==L"workspaces")return workspaceGroup;if(kind==L"document_title")return document;
        if(kind==L"clock")return systemStatus->Clock();if(kind==L"battery")return systemStatus->Battery();
        if(kind==L"space"){Border space;space.Background(clear());return space;}
        auto presses=kind==L"tool"&&pickerControl(object(item,L"control"))?std::make_shared<DoublePress>():nullptr;
        auto pick=button(data,L"",[weak=weak_from_this(),id,presses]{if(auto self=weak.lock()){
            if(presses&&!self->editing&&presses->second()){
                self->data->dispatch(O({{L"type",S(L"color_picker")},{L"action",O({{L"kind",S(L"settings")},
                    {L"anchor",O({{L"kind",S(L"header")},{L"id",N(id)}})}})}}));
                return;
            }
            if(presses&&self->editing)presses->reset();
            self->activate(id);
        }});style(pick,data,false);pick.Padding({0});
        if(presses)presses->listen(pick);
        return pick;
    }
    static bool shown(FrameworkElement element){
        if(!element||!element.IsLoaded()||element.ActualWidth()<=0)return false;
        for(auto node=element.as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node))
            if(auto item=node.try_as<UIElement>();item&&item.Visibility()!=Visibility::Visible)return false;
        return true;
    }
    void showDrawings(FrameworkElement anchor=nullptr){drawings->show(shown(anchor)?anchor:shown(document)?FrameworkElement(document):FrameworkElement(root));}
    void activate(uint32_t id,FrameworkElement anchor=nullptr){
        auto it=items.find(id);if(it==items.end())return;
        auto kind=str(object(it->second.entry,L"item"),L"kind");
        if(kind==L"capy"||kind==L"settings"){data->dispatch(invoke(kind==L"capy"?L"zen_mode":L"settings"));return;}
        if(kind==L"document_title"){showDrawings(anchor);return;}
        if(kind==L"tool"){data->dispatch(O({{L"type",S(L"activate_header_item")},{L"id",N(id)}}));return;}
        if(kind==L"menu"||kind==L"menu_labels"||kind==L"workspaces"){
            auto flyout=kind==L"workspaces"?workspaceOverflow.Flyout():primary.Flyout();
            flyout.ShowAt(anchor?anchor:it->second.frame.as<FrameworkElement>());
        }
    }
    void applyItems(){
        std::map<uint32_t,hstring> incoming;
        for(auto zone:array(object(view,L"model"),L"zones"))for(auto value:zone.GetArray()){
            auto entry=value.GetObject();incoming.emplace(uint32_t(num(entry,L"id")),object(entry,L"item").Stringify());
        }
        // Release removed singleton parents before a replacement entry uses them.
        for(auto it=items.begin();it!=items.end();){
            auto next=incoming.find(it->first);
            if(next!=incoming.end()&&next->second==it->second.key){++it;continue;}
            it->second.content.Children().Clear();
            uint32_t at;if(canvas.Children().IndexOf(it->second.frame,at))canvas.Children().RemoveAt(at);
            it=items.erase(it);
        }
        bool status=false;
        for(auto zone:array(object(view,L"model"),L"zones"))for(auto value:zone.GetArray()){
            auto entry=value.GetObject();auto id=uint32_t(num(entry,L"id"));
            auto item=object(entry,L"item");auto kind=str(item,L"kind");status|=kind==L"clock"||kind==L"battery";
            auto [it,added]=items.try_emplace(id);auto& native=it->second;auto key=item.Stringify();
            if(added){
                native.key=key;native.view=control(id,item);native.view.HorizontalAlignment(HorizontalAlignment::Stretch);native.frame.Background(clear());native.frame.CornerRadius({6,6,6,6});native.frame.UseLayoutRounding(false);
                ColumnDefinition grip;grip.Width({0,GridUnitType::Pixel});native.content.ColumnDefinitions().Append(grip);
                ColumnDefinition body;body.Width({1,GridUnitType::Star});native.content.ColumnDefinitions().Append(body);
                native.grip=panelGrip(data->theme());native.content.Children().Append(native.grip);Grid::SetColumn(native.view,1);native.content.Children().Append(native.view);
                Grid layers;layers.Children().Append(native.content);
                native.editor=button(data,L"",[weak=weak_from_this(),id]{if(auto self=weak.lock())self->input->Select(id);});
                native.editor.Background(clear());native.editor.HorizontalAlignment(HorizontalAlignment::Stretch);native.editor.VerticalAlignment(VerticalAlignment::Stretch);
                layers.Children().Append(native.editor);
                native.outline.IsHitTestVisible(false);native.outline.CornerRadius({6,6,6,6});layers.Children().Append(native.outline);native.frame.Child(layers);
                AutomationProperties::SetAutomationId(native.frame,L"header-item-"+to_hstring(id));
                AutomationProperties::SetAutomationId(native.editor,L"header-select-"+to_hstring(id));
                canvas.Children().Append(native.frame);Canvas::SetZIndex(native.frame,5);
            }
            native.entry=entry;auto spec=findId(array(view,L"items"),id);auto label=str(spec,L"label");
            input->Source(native.frame,O({{L"kind",S(L"item")},{L"value",N(id)}}),label);
            AutomationProperties::SetName(native.editor,label);
            ToolTipService::SetToolTip(native.frame,box_value(kind==L"tool"&&pickerControl(object(item,L"control"))&&!editing?pickerTooltip(label):label));
            native.content.ColumnDefinitions().GetAt(0).Width({editing?20.:0.,GridUnitType::Pixel});
            native.grip.Visibility(editing?Visibility::Visible:Visibility::Collapsed);native.editor.Visibility(editing?Visibility::Visible:Visibility::Collapsed);
            native.view.IsHitTestVisible(!editing);native.editor.IsTabStop(editing);
            if(auto pick=native.view.try_as<Button>()){
                auto iconName=str(spec,L"icon");
                if(kind==L"capy")iconName=str(find(array(data->state,L"commands"),L"id",L"zen_mode"),L"icon");
                else if(kind==L"settings")iconName=L"settings";else if(kind==L"menu")iconName=L"menu";
                auto ctl=object(item,L"control");auto ctlKind=str(ctl,L"kind");
                if(kind==L"tool"&&ctlKind==L"color"){
                    if(!native.colors)native.colors=std::make_shared<ColorPair>(data);
                    if(pick.Content()!=native.colors->root){pick.Content(native.colors->root);native.iconKey=L"";}
                    native.colors->Update(data,iconSize);
                }else if(kind==L"tool"&&ctlKind==L"divider"){
                    Border line;line.Height(1);line.Margin({6,6,6,6});line.Background(data->brush(L"tabbar"));pick.Content(line);
                }else if(!iconName.empty()){
                    auto glyphSize=kind==L"capy"?tile*440./512.:iconSize;
                    auto iconKey=iconName+L":"+to_hstring(glyphSize);
                    if(native.iconKey!=iconKey){pick.Content(icon(iconName,data->theme(),glyphSize));native.iconKey=iconKey;}
                    if(kind==L"capy")pick.Padding({0,0,0,0});
                }
                pick.IsTabStop(!editing);pick.Width(tile);pick.Height(tile);pick.IsEnabled(editing||flag(spec,L"enabled",true));pick.Opacity(editing||flag(spec,L"enabled",true)?1.:.36);
                auto anchor=object(object(object(data->state,L"customization"),L"drawer"),L"anchor");
                bool open=str(anchor,L"kind")==L"header"&&num(anchor,L"id")==id;
                pick.Background(flag(spec,L"selected")?data->glass(L"header_selection"):open?data->glass(L"panel"):clear());
                double r=corner();pick.CornerRadius(open?CornerRadius{r,r,0,0}:CornerRadius{r,r,r,r});
                AutomationProperties::SetItemStatus(pick,open?L"Open":flag(spec,L"selected")?L"On":L"Off");
                AutomationProperties::SetName(pick,label);
            }
        }
        // The editor overlay owns keyboard activation as well as pointer input.
        for(auto const& item:menus)item.IsTabStop(!editing);
        menuOverflow.IsTabStop(!editing);workspaceOverflow.IsTabStop(!editing);
        for(auto const& [item,id]:workspaces)item.IsTabStop(!editing);
        systemStatus->Apply(fullscreenActive,hidden||!status,editing);
    }
    void buildBank(){
        auto model=object(view,L"model");auto key=array(model,L"zones").Stringify();
        if(key==bankKey)return;bankKey=key;bankContent.Children().Clear();bankParts.clear();sizes.clear();
        A components;components.Append(O({{L"item",O({{L"kind",S(L"tools")}})},{L"label",S(L"Add Tools…")},{L"singleton",B(false)}}));
        for(auto component:array(view,L"components"))components.Append(component);
        for(auto value:components){
            auto component=value.GetObject();auto item=object(component,L"item");bool exists=false;
            if(flag(component,L"singleton"))for(auto const& [id,native]:items)exists|=object(native.entry,L"item").Stringify()==item.Stringify();
            if(exists)continue;
            auto kind=str(item,L"kind"),labelText=str(component,L"label");Border chip;chip.Background(buttonBackground(data));chip.CornerRadius({6,6,6,6});chip.Height(36);
            StackPanel content;content.Orientation(Orientation::Horizontal);content.Padding({0,0,8,0});content.VerticalAlignment(VerticalAlignment::Center);
            auto grip=panelGrip(data->theme());grip.Width(20);content.Children().Append(grip);content.Children().Append(label(data,labelText));
            chip.Child(content);chip.Width(textWidth(labelText)+28);
            input->Source(chip,kind==L"tools"?O({{L"kind",S(L"tools")}}):O({{L"kind",S(L"component")},{L"value",item}}),labelText);
            AutomationProperties::SetAutomationId(chip,L"header-component-"+kind);bankContent.Children().Append(chip);bankParts.push_back(chip);
        }
        editorControls=StackPanel();editorControls.Orientation(Orientation::Horizontal);editorControls.Spacing(6);editorControls.Height(36);
        sizeChoices=StackPanel();sizeChoices.Orientation(Orientation::Horizontal);
        for(auto value:array(view,L"sizes")){
            auto size=value.GetObject();auto id=str(size,L"id");auto choice=button(data,str(size,L"label"),[data=data,id]{data->dispatch(edit(O({{L"type",S(L"set_size")},{L"size",S(id)}})));});
            choice.Padding({8,0,8,0});choice.Height(36);AutomationProperties::SetAutomationId(choice,L"header-size-"+id);
            sizeChoices.Children().Append(choice);sizes.emplace_back(choice,id);
        }
        editorControls.Children().Append(sizeChoices);
        footer=CheckBox();footer.Content(box_value(L"Show footer"));footer.MinWidth(0);footer.MinHeight(0);footer.Height(36);footer.FontSize(data->textSize());
        AutomationProperties::SetAutomationId(footer,L"header-show-footer");
        auto change=[weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&!self->applying)
            self->data->dispatch(edit(O({{L"type",S(L"canvas_info")},{L"visible",B(self->footer.IsChecked().Value())}})));
        };
        footer.Checked(change);footer.Unchecked(change);editorControls.Children().Append(footer);
        for(auto done:{false,true}){
            auto item=button(data,done?L"Done":L"Cancel",[data=data,done]{
                data->dispatch(edit(done?O({{L"type",S(L"edit")},{L"editing",B(false)}}):O({{L"type",S(L"cancel")}})));
            });item.Padding({10,0,10,0});item.Height(36);item.Background(done?accent(data):clear());if(done)item.Foreground(data->brush(L"accent_foreground"));
            AutomationProperties::SetAutomationId(item,done?L"header-edit-done":L"header-edit-cancel");editorControls.Children().Append(item);
        }
        bankContent.Children().Append(editorControls);
    }
    void layoutBank(double width){
        if(!editing){totalHeight=height;bank.Visibility(Visibility::Collapsed);return;}
        buildBank();bank.Visibility(Visibility::Visible);
        double available=std::max(1.,width-24),x=0,y=0;
        for(auto const& part:bankParts){double w=std::min(available,part.Width());if(x>0&&x+w>available){x=0;y+=42;}
            place(part,O({{L"x",N(x)},{L"y",N(y)},{L"width",N(w)},{L"height",N(36)}}));x+=w+6;
        }
        editorControls.Measure({std::numeric_limits<float>::infinity(),36});double trailing=std::min(available,double(editorControls.DesiredSize().Width));
        if(x>0&&x+trailing>available)y+=42;
        place(editorControls,O({{L"x",N(available-trailing)},{L"y",N(y)},{L"width",N(trailing)},{L"height",N(36)}}));
        bankContent.Width(available);bankContent.Height(y+36);
        double windowHeight=root.XamlRoot()?root.XamlRoot().Size().Height:480;
        double shown=std::min(y+48,std::max(48.,std::min(230.,windowHeight-height-24)));
        place(bank,O({{L"x",N(6)},{L"y",N(height+6)},{L"width",N(std::max(1.,width-12))},{L"height",N(shown)}}));
        totalHeight=height+shown+12;
        for(auto const& [item,id]:sizes)item.Background(id==str(object(view,L"model"),L"size")?selected(data):clear());
        footer.IsChecked(flag(object(object(object(data->state,L"workspace"),L"layout"),L"canvas_info"),L"visible",true));
    }
    void reflow(){
        if(!built||applying||root.ActualWidth()<=0)return;
        applying=true;struct Reset{bool& value;~Reset(){value=false;}}reset{applying};
        double width=root.ActualWidth();layoutBank(width);root.Height(totalHeight);
        place(background,O({{L"x",N(0)},{L"y",N(0)},{L"width",N(width)},{L"height",N(height)}}));
        background.Visibility(hidden?Visibility::Collapsed:Visibility::Visible);
        A metrics;auto model=object(view,L"model");
        for(auto const& [id,native]:items){
            auto kind=str(object(native.entry,L"item"),L"kind");double natural=tile,compact=tile;
            if(kind==L"menu_labels")natural=menuWidth;
            else if(kind==L"workspaces")natural=switchWidth;
            else if(kind==L"document_title"){
                if(drawings->single()){natural=textWidth(drawings->plainTitle(),false,true)+16;compact=std::min(natural,80.);}
                else{natural=std::clamp(double(array(object(data->model,L"windows_tabs"),L"tabs").Size())*180.,180.,720.);compact=140.;}
            }
            else if(kind==L"clock")natural=fullscreenActive||editing?textWidth(editing&&!fullscreenActive?L"Clock":systemStatus->Clock().Child().as<TextBlock>().Text())+12:0;
            else if(kind==L"battery")natural=editing||(fullscreenActive&&systemStatus->HasBattery())?tile:0;
            if(kind==L"clock"||kind==L"battery")compact=natural;
            double grip=editing?20.:0.;metrics.Append(O({{L"id",N(id)},{L"width",N(natural+grip)},{L"compact",N(compact+grip)}}));
        }
        bool navigation=false;
        for(auto const& [id,native]:items){auto kind=str(object(native.entry,L"item"),L"kind");navigation|=kind==L"capy"||kind==L"menu"||kind==L"menu_labels";}
        A insets;insets.Append(N(leftInset+(!editing&&!navigation?tile+6:0)));insets.Append(N(rightInset));
        configuration=O({{L"op",S(L"geometry")},{L"width",N(width)},{L"insets",insets},{L"metrics",metrics}});
        input->Configure(model,editing,configuration);
        auto key=model.Stringify()+configuration.Stringify()+(editing?L":editing":L":normal");
        desiredKey=key;
        if(key!=geometryKey&&!queryBusy){
            queryBusy=true;
            bool accepted=QueryWorkspace(data->query,O({{L"type",S(L"header")},{L"request",configuration}}),
                [weak=weak_from_this(),key](J reply){if(auto self=weak.lock()){
                    self->queryBusy=false;
                    if(self->desiredKey==key){
                        auto next=object(reply,L"result");
                        if(next.Size()){self->geometry=next;self->geometryKey=key;self->present();self->publish();}
                    }
                    self->schedule();
                }});
            if(!accepted){queryBusy=false;geometryTimer.Start();}
            else geometryTimer.Stop();
        }
        present();publish();
    }
    void publish(){
        if(!geometry.Size()||geometryKey!=desiredKey||input->Busy())return;
        auto measured=array(geometry,L"items");A bounds=A::Parse(measured.Stringify());
        auto hiddenItems=array(geometry,L"hidden"),more=array(geometry,L"overflow");
        for(uint32_t zone=0;zone<std::min(hiddenItems.Size(),more.Size());++zone){
            if(more.GetAt(zone).ValueType()!=JsonValueType::Object)continue;
            for(auto id:hiddenItems.GetArrayAt(zone))bounds.Append(O({{L"id",id},{L"bounds",more.GetObjectAt(zone)}}));
        }
        auto action=O({{L"type",S(L"measure_header")},{L"height",N(totalHeight)},{L"items",bounds}});
        auto key=action.Stringify();if(key!=lastMeasurement){lastMeasurement=key;data->dispatch(action);}
    }
    void present(){
        if(!built)return;
        auto preview=input->Preview();auto resolved=object(preview,L"geometry");if(!resolved.Size())resolved=geometry;
        auto source=input->Source();uint32_t held=str(source,L"kind")==L"item"?uint32_t(num(source,L"value")):0;
        auto placed=array(resolved,L"items");auto metrics=array(configuration,L"metrics");
        auto barList=array(resolved,L"bars");std::set<uint32_t> joined;std::array<bool,3> overflowJoined{};
        for(auto value:barList){
            auto bar=value.GetObject();for(auto member:array(bar,L"items"))joined.insert(uint32_t(member.GetNumber()));
            auto zone=bar.GetNamedValue(L"overflow",JsonValue::CreateNullValue());
            if(zone.ValueType()==JsonValueType::Number&&zone.GetNumber()>=0&&zone.GetNumber()<3)overflowJoined[size_t(zone.GetNumber())]=true;
        }
        while(bars.size()<barList.Size()){Border bar;bar.IsHitTestVisible(false);bar.Background(headerSurface(data));canvas.Children().Append(bar);Canvas::SetZIndex(bar,4);bars.push_back(bar);}
        for(size_t i=0;i<bars.size();++i){
            bool shown=i<barList.Size()&&!hidden;bars[i].Visibility(shown?Visibility::Visible:Visibility::Collapsed);
            if(shown){place(bars[i],object(barList.GetObjectAt(uint32_t(i)),L"bounds"));bars[i].CornerRadius({corner(),corner(),corner(),corner()});}
        }
        for(auto& [id,native]:items){
            auto bounds=id==held&&preview.Size()?object(preview,L"held"):object(findId(placed,id),L"bounds");
            bool visible=bounds.Size()&&!hidden;native.frame.Visibility(visible?Visibility::Visible:Visibility::Collapsed);
            if(!visible)continue;
            // Only drag neighbors animate. Normal controls and native caption
            // hit regions must adopt a completed layout together.
            if(!preview.Size()||id==held)native.frame.Transitions().Clear();
            else if(!native.frame.Transitions().Size())native.frame.Transitions().Append(Media::Animation::RepositionThemeTransition());
            place(native.frame,bounds);Canvas::SetZIndex(native.frame,id==held?30:5);
            bool compact=num(bounds,L"width")<num(findId(metrics,id),L"width")-.5;
            auto kind=str(object(native.entry,L"item"),L"kind");
            if(kind==L"menu_labels"){menuCapsule.Visibility(compact?Visibility::Collapsed:Visibility::Visible);menuOverflow.Visibility(compact?Visibility::Visible:Visibility::Collapsed);}
            bool inBar=joined.contains(id);
            auto drawerAnchor=object(object(object(data->state,L"customization"),L"drawer"),L"anchor");
            bool open=str(drawerAnchor,L"kind")==L"header"&&num(drawerAnchor,L"id")==id&&!flag(findId(array(view,L"items"),id),L"selected");
            bool ownSurface=kind==L"document_title"||kind==L"clock"||kind==L"battery"||kind==L"space"||(!compact&&(kind==L"menu_labels"||kind==L"workspaces"));
            native.frame.Background(inBar||ownSurface||open?clear():headerSurface(data));native.frame.CornerRadius({corner(),corner(),corner(),corner()});native.outline.CornerRadius({corner(),corner(),corner(),corner()});
            if(kind==L"workspaces"){switcher.Visibility(compact?Visibility::Collapsed:Visibility::Visible);workspaceOverflow.Visibility(compact?Visibility::Visible:Visibility::Collapsed);}
            native.outline.BorderThickness(editing?Thickness{1,1,1,1}:Thickness{});
            native.outline.BorderBrush(held==id&&flag(preview,L"detached")?fill(color(L"#dc3545")):input->Selected()==id?accent(data):data->brush(L"tabbar"));
            AutomationProperties::SetItemStatus(native.frame,editing&&input->Selected()==id?L"Selected":L"");
        }
        auto zoneBounds=array(resolved,L"zones"),more=array(resolved,L"overflow");
        for(uint32_t i=0;i<3;++i){
            zones[i].Visibility(editing&&!hidden&&i<zoneBounds.Size()?Visibility::Visible:Visibility::Collapsed);
            if(i<zoneBounds.Size())place(zones[i],zoneBounds.GetObjectAt(i));zones[i].BorderBrush(data->brush(L"tabbar"));
            bool show=!hidden&&i<more.Size()&&more.GetAt(i).ValueType()==JsonValueType::Object;
            overflow[i].Visibility(show?Visibility::Visible:Visibility::Collapsed);if(show)place(overflow[i],more.GetObjectAt(i));
            overflow[i].Background(overflowJoined[i]?clear():headerSurface(data));overflow[i].CornerRadius({corner(),corner(),corner(),corner()});
        }
        ghost.Visibility(preview.Size()&&!held?Visibility::Visible:Visibility::Collapsed);
        if(preview.Size()&&!held){
            place(ghost,object(preview,L"held"));ghost.BorderBrush(flag(preview,L"detached")?fill(color(L"#dc3545")):accent(data));
            hstring text=L"Add Tools…";
            if(str(source,L"kind")==L"component")for(auto v:array(view,L"components"))if(object(v.GetObject(),L"item").Stringify()==object(source,L"value").Stringify())text=str(v.GetObject(),L"label");
            ghost.Child(label(data,text));ghost.Padding({6,6,6,6});
        }
        bool navigation=false;for(auto const& [id,native]:items){auto k=str(object(native.entry,L"item"),L"kind");navigation|=k==L"capy"||k==L"menu"||k==L"menu_labels";}
        recovery.Visibility(!hidden&&!navigation&&!editing?Visibility::Visible:Visibility::Collapsed);
        place(recovery,O({{L"x",N(leftInset+6)},{L"y",N(6)},{L"width",N(tile)},{L"height",N(tile)}}));
        bank.Visibility(editing&&!hidden?Visibility::Visible:Visibility::Collapsed);
        if(editing&&input->Selected()!=focusedItem){
            focusedItem=input->Selected();if(auto it=items.find(focusedItem);it!=items.end()&&it->second.frame.Visibility()==Visibility::Visible)it->second.editor.Focus(FocusState::Programmatic);
        }
        evidence();if(changed)changed();
    }
    void evidence(){
        if(!trace||!built||!canvas.IsLoaded())return;A actual;
        for(auto const& [id,native]:items)if(native.frame.Visibility()==Visibility::Visible)
            actual.Append(O({{L"id",N(id)},{L"bounds",rectangle(visibleBounds(native.frame,canvas))}}));
        auto value=O({{L"geometry",geometry},{L"metrics",array(configuration,L"metrics")},{L"actual_items",actual},
            {L"height",N(height)},{L"total_height",N(totalHeight)},{L"editing",B(editing)}});
        auto key=value.Stringify();if(key!=traceKey){traceKey=key;AutomationProperties::SetItemStatus(canvas,key);}
    }
    void apply(J const& snapshot){
        if(!snapshot.HasKey(L"state"))return;
        data->model=snapshot;data->state=object(snapshot,L"state");data->refreshPalette();view=object(snapshot,L"header");
        if(!view.Size())return;
        editing=flag(view,L"editing");hidden=flag(snapshot,L"chrome_hidden")&&!flag(snapshot,L"windows_rendering_suspended");
        auto size=find(array(view,L"sizes"),L"id",str(object(view,L"model"),L"size"));tile=num(size,L"tile",36);iconSize=num(size,L"icon",20);height=num(size,L"height",48);
        auto nextTheme=data->theme(),nextPalette=object(data->state,L"palette").Stringify();
        if(!built||nextTheme!=theme||nextPalette!=palette){theme=nextTheme;palette=nextPalette;build();}
        root.RequestedTheme(theme==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        root.TabFocusNavigation(editing?Input::KeyboardNavigationMode::Cycle:Input::KeyboardNavigationMode::Local);
        drawings->refresh();
        applyWorkspaces();switchWidth=8+2*std::max(0,int(workspaces.size())-1);
        for(auto const& [item,id]:workspaces){item.Width(std::min(130.,unbox_value<double>(item.Tag())+20));switchWidth+=item.Width();}
        switchWidth=std::clamp(switchWidth,tile,480.);
        for(auto const& item:overflow)item.Content(icon(L"menu",theme,iconSize));
        for(auto item:{menuOverflow,workspaceOverflow,recovery}){item.Content(icon(L"menu",theme,iconSize));item.Width(tile);}
        applyItems();reflow();requests();
    }
    double corner()const{return tile*.5*CornerFit;}
    void glass(A& regions,DependencyObject const& node,UIElement const& reference)const{
        auto element=node.try_as<FrameworkElement>();if(!element||element.Visibility()!=Visibility::Visible)return;
        Brush surface{nullptr};CornerRadius corners{};
        if(auto border=node.try_as<Border>()){surface=border.Background();corners=border.CornerRadius();}
        else if(auto grid=node.try_as<Grid>()){surface=grid.Background();corners=grid.CornerRadius();}
        else if(auto stack=node.try_as<StackPanel>()){surface=stack.Background();corners=stack.CornerRadius();}
        else if(auto control=node.try_as<Control>()){surface=control.Background();corners=control.CornerRadius();}
        if(surface)for(auto role:{L"chip",L"switcher",L"panel"})if(surface==data->glass(role)){appendGlass(regions,element,reference,cornerRadii(corners));return;}
        for(int i=0,count=VisualTreeHelper::GetChildrenCount(node);i<count;++i)glass(regions,VisualTreeHelper::GetChild(node,i),reference);
    }
    std::vector<Windows::Graphics::RectInt32> drag(float scale,uint32_t width)const{
        if(editing)return {};
        std::vector<std::pair<float,float>> controls;
        auto take=[&](FrameworkElement item){
            if(item.Visibility()!=Visibility::Visible||item.ActualWidth()<=0)return;
            auto box=item.TransformToVisual(root).TransformBounds({0,0,float(item.ActualWidth()),float(item.ActualHeight())});controls.emplace_back(box.X,box.X+box.Width);
        };
        for(auto const& [id,item]:items){
            auto kind=str(object(item.entry,L"item"),L"kind");
            if(kind!=L"clock"&&kind!=L"battery"&&kind!=L"space"&&!(kind==L"document_title"&&drawings->single()))take(item.frame);
        }
        for(auto item:overflow)take(item);take(recovery);
        std::sort(controls.begin(),controls.end());float next=leftInset,limit=float(width)/scale-rightInset;
        std::vector<Windows::Graphics::RectInt32> result;
        auto add=[&](float from,float to){int32_t x=int32_t(std::ceil(from*scale)),end=int32_t(std::floor(to*scale));
            if(end>x)result.push_back({x,0,end-x,int32_t(std::lround(height*scale))});
        };
        for(auto [left,right]:controls){if(left>next)add(next,std::min(left,limit));next=std::max(next,right);}
        if(next<limit)add(next,limit);return result;
    }
};
HeaderView::HeaderView(Dispatch send,Json catalog,std::function<void(bool)> popup,std::function<void()> layout,
    std::function<void()> fullscreen,std::function<void()> newWindow,PreviewTransport queries,Dispatch input,Dispatch documents):impl(std::make_shared<Impl>()){
    impl->data->send=std::move(send);impl->data->catalog=catalog;impl->data->query=std::move(queries);impl->data->input=std::move(input);impl->data->document=std::move(documents);
    impl->data->popupChanged=std::move(popup);impl->changed=std::move(layout);impl->fullscreen=std::move(fullscreen);impl->newWindow=std::move(newWindow);impl->init();
}
HeaderView::~HeaderView()=default;
Grid HeaderView::Root()const{return impl->root;}
void HeaderView::Apply(Json const& snapshot){impl->apply(snapshot);}
void HeaderView::SetInsets(float left,float right){if(left!=impl->leftInset||right!=impl->rightInset){impl->leftInset=left;impl->rightInset=right;impl->schedule();}}
void HeaderView::SetFullscreen(bool active){
    if(active==impl->fullscreenActive)return;
    impl->fullscreenActive=active;
    impl->data->dispatch(O({{L"type",S(L"window_fullscreen")},{L"fullscreen",B(active)}}));
    if(impl->built)impl->applyItems();
    impl->schedule();
}
void HeaderView::SetBlocked(bool blocked){impl->data->externalPopup=blocked;if(blocked)impl->input->Cancel();}
bool HeaderView::Key(Input::KeyRoutedEventArgs const& e,bool pressed){
    using Windows::System::VirtualKey;auto key=e.Key();bool shift=GetKeyState(VK_SHIFT)&0x8000;
    if(pressed&&(GetKeyState(VK_CONTROL)&0x8000)&&!(GetKeyState(VK_MENU)&0x8000)&&impl->drawings&&impl->drawings->available()){
        if(key==VirtualKey::Tab||key==VirtualKey::PageDown||key==VirtualKey::PageUp){
            e.Handled(true);impl->drawings->send(O({{L"op",S(L"adjacent")},{L"forward",B(key==VirtualKey::PageDown||(key==VirtualKey::Tab&&!shift))}}));return true;
        }
        if(key==VirtualKey::A&&shift){e.Handled(true);impl->showDrawings();return true;}
    }
    return impl->input->Key(e,pressed);
}
void HeaderView::AppendGlass(A& regions,UIElement const& reference)const{if(impl->built&&!impl->hidden&&!impl->editing)impl->glass(regions,impl->root,reference);}
std::vector<Windows::Graphics::RectInt32> HeaderView::DragRegions(float scale,uint32_t width)const{return impl->built?impl->drag(scale,width):std::vector<Windows::Graphics::RectInt32>{};}

std::vector<Windows::Graphics::RectInt32> HeaderView::InputRegions(float scale,uint32_t width)const{
    if(!impl->built||scale<=0)return {};
    auto caption=impl->drag(scale,width);
    int32_t next=int32_t(std::ceil(impl->leftInset*scale));
    int32_t limit=int32_t(width)-int32_t(std::ceil(impl->rightInset*scale));
    int32_t height=int32_t(std::lround(impl->height*scale));
    std::vector<Windows::Graphics::RectInt32> result;
    for(auto region:caption){
        if(region.X>next)result.push_back({next,0,region.X-next,height});
        next=std::max(next,region.X+region.Width);
    }
    if(next<limit)result.push_back({next,0,limit-next,height});
    return result;
}
