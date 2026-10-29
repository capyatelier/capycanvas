#include "pch.h"
#include "LayersView.h"
#include "EffectControls.h"
#include "WorkspaceQuery.h"
#include "WorkspaceGeometry.h"
#include "PanelScroll.h"
#include <chrono>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

using namespace CapyLayers;
namespace CapyLayers {
UIElement ElementFactory::GetElement(ElementFactoryGetArgs const& args){
    auto view=owner.lock();if(!view)return Border();
    if(args.Data().try_as<J>())return view->addPaper;
    auto row=std::make_shared<LayerRow>();row->owner=view;row->data=view->data;
    row->id=unbox_value<double>(args.Data());row->epoch=view->epoch;row->init();if(view->pickup)view->pickup->Attach(row);row->refresh();
    view->rows.emplace(row->root.as<::IUnknown>().get(),row);return row->root;
}
void ElementFactory::RecycleElement(ElementFactoryRecycleArgs const& args){
    if(auto view=owner.lock()){
        auto found=view->rows.find(args.Element().as<::IUnknown>().get());
        if(found!=view->rows.end()){found->second->commit(false);view->rows.erase(found);}
    }
}
LayersView::~LayersView(){ForgetLayerThumbnails(data->thumbnails,this);if(outsideSurface)outsideSurface.RemoveHandler(UIElement::PointerPressedEvent(),box_value(outsidePress));if(pickup)pickup->Cancel();if(timer)timer.Stop();if(menu)menu.Hide();}
void LayersView::init(){
    auto weak=weak_from_this();root.RowSpacing(0);
    outsidePress=PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& event){if(auto self=weak.lock()){
        auto original=event.OriginalSource().try_as<DependencyObject>();
        for(auto const& [key,row]:self->rows){
            bool within=false;for(auto node=original;node;node=VisualTreeHelper::GetParent(node))if(node==row->root){within=true;break;}
            if(!within)row->swipe(0);
        }
    }});
    root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->outsideSurface){
        self->outsideSurface=self->root.XamlRoot().Content();
        self->outsideSurface.AddHandler(UIElement::PointerPressedEvent(),box_value(self->outsidePress),true);
    }});
    root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->outsideSurface){
        self->outsideSurface.RemoveHandler(UIElement::PointerPressedEvent(),box_value(self->outsidePress));self->outsideSurface=nullptr;
    }});
    AutomationProperties::SetAutomationId(root,L"layer-panel");
    for(auto unit:{GridUnitType::Auto,GridUnitType::Star,GridUnitType::Auto}){
        RowDefinition row;row.Height({1,unit});root.RowDefinitions().Append(row);
    }
    auto iconSlot=data->panelMetric(L"control_height"),compound=data->panelMetric(L"compound_spacing");
    header.Padding({6,4,6,4});header.Spacing(compound);values.ColumnSpacing(data->panelMetric(L"group_spacing"));
    for(int i=0;i<2;i++){ColumnDefinition column;column.Width({1,GridUnitType::Star});values.ColumnDefinitions().Append(column);}
    blend=button(data,L"",[weak]{if(auto self=weak.lock();self&&!self->data->updating){auto layer=self->editing();if(layer.Size())self->context(num(layer,L"id"),false,self->blend,{},false,true);}});
    blendLabel=label(data,L"",true);blendLabel.TextTrimming(TextTrimming::CharacterEllipsis);blendLabel.VerticalAlignment(VerticalAlignment::Center);
    {Grid content;ColumnDefinition text;text.Width({1,GridUnitType::Star});ColumnDefinition arrow;arrow.Width({1,GridUnitType::Auto});
        content.ColumnDefinitions().Append(text);content.ColumnDefinitions().Append(arrow);content.ColumnSpacing(4);content.Children().Append(blendLabel);
        ContentControl chevron;chevron.VerticalContentAlignment(VerticalAlignment::Center);chevron.IsTabStop(false);chevron.IsHitTestVisible(false);Grid::SetColumn(chevron,1);content.Children().Append(chevron);
        controls.emplace_back([data=data,chevron,theme=std::make_shared<hstring>()](J,J){if(*theme!=data->theme()){*theme=data->theme();chevron.Content(icon(L"chevron-down",*theme,16));}});blend.Content(content);}
    blend.MinWidth(0);blend.MinHeight(data->controlHeight());blend.Height(data->controlHeight());blend.Padding({6,0,6,0});blend.HorizontalContentAlignment(HorizontalAlignment::Stretch);
    copyName(data,header,data->copyCaption(L"layers",L"controls"));copyName(data,values,data->copyCaption(L"layers",L"blend_opacity"));
    copyName(data,tools,data->copyCaption(L"layers",L"flags"));copyName(data,footerFrame,data->copyCaption(L"layers",L"actions"));
    AutomationProperties::SetAutomationId(header,L"layer-controls");
    AutomationProperties::SetAutomationId(values,L"layer-options");
    AutomationProperties::SetAutomationId(tools,L"layer-flags");
    AutomationProperties::SetAutomationId(footerFrame,L"layer-footer");
    blend.FontSize(data->textSize());blend.FontWeight(Windows::UI::Text::FontWeights::Bold());
    blend.Background(data->brush(L"control_fill"));blend.BorderThickness({0,0,0,0});blend.CornerRadius(data->controlCorner());
    blend.HorizontalAlignment(HorizontalAlignment::Stretch);
    copyName(data,blend,data->copyCaption(L"layers",L"blend"));AutomationProperties::SetAutomationId(blend,L"layer-blend");
    opacityGate.HorizontalContentAlignment(HorizontalAlignment::Stretch);
    values.Children().Append(blend);Grid::SetColumn(opacityGate,1);values.Children().Append(opacityGate);header.Children().Append(values);
    tools.Orientation(Orientation::Horizontal);tools.Spacing(compound);
    struct Toggle {wchar_t const* icon;LocalizedCopy label;wchar_t const* property;wchar_t const* op;wchar_t const* capability;};
    for(auto spec:{Toggle{L"alpha-lock",data->copyCaption(L"layers",L"alpha_lock"),L"alpha_locked",L"alpha_lock",L"alpha_lock"},
        Toggle{L"lock",data->copyCaption(L"layers",L"lock_editing"),L"locked",L"lock",L"edit_lock"}}){
        auto pick=button<Primitives::ToggleButton>(data,spec.label,[weak,spec]{if(auto self=weak.lock()){
            auto layer=self->editing();if(layer.Size())self->action(O({{L"op",S(spec.op)},
                {L"id",layer.GetNamedValue(L"id")},{L"value",B(!flag(layer,spec.property))}}));
        }});
        pick.Width(iconSlot);pick.Height(iconSlot);pick.Content(icon(spec.icon,data->theme()));
        AutomationProperties::SetAutomationId(pick,L"layer-"+hstring(spec.op));
        actionTooltip(data,pick,[weak,spec]{auto self=weak.lock();auto layer=self?self->editing():J{};if(!layer.Size())return J{};
            return O({{L"type",S(L"layer")},{L"action",O({{L"op",S(spec.op)},{L"id",layer.GetNamedValue(L"id")},{L"value",B(!flag(layer,spec.property))}})}});});
        tools.Children().Append(pick);controls.emplace_back([data=data,pick,spec](J layer,J capabilities){
            pick.IsEnabled(flag(capabilities,spec.capability));pick.IsChecked(flag(layer,spec.property));pick.Opacity(pick.IsEnabled()?1.:.36);pick.Background(flag(layer,spec.property)?selected(data):clear());
        });
    }
    auto attachment=button<Primitives::ToggleButton>(data,L"",[weak]{if(auto self=weak.lock()){
        auto operation=object(object(self->view(),L"attachment"),L"action");if(operation.Size())self->action(operation);
    }});
    attachment.Width(iconSlot);attachment.Height(iconSlot);AutomationProperties::SetAutomationId(attachment,L"layer-attachment");tools.Children().Append(attachment);
    controls.emplace_back([weak,attachment,key=std::make_shared<hstring>()](J,J){if(auto self=weak.lock()){
        auto state=object(self->view(),L"attachment");auto next=self->data->theme()+L":"+str(state,L"icon");
        if(*key!=next){*key=next;attachment.Content(icon(str(state,L"icon"),self->data->theme()));}
        attachment.IsEnabled(object(state,L"action").Size()!=0);attachment.IsChecked(flag(state,L"checked"));
        attachment.Opacity(attachment.IsEnabled()?1.:.36);attachment.Background(flag(state,L"checked")?selected(self->data):clear());
        AutomationProperties::SetName(attachment,str(state,L"label"));AutomationProperties::SetHelpText(attachment,str(state,L"description"));
        CapyUi::tooltip(attachment,str(state,L"description"));
    }});
    auto reference=button<Primitives::ToggleButton>(data,L"",[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"reference_selection")}}));});
    reference.Width(iconSlot);reference.Height(iconSlot);reference.Content(icon(L"reference",data->theme()));
    buttonFeedback(reference,data->brush(L"text"),20,41,nullptr,false,data->tint(L"text",31));
    AutomationProperties::SetAutomationId(reference,L"layer-reference");actionTooltip(data,reference,[]{return O({{L"type",S(L"layer")},{L"action",O({{L"op",S(L"reference_selection")}})}});});tools.Children().Append(reference);
    controls.emplace_back([weak,reference](J,J){if(auto self=weak.lock()){
        auto view=self->view();reference.IsEnabled(flag(view,L"can_reference"));reference.Opacity(reference.IsEnabled()?1.:.36);
        reference.IsChecked(flag(view,L"references_selected"));reference.Background(flag(view,L"references_selected")?self->data->tint(L"text",31):clear());
        auto text=str(view,L"reference_action_label");if(AutomationProperties::GetName(reference)!=text){AutomationProperties::SetName(reference,text);CapyUi::tooltip(reference,text);}
    }});
    header.Children().Append(tools);root.Children().Append(header);
    auto factory=make_self<ElementFactory>();factory->owner=weak;repeater.ItemTemplate(factory.as<IElementFactory>());
    StackLayout layout;layout.Orientation(Orientation::Vertical);layout.Spacing(0);repeater.Layout(layout);repeater.ItemsSource(source);
    repeater.HorizontalCacheLength(.0);repeater.VerticalCacheLength(.5);
    addPaper=button(data,L"",[weak]{if(auto self=weak.lock();self&&!self->data->updating){
        auto action=object(self->view(),L"add_paper");
        if(action.Size())self->data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",action.GetNamedValue(L"action")}}),epochOf(self->data));
    }});
    addPaper.MinHeight(LayerRowHeight);addPaper.Padding(LayerRowPadding);addPaper.CornerRadius({0});
    addPaper.FontWeight(Windows::UI::Text::FontWeights::Normal());addPaper.HorizontalAlignment(HorizontalAlignment::Stretch);
    AutomationProperties::SetAutomationId(addPaper,L"layer-add-paper");
    list.Content(repeater);list.HorizontalScrollMode(ScrollingScrollMode::Disabled);
    list.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);list.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);mouseScrollbars(list);list.ContentOrientation(ScrollingContentOrientation::Vertical);panelScrolling(list,repeater,data);
    AutomationProperties::SetAutomationId(list,L"layer-list");copyName(data,list,data->copyCaption(L"layers",L"title"));listFrame.Children().Append(list);
    connectionOverlay.IsHitTestVisible(false);AutomationProperties::SetAccessibilityView(connectionOverlay,Automation::Peers::AccessibilityView::Raw);
    listFrame.Children().Append(connectionOverlay);Grid::SetRow(listFrame,1);root.Children().Append(listFrame);
    list.ViewChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->connections();});
    listFrame.SizeChanged([weak](auto&&,SizeChangedEventArgs const& e){if(auto self=weak.lock()){
        RectangleGeometry clip;clip.Rect({0,0,e.NewSize().Width,e.NewSize().Height});self->connectionOverlay.Clip(clip);self->connections();
    }});
    repeater.LayoutUpdated([weak](auto&&,auto&&){if(auto self=weak.lock())self->connections();});
    footer.Orientation(Orientation::Horizontal);footer.Spacing(compound);
    auto footerButton=[&](hstring const& iconName,LocalizedCopy const& text,hstring const& id,std::function<void()> action,hstring const& slot=L""){
        auto pick=button(data,L"",std::move(action));pick.Width(iconSlot);pick.Height(iconSlot);pick.Content(icon(iconName,data->theme()));
        AutomationProperties::SetAutomationId(pick,id);copyName(data,pick,text,true);footer.Children().Append(pick);
        if(!slot.empty())footerButtons.emplace_back(slot,pick);
        return pick;
    };
    for(bool group:{false,true}){
        auto create=O({{L"op",S(L"new")},{L"group",B(group)},{L"clipped",B(false)}});
        auto pick=footerButton(group?L"folder":L"add-layer",group?data->copyCaption(L"layers",L"new_group"):data->copyCaption(L"layers",L"new_layer"),group?L"layer-new-group":L"layer-new",
            [weak,create]{if(auto self=weak.lock())self->action(create);},group?L"new_group":L"new_layer");
        actionTooltip(data,pick,[create]{return O({{L"type",S(L"layer")},{L"action",create}});});
        if(group)continue;
        auto vectorLayer=footerButton(L"add-vector-layer",data->copyCaption(L"layers",L"new_vector_layer"),L"layer-new-vector",[weak]{if(auto self=weak.lock();self&&!self->data->updating)
            self->data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"new_vector_layer")}}));},L"new_vector_layer");
        actionTooltip(data,vectorLayer,[]{return O({{L"type",S(L"invoke")},{L"command",S(L"new_vector_layer")}});});
        controls.emplace_back([data=data,vectorLayer](J,J){
            vectorLayer.IsEnabled(flag(find(array(data->state,L"commands"),L"id",L"new_vector_layer"),L"enabled"));vectorLayer.Opacity(vectorLayer.IsEnabled()?1.:.36);
        });
    }
    auto selectionLayer=footerButton(L"selection-brush",data->copyCaption(L"layers",L"new_selection_layer"),L"layer-new-selection",[weak]{if(auto self=weak.lock();self&&!self->data->updating)
        self->data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"new_selection_layer")}}));},L"new_selection_layer");
    controls.emplace_back([data=data,selectionLayer](J,J){
        auto command=find(array(data->state,L"commands"),L"id",L"new_selection_layer");
        selectionLayer.IsEnabled(flag(command,L"enabled"));selectionLayer.Opacity(selectionLayer.IsEnabled()?1.:.36);
    });
    auto mask=footerButton(L"mask",data->copyCaption(L"layers",L"add_mask"),L"layer-add-mask",[weak]{if(auto self=weak.lock()){
        auto layer=self->editing();if(layer.Size())self->action(O({{L"op",S(L"add_mask")},{L"id",layer.GetNamedValue(L"id")},{L"replace",B(false)}}));
    }},L"add_mask");
    controls.emplace_back([mask](J,J capabilities){mask.IsEnabled(flag(capabilities,L"mask"));mask.Opacity(mask.IsEnabled()?1.:.36);});
    auto addFilter=footerButton(L"add-filter",data->copyCaption(L"layers",L"add_filter"),L"layer-add-filter",[weak]{if(auto self=weak.lock())self->showMenu(object(self->view(),L"add_filter"),self->footer);},L"add_filter");
    controls.emplace_back([weak,addFilter](J,J){if(auto self=weak.lock()){addFilter.IsEnabled(object(self->view(),L"add_filter").Size()!=0);addFilter.Opacity(addFilter.IsEnabled()?1.:.36);}});
    auto import=footerButton(L"image",data->copyCaption(L"layers",L"import_image"),L"layer-import",[weak]{if(auto self=weak.lock()){
        self->data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"import_image")}}));
    }},L"import_image");
    controls.emplace_back([data=data,import](J,J){
        auto file=object(data->state,L"document_file");
        import.IsEnabled(flag(data->model,L"brush_ready")&&!flag(file,L"busy")&&!flag(file,L"close_ready"));import.Opacity(import.IsEnabled()?1.:.36);
    });
    auto remove=footerButton(L"delete",data->copyCaption(L"layers",L"delete_selected"),L"layer-delete",[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"delete_selected")}}));},L"delete");
    controls.emplace_back([weak,remove](J,J){if(auto self=weak.lock()){remove.IsEnabled(flag(self->view(),L"can_delete"));remove.Opacity(remove.IsEnabled()?1.:.36);}});
    auto more=footerButton(L"more-small",data->copyCaption(L"layers",L"actions"),L"layer-actions",[weak]{if(auto self=weak.lock()){
        auto editing=self->editing();
        bool maskRow=flag(self->view(),L"quick_mask")||flag(editing,L"selection_layer");
        self->context(maskRow?num(editing,L"id",-1):-1,false,self->footer,{},false,false,self->footerOverflow());
    }});
    controls.emplace_back([weak,more](J layer,J){
        auto self=weak.lock();if(!self)return;
        self->fitFooter();more.IsEnabled(layer.Size()!=0);more.Opacity(more.IsEnabled()?1.:.36);
    });
    footer.Children().RemoveAtEnd();
    footerFrame.Padding({6,4,6,4});
    footerFrame.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->fitFooter();});
    ColumnDefinition actionsColumn;actionsColumn.Width({1,GridUnitType::Star});footerFrame.ColumnDefinitions().Append(actionsColumn);
    ColumnDefinition moreColumn;moreColumn.Width({iconSlot,GridUnitType::Pixel});footerFrame.ColumnDefinitions().Append(moreColumn);
    footerFrame.Children().Append(footer);Grid::SetColumn(more,1);footerFrame.Children().Append(more);
    Grid::SetRow(footerFrame,2);root.Children().Append(footerFrame);
    pickup=std::make_unique<LayerRowDrag>(shared_from_this());
    timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(120));
    timer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->preview();});
    root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->data->thumbnails){self->timer.Start();self->preview();}});
    root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){
        self->timer.Stop();ForgetLayerThumbnails(self->data->thumbnails,self.get());self->menuOwner.Cancel();if(self->pickup)self->pickup->Cancel();
    }});
}
void LayersView::refresh(){
    CapyEffects::Updating updating(data);
    auto panel=find(array(data->model,L"panels"),L"id",L"layers");
    auto shown=[&](wchar_t const* control){return flag(find(array(panel,L"controls"),L"control",control),L"visible_in_panel",true);};
    bool rowsShown=shown(L"layers"),opacityShown=shown(L"layer_opacity");
    header.Visibility(full&&(rowsShown||opacityShown)?Visibility::Visible:Visibility::Collapsed);
    blend.Visibility(rowsShown?Visibility::Visible:Visibility::Collapsed);tools.Visibility(rowsShown?Visibility::Visible:Visibility::Collapsed);
    list.Visibility(rowsShown?Visibility::Visible:Visibility::Collapsed);listFrame.Visibility(list.Visibility());opacityGate.Visibility(opacityShown?Visibility::Visible:Visibility::Collapsed);
    footerFrame.Visibility(full&&shown(L"layer_actions")?Visibility::Visible:Visibility::Collapsed);
    auto nextEpoch=epochOf(data);if(epoch!=nextEpoch){
        epoch=nextEpoch;menuOwner.Cancel();if(pickup)pickup->Cancel();source.Clear();
    }
    auto active=editing(),capabilities=object(view(),L"controls");
    blend.IsEnabled(flag(capabilities,L"blend"));
    if(auto text=str(active,L"blend_label");blendLabel.Text()!=text){blendLabel.Text(text);AutomationProperties::SetItemStatus(blend,text);}
    auto activeId=num(active,L"id",-1);auto nextKey=epoch+L":"+to_hstring(activeId);
    if(opacityKey!=nextKey){
        revealTarget=activeId>=0?std::optional<double>(activeId):std::nullopt;
        opacityKey=nextKey;opacityLayer=activeId;opacityBindings.clear();
        auto weak=weak_from_this();auto generation=epoch;NumberPresentation presentation;
        presentation.title=[weak]{if(auto self=weak.lock())return self->data->caption(L"layers",L"opacity");return hstring();};
        auto captured=std::make_shared<bool>(false);
        auto apply=[weak,generation,activeId](double value,hstring const& phase){if(auto self=weak.lock();self&&epochOf(self->data)==generation){
            bool continuing=!phase.empty()&&phase!=L"down";
            if(!continuing&&(self->data->updating||num(self->editing(),L"id",-1)!=activeId||!flag(object(self->view(),L"controls"),L"opacity")))return;
            auto action=O({{L"op",S(L"layer_opacity")},{L"layer",N(activeId)},{L"opacity",N(value)}});
            if(!phase.empty())action=O({{L"op",S(L"gesture")},{L"phase",S(phase)},{L"action",action}});
            self->data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",action}}),generation);
        }};
        presentation.phase=[apply,captured](hstring const& phase,double value){*captured=phase==L"down";apply(value,phase);};
        opacityGate.Content(number(data,data->caption(L"layers",L"opacity"),object(data->catalog,L"layer_opacity"),
            [weak]{if(auto self=weak.lock())return num(self->editing(),L"opacity",1);return 1.;},
            [apply,captured](double value){apply(value,*captured?L"move":L"");},opacityBindings,nullptr,true,L"layer-opacity",true,presentation));
    }
    opacityGate.IsEnabled(flag(capabilities,L"opacity"));for(auto const& bind:opacityBindings)bind();
    bool opacityRequested=num(view(),L"opacity_popup",-1)>=0
        &&num(view(),L"opacity_surface",-1)==double(reinterpret_cast<uintptr_t>(get_abi(root)));
    if(opacityRequested&&(!opacityPopup||opacityPopupKey!=nextKey)){
        if(opacityPopup){auto previous=opacityPopup;opacityPopup=nullptr;previous.Hide();}
        opacityPopupBindings.clear();opacityPopupKey=nextKey;
        auto weak=weak_from_this();auto generation=epoch;
        auto content=number(data,data->caption(L"layers",L"opacity"),object(data->catalog,L"layer_opacity"),
            [weak]{if(auto self=weak.lock())return num(self->editing(),L"opacity",1);return 1.;},
            [weak,generation,activeId](double value){if(auto self=weak.lock();self&&epochOf(self->data)==generation&&!self->data->updating
                &&num(self->view(),L"opacity_popup",-1)==activeId){
                self->data->dispatchDocument(O({{L"type",S(L"set_layer_opacity")},{L"id",N(activeId)},{L"opacity",N(value)}}),generation);
            }},opacityPopupBindings,nullptr,false,L"layer-gesture-opacity");
        content.Width(240);opacityPopup=Flyout();opacityPopup.Content(content);TrackPopup(opacityPopup,data);
        auto opened=make_weak(opacityPopup);
        opacityPopup.Closed([weak,opened,generation](auto&&,auto&&){if(auto self=weak.lock();self&&self->opacityPopup==opened.get()){
            self->opacityPopup=nullptr;self->opacityPopupBindings.clear();
            if(epochOf(self->data)==generation)self->data->dispatchDocument(O({{L"type",S(L"layer")},{L"action",O({{L"op",S(L"close_opacity")}})}}),generation);
        }});
        FrameworkElement anchor=root;
        for(auto const& [element,row]:rows)if(row->id==activeId){anchor=row->root;break;}
        opacityPopup.ShowAt(anchor);
    }else if(!opacityRequested&&opacityPopup){auto previous=opacityPopup;opacityPopup=nullptr;opacityPopupBindings.clear();previous.Hide();}
    for(auto const& bind:opacityPopupBindings)bind();
    for(auto const& bind:controls)bind(active,capabilities);
    auto paper=object(view(),L"add_paper");addPaper.Visibility(paper.Size()?Visibility::Visible:Visibility::Collapsed);
    auto nextPaperKey=data->theme()+paper.Stringify();
    if(paper.Size()&&paperKey!=nextPaperKey){
        paperKey=nextPaperKey;StackPanel face;face.Orientation(Orientation::Horizontal);face.Spacing(data->panelMetric(L"group_spacing"));
        face.HorizontalAlignment(HorizontalAlignment::Center);face.VerticalAlignment(VerticalAlignment::Center);
        face.Children().Append(icon(str(paper,L"icon"),data->theme()));face.Children().Append(label(data,str(paper,L"label")));
        addPaper.Content(face);AutomationProperties::SetName(addPaper,str(paper,L"label"));
    }
    std::vector<double> ids;
    for(auto value:array(data->state,L"layers")){
        auto layer=value.GetObject();ids.push_back(num(layer,L"id"));
    }
    // Preserve existing elements on value changes; structural edits only change
    // the affected items. ItemsRepeater creates native widgets near the viewport.
    for(uint32_t i=0;i<ids.size();i++){
        double id=ids[i];
        if(i<source.Size()&&unbox_value_or<double>(source.GetAt(i),-1.)==id)continue;
        if(i<source.Size()&&source.GetAt(i).try_as<J>()){source.SetAt(i,box_value(id));continue;}
        for(uint32_t j=i+1;j<source.Size();j++)if(unbox_value_or<double>(source.GetAt(j),-1.)==id){source.RemoveAt(j);break;}
        source.InsertAt(i,box_value(id));
    }
    while(source.Size()>ids.size()+(paper.Size()?1:0))source.RemoveAtEnd();
    if(paper.Size()){
        if(source.Size()==ids.size())source.Append(paper);
        else if(!source.GetAt(uint32_t(ids.size())).try_as<J>())source.SetAt(uint32_t(ids.size()),paper);
    }
    for(auto const& [element,row]:rows)row->refresh();
    if(menuTarget&&menuOwner.Active()&&!panelRow(data->state,*menuTarget).Size()){
        menuOwner.Cancel();
    }
    if(pickup)pickup->Refresh();
    connections();
}
void LayersView::connections(){
    if(connectionFrame)return;
    connectionFrame=CompositionTarget::Rendering(auto_revoke,[weak=weak_from_this()](auto&&,auto&&){
        if(auto self=weak.lock()){self->connectionFrame.revoke();self->layoutConnections();}
    });
}
void LayersView::layoutConnections(){
    if(!list.IsLoaded()||list.Visibility()!=Visibility::Visible)return;
    revealEditing();
    auto layers=array(data->state,L"layers");std::map<double,std::pair<uint32_t,double>> order;
    for(uint32_t i=0;i<layers.Size();i++)order[num(layers.GetObjectAt(i),L"id")]={i,num(layers.GetObjectAt(i),L"depth")};
    std::map<double,Windows::Foundation::Rect> geometry;double column=0;uint32_t first=layers.Size(),last=0;
    for(auto const& [key,row]:rows){
        if(!row->current()||!row->root.IsLoaded()||row->content.ActualWidth()<=0||!order.contains(row->id))continue;
        auto rect=row->content.TransformToVisual(list).TransformBounds({0,0,float(row->content.ActualWidth()),float(row->content.ActualHeight())});
        geometry[row->id]=rect;auto [index,depth]=order.at(row->id);
        if(index<first){first=index;column=rect.X+row->swipeOffset-std::min(depth*8,24.);}last=std::max(last,index);
    }
    A shapes;auto edges=array(view(),L"connections");double height=list.ActualHeight();
    auto endpoint=[&](double id,bool bottom)->std::optional<double>{
        if(auto at=geometry.find(id);at!=geometry.end())return at->second.Y+(bottom?at->second.Height:0);
        if(auto at=order.find(id);at!=order.end()){
            if(at->second.first<first)return 0.;if(at->second.first>last)return height;
        }return {};
    };
    if(!geometry.empty())for(auto value:edges){
        auto edge=value.GetObject();bool effect=str(edge,L"kind")==L"effect";
        auto top=endpoint(num(edge,L"from"),effect),bottom=endpoint(num(edge,L"to"),!effect);
        if(!top||!bottom||*bottom<=*top||*bottom<0||*top>height)continue;
        shapes.Append(O({{L"effect",B(effect)},{L"x",N(column+std::min(num(edge,L"depth")*8,24.))},{L"top",N(*top)},{L"bottom",N(*bottom)}}));
    }
    auto key=data->theme()+shapes.Stringify();if(connectionKey==key)return;connectionKey=key;
    if(connectionTheme!=data->theme()){connectionTheme=data->theme();connectionOverlay.Children().Clear();connectionLines.clear();connectionGlyphs.clear();}
    struct Segment{double x,top,bottom;bool effect;};std::vector<Segment> segments;std::vector<Windows::Foundation::Point> glyphs;
    for(auto value:shapes){auto shape=value.GetObject();bool effect=flag(shape,L"effect");double x=num(shape,L"x"),top=num(shape,L"top"),bottom=num(shape,L"bottom");
        if(!effect){segments.push_back({x-3.5,top,bottom,false});continue;}
        double center=x+15,y=(top+bottom)*.5;
        if(y-top>6)segments.push_back({center,top,y-6,true});if(bottom-y>6)segments.push_back({center,y+6,bottom,true});
        glyphs.push_back({float(center-6),float(y-6)});
    }
    auto remove=[this](UIElement const& item){uint32_t at;if(connectionOverlay.Children().IndexOf(item,at))connectionOverlay.Children().RemoveAt(at);};
    while(connectionLines.size()>segments.size()){remove(connectionLines.back());connectionLines.pop_back();}
    while(connectionLines.size()<segments.size()){
        Shapes::Line item;item.StrokeStartLineCap(PenLineCap::Round);item.StrokeEndLineCap(PenLineCap::Round);connectionOverlay.Children().Append(item);connectionLines.push_back(item);
    }
    for(size_t i=0;i<segments.size();i++){
        auto const& [x,top,bottom,effect]=segments[i];auto const& item=connectionLines[i];
        item.X1(x);item.X2(x);item.Y1(top);item.Y2(bottom);item.StrokeThickness(effect?1:2);item.Stroke(data->brush(effect?L"text":L"relationship"));
    }
    while(connectionGlyphs.size()>glyphs.size()){remove(connectionGlyphs.back());connectionGlyphs.pop_back();}
    while(connectionGlyphs.size()<glyphs.size()){auto glyph=icon(L"effect-link",data->theme(),12);connectionOverlay.Children().Append(glyph);connectionGlyphs.push_back(glyph);}
    for(size_t i=0;i<glyphs.size();i++){Canvas::SetLeft(connectionGlyphs[i],glyphs[i].X);Canvas::SetTop(connectionGlyphs[i],glyphs[i].Y);}
}
void LayersView::preview(){
    if(!root.IsLoaded()||!root.XamlRoot()||!root.XamlRoot().IsHostVisible()||list.ActualHeight()<=0){ForgetLayerThumbnails(data->thumbnails,this);timer.Interval(std::chrono::milliseconds(120));return;}
    auto viewport=visibleBounds(list,root.XamlRoot().Content().as<FrameworkElement>());
    if(viewport.Width<=0||viewport.Height<=0){ForgetLayerThumbnails(data->thumbnails,this);timer.Interval(std::chrono::milliseconds(120));return;}
    std::vector<LayerThumbnail> visible;
    for(auto const& [element,row]:rows){
        if(!row->root.IsLoaded())continue;
        auto rect=row->root.TransformToVisual(list).TransformBounds({0,0,float(row->root.ActualWidth()),float(row->root.ActualHeight())});
        if(rect.Y+rect.Height>0&&rect.Y<list.ActualHeight())row->thumbnails(visible);
    }
    auto interval=RefreshLayerThumbnails(data->thumbnails,this,epoch,visible);
    if(timer.Interval()!=interval)timer.Interval(interval);
}
void LayersView::showMenu(J spec, FrameworkElement const& anchor){
    if(data->updating||!spec.Size()||!anchor.XamlRoot())return;
    if(pickup)pickup->Cancel();menuOwner.Cancel();
    auto document=epochOf(data);auto weak=weak_from_this();
    menuTarget.reset();menu=MenuFlyout();auto ticket=menuOwner.Begin(data,anchor,menu);
    menu.Opened([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->pickup)self->pickup->MenuChanged();});
    menu.Closed([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->pickup)self->pickup->MenuChanged();});
    NativeMenuItems(menu.Items(),array(spec,L"sections"),data,[weak,flyout=make_weak(menu),document](J action){
        if(auto self=weak.lock();self&&self->menu==flyout.get()&&epochOf(self->data)==document)self->data->dispatchDocument(action,document);
    });
    ticket.Show(anchor,Primitives::FlyoutShowOptions{});
}

void LayersView::fitFooter(){
    double width=footerFrame.ActualWidth();
    if(width<=0)return;
    for(auto const& value:array(view(),L"footer")){
        auto slot=value.GetObject();auto id=str(slot,L"button");
        for(auto const& [name,pick]:footerButtons)if(name==id)pick.Visibility(width+.5<num(slot,L"min_width")?Visibility::Collapsed:Visibility::Visible);
    }
}
JsonArray LayersView::footerOverflow(){
    JsonArray items;double width=footerFrame.ActualWidth();
    for(auto const& value:array(view(),L"footer")){auto slot=value.GetObject();if(width+.5<num(slot,L"min_width"))items.Append(object(slot,L"item"));}
    return items;
}
void LayersView::context(double id,bool mask,UIElement const& anchor,std::optional<Windows::Foundation::Point> at,bool holding,bool blendMenu,JsonArray extra){
    if(data->updating||!anchor.XamlRoot())return;
    if(!holding&&pickup)pickup->Cancel();
    if(id>=0&&!blendMenu)action(O({{L"op",S(L"context")},{L"id",N(id)},{L"mask",B(mask)}}));
    auto document=epoch;
    menuOwner.Cancel();menu=MenuFlyout();auto ticket=menuOwner.Begin(data,anchor.as<FrameworkElement>(),menu);
    menuTarget=id>=0?std::optional<double>(id):std::nullopt;
    auto weak=weak_from_this();auto target=make_weak(anchor);auto queue=root.DispatcherQueue();
    menu.Opened([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->pickup)self->pickup->MenuChanged();});
    menu.Closed([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->pickup)self->pickup->MenuChanged();});
    auto query=O({{L"epoch",S(document)},{L"id",id>=0?S(to_hstring(uint64_t(id))):JsonValue::CreateNullValue()},
        {L"mask",id>=0?B(mask):JsonValue::CreateNullValue()},{L"blend",B(blendMenu)}});
    bool queued=data->query(CanvasQueryKind::LayerMenu,to_string(query.Stringify()),[weak,target,queue,ticket,document,id,at,holding,extra](PreviewPacket packet){
        queue.TryEnqueue([weak,target,ticket,document,id,at,holding,extra,packet=std::move(packet)]{
            auto self=weak.lock();auto anchor=target.get();
            if(!self||!ticket.Current())return;
            if(epochOf(self->data)!=document||!packet||(id>=0&&!panelRow(self->data->state,id).Size())){ticket.Cancel();return;}
            try{
                auto reply=J::Parse(to_hstring(NativeString(capy_preview_metadata(packet.get())).get()));
                if(str(reply,L"epoch")!=document){ticket.Cancel();return;}
                auto spec=object(reply,L"menu");if(!spec.Size()){ticket.Cancel();return;}
                auto menuId=double(std::stoull(to_string(str(reply,L"id"))));
                JsonArray sections;
                if(extra&&extra.Size())sections.Append(extra);
                for(auto const& section:array(spec,L"sections"))sections.Append(section);
                NativeMenuItems(self->menu.Items(),sections,self->data,[weak,flyout=make_weak(self->menu),document,menuId](J action){
                    if(auto self=weak.lock();self&&self->menu==flyout.get()&&epochOf(self->data)==document
                        &&(panelRow(self->data->state,menuId).Size()||num(self->editing(),L"id",-1)==menuId))self->data->dispatchDocument(action,document);
                });
                Primitives::FlyoutShowOptions options;
                if(at)options.Position(*at);
                options.ShowMode(holding?Primitives::FlyoutShowMode::Transient:Primitives::FlyoutShowMode::Standard);
                ticket.Show(anchor.as<FrameworkElement>(),options);
            }catch(hresult_error const& error){ticket.Cancel();OutputDebugStringW(error.message().c_str());}
        });
    });
    if(!queued)ticket.Cancel();
    if(pickup)pickup->MenuChanged();
}

}
namespace CapyLayers {
void LayersView::revealEditing(){
    if(!revealTarget||list.ActualHeight()<=0)return;
    auto target=*revealTarget;revealTarget.reset();
    for(uint32_t i=0;i<source.Size();++i)if(unbox_value_or<double>(source.GetAt(i),-1.)==target){
        auto element=repeater.GetOrCreateElement(i);element.UpdateLayout();element.StartBringIntoView();return;
    }
}
J LayersView::scrollAnchor()const{
    J anchor;double top=0;
    for(auto const& [key,row]:rows)if(row->current()&&row->root.IsLoaded()){
        auto rect=row->root.TransformToVisual(list).TransformBounds({0,0,float(row->root.ActualWidth()),float(row->root.ActualHeight())});
        if(rect.Y+rect.Height>0&&rect.Y<list.ActualHeight()&&(!anchor.Size()||rect.Y<top)){top=rect.Y;anchor=O({{L"id",N(row->id)},{L"offset",N(rect.Y)}});}
    }
    return anchor;
}
bool LayersView::restoreAnchor(J const& anchor){
    auto id=num(anchor,L"id",-1);
    for(auto const& [key,row]:rows)if(row->id==id&&row->current()&&row->root.IsLoaded()){
        auto rect=row->root.TransformToVisual(list).TransformBounds({0,0,float(row->root.ActualWidth()),float(row->root.ActualHeight())});
        auto delta=rect.Y-num(anchor,L"offset");if(std::abs(delta)<.5)return true;
        auto before=list.VerticalOffset();
        auto target=std::clamp(before+delta,0.,std::max(0.,list.ExtentHeight()-list.ActualHeight()));
        if(std::abs(target-before)<.5)return true;
        scrollVertical(list,target);
        return false;
    }
    auto layers=array(data->state,L"layers");
    for(uint32_t i=0;i<layers.Size();++i)if(num(layers.GetObjectAt(i),L"id")==id){auto element=repeater.GetOrCreateElement(i);element.UpdateLayout();element.StartBringIntoView();return false;}
    return true;
}
}
FrameworkElement LayersPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,std::function<double(double)>* contentHeight,std::function<J()>* scrollMetrics,std::shared_ptr<LayersView>* layerView){
    auto view=std::make_shared<LayersView>();view->data=data;view->init();bindings.emplace_back([view]{view->refresh();});
    if(layerView)*layerView=view;
    if(scrollMetrics)*scrollMetrics=[weak=std::weak_ptr(view)]{
        if(auto view=weak.lock()){
            auto height=[](FrameworkElement const& item){return item.Visibility()==Visibility::Visible?item.ActualHeight():0.;};
            double unit=0;for(auto const& [key,row]:view->rows)if(row->root.IsLoaded()&&row->root.ActualHeight()>0){auto margin=row->root.Margin();unit=row->root.ActualHeight()+margin.Top+margin.Bottom;break;}
            return O({{L"fixed_height",N(height(view->header)+height(view->footerFrame))},{L"unit_height",N(std::max(unit,height(view->addPaper)))}});
        }return J{};
    };
    if(contentHeight)*contentHeight=[weak=std::weak_ptr(view)](double){
        if(auto view=weak.lock();view&&view->root.IsLoaded()){
            auto height=[](FrameworkElement const& item){return item.Visibility()==Visibility::Visible?item.ActualHeight():0.;};
            return height(view->header)+height(view->footerFrame)+
                (view->list.Visibility()==Visibility::Visible?view->list.ExtentHeight():0.);
        }
        return -1.;
    };
    return view->root;
}
