#include "pch.h"
#include "LayersView.h"
#include "ColorEditor.h"
#include "ExternalImages.h"
#include "WorkspaceQuery.h"
#include "WorkspaceGeometry.h"

using namespace CapyLayers;
namespace CapyLayers {
namespace {
double renameTarget(std::shared_ptr<WorkspaceData> const& data){
    auto value=object(data->state,L"layer_tools").GetNamedValue(L"rename_layer",JsonValue::CreateNullValue());
    return value.ValueType()==JsonValueType::Number?value.GetNumber():-1;
}
LayerThumbnail thumbnail(J const& layer,bool mask){
    return {to_hstring(uint64_t(num(layer,L"id"))),to_hstring(uint64_t(num(layer,mask?L"mask_id":L"id"))),
        to_hstring(uint64_t(num(layer,mask?L"mask_revision":L"paint_revision"))),mask};
}
J rowSelection(double id,bool checkbox=false){return O({{L"op",S(L"select_row")},{L"id",N(id)},
    {L"extend",B((GetKeyState(VK_SHIFT)&0x8000)!=0)},{L"toggle",B(checkbox||(GetKeyState(VK_CONTROL)&0x8000)!=0)}});}
}
void ThumbnailEdge::init(Canvas const& frame){
    auto ring=[](Geometry const& outside,Geometry const& inside){
        GeometryGroup edge;edge.FillRule(FillRule::EvenOdd);edge.Children().Append(outside);edge.Children().Append(inside);return edge;
    };
    auto inside=squircleRectangle(28,28,{14,14,14,14});TranslateTransform inset;inset.X(1);inset.Y(1);inside.Transform(inset);
    base.Data(ring(squircleRectangle(30,30,{15,15,15,15}),inside));base.Width(30);base.Height(30);
    inside=squircleRectangle(26,26,{13,13,13,13});inset=TranslateTransform();inset.X(3);inset.Y(3);inside.Transform(inset);
    outline.Data(ring(squircleRectangle(32,32,{16,16,16,16}),inside));outline.Width(32);outline.Height(32);outline.Opacity(0);
    scale.CenterX(16);scale.CenterY(16);outline.RenderTransform(scale);
    Canvas::SetLeft(outline,-1);Canvas::SetTop(outline,-1);
    frame.Children().Append(base);frame.Children().Append(outline);
}
void ThumbnailEdge::stop(){if(animation){animation.Stop();animation=nullptr;}}
void ThumbnailEdge::update(std::shared_ptr<WorkspaceData> const& data,bool editing){
    base.Fill(editing?accent(data):data->brush(L"text"));base.Opacity(editing?1:.1);outline.Fill(accent(data));
    if(selected==editing)return;
    bool animate=selected.has_value()&&outline.IsLoaded()&&Windows::UI::ViewManagement::UISettings().AnimationsEnabled();
    double fromScale=scale.ScaleX(),fromOpacity=outline.Opacity();
    stop();selected=editing;
    double toScale=editing?1:38./32,toOpacity=editing?1:0;
    scale.ScaleX(toScale);scale.ScaleY(toScale);outline.Opacity(toOpacity);
    if(!animate)return;
    animation=Animation::Storyboard();
    auto move=[&](wchar_t const* property,double from,double to){
        Animation::DoubleAnimationUsingKeyFrames motion;
        Animation::DiscreteDoubleKeyFrame start;start.KeyTime({std::chrono::milliseconds(0)});start.Value(from);
        Animation::SplineDoubleKeyFrame end;end.KeyTime({std::chrono::milliseconds(200)});end.Value(to);
        Animation::KeySpline ease;ease.ControlPoint1({.25f,.46f});ease.ControlPoint2({.45f,.94f});end.KeySpline(ease);
        motion.KeyFrames().Append(start);motion.KeyFrames().Append(end);
        Animation::Storyboard::SetTarget(motion,outline);Animation::Storyboard::SetTargetProperty(motion,property);animation.Children().Append(motion);
    };
    move(L"(UIElement.RenderTransform).(ScaleTransform.ScaleX)",fromScale,toScale);
    move(L"(UIElement.RenderTransform).(ScaleTransform.ScaleY)",fromScale,toScale);
    move(L"Opacity",fromOpacity,toOpacity);
    animation.Begin();
}
J LayerRow::model()const{return panelRow(data->state,id);}
bool LayerRow::current()const{return epoch==epochOf(data)&&model().Size()!=0;}
bool LayerRow::clickAllowed()const{auto view=owner.lock();return view&&(!view->pickup||!view->pickup->SuppressClick());}
bool LayerRow::contextAllowed()const{auto view=owner.lock();return view&&(!view->pickup||!view->pickup->SuppressContext());}
void LayerRow::action(J operation){if(!data->updating&&current())data->dispatchDocument(image?imageAction(operation):layerAction(operation),epoch);}
void LayerRow::context(bool isMask,UIElement const& anchor){if(current())if(auto view=owner.lock())view->context(id,isMask,anchor);}
void LayerRow::editFill(){
    auto fill=object(model(),L"fill_color");
    if(!fill.Size()||data->updating||!current())return;
    auto key=str(fill,L"key");
    EditColor(data,content,O({{L"color",object(fill,L"color")}}),flag(fill,L"opaque"),[weak=weak_from_this(),key](J color,std::optional<double>){
        auto self=weak.lock();if(!self||!self->current())return;
        self->data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",O({{L"op",S(L"set")},{L"layer",N(self->id)},{L"key",S(key)},
            {L"value",O({{L"kind",S(L"color")},{L"value",color}})}})}}),self->epoch);
    });
}
bool LayerRow::loadThumbnail(bool isMask){
    if(!(GetKeyState(VK_CONTROL)&0x8000)||flag(model(),L"group")||data->updating||!current())return false;
    data->dispatchDocument(O({{L"type",S(L"selection")},{L"action",O({{L"op",S(L"load_thumbnail")},{L"id",N(id)},{L"mask",B(isMask)},
        {L"shift",B((GetKeyState(VK_SHIFT)&0x8000)!=0)},{L"alt",B((GetKeyState(VK_MENU)&0x8000)!=0)}})}}),epoch);
    return true;
}
void LayerRow::initImage(){
    auto weak=weak_from_this();root.Child(body);
    root.MinHeight(40);root.Padding({6,2,6,2});root.BorderThickness({0});body.ColumnSpacing(0);body.VerticalAlignment(VerticalAlignment::Center);
    AutomationProperties::SetAutomationId(root,L"image-row-"+to_hstring(uint64_t(id)));
    for(auto width:{26.,26.,0.,5.,32.,0.,0.,0.,-1.,0.,0.,16.}){
        ColumnDefinition column;column.Width({width<0?1:width,width<0?GridUnitType::Star:GridUnitType::Pixel});body.ColumnDefinitions().Append(column);
    }
    auto select=[weak]{if(auto self=weak.lock();self&&self->clickAllowed())self->action(O({{L"op",S(L"select")},{L"id",N(self->id)},
        {L"extend",B((GetKeyState(VK_SHIFT)&0x8000)!=0||(GetKeyState(VK_CONTROL)&0x8000)!=0)}}));};
    eye=button(data,data->caption(L"layers",L"hide_image"),[weak]{if(auto self=weak.lock();self&&self->clickAllowed()){
        auto row=self->model();if(flag(row,L"editable"))self->action(O({{L"op",S(L"visibility")},{L"id",N(self->id)},{L"visible",B(!flag(row,L"visible"))}}));
    }});
    eye.Width(24);eye.MinHeight(24);eye.VerticalAlignment(VerticalAlignment::Stretch);eye.HorizontalAlignment(HorizontalAlignment::Left);body.Children().Append(eye);
    Grid::SetColumn(indent,2);body.Children().Append(indent);
    content=button(data,L"",[]{});content.Width(30);content.Height(30);content.IsHitTestVisible(false);content.IsTabStop(false);
    content.HorizontalAlignment(HorizontalAlignment::Left);content.UseSystemFocusVisuals(false);
    content.CornerRadius({15*CornerFit,15*CornerFit,15*CornerFit,15*CornerFit});
    contentThumbnail.Width(28);contentThumbnail.Height(28);contentThumbnail.IsHitTestVisible(false);contentThumbnail.IsTabStop(false);
    contentThumbnail.HorizontalAlignment(HorizontalAlignment::Center);contentThumbnail.VerticalAlignment(VerticalAlignment::Center);
    {Shapes::Path shape;shape.Width(28);shape.Height(28);shape.Data(squircleRectangle(28,28,{14,14,14,14}));
        contentPreview.Stretch(Stretch::Fill);shape.Fill(contentPreview);contentThumbnail.Content(shape);}
    contentTile.Width(30);contentTile.Height(30);contentSymbol.IsHitTestVisible(false);contentSymbol.Child(icon(L"image",data->theme(),16));
    contentSymbol.HorizontalAlignment(HorizontalAlignment::Center);contentSymbol.VerticalAlignment(VerticalAlignment::Center);
    contentTile.Children().Append(contentSymbol);contentTile.Children().Append(contentThumbnail);content.Content(contentTile);
    Grid::SetColumn(content,4);body.Children().Append(content);
    name=button(data,L"",select);
    name.MinHeight(36);name.HorizontalAlignment(HorizontalAlignment::Stretch);name.HorizontalContentAlignment(HorizontalAlignment::Stretch);
    name.FontWeight(Windows::UI::Text::FontWeights::Normal());name.Padding({0});name.Margin({6,0,2,0});
    title=label(data,L"");title.TextTrimming(TextTrimming::CharacterEllipsis);title.LineHeight(20);name.Content(title);
    Grid::SetColumn(name,8);body.Children().Append(name);
    name.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){
        if(e.Key()!=Windows::System::VirtualKey::Application&&!(e.Key()==Windows::System::VirtualKey::F10&&(GetKeyState(VK_SHIFT)&0x8000)))return;
        if(auto self=weak.lock()){self->context(false,self->name);e.Handled(true);}
    });
    grip=button(data,L"",[]{});grip.Width(16);grip.Height(16);grip.HorizontalAlignment(HorizontalAlignment::Left);grip.Opacity(.6);
    grip.Content(icon(L"grip",data->theme()));copyName(data,grip,data->copyCaption(L"layers",L"move_image"));
    Grid::SetColumn(grip,11);body.Children().Append(grip);
    for(auto item:{std::pair{eye,L"visibility"},std::pair{name,L"name"},std::pair{grip,L"drag"}})
        AutomationProperties::SetAutomationId(item.first,L"image-"+to_hstring(uint64_t(id))+L"-"+item.second);
    AutomationProperties::SetAutomationId(contentThumbnail,L"image-"+to_hstring(uint64_t(id))+L"-thumbnail");
    copyName(data,contentThumbnail,data->copyCaption(L"layers",L"preview"));
    root.Tapped([weak,select](auto&&,TappedRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->clickAllowed()){
        for(auto node=e.OriginalSource().try_as<DependencyObject>();node&&node!=self->root;node=VisualTreeHelper::GetParent(node))
            if(node.try_as<Controls::Primitives::ButtonBase>())return;
        select();e.Handled(true);
    }});
    root.RightTapped([weak](auto&&,RightTappedRoutedEventArgs const& e){
        if(auto self=weak.lock();self&&self->contextAllowed())self->context(false,self->root);
        e.Handled(true);
    });
    dropMark.IsHitTestVisible(false);dropMark.BorderBrush(accent(data));dropMark.Margin({-6,-2,-6,-2});
    Grid::SetColumnSpan(dropMark,12);body.Children().Append(dropMark);
}
void LayerRow::init(){
    if(image){initImage();return;}
    auto weak=weak_from_this();root.Child(swipeFrame);
    swipeDelete=button(data,data->copyCommon(L"delete"),[weak]{if(auto self=weak.lock()){self->swipe(0);self->action(O({{L"op",S(L"delete")},{L"id",N(self->id)}}));}});
    swipeDelete.HorizontalAlignment(HorizontalAlignment::Right);swipeDelete.VerticalAlignment(VerticalAlignment::Stretch);
    swipeDelete.MinWidth(0);swipeDelete.Padding({0});swipeDelete.CornerRadius({0});swipeDelete.Background(fill({255,192,28,40}));swipeDelete.Foreground(fill({255,255,255,255}));
    AutomationProperties::SetAutomationId(swipeDelete,L"layer-"+to_hstring(uint64_t(id))+L"-swipe-delete");
    swipeFrame.Children().Append(swipeDelete);swipeFrame.Children().Append(body);body.RenderTransform(swipeTransform);swipe(0);
    swipeFrame.SizeChanged([weak](auto&&,SizeChangedEventArgs const& e){if(auto self=weak.lock()){RectangleGeometry clip;clip.Rect({0,0,e.NewSize().Width,e.NewSize().Height});self->swipeFrame.Clip(clip);}});
    root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->swipe(0);self->contentEdge.stop();self->maskEdge.stop();}});root.MinHeight(40);root.Padding({6,2,6,2});root.BorderThickness({0});
    root.BorderBrush(accent(data));body.ColumnSpacing(0);body.VerticalAlignment(VerticalAlignment::Center);
    AutomationProperties::SetAutomationId(root,L"layer-row-"+to_hstring(uint64_t(id)));
    auto rowCaption=[weak]{
        auto self=weak.lock();if(!self||!self->current())return hstring();
        return self->data->caption(O({{L"type",S(L"layer_row")},{L"title",S(str(self->model(),L"label"))}}));
    };
    copyName(data,root,LocalizedCopy(rowCaption(),rowCaption));
    root.AllowDrop(true);
    root.DragOver([weak](auto&&,DragEventArgs const& event){if(auto self=weak.lock();self&&self->current()&&fileDrag(event)){
        event.Handled(true);auto fraction=float(event.GetPosition(self->root).Y/std::max(1.,self->root.ActualHeight()));auto deferral=event.GetDeferral();
        if(!QueryWorkspace(self->data->query,O({{L"type",S(L"image_layer_drop")},{L"target",N(self->id)},{L"fraction",N(fraction)}}),
            [event,deferral,data=self->data](J reply){auto position=str(object(reply,L"result"),L"position");
                event.AcceptedOperation(position.empty()?winrt::Windows::ApplicationModel::DataTransfer::DataPackageOperation::None:winrt::Windows::ApplicationModel::DataTransfer::DataPackageOperation::Copy);
                event.DragUIOverride().Caption(position==L"into"?data->caption(L"layers",L"drop_into"):position==L"above"?data->caption(L"layers",L"drop_above"):data->caption(L"layers",L"drop_below"));deferral.Complete();}))deferral.Complete();
    }});
    root.Drop([weak](auto&&,DragEventArgs const& event){if(auto self=weak.lock();self&&self->current()&&fileDrag(event)){
        auto action=imageDrop(self->data->state);A row;row.Append(N(self->id));row.Append(N(event.GetPosition(self->root).Y/std::max(1.,self->root.ActualHeight())));action.Insert(L"layer",row);
        receiveImageDrop(event,action,self->data->document);
    }});
    // Include the two-DIP gaps only beside visible flex items. Empty mask and
    // indentation columns must not add their own gaps.
    for(auto width:{26.,26.,0.,5.,32.,0.,12.,32.,-1.,0.,14.,16.}){
        ColumnDefinition column;column.Width({width<0?1:width,width<0?GridUnitType::Star:GridUnitType::Pixel});
        body.ColumnDefinitions().Append(column);
    }
    auto pick=[&](hstring const& title,int column,std::function<void()> action){
        auto control=button(data,title,[weak,action=std::move(action)]{
            auto self=weak.lock();auto view=self?self->owner.lock():nullptr;
            if(view&&(!view->pickup||!view->pickup->SuppressClick()))action();
        });control.Width(column==4||column==5||column==7?30:column==6?10:column==11?16:24);
        control.Height(30);control.HorizontalAlignment(HorizontalAlignment::Left);Grid::SetColumn(control,column);body.Children().Append(control);return control;
    };
    eye=pick(data->caption(L"layers",L"visibility"),0,[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"visibility")},{L"id",N(self->id)},{L"value",B(!flag(self->model(),L"visible"))}}));});
    check=pick(L"",1,[weak]{if(auto self=weak.lock())self->action(rowSelection(self->id,true));});
    copyName(data,check,data->copyCaption(L"layers",L"select_row_help"));
    for(auto control:{eye,check}){control.ClearValue(FrameworkElement::HeightProperty());control.MinHeight(24);control.VerticalAlignment(VerticalAlignment::Stretch);}
    Grid::SetColumn(indent,2);body.Children().Append(indent);
    content=pick(data->caption(L"layers",L"edit_content"),4,[weak]{if(auto self=weak.lock();self&&!self->loadThumbnail(false)){
        auto layer=self->model();self->action(flag(layer,L"group")?O({{L"op",S(L"collapse")},{L"id",N(self->id)}}):
            O({{L"op",S(L"select")},{L"id",N(self->id)},{L"mask",B(false)}}));
        self->editFill();
    }});
    load=pick(L"",5,[weak]{if(auto self=weak.lock();self&&!self->data->updating&&self->current())
        self->data->dispatchDocument(O({{L"type",S(L"selection")},{L"action",O({{L"op",S(L"load_layer")},{L"id",N(self->id)},
            {L"mode",S(L"new")},{L"inverted",B(false)}})}}),self->epoch);});
    load.Visibility(Visibility::Collapsed);
    mask=pick(L"",7,[weak]{if(auto self=weak.lock();self&&!self->loadThumbnail(true))self->action(O({{L"op",S(L"select")},{L"id",N(self->id)},{L"mask",B(true)}}));});
    copyName(data,mask,data->copyCaption(L"layers",L"edit_mask"));
    auto preview=[](ContentControl const& host,ImageBrush const& brush,Grid const& tile){
        host.Width(28);host.Height(28);host.IsHitTestVisible(false);host.IsTabStop(false);
        host.HorizontalAlignment(HorizontalAlignment::Center);host.VerticalAlignment(VerticalAlignment::Center);
        Shapes::Path shape;shape.Width(28);shape.Height(28);shape.Data(squircleRectangle(28,28,{14,14,14,14}));
        brush.Stretch(Stretch::Fill);shape.Fill(brush);host.Content(shape);
        tile.Width(30);tile.Height(30);tile.Children().Append(host);
    };
    preview(contentThumbnail,contentPreview,contentTile);preview(maskThumbnail,maskPreview,maskTile);
    contentSymbol.IsHitTestVisible(false);contentTile.Children().Append(contentSymbol);
    groupMode.IsHitTestVisible(false);groupMode.Width(14);groupMode.Height(14);groupMode.CornerRadius({2,2,2,2});groupMode.Background(data->brush(L"input"));
    groupMode.HorizontalAlignment(HorizontalAlignment::Right);groupMode.VerticalAlignment(VerticalAlignment::Bottom);groupMode.Margin({0,0,3,3});contentTile.Children().Append(groupMode);
    contentEdge.init(contentFrame);maskEdge.init(maskFrame);
    for(auto frame:{contentFrame,maskFrame}){
        frame.Width(30);frame.Height(30);frame.IsHitTestVisible(false);
        frame.HorizontalAlignment(HorizontalAlignment::Left);frame.VerticalAlignment(VerticalAlignment::Center);body.Children().Append(frame);
    }
    Grid::SetColumn(contentFrame,4);Grid::SetColumn(maskFrame,7);
    content.Content(contentTile);mask.Content(maskTile);
    content.UseSystemFocusVisuals(false);mask.UseSystemFocusVisuals(false);
    CornerRadius radius{15*CornerFit,15*CornerFit,15*CornerFit,15*CornerFit};content.CornerRadius(radius);mask.CornerRadius(radius);
    link=pick(data->caption(L"layers",L"link_mask"),6,[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"link_mask")},{L"id",N(self->id)},{L"value",B(!flag(self->model(),L"mask_linked"))}}));});
    actionTooltip(data,eye,[weak]{auto self=weak.lock();if(!self)return J{};
        return O({{L"type",S(L"set_layer_visibility")},{L"id",N(self->id)},{L"visible",B(!flag(self->model(),L"visible"))}});});
    actionTooltip(data,content,[weak]{auto self=weak.lock();if(!self)return J{};
        auto target=flag(self->model(),L"group")?O({{L"op",S(L"collapse")},{L"id",N(self->id)}}):O({{L"op",S(L"select")},{L"id",N(self->id)},{L"mask",B(false)}});
        return O({{L"type",S(L"layer")},{L"action",target}});});
    actionTooltip(data,mask,[weak]{auto self=weak.lock();if(!self)return J{};
        return O({{L"type",S(L"layer")},{L"action",O({{L"op",S(L"select")},{L"id",N(self->id)},{L"mask",B(true)}})}});});
    link.Content(icon(L"link",data->theme(),10));
    name=button(data,data->caption(L"layers",L"layer"),[weak]{if(auto self=weak.lock();self&&self->clickAllowed())self->action(rowSelection(self->id));});
    name.MinHeight(36);name.HorizontalAlignment(HorizontalAlignment::Stretch);name.HorizontalContentAlignment(HorizontalAlignment::Stretch);
    name.FontWeight(Windows::UI::Text::FontWeights::Normal());name.Padding({0});name.Margin({6,0,2,0});
    StackPanel caption;title=label(data,L"");title.TextTrimming(TextTrimming::CharacterEllipsis);caption.Children().Append(title);
    title.LineHeight(20);AutomationProperties::SetAutomationId(title,L"layer-"+to_hstring(uint64_t(id))+L"-label");
    meta=label(data,L"");meta.Opacity(.55);meta.LineHeight(20);
    AutomationProperties::SetAutomationId(meta,L"layer-"+to_hstring(uint64_t(id))+L"-meta");
    meta.TextTrimming(TextTrimming::CharacterEllipsis);caption.Children().Append(meta);name.Content(caption);
    Grid::SetColumn(name,8);body.Children().Append(name);
    rename.MinWidth(0);rename.MinHeight(24);rename.Height(24);rename.Padding({2,0,2,0});rename.Margin({6,0,2,0});rename.FontSize(data->textSize());
    rename.MaxLength(128);rename.Background(data->brush(L"input"));rename.Visibility(Visibility::Collapsed);
    copyName(data,rename,data->copyCaption(L"layers",L"name"));Grid::SetColumn(rename,8);body.Children().Append(rename);
    rename.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
        if(composingKey(e))return;
        if(e.Key()==Windows::System::VirtualKey::Enter){self->commit(false);e.Handled(true);}
        if(e.Key()==Windows::System::VirtualKey::Escape){self->commit(true);e.Handled(true);}
    }});
    rename.TextChanging([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating)self->committing=false;});
    rename.LosingFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->commit(false);});
    rename.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->commit(false);});
    name.DoubleTapped([weak](auto&&,DoubleTappedRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->clickAllowed()&&flag(self->model(),L"can_rename"))
        self->action(O({{L"op",S(L"begin_rename")},{L"id",N(self->id)}}));e.Handled(true);});
    name.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
        if(e.Key()==Windows::System::VirtualKey::F2){if(flag(self->model(),L"can_rename"))self->action(O({{L"op",S(L"begin_rename")},{L"id",N(self->id)}}));e.Handled(true);}
    }});
    for(auto target:{name,content,mask})target.KeyDown([weak](auto&& sender,KeyRoutedEventArgs const& e){
        if(e.Key()!=Windows::System::VirtualKey::Application&&!(e.Key()==Windows::System::VirtualKey::F10&&(GetKeyState(VK_SHIFT)&0x8000)))return;
        if(auto self=weak.lock()){
            auto anchor=sender.template as<Button>();self->context(anchor==self->mask,anchor);e.Handled(true);
        }
    });
    expand=pick(L"",9,[weak]{if(auto self=weak.lock();self&&!self->data->updating&&self->current()){auto layer=self->model();
        self->data->dispatchDocument(imageAction(O({{L"op",S(L"expand")},{L"layer",N(self->id)},{L"expanded",B(!flag(layer,L"expanded"))}})),self->epoch);}});
    expand.Visibility(Visibility::Collapsed);
    lockImage.Width(12);lockImage.Height(12);lockImage.HorizontalAlignment(HorizontalAlignment::Left);lockImage.IsHitTestVisible(false);Grid::SetColumn(lockImage,10);body.Children().Append(lockImage);
    grip=pick(L"",11,[]{});grip.Height(16);grip.Content(icon(L"grip",data->theme()));grip.Opacity(.6);
    copyName(data,grip,data->copyCaption(L"layers",L"move_layer"));
    for(auto item:{std::pair{eye,L"visibility"},std::pair{check,L"selection"},std::pair{content,L"content"},
        std::pair{load,L"load"},std::pair{mask,L"mask"},std::pair{link,L"link"},std::pair{name,L"name"},std::pair{expand,L"expand"},std::pair{grip,L"drag"}})
        AutomationProperties::SetAutomationId(item.first,L"layer-"+to_hstring(uint64_t(id))+L"-"+item.second);
    AutomationProperties::SetAutomationId(rename,L"layer-"+to_hstring(uint64_t(id))+L"-rename");
    copyName(data,contentThumbnail,data->copyCaption(L"layers",L"preview"));copyName(data,maskThumbnail,data->copyCaption(L"layers",L"mask_preview"));
    AutomationProperties::SetAutomationId(contentThumbnail,L"layer-"+to_hstring(uint64_t(id))+L"-thumbnail");
    AutomationProperties::SetAutomationId(maskThumbnail,L"layer-"+to_hstring(uint64_t(id))+L"-mask-thumbnail");
    root.Tapped([weak](auto&&,TappedRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->clickAllowed()){
        for(auto node=e.OriginalSource().try_as<DependencyObject>();node&&node!=self->root;node=VisualTreeHelper::GetParent(node))
            if(node.try_as<Controls::Primitives::ButtonBase>()||node.try_as<TextBox>())return;
        self->action(rowSelection(self->id));e.Handled(true);
    }});
    root.RightTapped([weak](auto&&,RightTappedRoutedEventArgs const& e){
        if(auto self=weak.lock()){
            if(self->renaming)return;
            if(self->contextAllowed())self->context(false,self->root);
        }
        e.Handled(true);
    });
    mask.RightTapped([weak](auto&&,RightTappedRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->contextAllowed())self->context(true,self->mask);e.Handled(true);});
    dropMark.IsHitTestVisible(false);dropMark.BorderBrush(accent(data));dropMark.Margin({-6,-2,-6,-2});
    Grid::SetColumnSpan(dropMark,12);body.Children().Append(dropMark);
}
void LayerRow::swipe(double offset){
    if(image){swipeOffset=0;return;}
    swipeOffset=std::clamp(offset,object(model(),L"right_swipe").Size()?-72.:0.,72.);swipeTransform.X(-swipeOffset);
    swipeDelete.Width(std::max(0.,swipeOffset));swipeDelete.Visibility(swipeOffset>0?Visibility::Visible:Visibility::Collapsed);
    if(auto view=owner.lock())view->connections();
}
void LayerRow::commit(bool cancel){
    if(!renaming||committing||data->updating||!current())return;
    if(renameTarget(data)!=id)return;
    committing=true;
    std::wstring_view text=rename.Text();
    bool blank=text.find_first_not_of(L" \t\r\n")==std::wstring_view::npos;
    action(cancel||blank||rename.Text()==str(model(),L"label")?O({{L"op",S(L"cancel_rename")}}):O({{L"op",S(L"rename")},{L"id",N(id)},{L"name",S(rename.Text())}}));
}
void LayerRow::highlight(int position, bool attachment){
    dropMark.BorderThickness(position==3?Thickness{2,2,2,2}:position==1?Thickness{0,2,0,0}:position==2?Thickness{0,0,0,2}:Thickness{0});
    content.BorderBrush(accent(data));content.BorderThickness(attachment||position==4?Thickness{2,2,2,2}:Thickness{0});
    AutomationProperties::SetHelpText(root,position==3?data->caption(L"layers",L"drop_into"):position==1?data->caption(L"layers",L"drop_above"):position==2?data->caption(L"layers",L"drop_below"):L"");
}
void LayerRow::refreshImage(J const& row){
    root.Background(flag(row,L"selected")?selected(data):clear());root.Opacity(flag(row,L"visible")?1:.6);
    auto label=str(row,L"label");title.Text(label);AutomationProperties::SetName(name,label);AutomationProperties::SetName(root,label);
    AutomationProperties::SetItemStatus(name,flag(row,L"selected")?data->caption(L"layers",L"selected"):data->caption(L"layers",L"unselected"));
    auto icons=data->theme()+(flag(row,L"visible")?L":eye":L":eye-hidden");
    if(icons!=iconKey){iconKey=icons;eye.Content(icon(flag(row,L"visible")?L"eye":L"eye-hidden",data->theme()));grip.Content(icon(L"grip",data->theme()));
        contentSymbol.Child(icon(L"image",data->theme(),16));}
    hstring eyeName=flag(row,L"visible")?data->caption(L"layers",L"hide_image"):data->caption(L"layers",L"show_image");
    if(AutomationProperties::GetName(eye)!=eyeName){AutomationProperties::SetName(eye,eyeName);CapyUi::tooltip(eye,eyeName);}
    eye.IsEnabled(flag(row,L"editable"));eye.Opacity(eye.IsEnabled()?1:.36);
    auto layer=findId(array(data->state,L"layers"),num(row,L"layer"));
    body.ColumnDefinitions().GetAt(2).Width({std::min(24.,(num(layer,L"depth")+1)*8),GridUnitType::Pixel});
    bool movable=flag(row,L"editable")&&(flag(row,L"can_raise")||flag(row,L"can_lower"));
    grip.Visibility(movable?Visibility::Visible:Visibility::Collapsed);grip.IsEnabled(movable);
}
void LayerRow::refresh(){
    if(!current())return;auto layer=model();
    if(image){refreshImage(layer);return;}
    root.Background(flag(layer,L"selected")?selected(data):clear());
    swipeDelete.IsEnabled(flag(layer,L"can_delete"));if((swipeOffset>0&&!flag(layer,L"can_delete"))||(swipeOffset<0&&!object(layer,L"right_swipe").Size())||renaming)swipe(0);
    title.Text(str(layer,L"label"));AutomationProperties::SetName(name,str(layer,L"label"));
    auto nextTitle=str(layer,L"label");if(nextTitle!=captionTitle){captionTitle=nextTitle;AutomationProperties::SetName(root,data->caption(O({{L"type",S(L"layer_row")},{L"title",S(nextTitle)}})));}
    auto shown=flag(layer,L"visible")&&!flag(layer,L"visibility_blocked")?L"eye":L"eye-hidden";
    eye.Opacity(flag(layer,L"visibility_blocked")?.35:1);
    auto icons=data->theme()+L":"+hstring(shown)+L":"+str(layer,L"selection_icon")+L":"+str(layer,L"content_icon")+L":"+
        to_hstring(flag(layer,L"pass_through"))+L":"+to_hstring(flag(layer,L"adjustment_effect"))+L":"+to_hstring(flag(layer,L"group"))+L":"+to_hstring(flag(layer,L"has_thumbnail"))+L":"+to_hstring(flag(layer,L"collapsed"))+L":"+to_hstring(flag(layer,L"locked"))+L":"+to_hstring(flag(layer,L"mask_linked"));
    if(icons!=iconKey){
        iconKey=icons;eye.Content(icon(shown,data->theme()));check.Content(icon(str(layer,L"selection_icon"),data->theme()));
        auto contentIcon=flag(layer,L"group")?(flag(layer,L"collapsed")?L"folder":L"folder-open"):str(layer,L"content_icon");
        bool adjustment=flag(layer,L"adjustment_effect"),preview=flag(layer,L"has_thumbnail"),symbol=!contentIcon.empty()&&!flag(layer,L"selection_layer");
        contentThumbnail.Visibility(preview?Visibility::Visible:Visibility::Collapsed);
        contentSymbol.Visibility(symbol?Visibility::Visible:Visibility::Collapsed);
        contentSymbol.Width(preview?14:adjustment?16:28);contentSymbol.Height(preview?14:adjustment?16:28);
        contentSymbol.Padding({preview?1.:0.});contentSymbol.CornerRadius({2,2,2,2});
        contentSymbol.HorizontalAlignment(preview?HorizontalAlignment::Right:HorizontalAlignment::Center);
        contentSymbol.VerticalAlignment(preview?VerticalAlignment::Bottom:VerticalAlignment::Center);
        contentSymbol.Margin(preview?Thickness{0,0,3,3}:Thickness{0});
        contentSymbol.Background(preview?data->brush(L"input"):clear());
        if(symbol){
            auto glyph=icon(contentIcon,data->theme(),preview?12:adjustment?16:28);
            if(preview)AutomationProperties::SetAutomationId(glyph,L"layer-"+to_hstring(uint64_t(id))+L"-type-symbol");
            contentSymbol.Child(glyph);
        }
        groupMode.Child(icon(L"group-pass-through",data->theme(),12));
        groupMode.Visibility(flag(layer,L"pass_through")?Visibility::Visible:Visibility::Collapsed);
        link.Content(icon(flag(layer,L"mask_linked")?L"link":L"unlink",data->theme(),10));load.Content(icon(L"selection-load",data->theme(),16));grip.Content(icon(L"grip",data->theme()));
        lockImage.Source(icon(flag(layer,L"locked")?L"lock":L"alpha-lock",data->theme(),12).Source());
    }
    bool selectionLayer=flag(layer,L"selection_layer");
    hstring eyeName=selectionLayer?(flag(layer,L"visible")?data->caption(L"layers",L"hide_selection"):data->caption(L"layers",L"show_selection")):(flag(layer,L"visible")?data->caption(L"layers",L"hide"):data->caption(L"layers",L"show"));
    if(AutomationProperties::GetName(eye)!=eyeName){AutomationProperties::SetName(eye,eyeName);CapyUi::tooltip(eye,eyeName);}
    hstring contentName=selectionLayer?data->caption(L"layers",L"edit_selection"):flag(layer,L"group")?(flag(layer,L"collapsed")?data->caption(L"layers",L"expand"):data->caption(L"layers",L"collapse")):data->caption(L"layers",L"edit_content");
    if(AutomationProperties::GetName(content)!=contentName){AutomationProperties::SetName(content,contentName);CapyUi::tooltip(content,contentName);}
    load.Visibility(selectionLayer?Visibility::Visible:Visibility::Collapsed);
    body.ColumnDefinitions().GetAt(5).Width({selectionLayer?32.:0.,GridUnitType::Pixel});
    if(selectionLayer){auto tip=str(layer,L"load_selection_tooltip");AutomationProperties::SetName(load,tip);CapyUi::tooltip(load,tip);}
    AutomationProperties::SetItemStatus(check,flag(layer,L"selected")?data->caption(L"search",L"selected"):data->caption(L"layers",L"unselected"));
    bool hasMask=flag(layer,L"has_mask");
    content.Background(clear());mask.Background(clear());
    contentEdge.update(data,flag(layer,L"content_selected"));maskEdge.update(data,flag(layer,L"mask_selected"));
    mask.Visibility(hasMask?Visibility::Visible:Visibility::Collapsed);link.Visibility(hasMask?Visibility::Visible:Visibility::Collapsed);
    maskFrame.Visibility(hasMask?Visibility::Visible:Visibility::Collapsed);
    body.ColumnDefinitions().GetAt(6).Width({hasMask?12.:0.,GridUnitType::Pixel});
    body.ColumnDefinitions().GetAt(7).Width({hasMask?32.:0.,GridUnitType::Pixel});
    link.IsEnabled(!flag(layer,L"locked"));
    hstring linkName=flag(layer,L"mask_linked")?data->caption(L"layers",L"unlink_mask"):data->caption(L"layers",L"link_mask_to_layer");
    AutomationProperties::SetName(link,linkName);CapyUi::tooltip(link,linkName);
    maskThumbnail.Opacity(flag(layer,L"mask_enabled")?1:.4);
    body.ColumnDefinitions().GetAt(2).Width({std::min(24.,num(layer,L"depth")*8),GridUnitType::Pixel});
    lockImage.Opacity(flag(layer,L"locked")||flag(layer,L"alpha_locked")?1:0);
    AutomationProperties::SetName(lockImage,flag(layer,L"locked")?data->caption(L"layers",L"locked"):flag(layer,L"alpha_locked")?data->caption(L"layers",L"alpha_locked"):L"");
    AutomationProperties::SetAccessibilityView(lockImage,flag(layer,L"locked")||flag(layer,L"alpha_locked")?Automation::Peers::AccessibilityView::Content:Automation::Peers::AccessibilityView::Raw);
    bool images=num(layer,L"object_count")>0;
    expand.Visibility(images?Visibility::Visible:Visibility::Collapsed);
    body.ColumnDefinitions().GetAt(9).Width({images?24.:0.,GridUnitType::Pixel});
    if(images){
        auto expandKey=data->theme()+(flag(layer,L"expanded")?L":open":L":closed");
        if(expandKey!=imageKey){imageKey=expandKey;auto glyph=icon(L"chevron-down",data->theme(),12);glyph.RenderTransformOrigin({.5f,.5f});
            RotateTransform turn;turn.Angle(flag(layer,L"expanded")?0:-90);glyph.RenderTransform(turn);expand.Content(glyph);}
        hstring expandName=flag(layer,L"expanded")?data->caption(L"layers",L"collapse_images"):data->caption(L"layers",L"expand_images");
        if(AutomationProperties::GetName(expand)!=expandName){AutomationProperties::SetName(expand,expandName);CapyUi::tooltip(expand,expandName);}
        AutomationProperties::SetItemStatus(expand,flag(layer,L"expanded")?data->caption(L"layers",L"expanded"):data->caption(L"layers",L"collapsed"));
    }
    body.ColumnDefinitions().GetAt(10).Width({flag(layer,L"can_drop_below")?14.:12.,GridUnitType::Pixel});
    body.ColumnDefinitions().GetAt(11).Width({flag(layer,L"can_drop_below")?16.:0.,GridUnitType::Pixel});
    grip.Visibility(flag(layer,L"can_drop_below")?Visibility::Visible:Visibility::Collapsed);
    grip.Opacity(flag(layer,L"can_drop_below")?.6:0);grip.IsEnabled(flag(layer,L"can_drop_below")&&!flag(layer,L"locked"));
    hstring details=str(layer,L"description");
    meta.Text(details);meta.Visibility(details.empty()?Visibility::Collapsed:Visibility::Visible);
    bool editing=renameTarget(data)==id;
    if(editing&&!renaming){renaming=true;committing=false;rename.Text(str(layer,L"label"));rename.Visibility(Visibility::Visible);name.Visibility(Visibility::Collapsed);
        focusRename();}
    else if(!editing&&renaming){renaming=false;committing=false;rename.Visibility(Visibility::Collapsed);name.Visibility(Visibility::Visible);}
}
void LayerRow::focusRename(){
    if(renameFocus){rename.LayoutUpdated(renameFocus);renameFocus={};}
    if(rename.Focus(FocusState::Programmatic)){rename.SelectAll();return;}
    renameFocus=rename.LayoutUpdated([weak=weak_from_this()](auto&&,auto&&){
        auto self=weak.lock();if(!self)return;
        if(!self->renaming||self->committing||self->rename.Focus(FocusState::Programmatic)){
            self->rename.LayoutUpdated(self->renameFocus);self->renameFocus={};
            if(self->renaming&&!self->committing)self->rename.SelectAll();
        }
    });
}
void LayerRow::thumbnails(std::vector<LayerThumbnail>& visible){
    if(!current())return;auto layer=model();
    if(image){
        auto key=to_hstring(uint64_t(id));LayerThumbnail item{key,key,to_hstring(uint64_t(num(layer,L"thumbnail_revision"))),false};visible.push_back(item);
        auto source=LayerThumbnailSource(data->thumbnails,epoch,item);
        if(contentPreview.ImageSource()!=source)contentPreview.ImageSource(source);
        contentSymbol.Visibility(source?Visibility::Collapsed:Visibility::Visible);
        AutomationProperties::SetItemStatus(contentThumbnail,source?data->caption(L"header",L"ready"):data->caption(L"layers",L"pending"));
        return;
    }
    for(bool isMask:{false,true}){
        if(isMask?!flag(layer,L"has_mask"):!flag(layer,L"has_thumbnail"))continue;
        auto item=thumbnail(layer,isMask);visible.push_back(item);
        auto source=LayerThumbnailSource(data->thumbnails,epoch,item);auto shown=isMask?maskThumbnail:contentThumbnail;
        auto preview=isMask?maskPreview:contentPreview;
        if(preview.ImageSource()!=source)preview.ImageSource(source);AutomationProperties::SetItemStatus(shown,source?data->caption(L"header",L"ready"):data->caption(L"layers",L"pending"));
    }
}
}
