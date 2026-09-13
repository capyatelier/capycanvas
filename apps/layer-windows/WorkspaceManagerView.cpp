#include "pch.h"
#include "WorkspaceManagerView.h"
#include "WorkspaceRowDrag.h"
#include "UiControls.h"
#include <algorithm>
#include <map>
#include <optional>

using namespace CapyUi;
struct WorkspaceManagerView::Impl:std::enable_shared_from_this<Impl> {
    Dispatch send;
    std::function<void()> changed;
    XamlRoot root{nullptr};
    ContentDialog dialog{nullptr};
    StackPanel body,listing,promptBody,toolbarTabs;
    Primitives::ToggleButton thisWorkspace,savedToolbars;
    Button toolbarActions;
    MenuFlyout toolbarMenu;
    Grid heading;
    TextBlock title,intro,error,progress,promptMessage;
    Button create,retry;
    TextBox search,name;
    ComboBox choices;
    ListView list;
    Grid listSurface;
    std::unique_ptr<WorkspaceRowDrag> rowDrag;
    uint64_t selectionSerial=0;
    struct Row {
        ListViewItem item;
        TextBlock title,subtitle;
        Image pinned,current;
        Button more,grip;
        MenuFlyout menu;
        hstring theme;
        ToggleMenuFlyoutItem show;
        MenuFlyoutItem up,down,rename,remove;
        MenuFlyoutSeparator separator;
    };
    std::map<std::wstring,Row> rows;
    std::vector<std::wstring> order;
    J snapshot,model;
    hstring promptKey,choicesKey,focusSent,toolbarActionsKey,toolbarPage;
    uint64_t focusedRequest=0;
    std::optional<hstring> nameDraft,searchDraft,selectionDraft;
    uint64_t active=0;
    bool showing=false,updating=false,programmatic=false,stopping=false,blocked=false,cancelPending=false;
    void dispatch(uint64_t id,J const& command) {
        send(to_string(O({{L"operation",S(L"manager")},{L"dialog",N(double(id))},{L"command",command}}).Stringify()));
    }
    void dispatch(J const& command){dispatch(active,command);}
    void editSwitcher(uint64_t epoch,J const& edit) {
        if(!showing||active!=epoch||prompt()||str(model,L"page")!=L"workspaces"
            ||flag(model,L"busy")||flag(object(snapshot,L"windows_workspace"),L"switcher_busy"))return;
        dispatch(epoch,O({{L"type",S(L"edit_switcher")},{L"edit",edit}}));
    }
    void moveWorkspace(uint64_t epoch,hstring const& id,bool down) {
        auto saved=array(object(snapshot,L"windows_workspace"),L"order");
        for(uint32_t i=0;i<saved.Size();++i)if(saved.GetStringAt(i)==id){
            if((down&&i+1==saved.Size())||(!down&&i==0))return;
            V before=JsonValue::CreateNullValue();
            if(!down||i+2<saved.Size())before=S(saved.GetStringAt(down?i+2:i-1));
            editSwitcher(epoch,O({{L"type",S(L"move")},{L"id",S(id)},{L"before",before}}));return;
        }
    }
    void fail() {
        send(to_string(O({{L"operation",S(L"failure")},{L"error",S(L"Windows could not show the workspace manager. Try again.")}}).Stringify()));
    }
    static void sync(TextBox const& text,hstring value,std::optional<hstring>& draft) {
        if(draft&&*draft==value)draft.reset();
        if(!draft&&text.Text()!=value)text.Text(value);
    }
    bool prompt()const{return object(model,L"prompt").Size()!=0;}
    bool canReorder()const{
        return showing&&!stopping&&!blocked&&!prompt()&&str(model,L"page")==L"workspaces"
            &&!flag(model,L"loading")&&!flag(model,L"busy")
            &&!flag(object(snapshot,L"windows_workspace"),L"switcher_busy");
    }
    void restoreSelection(){
        auto found=rows.find(std::wstring(selectionDraft.value_or(str(model,L"selected"))));
        auto desired=found==rows.end()?Windows::Foundation::IInspectable{nullptr}:found->second.item.as<Windows::Foundation::IInspectable>();
        bool previous=updating;updating=true;
        if(list.SelectedItem()!=desired)list.SelectedItem(desired);
        updating=previous;
    }
    void select(hstring const& id){
        if(id==selectionDraft.value_or(str(model,L"selected")))return;
        // Reflect the accepted UI request immediately. A subsequent accessibility
        // selection must not disappear while Rust still publishes the old preview.
        selectionDraft=id;++selectionSerial;restoreSelection();
        dispatch(O({{L"type",S(L"select")},{L"id",id.empty()?JsonValue::CreateNullValue():S(id)}}));
    }
    void cancel() {
        if(stopping||!showing||flag(model,L"busy")||cancelPending)return;
        rowDrag->Cancel();cancelPending=true;
        dispatch(O({{L"type",S(prompt()&&str(model,L"page")!=L"prompt"?L"back":L"cancel")}}));
    }
    void init() {
        body.Spacing(12);listing.Spacing(10);promptBody.Spacing(12);
        toolbarTabs.Orientation(Orientation::Horizontal);toolbarTabs.Spacing(8);
        thisWorkspace.Content(box_value(L"This Workspace"));savedToolbars.Content(box_value(L"Saved Toolbars"));
        AutomationProperties::SetAutomationId(thisWorkspace,L"workspace-toolbar-current");
        AutomationProperties::SetAutomationId(savedToolbars,L"workspace-toolbar-library");
        toolbarTabs.Children().Append(thisWorkspace);toolbarTabs.Children().Append(savedToolbars);
        thisWorkspace.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating)self->dispatch(O({{L"type",S(L"toolbar_page")},{L"page",S(L"this_workspace")}}));});
        savedToolbars.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating)self->dispatch(O({{L"type",S(L"toolbar_page")},{L"page",S(L"toolbar_library")}}));});
        toolbarActions.Content(box_value(L"Toolbar actions"));toolbarActions.HorizontalAlignment(HorizontalAlignment::Left);
        AutomationProperties::SetAutomationId(toolbarActions,L"workspace-toolbar-actions");toolbarActions.Flyout(toolbarMenu);
        heading.ColumnDefinitions().Append(ColumnDefinition());
        ColumnDefinition end;end.Width({1,GridUnitType::Auto});heading.ColumnDefinitions().Append(end);
        title.TextWrapping(TextWrapping::Wrap);title.Margin({0,0,12,0});
        create.Content(box_value(L"+"));create.Width(36);create.Height(36);create.Padding({0,0,0,0});
        Grid::SetColumn(create,1);heading.Children().Append(title);heading.Children().Append(create);
        AutomationProperties::SetAutomationId(create,L"workspace-manager-create");
        intro.TextWrapping(TextWrapping::Wrap);promptMessage.TextWrapping(TextWrapping::Wrap);
        error.TextWrapping(TextWrapping::Wrap);
        progress.Text(L"Loading…");progress.Visibility(Visibility::Collapsed);
        search.PlaceholderText(L"Search");AutomationProperties::SetAutomationId(search,L"workspace-manager-search");
        list.SelectionMode(ListViewSelectionMode::Single);list.IsItemClickEnabled(true);
        ScrollViewer::SetHorizontalScrollBarVisibility(list,ScrollBarVisibility::Disabled);
        AutomationProperties::SetAutomationId(list,L"workspace-manager-items");
        name.Header(box_value(L"Name"));name.MaxLength(100);
        AutomationProperties::SetAutomationId(name,L"workspace-manager-name");
        choices.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetAutomationId(choices,L"workspace-manager-choice");
        retry.Content(box_value(L"Retry"));retry.HorizontalAlignment(HorizontalAlignment::Left);
        AutomationProperties::SetAutomationId(retry,L"workspace-manager-retry");
        listSurface.Children().Append(list);
        rowDrag=std::make_unique<WorkspaceRowDrag>(list,listSurface,
            [weak=weak_from_this()]{auto self=weak.lock();return self&&self->canReorder();},
            [weak=weak_from_this()](hstring id,std::optional<hstring> before){if(auto self=weak.lock())
                self->editSwitcher(self->active,O({{L"type",S(L"move")},{L"id",S(id)},{L"before",before?S(*before):JsonValue::CreateNullValue()}}));},
            [weak=weak_from_this()](hstring id){if(auto self=weak.lock())self->select(id);},
            [weak=weak_from_this()]{if(auto self=weak.lock()){++self->selectionSerial;self->restoreSelection();}});
        listing.Children().Append(search);listing.Children().Append(listSurface);
        promptBody.Children().Append(promptMessage);promptBody.Children().Append(name);promptBody.Children().Append(choices);
        body.Children().Append(toolbarTabs);body.Children().Append(intro);body.Children().Append(listing);body.Children().Append(toolbarActions);body.Children().Append(promptBody);
        body.Children().Append(progress);body.Children().Append(error);body.Children().Append(retry);
        create.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->dispatch(O({{L"type",S(L"create")}}));});
        retry.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->dispatch(O({{L"type",S(L"retry")}}));});
        search.TextChanged([weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock();self&&!self->updating){self->searchDraft=self->search.Text();self->dispatch(O({{L"type",S(L"search")},{L"query",S(self->search.Text())}}));}
        });
        name.TextChanging([weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock();self&&!self->updating)self->nameDraft=self->name.Text();
        });
        list.SelectionChanged([weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock();self&&!self->updating){
                auto item=self->list.SelectedItem().try_as<ListViewItem>();
                auto id=item?unbox_value<hstring>(item.Tag()):hstring{};
                auto epoch=self->active,serial=++self->selectionSerial;
                // ListView can select during its class PointerPressed handler.
                // Let the routed press establish gesture ownership first.
                self->list.DispatcherQueue().TryEnqueue([weak,epoch,serial,id]{
                    if(auto owner=weak.lock();owner&&owner->showing&&owner->active==epoch&&owner->selectionSerial==serial){
                        if(owner->rowDrag->FenceSelection())owner->restoreSelection();
                        else owner->select(id);
                    }
                });
            }
        });
        // Enter and double-click on a row only select/preview, never apply.
        list.ItemClick([weak=weak_from_this()](auto&&,ItemClickEventArgs const& e){
            if(auto self=weak.lock();self&&!self->updating&&!self->rowDrag->SuppressClick()){
                auto item=e.ClickedItem().try_as<ListViewItem>();
                if(item)self->select(unbox_value<hstring>(item.Tag()));
            }
        });
        list.KeyDown([weak=weak_from_this()](auto&&,Input::KeyRoutedEventArgs const& e){
            if(e.Key()!=Windows::System::VirtualKey::Enter)return;
            e.Handled(true);
            if(auto self=weak.lock();self&&!self->rowDrag->FenceSelection()){
                auto item=self->list.SelectedItem().try_as<ListViewItem>();
                if(item)self->select(unbox_value<hstring>(item.Tag()));
            }
        });
    }
    Row makeRow(hstring id,uint64_t epoch) {
        Row row;row.item.Tag(box_value(id));row.item.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetAutomationId(row.item,L"workspace-manager-row-"+id);
        Grid content;content.MinHeight(48);
        ColumnDefinition handle;handle.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(handle);
        content.ColumnDefinitions().Append(ColumnDefinition());
        ColumnDefinition pinColumn;pinColumn.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(pinColumn);
        ColumnDefinition currentColumn;currentColumn.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(currentColumn);
        ColumnDefinition end;end.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(end);
        StackPanel text;text.Spacing(2);text.Margin({0,4,8,4});text.VerticalAlignment(VerticalAlignment::Center);
        row.title.TextWrapping(TextWrapping::Wrap);row.subtitle.TextWrapping(TextWrapping::Wrap);
        row.subtitle.FontSize(12);row.subtitle.Opacity(.7);
        text.Children().Append(row.title);text.Children().Append(row.subtitle);Grid::SetColumn(text,1);content.Children().Append(text);
        row.more.Content(box_value(L"⋮"));row.more.Width(32);row.more.Height(32);row.more.Padding({0,0,0,0});
        row.more.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(row.more,4);
        AutomationProperties::SetAutomationId(row.more,L"workspace-manager-options-"+id);
        auto options=row.menu;row.rename.Text(L"Rename…");row.remove.Text(L"Delete…");
        AutomationProperties::SetAutomationId(row.rename,L"workspace-manager-rename");
        AutomationProperties::SetAutomationId(row.remove,L"workspace-manager-delete");
        row.rename.Click([weak=weak_from_this(),id,epoch](auto&&,auto&&){if(auto self=weak.lock())self->dispatch(epoch,O({{L"type",S(L"rename")},{L"id",S(id)}}));});
        row.remove.Click([weak=weak_from_this(),id,epoch](auto&&,auto&&){if(auto self=weak.lock())self->dispatch(epoch,O({{L"type",S(L"delete")},{L"id",S(id)}}));});
        row.show.Text(L"Show in top bar");row.up.Text(L"Move Up");row.down.Text(L"Move Down");
        AutomationProperties::SetAutomationId(row.show,L"workspace-manager-show");
        AutomationProperties::SetAutomationId(row.up,L"workspace-manager-move-up");
        AutomationProperties::SetAutomationId(row.down,L"workspace-manager-move-down");
        // Observe checked state for pointer, keyboard and accessibility actions;
        // model-driven updates are guarded below.
        row.show.RegisterPropertyChangedCallback(ToggleMenuFlyoutItem::IsCheckedProperty(),
            [weak=weak_from_this(),id,epoch](DependencyObject const& sender,auto&&){if(auto self=weak.lock()){
                if(!self->updating)self->editSwitcher(epoch,O({{L"type",S(L"show")},{L"id",S(id)},{L"visible",B(sender.as<ToggleMenuFlyoutItem>().IsChecked())}}));}});
        row.up.Click([weak=weak_from_this(),id,epoch](auto&&,auto&&){if(auto self=weak.lock())self->moveWorkspace(epoch,id,false);});
        row.down.Click([weak=weak_from_this(),id,epoch](auto&&,auto&&){if(auto self=weak.lock())self->moveWorkspace(epoch,id,true);});
        options.Items().Append(row.show);options.Items().Append(row.up);options.Items().Append(row.down);options.Items().Append(row.separator);
        options.Items().Append(row.rename);options.Items().Append(row.remove);row.more.Flyout(options);
        row.pinned=icon(L"pin",str(object(snapshot,L"state"),L"theme",L"dark"),14);row.pinned.Opacity(.7);
        row.pinned.Margin({0,0,8,0});row.pinned.VerticalAlignment(VerticalAlignment::Center);
        Grid::SetColumn(row.pinned,2);content.Children().Append(row.pinned);
        AutomationProperties::SetAutomationId(row.pinned,L"workspace-manager-pinned-"+id);
        AutomationProperties::SetName(row.pinned,L"Shown in top bar");
        row.pinned.IsHitTestVisible(true);ToolTipService::SetToolTip(row.pinned,box_value(L"Shown in top bar"));
        row.current.Width(14);row.current.Height(14);row.current.Margin({0,0,8,0});
        row.current.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(row.current,3);content.Children().Append(row.current);
        AutomationProperties::SetAutomationId(row.current,L"workspace-manager-current-"+id);
        AutomationProperties::SetName(row.current,L"Current workspace");ToolTipService::SetToolTip(row.current,box_value(L"Current workspace"));
        row.grip.Width(20);row.grip.Height(32);row.grip.Margin({0,0,8,0});row.grip.Padding({0,0,0,0});
        row.grip.Background(clear());row.grip.BorderThickness({0,0,0,0});
        row.grip.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(row.grip,0);
        AutomationProperties::SetAutomationId(row.grip,L"workspace-manager-grip-"+id);
        AutomationProperties::SetHelpText(row.grip,L"Drag to reorder, or open for Move Up and Move Down.");
        content.Children().Append(row.more);content.Children().Append(row.grip);row.item.Content(content);
        rowDrag->Attach(row.item,row.grip,row.more,row.menu,id);return row;
    }
    void applyRows() {
        auto values=array(model,L"rows");std::vector<std::wstring> next;
        auto storage=object(snapshot,L"windows_workspace");auto pins=array(storage,L"switcher"),saved=array(storage,L"order");
        bool configurable=str(model,L"page")==L"workspaces",saving=flag(storage,L"switcher_busy");
        for(auto value:values){
            auto data=value.GetObject();auto id=str(data,L"id");std::wstring key(id);next.push_back(key);
            auto found=rows.find(key);
            if(found==rows.end())found=rows.emplace(key,makeRow(id,active)).first;
            auto& row=found->second;auto label=str(data,L"title"),detail=str(data,L"subtitle");
            row.title.Text(label);row.subtitle.Text(detail);row.subtitle.Visibility(detail.empty()?Visibility::Collapsed:Visibility::Visible);
            bool pinned=false;for(auto pin:pins)if(str(pin.GetObject(),L"id")==id)pinned=true;
            uint32_t position=saved.Size();for(uint32_t i=0;i<saved.Size();++i)if(saved.GetStringAt(i)==id)position=i;
            row.pinned.Visibility(configurable&&pinned?Visibility::Visible:Visibility::Collapsed);
            row.current.Visibility(configurable&&id==str(storage,L"id")?Visibility::Visible:Visibility::Collapsed);
            auto theme=str(object(snapshot,L"state"),L"theme",L"dark");
            if(theme!=row.theme){row.theme=theme;row.pinned.Source(icon(L"pin",theme,14).Source());row.current.Source(icon(L"check",theme,14).Source());row.grip.Content(panelGrip(theme));}
            row.grip.Visibility(configurable?Visibility::Visible:Visibility::Collapsed);row.grip.IsEnabled(canReorder());
            AutomationProperties::SetName(row.grip,L"Reorder "+label);
            row.show.IsChecked(pinned);row.show.IsEnabled(!saving);row.up.IsEnabled(!saving&&position>0&&position<saved.Size());
            row.down.IsEnabled(!saving&&position+1<saved.Size());
            for(auto item:{row.show.as<UIElement>(),row.up.as<UIElement>(),row.down.as<UIElement>(),row.separator.as<UIElement>()})
                item.Visibility(configurable?Visibility::Visible:Visibility::Collapsed);
            row.rename.IsEnabled(!saving&&flag(data,L"rename"));row.remove.IsEnabled(!saving&&flag(data,L"delete"));
            row.more.Visibility(configurable||flag(data,L"rename")||flag(data,L"delete")?Visibility::Visible:Visibility::Collapsed);
            AutomationProperties::SetName(row.item,label+(detail.empty()?L"":L" · "+detail));
            AutomationProperties::SetName(row.more,L"Options for "+label);
        }
        if(next!=order){
            auto items=list.Items();
            for(uint32_t i=0;i<next.size();++i){
                auto item=rows.at(next[i]).item;
                if(i<items.Size()&&items.GetAt(i)==item)continue;
                uint32_t from=0;if(items.IndexOf(item,from))items.RemoveAt(from);
                items.InsertAt(i,item);
            }
            while(items.Size()>next.size())items.RemoveAtEnd();
            for(auto it=rows.begin();it!=rows.end();)if(std::find(next.begin(),next.end(),it->first)==next.end())it=rows.erase(it);else ++it;
            order=std::move(next);
        }
        if(selectionDraft&&(*selectionDraft==str(model,L"selected")||!rows.contains(std::wstring(*selectionDraft))))selectionDraft.reset();
        restoreSelection();
        std::vector<hstring> savedOrder;for(auto id:saved)savedOrder.push_back(id.GetString());
        rowDrag->Refresh(savedOrder);
    }
    void update() {
        if(!dialog||!model.Size())return;
        if(!canReorder())rowDrag->Cancel();
        updating=true;struct Reset{bool& flag;~Reset(){flag=false;}}reset{updating};
        auto details=object(model,L"prompt");bool naming=details.Size()!=0,busy=flag(model,L"busy");
        auto page=str(model,L"page");bool toolbars=page==L"this_workspace"||page==L"toolbar_library";
        if(page!=toolbarPage){toolbarPage=page;searchDraft.reset();}
        toolbarTabs.Visibility(toolbars&&!naming?Visibility::Visible:Visibility::Collapsed);
        thisWorkspace.IsChecked(page==L"this_workspace");savedToolbars.IsChecked(page==L"toolbar_library");
        thisWorkspace.IsEnabled(!busy);savedToolbars.IsEnabled(!busy);
        auto actions=array(model,L"toolbar_actions");auto actionKey=actions.Stringify();
        if(actionKey!=toolbarActionsKey){
            toolbarActionsKey=actionKey;toolbarMenu.Items().Clear();
            for(auto value:actions){
                auto data=value.GetObject();if(flag(data,L"primary"))continue;
                auto action=object(data,L"action");MenuFlyoutItem item;item.Text(str(data,L"label"));item.IsEnabled(flag(data,L"enabled"));
                AutomationProperties::SetAutomationId(item,L"workspace-toolbar-"+str(action,L"type"));
                auto epoch=active;item.Click([weak=weak_from_this(),action,epoch](auto&&,auto&&){if(auto self=weak.lock())self->dispatch(epoch,O({{L"type",S(L"toolbar")},{L"action",action}}));});
                toolbarMenu.Items().Append(item);
            }
        }
        toolbarActions.Visibility(toolbars&&!naming&&toolbarMenu.Items().Size()?Visibility::Visible:Visibility::Collapsed);
        toolbarActions.IsEnabled(!busy&&!flag(model,L"loading"));
        auto key=naming?str(details,L"title")+L":"+str(details,L"confirm"):L"";
        if(key!=promptKey){promptKey=key;nameDraft.reset();choicesKey=L"";}
        title.Text(naming?str(details,L"title"):str(model,L"title"));
        auto theme=str(object(snapshot,L"state"),L"theme",L"dark");
        dialog.RequestedTheme(theme==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        body.Width(std::max(200.,std::min(412.,double(root.Size().Width)-80.)));
        list.Height(std::max(120.,std::min(280.,double(root.Size().Height)-(toolbars?430.:330.))));
        heading.Width(body.Width());
        intro.Text(str(model,L"intro"));intro.Visibility(naming||intro.Text().empty()?Visibility::Collapsed:Visibility::Visible);
        create.Visibility(!naming&&(page==L"workspaces"||page==L"this_workspace")?Visibility::Visible:Visibility::Collapsed);
        create.IsEnabled(!busy&&!flag(model,L"loading"));AutomationProperties::SetName(create,page==L"this_workspace"?L"New Toolbar":L"New Workspace");
        listing.Visibility(naming||page==L"prompt"?Visibility::Collapsed:Visibility::Visible);
        promptBody.Visibility(naming?Visibility::Visible:Visibility::Collapsed);
        list.IsEnabled(!busy);search.IsEnabled(!busy);
        search.Visibility(page==L"history"||array(model,L"rows").Size()<=7&&str(model,L"query").empty()?Visibility::Collapsed:Visibility::Visible);
        sync(search,str(model,L"query"),searchDraft);
        if(!naming)applyRows();
        if(naming){
            promptMessage.Text(str(details,L"message"));promptMessage.Visibility(promptMessage.Text().empty()?Visibility::Collapsed:Visibility::Visible);
            bool named=details.HasKey(L"name")&&details.GetNamedValue(L"name").ValueType()==Windows::Data::Json::JsonValueType::String;
            name.Visibility(named?Visibility::Visible:Visibility::Collapsed);name.IsEnabled(!busy);
            sync(name,str(details,L"name"),nameDraft);
            auto options=array(details,L"choices");choices.Visibility(options.Size()?Visibility::Visible:Visibility::Collapsed);
            choices.Header(box_value(str(details,L"choice_label")));choices.IsEnabled(!busy);
            auto signature=options.Stringify();
            if(signature!=choicesKey){
                choicesKey=signature;choices.Items().Clear();
                auto chosen=str(details,L"choice");
                for(auto value:options){
                    auto option=value.GetObject();ComboBoxItem item;item.Tag(box_value(str(option,L"id")));item.Content(box_value(str(option,L"title")));
                    choices.Items().Append(item);if(str(option,L"id")==chosen)choices.SelectedItem(item);
                }
            }
        }
        auto message=str(model,L"error");if(message.empty())message=str(object(snapshot,L"windows_workspace"),L"switcher_error");error.Text(message);error.Visibility(message.empty()?Visibility::Collapsed:Visibility::Visible);
        error.Foreground(fill(color(theme==L"dark"?L"#ffb4ab":L"#b3261e")));
        progress.Text(busy?L"Saving…":L"Loading…");progress.Visibility(busy||flag(model,L"loading")?Visibility::Visible:Visibility::Collapsed);
        retry.Visibility(flag(model,L"can_retry")?Visibility::Visible:Visibility::Collapsed);retry.IsEnabled(!busy);
        dialog.PrimaryButtonText(naming?str(details,L"confirm"):str(model,L"apply_label"));
        dialog.IsPrimaryButtonEnabled(!busy&&!flag(model,L"loading")&&(naming||flag(model,L"can_apply")));
        dialog.CloseButtonText(L"Cancel");
        dialog.DefaultButton(naming?ContentDialogButton::Primary:ContentDialogButton::None);
        cancelPending=false;
    }
    fire_and_forget show() {
        auto lifetime=shared_from_this();
        if(showing||stopping||blocked||!model.Size())co_return;
        showing=true;active=uint64_t(num(model,L"id"));auto epoch=active;changed();
        try{
            dialog=ContentDialog();dialog.XamlRoot(root);dialog.Title(heading);dialog.Content(body);
            AutomationProperties::SetAutomationId(dialog,L"workspace-manager");
            dialog.PreviewKeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
                if(e.Key()==Windows::System::VirtualKey::Escape)if(auto self=weak.lock();self&&self->rowDrag->Escape())e.Handled(true);
            });
            dialog.PrimaryButtonClick([weak=weak_from_this()](auto&&,ContentDialogButtonClickEventArgs const& e){
                e.Cancel(true);if(auto self=weak.lock()){
                    if(self->prompt()){
                        auto selected=self->choices.SelectedItem().try_as<ComboBoxItem>();
                        auto option=selected?S(unbox_value<hstring>(selected.Tag())):Windows::Data::Json::JsonValue::CreateNullValue();
                        self->dispatch(O({{L"type",S(L"submit")},{L"name",S(self->name.Text())},{L"choice",option}}));
                    }else self->dispatch(O({{L"type",S(L"apply")}}));
                }
            });
            dialog.CloseButtonClick([weak=weak_from_this()](auto&&,ContentDialogButtonClickEventArgs const& e){
                e.Cancel(true);if(auto self=weak.lock())self->cancel();
            });
            dialog.Closing([weak=weak_from_this()](auto&&,ContentDialogClosingEventArgs const& e){
                if(auto self=weak.lock();self&&!self->programmatic&&!self->stopping){e.Cancel(true);self->cancel();}
            });
            update();co_await dialog.ShowAsync();
        }catch(hresult_canceled const&){
        }catch(...){if(!stopping)fail();}
        rowDrag->Cancel();
        if(dialog){dialog.Content(nullptr);dialog.Title(nullptr);}dialog=nullptr;
        if(!stopping&&!programmatic)dispatch(epoch,O({{L"type",S(L"cancel")}}));
        list.Items().Clear();rows.clear();order.clear();promptKey=L"";choicesKey=L"";nameDraft.reset();searchDraft.reset();selectionDraft.reset();
        toolbarActionsKey=L"";toolbarPage=L"";toolbarMenu.Items().Clear();
        showing=false;programmatic=false;active=0;changed();
    }
    static hstring focusOwner(hstring const& owner) {
        struct Target {std::wstring property;HWND window=nullptr;} target{L"CapyCanvas.WorkspaceOwner."+std::wstring(owner)};
        EnumWindows([](HWND window,LPARAM context)->BOOL{
            auto& target=*reinterpret_cast<Target*>(context);
            if(GetPropW(window,target.property.c_str())&&IsWindowVisible(window)){target.window=window;return FALSE;}
            return TRUE;
        },reinterpret_cast<LPARAM>(&target));
        if(!target.window)return L"This workspace is open in another window that is no longer available. Close it there or retry after its ownership expires.";
        if(IsIconic(target.window))ShowWindowAsync(target.window,SW_RESTORE);
        if(!SetForegroundWindow(target.window)&&GetForegroundWindow()!=target.window)return L"Windows could not activate the other window. Select it from the taskbar.";
        return L"";
    }
    void apply(J const& value,bool unavailable) {
        snapshot=value;model=object(snapshot,L"windows_workspace_manager");blocked=unavailable;
        if(stopping)return;
        auto desired=uint64_t(num(model,L"id"));
        auto owner=str(model,L"focus_owner");
        if(!owner.empty()){
            // ContentDialog restores focus to its source while closing. Activate
            // another window only after ShowAsync has fully released the dialog.
            if(showing){programmatic=true;if(dialog)dialog.Hide();return;}
            if(owner!=focusSent||desired!=focusedRequest){
                focusSent=owner;focusedRequest=desired;auto focusError=focusOwner(owner);
                V result=focusError.empty()?JsonValue::CreateNullValue():S(focusError);
                dispatch(desired,O({{L"type",S(L"focus_result")},{L"error",result}}));
            }
            return;
        }
        focusSent=L"";
        if(showing){
            if(!model.Size()||desired!=active){programmatic=true;if(dialog)dialog.Hide();}
            else update();
        }else if(model.Size()&&!blocked)show();
    }
};
WorkspaceManagerView::WorkspaceManagerView(Dispatch send,XamlRoot root,std::function<void()> changed):impl(std::make_shared<Impl>()){
    impl->send=std::move(send);impl->root=root;impl->changed=std::move(changed);impl->init();
}
WorkspaceManagerView::~WorkspaceManagerView()=default;
void WorkspaceManagerView::Apply(Json const& snapshot,bool blocked){impl->apply(snapshot,blocked);}
bool WorkspaceManagerView::IsOpen()const{return impl->showing;}
void WorkspaceManagerView::CancelAll(){impl->rowDrag->Cancel();if(impl->showing)impl->dispatch(CapyUi::O({{L"type",CapyUi::S(L"cancel")}}));}
void WorkspaceManagerView::Hide(){impl->rowDrag->Cancel();impl->stopping=true;impl->programmatic=true;if(impl->dialog)impl->dialog.Hide();}
