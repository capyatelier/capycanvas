#include "pch.h"
#include "EffectControls.h"
#include <chrono>
using namespace CapyEffects;
namespace {
struct FiltersView : std::enable_shared_from_this<FiltersView> {
    std::shared_ptr<WorkspaceData> data;
    Grid root,header;
    ComboBox category;
    TextBox search;
    StackPanel rows;
    ScrollView list;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    struct PreviewRow {hstring id;Button button{nullptr};Image image{nullptr};};
    std::vector<PreviewRow> previews;
    ~FiltersView(){if(timer)timer.Stop();}
    void preview(){
        if(!root.IsLoaded()||!root.XamlRoot()||!root.XamlRoot().IsHostVisible()||list.ActualHeight()<=0)return;
        double density=root.XamlRoot().RasterizationScale();
        int width=std::clamp(int(std::round((list.ActualWidth()-12)*density)),80,512);
        int height=std::clamp(int(std::round(40*density)),1,128);
        auto file=object(data->state,L"document_file");
        auto context=O({{L"epoch",file.GetNamedValue(L"epoch")},{L"revision",file.GetNamedValue(L"revision")},
            {L"layer",object(data->state,L"layer_properties").GetNamedValue(L"layer")},
            {L"catalog",data->state.GetNamedValue(L"filter_catalog_revision")}}).Stringify();
        std::vector<hstring> visible;
        for(auto const& row:previews){
            auto rect=row.button.TransformToVisual(list).TransformBounds({0,0,float(row.button.ActualWidth()),float(row.button.ActualHeight())});
            if(rect.Y+rect.Height>0&&rect.Y<list.ActualHeight())visible.push_back(row.id);
            auto source=FilterPreviewSource(data->previews,context,width,height,row.id);
            if(row.image.Source()!=source)row.image.Source(source);
            AutomationProperties::SetItemStatus(row.image,source?L"Ready":L"Pending");
        }
        RefreshFilterPreviews(data->previews,context,width,height,visible);
    }
    hstring catalogKey,listKey;
    void send(J action){if(!data->updating)data->dispatch(O({{L"type",S(L"filter_picker")},{L"action",action}}));}
    void init(){
        auto weak=weak_from_this();root.Padding({6,6,6,6});root.RowSpacing(6);
        AutomationProperties::SetAutomationId(root,L"filter-picker");
        RowDefinition a;a.Height({1,GridUnitType::Auto});root.RowDefinitions().Append(a);
        RowDefinition b;b.Height({1,GridUnitType::Star});root.RowDefinitions().Append(b);
        ColumnDefinition content;content.Width({1,GridUnitType::Star});header.ColumnDefinitions().Append(content);
        ColumnDefinition action;action.Width({32,GridUnitType::Pixel});header.ColumnDefinitions().Append(action);header.ColumnSpacing(6);
        category.MinWidth(0);category.MinHeight(32);category.FontSize(data->textSize());category.Background(data->brush(L"input"));
        category.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetName(category,L"Filter category");AutomationProperties::SetAutomationId(category,L"filter-category");
        category.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating){
            auto categories=array(self->data->state,L"filter_categories");int index=self->category.SelectedIndex();
            if(index>=0&&uint32_t(index)<categories.Size())self->send(O({{L"op",S(L"category")},
                {L"category",categories.GetObjectAt(index).GetNamedValue(L"id")}}));
        }});
        header.Children().Append(category);
        search.MinWidth(0);search.MinHeight(32);search.FontSize(data->textSize());search.MaxLength(120);
        search.Background(data->brush(L"input"));search.Padding({6,4,6,4});search.BorderThickness({0});
        AutomationProperties::SetAutomationId(search,L"filter-search");header.Children().Append(search);
        search.TextChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->send(O({{L"op",S(L"search")},{L"query",S(self->search.Text())}}));});
        search.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(e.Key()==Windows::System::VirtualKey::Escape){
            if(auto self=weak.lock())self->send(O({{L"op",S(L"toggle_search")}}));e.Handled(true);
        }});
        auto toggle=button(data,L"Search filters",[weak]{if(auto self=weak.lock())self->send(O({{L"op",S(L"toggle_search")}}));});
        toggle.Content(icon(L"search",data->theme()));toggle.Height(32);Grid::SetColumn(toggle,1);
        AutomationProperties::SetAutomationId(toggle,L"filter-search-toggle");header.Children().Append(toggle);root.Children().Append(header);
        list.Content(rows);list.HorizontalScrollMode(ScrollingScrollMode::Disabled);
        list.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);list.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);
        AutomationProperties::SetAutomationId(list,L"filter-list");rows.Spacing(2);Grid::SetRow(list,1);root.Children().Append(list);
        timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(200));
        timer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->preview();});
        root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->timer.Start();self->preview();}});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->timer.Stop();});
    }
    void refresh(){
        Updating updating(data);auto picker=object(data->state,L"filter_picker");auto categories=array(data->state,L"filter_categories");
        auto key=categories.Stringify();
        if(key!=catalogKey){catalogKey=key;category.Items().Clear();for(auto value:categories)category.Items().Append(box_value(str(value.GetObject(),L"label")));}
        int selected=0;for(uint32_t i=0;i<categories.Size();i++)if(str(categories.GetObjectAt(i),L"id")==str(picker,L"category"))selected=i;
        category.SelectedIndex(selected);
        bool open=picker.GetNamedValue(L"search",JsonValue::CreateNullValue()).ValueType()==JsonValueType::String;
        bool wasOpen=search.Visibility()==Visibility::Visible;
        category.Visibility(open?Visibility::Collapsed:Visibility::Visible);search.Visibility(open?Visibility::Visible:Visibility::Collapsed);
        search.PlaceholderText(str(picker,L"search_label"));AutomationProperties::SetName(search,str(picker,L"search_label"));
        auto query=str(picker,L"search");if(search.Text()!=query)search.Text(query);
        if(open&&!wasOpen)search.Focus(FocusState::Programmatic);
        auto choices=array(data->state,L"adjustments");auto next=choices.Stringify();if(next==listKey)return;listKey=next;
        rows.Children().Clear();previews.clear();hstring section;
        for(auto value:choices){
            auto choice=value.GetObject();
            if(section!=str(choice,L"category")){section=str(choice,L"category");auto heading=label(data,str(choice,L"category_label"),true);
                heading.Opacity(.55);heading.Margin({8,8,8,4});rows.Children().Append(heading);}
            auto pick=button(data,str(choice,L"label"),[data=data,action=object(choice,L"action")]{data->dispatch(action);});
            pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            pick.Padding({6,3,6,3});
            StackPanel content;Image preview;preview.Height(40);preview.Stretch(Stretch::Fill);preview.IsHitTestVisible(false);
            AutomationProperties::SetName(preview,str(choice,L"label")+L" preview");
            AutomationProperties::SetAutomationId(preview,L"filter-preview-"+str(choice,L"id"));
            content.Children().Append(preview);
            Grid caption;ColumnDefinition mark;mark.Width({1,GridUnitType::Auto});caption.ColumnDefinitions().Append(mark);
            ColumnDefinition name;name.Width({1,GridUnitType::Star});caption.ColumnDefinitions().Append(name);
            if(flag(choice,L"animated")){auto animation=icon(L"animation",data->theme(),12);animation.Opacity(.55);animation.Margin({0,0,4,0});caption.Children().Append(animation);}
            auto text=label(data,str(choice,L"label"));text.TextTrimming(TextTrimming::CharacterEllipsis);text.TextAlignment(TextAlignment::Right);
            Grid::SetColumn(text,1);caption.Children().Append(text);content.Children().Append(caption);pick.Content(content);
            previews.push_back({str(choice,L"id"),pick,preview});
            AutomationProperties::SetAutomationId(pick,L"filter-"+str(choice,L"id"));
            ToolTipService::SetToolTip(pick,box_value(str(choice,L"tooltip")));rows.Children().Append(pick);
        }
        if(!choices.Size()){auto empty=label(data,str(picker,L"empty_label"));empty.Margin({8,8,8,8});empty.Opacity(.55);rows.Children().Append(empty);}
    }
};
}
FrameworkElement FiltersPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<FiltersView>();view->data=data;view->init();bindings.emplace_back([view]{view->refresh();});return view->root;
}
