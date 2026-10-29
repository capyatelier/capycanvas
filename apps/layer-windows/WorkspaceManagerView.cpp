#include "pch.h"
#include "WorkspaceManagerView.h"
#include "WorkspaceRowDrag.h"
#include "UiControls.h"
#include <winrt/Microsoft.UI.Xaml.Markup.h>
#include "WorkspaceQuery.h"
#include "WorkspaceShadow.h"
#include <algorithm>
#include <map>
#include <optional>
#include <limits>
#include <winrt/Microsoft.UI.Text.h>
#include <winrt/Microsoft.UI.Xaml.Documents.h>
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>

using namespace CapyUi;
struct WorkspaceManagerView::Impl:std::enable_shared_from_this<Impl> {
    Dispatch send;
    PreviewTransport query;
    uint64_t validationSerial=0;
    hstring validationName,validationError;
    bool nameValid=false,validationRunning=false,validationQueued=true;
    std::shared_ptr<WorkspaceData> copyData=std::make_shared<WorkspaceData>();
    J catalog;
    NativeLocalization localization;
    hstring copy(wchar_t const* group,wchar_t const* key)const{return str(object(object(catalog,L"native_copy"),group),key);}
    hstring caption(J const& request)const{
        auto source=to_string(request.Stringify());NativeString raw(capy_native_caption(localization.get(),source.c_str()),capy_string_free);
        if(!raw)throw hresult_error(E_OUTOFMEMORY);return str(J::Parse(to_hstring(raw.get())),L"text");
    }
    hstring recovery(wchar_t const* key) const {return str(object(object(catalog,L"bootstrap"),L"recovery"),key);}
    hstring common(wchar_t const* key) const {return str(object(object(catalog,L"bootstrap"),L"common"),key);}
    std::function<void()> changed;
    XamlRoot root{nullptr};
    ContentDialog dialog{nullptr};
    StackPanel body,listing,promptBody,toolbarTabs,rowActions,workspaceFooter,detailsBody;
    Grid split;
    ScrollViewer detailsScroll;
    hstring detailsKey;
    Primitives::ToggleButton thisWorkspace,savedToolbars;
    Button toolbarActions;
    MenuFlyout toolbarMenu;
    Grid heading;
    TextBlock title,intro,error,progress,promptMessage;
    Button create,removeSelected,retry,workspaceCancel,workspaceApply;
    TextBox search,name,description;
    ComboBox choices;
    ListView list;
    Grid listSurface;RichEditBox nameLine;TextBlock nameText;bool nameAlignmentPending=true;
    std::unique_ptr<WorkspaceRowDrag> rowDrag;
    uint64_t selectionSerial=0;
    struct Row {
        ListViewItem item;
        TextBlock title,subtitle;
        Image pinIcon,current;
        Primitives::ToggleButton pin;
        Button grip;
        TextBox entry;
        SwipeControl swipe;
        SwipeItem swipeDelete;
        SwipeItems swipeItems;
        MenuFlyout menu,context;
        hstring theme,captionTitle,pinGlyph;
        MenuFlyoutItem up,down,contextUp,contextDown,rename,remove;
    };
    std::map<std::wstring,Row> rows;
    std::vector<std::wstring> order;
    J snapshot,model;
    hstring promptKey,choicesKey,focusSent,toolbarActionsKey,toolbarPage;
    std::optional<hstring> nameDraft,descriptionDraft,searchDraft,selectionDraft;
    hstring editingId;
    bool submittedName=false;
    bool showing=false,updating=false,programmatic=false,stopping=false,blocked=false,cancelPending=false;
    bool inlineRename()const{return str(model,L"page")==L"workspaces"&&str(object(model,L"prompt_action"),L"type")==L"rename";}
    bool finishName(){
        if(editingId.empty()||submittedName)return true;
        auto found=rows.find(std::wstring(editingId));if(found==rows.end())return true;
        submittedName=true;
        dispatch(O({{L"type",S(L"submit")},{L"name",S(found->second.entry.Text())}}));return true;
    }
    void dispatch(J const& input) {
        auto type=str(input,L"type");
        if(type!=L"submit"&&type!=L"cancel"&&type!=L"dismiss"&&type!=L"edit_switcher"&&type!=L"refresh_switcher")if(!finishName())return;
        send(to_string(O({{L"operation",S(L"input")},{L"input",input}}).Stringify()));
    }
    static J form(hstring const& type,V value=nullptr) {
        auto action=O({{L"type",S(type)}});if(value)action.Insert(L"value",value);
        return O({{L"type",S(L"form")},{L"action",action}});
    }
    hstring page()const{auto value=str(model,L"page");return value.empty()&&prompt()?hstring(L"prompt"):value;}
    bool open()const{return !str(model,L"page").empty()||prompt();}
    void editSwitcher(J const& edit) {
        if(!showing||prompt()&&!inlineRename()||page()!=L"workspaces"||flag(model,L"busy")||flag(model,L"switcher_busy"))return;
        dispatch(O({{L"type",S(L"edit_switcher")},{L"edit",edit}}));
    }
    void moveWorkspace(hstring const& id,bool down) {
        auto saved=array(model,L"order");
        for(uint32_t i=0;i<saved.Size();++i)if(saved.GetStringAt(i)==id){
            if((down&&i+1==saved.Size())||(!down&&i==0))return;
            V before=JsonValue::CreateNullValue();
            if(!down||i+2<saved.Size())before=S(saved.GetStringAt(down?i+2:i-1));
            editSwitcher(O({{L"type",S(L"move")},{L"id",S(id)},{L"before",before}}));return;
        }
    }
    void fail() {
        send(to_string(O({{L"operation",S(L"failure")},{L"error",S(recovery(L"workspace_manager_failed"))}}).Stringify()));
    }
    static void sync(TextBox const& text,hstring value,std::optional<hstring>& draft) {
        if(draft&&*draft==value)draft.reset();
        if(!draft&&text.Text()!=value)text.Text(value);
    }
    void validateName(){
        if(!prompt()||name.Visibility()!=Visibility::Visible)return;
        auto draft=name.Text();
        if(draft!=validationName){validationName=draft;nameValid=false;validationError=L"";++validationSerial;validationQueued=true;}
        if(validationRunning||!validationQueued)return;
        validationRunning=true;validationQueued=false;auto serial=validationSerial;auto owner=promptKey;
        if(dialog)dialog.IsPrimaryButtonEnabled(false);
        bool queued=QueryWorkspace(query,O({{L"type",S(L"workspace_name_validation")},{L"name",S(draft)}}),[weak=weak_from_this(),serial,draft,owner](J reply){
            if(auto self=weak.lock()){
                self->validationRunning=false;
                if(self->showing&&self->validationSerial==serial&&self->promptKey==owner&&self->name.Text()==draft){
                    auto reason=reply.GetNamedValue(L"result",JsonValue::CreateNullValue());
                    self->nameValid=reply.GetNamedValue(L"error",JsonValue::CreateNullValue()).ValueType()==JsonValueType::Null&&reason.ValueType()==JsonValueType::Null;
                    self->validationError=reason.ValueType()==JsonValueType::String?reason.GetString():hstring();self->update();
                }else if(self->showing)self->validateName();
            }
        });
        if(!queued){validationRunning=false;validationQueued=true;auto timer=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread().CreateTimer();timer.IsRepeating(false);timer.Interval(std::chrono::milliseconds(16));timer.Tick([weak=weak_from_this()](auto&& timer,auto&&){timer.Stop();if(auto self=weak.lock();self&&self->showing)self->validateName();});timer.Start();}
    }
    bool prompt()const{return object(model,L"prompt").Size()!=0;}
    bool canReorder()const{
        return showing&&!stopping&&!blocked&&!prompt()&&page()==L"workspaces"
            &&!flag(model,L"loading")&&!flag(model,L"busy")&&!flag(model,L"switcher_busy");
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
        if(inlineRename())selectionDraft.reset();else selectionDraft=id;
        ++selectionSerial;restoreSelection();
        dispatch(O({{L"type",S(L"select")},{L"id",id.empty()?JsonValue::CreateNullValue():S(id)}}));
    }
    void cancel() {
        if(stopping||!showing||cancelPending||focusedTextComposing(root))return;
        rowDrag->Cancel();cancelPending=true;
        dispatch(O({{L"type",S(prompt()&&!inlineRename()&&page()!=L"prompt"?L"cancel":L"dismiss")}}));
    }
    void init() {
        body.Spacing(12);listing.Spacing(4);promptBody.Spacing(12);
        toolbarTabs.Orientation(Orientation::Horizontal);toolbarTabs.Spacing(8);
        thisWorkspace=button<Primitives::ToggleButton>(copyData,L"",[]{});savedToolbars=button<Primitives::ToggleButton>(copyData,L"",[]{});
        toolbarActions=button(copyData,L"",[]{});retry=button(copyData,L"",[]{});
        for(auto control:{thisWorkspace.as<Control>(),savedToolbars.as<Control>(),toolbarActions.as<Control>(),retry.as<Control>()}){control.Padding({10,5,10,5});control.MinHeight(34);}
        title=label(copyData,L"",true);title.HorizontalAlignment(HorizontalAlignment::Stretch);title.TextAlignment(TextAlignment::Center);intro=label(copyData,L"");
        thisWorkspace.Content(box_value(copy(L"header",L"this_workspace")));savedToolbars.Content(box_value(copy(L"header",L"saved_toolbars")));
        AutomationProperties::SetAutomationId(thisWorkspace,L"workspace-toolbar-current");
        AutomationProperties::SetAutomationId(savedToolbars,L"workspace-toolbar-library");
        toolbarTabs.Children().Append(thisWorkspace);toolbarTabs.Children().Append(savedToolbars);
        thisWorkspace.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating)self->dispatch(O({{L"type",S(L"open")},{L"page",S(L"this_workspace")}}));});
        savedToolbars.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating)self->dispatch(O({{L"type",S(L"open")},{L"page",S(L"toolbar_library")}}));});
        toolbarActions.Content(box_value(copy(L"header",L"toolbar_actions")));toolbarActions.HorizontalAlignment(HorizontalAlignment::Left);
        buttonColors(copyData,thisWorkspace);buttonColors(copyData,savedToolbars);buttonColors(copyData,toolbarActions);buttonColors(copyData,retry);
        AutomationProperties::SetAutomationId(toolbarActions,L"workspace-toolbar-actions");toolbarActions.Flyout(toolbarMenu);
        title.TextWrapping(TextWrapping::Wrap);heading.Children().Append(title);
        for(auto control:{create,removeSelected,workspaceCancel,workspaceApply}){
            control.Background(clear());control.BorderThickness({0,0,0,0});control.CornerRadius(copyData->controlCorner());buttonColors(copyData,control,38);
        }
        workspaceFooter.Orientation(Orientation::Horizontal);workspaceFooter.Spacing(8);workspaceFooter.HorizontalAlignment(HorizontalAlignment::Right);
        for(auto control:{workspaceCancel,workspaceApply}){control.MinWidth(0);control.MinHeight(34);control.Padding({16,0,16,0});control.FontWeight(Windows::UI::Text::FontWeights::Bold());}
        workspaceFooter.Children().Append(workspaceCancel);workspaceFooter.Children().Append(workspaceApply);
        workspaceCancel.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        workspaceApply.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->dispatch(O({{L"type",S(L"confirm")}}));});
        create.HorizontalAlignment(HorizontalAlignment::Left);create.Width(34);create.Height(34);create.MinWidth(34);create.MinHeight(34);create.Padding({0,0,0,0});
        removeSelected.Width(34);removeSelected.Height(34);removeSelected.MinWidth(34);removeSelected.MinHeight(34);removeSelected.Padding({0,0,0,0});
        rowActions.Orientation(Orientation::Horizontal);rowActions.Spacing(4);rowActions.Children().Append(create);rowActions.Children().Append(removeSelected);
        AutomationProperties::SetAutomationId(removeSelected,L"workspace-manager-remove");
        AutomationProperties::SetName(removeSelected,common(L"delete"));
        removeSelected.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->dispatch(form(L"delete",S(str(self->model,L"selected"))));});
        AutomationProperties::SetAutomationId(create,L"workspace-manager-create");
        intro.TextWrapping(TextWrapping::Wrap);promptMessage.TextWrapping(TextWrapping::Wrap);
        error.TextWrapping(TextWrapping::Wrap);
        progress.Text(recovery(L"loading"));progress.Visibility(Visibility::Collapsed);
        search.PlaceholderText(copy(L"header",L"search"));AutomationProperties::SetAutomationId(search,L"workspace-manager-search");
        list.SelectionMode(ListViewSelectionMode::Single);list.IsItemClickEnabled(true);
        ScrollViewer::SetHorizontalScrollBarVisibility(list,ScrollBarVisibility::Disabled);
        AutomationProperties::SetAutomationId(list,L"workspace-manager-items");
        name.Header(box_value(str(object(object(catalog,L"bootstrap"),L"common"),L"name")));name.MaxLength(100);
        AutomationProperties::SetAutomationId(name,L"workspace-manager-name");
        description.Header(box_value(copy(L"header",L"description")));description.AcceptsReturn(true);description.TextWrapping(TextWrapping::Wrap);description.MinHeight(72);
        AutomationProperties::SetAutomationId(description,L"workspace-manager-description");
        choiceColors(copyData,choices);choices.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetAutomationId(choices,L"workspace-manager-choice");
        retry.Content(box_value(copy(L"header",L"retry")));retry.HorizontalAlignment(HorizontalAlignment::Left);
        AutomationProperties::SetAutomationId(retry,L"workspace-manager-retry");
        listSurface.BorderThickness({1,1,1,1});listSurface.BorderBrush(copyData->line(L"settings",L"card_ring"));listSurface.CornerRadius({12,12,12,12});listSurface.Children().Append(list);
        for(auto const& metric:{nameLine.as<FrameworkElement>(),nameText.as<FrameworkElement>()}){
            metric.Opacity(0);metric.IsHitTestVisible(false);metric.MaxWidth(0);metric.HorizontalAlignment(HorizontalAlignment::Left);metric.VerticalAlignment(VerticalAlignment::Top);
            AutomationProperties::SetAccessibilityView(metric,Automation::Peers::AccessibilityView::Raw);listSurface.Children().Append(metric);
        }
        nameLine.IsTabStop(false);nameLine.MinWidth(0);nameLine.MinHeight(0);nameLine.BorderThickness({0,0,0,0});nameLine.Padding({0,0,0,0});
        nameLine.AcceptsReturn(false);nameLine.TextWrapping(TextWrapping::NoWrap);nameLine.Height(copyData->controlHeight());
        nameLine.LayoutUpdated([weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock();self&&self->showing&&self->nameAlignmentPending&&self->nameLine.IsLoaded()&&self->nameLine.ActualHeight()>0){
                self->nameAlignmentPending=false;try{self->alignNames();}catch(...){self->fail();}
            }
        });
        listSurface.SizeChanged([](auto const& sender,SizeChangedEventArgs const& e){auto size=e.NewSize();ClipSurface(sender.template as<UIElement>(),size.Width,size.Height,{12,12,12,12},false);});
        rowDrag=std::make_unique<WorkspaceRowDrag>(list,listSurface,
            [weak=weak_from_this()]{auto self=weak.lock();return self&&self->canReorder();},
            [weak=weak_from_this()](hstring id,std::optional<hstring> before){if(auto self=weak.lock())
                self->editSwitcher(O({{L"type",S(L"move")},{L"id",S(id)},{L"before",before?S(*before):JsonValue::CreateNullValue()}}));},
            [weak=weak_from_this()]{if(auto self=weak.lock()){++self->selectionSerial;self->restoreSelection();}});
        listing.Children().Append(search);listing.Children().Append(listSurface);listing.Children().Append(rowActions);
        promptBody.Children().Append(promptMessage);promptBody.Children().Append(name);promptBody.Children().Append(description);promptBody.Children().Append(choices);
        body.Children().Append(toolbarTabs);body.Children().Append(intro);body.Children().Append(split);
        ColumnDefinition sidebar;sidebar.Width({280,GridUnitType::Pixel});ColumnDefinition details;details.Width({1,GridUnitType::Star});split.ColumnDefinitions().Append(sidebar);split.ColumnDefinitions().Append(details);
        split.Children().Append(listing);detailsBody.Spacing(12);detailsBody.Margin({18,0,8,0});detailsScroll.Content(detailsBody);detailsScroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);mouseScrollbars(detailsScroll);Grid::SetColumn(detailsScroll,1);split.Children().Append(detailsScroll);
        AutomationProperties::SetAutomationId(detailsBody,L"workspace-manager-details");
        body.Children().Append(toolbarActions);body.Children().Append(promptBody);
        body.Children().Append(progress);body.Children().Append(error);body.Children().Append(retry);body.Children().Append(workspaceFooter);
        create.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->dispatch(self->page()==L"this_workspace"?form(L"new_toolbar",JsonValue::CreateNullValue()):form(L"new"));});
        retry.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->dispatch(O({{L"type",S(L"retry")}}));});
        search.TextChanged([weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock();self&&!self->updating){self->searchDraft=self->search.Text();self->dispatch(O({{L"type",S(L"search")},{L"query",S(self->search.Text())}}));}
        });
        name.TextChanging([weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock();self&&!self->updating){self->nameDraft=self->name.Text();self->validateName();}
        });
        description.TextChanging([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating)self->descriptionDraft=self->description.Text();});
        list.SelectionChanged([weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock();self&&!self->updating){
                auto item=self->list.SelectedItem().try_as<ListViewItem>();
                auto id=item?unbox_value<hstring>(item.Tag()):hstring{};
                auto serial=++self->selectionSerial;
                // ListView can select during its class PointerPressed handler.
                // Let the routed press establish gesture ownership first.
                self->list.DispatcherQueue().TryEnqueue([weak,serial,id]{
                    if(auto owner=weak.lock();owner&&owner->showing&&owner->selectionSerial==serial){
                        if(owner->rowDrag->FenceSelection())owner->restoreSelection();
                        else owner->select(id);
                    }
                });
            }
        });
        list.ItemClick([weak=weak_from_this()](auto&&,ItemClickEventArgs const& e){
            if(auto self=weak.lock();self&&!self->updating&&!self->rowDrag->SuppressClick()){
                for(auto const& [id,row]:self->rows)if(e.ClickedItem()==row.item||e.ClickedItem()==row.item.Content()){
                    self->select(hstring(id));return;
                }
            }
        });
        list.KeyDown([weak=weak_from_this()](auto&&,Input::KeyRoutedEventArgs const& e){
            if(e.Key()!=Windows::System::VirtualKey::Enter||composingKey(e))return;
            for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node)){
                if(node.try_as<Primitives::ButtonBase>())return;
                if(node.try_as<ListView>())break;
            }
            e.Handled(true);
            if(auto self=weak.lock();self&&!self->rowDrag->FenceSelection()){
                auto item=self->list.SelectedItem().try_as<ListViewItem>();
                if(item)self->select(unbox_value<hstring>(item.Tag()));
            }
        });
    }
    void alignNames(){
        using namespace winrt::Microsoft::UI::Text;
        auto font=[](auto const& target,auto const& source){
            target.FontFamily(source.FontFamily());target.FontSize(source.FontSize());target.FontWeight(source.FontWeight());
            target.FontStyle(source.FontStyle());target.FontStretch(source.FontStretch());target.CharacterSpacing(source.CharacterSpacing());
            target.Language(source.Language());target.FlowDirection(source.FlowDirection());target.IsTextScaleFactorEnabled(source.IsTextScaleFactorEnabled());
            target.UseLayoutRounding(source.UseLayoutRounding());
        };
        auto available=winrt::Windows::Foundation::Size{std::numeric_limits<float>::infinity(),std::numeric_limits<float>::infinity()};
        for(auto& [id,row]:rows){
            auto slot=row.entry.Parent().try_as<Grid>();if(!slot||!slot.IsLoaded()||slot.ActualWidth()<=0)continue;
            nameLine.MaxWidth(slot.ActualWidth());nameText.MaxWidth(slot.ActualWidth());
            font(nameLine,row.entry);font(nameText,row.title);nameText.LineHeight(row.title.LineHeight());
            nameText.LineStackingStrategy(row.title.LineStackingStrategy());nameText.TextLineBounds(row.title.TextLineBounds());
            nameText.Text(row.title.Text().empty()?hstring(L"0"):row.title.Text());nameText.Measure(available);auto title=nameText.DesiredSize();
            nameText.Arrange({0,0,title.Width,title.Height});
            auto character=nameText.ContentStart().GetCharacterRect(winrt::Microsoft::UI::Xaml::Documents::LogicalDirection::Forward);
            auto text=row.entry.Visibility()==Visibility::Visible?row.entry.Text():row.title.Text();
            nameLine.Height(copyData->controlHeight());nameLine.Document().SetText(TextSetOptions::None,text.empty()?hstring(L"0"):text);
            nameLine.Measure(available);auto native=nameLine.DesiredSize();nameLine.Arrange({0,0,native.Width,native.Height});
            auto range=nameLine.Document().GetRange(0,1);winrt::Windows::Foundation::Point top,baseline,bottom;
            auto options=PointOptions::Start|PointOptions::ClientCoordinates|PointOptions::AllowOffClient;
            range.GetPoint(HorizontalCharacterAlignment::Left,VerticalCharacterAlignment::Top,options,top);
            range.GetPoint(HorizontalCharacterAlignment::Left,VerticalCharacterAlignment::Baseline,options,baseline);
            range.GetPoint(HorizontalCharacterAlignment::Left,VerticalCharacterAlignment::Bottom,options,bottom);
            double lineHeight=bottom.Y-top.Y,ascent=baseline.Y-top.Y;
            double shift=character.Y+nameText.BaselineOffset()-title.Height/2-(ascent-lineHeight/2);
            if(title.Height<=0||lineHeight<=0||!std::isfinite(shift))throw hresult_error(E_FAIL);
            auto height=std::max({copyData->controlHeight(),double(title.Height),lineHeight+2*std::abs(shift)});
            auto padding=(height-lineHeight)/2,fieldPadding=copyData->panelMetric(L"field_padding");
            slot.MinHeight(height);row.entry.Height(height);row.entry.Padding({fieldPadding,padding+shift,fieldPadding,padding-shift});
        }
    }
    void fitRows() {
        double height=0;
        for(uint32_t index=0;index<std::min(7u,list.Items().Size());++index)
            height+=std::max(56.,list.Items().GetAt(index).as<ListViewItem>().ActualHeight());
        auto toolbars=page()==L"this_workspace"||page()==L"toolbar_library";
        list.Height(std::max(56.,std::min(height,double(root.Size().Height)-(toolbars?330.:230.))));
    }
    Row makeRow(hstring id) {
        Row row;row.item.Padding({0,0,0,0});row.item.MinHeight(56);row.item.BorderThickness({0,0,0,0});row.item.Tag(box_value(id));row.item.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        row.item.Template(Markup::XamlReader::Load(L"<ControlTemplate xmlns='http://schemas.microsoft.com/winfx/2006/xaml/presentation' TargetType='ListViewItem'><ListViewItemPresenter ContentTransitions='{TemplateBinding ContentTransitions}' ContentMargin='{TemplateBinding Padding}' HorizontalContentAlignment='{TemplateBinding HorizontalContentAlignment}' PointerOverBackground='{ThemeResource ListViewItemBackgroundPointerOver}' PressedBackground='{ThemeResource ListViewItemBackgroundPressed}' SelectedBackground='{ThemeResource ListViewItemBackgroundSelected}' SelectedPointerOverBackground='{ThemeResource ListViewItemBackgroundSelectedPointerOver}' SelectedPressedBackground='{ThemeResource ListViewItemBackgroundSelectedPressed}' SelectionIndicatorVisualEnabled='False' SelectedBorderThickness='0' CornerRadius='0'/></ControlTemplate>").as<ControlTemplate>());
        AutomationProperties::SetAutomationId(row.item,L"workspace-manager-row-"+id);
        row.item.SizeChanged([weak=weak_from_this()](auto&&,SizeChangedEventArgs const& e){
            if(e.NewSize().Height>0&&e.NewSize().Height!=e.PreviousSize().Height)
                if(auto self=weak.lock();self&&self->showing&&!self->stopping&&self->dialog)self->fitRows();
        });
        auto fieldPadding=copyData->panelMetric(L"field_padding");
        Grid content;content.MinHeight(56);content.Padding({12-fieldPadding,0,12,0});content.BorderBrush(copyData->line(L"settings",L"card_shade"));
        content.ColumnDefinitions().Append(ColumnDefinition());
        for(int i=0;i<3;++i){ColumnDefinition column;column.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(column);}
        StackPanel text;text.Spacing(2);text.Margin({0,4,4,4});text.VerticalAlignment(VerticalAlignment::Center);
        Grid nameSlot;nameSlot.MinHeight(copyData->controlHeight());
        AutomationProperties::SetAutomationId(nameSlot,L"workspace-name-slot-"+id);
        row.title=label(copyData,L"");row.subtitle=label(copyData,L"");
        row.title.TextTrimming(TextTrimming::CharacterEllipsis);row.subtitle.TextTrimming(TextTrimming::CharacterEllipsis);
        row.title.Margin({fieldPadding,0,fieldPadding,0});row.title.VerticalAlignment(VerticalAlignment::Center);row.subtitle.Margin({fieldPadding,0,fieldPadding,0});row.subtitle.FontSize(copyData->textSize()/1.2);row.subtitle.Opacity(.55);
        row.entry.Visibility(Visibility::Collapsed);row.entry.MinWidth(0);row.entry.MinHeight(0);row.entry.Height(copyData->controlHeight());row.entry.Padding({fieldPadding,0,fieldPadding,0});row.entry.Background(copyData->brush(L"input"));row.entry.BorderThickness({0,0,0,0});row.entry.CornerRadius(copyData->controlCorner());row.entry.VerticalContentAlignment(VerticalAlignment::Center);
        row.entry.FontSize(copyData->textSize());row.title.FontSize(copyData->textSize());
        auto nameFont=FontFamily(L"Segoe UI");row.title.FontFamily(nameFont);row.entry.FontFamily(nameFont);
        row.title.SizeChanged([weak=weak_from_this()](auto&&,SizeChangedEventArgs const& e){
            if(e.NewSize().Height>0)if(auto self=weak.lock())self->nameAlignmentPending=true;
        });
        for(auto state:{L"",L"PointerOver",L"Focused"})row.entry.Resources().Insert(box_value(hstring(L"TextControlBackground")+state),copyData->brush(L"input"));
        AutomationProperties::SetAutomationId(row.entry,L"workspace-manager-rename-"+id);AutomationProperties::SetName(row.entry,common(L"name"));
        row.entry.TextChanged([weak=weak_from_this(),id](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&self->editingId==id){self->submittedName=false;self->nameAlignmentPending=true;}});
        row.entry.KeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){if(!composingKey(e))if(auto self=weak.lock()){
            if(e.Key()==Windows::System::VirtualKey::Enter){e.Handled(true);self->finishName();}
            else if(e.Key()==Windows::System::VirtualKey::Escape){e.Handled(true);self->dispatch(O({{L"type",S(L"cancel")}}));}
        }});
        row.entry.LostFocus([weak=weak_from_this(),id](auto&&,auto&&){if(auto self=weak.lock();self&&self->editingId==id&&!self->updating)self->finishName();});
        row.title.DoubleTapped([weak=weak_from_this(),id](auto&&,DoubleTappedRoutedEventArgs const& e){if(auto self=weak.lock();self&&!self->rowDrag->SuppressClick()){
            auto found=self->rows.find(std::wstring(id));if(found!=self->rows.end()&&found->second.rename.IsEnabled()){e.Handled(true);self->dispatch(form(L"rename",S(id)));}
        }});
        row.item.KeyDown([weak=weak_from_this(),id](auto&&,KeyRoutedEventArgs const& e){if(e.Key()==Windows::System::VirtualKey::F2&&!composingKey(e))if(auto self=weak.lock()){
            auto found=self->rows.find(std::wstring(id));if(found!=self->rows.end()&&found->second.rename.IsEnabled()){e.Handled(true);self->dispatch(form(L"rename",S(id)));}
        }});
        nameSlot.Children().Append(row.title);nameSlot.Children().Append(row.entry);text.Children().Append(nameSlot);text.Children().Append(row.subtitle);content.Children().Append(text);
        auto options=row.menu;auto metadata=row.context;row.rename.Text(recovery(L"rename"));row.remove.Text(common(L"delete"));
        AutomationProperties::SetAutomationId(row.rename,L"workspace-manager-rename");
        AutomationProperties::SetAutomationId(row.remove,L"workspace-manager-delete");
        row.rename.Click([weak=weak_from_this(),id](auto&&,auto&&){if(auto self=weak.lock();self&&self->showing)self->dispatch(form(L"rename",S(id)));});
        row.remove.Click([weak=weak_from_this(),id](auto&&,auto&&){if(auto self=weak.lock();self&&self->showing)self->dispatch(form(L"delete",S(id)));});
        row.up.Text(copy(L"header",L"move_up"));row.down.Text(copy(L"header",L"move_down"));
        AutomationProperties::SetAutomationId(row.up,L"workspace-manager-move-up");
        AutomationProperties::SetAutomationId(row.down,L"workspace-manager-move-down");
        row.pin.RegisterPropertyChangedCallback(Primitives::ToggleButton::IsCheckedProperty(),[weak=weak_from_this(),id](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating){
            for(auto value:array(self->model,L"rows")){
                auto data=value.GetObject();if(str(data,L"id")!=id)continue;
                auto pin=object(data,L"pin");if(flag(pin,L"enabled"))self->dispatch(O({{L"type",S(L"edit_switcher")},{L"edit",object(pin,L"edit")}}));break;
            }
        }});
        row.up.Click([weak=weak_from_this(),id](auto&&,auto&&){if(auto self=weak.lock())self->moveWorkspace(id,false);});
        row.down.Click([weak=weak_from_this(),id](auto&&,auto&&){if(auto self=weak.lock())self->moveWorkspace(id,true);});
        options.Items().Append(row.up);options.Items().Append(row.down);
        metadata.Items().Append(row.rename);metadata.Items().Append(row.remove);
        row.contextUp.Text(copy(L"header",L"move_up"));row.contextDown.Text(copy(L"header",L"move_down"));
        AutomationProperties::SetAutomationId(row.contextUp,L"workspace-manager-move-up");AutomationProperties::SetAutomationId(row.contextDown,L"workspace-manager-move-down");
        row.contextUp.Click([weak=weak_from_this(),id](auto&&,auto&&){if(auto self=weak.lock())self->moveWorkspace(id,false);});
        row.contextDown.Click([weak=weak_from_this(),id](auto&&,auto&&){if(auto self=weak.lock())self->moveWorkspace(id,true);});
        metadata.Items().Append(row.contextUp);metadata.Items().Append(row.contextDown);
        row.pin.Content(row.pinIcon);row.pin.Width(34);row.pin.Height(34);row.pin.Padding({0,0,0,0});
        row.pin.Background(clear());row.pin.BorderBrush(clear());row.pin.BorderThickness({0,0,0,0});
        buttonFeedback(row.pin,copyData->brush(L"text"));
        row.pin.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(row.pin,2);content.Children().Append(row.pin);
        AutomationProperties::SetAutomationId(row.pin,L"workspace-manager-pin-"+id);
        row.current.Width(14);row.current.Height(14);row.current.Margin({0,0,8,0});
        row.current.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(row.current,1);content.Children().Append(row.current);
        AutomationProperties::SetAutomationId(row.current,L"workspace-manager-current-"+id);
        AutomationProperties::SetName(row.current,copy(L"header",L"current_workspace"));tooltip(row.current,copy(L"header",L"current_workspace"));
        row.grip.Width(16);row.grip.MinWidth(16);row.grip.Height(34);row.grip.Margin({4,0,0,0});row.grip.Padding({0,0,0,0});
        row.grip.Background(clear());row.grip.BorderThickness({0,0,0,0});
        buttonFeedback(row.grip,copyData->brush(L"text"));
        row.grip.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(row.grip,3);
        AutomationProperties::SetAutomationId(row.grip,L"workspace-manager-grip-"+id);
        AutomationProperties::SetHelpText(row.grip,copy(L"header",L"drag_to_reorder"));
        content.Children().Append(row.grip);
        row.swipeItems.Mode(SwipeMode::Reveal);row.swipeItems.Append(row.swipeDelete);row.swipe.RightItems(row.swipeItems);row.swipe.Content(content);row.item.Content(row.swipe);
        row.swipeDelete.Invoked([weak=weak_from_this(),id](auto&&,SwipeItemInvokedEventArgs const&){if(auto self=weak.lock())self->dispatch(form(L"delete",S(id)));});
        AutomationProperties::SetAutomationId(row.swipe,L"workspace-manager-swipe-"+id);
        AutomationProperties::SetAutomationId(row.swipeDelete,L"workspace-manager-swipe-delete-"+id);
        rowDrag->Attach(row.item,row.grip,nullptr,row.pin,row.menu,id,row.context);return row;
    }
    void applyRows() {
        auto values=array(model,L"rows");std::vector<std::wstring> next;
        auto saved=array(model,L"order");
        bool configurable=page()==L"workspaces",saving=flag(model,L"switcher_busy");
        for(auto value:values){
            auto data=value.GetObject();auto id=str(data,L"id");std::wstring key(id);next.push_back(key);
            auto found=rows.find(key);
            if(found==rows.end())found=rows.emplace(key,makeRow(id)).first;
            auto& row=found->second;row.swipe.Content().as<Grid>().BorderThickness({0,next.size()==1?0.:1.,0,0});auto label=str(data,L"title"),detail=str(data,L"subtitle");
            if(row.title.Text()!=label)nameAlignmentPending=true;row.title.Text(label);row.subtitle.Text(detail);row.subtitle.Visibility(detail.empty()?Visibility::Collapsed:Visibility::Visible);
            auto pin=object(data,L"pin");
            uint32_t position=saved.Size();for(uint32_t i=0;i<saved.Size();++i)if(saved.GetStringAt(i)==id)position=i;
            row.pin.Visibility(configurable?Visibility::Visible:Visibility::Collapsed);
            row.pin.IsChecked(flag(pin,L"selected"));row.pin.IsEnabled(flag(pin,L"enabled"));row.pin.Opacity(flag(pin,L"enabled")?1:.5);
            AutomationProperties::SetName(row.pin,str(pin,L"label"));tooltip(row.pin,str(pin,L"label"));

            row.current.Visibility(configurable&&flag(data,L"current")?Visibility::Visible:Visibility::Collapsed);
            auto theme=str(object(snapshot,L"state"),L"theme",L"dark"),glyph=str(pin,L"icon",L"pin");
            if(theme!=row.theme||glyph!=row.pinGlyph){row.pinGlyph=glyph;setIcon(row.pinIcon,glyph,theme,16);}
            if(theme!=row.theme){row.theme=theme;row.current.Source(icon(L"check",theme,14).Source());row.grip.Content(panelGrip(theme));}
            row.grip.Visibility(configurable?Visibility::Visible:Visibility::Collapsed);row.grip.IsEnabled(canReorder());
            if(row.captionTitle!=label){row.captionTitle=label;AutomationProperties::SetName(row.grip,caption(O({{L"type",S(L"reorder")},{L"title",S(label)}})));}
            row.up.IsEnabled(!saving&&position>0&&position<saved.Size());
            row.down.IsEnabled(!saving&&position+1<saved.Size());
            for(auto item:{row.up.as<UIElement>(),row.down.as<UIElement>()})
                item.Visibility(configurable?Visibility::Visible:Visibility::Collapsed);
            auto option=[&](hstring const& kind){for(auto value:array(data,L"options")){
                auto button=value.GetObject();if(str(object(button,L"action"),L"type")==kind)return button;
            }return J{};};
            auto renameOption=option(L"rename"),deleteOption=option(L"delete");
            bool rename=configurable&&renameOption.Size(),remove=configurable&&deleteOption.Size();
            row.rename.Text(str(renameOption,L"label"));row.remove.Text(str(deleteOption,L"label"));
            row.rename.Visibility(rename?Visibility::Visible:Visibility::Collapsed);
            row.remove.Visibility(remove?Visibility::Visible:Visibility::Collapsed);
            row.rename.IsEnabled(!saving&&rename&&flag(renameOption,L"enabled"));row.remove.IsEnabled(!saving&&remove&&flag(deleteOption,L"enabled"));
            row.contextUp.IsEnabled(row.up.IsEnabled());row.contextDown.IsEnabled(row.down.IsEnabled());
            row.swipeDelete.Text(str(deleteOption,L"label"));
            if(remove&&flag(deleteOption,L"enabled")){if(!row.swipe.RightItems())row.swipe.RightItems(row.swipeItems);}else row.swipe.RightItems(nullptr);
            AutomationProperties::SetName(row.item,label+(detail.empty()?L"":L" · "+detail));

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
        auto details=object(model,L"prompt");bool inlineName=inlineRename(),naming=details.Size()!=0&&!inlineName,busy=flag(model,L"busy");
        auto page=this->page();bool toolbars=page==L"this_workspace"||page==L"toolbar_library";
        listing.Spacing(page==L"workspaces"?6:4);
        if(page!=toolbarPage){toolbarPage=page;searchDraft.reset();}
        toolbarTabs.Visibility(toolbars&&!naming?Visibility::Visible:Visibility::Collapsed);
        thisWorkspace.IsChecked(page==L"this_workspace");savedToolbars.IsChecked(page==L"toolbar_library");
        thisWorkspace.IsEnabled(!busy);savedToolbars.IsEnabled(!busy);
        auto actions=toolbars?array(object(model,L"details"),L"actions"):A{};auto actionKey=actions.Stringify();
        if(actionKey!=toolbarActionsKey){
            toolbarActionsKey=actionKey;toolbarMenu.Items().Clear();
            for(auto value:actions){
                auto data=value.GetObject();if(flag(data,L"primary"))continue;
                auto action=object(data,L"action");MenuFlyoutItem item;item.Text(str(data,L"label"));item.IsEnabled(flag(data,L"enabled"));
                AutomationProperties::SetAutomationId(item,L"workspace-toolbar-"+str(action,L"type"));
                item.Click([weak=weak_from_this(),action](auto&&,auto&&){if(auto self=weak.lock();self&&self->showing)self->dispatch(O({{L"type",S(L"action")},{L"action",action}}));});
                toolbarMenu.Items().Append(item);
            }
        }
        toolbarActions.Visibility(toolbars&&!naming&&toolbarMenu.Items().Size()?Visibility::Visible:Visibility::Collapsed);
        toolbarActions.IsEnabled(!busy&&!flag(model,L"loading"));
        auto key=naming?object(model,L"prompt_action").Stringify():L"";
        if(key!=promptKey){promptKey=key;nameDraft.reset();descriptionDraft.reset();validationName=L"";++validationSerial;validationQueued=true;nameValid=false;validationError=L"";choicesKey=L"";}
        title.Text(naming?str(details,L"title"):str(model,L"title"));
        auto theme=str(object(snapshot,L"state"),L"theme",L"dark");
        dialogSurface(copyData,dialog);
        body.Width(std::max(200.,std::min(472.,double(root.Size().Width)-80.)));
        split.ColumnDefinitions().GetAt(0).Width({std::max(220.,std::min(280.,body.Width()*.54)),GridUnitType::Pixel});
        heading.Width(body.Width());
        intro.Text(str(model,L"intro"));intro.Visibility(page==L"workspaces"||naming||intro.Text().empty()?Visibility::Collapsed:Visibility::Visible);
        create.Visibility(!naming&&(page==L"workspaces"||page==L"this_workspace")?Visibility::Visible:Visibility::Collapsed);
        create.IsEnabled((!busy||inlineName)&&!flag(model,L"loading"));AutomationProperties::SetName(create,page==L"this_workspace"?copy(L"header",L"new_toolbar"):copy(L"header",L"new_workspace"));
        create.Content(icon(L"plus",theme,16));removeSelected.Content(icon(L"delete",theme,16));
        bool deletable=false;for(auto value:array(model,L"rows")){auto row=value.GetObject();if(str(row,L"id")==str(model,L"selected"))for(auto option:array(row,L"options")){auto action=option.GetObject();if(str(object(action,L"action"),L"type")==L"delete")deletable=flag(action,L"enabled");}}
        removeSelected.Visibility(page==L"workspaces"?Visibility::Visible:Visibility::Collapsed);removeSelected.IsEnabled((!busy||inlineName)&&deletable);
        removeSelected.Opacity(removeSelected.IsEnabled()?1:.36);create.Opacity(create.IsEnabled()?1:.36);
        rowActions.Visibility(!naming&&(page==L"workspaces"||page==L"this_workspace")?Visibility::Visible:Visibility::Collapsed);
        split.Visibility(naming||page==L"prompt"?Visibility::Collapsed:Visibility::Visible);
        promptBody.Visibility(naming?Visibility::Visible:Visibility::Collapsed);
        list.IsEnabled(!busy||inlineName);search.IsEnabled(!busy);
        search.Visibility(toolbars?Visibility::Visible:Visibility::Collapsed);
        sync(search,str(model,L"query"),searchDraft);
        if(!naming)applyRows();
        fitRows();
        auto renameId=inlineName?str(object(model,L"prompt_action"),L"value"):hstring{};
        for(auto& [id,row]:rows){bool editing=hstring(id)==renameId;row.title.Visibility(editing?Visibility::Collapsed:Visibility::Visible);row.entry.Visibility(editing?Visibility::Visible:Visibility::Collapsed);
            if(editing&&editingId!=renameId){row.entry.Text(str(details,L"name"));auto entry=row.entry;entry.DispatcherQueue().TryEnqueue([entry]{entry.Focus(FocusState::Keyboard);entry.SelectAll();});}
        }
        if(editingId!=renameId){editingId=renameId;submittedName=false;}
        if(inlineName&&!busy&&!str(model,L"error").empty())submittedName=false;
        if(naming){
            promptMessage.Text(str(details,L"message"));promptMessage.Visibility(promptMessage.Text().empty()?Visibility::Collapsed:Visibility::Visible);
            bool named=details.HasKey(L"name")&&details.GetNamedValue(L"name").ValueType()==Windows::Data::Json::JsonValueType::String;
            name.Visibility(named?Visibility::Visible:Visibility::Collapsed);name.IsEnabled(!busy);
            sync(name,str(details,L"name"),nameDraft);if(named)validateName();
            bool described=details.GetNamedValue(L"description",JsonValue::CreateNullValue()).ValueType()==JsonValueType::String;
            description.Visibility(described?Visibility::Visible:Visibility::Collapsed);description.IsEnabled(!busy);
            sync(description,str(details,L"description"),descriptionDraft);
            auto options=array(details,L"choices");choices.Visibility(options.Size()?Visibility::Visible:Visibility::Collapsed);
            choices.Header(box_value(str(details,L"choice_label")));choices.IsEnabled(!busy);
            auto signature=options.Stringify();
            if(signature!=choicesKey){
                choicesKey=signature;choices.Items().Clear();
                auto chosen=str(details,L"selected");
                for(auto value:options){
                    auto option=value.GetObject();ComboBoxItem item;item.Tag(box_value(str(option,L"id")));item.Content(box_value(str(option,L"label")));
                    choices.Items().Append(item);if(str(option,L"id")==chosen)choices.SelectedItem(item);
                }
            }
        }
        auto message=validationError;if(message.empty())message=str(model,L"error");if(message.empty())message=str(model,L"switcher_error");error.Text(message);error.Visibility(message.empty()?Visibility::Collapsed:Visibility::Visible);
        error.Foreground(copyData->brush(L"error"));
        progress.Text(busy?recovery(L"saving"):recovery(L"loading"));progress.Visibility((busy&&!inlineName)||flag(model,L"loading")?Visibility::Visible:Visibility::Collapsed);
        retry.Visibility(flag(model,L"retry")?Visibility::Visible:Visibility::Collapsed);retry.IsEnabled(!busy);
        bool workspacePage=page==L"workspaces"&&!naming;
        auto palette=object(object(snapshot,L"state"),L"palette");
        listSurface.Background(copyData->brush(L"card"));
        for(auto const& [id,row]:rows){
            for(auto state:{L"Selected",L"SelectedPointerOver",L"SelectedPressed"})row.item.Resources().Insert(box_value(hstring(L"ListViewItemBackground")+state),selected(copyData));
            row.item.Resources().Insert(box_value(L"ListViewItemBackgroundPointerOver"),copyData->tint(L"text",8));row.item.Resources().Insert(box_value(L"ListViewItemBackgroundPressed"),copyData->tint(L"text",20));
            row.title.Foreground(copyData->brush(L"text"));row.subtitle.Foreground(copyData->brush(L"text"));
        }
        auto detailSpec=object(model,L"details");auto detailKey=detailSpec.Stringify()+to_hstring(busy)+theme;
        if(detailKey!=detailsKey){detailsKey=detailKey;detailsBody.Children().Clear();
            auto title=label(copyData,str(detailSpec,L"title"),true);title.TextWrapping(TextWrapping::Wrap);detailsBody.Children().Append(title);
            auto description=label(copyData,str(detailSpec,L"description"));description.TextWrapping(TextWrapping::Wrap);description.IsTextSelectionEnabled(true);detailsBody.Children().Append(description);
            for(auto value:array(detailSpec,L"actions")){auto spec=value.GetObject();auto action=object(spec,L"action");auto control=button(copyData,str(spec,L"label"),[weak=weak_from_this(),action]{if(auto self=weak.lock())self->dispatch(O({{L"type",S(L"action")},{L"action",action}}));});control.Padding({10,5,10,5});control.MinHeight(34);control.IsEnabled(flag(spec,L"enabled")&&!busy);detailsBody.Children().Append(control);}
        }
        workspaceFooter.Visibility(workspacePage?Visibility::Visible:Visibility::Collapsed);
        workspaceCancel.Content(box_value(common(L"cancel")));workspaceApply.Content(box_value(str(model,L"primary")));
        workspaceCancel.Background(copyData->tint(L"text",26));
        workspaceApply.Background(fill(color(str(palette,L"accent"))));workspaceApply.Foreground(fill(color(str(palette,L"accent_foreground"))));
        workspaceApply.IsEnabled((!busy||inlineName)&&flag(model,L"enabled")&&!flag(model,L"loading"));workspaceApply.Opacity(workspaceApply.IsEnabled()?1:.5);
        dialog.PrimaryButtonText(workspacePage?L"":naming?str(details,L"confirm"):str(model,L"primary"));
        dialog.IsPrimaryButtonEnabled((!busy||inlineName)&&!flag(model,L"loading")&&(naming?(name.Visibility()!=Visibility::Visible||nameValid):flag(model,L"enabled")));
        dialog.CloseButtonText(workspacePage?L"":common(L"cancel"));
        dialog.DefaultButton(naming?ContentDialogButton::Primary:ContentDialogButton::None);
        cancelPending=false;
    }
    fire_and_forget show() {
        auto lifetime=shared_from_this();
        if(showing||stopping||blocked||!open())co_return;
        showing=true;changed();
        try{
            dialog=ContentDialog();dialog.XamlRoot(root);dialog.Title(heading);dialog.Content(body);
            auto tag=str(object(catalog,L"bootstrap"),L"active_tag");if(!tag.empty())dialog.Language(tag);
            AutomationProperties::SetAutomationId(dialog,L"workspace-manager");
            dialog.KeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){
                if(e.Key()==Windows::System::VirtualKey::Escape&&!composingKey(e))if(auto self=weak.lock()){e.Handled(true);if(!self->rowDrag->Escape())self->cancel();}
            });
            dialog.PrimaryButtonClick([weak=weak_from_this()](auto&&,ContentDialogButtonClickEventArgs const& e){
                e.Cancel(true);if(auto self=weak.lock();self&&!focusedTextComposing(self->root)){
                    if(self->prompt()&&!self->inlineRename()){
                        auto selected=self->choices.SelectedItem().try_as<ComboBoxItem>();
                        auto option=selected?S(unbox_value<hstring>(selected.Tag())):Windows::Data::Json::JsonValue::CreateNullValue();
                        self->dispatch(O({{L"type",S(L"submit")},{L"name",S(self->name.Text())},{L"description",self->description.Visibility()==Visibility::Visible?S(self->description.Text()):JsonValue::CreateNullValue()},{L"choice",option}}));
                    }else self->dispatch(O({{L"type",S(L"confirm")}}));
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
        if(!stopping&&!programmatic)dispatch(O({{L"type",S(L"dismiss")}}));
        list.Items().Clear();rows.clear();order.clear();promptKey=L"";choicesKey=L"";nameDraft.reset();searchDraft.reset();selectionDraft.reset();
        toolbarActionsKey=L"";toolbarPage=L"";toolbarMenu.Items().Clear();
        showing=false;programmatic=false;changed();
    }
    hstring focusOwner(hstring const& owner) {
        struct Target {std::wstring property;HWND window=nullptr;} target{L"CapyCanvas.WorkspaceOwner."+std::wstring(owner)};
        EnumWindows([](HWND window,LPARAM context)->BOOL{
            auto& target=*reinterpret_cast<Target*>(context);
            if(GetPropW(window,target.property.c_str())&&IsWindowVisible(window)){target.window=window;return FALSE;}
            return TRUE;
        },reinterpret_cast<LPARAM>(&target));
        if(!target.window)return recovery(L"workspace_missing");
        if(IsIconic(target.window))ShowWindowAsync(target.window,SW_RESTORE);
        if(!SetForegroundWindow(target.window)&&GetForegroundWindow()!=target.window)return recovery(L"workspace_activation_failed");
        return L"";
    }
    void apply(J const& value,bool unavailable) {
        copyData->model=value;copyData->state=object(value,L"state");copyData->refreshPalette();
        if(copyData->adoptLocalization(value)){nameAlignmentPending=true;
            catalog=copyData->catalog;localization=copyData->localization;
            thisWorkspace.Content(box_value(copy(L"header",L"this_workspace")));savedToolbars.Content(box_value(copy(L"header",L"saved_toolbars")));
            toolbarActions.Content(box_value(copy(L"header",L"toolbar_actions")));retry.Content(box_value(copy(L"header",L"retry")));
            progress.Text(recovery(L"loading"));search.PlaceholderText(copy(L"header",L"search"));
            description.Header(box_value(copy(L"header",L"description")));description.Language(copyData->language());
            validationName=L"";++validationSerial;validationQueued=true;
            name.Header(box_value(common(L"name")));name.Language(copyData->language());search.Language(copyData->language());
            if(dialog)dialog.Language(copyData->language());
            for(auto& [id,row]:rows){row.captionTitle=L"";
                row.rename.Text(recovery(L"rename"));row.remove.Text(common(L"delete"));
                row.up.Text(copy(L"header",L"move_up"));row.down.Text(copy(L"header",L"move_down"));row.contextUp.Text(copy(L"header",L"move_up"));row.contextDown.Text(copy(L"header",L"move_down"));row.entry.Language(copyData->language());
                AutomationProperties::SetName(row.current,copy(L"header",L"current_workspace"));tooltip(row.current,copy(L"header",L"current_workspace"));
                AutomationProperties::SetHelpText(row.grip,copy(L"header",L"drag_to_reorder"));
            }
        }
        snapshot=value;model=object(snapshot,L"windows_workspace");blocked=unavailable;
        if(stopping)return;
        auto focus=object(model,L"focus_window");
        if(focus.Size()){
            // ContentDialog restores focus to its source while closing. Activate
            // another window only after ShowAsync has fully released the dialog.
            if(showing){programmatic=true;if(dialog)dialog.Hide();return;}
            auto key=str(focus,L"owner")+L"|"+str(focus,L"id");
            if(key!=focusSent){
                focusSent=key;auto focusError=focusOwner(str(focus,L"owner"));
                if(!focusError.empty())dispatch(O({{L"type",S(L"focus_failed")},{L"error",S(focusError)}}));
            }
            return;
        }
        focusSent=L"";
        if(showing){
            if(!open()){programmatic=true;if(dialog)dialog.Hide();}
            else update();
        }else if(open()&&!blocked)show();
    }
};
WorkspaceManagerView::WorkspaceManagerView(Dispatch send,Json catalog,NativeLocalization localization,XamlRoot root,std::function<void()> changed,PreviewTransport query):impl(std::make_shared<Impl>()){
    impl->query=std::move(query);impl->copyData->catalog=catalog;impl->copyData->localization=localization;impl->send=std::move(send);impl->catalog=catalog;impl->localization=std::move(localization);impl->root=root;impl->changed=std::move(changed);impl->init();
}
WorkspaceManagerView::~WorkspaceManagerView()=default;
void WorkspaceManagerView::Apply(Json const& snapshot,bool blocked){impl->apply(snapshot,blocked);}
bool WorkspaceManagerView::IsOpen()const{return impl->showing;}
void WorkspaceManagerView::CancelAll(){impl->rowDrag->Cancel();if(impl->showing)impl->dispatch(CapyUi::O({{L"type",CapyUi::S(L"dismiss")}}));}
void WorkspaceManagerView::Hide(){impl->rowDrag->Cancel();impl->stopping=true;impl->programmatic=true;if(impl->dialog)impl->dialog.Hide();}
