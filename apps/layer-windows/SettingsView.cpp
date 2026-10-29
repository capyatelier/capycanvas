#include "pch.h"
#include "SettingsView.h"
#include "UiControls.h"
#include "WorkflowMotion.h"
#include "Checker.h"
#include "ShortcutPage.h"
#include "WorkspaceGeometry.h"
#include <map>
#include <tuple>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Input;
using namespace CapyUi;
namespace {
J preferences(std::shared_ptr<WorkspaceData> const& data){return object(data->model,L"preferences");}
J rowFor(std::shared_ptr<WorkspaceData> const& data,hstring const& id){
    for(auto page:array(preferences(data),L"pages"))
        for(auto group:array(page.GetObject(),L"groups"))
            for(auto row:array(group.GetObject(),L"rows"))
                if(str(row.GetObject(),L"id")==id)return row.GetObject();
    return J{};
}
void send(std::shared_ptr<WorkspaceData> const& data,J const& action){
    data->dispatch(O({{L"type",S(L"preferences")},{L"action",action}}));
}
void edit(std::shared_ptr<WorkspaceData> const& data,hstring const& id,V const& value){
    send(data,O({{L"type",S(L"edit")},{L"id",S(id)},{L"value",value}}));
}
TextBlock description(std::shared_ptr<WorkspaceData> const& data,hstring const& value){
    auto text=label(data,value);text.TextWrapping(TextWrapping::Wrap);text.Opacity(.55);return text;
}
Button actionButton(std::shared_ptr<WorkspaceData> const& data,hstring const& title,J action){
    auto result=raisedButton(data,title,[data,action]{send(data,action);});
    result.MinHeight(34);
    buttonFeedback(result,data->brush(L"text"),8,20);
    return result;
}
Button headerButton(std::shared_ptr<WorkspaceData> const& data,hstring const& name,double side,std::function<void()> action){
    auto result=button(data,L"",std::move(action));result.Width(side);result.Height(side);result.CornerRadius({side/2,side/2,side/2,side/2});
    result.VerticalAlignment(VerticalAlignment::Center);AutomationProperties::SetName(result,name);tooltip(result,name);
    return result;
}
}
struct SettingsView::Impl : std::enable_shared_from_this<Impl> {
    static constexpr double HeaderHeight=47,DialogWidth=1000,DialogHeight=744;
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    ContentDialog dialog;
    Grid frame,body,overlay;
    Border sidebarSurface;
    StackPanel sidebar,pages,results;
    TextBox search;
    TextBlock title,error,empty;
    Primitives::ToggleButton searchToggle{nullptr};
    Button back{nullptr},close{nullptr};
    ScrollViewer scroller;
    Bindings bindings,commits,themeBindings;
    std::map<std::wstring,StackPanel> pageNodes;
    std::map<std::wstring,Button> tabs;
    std::map<std::wstring,FrameworkElement> rows;
    std::map<std::wstring,std::array<FrameworkElement,3>> swatchAllocations;
    bool trace=GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)!=0;
    hstring allocationTrace;
    std::unique_ptr<ShortcutPage> shortcutPage;
    Key key;
    Dispatch report;
    std::function<void()> changed;
    XamlRoot xamlRoot{nullptr};
    hstring theme,resultsKey,revealed;
    double searchFocus=-1;
    bool showing=false,closing=false,built=false,stopping=false,showFailed=false,dismissing=false;
    enum class ProfileReturn{None,Requested,Managing} profileReturn=ProfileReturn::None;
    uint64_t escapes=0,escaped=0;
    void publishAllocations(){
        if(!trace||!title.IsLoaded()||!frame.IsLoaded())return;
        A allocations;
        for(auto const& [id,parts]:swatchAllocations){
            auto visible=visibleBounds(parts[0],frame);if(visible.Width<=0||visible.Height<=0)continue;
            bool inside=false;for(auto node=VisualTreeHelper::GetParent(parts[2]);node&&node!=frame;node=VisualTreeHelper::GetParent(node))inside=inside||node==parts[1];
            auto entry=visibleBounds(parts[2],frame);
            allocations.Append(O({{L"id",S(hstring(id))},{L"native_selected",N(num(object(rowFor(data,hstring(id)),L"kind"),L"selected"))},{L"native_value",S(str(object(rowFor(data,hstring(id)),L"kind"),L"value"))},{L"row",rectangle(allocatedBounds(parts[0],frame))},
                {L"strip",rectangle(allocatedBounds(parts[1],frame))},{L"entry",rectangle(allocatedBounds(parts[2],frame))},
                {L"entry_visible",B(entry.Width>0&&entry.Height>0)},{L"entry_in_strip",B(inside)}}));
        }
        auto value=O({{L"title",rectangle(allocatedBounds(title,frame))},{L"rows",allocations}}).Stringify();
        if(value!=allocationTrace){allocationTrace=value;AutomationProperties::SetItemStatus(title,value);}
    }
    void init(){
        dialog.XamlRoot(xamlRoot);inheritLanguage(dialog,data);
        for(auto [resource,value]:{std::pair{L"ContentDialogMaxWidth",DialogWidth},std::pair{L"ContentDialogMinWidth",0.},
            std::pair{L"ContentDialogMaxHeight",DialogHeight},std::pair{L"ContentDialogMinHeight",0.}})
            dialog.Resources().Insert(box_value(resource),box_value(value));
        dialog.Resources().Insert(box_value(L"ContentDialogPadding"),box_value(Thickness{0,0,0,0}));
        AutomationProperties::SetName(dialog,data->caption(L"shortcuts",L"preferences"));
        dialog.Closing([weak=weak_from_this()](auto&&,ContentDialogClosingEventArgs const& e){
            if(auto self=weak.lock()){
                if(self->stopping){self->closing=true;return;}
                if(!std::exchange(self->dismissing,false)){
                    e.Cancel(true);
                    self->dialog.DispatcherQueue().TryEnqueue([weak]{if(auto self=weak.lock())self->escape();});
                    return;
                }
                self->closing=true;if(preferences(self->data).Size())self->data->dispatch(O({{L"type",S(L"close_settings")}}));
            }
        });
        dialog.PreviewKeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
            if(composingKey(e))return;
            if(auto self=weak.lock()){
                if(e.Key()==winrt::Windows::System::VirtualKey::Escape&&!e.KeyStatus().WasKeyDown)++self->escapes;
                bool capturing=self->capturing();if(capturing||e.Key()!=winrt::Windows::System::VirtualKey::Escape)self->key(e,true);if(capturing)e.Handled(true);
            }
        });
        dialog.PreviewKeyUp([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
            if(auto self=weak.lock()){bool capturing=self->capturing();self->key(e,false);if(capturing)e.Handled(true);}
        });
        dialog.KeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&e.Key()==winrt::Windows::System::VirtualKey::Escape){
                e.Handled(true);self->escape();
            }
        });
    }
    void hide(){dismissing=true;dialog.Hide();}
    void escape(){
        if(escaped==escapes)return;
        escaped=escapes;
        if(!shortcutPage||!shortcutPage->Dismiss())hide();
    }
    bool capturing()const{return object(preferences(data),L"capture").Size()!=0;}
    Grid header(){
        Grid bar;bar.Height(HeaderHeight);
        return bar;
    }
    void build(J const& model){
        bindings.clear();commits.clear();themeBindings.clear();pageNodes.clear();tabs.clear();rows.clear();swatchAllocations.clear();allocationTrace=L"";resultsKey=L"";
        frame=Grid();overlay=Grid();body=Grid();
        ColumnDefinition navigation;navigation.Width({210,GridUnitType::Pixel});
        ColumnDefinition main;main.Width({1,GridUnitType::Star});body.ColumnDefinitions().Append(navigation);body.ColumnDefinitions().Append(main);
        Grid side;RowDefinition sideHead;sideHead.Height({HeaderHeight,GridUnitType::Pixel});RowDefinition sideRest;sideRest.Height({1,GridUnitType::Star});
        side.RowDefinitions().Append(sideHead);side.RowDefinitions().Append(sideRest);
        auto sideTitle=label(data,data->copyCaption(L"shortcuts",L"preferences"),true);sideTitle.HorizontalAlignment(HorizontalAlignment::Stretch);sideTitle.VerticalAlignment(VerticalAlignment::Center);sideTitle.Margin({46,0,46,0});sideTitle.TextAlignment(TextAlignment::Center);sideTitle.TextTrimming(TextTrimming::CharacterEllipsis);sideTitle.MaxLines(1);AutomationProperties::SetAutomationId(sideTitle,L"preferences-heading");
        side.Children().Append(sideTitle);
        searchToggle=button<Primitives::ToggleButton>(data,L"",[data=data]{
            send(data,O({{L"type",S(L"toggle_search")},{L"open",B(!flag(preferences(data),L"searching"))}}));
        });
        buttonFeedback(searchToggle,data->brush(L"text"),20,41,nullptr,false,data->tint(L"text",26));searchToggle.Width(34);searchToggle.Height(34);searchToggle.CornerRadius(data->controlCorner());searchToggle.VerticalAlignment(VerticalAlignment::Center);
        AutomationProperties::SetAutomationId(searchToggle,L"preference-search-toggle");searchToggle.HorizontalAlignment(HorizontalAlignment::Left);searchToggle.Margin({6,0,0,0});side.Children().Append(searchToggle);
        themeBindings.emplace_back([data=data,toggle=searchToggle]{toggle.Content(icon(L"search",data->theme()));});
        sidebar=StackPanel();sidebar.Spacing(2);sidebar.Padding({6,0,6,6});
        search=TextBox();fieldColors(data,search,ControlSurface::Settings);search.PlaceholderText(data->caption(L"shortcuts",L"search_preferences"));search.Margin({0,0,0,6});
        AutomationProperties::SetName(search,data->caption(L"shortcuts",L"search_preferences"));AutomationProperties::SetAutomationId(search,L"settings-search");
        search.TextChanged([data=data](auto&& sender,auto&&){
            if(!data->updating)send(data,O({{L"type",S(L"search")},{L"query",S(sender.template as<TextBox>().Text())}}));
        });
        search.KeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){auto self=weak.lock();if(!self||composingKey(e))return;
            if(e.Key()==winrt::Windows::System::VirtualKey::Escape){self->escaped=self->escapes;send(self->data,O({{L"type",S(L"toggle_search")},{L"open",B(false)}}));e.Handled(true);}
        });sidebar.Children().Append(search);
        results=StackPanel();results.Spacing(2);sidebar.Children().Append(results);
        empty=description(data,data->caption(L"shortcuts",L"no_matching_preferences"));empty.Visibility(Visibility::Collapsed);empty.HorizontalAlignment(HorizontalAlignment::Center);empty.Margin({24,48,24,48});
        AutomationProperties::SetLiveSetting(empty,Automation::Peers::AutomationLiveSetting::Polite);sidebar.Children().Append(empty);
        ScrollViewer navigationScroll;navigationScroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);navigationScroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);navigationScroll.HorizontalScrollMode(ScrollMode::Disabled);navigationScroll.Content(sidebar);mouseScrollbars(navigationScroll);
        Grid::SetRow(navigationScroll,1);side.Children().Append(navigationScroll);
        sidebarSurface=Border();sidebarSurface.Child(side);sidebarSurface.BorderThickness({0,0,1,0});sidebarSurface.BackgroundSizing(BackgroundSizing::OuterBorderEdge);body.Children().Append(sidebarSurface);

        Grid pane;Grid::SetColumn(pane,1);
        for(auto height:{GridLength{HeaderHeight,GridUnitType::Pixel},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star}}){RowDefinition row;row.Height(height);pane.RowDefinitions().Append(row);}
        auto head=header();
        back=headerButton(data,data->common(L"back"),34,[weak=weak_from_this()]{if(auto self=weak.lock();self&&self->shortcutPage)self->shortcutPage->Back();});
        back.HorizontalAlignment(HorizontalAlignment::Left);back.Margin({6,0,0,0});back.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(back,L"shortcut-category-back");head.Children().Append(back);
        themeBindings.emplace_back([data=data,button=back]{button.Content(icon(L"go-previous",data->theme()));});
        title=label(data,L"",true);title.HorizontalAlignment(HorizontalAlignment::Center);title.VerticalAlignment(VerticalAlignment::Center);
        AutomationProperties::SetAutomationId(title,L"settings-title");head.Children().Append(title);
        close=headerButton(data,data->common(L"close"),24,[weak=weak_from_this()]{if(auto self=weak.lock())self->hide();});
        close.HorizontalAlignment(HorizontalAlignment::Right);close.VerticalAlignment(VerticalAlignment::Center);close.Margin({0,0,11,0});
        AutomationProperties::SetAutomationId(close,L"CloseButton");head.Children().Append(close);
        themeBindings.emplace_back([data=data,button=close]{button.Content(icon(L"window-close",data->theme()));button.Background(data->tint(L"text",26));});
        pane.Children().Append(head);
        error=description(data,L"");error.Opacity(1);error.Margin({12,12,12,12});Grid::SetRow(error,1);
        AutomationProperties::SetLiveSetting(error,Automation::Peers::AutomationLiveSetting::Polite);pane.Children().Append(error);
        pages=StackPanel();pages.Spacing(16);pages.Padding({12,24,12,24});
        scroller=ScrollViewer();scroller.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);mouseScrollbars(scroller);scroller.Content(pages);Grid::SetRow(scroller,2);
        pane.Children().Append(scroller);body.Children().Append(pane);
        shortcutPage=std::make_unique<ShortcutPage>(data,overlay);
        bindings.emplace_back([this]{
            auto rename=[](auto const& target,hstring const& name){if(AutomationProperties::GetName(target)!=name){AutomationProperties::SetName(target,name);tooltip(target,name);}};
            auto searchName=data->caption(L"shortcuts",L"search_preferences");
            AutomationProperties::SetName(dialog,data->caption(L"shortcuts",L"preferences"));
            search.PlaceholderText(searchName);AutomationProperties::SetName(search,searchName);rename(searchToggle,searchName);
            rename(back,data->common(L"back"));rename(close,data->common(L"close"));
            empty.Text(data->caption(L"shortcuts",L"no_matching_preferences"));
        });
        for(auto pageValue:array(model,L"pages")){
            auto page=pageValue.GetObject();auto id=str(page,L"id");
            auto tab=button(data,L"",[data=data,id]{send(data,O({{L"type",S(L"page")},{L"page",S(id)}}));});
            tab.MinHeight(42);tab.Padding({14,12,14,12});tab.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());tab.HorizontalAlignment(HorizontalAlignment::Stretch);tab.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            buttonFeedback(tab,data->brush(L"text"),8,20);
            Grid tabContent;tabContent.ColumnSpacing(12);for(auto width:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star}}){ColumnDefinition column;column.Width(width);tabContent.ColumnDefinitions().Append(column);}
            Image tabIcon;tabIcon.VerticalAlignment(VerticalAlignment::Center);tabContent.Children().Append(tabIcon);auto tabLabel=label(data,str(page,L"title"));tabLabel.TextWrapping(TextWrapping::Wrap);Grid::SetColumn(tabLabel,1);tabContent.Children().Append(tabLabel);
            bindings.emplace_back([data=data,id,tab,tabLabel]{auto page=find(array(preferences(data),L"pages"),L"id",id);auto text=str(page,L"title");tabLabel.Text(text);AutomationProperties::SetName(tab,text);});
            themeBindings.emplace_back([data=data,tabIcon,glyph=str(page,L"icon")]{setIcon(tabIcon,glyph,data->theme());});
            tab.Content(tabContent);AutomationProperties::SetName(tab,str(page,L"title"));AutomationProperties::SetAutomationId(tab,L"preference-page-"+id);
            sidebar.Children().InsertAt(sidebar.Children().Size()-2,tab);tabs.emplace(id.c_str(),tab);
            StackPanel node;node.Spacing(24);AutomationProperties::SetName(node,str(page,L"title"));
            pageNodes.emplace(id.c_str(),node);pages.Children().Append(node);
            auto container=shortcutPage->Container(id,node);
            uint32_t sectionIndex=0;
            for(auto groupValue:array(page,L"groups")){
                auto group=groupValue.GetObject();auto cardGroup=preferenceGroup(data,str(group,L"title"));auto section=cardGroup.section;auto heading=cardGroup.title;
                bindings.emplace_back([data=data,pageId=id,groupIndex=sectionIndex++,heading]{auto page=find(array(preferences(data),L"pages"),L"id",pageId);heading.Text(str(array(page,L"groups").GetObjectAt(groupIndex),L"title"));});
                auto list=cardGroup.list;container.Children().Append(section);
                A ids;
                for(auto rowValue:array(group,L"rows")){
                    auto row=rowValue.GetObject();ids.Append(S(str(row,L"id")));
                    if(list.Children().Size()){Border line;line.Height(1);line.Background(data->line(L"settings",L"card_shade"));list.Children().Append(line);}
                    list.Children().Append(field(row));
                }
                bindings.emplace_back([data=data,section,ids]{
                    bool visible=false;for(auto id:ids)visible|=flag(rowFor(data,id.GetString()),L"visible",true);
                    section.Visibility(visible?Visibility::Visible:Visibility::Collapsed);
                });
            }
            if(id==L"color"){
                auto profiles=raisedButton(data,data->copyCaption(L"color",L"manage_profiles"),[data=data,weak=weak_from_this()]{
                    if(auto self=weak.lock())self->profileReturn=ProfileReturn::Requested;
                    data->dispatch(O({{L"type",S(L"close_settings")}}));data->document(to_string(O({{L"operation",S(L"workflow_begin")},{L"id",N(0)}}).Stringify()));
                });
                AutomationProperties::SetAutomationId(profiles,L"manage-color-profiles");profiles.MinHeight(34);
                auto group=preferenceGroup(data,L"");Grid row;row.MinHeight(54);row.Padding({14,8,14,8});row.ColumnSpacing(12);
                ColumnDefinition words;words.Width({1,GridUnitType::Star});ColumnDefinition suffix;suffix.Width({1,GridUnitType::Auto});row.ColumnDefinitions().Append(words);row.ColumnDefinitions().Append(suffix);
                StackPanel text;text.Spacing(3);text.VerticalAlignment(VerticalAlignment::Center);text.Children().Append(label(data,data->copyCaption(L"color",L"profile_library_title")));
                auto detail=label(data,data->copyCaption(L"color",L"profile_menu_help"));detail.TextWrapping(TextWrapping::Wrap);detail.Opacity(.55);detail.FontSize(data->textSize()/1.2);opticalText(detail,detail.FontSize());text.Children().Append(detail);row.Children().Append(text);
                Grid::SetColumn(profiles,1);profiles.VerticalAlignment(VerticalAlignment::Center);row.Children().Append(profiles);group.list.Children().Append(row);container.Children().Append(group.section);
            }
        }
        frame.Children().Append(body);frame.Children().Append(overlay);
        dialog.Content(frame);built=true;
        if(trace)frame.LayoutUpdated([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->publishAllocations();});
    }
    FrameworkElement field(J const& row){
        auto id=str(row,L"id"),titleText=str(row,L"title");auto kind=object(row,L"kind");auto type=str(kind,L"type");
        Grid line;line.MinHeight(54);line.Padding({14,8,14,8});line.ColumnSpacing(12);
        ColumnDefinition textColumn;textColumn.Width({1,GridUnitType::Star});
        ColumnDefinition controlColumn;controlColumn.Width({1,GridUnitType::Auto});
        line.ColumnDefinitions().Append(textColumn);line.ColumnDefinitions().Append(controlColumn);
        StackPanel text;text.Spacing(3);text.VerticalAlignment(VerticalAlignment::Center);
        auto heading=label(data,titleText);heading.TextWrapping(TextWrapping::Wrap);text.Children().Append(heading);
        bindings.emplace_back([data=data,id,heading]{heading.Text(str(rowFor(data,id),L"title"));});
        if(!str(row,L"description").empty()){
            auto detail=description(data,str(row,L"description"));detail.FontSize(data->textSize()/1.2);opticalText(detail,detail.FontSize());text.Children().Append(detail);
            bindings.emplace_back([data=data,id,detail]{detail.Text(str(rowFor(data,id),L"description"));});
        }
        line.Children().Append(text);
        FrameworkElement widget{nullptr};
        if(type==L"number"){
            text.Children().Clear();
            NumberPresentation presentation{true,str(row,L"description")};presentation.title=[source=std::weak_ptr<WorkspaceData>(data),id]{if(auto data=source.lock())return str(rowFor(data,id),L"title");return hstring();};
            presentation.descriptionText=[source=std::weak_ptr<WorkspaceData>(data),id]{if(auto data=source.lock())return str(rowFor(data,id),L"description");return hstring();};
            auto numericField=number(data,titleText,object(kind,L"control"),
                [data=data,id]{return num(object(rowFor(data,id),L"kind"),L"value");},
                [data=data,id](double value){edit(data,id,N(value));},bindings,&commits,false,L"setting-number-"+id,false,
                presentation);
            text.Children().InsertAt(0,numericField);Grid::SetColumnSpan(text,2);
        }else if(type==L"action"){
            auto control=raisedButton(data,str(kind,L"label"),[data=data,id]{if(!data->updating)data->dispatch(object(object(rowFor(data,id),L"kind"),L"action"));});
            control.MinHeight(34);AutomationProperties::SetAutomationId(control,L"setting-action-"+id);widget=control;
            bindings.emplace_back([data=data,id,control]{buttonCaption(control,str(object(rowFor(data,id),L"kind"),L"label"));});
        }else if(type==L"switch"){
            ToggleSwitch control;control.MinWidth(0);control.OnContent(box_value(L""));control.OffContent(box_value(L""));
            control.Toggled([data=data,id](auto&& sender,auto&&){
                if(!data->updating)edit(data,id,B(sender.template as<ToggleSwitch>().IsOn()));
            });widget=control;
            bindings.emplace_back([data=data,id,control]{control.IsOn(flag(object(rowFor(data,id),L"kind"),L"active"));});
        }else if(type==L"choice"&&str(object(kind,L"presentation"),L"type")==L"image_tiles"){
            Grid choices;choices.RowSpacing(6);choices.ColumnSpacing(6);choices.HorizontalAlignment(HorizontalAlignment::Center);
            auto options=array(kind,L"options"),icons=array(kind,L"icons");
            auto columns=std::max(1u,uint32_t(num(object(kind,L"presentation"),L"columns",4)));
            for(uint32_t i=0;i<columns;++i){ColumnDefinition column;column.Width({64,GridUnitType::Pixel});choices.ColumnDefinitions().Append(column);}
            for(uint32_t i=0;i<(options.Size()+columns-1)/columns;++i){RowDefinition track;track.Height({64,GridUnitType::Pixel});choices.RowDefinitions().Append(track);}
            for(uint32_t i=0;i<options.Size();++i){
                Primitives::ToggleButton choice;choice.Width(64);choice.Height(64);choice.Padding({8,8,8,8});choice.CornerRadius({32*CornerFit,32*CornerFit,32*CornerFit,32*CornerFit});
                choice.BorderThickness({2,2,2,2});AutomationProperties::SetName(choice,options.GetStringAt(i));
                buttonColors(data,choice);
                tooltip(choice,options.GetStringAt(i));
                if(i<icons.Size())themeBindings.emplace_back([data=data,choice,id=icons.GetStringAt(i)]{
                    choice.Content(icon(id,data->theme(),48));
                });
                choice.Click([data=data,id,i](auto&&,auto&&){if(!data->updating)edit(data,id,N(i));});
                Grid::SetColumn(choice,i%columns);Grid::SetRow(choice,i/columns);choices.Children().Append(choice);
                bindings.emplace_back([data=data,id,i,choice]{
                    bool active=num(object(rowFor(data,id),L"kind"),L"selected")==i;
                    choice.IsChecked(active);choice.Background(active?selected(data):data->tint(L"text",10));choice.BorderBrush(active?accent(data):clear());
                });
            }
            text.Spacing(12);text.Children().Append(choices);Grid::SetColumnSpan(text,2);
        }else if(type==L"choice"&&str(object(kind,L"presentation"),L"type")==L"circles"){
            StackPanel strip;strip.Orientation(Orientation::Horizontal);strip.Spacing(8);
            auto options=array(kind,L"options");auto alphas=array(object(kind,L"presentation"),L"alphas");
            for(uint32_t i=0;i<options.Size();++i){
                Button choice;choice.Width(28);choice.Height(28);choice.MinWidth(0);choice.MinHeight(0);choice.Padding({0,0,0,0});
                choice.CornerRadius({14,14,14,14});choice.BorderThickness({0,0,0,0});choice.Background(clear());
                buttonColors(data,choice);
                AutomationProperties::SetName(choice,options.GetStringAt(i));tooltip(choice,options.GetStringAt(i));
                AutomationProperties::SetAutomationId(choice,L"preference-"+id+L"-"+to_hstring(i));
                choice.Click([data=data,id,i](auto&&,auto&&){if(!data->updating)edit(data,id,N(i));});
                auto paint=[data=data,id,i,choice,alphas]{
                    bool dark=data->theme()==L"dark";auto pair=i<alphas.Size()?alphas.GetArrayAt(i):A{};
                    double alpha=pair.Size()==2?pair.GetNumberAt(dark?1:0):1;
                    bool checked=num(object(rowFor(data,id),L"kind"),L"selected")==i;
                    auto grey=[](double value,double opacity=1){auto c=uint8_t(std::lround(value*255));return winrt::Windows::UI::Color{uint8_t(std::lround(opacity*255)),c,c,c};};
                    Grid disc;disc.Width(28);disc.Height(28);disc.IsHitTestVisible(false);
                    auto circle=[&](Brush const& brush){Shapes::Ellipse shape;shape.Fill(brush);disc.Children().Append(shape);};
                    if(alpha<1){auto palette=object(data->state,L"palette");Media::ImageBrush checks;checks.ImageSource(checkerBitmap(28,2,color(str(palette,L"checker_light")),color(str(palette,L"checker_dark")),7));checks.Stretch(Stretch::Fill);circle(checks);}
                    circle(fill(grey(dark?.55:.8,alpha)));
                    if(alpha<1){
                        Media::RadialGradientBrush sheen;sheen.Center({.32f,.26f});sheen.GradientOrigin({.32f,.26f});sheen.RadiusX(.5);sheen.RadiusY(.5);
                        GradientStop from;from.Color(grey(1,std::min(1.,.25+1.2*(1-alpha))));GradientStop to;to.Color(grey(1,0));to.Offset(1);
                        sheen.GradientStops().Append(from);sheen.GradientStops().Append(to);circle(sheen);
                    }
                    Shapes::Ellipse ring;ring.Stroke(fill(grey(.5,.45)));ring.StrokeThickness(1);disc.Children().Append(ring);
                    if(checked)for(auto [width,value,opacity]:{std::tuple{4.,dark?0.:1.,.45},std::tuple{2.,dark?1.:.18,1.}}){
                        Shapes::Polyline mark;for(auto [x,y]:{std::pair{.3,.52},std::pair{.44,.66},std::pair{.71,.36}})mark.Points().Append({float(x*28),float(y*28)});
                        mark.Stroke(fill(grey(value,opacity)));mark.StrokeThickness(width);mark.StrokeStartLineCap(PenLineCap::Round);mark.StrokeEndLineCap(PenLineCap::Round);
                        mark.StrokeLineJoin(PenLineJoin::Round);disc.Children().Append(mark);
                    }
                    Shapes::Ellipse feedback;feedback.Fill(buttonFeedbackBrush(choice,data->brush(L"text")));disc.Children().Append(feedback);
                    choice.Content(disc);AutomationProperties::SetItemStatus(choice,checked?data->caption(L"search",L"selected"):L"");
                };
                bindings.emplace_back(paint);themeBindings.emplace_back(paint);strip.Children().Append(choice);
            }
            widget=strip;
        }else if(type==L"choice"){
            ComboBox control;choiceColors(data,control,ControlSurface::Card);control.Background(clear());control.MinHeight(38);AutomationProperties::SetAutomationId(control,L"preference-choice-"+id);control.MinWidth(132);control.MaxWidth(220);
            auto options=array(kind,L"options"),icons=array(kind,L"icons");
            for(uint32_t i=0;i<options.Size();++i){
                StackPanel option;option.Orientation(Orientation::Horizontal);option.Spacing(12);
                if(i<icons.Size()&&!icons.GetStringAt(i).empty()){
                    Image glyph;glyph.VerticalAlignment(VerticalAlignment::Center);option.Children().Append(glyph);
                    themeBindings.emplace_back([data=data,glyph,id=icons.GetStringAt(i)]{setIcon(glyph,id,data->theme(),24);});
                }
                auto optionLabel=label(data,options.GetStringAt(i));optionLabel.VerticalAlignment(VerticalAlignment::Center);option.Children().Append(optionLabel);
                bindings.emplace_back([data=data,id,i,optionLabel]{auto options=array(object(rowFor(data,id),L"kind"),L"options");if(i<options.Size())optionLabel.Text(options.GetStringAt(i));});
                ComboBoxItem choice;choice.Content(option);AutomationProperties::SetName(choice,options.GetStringAt(i));
                control.Items().Append(choice);
                bindings.emplace_back([data=data,id,i,choice]{auto options=array(object(rowFor(data,id),L"kind"),L"options");if(i<options.Size())AutomationProperties::SetName(choice,options.GetStringAt(i));});
            }
            control.SelectionChanged([data=data,id](auto&& sender,auto&&){
                auto index=sender.template as<ComboBox>().SelectedIndex();
                if(!data->updating&&index>=0)edit(data,id,N(index));
            });widget=control;
            bindings.emplace_back([data=data,id,control]{control.SelectedIndex(int32_t(num(object(rowFor(data,id),L"kind"),L"selected")));});
        }else if(type==L"swatches"){
            bool inlineRow=flag(kind,L"inline");double side=inlineRow?28:32;
            Panel circles{nullptr};
            if(inlineRow){StackPanel strip;strip.Orientation(Orientation::Horizontal);strip.Spacing(2);circles=strip;}
            else{VariableSizedWrapGrid grid;grid.Orientation(Orientation::Horizontal);grid.ItemWidth(side+10);grid.ItemHeight(side+10);grid.HorizontalAlignment(HorizontalAlignment::Center);circles=grid;}
            TextBox entry;fieldColors(data,entry,ControlSurface::Card);entry.Width(96);entry.MaxLength(7);entry.PlaceholderText(str(kind,L"placeholder"));entry.VerticalAlignment(VerticalAlignment::Center);
            AutomationProperties::SetAutomationId(entry,L"setting-text-"+id);AutomationProperties::SetName(entry,titleText);
            Border entryPage;entryPage.Padding(inlineRow?Thickness{0,0,8,0}:Thickness{0,12,0,0});entryPage.Child(entry);
            auto entryPages=std::make_shared<WorkflowPages>();Border emptyEntry;entryPages->append(emptyEntry);entryPages->append(entryPage);
            struct Draft{hstring text,key,saved;bool changed=false,editing=false;std::vector<Button> buttons;std::vector<Border> rings;std::vector<hstring> colors;};
            auto draft=std::make_shared<Draft>();
            entry.TextChanging([data=data,draft](auto&& sender,auto&&){
                if(!data->updating){draft->text=sender.template as<TextBox>().Text();draft->changed=true;}
            });
            auto commit=[data=data,id,draft]{if(draft->changed){draft->changed=false;edit(data,id,S(draft->text));}};
            commits.push_back(commit);
            entry.LostFocus([commit](auto&&,auto&&){commit();});
            entry.KeyDown([commit](auto&&,KeyRoutedEventArgs const& e){if(composingKey(e))return;if(e.Key()==winrt::Windows::System::VirtualKey::Enter){commit();e.Handled(true);}});
            auto refresh=std::make_shared<std::function<void()>>();
            *refresh=[data=data,id,side,inlineRow,circles,entry,entryPages,draft,repaint=std::weak_ptr<std::function<void()>>(refresh)]{
                auto current=object(rowFor(data,id),L"kind");auto swatches=array(current,L"swatches");auto chosen=uint32_t(num(current,L"selected"));
                if(auto saved=str(current,L"value");saved!=draft->saved){draft->saved=saved;draft->editing=false;}
                uint32_t customIndex=swatches.Size();
                for(uint32_t i=0;i<swatches.Size();++i)if(flag(swatches.GetObjectAt(i),L"custom"))customIndex=i;
                if(draft->editing&&customIndex<swatches.Size())chosen=customIndex;
                hstring key=to_hstring(swatches.Size());
                for(auto value:swatches){auto swatch=value.GetObject();for(auto field:{L"value",L"icon",L"custom"})key=key+swatch.GetNamedValue(field,JsonValue::CreateNullValue()).Stringify()+L"|";}
                bool custom=chosen<swatches.Size()&&chosen==customIndex;
                entryPages->show(custom?1:0,true,!inlineRow);
                if(!draft->changed&&entry.FocusState()==FocusState::Unfocused)entry.Text(str(current,L"custom"));
                if(key!=draft->key){
                    draft->key=key;circles.Children().Clear();draft->buttons.clear();draft->rings.clear();draft->colors.clear();
                    for(uint32_t i=0;i<swatches.Size();++i){
                        auto swatch=swatches.GetObjectAt(i);
                        Button circle;circle.Width(side);circle.Height(side);circle.Padding({0,0,0,0});circle.MinWidth(0);circle.MinHeight(0);circle.CornerRadius({side/2,side/2,side/2,side/2});buttonColors(data,circle);
                        if(auto glyph=str(swatch,L"icon");!glyph.empty())circle.Content(icon(glyph,data->theme(),side*.5));
                        AutomationProperties::SetAutomationId(circle,L"setting-"+id+L"-swatch-"+to_hstring(i));draft->buttons.push_back(circle);draft->colors.emplace_back();
                        circle.Click([data,id,swatch,editor=make_weak(entry),draftState=std::weak_ptr<Draft>(draft),repaint](auto&&,auto&&){
                            auto draft=draftState.lock();if(!draft||data->updating)return;
                            draft->editing=flag(swatch,L"custom");
                            if(!draft->editing)edit(data,id,S(str(swatch,L"value")));
                            if(auto refresh=repaint.lock())(*refresh)();
                            if(draft->editing)if(auto entry=editor.get()){entry.Focus(FocusState::Programmatic);entry.SelectAll();}
                        });
                        Border ring;ring.Padding({2,2,2,2});ring.CornerRadius({side/2+4,side/2+4,side/2+4,side/2+4});ring.BorderThickness({2,2,2,2});ring.Child(circle);circles.Children().Append(ring);draft->rings.push_back(ring);
                    }
                }
                for(uint32_t i=0;i<swatches.Size();++i){
                    auto swatch=swatches.GetObjectAt(i);auto title=str(swatch,L"label");auto button=draft->buttons[i];
                    tooltip(button,title);AutomationProperties::SetName(button,title);AutomationProperties::SetItemStatus(button,i==chosen?data->caption(L"search",L"selected"):L"");draft->rings[i].BorderBrush(i==chosen?data->brush(L"text"):clear());
                    auto paint=swatch.GetNamedValue(L"color",JsonValue::CreateNullValue());auto colorKey=paint.Stringify();
                    if(draft->colors[i]!=colorKey){draft->colors[i]=colorKey;button.Background(paint.ValueType()==JsonValueType::String?fill(color(paint.GetString())):data->tint(L"text",26));button.BorderThickness(paint.ValueType()==JsonValueType::String?Thickness{}:Thickness{1,1,1,1});button.BorderBrush(data->tint(L"text",38));}
                    if(auto glyph=button.Content().try_as<Image>()){setIcon(glyph,str(swatch,L"icon"),data->theme(),side*.5);glyph.Visibility(i==chosen?Visibility::Collapsed:Visibility::Visible);}
                }
            };
            bindings.emplace_back([refresh]{(*refresh)();});(*refresh)();
            StackPanel host;if(inlineRow){host.Orientation(Orientation::Horizontal);host.Children().Append(entryPages->root);host.Children().Append(circles);}else{host.Children().Append(circles);host.Children().Append(entryPages->root);}
            if(trace)swatchAllocations.insert_or_assign(id.c_str(),std::array<FrameworkElement,3>{line,circles,entry});
            if(inlineRow){widget=host;}
            else{host.HorizontalAlignment(HorizontalAlignment::Center);text.Spacing(12);text.Children().Append(host);Grid::SetColumnSpan(text,2);}
        }else if(type==L"link"){
            HyperlinkButton control;control.Content(box_value(str(kind,L"label")));control.NavigateUri(winrt::Windows::Foundation::Uri(str(kind,L"url")));
            control.Foreground(accent(data));control.FontSize(data->textSize());control.Padding({0,0,0,0});widget=control;
        }else {
            auto control=label(data,str(kind,L"value"));control.TextWrapping(TextWrapping::Wrap);control.MaxWidth(240);control.IsTextSelectionEnabled(true);widget=control;
        }
        if(widget){
            bindings.emplace_back([data=data,id,widget]{AutomationProperties::SetName(widget,str(rowFor(data,id),L"title"));widget.Language(data->language());});
            widget.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(widget,1);
            AutomationProperties::SetName(widget,titleText);line.Children().Append(widget);
        }
        MenuFlyout reset;
        reset.Opening([data=data,id](winrt::Windows::Foundation::IInspectable const& sender,auto&&){
            auto menu=sender.as<MenuFlyout>();menu.Items().Clear();auto spec=object(rowFor(data,id),L"reset");
            if(!spec.Size())return;
            MenuFlyoutItem item;item.Text(str(spec,L"label"));item.KeyboardAcceleratorTextOverride(str(spec,L"hint"));item.IsEnabled(flag(spec,L"enabled"));
            item.Click([data,id](auto&&,auto&&){send(data,O({{L"type",S(L"reset")},{L"id",S(id)}}));});menu.Items().Append(item);
        });
        ContentControl field;field.IsTabStop(false);field.HorizontalContentAlignment(HorizontalAlignment::Stretch);field.Content(line);
        field.ContextFlyout(reset);rows.emplace(id.c_str(),field);
        bindings.emplace_back([data=data,id,field]{
            auto row=rowFor(data,id);field.Visibility(flag(row,L"visible",true)?Visibility::Visible:Visibility::Collapsed);
            field.IsEnabled(flag(row,L"enabled",true));field.Opacity(flag(row,L"enabled",true)?1:.5);
        });
        return field;
    }
    void updateResults(J const& model){
        auto resultsModel=array(model,L"search_results");auto resultKey=resultsModel.Stringify()+data->theme();
        if(resultKey==resultsKey)return;
        resultsKey=resultKey;results.Children().Clear();
        for(auto value:resultsModel){
            auto spec=value.GetObject();auto item=actionButton(data,str(spec,L"title"),object(spec,L"action"));
            item.HorizontalAlignment(HorizontalAlignment::Stretch);item.Padding({10,10,10,10});item.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());
            StackPanel text;text.Spacing(3);text.Children().Append(label(data,str(spec,L"title")));
            if(auto detail=str(spec,L"description");!detail.empty()){auto line=description(data,detail);line.FontSize(data->textSize()/1.2);opticalText(line,line.FontSize());text.Children().Append(line);}
            item.Content(text);item.HorizontalContentAlignment(HorizontalAlignment::Left);results.Children().Append(item);
        }
    }
    fire_and_forget show(){
        auto lifetime=shared_from_this();showing=true;closing=false;changed();
        try{co_await dialog.ShowAsync();}
        catch(hresult_error const& failure){
            showing=false;closing=false;showFailed=true;
            if(!stopping){
                data->dispatch(O({{L"type",S(L"close_settings")}}));
                report(to_string(failure.message()));
            }
            changed();co_return;
        }
        // The popup disappears before ShowAsync completes. A new shared open
        // request may arrive during that closing animation; reconcile it only
        // after the previous operation releases the window's dialog slot.
        showing=false;closing=false;
        changed();
    }
    void apply(J const& snapshot){
        data->adoptLocalization(snapshot);
        if(!snapshot.HasKey(L"state"))return;
        data->state=object(snapshot,L"state");data->refreshPalette();data->model=snapshot;auto model=preferences(data);
        auto document=str(object(snapshot,L"windows_document"),L"type");bool managing=document==L"workflow"||document==L"workflow_busy";
        if(profileReturn==ProfileReturn::Requested&&managing)profileReturn=ProfileReturn::Managing;
        else if(profileReturn==ProfileReturn::Managing&&!managing){profileReturn=ProfileReturn::None;data->dispatch(O({{L"type",S(L"open_settings")},{L"page",S(L"color")}}));}
        if(!model.Size()){if(shortcutPage)shortcutPage->Apply(snapshot);showFailed=false;if(showing&&!closing){dismissing=true;dialog.Hide();}return;}
        if(showFailed)return;
        data->updating=true;struct Reset{bool& value;~Reset(){value=false;}}reset{data->updating};
        // Retain native controls and their automation peers across theme changes.
        bool restyle=!built||theme!=data->theme();theme=data->theme();
        if(!built)build(model);
        dialogSurface(data,dialog);
        sidebarSurface.Background(data->brush(L"sidebar"));sidebarSurface.BorderBrush(data->line(L"sidebar_border"));
        auto size=xamlRoot.Size();frame.Width(std::max(320.,std::min(DialogWidth,double(size.Width)-60)));
        auto insets=array(snapshot,L"titlebar_insets");
        auto captionHeight=insets.Size()==3?insets.GetAt(2).GetNumber():0.;
        frame.Height(std::max(240.,std::min(DialogHeight,double(size.Height)-40-2*captionHeight)));
        shortcutPage->Apply(snapshot);
        auto page=str(model,L"page"),query=str(model,L"query");
        bool opening=search.Visibility()==Visibility::Collapsed&&(flag(model,L"searching")||!query.empty());
        searchToggle.IsChecked(flag(model,L"searching"));searchToggle.Background(flag(model,L"searching")?data->tint(L"text",26):clear());
        search.Visibility(flag(model,L"searching")||!query.empty()?Visibility::Visible:Visibility::Collapsed);
        if(search.Text()!=query)search.Text(query);
        auto focus=num(model,L"search_focus");
        if(opening||(searchFocus>=0&&focus!=searchFocus)){search.Focus(FocusState::Programmatic);search.Select(int32_t(search.Text().size()),0);}
        searchFocus=focus;
        empty.Visibility(flag(model,L"empty")?Visibility::Visible:Visibility::Collapsed);
        for(auto const& [id,node]:pageNodes)node.Visibility(id==page?Visibility::Visible:Visibility::Collapsed);
        for(auto const& [id,item]:tabs){
            item.Background(id==page?data->tint(L"text",26):clear());item.Visibility(query.empty()?Visibility::Visible:Visibility::Collapsed);
            AutomationProperties::SetItemStatus(item,id==page?data->caption(L"search",L"selected"):hstring());
        }
        auto subpage=shortcutPage->Title();
        title.Text(subpage.empty()?str(find(array(model,L"pages"),L"id",page),L"title"):subpage);
        back.Visibility(subpage.empty()?Visibility::Collapsed:Visibility::Visible);
        auto message=str(model,L"error");
        if(message.empty())message=str(snapshot,L"error");
        if(message.empty())message=str(data->state,L"host_error");
        error.Text(message);error.Foreground(data->brush(L"error"));
        error.Visibility(error.Text().empty()?Visibility::Collapsed:Visibility::Visible);
        for(auto const& bind:bindings)bind();
        updateResults(model);
        auto reveal=str(model,L"reveal");
        if(reveal!=revealed){revealed=reveal;auto it=rows.find(reveal.c_str());if(it!=rows.end()){
            it->second.StartBringIntoView();
            if(auto target=FocusManager::FindFirstFocusableElement(it->second).try_as<UIElement>())target.Focus(FocusState::Programmatic);
        }}
        if(restyle)for(auto const& bind:themeBindings)bind();
        if(!showing)show();
    }
};
SettingsView::SettingsView(Dispatch dispatch,Json catalog,NativeLocalization localization,XamlRoot root,Key key,Dispatch report,std::function<void()> changed,Dispatch document):impl(std::make_shared<Impl>()){
    impl->data->document=std::move(document);impl->data->send=std::move(dispatch);impl->data->localization=localization;impl->data->catalog=catalog;impl->xamlRoot=root;impl->key=std::move(key);impl->report=std::move(report);impl->changed=std::move(changed);impl->init();
}
SettingsView::~SettingsView()=default;
void SettingsView::Apply(Json const& snapshot){impl->apply(snapshot);}
bool SettingsView::IsOpen()const{return impl->showing;}
void SettingsView::CommitEdits(){for(auto const& commit:impl->commits)commit();}
void SettingsView::Hide(){impl->stopping=true;if(impl->showing)impl->dialog.Hide();}
void SettingsView::SetWindowId(uint64_t id){impl->data->windowId=id;}
