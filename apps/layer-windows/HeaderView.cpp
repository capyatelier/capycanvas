#include "pch.h"
#include "HeaderView.h"
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
    return {255,uint8_t(bg.R+(ink.R-bg.R)*amount),uint8_t(bg.G+(ink.G-bg.G)*amount),uint8_t(bg.B+(ink.B-bg.B)*amount)};
}
void style(Button const& item,std::shared_ptr<WorkspaceData> const& data) {
    auto bg=color(str(object(data->state,L"palette"),L"bg",L"#333333"));
    auto text=color(str(object(data->state,L"palette"),L"text",L"#fafafb"));
    item.Background(fill(bg));item.Height(36);item.Padding({17,5,17,5});
    item.Resources().Insert(box_value(L"ButtonBackgroundPointerOver"),fill(blend(bg,text,.08f)));
    item.Resources().Insert(box_value(L"ButtonBackgroundPressed"),fill(blend(bg,text,.16f)));
}
}
struct HeaderView::Impl : std::enable_shared_from_this<Impl> {
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    std::shared_ptr<PopupState> popups=std::make_shared<PopupState>();
    std::function<void()> changed,fullscreen;
    Grid root;
    StackPanel start,end;
    Border document;
    TextBlock title;
    Button zen,settings,screen;
    bool fullscreenActive=false;
    std::vector<std::pair<Button,hstring>> commands;
    hstring theme,palette;
    float leftInset=0,rightInset=0;
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
        root.Height(48);root.VerticalAlignment(VerticalAlignment::Top);
        AutomationProperties::SetName(root,L"Application header");
        root.SizeChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->reflow();});
        requestTimer=root.DispatcherQueue().CreateTimer();requestTimer.Interval(std::chrono::milliseconds(200));
        requestTimer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->requests();});
        root.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->requests();});
        root.Unloaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->requestTimer.Stop();});
    }
    void build(){
        root.Children().Clear();commands.clear();start=StackPanel();end=StackPanel();
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
            item.Flyout(flyout);start.Children().Append(item);
        }
        zen=command(L"zen_mode",num(data->catalog,L"zen_icon_size",28));
        zen.HorizontalAlignment(HorizontalAlignment::Left);zen.VerticalAlignment(VerticalAlignment::Top);
        settings=command(L"settings",16);
        screen=button(data,L"Full screen",fullscreen);style(screen,data);screen.Width(36);screen.Padding({0});
        screen.Content(icon(fullscreenActive?L"fullscreen-exit":L"fullscreen-enter",data->theme()));
        auto screenLabel=fullscreenActive?L"Exit full screen":L"Full screen";
        AutomationProperties::SetName(screen,screenLabel);ToolTipService::SetToolTip(screen,box_value(screenLabel));
        end.Children().Append(screen);end.Children().Append(settings);
        title=label(data,L"");title.FontWeight(Windows::UI::Text::FontWeights::SemiBold());
        title.VerticalAlignment(VerticalAlignment::Center);title.IsTextSelectionEnabled(true);
        document=Border();document.Child(title);document.Background(data->brush(L"bg"));
        document.Padding({8,4,8,4});document.CornerRadius({6,6,6,6});document.Height(36);
        document.HorizontalAlignment(HorizontalAlignment::Center);document.VerticalAlignment(VerticalAlignment::Top);document.Margin({0,6,0,0});
        root.Children().Append(document);root.Children().Append(start);root.Children().Append(end);root.Children().Append(zen);
        built=true;
    }
    Button command(hstring const& id,double size) {
        auto item=button(data,id,[data=data,id]{data->dispatch(invoke(id));});style(item,data);
        item.Width(36);item.Padding({0});
        item.Tag(box_value(size));commands.emplace_back(item,id);return item;
    }
    void reflow() {
        if(!built)return;
        start.Margin({6+leftInset,6,0,0});end.Margin({0,6,6+rightInset,0});zen.Margin({6+leftInset,6,0,0});
        start.Visibility(hidden?Visibility::Collapsed:Visibility::Visible);
        end.Visibility(hidden?Visibility::Collapsed:Visibility::Visible);
        zen.Visibility(!hidden||keepZen?Visibility::Visible:Visibility::Collapsed);
        title.Measure({std::numeric_limits<float>::infinity(),36});
        double width=root.ActualWidth(),half=(title.DesiredSize().Width+16)/2;
        bool fits=width>640&&width/2-half>leftInset+start.ActualWidth()+12&&
            width/2+half<width-rightInset-end.ActualWidth()-12;
        document.Visibility(!hidden&&fits?Visibility::Visible:Visibility::Collapsed);
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
            title.Text(str(tab,L"title")+L" · "+to_hstring(int(num(tab,L"width")))+L" × "+to_hstring(int(num(tab,L"height"))));
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
        reflow();requests();
    }
    std::vector<Windows::Graphics::RectInt32> drag(float scale,uint32_t width)const {
        std::vector<std::pair<float,float>> controls;
        for(FrameworkElement item:std::array<FrameworkElement,4>{start,end,document,zen}){
            if(item.Visibility()!=Visibility::Visible||item.ActualWidth()<=0)continue;
            auto position=item.TransformToVisual(root).TransformPoint({0,0});
            controls.emplace_back(position.X,position.X+float(item.ActualWidth()));
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
    std::function<void()> layout,std::function<void()> fullscreen,PreviewTransport queries):impl(std::make_shared<Impl>()){
    impl->data->query=std::move(queries);
    impl->data->send=std::move(send);impl->data->catalog=catalog;impl->popups->changed=std::move(popup);
    impl->changed=std::move(layout);impl->fullscreen=std::move(fullscreen);impl->init();
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
    }
}
