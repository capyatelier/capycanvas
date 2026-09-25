#include "pch.h"
#include "EffectControls.h"
#include <chrono>
#include <optional>
using namespace CapyEffects;
namespace {
struct FiltersView : std::enable_shared_from_this<FiltersView> {
    std::shared_ptr<WorkspaceData> data;
    Grid root,header;
    bool split=false;
    ComboBox category;
    Image categoryGlyph;
    hstring categoryGlyphKey;
    TextBox search;
    StackPanel rows;
    ScrollView list;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    struct PreviewRow {hstring id;Button button{nullptr};Image image{nullptr};};
    std::vector<PreviewRow> previews;
    ~FiltersView(){if(timer)timer.Stop();RemoveFilterPreviewView(data->previews,reinterpret_cast<uint64_t>(this));}
    void preview(){
        if(!root.IsLoaded()||!root.XamlRoot()||!root.XamlRoot().IsHostVisible()||list.ActualHeight()<=0){RemoveFilterPreviewView(data->previews,reinterpret_cast<uint64_t>(this));return;}
        double density=root.XamlRoot().RasterizationScale();
        int width=std::clamp(int(std::round((list.ActualWidth()-12)*density)),80,512);
        int height=std::clamp(int(std::round(40*density)),1,128);
        auto file=object(data->state,L"document_file");
        auto context=file.GetNamedValue(L"epoch").Stringify();
        std::vector<hstring> visible;
        for(auto const& row:previews){
            auto rect=row.button.TransformToVisual(list).TransformBounds({0,0,float(row.button.ActualWidth()),float(row.button.ActualHeight())});
            if(rect.Y+rect.Height>0&&rect.Y<list.ActualHeight())visible.push_back(row.id);
            auto source=FilterPreviewSource(data->previews,context,row.id);
            if(row.image.Source()!=source)row.image.Source(source);
            AutomationProperties::SetItemStatus(row.image,source?L"Ready":L"Pending");
        }
        RefreshFilterPreviews(data->previews,reinterpret_cast<uint64_t>(this),context,width,height,visible);
    }
    hstring catalogKey,listKey;
    std::optional<hstring> searchDraft;
    void send(J action){if(!data->updating)data->dispatch(O({{L"type",S(L"filter_picker")},{L"action",action}}));}
    void init(){
        auto weak=weak_from_this();root.Padding({6,6,6,6});root.RowSpacing(split?0:6);
        header.Visibility(split?Visibility::Collapsed:Visibility::Visible);
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
        category.Margin({22,0,0,0});header.Children().Append(category);
        categoryGlyph.Width(16);categoryGlyph.Height(16);categoryGlyph.IsHitTestVisible(false);
        categoryGlyph.HorizontalAlignment(HorizontalAlignment::Left);header.Children().Append(categoryGlyph);
        search.MinWidth(0);search.MinHeight(32);search.FontSize(data->textSize());search.MaxLength(120);
        search.Background(data->brush(L"input"));search.Padding({6,4,6,4});search.BorderThickness({0});
        AutomationProperties::SetAutomationId(search,L"filter-search");header.Children().Append(search);
        // TextChanging is synchronous, so programmatic updates stay inside
        // Updating. Keep typed text until its own model acknowledgement arrives.
        search.TextChanging([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating){
            self->searchDraft=self->search.Text();
            self->send(O({{L"op",S(L"search")},{L"query",S(*self->searchDraft)}}));
        }});
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
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->timer.Stop();RemoveFilterPreviewView(self->data->previews,reinterpret_cast<uint64_t>(self.get()));}});
    }
    void refresh(){
        Updating updating(data);auto picker=object(data->state,L"filter_picker");auto categories=array(data->state,L"filter_categories");
        auto key=categories.Stringify();
        if(key!=catalogKey){catalogKey=key;category.Items().Clear();for(auto value:categories)category.Items().Append(box_value(str(value.GetObject(),L"label")));}
        int categorySelected=0;for(uint32_t i=0;i<categories.Size();i++)if(str(categories.GetObjectAt(i),L"id")==str(picker,L"category"))categorySelected=i;
        category.SelectedIndex(categorySelected);
        auto glyph=categorySelected>=0&&uint32_t(categorySelected)<categories.Size()?str(categories.GetObjectAt(categorySelected),L"icon",L"adjustments"):hstring(L"adjustments");
        if(glyph!=categoryGlyphKey){categoryGlyphKey=glyph;categoryGlyph.Source(icon(glyph,data->theme()).Source());}
        bool open=picker.GetNamedValue(L"search",JsonValue::CreateNullValue()).ValueType()==JsonValueType::String;
        bool wasOpen=search.Visibility()==Visibility::Visible;
        categoryGlyph.Visibility(open?Visibility::Collapsed:Visibility::Visible);
        category.Visibility(open?Visibility::Collapsed:Visibility::Visible);search.Visibility(open?Visibility::Visible:Visibility::Collapsed);
        search.PlaceholderText(str(picker,L"search_label"));AutomationProperties::SetName(search,str(picker,L"search_label"));
        auto query=str(picker,L"search");
        if(!open||(searchDraft&&query==*searchDraft))searchDraft.reset();
        if(!searchDraft&&search.Text()!=query)search.Text(query);
        if(open&&!wasOpen)search.Focus(FocusState::Programmatic);
        auto markSelected=[&]{for(auto const& row:previews){bool active=row.id==str(picker,L"selected");row.button.Background(active?selected(data):clear());AutomationProperties::SetItemStatus(row.button,active?L"Selected":L"");}};
        auto choices=array(data->state,L"adjustments");auto next=choices.Stringify();if(next==listKey){markSelected();return;}listKey=next;
        rows.Children().Clear();previews.clear();hstring section;
        for(auto value:choices){
            auto choice=value.GetObject();
            if(!split&&section!=str(choice,L"category")){
                section=str(choice,L"category");StackPanel heading;heading.Orientation(Orientation::Horizontal);heading.Spacing(6);
                heading.Children().Append(icon(str(choice,L"category_icon",L"adjustments"),data->theme()));
                heading.Children().Append(label(data,str(choice,L"category_label"),true));
                heading.Opacity(.55);heading.Margin({8,8,8,4});rows.Children().Append(heading);
            }
            auto pick=button(data,str(choice,L"label"),[data=data,action=object(choice,L"action")]{data->dispatch(action);});
            pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            pick.Padding({6,3,6,3});
            StackPanel content;Image preview;preview.Height(40);preview.Stretch(Stretch::Fill);preview.IsHitTestVisible(false);
            AutomationProperties::SetName(preview,str(choice,L"label")+L" preview");
            AutomationProperties::SetAutomationId(preview,L"filter-preview-"+str(choice,L"id"));
            content.Children().Append(preview);
            Grid caption;caption.HorizontalAlignment(HorizontalAlignment::Right);ColumnDefinition mark;mark.Width({1,GridUnitType::Auto});caption.ColumnDefinitions().Append(mark);
            ColumnDefinition name;name.Width({1,GridUnitType::Star});caption.ColumnDefinitions().Append(name);
            StackPanel marks;marks.Orientation(Orientation::Horizontal);marks.Spacing(4);marks.Margin({0,0,4,0});
            if(flag(choice,L"animated")){auto animation=icon(L"animation",data->theme(),12);animation.Opacity(.55);marks.Children().Append(animation);}
            marks.Children().Append(icon(str(choice,L"icon",L"adjustments"),data->theme()));caption.Children().Append(marks);
            auto text=label(data,str(choice,L"label"));text.TextTrimming(TextTrimming::CharacterEllipsis);text.TextAlignment(TextAlignment::Right);
            Grid::SetColumn(text,1);caption.Children().Append(text);content.Children().Append(caption);pick.Content(content);
            previews.push_back({str(choice,L"id"),pick,preview});
            AutomationProperties::SetAutomationId(pick,L"filter-"+str(choice,L"id"));
            ToolTipService::SetToolTip(pick,box_value(str(choice,L"tooltip")));rows.Children().Append(pick);
        }
        markSelected();
        if(!choices.Size()){auto empty=label(data,str(picker,L"empty_label"));empty.Margin({8,8,8,8});empty.Opacity(.55);rows.Children().Append(empty);}
    }
};
}
FrameworkElement FiltersPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,std::function<double()>* contentHeight,std::function<J()>* scrollMetrics,bool split){
    auto view=std::make_shared<FiltersView>();view->data=data;view->split=split;view->init();bindings.emplace_back([view]{view->refresh();});
    if(scrollMetrics)*scrollMetrics=[weak=std::weak_ptr(view)]{
        if(auto view=weak.lock()){
            double unit=0;for(auto const& row:view->previews)if(row.button.IsLoaded()&&row.button.ActualHeight()>0){auto margin=row.button.Margin();unit=row.button.ActualHeight()+margin.Top+margin.Bottom+view->rows.Spacing();break;}
            return O({{L"fixed_height",N((view->split?12.:18.+view->header.ActualHeight()))},{L"unit_height",N(unit)}});
        }return J{};
    };
    if(contentHeight)*contentHeight=[weak=std::weak_ptr(view)]{
        if(auto view=weak.lock())return (view->split?12.:18.+view->header.ActualHeight())+view->list.ExtentHeight();
        return -1.;
    };
    return view->root;
}

FrameworkElement FilterTypesPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    struct State {hstring key;std::vector<Button> buttons;};
    auto state=std::make_shared<State>();Grid root;root.Padding({6,6,6,6});root.RowSpacing(6);
    RowDefinition rows;rows.Height({1,GridUnitType::Star});root.RowDefinitions().Append(rows);
    RowDefinition footer;footer.Height({1,GridUnitType::Auto});root.RowDefinitions().Append(footer);
    StackPanel choices;choices.Spacing(2);
    ScrollView scroll;scroll.Content(choices);scroll.HorizontalScrollMode(ScrollingScrollMode::Disabled);
    scroll.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);
    scroll.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);root.Children().Append(scroll);
    auto cancel=button(data,L"Cancel",[data]{data->dispatch(O({{L"type",S(L"effect")},{L"action",O({{L"op",S(L"cancel_filter")}})}}));});
    cancel.HorizontalAlignment(HorizontalAlignment::Left);cancel.Height(36);
    AutomationProperties::SetAutomationId(cancel,L"cancel-filter");Grid::SetRow(cancel,1);root.Children().Append(cancel);
    bindings.emplace_back([data,state,choices]{
        auto categories=array(data->state,L"filter_categories");auto key=categories.Stringify()+data->theme();
        if(state->key!=key){
            state->key=key;state->buttons.clear();choices.Children().Clear();
            for(auto value:categories){
                auto item=value.GetObject();auto category=item.GetNamedValue(L"id");
                auto pick=button(data,str(item,L"label"),[data,category]{data->dispatch(O({{L"type",S(L"filter_picker")},
                    {L"action",O({{L"op",S(L"category")},{L"category",category}})}}));});
                pick.Height(44);pick.Padding({6,5,6,5});pick.HorizontalAlignment(HorizontalAlignment::Stretch);
                pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
                StackPanel labelRow;labelRow.Orientation(Orientation::Horizontal);labelRow.Spacing(6);
                labelRow.Children().Append(icon(str(item,L"icon"),data->theme()));labelRow.Children().Append(label(data,str(item,L"label")));
                pick.Content(labelRow);AutomationProperties::SetAutomationId(pick,L"filter-type-"+str(item,L"id",L"all"));
                state->buttons.push_back(pick);choices.Children().Append(pick);
            }
        }
        auto selectedCategory=str(object(data->state,L"filter_picker"),L"category");
        for(uint32_t i=0;i<categories.Size();++i){
            bool active=str(categories.GetObjectAt(i),L"id")==selectedCategory;
            state->buttons[i].Background(active?selected(data):clear());
            AutomationProperties::SetItemStatus(state->buttons[i],active?L"Selected":L"");
        }
    });
    return root;
}
