#include "pch.h"
#include "LayersView.h"
#include "EffectControls.h"
#include <chrono>

using namespace CapyLayers;
namespace CapyLayers {
UIElement ElementFactory::GetElement(ElementFactoryGetArgs const& args){
    auto view=owner.lock();if(!view)return Border();
    auto row=std::make_shared<LayerRow>();row->owner=view;row->data=view->data;
    row->id=unbox_value<double>(args.Data());row->epoch=view->epoch;row->init();row->refresh();
    view->rows.emplace(row->root.as<::IUnknown>().get(),row);return row->root;
}
void ElementFactory::RecycleElement(ElementFactoryRecycleArgs const& args){
    if(auto view=owner.lock()){
        auto found=view->rows.find(args.Element().as<::IUnknown>().get());
        if(found!=view->rows.end()){found->second->commit(false);view->rows.erase(found);}
    }
}
LayersView::~LayersView(){if(timer)timer.Stop();if(dragTimer)dragTimer.Stop();if(menu)menu.Hide();}
void LayersView::init(){
    auto weak=weak_from_this();root.RowSpacing(0);
    AutomationProperties::SetAutomationId(root,L"layer-panel");
    for(auto unit:{GridUnitType::Auto,GridUnitType::Star,GridUnitType::Auto}){
        RowDefinition row;row.Height({1,unit});root.RowDefinitions().Append(row);
    }
    header.Padding({6,4,6,4});header.Spacing(2);values.ColumnSpacing(6);
    for(int i=0;i<2;i++){ColumnDefinition column;column.Width({1,GridUnitType::Star});values.ColumnDefinitions().Append(column);}
    blend.MinWidth(0);blend.MinHeight(26);blend.Height(26);blend.Padding({6,0,0,0});
    blend.FontSize(data->textSize());blend.Background(data->brush(L"input"));
    blend.HorizontalAlignment(HorizontalAlignment::Stretch);
    AutomationProperties::SetName(blend,L"Layer blend mode");AutomationProperties::SetAutomationId(blend,L"layer-blend");
    for(auto value:array(data->catalog,L"layer_blends"))blend.Items().Append(box_value(value.GetString()));
    blend.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating){
        int index=self->blend.SelectedIndex();auto layer=self->editing();
        if(index>=0&&layer.Size()&&flag(object(self->view(),L"controls"),L"blend"))
            self->action(O({{L"op",S(L"blend")},{L"id",layer.GetNamedValue(L"id")},{L"value",N(index)}}));
    }});
    auto blendOpen=std::make_shared<bool>(false);
    blend.DropDownOpened([data=data,blendOpen](auto&&,auto&&){if(!std::exchange(*blendOpen,true))data->popup(true);});
    blend.DropDownClosed([data=data,blendOpen](auto&&,auto&&){if(std::exchange(*blendOpen,false))data->popup(false);});
    blend.Unloaded([data=data,blendOpen](auto&&,auto&&){if(std::exchange(*blendOpen,false))data->popup(false);});
    opacityGate.HorizontalContentAlignment(HorizontalAlignment::Stretch);
    values.Children().Append(blend);Grid::SetColumn(opacityGate,1);values.Children().Append(opacityGate);header.Children().Append(values);
    tools.Orientation(Orientation::Horizontal);tools.Spacing(2);
    struct Toggle {wchar_t const* icon;wchar_t const* label;wchar_t const* property;wchar_t const* op;wchar_t const* capability;};
    for(auto spec:{Toggle{L"alpha-lock",L"Alpha lock",L"alpha_locked",L"alpha_lock",L"alpha_lock"},
        Toggle{L"lock",L"Lock editing",L"locked",L"lock",L"edit_lock"},Toggle{L"clip",L"Clip to layer below",L"clipped",L"clip",L"clip"}}){
        auto pick=button(data,spec.label,[weak,spec]{if(auto self=weak.lock()){
            auto layer=self->editing();if(layer.Size())self->action(O({{L"op",S(spec.op)},
                {L"id",layer.GetNamedValue(L"id")},{L"value",B(!flag(layer,spec.property))}}));
        }});
        pick.Width(24);pick.Height(24);pick.Content(icon(spec.icon,data->theme()));
        AutomationProperties::SetAutomationId(pick,L"layer-"+hstring(spec.op));ToolTipService::SetToolTip(pick,box_value(spec.label));
        tools.Children().Append(pick);controls.emplace_back([data=data,pick,spec](J layer,J capabilities){
            pick.IsEnabled(flag(capabilities,spec.capability));pick.Background(flag(layer,spec.property)?selected():clear());
        });
    }
    auto reference=button(data,L"Use selected layers as references",[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"reference_selection")}}));});
    reference.Width(24);reference.Height(24);reference.Content(icon(L"reference",data->theme()));
    AutomationProperties::SetAutomationId(reference,L"layer-reference");tools.Children().Append(reference);
    controls.emplace_back([weak,reference](J,J){if(auto self=weak.lock()){
        auto view=self->view();reference.IsEnabled(flag(view,L"can_reference"));
        reference.Background(flag(view,L"references_selected")?selected():clear());
        auto text=str(view,L"reference_action_label");AutomationProperties::SetName(reference,text);ToolTipService::SetToolTip(reference,box_value(text));
    }});
    header.Children().Append(tools);root.Children().Append(header);
    auto factory=make_self<ElementFactory>();factory->owner=weak;repeater.ItemTemplate(factory.as<IElementFactory>());
    StackLayout layout;layout.Orientation(Orientation::Vertical);layout.Spacing(0);repeater.Layout(layout);repeater.ItemsSource(source);
    repeater.HorizontalCacheLength(.0);repeater.VerticalCacheLength(.5);
    list.Content(repeater);list.HorizontalScrollMode(ScrollingScrollMode::Disabled);
    list.HorizontalScrollBarVisibility(ScrollingScrollBarVisibility::Hidden);list.VerticalScrollBarVisibility(ScrollingScrollBarVisibility::Auto);
    AutomationProperties::SetAutomationId(list,L"layer-scroll-container");Grid::SetRow(list,1);root.Children().Append(list);
    footer.Orientation(Orientation::Horizontal);footer.Spacing(2);footer.Padding({6,4,6,4});
    auto footerButton=[&](hstring const& iconName,hstring const& text,hstring const& id,std::function<void()> action){
        auto pick=button(data,text,std::move(action));pick.Width(24);pick.Height(24);pick.Content(icon(iconName,data->theme()));
        AutomationProperties::SetAutomationId(pick,id);ToolTipService::SetToolTip(pick,box_value(text));footer.Children().Append(pick);return pick;
    };
    footerButton(L"plus",L"New layer",L"layer-new",[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"new")},{L"group",B(false)},{L"clipped",B(false)}}));});
    footerButton(L"folder",L"New group",L"layer-new-group",[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"new")},{L"group",B(true)},{L"clipped",B(false)}}));});
    auto mask=footerButton(L"mask",L"Add layer mask",L"layer-add-mask",[weak]{if(auto self=weak.lock()){
        auto layer=self->editing();if(layer.Size())self->action(O({{L"op",S(L"add_mask")},{L"id",layer.GetNamedValue(L"id")},{L"replace",B(false)}}));
    }});
    controls.emplace_back([mask](J,J capabilities){mask.IsEnabled(flag(capabilities,L"mask"));});
    auto remove=footerButton(L"delete",L"Delete selected layers",L"layer-delete",[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"delete_selected")}}));});
    controls.emplace_back([weak,remove](J,J){if(auto self=weak.lock())remove.IsEnabled(flag(self->view(),L"can_delete"));});
    auto more=footerButton(L"more",L"Layer actions",L"layer-actions",[weak]{if(auto self=weak.lock()){
        self->context(-1,false,self->footer);
    }});
    controls.emplace_back([more](J layer,J){more.IsEnabled(layer.Size()!=0);});
    footer.Children().RemoveAtEnd();footer.Padding({0});
    Grid footerFrame;footerFrame.Padding({6,4,6,4});
    ColumnDefinition actionsColumn;actionsColumn.Width({1,GridUnitType::Star});footerFrame.ColumnDefinitions().Append(actionsColumn);
    ColumnDefinition moreColumn;moreColumn.Width({24,GridUnitType::Pixel});footerFrame.ColumnDefinitions().Append(moreColumn);
    footerFrame.Children().Append(footer);Grid::SetColumn(more,1);footerFrame.Children().Append(more);
    Grid::SetRow(footerFrame,2);root.Children().Append(footerFrame);
    dragTimer=root.DispatcherQueue().CreateTimer();dragTimer.Interval(std::chrono::milliseconds(16));
    dragTimer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock()){
        if(!self->dragCurrent()||self->dragSpeed==0){self->dragTimer.Stop();return;}
        ScrollingScrollOptions options(ScrollingAnimationMode::Disabled,ScrollingSnapPointsMode::Ignore);
        self->list.ScrollBy(0,self->dragSpeed,options);
    }});
    list.AddHandler(UIElement::DragOverEvent(),box_value(DragEventHandler([weak](auto&&,DragEventArgs const& e){
        if(auto self=weak.lock();self&&self->dragCurrent()){
            auto point=e.GetPosition(self->list);double height=self->list.ActualHeight();
            self->dragSpeed=point.Y<32?-std::clamp((32-point.Y)/2.,1.,16.):
                point.Y>height-32?std::clamp((point.Y-height+32)/2.,1.,16.):0;
            if(self->dragSpeed)self->dragTimer.Start();else self->dragTimer.Stop();
        }
    })),true);
    list.DragLeave([weak](auto&&,DragEventArgs const& e){if(auto self=weak.lock()){
        auto point=e.GetPosition(self->list);
        if(point.X<0||point.Y<0||point.X>self->list.ActualWidth()||point.Y>self->list.ActualHeight()){
            self->dragSpeed=0;self->dragTimer.Stop();
        }
    }});
    timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(120));
    timer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->preview();});
    root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->timer.Start();self->preview();}});
    root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){
        self->timer.Stop();++self->menuGeneration;if(self->menu)self->menu.Hide();self->clearDrag();
    }});
}
void LayersView::refresh(){
    CapyEffects::Updating updating(data);
    auto nextEpoch=epochOf(data);if(epoch!=nextEpoch){
        epoch=nextEpoch;++menuGeneration;if(menu)menu.Hide();clearDrag();source.Clear();
    }
    auto active=editing(),capabilities=object(view(),L"controls");
    blend.IsEnabled(flag(capabilities,L"blend"));blend.SelectedIndex(int(num(active,L"blend")));
    auto activeId=num(active,L"id",-1);auto nextKey=epoch+L":"+to_hstring(activeId);
    if(opacityKey!=nextKey){
        opacityKey=nextKey;opacityLayer=activeId;opacityBindings.clear();
        auto weak=weak_from_this();auto generation=epoch;
        opacityGate.Content(number(data,L"Layer opacity",object(data->catalog,L"layer_opacity"),
            [weak]{if(auto self=weak.lock())return num(self->editing(),L"opacity",1);return 1.;},
            [weak,generation,activeId](double value){if(auto self=weak.lock();self&&epochOf(self->data)==generation
                &&num(self->editing(),L"id",-1)==activeId&&flag(object(self->view(),L"controls"),L"opacity"))
                self->data->dispatchDocument(O({{L"type",S(L"set_layer_opacity")},{L"opacity",N(value)}}),generation);
            },opacityBindings,nullptr,true,L"layer-opacity"));
    }
    opacityGate.IsEnabled(flag(capabilities,L"opacity"));for(auto const& bind:opacityBindings)bind();
    for(auto const& bind:controls)bind(active,capabilities);
    auto layers=array(data->state,L"layers");
    // Preserve existing elements on value changes; structural edits only change
    // the affected items. ItemsRepeater creates native widgets near the viewport.
    for(uint32_t i=0;i<layers.Size();i++){
        double id=num(layers.GetObjectAt(i),L"id");
        if(i<source.Size()&&unbox_value<double>(source.GetAt(i))==id)continue;
        for(uint32_t j=i+1;j<source.Size();j++)if(unbox_value<double>(source.GetAt(j))==id){source.RemoveAt(j);break;}
        source.InsertAt(i,box_value(id));
    }
    while(source.Size()>layers.Size())source.RemoveAtEnd();
    for(auto const& [element,row]:rows)row->refresh();
    if(dragged&&!dragCurrent())clearDrag();
}
void LayersView::preview(){
    if(!root.IsLoaded()||!root.XamlRoot()||!root.XamlRoot().IsHostVisible()||list.ActualHeight()<=0)return;
    // Templates can create or replace the scroll provider after Loaded.
    if(auto presenter=list.ScrollPresenter();presenter&&AutomationProperties::GetAutomationId(presenter)!=L"layer-list"){
        AutomationProperties::SetAutomationId(presenter,L"layer-list");AutomationProperties::SetName(presenter,L"Layers");
    }
    std::vector<LayerThumbnail> visible;
    for(auto const& [element,row]:rows){
        if(!row->root.IsLoaded())continue;
        auto rect=row->root.TransformToVisual(list).TransformBounds({0,0,float(row->root.ActualWidth()),float(row->root.ActualHeight())});
        if(rect.Y+rect.Height>0&&rect.Y<list.ActualHeight())row->thumbnails(visible);
    }
    RefreshLayerThumbnails(data->thumbnails,epoch,visible);
}
void LayersView::context(double id,bool mask,UIElement const& anchor){
    if(data->updating||!anchor.XamlRoot())return;
    if(id>=0)action(O({{L"op",S(L"context")},{L"id",N(id)},{L"mask",B(mask)}}));
    auto generation=++menuGeneration;auto document=epoch;
    if(menu)menu.Hide();auto weak=weak_from_this();auto target=make_weak(anchor);auto queue=root.DispatcherQueue();
    auto query=O({{L"epoch",S(document)},{L"id",id>=0?S(to_hstring(uint64_t(id))):JsonValue::CreateNullValue()},
        {L"mask",id>=0?B(mask):JsonValue::CreateNullValue()}});
    data->query(CanvasQueryKind::LayerMenu,to_string(query.Stringify()),[weak,target,queue,generation,document,id](PreviewPacket packet){
        queue.TryEnqueue([weak,target,generation,document,id,packet=std::move(packet)]{
            auto self=weak.lock();auto anchor=target.get();
            if(!self||!anchor||!anchor.XamlRoot()||!self->root.IsLoaded()||self->menuGeneration!=generation||epochOf(self->data)!=document||!packet)return;
            try{
                auto reply=J::Parse(to_hstring(capy_preview_metadata(packet.get())));
                if(str(reply,L"epoch")!=document)return;
                auto spec=object(reply,L"menu");if(!spec.Size())return;
                auto menuId=double(std::stoull(to_string(str(reply,L"id"))));
                self->menu=MenuFlyout();TrackPopup(self->menu,self->data);
                NativeMenuItems(self->menu.Items(),array(spec,L"sections"),self->data,[weak,generation,document,menuId](J action){
                    if(auto self=weak.lock();self&&self->menuGeneration==generation&&epochOf(self->data)==document
                        &&(findId(array(self->data->state,L"layers"),menuId).Size()||num(self->editing(),L"id",-1)==menuId))self->data->dispatchDocument(action,document);
                });
                self->menu.ShowAt(anchor.as<FrameworkElement>());
            }catch(hresult_error const& error){OutputDebugStringW(error.message().c_str());}
        });
    });
}
bool LayersView::dragCurrent()const{
    if(!dragged||dragEpoch!=epochOf(data))return false;
    auto layer=findId(array(data->state,L"layers"),*dragged);
    return layer.Size()&&flag(layer,L"can_drop_below")&&!flag(layer,L"locked");
}
void LayersView::clearDrag(){dragged.reset();dragSpeed=0;if(dragTimer)dragTimer.Stop();for(auto const& [element,row]:rows)row->highlight(0);}
}
FrameworkElement LayersPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<LayersView>();view->data=data;view->init();bindings.emplace_back([view]{view->refresh();});return view->root;
}
