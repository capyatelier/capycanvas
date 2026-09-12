#include "pch.h"
#include "HeaderView.h"
#include "HeaderStatus.h"
#include "UiControls.h"
#include "NativeMenus.h"
#include "WorkspaceQuery.h"
#include <chrono>
#include <set>
#include <winrt/Windows.Graphics.h>
#include <winrt/Windows.UI.Text.h>
#include <array>
#include <limits>

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Media;
using namespace CapyUi;
namespace {
struct PopupState {int count=0;std::function<void(bool)> changed;};
J invoke(hstring const& command){return O({{L"type",S(L"invoke")},{L"command",S(command)}});}
void menuItems(Windows::Foundation::Collections::IVector<MenuFlyoutItemBase> const& target,
    A const& sections,std::shared_ptr<WorkspaceData> const& data) {
    NativeMenuItems(target,sections,data,[data](J action){data->dispatch(action);});
}
Windows::UI::Color blend(Windows::UI::Color bg,Windows::UI::Color ink,float amount){
    return {255,uint8_t(std::lround(bg.R+(ink.R-bg.R)*amount)),uint8_t(std::lround(bg.G+(ink.G-bg.G)*amount)),uint8_t(std::lround(bg.B+(ink.B-bg.B)*amount))};
}
void style(Button const& item,std::shared_ptr<WorkspaceData> const& data) {
    auto bg=color(str(object(data->state,L"palette"),L"bg",L"#333333"));
    auto text=color(str(object(data->state,L"palette"),L"text",L"#fafafb"));
    item.Background(fill(bg));item.Height(36);item.Padding({6,0,6,0});item.UseLayoutRounding(false);
    item.Resources().Insert(box_value(L"ButtonBackgroundPointerOver"),fill(blend(bg,text,.08f)));
    item.Resources().Insert(box_value(L"ButtonBackgroundPressed"),fill(blend(bg,text,.16f)));
}
}
struct HeaderView::Impl : std::enable_shared_from_this<Impl> {
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    std::shared_ptr<PopupState> popups=std::make_shared<PopupState>();
    std::function<void()> changed,fullscreen,newWindow;
    Grid root;
    StackPanel start,end,menuLabels;
    std::vector<Button> menus;
    Button menuOverflow;
    std::unique_ptr<HeaderStatus> systemStatus;
    bool reflowing=false,menusCollapsed=false;
    double menuNaturalWidth=0,measuredMenuFont=-1,measuredMenuGap=-1;
    Border document,switcher;
    StackPanel switches;
    std::vector<std::pair<Primitives::ToggleButton,hstring>> workspaces;
    TextBlock title;
    Button zen,settings,screen;
    bool fullscreenActive=false;
    std::vector<std::pair<Button,hstring>> commands;
    hstring theme,palette;
    float leftInset=0,rightInset=0,titleWidth=0;
    bool hidden=false,keepZen=true,built=false,resolvingLink=false;
    std::set<uint32_t> handledRequests;
    Microsoft::UI::Dispatching::DispatcherQueueTimer requestTimer{nullptr};
    ~Impl(){if(requestTimer)requestTimer.Stop();}
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
            if(type==L"set_fullscreen"){
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
    void init() {
        root.Height(48);root.VerticalAlignment(VerticalAlignment::Top);root.UseLayoutRounding(false);
        AutomationProperties::SetName(root,L"Application header");
        root.SizeChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->reflow();});
        // Rebuilt siblings and margin changes can move controls without a new
        // SizeChanged event. Publish hit regions from completed native arrange.
        root.LayoutUpdated([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&self->changed)self->changed();});
        requestTimer=root.DispatcherQueue().CreateTimer();requestTimer.Interval(std::chrono::milliseconds(200));
        requestTimer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->requests();});
        root.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->requests();});
        root.Unloaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->requestTimer.Stop();});
    }
    void build(){
        root.Children().Clear();commands.clear();workspaces.clear();menus.clear();
        start=StackPanel();end=StackPanel();menuLabels=StackPanel();measuredMenuFont=-1;
        start.UseLayoutRounding(false);end.UseLayoutRounding(false);menuLabels.UseLayoutRounding(false);
        menuLabels.Orientation(Orientation::Horizontal);
        start.Orientation(Orientation::Horizontal);start.Spacing(6);start.HorizontalAlignment(HorizontalAlignment::Left);
        end.Orientation(Orientation::Horizontal);end.Spacing(6);end.HorizontalAlignment(HorizontalAlignment::Right);
        start.VerticalAlignment(VerticalAlignment::Top);end.VerticalAlignment(VerticalAlignment::Top);
        Border spacer;spacer.Width(36);spacer.Height(36);start.Children().Append(spacer);
        auto layout=[weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->reflow();};
        start.SizeChanged(layout);end.SizeChanged(layout);
        for(auto value:array(data->model,L"application_menus")) {
            auto spec=value.GetObject();auto item=button(data,str(spec,L"label"),[]{});style(item,data);
            AutomationProperties::SetAutomationId(item,L"application-menu-"+str(spec,L"id"));
            MenuFlyout flyout;
            flyout.Opening([data=data,id=str(spec,L"id")](Windows::Foundation::IInspectable const& sender,auto&&){
                auto menu=sender.as<MenuFlyout>();menu.Items().Clear();
                auto current=find(array(data->model,L"application_menus"),L"id",id);
                menuItems(menu.Items(),array(object(current,L"model"),L"sections"),data);
            });
            flyout.Opened([state=popups](auto&&,auto&&){++state->count;state->changed(true);});
            flyout.Closed([state=popups](auto&&,auto&&){state->count=std::max(0,state->count-1);state->changed(state->count>0);});
            item.Flyout(flyout);menuLabels.Children().Append(item);menus.push_back(item);
        }
        start.Children().Append(menuLabels);
        menuOverflow=button(data,L"Menus",[]{});style(menuOverflow,data);menuOverflow.Width(36);menuOverflow.Padding({0});
        menuOverflow.Content(icon(L"menu",data->theme()));menuOverflow.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(menuOverflow,L"application-menus");
        MenuFlyout all;
        all.Opening([data=data](Windows::Foundation::IInspectable const& sender,auto&&){
            auto flyout=sender.as<MenuFlyout>();flyout.Items().Clear();
            for(auto value:array(data->model,L"application_menus")){
                auto spec=value.GetObject();MenuFlyoutSubItem item;item.Text(str(spec,L"label"));item.FontSize(data->textSize());
                AutomationProperties::SetAutomationId(item,L"application-menu-"+str(spec,L"id"));
                menuItems(item.Items(),array(object(spec,L"model"),L"sections"),data);flyout.Items().Append(item);
            }
        });
        all.Opened([state=popups](auto&&,auto&&){++state->count;state->changed(true);});
        all.Closed([state=popups](auto&&,auto&&){state->count=std::max(0,state->count-1);state->changed(state->count>0);});
        menuOverflow.Flyout(all);start.Children().Append(menuOverflow);
        zen=command(L"zen_mode",num(data->catalog,L"zen_icon_size",28));
        AutomationProperties::SetAutomationId(zen,L"zen-button");
        zen.HorizontalAlignment(HorizontalAlignment::Left);zen.VerticalAlignment(VerticalAlignment::Top);
        settings=command(L"settings",16);
        AutomationProperties::SetAutomationId(settings,L"settings-button");
        screen=button(data,L"Full screen",fullscreen);style(screen,data);screen.Width(36);screen.Padding({0});
        AutomationProperties::SetAutomationId(screen,L"fullscreen");
        screen.Content(icon(fullscreenActive?L"fullscreen-exit":L"fullscreen-enter",data->theme()));
        auto screenLabel=fullscreenActive?L"Exit full screen":L"Full screen";
        AutomationProperties::SetName(screen,screenLabel);ToolTipService::SetToolTip(screen,box_value(screenLabel));
        switcher=Border();switches=StackPanel();switcher.UseLayoutRounding(false);switches.UseLayoutRounding(false);switches.Orientation(Orientation::Horizontal);switches.Spacing(2);
        switcher.Child(switches);switcher.Height(34);switcher.Padding({4,4,4,4});switcher.CornerRadius({18,18,18,18});
        auto bg=color(str(object(data->state,L"palette"),L"bg",L"#333333"));
        switcher.Background(fill(blend(bg,{255,0,0,0},.20f)));switcher.BorderThickness({0});
        AutomationProperties::SetAutomationId(switcher,L"workspace-switcher");
        AutomationProperties::SetName(switcher,L"Task workspaces");
        switcher.HorizontalAlignment(HorizontalAlignment::Left);switcher.VerticalAlignment(VerticalAlignment::Top);
        systemStatus=std::make_unique<HeaderStatus>(data,[weak=weak_from_this()]{if(auto self=weak.lock())self->reflow();});
        end.Children().Append(systemStatus->Root());end.Children().Append(screen);end.Children().Append(settings);
        title=label(data,L"");title.FontWeight(Windows::UI::Text::FontWeights::SemiBold());
        title.VerticalAlignment(VerticalAlignment::Center);title.IsTextSelectionEnabled(true);
        title.TextAlignment(TextAlignment::Center);title.TextTrimming(TextTrimming::CharacterEllipsis);
        document=Border();document.UseLayoutRounding(false);document.Child(title);document.Background(data->brush(L"bg"));
        document.Padding({6,0,6,0});document.CornerRadius({6,6,6,6});document.Height(36);
        document.HorizontalAlignment(HorizontalAlignment::Stretch);document.VerticalAlignment(VerticalAlignment::Top);
        AutomationProperties::SetAutomationId(document,L"document-title");
        AutomationProperties::SetName(document,L"Document title");
        root.Children().Append(document);root.Children().Append(start);root.Children().Append(switcher);root.Children().Append(end);root.Children().Append(zen);
        built=true;
    }
    Button command(hstring const& id,double size) {
        auto item=button(data,id,[data=data,id]{data->dispatch(invoke(id));});style(item,data);
        item.Width(36);item.Padding({0});
        item.Tag(box_value(size));commands.emplace_back(item,id);return item;
    }
    void applyWorkspaces() {
        auto storage=object(data->model,L"windows_workspace");
        auto values=array(storage,L"defaults");auto active=str(storage,L"id");
        auto bg=color(str(object(data->state,L"palette"),L"bg",L"#333333"));
        auto chosen=fill(blend(bg,{255,53,132,228},.28f)),hover=fill(blend(bg,{255,53,132,228},.36f));
        for(auto value:values){
            auto choice=value.GetObject();auto id=str(choice,L"id");
            auto found=std::find_if(workspaces.begin(),workspaces.end(),[&](auto const& p){return p.second==id;});
            if(found==workspaces.end()){
                Primitives::ToggleButton item;item.UseLayoutRounding(false);item.MinWidth(0);item.MinHeight(0);item.Height(26);item.Padding({10,0,10,0});
                item.BorderThickness({0});item.CornerRadius({15,15,15,15});item.FontSize(data->textSize());
                // Chrome resolves the shared CSS medium weight to Segoe UI Semibold.
                item.FontFamily(FontFamily(L"Segoe UI"));item.FontWeight(Windows::UI::Text::FontWeights::SemiBold());
                item.Foreground(data->brush(L"text"));item.Background(fill({0,0,0,0}));
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
            content.Foreground(data->brush(L"text"));item.IsChecked(id==active);item.IsEnabled(flag(storage,L"can_switch"));
            item.Background(id==active?chosen:clear());
            item.Foreground(data->brush(L"text"));
            AutomationProperties::SetName(item,name);ToolTipService::SetToolTip(item,box_value(L"Switch to "+name+L" workspace"));
        }
        switcher.Visibility(values.Size()?Visibility::Visible:Visibility::Collapsed);
    }
    void reflow() {
        if(!built||reflowing)return;
        reflowing=true;struct Reset{bool& value;~Reset(){value=false;}}reset{reflowing};
        start.Margin({6+leftInset,6,0,0});end.Margin({0,6,6+rightInset,0});zen.Margin({6+leftInset,6,0,0});
        start.Visibility(hidden?Visibility::Collapsed:Visibility::Visible);
        end.Visibility(hidden?Visibility::Collapsed:Visibility::Visible);
        zen.Visibility(!hidden||keepZen?Visibility::Visible:Visibility::Collapsed);
        double width=root.ActualWidth(),gap=width<=850?0.:6.;
        start.Spacing(gap);end.Spacing(gap);menuLabels.Spacing(gap);
        systemStatus->Apply(fullscreenActive,hidden,gap);
        double menuFont=width<=850?12.:data->textSize();
        if(menuFont!=measuredMenuFont||gap!=measuredMenuGap){
            measuredMenuFont=menuFont;measuredMenuGap=gap;menuNaturalWidth=0;
            for(auto const& item:menus){
                item.FontSize(menuFont);
                auto measure=label(data,AutomationProperties::GetName(item),true);measure.FontSize(menuFont);measure.UseLayoutRounding(false);
                measure.Measure({std::numeric_limits<float>::infinity(),36});
                item.Width(measure.DesiredSize().Width+12);menuNaturalWidth+=item.Width();
            }
            if(!menus.empty())menuNaturalWidth+=gap*(menus.size()-1);
            menuLabels.Width(menuNaturalWidth);
        }
        double switchWidth=8+2*std::max(0,int(workspaces.size())-1);
        for(auto const& [item,id]:workspaces){
            double padding=width<=760?5.:10.,limit=width<=760?90.:130.;
            item.Padding({padding,0,padding,0});item.MaxWidth(limit);
            item.Content().as<TextBlock>().MaxWidth(limit-2*padding);
            // Measure text separately so template rounding cannot accumulate
            // across each button's fractional text width and horizontal padding.
            item.Width(std::min(limit,unbox_value<double>(item.Tag())+2*padding));switchWidth+=item.Width();
        }
        double endWidth=0;int endCount=0;
        for(auto child:end.Children())if(auto item=child.try_as<FrameworkElement>();item&&item.Visibility()==Visibility::Visible){
            item.Measure({std::numeric_limits<float>::infinity(),36});endWidth+=item.DesiredSize().Width;++endCount;
        }
        endWidth+=gap*std::max(0,endCount-1);
        double inner=std::max(0.,width-leftInset-rightInset-12);
        bool showSwitches=!workspaces.empty()&&inner>=72+gap+switchWidth+endWidth+2*gap;
        if(!showSwitches)switchWidth=0;
        switcher.Width(switchWidth);switcher.Visibility(!hidden&&showSwitches?Visibility::Visible:Visibility::Collapsed);
        bool titleVisible=width>850;
        double natural=36+gap+menuNaturalWidth;
        double gaps=gap*((showSwitches?2:1)+int(titleVisible));
        bool collapse=natural+switchWidth+endWidth+gaps>inner+.01;
        bool moveMenuFocus=false;
        if(collapse!=menusCollapsed){
            moveMenuFocus=popups->count>0;
            if(auto xaml=root.XamlRoot())for(auto focused=FocusManager::GetFocusedElement(xaml).try_as<DependencyObject>();focused;focused=VisualTreeHelper::GetParent(focused)){
                if(focused==menuLabels||focused==menuOverflow){moveMenuFocus=true;break;}
            }
            for(auto const& item:menus)if(item.Flyout())item.Flyout().Hide();
            menuOverflow.Flyout().Hide();menusCollapsed=collapse;
        }
        menuLabels.Visibility(collapse?Visibility::Collapsed:Visibility::Visible);
        menuOverflow.Visibility(collapse?Visibility::Visible:Visibility::Collapsed);
        if(moveMenuFocus&&!hidden){
            if(collapse)menuOverflow.Focus(FocusState::Programmatic);
            else if(!menus.empty())menus.front().Focus(FocusState::Programmatic);
        }
        double startWidth=collapse?72+gap:natural;start.Width(startWidth);
        double free=inner-startWidth-switchWidth-endWidth;
        double titleSpace=std::max(0.,free-gaps);
        document.Margin({leftInset+6+startWidth+gap,6,rightInset+6+endWidth+switchWidth+gap*(showSwitches?2:1),0});
        document.Visibility(!hidden&&titleVisible&&titleSpace>0?Visibility::Visible:Visibility::Collapsed);
        title.TextAlignment(fullscreenActive?TextAlignment::Right:TextAlignment::Center);
        // With no title, the shared header distributes spare space between its
        // start, workspace pill and end controls. With a title, that text flexes.
        double between=titleVisible?gap:std::max(gap,free/(showSwitches?2:1));
        double switchX=leftInset+6+startWidth+between+(titleVisible?titleSpace+gap:0);
        switcher.Margin({switchX,7,0,0});
        if(changed)changed();
    }
    void apply(J const& snapshot) {
        if(!snapshot.HasKey(L"state"))return;
        data->model=snapshot;data->state=object(snapshot,L"state");
        data->refreshPalette();
        auto nextTheme=data->theme(),nextPalette=object(data->state,L"palette").Stringify();
        if(!built||nextTheme!=theme||nextPalette!=palette){theme=nextTheme;palette=nextPalette;build();}
        root.RequestedTheme(theme==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        auto tabs=array(data->state,L"tabs");
        if(tabs.Size()){
            auto tab=tabs.GetObjectAt(0);
            auto text=str(tab,L"title")+L" · "+to_hstring(int(num(tab,L"width")))+L" × "+to_hstring(int(num(tab,L"height")));
            if(title.Text()!=text){
                title.Text(text);title.Measure({std::numeric_limits<float>::infinity(),36});
                titleWidth=title.DesiredSize().Width;
            }
        }
        for(auto const& [item,id]:commands) {
            auto state=find(array(data->state,L"commands"),L"id",id);
            item.IsEnabled(flag(state,L"enabled"));
            item.Content(icon(str(state,L"icon"),theme,unbox_value<double>(item.Tag())));
            AutomationProperties::SetName(item,str(state,L"label"));
            AutomationProperties::SetItemStatus(item,flag(state,L"selected")?L"On":L"Off");
            ToolTipService::SetToolTip(item,box_value(str(state,L"tooltip")));
        }
        hidden=flag(snapshot,L"chrome_hidden");keepZen=flag(snapshot,L"keep_zen_button",true);
        applyWorkspaces();reflow();requests();
    }
    std::vector<Windows::Graphics::RectInt32> drag(float scale,uint32_t width)const {
        std::vector<std::pair<float,float>> controls;
        for(FrameworkElement item:std::array<FrameworkElement,5>{start,switcher,end,document,zen}){
            if(item.Visibility()!=Visibility::Visible||item.ActualWidth()<=0)continue;
            auto position=item.TransformToVisual(root).TransformPoint({0,0});
            float controlWidth=float(item.ActualWidth());
            if(item==document){
                // Keep unused title space draggable; only the selectable
                // document text needs client hit testing.
                float textWidth=std::min(controlWidth,titleWidth+12.f);
                position.X+=(controlWidth-textWidth)/(fullscreenActive?1:2);controlWidth=textWidth;
            }
            controls.emplace_back(position.X,position.X+controlWidth);
        }
        // Zen toolbars are genuine client controls inside the titlebar. Exclude
        // them from native move regions as well as avoiding caption buttons.
        if(flag(data->model,L"partial_zen"))for(auto value:array(object(data->model,L"zen_toolbars"),L"sections")){
            auto bounds=object(value.GetObject(),L"bounds");
            if(num(bounds,L"y")<48&&num(bounds,L"y")+num(bounds,L"height")>0&&num(bounds,L"width")>0)
                controls.emplace_back(float(num(bounds,L"x")),float(num(bounds,L"x")+num(bounds,L"width")));
        }
        std::sort(controls.begin(),controls.end());
        float next=leftInset,limit=float(width)/scale-rightInset;
        std::vector<Windows::Graphics::RectInt32> result;
        auto add=[&](float from,float to){
            int32_t x=int32_t(std::ceil(from*scale)),end=int32_t(std::floor(to*scale));
            if(end>x)result.push_back({x,0,end-x,int32_t(std::lround(48*scale))});
        };
        for(auto [left,right]:controls){if(left>next)add(next,std::min(left,limit));next=std::max(next,right);}
        if(next<limit)add(next,limit);
        return result;
    }
};
HeaderView::HeaderView(Dispatch send,Json catalog,std::function<void(bool)> popup,
    std::function<void()> layout,std::function<void()> fullscreen,std::function<void()> newWindow,PreviewTransport queries):impl(std::make_shared<Impl>()){
    impl->data->query=std::move(queries);
    impl->data->send=std::move(send);impl->data->catalog=catalog;impl->popups->changed=std::move(popup);
    impl->changed=std::move(layout);impl->fullscreen=std::move(fullscreen);impl->newWindow=std::move(newWindow);impl->init();
}
HeaderView::~HeaderView()=default;
Grid HeaderView::Root()const{return impl->root;}
void HeaderView::Apply(Json const& snapshot){impl->apply(snapshot);}
void HeaderView::SetInsets(float left,float right){
    if(left==impl->leftInset&&right==impl->rightInset)return;
    impl->leftInset=left;impl->rightInset=right;impl->reflow();
}
std::vector<Windows::Graphics::RectInt32> HeaderView::DragRegions(float scale,uint32_t width)const{return impl->drag(scale,width);}

void HeaderView::SetFullscreen(bool active){
    if(active==impl->fullscreenActive)return;
    impl->fullscreenActive=active;
    if(impl->built){
        impl->screen.Content(icon(active?L"fullscreen-exit":L"fullscreen-enter",impl->data->theme()));
        AutomationProperties::SetName(impl->screen,active?L"Exit full screen":L"Full screen");
        ToolTipService::SetToolTip(impl->screen,box_value(active?L"Exit full screen":L"Full screen"));
        impl->reflow();
    }
}
