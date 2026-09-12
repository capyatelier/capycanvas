#include "pch.h"
#include "WorkspaceDialogs.h"
#include "UiControls.h"
#include <limits>
#include <optional>

using namespace CapyUi;
namespace {
hstring choiceId(J const& choice){
    auto control=object(choice,L"control");auto kind=str(control,L"kind");
    auto value=str(control,L"command",str(control,L"panel"));
    if(value.empty())value=to_hstring(uint32_t(num(control,L"id",num(control,L"pixels"))));
    return kind+L"-"+value;
}
}
struct WorkspaceDialogs::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    ContentDialog dialog;
    XamlRoot xamlRoot{nullptr};
    Grid body;
    TextBox name,search;
    TextBlock description,error;
    ScrollView scroller;
    ItemsRepeater repeater;
    Windows::Foundation::Collections::IObservableVector<Windows::Foundation::IInspectable> source{
        single_threaded_observable_vector<Windows::Foundation::IInspectable>()};
    struct Row {Primitives::ToggleButton toggle{nullptr};Image image;TextBlock title,detail;hstring iconKey;std::shared_ptr<bool> updating=std::make_shared<bool>(false);};
    std::map<void*,Row> rows;
    std::map<std::wstring,J> choices;
    hstring kind,currentIdentity;
    bool showing=false,closing=false,programmatic=false,stopping=false,failed=false,cancelPending=false;
    std::optional<hstring> nameDraft,searchDraft;
    static void sync(TextBox const& entry,hstring const& value,std::optional<hstring>& draft){
        if(draft&&*draft==value)draft.reset();
        if(!draft&&entry.Text()!=value)entry.Text(value);
    }
    std::function<void()> changed;
    Dispatch report;
    struct Factory:implements<Factory,IElementFactory>{
        std::weak_ptr<Impl> owner;
        UIElement GetElement(ElementFactoryGetArgs const& args){
            if(auto view=owner.lock())return view->makeRow(unbox_value<hstring>(args.Data()));
            return Border();
        }
        void RecycleElement(ElementFactoryRecycleArgs const& args){
            if(auto view=owner.lock())view->rows.erase(args.Element().as<::IUnknown>().get());
        }
    };
    hstring desiredKind()const{
        if(object(data->model,L"toolbar_prompt").Size())return L"toolbar_prompt";
        if(object(data->model,L"picker").Size())return L"picker";
        if(object(data->model,L"toolbar_manager").Size())return L"toolbar_manager";
        return L"";
    }
    J view()const{return kind.empty()?J{}:object(data->model,kind.c_str());}
    hstring identity(hstring next)const{
        auto customization=object(data->state,L"customization");
        auto state=object(customization,next.c_str());
        if(next==L"picker"){
            auto destination=J::Parse(object(state,L"destination").Stringify());
            if(destination.HasKey(L"name"))destination.Remove(L"name");
            return next+destination.Stringify();
        }
        if(next==L"toolbar_prompt")return next+str(state,L"panel")+L":"+str(state,L"operation");
        return next;
    }
    void send(J const& action)const{data->dispatch(O({{L"type",S(L"customize")},{L"action",action}}));}
    void cancel(hstring target){
        if(target.empty())return;
        send(O({{L"type",S(target==L"picker"?L"cancel_tools":target==L"toolbar_prompt"?L"cancel_toolbar":L"close_toolbar_manager")}}));
    }
    void cancelAll(){
        auto customization=object(data->state,L"customization");
        for(auto target:{L"picker",L"toolbar_prompt",L"toolbar_manager"})if(object(customization,target).Size())cancel(target);
    }
    void init(){
        dialog.XamlRoot(xamlRoot);dialog.Content(body);dialog.DefaultButton(ContentDialogButton::Primary);
        dialog.Resources().Insert(box_value(L"ContentDialogMaxWidth"),box_value(620.));
        dialog.Resources().Insert(box_value(L"ContentDialogMinWidth"),box_value(0.));
        body.RowSpacing(12);
        for(auto unit:{GridUnitType::Auto,GridUnitType::Auto,GridUnitType::Auto,GridUnitType::Star,GridUnitType::Auto}){
            RowDefinition row;row.Height({1,unit});body.RowDefinitions().Append(row);
        }
        description.TextWrapping(TextWrapping::Wrap);body.Children().Append(description);
        name.HorizontalAlignment(HorizontalAlignment::Stretch);name.MaxLength(256);
        Grid::SetRow(name,1);body.Children().Append(name);
        search.HorizontalAlignment(HorizontalAlignment::Stretch);
        Grid::SetRow(search,2);body.Children().Append(search);
        AutomationProperties::SetAutomationId(name,L"toolbar-name");
        AutomationProperties::SetAutomationId(search,L"tool-picker-search");
        auto factory=make_self<Factory>();factory->owner=weak_from_this();repeater.ItemTemplate(factory.as<IElementFactory>());
        StackLayout layout;layout.Orientation(Orientation::Vertical);layout.Spacing(1);repeater.Layout(layout);
        repeater.ItemsSource(source);repeater.VerticalCacheLength(.5);repeater.HorizontalCacheLength(0);
        scroller.Content(repeater);scroller.HorizontalScrollMode(ScrollingScrollMode::Disabled);
        scroller.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
        scroller.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);
        AutomationProperties::SetAutomationId(scroller,L"workspace-dialog-list");
        Grid::SetRow(scroller,3);body.Children().Append(scroller);
        error.TextWrapping(TextWrapping::Wrap);Grid::SetRow(error,4);body.Children().Append(error);
        AutomationProperties::SetAutomationId(error,L"workspace-dialog-error");
        auto weak=weak_from_this();
        // TextChanging is synchronous; TextChanged arrives after rendering and
        // can lose a newer edit to an intervening owner snapshot.
        name.TextChanging([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating&&!self->closing&&!self->cancelPending){
            self->nameDraft=self->name.Text();
            self->send(O({{L"type",S(self->kind==L"picker"?L"picker_name":L"toolbar_name")},{L"name",S(self->name.Text())}}));
        }});
        name.TextChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->nameDraft){
            self->dialog.IsPrimaryButtonEnabled(false);
        }});
        search.TextChanging([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating&&!self->closing&&self->kind==L"picker"&&!self->cancelPending){
            self->searchDraft=self->search.Text();
            self->send(O({{L"type",S(L"picker_search")},{L"query",S(self->search.Text())}}));
        }});
        dialog.PrimaryButtonClick([weak](auto&&,ContentDialogButtonClickEventArgs const& e){
            e.Cancel(true);
            if(auto self=weak.lock();self&&!self->closing&&!self->cancelPending&&!self->nameDraft){
                auto view=self->view();if(!view.Size())return;
                if(self->kind==L"toolbar_manager"){
                    auto action=object(view,L"delete_action");if(action.Size())self->send(action);
                }else if(flag(view,L"can_confirm")){
                    self->send(O({{L"type",S(self->kind==L"picker"?L"confirm_tools":L"confirm_toolbar")}}));
                }
            }
        });
        dialog.Closing([weak](auto&&,ContentDialogClosingEventArgs const& e){if(auto self=weak.lock()){
            if(!self->stopping&&!self->programmatic&&self->currentIdentity==self->identity(self->desiredKind())){
                // Keep the native modal until Core acknowledges cancellation.
                // A delayed snapshot must not reopen it after ShowAsync returns.
                e.Cancel(true);
                if(!self->cancelPending){
                    self->cancelPending=true;self->dialog.IsPrimaryButtonEnabled(false);self->cancel(self->kind);
                }
                return;
            }
            self->closing=true;
        }});
        dialog.Opened([weak](auto&&,auto&&){if(auto self=weak.lock()){
            if(self->name.Visibility()==Visibility::Visible){self->name.Focus(FocusState::Programmatic);self->name.SelectAll();}
            else if(self->search.Visibility()==Visibility::Visible)self->search.Focus(FocusState::Programmatic);
        }});
    }
    UIElement makeRow(hstring const& key){
        auto found=choices.find(std::wstring(key));if(found==choices.end())return Border();
        Row row;auto spec=found->second;
        if(kind==L"picker")row.toggle=CheckBox();else {RadioButton radio;radio.GroupName(L"workspace-managed-toolbars");row.toggle=radio;}
        row.toggle.Tag(box_value(key));row.toggle.MinHeight(kind==L"picker"?58:64);
        row.toggle.Padding({8,8,8,8});row.toggle.HorizontalAlignment(HorizontalAlignment::Stretch);
        row.toggle.HorizontalContentAlignment(HorizontalAlignment::Stretch);row.toggle.FontSize(data->textSize());
        Grid content;content.ColumnSpacing(12);
        ColumnDefinition imageColumn;imageColumn.Width({24,GridUnitType::Pixel});content.ColumnDefinitions().Append(imageColumn);
        ColumnDefinition textColumn;textColumn.Width({1,GridUnitType::Star});content.ColumnDefinitions().Append(textColumn);
        row.image.Width(20);row.image.Height(20);row.image.VerticalAlignment(VerticalAlignment::Center);row.image.IsHitTestVisible(false);
        content.Children().Append(row.image);
        StackPanel labels;labels.Spacing(3);
        row.title.FontSize(data->textSize());row.title.TextWrapping(TextWrapping::Wrap);row.title.FontWeight(Windows::UI::Text::FontWeights::SemiBold());
        row.detail.FontSize(data->textSize()*.833333);row.detail.TextWrapping(TextWrapping::Wrap);
        labels.Children().Append(row.title);labels.Children().Append(row.detail);Grid::SetColumn(labels,1);content.Children().Append(labels);
        row.toggle.Content(content);
        auto weak=weak_from_this();auto toggle=make_weak(row.toggle);
        auto select=[weak,toggle,key,updating=row.updating](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating&&!*updating&&!self->closing&&!self->cancelPending){
            auto selected=toggle.get();auto found=self->choices.find(std::wstring(key));if(!selected||found==self->choices.end())return;
            if(self->kind==L"picker")self->send(O({{L"type",S(L"picker_select")},{L"control",object(found->second,L"control")},
                {L"selected",B(selected.IsChecked()&&selected.IsChecked().Value())}}));
            else if(selected.IsChecked()&&selected.IsChecked().Value())self->send(O({{L"type",S(L"select_managed_toolbar")},{L"panel",S(str(found->second,L"panel"))}}));
        }};
        row.toggle.Checked(select);row.toggle.Unchecked(select);
        auto control=row.toggle;auto [it,inserted]=rows.emplace(control.as<::IUnknown>().get(),std::move(row));(void)inserted;
        refreshRow(it->second,key);return control;
    }
    void refreshRow(Row& row,hstring const& key){
        auto found=choices.find(std::wstring(key));if(found==choices.end())return;
        auto spec=found->second;bool picker=kind==L"picker";
        auto title=str(spec,picker?L"label":L"title"),detail=str(spec,picker?L"description":L"subtitle");
        row.title.Text(title);row.detail.Text(detail);row.title.Foreground(data->brush(L"text"));row.detail.Foreground(data->brush(L"settings_secondary"));
        *row.updating=true;
        row.toggle.IsChecked(picker?flag(spec,L"selected"):str(view(),L"selected")==str(spec,L"panel"));
        *row.updating=false;
        AutomationProperties::SetName(row.toggle,title);AutomationProperties::SetHelpText(row.toggle,detail);
        AutomationProperties::SetAutomationId(row.toggle,picker?L"picker-choice-"+choiceId(spec):L"managed-toolbar-"+str(spec,L"panel"));
        auto imageKey=str(spec,L"icon")+L":"+data->theme();
        if(row.iconKey!=imageKey){row.iconKey=imageKey;row.image.Source(icon(str(spec,L"icon"),data->theme()).Source());}
    }
    void updateRows(J const& model){
        std::vector<hstring> keys;std::map<std::wstring,J> next;
        for(auto value:array(model,kind==L"picker"?L"choices":L"toolbars")){
            auto spec=value.GetObject();auto key=kind==L"picker"?object(spec,L"control").Stringify():str(spec,L"panel");
            keys.push_back(key);next[std::wstring(key)]=spec;
        }
        choices=std::move(next);
        for(uint32_t i=0;i<keys.size();++i){
            if(i<source.Size()&&unbox_value<hstring>(source.GetAt(i))==keys[i])continue;
            for(uint32_t j=i+1;j<source.Size();++j)if(unbox_value<hstring>(source.GetAt(j))==keys[i]){source.RemoveAt(j);break;}
            source.InsertAt(i,box_value(keys[i]));
        }
        while(source.Size()>keys.size())source.RemoveAtEnd();
        for(auto& [_,row]:rows)refreshRow(row,unbox_value<hstring>(row.toggle.Tag()));
    }
    fire_and_forget show(){
        auto lifetime=shared_from_this();showing=true;closing=false;programmatic=false;cancelPending=false;
        try{co_await dialog.ShowAsync();}
        catch(hresult_error const& error){
            failed=true;
            if(!stopping){cancel(kind);report("Cannot open workspace dialog: "+to_string(error.message()));}
        }
        showing=false;closing=false;programmatic=false;cancelPending=false;changed();
    }
    void hide(){if(showing&&!closing){programmatic=true;closing=true;dialog.Hide();}}
    void apply(J const& snapshot,bool blocked){
        if(!snapshot.HasKey(L"state"))return;
        data->model=snapshot;data->state=object(snapshot,L"state");data->refreshPalette();
        auto next=desiredKind(),nextIdentity=identity(next);
        if(stopping||next.empty()){
            if(next.empty()){failed=false;nameDraft.reset();searchDraft.reset();}
            hide();if(next.empty())currentIdentity=L"";return;
        }
        if(showing&&nextIdentity!=currentIdentity){hide();return;}
        if(blocked||closing||failed||cancelPending)return;
        data->updating=true;struct Reset{bool& updating;~Reset(){updating=false;}}reset{data->updating};
        if(nextIdentity!=currentIdentity){
            kind=next;currentIdentity=nextIdentity;source.Clear();choices.clear();nameDraft.reset();searchDraft.reset();
        }
        auto model=view();bool picker=kind==L"picker",manager=kind==L"toolbar_manager";
        dialog.Title(box_value(str(model,L"title")));
        AutomationProperties::SetAutomationId(dialog,picker?L"tool-picker":manager?L"toolbar-manager":L"toolbar-prompt");
        dialog.RequestedTheme(data->theme()==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        dialog.CloseButtonText(manager?str(model,L"close_label"):str(model,L"cancel_label",L"Cancel"));
        dialog.PrimaryButtonText(manager?str(model,L"delete_label"):str(model,L"confirm_label"));
        auto size=xamlRoot.Size();body.Width(std::max(240.,std::min(480.,double(size.Width)-96)));
        body.Height(manager||picker?std::max(160.,std::min(manager?360.:520.,double(size.Height)-200)):std::numeric_limits<double>::quiet_NaN());
        auto text=manager?str(model,L"description"):str(model,L"message");
        if(manager&&!array(model,L"toolbars").Size())text=text+L"\n"+str(model,L"empty_label");
        description.Text(text);description.FontSize(data->textSize());description.Foreground(data->brush(L"settings_secondary"));
        description.Visibility(text.empty()?Visibility::Collapsed:Visibility::Visible);
        bool hasName=model.GetNamedValue(L"name",JsonValue::CreateNullValue()).ValueType()==JsonValueType::String;
        name.Visibility(hasName?Visibility::Visible:Visibility::Collapsed);name.Header(box_value(str(model,L"name_label")));
        sync(name,str(model,L"name"),nameDraft);
        dialog.IsPrimaryButtonEnabled(!nameDraft&&(manager?object(model,L"delete_action").Size()>0:flag(model,L"can_confirm")));
        search.Visibility(picker?Visibility::Visible:Visibility::Collapsed);search.PlaceholderText(str(model,L"search_hint"));
        AutomationProperties::SetName(search,str(model,L"search_hint"));
        sync(search,str(model,L"query"),searchDraft);
        error.Text(str(model,L"error"));error.Visibility(error.Text().empty()?Visibility::Collapsed:Visibility::Visible);
        error.FontSize(data->textSize());error.Foreground(data->brush(L"text"));
        scroller.Visibility(picker||manager?Visibility::Visible:Visibility::Collapsed);
        updateRows(model);
        if(!showing)show();
    }
};
WorkspaceDialogs::WorkspaceDialogs(Dispatch send,Json catalog,XamlRoot root,std::function<void()> changed,Dispatch report):
    impl(std::make_shared<Impl>()){
    impl->data->send=std::move(send);impl->data->catalog=catalog;impl->xamlRoot=root;
    impl->changed=std::move(changed);impl->report=std::move(report);impl->init();
}
WorkspaceDialogs::~WorkspaceDialogs()=default;
void WorkspaceDialogs::Apply(Json const& snapshot,bool blocked){impl->apply(snapshot,blocked);}
bool WorkspaceDialogs::IsOpen()const{return impl->showing;}
void WorkspaceDialogs::CancelAll(){impl->cancelAll();}
void WorkspaceDialogs::Hide(){impl->stopping=true;impl->hide();}
