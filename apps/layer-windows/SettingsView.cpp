#include "pch.h"
#include "SettingsView.h"
#include "UiControls.h"
#include <map>
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
    auto text=label(data,value);text.TextWrapping(TextWrapping::Wrap);text.Opacity(.75);return text;
}
Button actionButton(std::shared_ptr<WorkspaceData> const& data,hstring const& title,J action){
    auto result=button(data,title,[data,action]{send(data,action);});
    result.Padding({10,5,10,5});result.MinHeight(34);return result;
}
}
struct SettingsView::Impl : std::enable_shared_from_this<Impl> {
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    ContentDialog dialog;
    Grid body;
    StackPanel sidebar,content,pages,results,shortcuts,editor;
    TextBox search,shortcutSearch;
    TextBlock title,error;
    ScrollViewer scroller;
    Bindings bindings,commits;
    std::map<std::wstring,StackPanel> pageNodes;
    std::map<std::wstring,Button> tabs;
    std::map<std::wstring,FrameworkElement> rows;
    Key key;
    Dispatch report;
    std::function<void()> changed;
    XamlRoot xamlRoot{nullptr};
    hstring theme,editorKey,shortcutKey,resultsKey,revealed;
    bool showing=false,closing=false,built=false,stopping=false,showFailed=false;
    void init(){
        dialog.XamlRoot(xamlRoot);dialog.Title(box_value(L"Preferences"));dialog.CloseButtonText(L"Close");
        dialog.Resources().Insert(box_value(L"ContentDialogMaxWidth"),box_value(920.));
        dialog.Resources().Insert(box_value(L"ContentDialogMinWidth"),box_value(0.));
        dialog.Closing([weak=weak_from_this()](auto&&,ContentDialogClosingEventArgs const& e){
            if(auto self=weak.lock()){
                if(self->stopping){self->closing=true;return;}
                auto model=preferences(self->data);
                if(object(model,L"capture").Size()){e.Cancel(true);send(self->data,O({{L"type",S(L"cancel_shortcut")}}));}
                else if(object(model,L"shortcut_editor").Size()){e.Cancel(true);send(self->data,O({{L"type",S(L"close_shortcut_editor")}}));}
                else {self->closing=true;if(model.Size())self->data->dispatch(O({{L"type",S(L"close_settings")}}));}
            }
        });
        dialog.PreviewKeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
            if(auto self=weak.lock())self->key(e,true);
        });
        dialog.PreviewKeyUp([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
            if(auto self=weak.lock())self->key(e,false);
        });
    }
    void build(J const& model){
        bindings.clear();commits.clear();pageNodes.clear();tabs.clear();rows.clear();resultsKey=L"";shortcutKey=L"";editorKey=L"";
        body=Grid();ColumnDefinition navigation;navigation.Width({192,GridUnitType::Pixel});
        ColumnDefinition main;main.Width({1,GridUnitType::Star});body.ColumnDefinitions().Append(navigation);body.ColumnDefinitions().Append(main);
        sidebar=StackPanel();sidebar.Spacing(6);sidebar.Margin({0,0,16,0});
        auto toggle=actionButton(data,L"Search preferences",O({{L"type",S(L"toggle_search")},{L"open",B(true)}}));
        sidebar.Children().Append(toggle);
        search=TextBox();search.PlaceholderText(L"Search preferences");search.Margin({0,0,0,6});
        AutomationProperties::SetName(search,L"Search preferences");
        search.TextChanged([data=data](auto&& sender,auto&&){
            if(!data->updating)send(data,O({{L"type",S(L"search")},{L"query",S(sender.template as<TextBox>().Text())}}));
        });sidebar.Children().Append(search);
        results=StackPanel();results.Spacing(4);sidebar.Children().Append(results);
        content=StackPanel();content.Spacing(12);Grid::SetColumn(content,1);
        title=label(data,L"",true);content.Children().Append(title);
        error=description(data,L"");AutomationProperties::SetLiveSetting(error,Automation::Peers::AutomationLiveSetting::Polite);content.Children().Append(error);
        pages=StackPanel();pages.Spacing(16);editor=StackPanel();editor.Spacing(12);
        scroller=ScrollViewer();scroller.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);
        StackPanel sections;sections.Children().Append(pages);sections.Children().Append(editor);scroller.Content(sections);
        content.Children().Append(scroller);
        for(auto pageValue:array(model,L"pages")){
            auto page=pageValue.GetObject();auto id=str(page,L"id");
            auto tab=actionButton(data,str(page,L"title"),O({{L"type",S(L"page")},{L"page",S(id)}}));
            tab.HorizontalContentAlignment(HorizontalAlignment::Left);sidebar.Children().Append(tab);tabs.emplace(id.c_str(),tab);
            StackPanel node;node.Spacing(16);AutomationProperties::SetName(node,str(page,L"title"));
            pageNodes.emplace(id.c_str(),node);pages.Children().Append(node);
            for(auto groupValue:array(page,L"groups")){
                auto group=groupValue.GetObject();StackPanel section;section.Spacing(8);
                section.Children().Append(label(data,str(group,L"title"),true));
                Border card;card.Background(data->brush(L"card"));card.CornerRadius({8,8,8,8});
                StackPanel list;card.Child(list);section.Children().Append(card);node.Children().Append(section);
                A ids;
                for(auto rowValue:array(group,L"rows")){
                    auto row=rowValue.GetObject();ids.Append(S(str(row,L"id")));
                    list.Children().Append(field(row));
                }
                bindings.emplace_back([data=data,section,ids]{
                    bool visible=false;for(auto id:ids)visible|=flag(rowFor(data,id.GetString()),L"visible",true);
                    section.Visibility(visible?Visibility::Visible:Visibility::Collapsed);
                });
            }
            if(id==L"shortcuts"){
                shortcutSearch=TextBox();shortcutSearch.PlaceholderText(L"Search shortcuts");AutomationProperties::SetName(shortcutSearch,L"Search shortcuts");
                shortcutSearch.TextChanged([data=data](auto&& sender,auto&&){
                    if(!data->updating)send(data,O({{L"type",S(L"search_shortcuts")},{L"query",S(sender.template as<TextBox>().Text())}}));
                });node.Children().Append(shortcutSearch);
                node.Children().Append(actionButton(data,L"Reset All",O({{L"type",S(L"reset_all_shortcuts")}})));
                shortcuts=StackPanel();shortcuts.Spacing(4);node.Children().Append(shortcuts);
            }
        }
        ScrollViewer navigationScroll;navigationScroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);navigationScroll.Content(sidebar);
        body.Children().Append(navigationScroll);body.Children().Append(content);dialog.Content(body);built=true;
    }
    FrameworkElement field(J const& row){
        auto id=str(row,L"id"),titleText=str(row,L"title");auto kind=object(row,L"kind");auto type=str(kind,L"type");
        Grid line;line.Margin({12,10,12,10});line.ColumnSpacing(12);
        ColumnDefinition textColumn;textColumn.Width({1,GridUnitType::Star});
        ColumnDefinition controlColumn;controlColumn.Width({1,GridUnitType::Auto});
        line.ColumnDefinitions().Append(textColumn);line.ColumnDefinitions().Append(controlColumn);
        StackPanel text;text.Spacing(4);auto heading=label(data,titleText);heading.TextWrapping(TextWrapping::Wrap);text.Children().Append(heading);
        if(!str(row,L"description").empty())text.Children().Append(description(data,str(row,L"description")));
        line.Children().Append(text);
        FrameworkElement widget{nullptr};
        if(type==L"number"){
            text.Children().Clear();
            auto numericField=number(data,titleText,object(kind,L"control"),
                [data=data,id]{return num(object(rowFor(data,id),L"kind"),L"value");},
                [data=data,id](double value){edit(data,id,N(value));},bindings,&commits,false,L"",false,
                NumberPresentation{true,str(row,L"description")});
            text.Children().InsertAt(0,numericField);Grid::SetColumnSpan(text,2);
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
                Primitives::ToggleButton choice;choice.Width(64);choice.Height(64);choice.Padding({8});choice.CornerRadius({6,6,6,6});
                choice.BorderThickness({0});AutomationProperties::SetName(choice,options.GetStringAt(i));
                for(auto role:{L"ToggleButtonBackgroundChecked",L"ToggleButtonBackgroundCheckedPointerOver",L"ToggleButtonBackgroundCheckedPressed"})
                    choice.Resources().Insert(box_value(role),selected());
                ToolTipService::SetToolTip(choice,box_value(options.GetStringAt(i)));
                if(i<icons.Size())choice.Content(icon(icons.GetStringAt(i),data->theme(),48));
                choice.Click([data=data,id,i](auto&&,auto&&){if(!data->updating)edit(data,id,N(i));});
                Grid::SetColumn(choice,i%columns);Grid::SetRow(choice,i/columns);choices.Children().Append(choice);
                bindings.emplace_back([data=data,id,i,choice]{
                    bool active=num(object(rowFor(data,id),L"kind"),L"selected")==i;
                    choice.IsChecked(active);choice.Background(active?selected():clear());
                });
            }
            text.Spacing(10);text.Children().Append(choices);Grid::SetColumnSpan(text,2);
        }else if(type==L"choice"){
            ComboBox control;control.MinWidth(132);control.MaxWidth(220);
            auto options=array(kind,L"options"),icons=array(kind,L"icons");
            for(uint32_t i=0;i<options.Size();++i){
                StackPanel option;option.Orientation(Orientation::Horizontal);option.Spacing(8);
                if(i<icons.Size()&&!icons.GetStringAt(i).empty())option.Children().Append(icon(icons.GetStringAt(i),data->theme()));
                option.Children().Append(label(data,options.GetStringAt(i)));control.Items().Append(option);
            }
            control.SelectionChanged([data=data,id](auto&& sender,auto&&){
                auto index=sender.template as<ComboBox>().SelectedIndex();
                if(!data->updating&&index>=0)edit(data,id,N(index));
            });widget=control;
            bindings.emplace_back([data=data,id,control]{control.SelectedIndex(int32_t(num(object(rowFor(data,id),L"kind"),L"selected")));});
        }else if(type==L"text"){
            TextBox control;control.Width(140);control.MaxLength(int32_t(num(kind,L"max_length",64)));
            control.PlaceholderText(str(kind,L"placeholder"));
            struct Draft{hstring text;bool changed=false;};
            auto draft=std::make_shared<Draft>();
            control.TextChanging([data=data,draft](auto&& sender,auto&&){
                if(!data->updating){draft->text=sender.template as<TextBox>().Text();draft->changed=true;}
            });
            auto commit=[data=data,id,draft,weak=make_weak(control)]{
                if(weak.get()&&draft->changed){draft->changed=false;edit(data,id,S(draft->text));}
            };
            commits.push_back(commit);
            control.LostFocus([commit](auto&&,auto&&){commit();});
            control.KeyDown([commit](auto&&,KeyRoutedEventArgs const& e){if(e.Key()==Windows::System::VirtualKey::Enter){commit();e.Handled(true);}});
            widget=control;
            bindings.emplace_back([data=data,id,control,draft]{
                if(!draft->changed&&control.FocusState()==FocusState::Unfocused)control.Text(str(object(rowFor(data,id),L"kind"),L"value"));
            });
        }else if(type==L"link"){
            HyperlinkButton control;control.Content(box_value(str(kind,L"label")));control.NavigateUri(Windows::Foundation::Uri(str(kind,L"url")));
            widget=control;
        }else {
            auto control=label(data,str(kind,L"value"));control.TextWrapping(TextWrapping::Wrap);control.MaxWidth(240);widget=control;
        }
        if(widget){
            widget.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(widget,1);
            AutomationProperties::SetName(widget,titleText);line.Children().Append(widget);
        }
        MenuFlyout reset;
        reset.Opening([data=data,id](Windows::Foundation::IInspectable const& sender,auto&&){
            auto menu=sender.as<MenuFlyout>();menu.Items().Clear();auto spec=object(rowFor(data,id),L"reset");
            if(!spec.Size())return;
            MenuFlyoutItem item;item.Text(str(spec,L"label"));item.KeyboardAcceleratorTextOverride(str(spec,L"hint"));item.IsEnabled(flag(spec,L"enabled"));
            item.Click([data,id](auto&&,auto&&){send(data,O({{L"type",S(L"reset")},{L"id",S(id)}}));});menu.Items().Append(item);
        });
        ContentControl field;field.IsTabStop(false);field.HorizontalContentAlignment(HorizontalAlignment::Stretch);field.Content(line);
        field.ContextFlyout(reset);rows.emplace(id.c_str(),field);
        bindings.emplace_back([data=data,id,field]{
            auto row=rowFor(data,id);field.Visibility(flag(row,L"visible",true)?Visibility::Visible:Visibility::Collapsed);
            field.IsEnabled(flag(row,L"enabled",true));field.Opacity(flag(row,L"enabled",true)?1:.45);
        });
        return field;
    }
    void updateLists(J const& model){
        auto resultsModel=array(model,L"search_results");auto resultKey=resultsModel.Stringify();
        if(resultKey!=resultsKey){resultsKey=resultKey;results.Children().Clear();
            for(auto value:resultsModel){auto spec=value.GetObject();results.Children().Append(actionButton(data,str(spec,L"title"),object(spec,L"action")));}
        }
        auto list=array(model,L"shortcuts");auto listKey=list.Stringify();
        if(listKey!=shortcutKey){shortcutKey=listKey;shortcuts.Children().Clear();
            for(auto value:list){auto spec=value.GetObject();if(!flag(spec,L"visible",true))continue;
                auto item=actionButton(data,str(spec,L"label"),O({{L"type",S(L"edit_shortcut")},{L"id",S(str(spec,L"id"))}}));
                StackPanel text;text.HorizontalAlignment(HorizontalAlignment::Left);text.Children().Append(label(data,str(spec,L"label")));
                text.Children().Append(description(data,str(spec,L"shortcut",L"Unassigned")));item.Content(text);shortcuts.Children().Append(item);
            }
        }
        auto capture=object(model,L"capture"),editModel=object(model,L"shortcut_editor");
        auto signature=capture.Stringify()+editModel.Stringify();
        bool editing=capture.Size()||editModel.Size();editor.Visibility(editing?Visibility::Visible:Visibility::Collapsed);pages.Visibility(editing?Visibility::Collapsed:Visibility::Visible);
        if(signature!=editorKey){editorKey=signature;editor.Children().Clear();
            if(capture.Size()){
                editor.Children().Append(label(data,L"Set Shortcut",true));editor.Children().Append(label(data,str(capture,L"label")));
                editor.Children().Append(description(data,L"Press the keys for this shortcut."));
                editor.Children().Append(label(data,str(capture,L"shortcut")));
                editor.Children().Append(description(data,str(capture,L"notice")));
                editor.Children().Append(description(data,str(capture,L"error")));
                bool conflict=capture.GetNamedValue(L"conflict").ValueType()!=JsonValueType::Null;
                auto confirm=actionButton(data,conflict?L"Replace Shortcut":L"Set Shortcut",O({{L"type",S(L"confirm_shortcut")},{L"replace",B(conflict)}}));
                confirm.IsEnabled(object(capture,L"chord").Size()!=0&&str(capture,L"error").empty());editor.Children().Append(confirm);
                auto cancel=actionButton(data,L"Cancel",O({{L"type",S(L"cancel_shortcut")}}));editor.Children().Append(cancel);
                cancel.Focus(FocusState::Programmatic);
            }else if(editModel.Size()){
                auto id=str(editModel,L"id");editor.Children().Append(label(data,str(editModel,L"label"),true));
                auto values=array(editModel,L"bindings");
                for(uint32_t i=0;i<values.Size();++i){
                    StackPanel row;row.Orientation(Orientation::Horizontal);row.Spacing(12);row.Children().Append(label(data,values.GetStringAt(i)));
                    row.Children().Append(actionButton(data,L"Remove",O({{L"type",S(L"remove_shortcut")},{L"id",S(id)},{L"index",N(i)}})));editor.Children().Append(row);
                }
                auto add=actionButton(data,L"Add Shortcut",O({{L"type",S(L"begin_shortcut")},{L"id",S(id)}}));add.IsEnabled(flag(editModel,L"can_add"));editor.Children().Append(add);
                auto reset=actionButton(data,L"Reset",O({{L"type",S(L"reset_shortcut")},{L"id",S(id)}}));reset.IsEnabled(flag(editModel,L"modified"));editor.Children().Append(reset);
                editor.Children().Append(actionButton(data,L"Back",O({{L"type",S(L"close_shortcut_editor")}})));
            }
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
        if(!snapshot.HasKey(L"state"))return;
        data->state=object(snapshot,L"state");data->refreshPalette();data->model=snapshot;auto model=preferences(data);
        if(!model.Size()){showFailed=false;if(showing&&!closing)dialog.Hide();return;}
        if(showFailed)return;
        data->updating=true;struct Reset{bool& value;~Reset(){value=false;}}reset{data->updating};
        if(!built||theme!=data->theme()){theme=data->theme();build(model);}
        dialog.RequestedTheme(theme==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        auto size=xamlRoot.Size();body.Width(std::max(320.,std::min(800.,double(size.Width)-96)));
        body.Height(std::max(200.,std::min(560.,double(size.Height)-200)));scroller.Height(std::max(140.,body.Height()-64));
        auto page=str(model,L"page"),query=str(model,L"query");
        search.Visibility(flag(model,L"searching")||!query.empty()?Visibility::Visible:Visibility::Collapsed);
        if(search.Text()!=query)search.Text(query);
        for(auto const& [id,node]:pageNodes)node.Visibility(id==page?Visibility::Visible:Visibility::Collapsed);
        for(auto const& [id,item]:tabs){
            item.Background(id==page?selected():clear());item.Visibility(query.empty()?Visibility::Visible:Visibility::Collapsed);
        }
        title.Text(str(find(array(model,L"pages"),L"id",page),L"title"));
        auto message=str(model,L"error");
        if(message.empty())message=str(snapshot,L"error");
        if(message.empty())message=str(data->state,L"host_error");
        error.Text(message);
        error.Visibility(error.Text().empty()?Visibility::Collapsed:Visibility::Visible);
        for(auto const& bind:bindings)bind();
        if(shortcutSearch.Text()!=str(model,L"shortcut_query"))shortcutSearch.Text(str(model,L"shortcut_query"));
        updateLists(model);
        auto reveal=str(model,L"reveal");
        if(reveal!=revealed){revealed=reveal;auto it=rows.find(reveal.c_str());if(it!=rows.end())it->second.StartBringIntoView();}
        if(!showing)show();
    }
};
SettingsView::SettingsView(Dispatch dispatch,Json catalog,XamlRoot root,Key key,Dispatch report,std::function<void()> changed):impl(std::make_shared<Impl>()){
    impl->data->send=std::move(dispatch);impl->data->catalog=catalog;impl->xamlRoot=root;impl->key=std::move(key);impl->report=std::move(report);impl->changed=std::move(changed);impl->init();
}
SettingsView::~SettingsView()=default;
void SettingsView::Apply(Json const& snapshot){impl->apply(snapshot);}
bool SettingsView::IsOpen()const{return impl->showing;}
void SettingsView::CommitEdits(){for(auto const& commit:impl->commits)commit();}
void SettingsView::Hide(){impl->stopping=true;if(impl->showing)impl->dialog.Hide();}
