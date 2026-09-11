#include "pch.h"
#include "HeaderView.h"
#include "UiControls.h"
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
    bool populated=false;
    for(auto sectionValue:sections) {
        auto section=sectionValue.GetArray();
        if(!section.Size())continue;
        if(populated)target.Append(MenuFlyoutSeparator());
        populated=true;
        for(auto value:section) {
            auto spec=value.GetObject();auto children=array(spec,L"sections");
            auto text=str(spec,L"label");auto action=object(spec,L"action");
            if(children.Size()) {
                MenuFlyoutSubItem item;item.Text(text);item.IsEnabled(flag(spec,L"enabled",true));
                item.FontSize(data->textSize());menuItems(item.Items(),children,data);target.Append(item);
            } else {
                auto checked=spec.GetNamedValue(L"selected",JsonValue::CreateNullValue());
                if(checked.ValueType()==JsonValueType::Boolean) {
                    ToggleMenuFlyoutItem item;item.Text(text);item.IsChecked(checked.GetBoolean());
                    item.IsEnabled(flag(spec,L"enabled",true));item.FontSize(data->textSize());item.MinHeight(34);
                    item.KeyboardAcceleratorTextOverride(str(spec,L"hint"));
                    item.Click([data,action](auto&&,auto&&){if(action.Size())data->dispatch(action);});target.Append(item);
                } else {
                    MenuFlyoutItem item;item.Text(text);item.IsEnabled(flag(spec,L"enabled",true));
                    item.FontSize(data->textSize());item.MinHeight(34);
                    item.KeyboardAcceleratorTextOverride(str(spec,L"hint"));
                    item.Click([data,action](auto&&,auto&&){if(action.Size())data->dispatch(action);});target.Append(item);
                }
            }
        }
    }
}
A commandSections(A const& sections,std::shared_ptr<WorkspaceData> const& data) {
    A result;
    for(auto section:sections) {
        A items;
        for(auto id:section.GetArray()) {
            auto command=find(array(data->state,L"commands"),L"id",id.GetString());
            if(!command.Size())continue;
            items.Append(O({{L"label",S(str(command,L"label"))},{L"enabled",B(flag(command,L"enabled"))},
                {L"selected",flag(command,L"checkable")?B(flag(command,L"selected")):JsonValue::CreateNullValue()},
                {L"hint",S(str(command,L"shortcut"))},{L"action",invoke(id.GetString())}}));
        }
        result.Append(items);
    }
    return result;
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
    bool hidden=false,keepZen=true,built=false;
    void init() {
        root.Height(48);root.VerticalAlignment(VerticalAlignment::Top);
        AutomationProperties::SetName(root,L"Application header");
        root.SizeChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->reflow();});
    }
    void build(){
        root.Children().Clear();commands.clear();start=StackPanel();end=StackPanel();
        start.Orientation(Orientation::Horizontal);start.Spacing(6);start.HorizontalAlignment(HorizontalAlignment::Left);
        end.Orientation(Orientation::Horizontal);end.Spacing(6);end.HorizontalAlignment(HorizontalAlignment::Right);
        start.VerticalAlignment(VerticalAlignment::Top);end.VerticalAlignment(VerticalAlignment::Top);
        Border spacer;spacer.Width(36);spacer.Height(36);start.Children().Append(spacer);
        auto layout=[weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->reflow();};
        start.SizeChanged(layout);end.SizeChanged(layout);
        for(auto value:array(data->catalog,L"menus")) {
            auto spec=value.GetObject();auto item=button(data,str(spec,L"label"),[]{});style(item,data);
            MenuFlyout flyout;
            flyout.Opening([data=data,spec](Windows::Foundation::IInspectable const& sender,auto&&){
                auto menu=sender.as<MenuFlyout>();menu.Items().Clear();
                auto sections=array(spec,L"sections");
                menuItems(menu.Items(),sections.Size()?commandSections(sections,data):array(object(data->model,L"workspace_menu"),L"sections"),data);
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
        reflow();
    }
    std::vector<Windows::Graphics::RectInt32> drag(float scale,uint32_t width)const {
        std::vector<std::pair<float,float>> controls;
        for(FrameworkElement item:std::array<FrameworkElement,4>{start,end,document,zen}){
            if(item.Visibility()!=Visibility::Visible||item.ActualWidth()<=0)continue;
            auto position=item.TransformToVisual(root).TransformPoint({0,0});
            controls.emplace_back(position.X,position.X+float(item.ActualWidth()));
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
    std::function<void()> layout,std::function<void()> fullscreen):impl(std::make_shared<Impl>()){
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
