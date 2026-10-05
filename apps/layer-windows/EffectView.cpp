#include "pch.h"
#include "EffectControls.h"
#include "NativeMenus.h"
#include "ScopesView.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
using namespace CapyEffects;
namespace {
Windows::UI::Color rgba(A const& a){
    if(a.Size()!=4)return {};
    auto byte=[&](int i){return uint8_t(std::round(std::clamp(a.GetNumberAt(i),0.,1.)*255));};
    return {byte(3),byte(0),byte(1),byte(2)};
}
struct ColorEditor : std::enable_shared_from_this<ColorEditor> {
    std::shared_ptr<WorkspaceData> data;
    hstring id;
    std::function<J()> get;
    std::function<void(J)> set;
    std::function<hstring()> context,currentTitle;
    std::function<bool()> opaque;
    bool compact=false;
    TextBlock name;Button pick;
    StackPanel root,fields;
    Bindings numbers;
    SolidColorBrush sample{Windows::UI::Color{}};
    hstring editingContext;
    std::shared_ptr<ColorForm> form;
    void rebuild(){
        fields.Children().Clear();form=std::make_shared<ColorForm>(data);auto weak=weak_from_this();
        form->init([weak,expected=editingContext](J value,std::optional<double>){if(auto self=weak.lock();self&&(!self->context||self->context()==expected))self->set(value);},id+L"-color");
        fields.Children().Append(form->root);
    }
    void init(hstring const& title){
        root.Spacing(6);fields.Spacing(6);fields.Visibility(Visibility::Collapsed);
        auto weak=weak_from_this();
        pick=button(data,title,[weak]{if(auto self=weak.lock()){
            self->fields.Visibility(self->fields.Visibility()==Visibility::Visible?Visibility::Collapsed:Visibility::Visible);
        }});
        pick.Background(data->brush(L"input"));
        Shapes::Rectangle color;color.Fill(sample);color.Margin({2,2,2,2});pick.Content(color);
        pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);pick.VerticalContentAlignment(VerticalAlignment::Stretch);
        AutomationProperties::SetAutomationId(pick,id+L"-color");
        if(compact){pick.Width(36);pick.Height(28);}
        else{
            Grid row;ColumnDefinition text;text.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(text);
            ColumnDefinition swatch;swatch.Width({56,GridUnitType::Pixel});row.ColumnDefinitions().Append(swatch);
            name=label(data,title);name.VerticalAlignment(VerticalAlignment::Center);row.Children().Append(name);
            pick.Height(32);pick.HorizontalAlignment(HorizontalAlignment::Stretch);
            Grid::SetColumn(pick,1);row.Children().Append(pick);root.Children().Append(row);
        }
        root.Children().Append(fields);
        rebuild();
    }
    void refresh(){
        auto title=currentTitle();if(!compact)name.Text(title);AutomationProperties::SetName(pick,title);tooltip(pick,title);
        auto next=context?context():L"";
        if(next!=editingContext){editingContext=next;rebuild();}
        form->load(displayColors(data->state),O({{L"color",get()}}),opaque(),object(data->model,L"color_panel"));
        sample.Color(displayColor(object(form->view,L"new")));
    }
};
hstring actionId(J const& action){
    auto role=str(action,L"role");
    return L"property-action-"+str(action,L"op")+(role.empty()?hstring():L"-"+role);
}
struct PropertiesView : std::enable_shared_from_this<PropertiesView> {
    std::shared_ptr<WorkspaceData> data;
    StackPanel root,body,actions;
    ContentControl bodyGate;
    TextBlock title;
    Button addFilter;
    ComboBox page,resource;
    Grid toolbar;
    FrameworkElement tonal{nullptr};
    Bindings tonalBindings;
    hstring actionSchema;
    std::vector<std::pair<uint32_t,Button>> actionButtons,groupButtons;
    std::vector<std::pair<uint32_t,MenuFlyoutItem>> menuItems;
    std::vector<uint32_t> presets;
    struct Field{FrameworkElement row{nullptr};Bindings bindings;hstring schema;};
    std::map<std::wstring,Field> fields;
    Bindings headings;
    hstring schema,pages,fieldOwner;
    static hstring fieldSchema(J const& c){
        return O({{L"kind",S(propertyKindSignature(c))},{L"color_action",object(c,L"color_action")},{L"domain",object(object(c,L"curve"),L"domain")}}).Stringify();
    }
    void arrange(std::vector<UIElement> const& items){
        auto children=body.Children();
        for(auto i=int32_t(children.Size())-1;i>=0;--i)if(std::find(items.begin(),items.end(),children.GetAt(uint32_t(i)))==items.end())children.RemoveAt(uint32_t(i));
        for(uint32_t i=0;i<items.size();++i){
            uint32_t at=0;
            if(i<children.Size()&&children.GetAt(i)==items[i])continue;
            if(children.IndexOf(items[i],at))children.Move(at,i);else children.InsertAt(i,items[i]);
        }
    }
    FrameworkElement build(J const& c,Bindings& bindings){
        auto property=std::make_shared<Property>(data,c);
        auto kind=object(c,L"kind");auto type=str(kind,L"kind");auto name=str(c,L"label");
        if(type==L"number"){
            auto captured=std::make_shared<bool>(false);NumberPresentation presentation;presentation.title=property->label().current;
            presentation.identity=[weak=std::weak_ptr<Property>(property)]{if(auto current=weak.lock())return current->identity();return hstring();};
            presentation.phase=[property,captured](hstring const& phase,double v){*captured=phase==L"down";property->action(property->setting(N(v)),phase);};
            return number(data,name,object(kind,L"numeric"),
                [property]{return num(object(property->model(),L"value"),L"value");},
                [property,captured](double v){property->action(property->setting(N(v)),*captured?L"move":L"");},
                bindings,nullptr,false,property->id(),false,presentation);
        }
        if(type==L"choice"){
            Grid row;row.ColumnSpacing(6);row.RowSpacing(6);
            ColumnDefinition caption;caption.Width({1,GridUnitType::Auto});row.ColumnDefinitions().Append(caption);
            ColumnDefinition choiceColumn;choiceColumn.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(choiceColumn);
            for(int i=0;i<2;i++){RowDefinition line;line.Height({1,GridUnitType::Auto});row.RowDefinitions().Append(line);}
            auto text=label(data,name);text.VerticalAlignment(VerticalAlignment::Center);row.Children().Append(text);
            ComboBox choices;choices.MinWidth(0);choices.MinHeight(32);choices.Height(32);
            choices.HorizontalAlignment(HorizontalAlignment::Stretch);choices.Padding({6,0,0,0});
            choices.FontSize(data->textSize());
            choices.Background(data->brush(L"input"));choices.BorderThickness({0,0,0,0});choices.CornerRadius({6,6,6,6});
            row.SizeChanged([text,weak=make_weak(choices)](auto&&,SizeChangedEventArgs const& event){
                auto choices=weak.get();if(!choices)return;
                text.Measure({INFINITY,INFINITY});
                bool wrap=event.NewSize().Width-text.DesiredSize().Width-6<std::min(150.f,event.NewSize().Width);
                Grid::SetRow(choices,wrap?1:0);Grid::SetColumn(choices,wrap?0:1);Grid::SetColumnSpan(choices,wrap?2:1);
            });
            for(auto option:array(kind,L"options"))comboOption(choices,option.GetString());
            AutomationProperties::SetName(choices,name);AutomationProperties::SetAutomationId(choices,property->id());
            choices.SelectionChanged([property,weak=make_weak(choices)](auto&&,auto&&){if(auto choices=weak.get();choices&&choices.SelectedIndex()>=0)property->set(N(choices.SelectedIndex()));});
            bindings.emplace_back([property,choices,text]{auto model=property->model();auto title=str(model,L"label");text.Text(title);AutomationProperties::SetName(choices,title);auto options=array(object(model,L"kind"),L"options");for(uint32_t i=0;i<options.Size();++i)comboOptionText(choices,i,options.GetStringAt(i));auto selected=int(num(object(model,L"value"),L"value"));if(choices.SelectedIndex()!=selected)choices.SelectedIndex(selected);});
            Grid::SetColumn(choices,1);row.Children().Append(choices);return row;
        }
        if(type==L"toggle"){
            CheckBox check;auto caption=label(data,name);check.Content(caption);check.MinHeight(32);check.MinWidth(0);
            AutomationProperties::SetName(check,name);AutomationProperties::SetAutomationId(check,property->id());
            check.Click([property,weak=make_weak(check)](auto&&,auto&&){if(auto c=weak.get())property->set(B(c.IsChecked().Value()));});
            bindings.emplace_back([property,check,caption]{auto model=property->model();auto title=str(model,L"label");caption.Text(title);AutomationProperties::SetName(check,title);check.IsChecked(flag(object(model,L"value"),L"value"));});return check;
        }
        if(type==L"color"){
            auto color=ColorField(property,name,[property]{return object(object(property->model(),L"value"),L"value");},
                [property](J a){property->set(a);},bindings);
            auto action=object(c,L"color_action");
            if(!action.Size())return color;
            Grid row;row.ColumnSpacing(6);
            ColumnDefinition fill;fill.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(fill);
            ColumnDefinition actionColumn;actionColumn.Width({40,GridUnitType::Pixel});row.ColumnDefinitions().Append(actionColumn);
            row.Children().Append(color);
            auto bucket=button(data,data->caption(L"color",L"use_selected"),[property,action]{if(property->current()&&!property->data->updating&&flag(property->view(),L"enabled"))property->data->dispatchDocument(action,to_hstring(uint64_t(property->epoch)));});
            bucket.Content(icon(L"fill",data->theme()));bucket.Width(40);bucket.Height(36);bucket.VerticalAlignment(VerticalAlignment::Top);
            std::wstring id(str(c,L"key").c_str());std::replace(id.begin(),id.end(),L'_',L'-');
            bindings.emplace_back([data=data,bucket]{auto caption=data->caption(L"color",L"use_selected");AutomationProperties::SetName(bucket,caption);CapyUi::tooltip(bucket,caption);});AutomationProperties::SetAutomationId(bucket,hstring(id+L"-bucket"));Grid::SetColumn(bucket,1);row.Children().Append(bucket);
            return row;
        }
        if(type==L"curve")return CurveField(property,bindings);
        if(type==L"gradient")return GradientField(property,bindings);
        return Grid();
    }
    void runAction(uint32_t index)const{
        auto view=object(data->state,L"layer_properties");auto list=array(view,L"actions");
        if(data->updating||index>=list.Size()||!flag(view,L"enabled"))return;
        data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",object(list.GetObjectAt(index),L"action")}}),
            to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch"))));
    }
    Button actionButton(J const& spec,hstring const& id,hstring const& text,std::function<void()> click){
        auto pick=button(data,text,std::move(click));pick.Height(28);pick.MinWidth(28);pick.Background(data->brush(L"input"));
        if(auto glyph=str(spec,L"icon");!glyph.empty()){pick.Width(28);pick.Content(icon(glyph,data->theme()));}
        else pick.Padding({8,0,8,0});
        AutomationProperties::SetAutomationId(pick,id);return pick;
    }
    void buildActions(A const& list){
        actions.Children().Clear();actionButtons.clear();groupButtons.clear();menuItems.clear();presets.clear();
        std::vector<hstring> groups;auto weak=weak_from_this();
        for(uint32_t i=0;i<list.Size();++i){
            auto spec=list.GetObjectAt(i);auto action=object(spec,L"action");
            if(str(action,L"op")==L"lookup_preset"){presets.push_back(i);continue;}
            auto group=object(spec,L"group");
            if(!group.Size()){
                auto pick=actionButton(spec,actionId(action),str(spec,L"label"),[weak,i]{if(auto self=weak.lock())self->runAction(i);});
                actionButtons.emplace_back(i,pick);actions.Children().Append(pick);continue;
            }
            auto groupId=str(group,L"id");if(std::find(groups.begin(),groups.end(),groupId)!=groups.end())continue;groups.push_back(groupId);
            auto menu=actionButton(spec,L"property-action-group-"+groupId,str(group,L"label"),[]{});
            MenuFlyout flyout;TrackPopup(flyout,data);
            for(uint32_t j=i;j<list.Size();++j){
                auto choice=list.GetObjectAt(j);if(str(object(choice,L"group"),L"id")!=groupId)continue;
                MenuFlyoutItem item;item.FontSize(data->textSize());item.MinHeight(32);
                AutomationProperties::SetAutomationId(item,actionId(object(choice,L"action")));
                item.Click([weak,j](auto&&,auto&&){if(auto self=weak.lock())self->runAction(j);});
                flyout.Items().Append(item);menuItems.emplace_back(j,item);
            }
            menu.Flyout(flyout);groupButtons.emplace_back(i,menu);actions.Children().Append(menu);
        }
    }
    void refreshActions(J const& view){
        auto list=array(view,L"actions");
        A keys;for(auto value:list){
            auto spec=value.GetObject();
            keys.Append(O({{L"action",object(spec,L"action")},{L"icon",spec.GetNamedValue(L"icon",JsonValue::CreateNullValue())},{L"group",S(str(object(spec,L"group"),L"id"))}}));
        }
        if(auto next=keys.Stringify();next!=actionSchema){actionSchema=next;buildActions(list);}
        auto name=[](Button const& pick,hstring const& text){AutomationProperties::SetName(pick,text);tooltip(pick,text);};
        for(auto const& [index,pick]:actionButtons){
            auto spec=list.GetObjectAt(index);auto text=str(spec,L"label");name(pick,text);
            if(str(spec,L"icon").empty())if(auto shown=pick.Content().try_as<Windows::Foundation::IReference<hstring>>();!shown||shown.Value()!=text)pick.Content(box_value(text));
        }
        for(auto const& [index,pick]:groupButtons){
            auto spec=list.GetObjectAt(index);auto text=str(object(spec,L"group"),L"label");name(pick,text);
            if(str(spec,L"icon").empty())pick.Content(box_value(text));
        }
        for(auto const& [index,item]:menuItems){auto text=str(list.GetObjectAt(index),L"label");item.Text(text);AutomationProperties::SetName(item,text);}
        for(auto const& [index,pick]:actionButtons)pick.IsEnabled(flag(view,L"enabled"));
        for(auto const& [index,pick]:groupButtons)pick.IsEnabled(flag(view,L"enabled"));
        auto resourceLabel=view.GetNamedValue(L"resource_label",JsonValue::CreateNullValue());
        bool lookup=resourceLabel.ValueType()==JsonValueType::String;
        resource.Visibility(lookup?Visibility::Visible:Visibility::Collapsed);resource.IsEnabled(flag(view,L"enabled"));
        if(lookup){
            std::vector<hstring> labels;for(auto index:presets)labels.push_back(str(list.GetObjectAt(index),L"label"));
            auto selection=view.GetNamedValue(L"resource_selection",JsonValue::CreateNullValue());int selected=-1;
            if(selection.ValueType()==JsonValueType::Number)
                if(auto at=std::find(presets.begin(),presets.end(),uint32_t(selection.GetNumber()));at!=presets.end())selected=int(at-presets.begin());
            auto resourceName=str(view,L"resource_name");
            if(selected<0){labels.push_back(resourceName.empty()?resourceLabel.GetString():resourceName);selected=int(labels.size())-1;}
            if(resource.Items().Size()!=labels.size()){resource.Items().Clear();for(auto const& text:labels)comboOption(resource,text);}
            for(uint32_t i=0;i<labels.size();++i)comboOptionText(resource,i,labels[i]);
            if(resource.SelectedIndex()!=selected)resource.SelectedIndex(selected);
            AutomationProperties::SetName(resource,resourceLabel.GetString());AutomationProperties::SetHelpText(resource,resourceName);
            tooltip(resource,resourceName.empty()?resourceLabel.GetString():resourceName);
        }
        bool leading=array(view,L"pages").Size()>1||lookup;
        toolbar.ColumnDefinitions().GetAt(0).Width({1,leading?GridUnitType::Star:GridUnitType::Auto});
        toolbar.Visibility(array(view,L"pages").Size()>1||list.Size()?Visibility::Visible:Visibility::Collapsed);
        bool statistics=flag(view,L"histogram");tonal.Visibility(statistics?Visibility::Visible:Visibility::Collapsed);
        if(statistics)for(auto const& bind:tonalBindings)bind();
    }
    void styleChoice(ComboBox const& box,hstring const& id){
        box.MinWidth(0);box.MinHeight(32);box.HorizontalAlignment(HorizontalAlignment::Stretch);box.FontSize(data->textSize());
        box.Background(data->brush(L"input"));box.BorderThickness({0,0,0,0});box.CornerRadius({6,6,6,6});
        AutomationProperties::SetAutomationId(box,id);box.Visibility(Visibility::Collapsed);
    }
    explicit PropertiesView(std::shared_ptr<WorkspaceData> source):data(std::move(source)){
        root.Spacing(6);body.Spacing(6);title=label(data,L"",true);
        AutomationProperties::SetAutomationId(root,L"layer-properties");
        bodyGate.Content(body);bodyGate.IsTabStop(false);bodyGate.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        styleChoice(page,L"properties-page");styleChoice(resource,L"property-resource-choice");
        toolbar.ColumnSpacing(6);
        for(auto width:{GridUnitType::Star,GridUnitType::Auto}){ColumnDefinition column;column.Width({1,width});toolbar.ColumnDefinitions().Append(column);}
        actions.Orientation(Orientation::Horizontal);actions.Spacing(4);actions.VerticalAlignment(VerticalAlignment::Center);
        Grid::SetColumn(actions,1);toolbar.Children().Append(page);toolbar.Children().Append(resource);toolbar.Children().Append(actions);
        tonal=CapyScopes::TonalScope(data,tonalBindings);tonal.Visibility(Visibility::Collapsed);
        page.SelectionChanged([weak=make_weak(page),data=data](auto&&,auto&&){
            auto box=weak.get();if(!box||data->updating||box.SelectedIndex()<0)return;
            auto view=object(data->state,L"layer_properties");auto pages=array(view,L"pages");
            if(uint32_t(box.SelectedIndex())>=pages.Size())return;auto id=str(pages.GetObjectAt(box.SelectedIndex()),L"id");
            if(id==str(view,L"page")||view.GetNamedValue(L"layer",JsonValue::CreateNullValue()).ValueType()!=JsonValueType::Number)return;
            data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",O({{L"op",S(L"select_page")},{L"layer",N(num(view,L"layer"))},{L"page",S(id)}})}}),
                to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch"))));
        });
        addFilter=button(data,L"",[]{});
        addFilter.Click([data=data,weak=make_weak(addFilter)](auto&&,auto&&){
            auto anchor=weak.get();auto model=object(object(data->state,L"layer_properties"),L"add_filter");
            if(data->updating||!anchor||!anchor.XamlRoot()||!model.Size())return;
            auto epoch=to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")));
            MenuFlyout menu;TrackPopup(menu,data);
            NativeMenuItems(menu.Items(),array(model,L"sections"),data,[data,epoch](J action){data->dispatchDocument(action,epoch);});
            menu.ShowAt(anchor);
        });
        AutomationProperties::SetAutomationId(addFilter,L"properties-add-filter");
        for(FrameworkElement part:{FrameworkElement(title),FrameworkElement(toolbar),tonal,FrameworkElement(bodyGate),FrameworkElement(addFilter)})root.Children().Append(part);
    }
    void init(){
        resource.SelectionChanged([weak=weak_from_this()](auto&&,auto&&){
            auto self=weak.lock();if(!self||self->data->updating)return;
            auto index=self->resource.SelectedIndex();
            if(index>=0&&size_t(index)<self->presets.size())self->runAction(self->presets[size_t(index)]);
        });
    }
    void refresh(){
        Updating updating(data);
        auto view=object(data->state,L"layer_properties");
        auto filterMenu=object(view,L"add_filter");addFilter.Visibility(filterMenu.Size()?Visibility::Visible:Visibility::Collapsed);
        addFilter.Content(box_value(str(filterMenu,L"title")));AutomationProperties::SetName(addFilter,str(filterMenu,L"title"));
        title.Text(str(view,L"title"));AutomationProperties::SetName(root,str(view,L"title"));CapyUi::tooltip(title,str(view,L"description"));
        auto pageChoices=array(view,L"pages");
        A pageIds;for(auto choice:pageChoices)pageIds.Append(S(str(choice.GetObject(),L"id")));
        if(auto key=pageIds.Stringify();key!=pages){pages=key;page.Items().Clear();for(auto choice:pageChoices)comboOption(page,str(choice.GetObject(),L"label"));}
        for(uint32_t i=0;i<pageChoices.Size();++i)comboOptionText(page,i,str(pageChoices.GetObjectAt(i),L"label"));
        int32_t selected=-1;for(uint32_t i=0;i<pageChoices.Size();++i)if(str(pageChoices.GetObjectAt(i),L"id")==str(view,L"page"))selected=int32_t(i);
        if(page.SelectedIndex()!=selected)page.SelectedIndex(selected);
        page.Visibility(pageChoices.Size()>1?Visibility::Visible:Visibility::Collapsed);page.IsEnabled(flag(view,L"enabled"));
        AutomationProperties::SetName(page,str(view,L"title"));
        refreshActions(view);
        A keys;for(auto value:array(view,L"controls")){
            auto c=value.GetObject();keys.Append(O({{L"key",S(str(c,L"key"))},{L"field",S(fieldSchema(c))},
                {L"section_id",c.GetNamedValue(L"section_id",JsonValue::CreateNullValue())}}));
        }
        auto owner=to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")))+L":"+view.GetNamedValue(L"layer",JsonValue::CreateNullValue()).Stringify();
        auto next=O({{L"owner",S(owner)},{L"controls",keys}}).Stringify();
        if(next!=schema){
            schema=next;headings.clear();std::vector<UIElement> items;
            auto previous=std::exchange(fields,{});if(owner!=fieldOwner)previous.clear();fieldOwner=owner;
            hstring sectionId=L"null";
            for(auto value:array(view,L"controls")){
                auto c=value.GetObject();
                auto nextSectionId=c.GetNamedValue(L"section_id",JsonValue::CreateNullValue()).Stringify();
                if(sectionId!=nextSectionId){sectionId=nextSectionId;auto section=str(c,L"section");
                    if(!items.empty()){Shapes::Rectangle line;line.Height(1);line.Opacity(.15);line.Fill(data->brush(L"text"));line.Margin({0,3,0,3});items.push_back(line);}
                    if(!section.empty()){auto heading=label(data,section,true);heading.Margin({6,0,0,0});items.push_back(heading);auto property=std::make_shared<Property>(data,c);headings.emplace_back([heading,property]{heading.Text(str(property->model(),L"section"));});}
                }
                std::wstring key=str(c,L"key").c_str();auto shape=fieldSchema(c);
                auto found=previous.find(key);
                Field field=found!=previous.end()&&found->second.schema==shape?std::move(found->second):Field{nullptr,{},shape};
                if(!field.row){field.row=build(c,field.bindings);}
                items.push_back(field.row);fields.insert_or_assign(key,std::move(field));
            }
            arrange(items);
        }
        for(auto const& update:headings)update();
        for(auto const& [key,field]:fields)for(auto const& update:field.bindings)update();
        bodyGate.IsEnabled(flag(view,L"enabled"));body.Opacity(flag(view,L"enabled")?1.:.4);
    }
};
}
FrameworkElement CapyEffects::ColorField(std::shared_ptr<Property> const& property,hstring const& title,
    std::function<J()> get,std::function<void(J)> set,Bindings& bindings,std::function<hstring()> context,std::function<hstring()> currentTitle){
    auto editor=std::make_shared<ColorEditor>();editor->data=property->data;editor->id=property->id();
    editor->get=std::move(get);editor->set=std::move(set);editor->context=std::move(context);
    editor->currentTitle=currentTitle?std::move(currentTitle):std::function<hstring()>([property]{return str(property->model(),L"label");});
    editor->opaque=[property]{return flag(object(property->model(),L"kind"),L"opaque");};
    editor->init(title);bindings.emplace_back([editor]{editor->refresh();});return editor->root;
}
ColorSwatch CapyEffects::CompactColorField(std::shared_ptr<WorkspaceData> const& data,hstring const& id,std::function<hstring()> title,
    std::function<J()> get,std::function<void(J)> set,Bindings& bindings,std::function<hstring()> context){
    auto editor=std::make_shared<ColorEditor>();editor->data=data;editor->id=id;editor->compact=true;
    editor->get=std::move(get);editor->set=std::move(set);editor->context=std::move(context);
    editor->currentTitle=std::move(title);editor->opaque=[]{return false;};
    editor->init(editor->currentTitle());bindings.emplace_back([editor]{editor->refresh();});return {editor->pick,editor->root};
}
FrameworkElement PropertiesPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<PropertiesView>(data);view->init();bindings.emplace_back([view]{view->refresh();});return view->root;
}
