#include "pch.h"
#include "LayersView.h"
#include <chrono>
#include <winrt/Windows.UI.ViewManagement.h>
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
void LayerRow::action(J operation){if(!data->updating&&current())data->dispatchDocument(layerAction(operation),epoch);}
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
void LayerRow::chooseTexture(){
    if(data->updating||!current())return;
    action(O({{L"op",S(L"select")},{L"id",N(id)},{L"mask",B(false)}}));
    if(flag(object(model(),L"paper"),L"editable"))ChooseTexture(data,texture,id);
}
bool LayerRow::loadThumbnail(bool isMask){
    using Mod=Windows::System::VirtualKeyModifiers;
    return loadThumbnail(isMask,(GetKeyState(VK_CONTROL)&0x8000?Mod::Control:Mod::None)|
        (GetKeyState(VK_SHIFT)&0x8000?Mod::Shift:Mod::None)|(GetKeyState(VK_MENU)&0x8000?Mod::Menu:Mod::None));
}
bool LayerRow::loadThumbnail(bool isMask,Windows::System::VirtualKeyModifiers modifiers){
    using Mod=Windows::System::VirtualKeyModifiers;
    if((modifiers&Mod::Control)==Mod::None||(!isMask&&flag(model(),L"group"))||data->updating||!current())return false;
    data->dispatchDocument(O({{L"type",S(L"selection")},{L"action",O({{L"op",S(L"load_thumbnail")},{L"id",N(id)},{L"mask",B(isMask)},
        {L"shift",B((modifiers&Mod::Shift)!=Mod::None)},{L"alt",B((modifiers&Mod::Menu)!=Mod::None)}})}}),epoch);
    return true;
}
void LayerRow::init(){
    auto weak=weak_from_this();root.Child(swipeFrame);
    swipeActions.HorizontalAlignment(HorizontalAlignment::Right);swipeActions.VerticalAlignment(VerticalAlignment::Stretch);
    swipeFrame.Children().Append(swipeActions);swipeFrame.Children().Append(body);body.RenderTransform(swipeTransform);
    swipeFrame.SizeChanged([weak](auto&&,SizeChangedEventArgs const& e){if(auto self=weak.lock()){RectangleGeometry clip;clip.Rect({0,0,e.NewSize().Width,e.NewSize().Height});self->swipeFrame.Clip(clip);}});
    root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->swipe(0,false);self->contentEdge.stop();self->maskEdge.stop();self->textureEdge.stop();}});root.MinHeight(LayerRowHeight);root.BorderThickness({0});
    root.BorderBrush(accent(data));body.ColumnSpacing(0);body.Padding(layerRowPadding(data));
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
        receiveFileDrop(event,action,self->data->document);
    }});
    // Include the two-DIP gaps only beside visible flex items. Empty mask and
    // indentation columns must not add their own gaps.
    auto slot=data->panelMetric(L"control_height"),gap=data->panelMetric(L"compound_spacing");
    for(auto width:{slot+gap,slot+gap,0.,5.,32.,0.,12.,32.,0.,-1.,0.,14.,16.}){
        ColumnDefinition column;column.Width({width<0?1:width,width<0?GridUnitType::Star:GridUnitType::Pixel});
        body.ColumnDefinitions().Append(column);
    }
    auto pick=[&](hstring const& title,int column,std::function<void()> action){
        auto control=button(data,title,[weak,action=std::move(action)]{
            auto self=weak.lock();auto view=self?self->owner.lock():nullptr;
            if(view&&(!view->pickup||!view->pickup->SuppressClick()))action();
        });control.Width(column==4||column==5||column==7||column==8?30:column==6?10:column==12?16:slot);
        control.Height(30);control.HorizontalAlignment(HorizontalAlignment::Left);Grid::SetColumn(control,column);body.Children().Append(control);return control;
    };
    eye=pick(data->caption(L"layers",L"visibility"),0,[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"visibility")},{L"id",N(self->id)},{L"value",B(!flag(self->model(),L"visible"))}}));});
    check=pick(L"",1,[weak]{if(auto self=weak.lock())self->action(rowSelection(self->id,true));});
    copyName(data,check,data->copyCaption(L"layers",L"select_row_help"));
    for(auto control:{eye,check}){control.ClearValue(FrameworkElement::HeightProperty());control.MinHeight(slot);control.VerticalAlignment(VerticalAlignment::Stretch);}
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
    texture=pick(data->caption(L"paper",L"choose"),8,[weak]{if(auto self=weak.lock())self->chooseTexture();});
    copyName(data,texture,data->copyCaption(L"paper",L"choose"),true);
    texturePreview=PaperThumbnail::Create(data,28,14);texture.Content(texturePreview->root);
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
    contentEdge.init(contentFrame);maskEdge.init(maskFrame);textureEdge.init(textureFrame);
    for(auto frame:{contentFrame,maskFrame,textureFrame}){
        frame.Width(30);frame.Height(30);frame.IsHitTestVisible(false);
        frame.HorizontalAlignment(HorizontalAlignment::Left);frame.VerticalAlignment(VerticalAlignment::Center);body.Children().Append(frame);
    }
    Grid::SetColumn(contentFrame,4);Grid::SetColumn(maskFrame,7);Grid::SetColumn(textureFrame,8);
    content.Content(contentTile);mask.Content(maskTile);
    CornerRadius radius{15*CornerFit,15*CornerFit,15*CornerFit,15*CornerFit};
    for(auto const& tile:{content,mask,texture}){tile.UseSystemFocusVisuals(false);tile.CornerRadius(radius);}
    link=pick(data->caption(L"layers",L"link_mask"),6,[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"link_mask")},{L"id",N(self->id)},{L"value",B(!flag(self->model(),L"mask_linked"))}}));});
    actionTooltip(data,eye,[weak]{auto self=weak.lock();if(!self)return J{};
        return O({{L"type",S(L"set_layer_visibility")},{L"id",N(self->id)},{L"visible",B(!flag(self->model(),L"visible"))}});});
    actionTooltip(data,content,[weak]{auto self=weak.lock();if(!self)return J{};
        auto target=flag(self->model(),L"group")?O({{L"op",S(L"collapse")},{L"id",N(self->id)}}):O({{L"op",S(L"select")},{L"id",N(self->id)},{L"mask",B(false)}});
        return O({{L"type",S(L"layer")},{L"action",target}});});
    actionTooltip(data,mask,[weak]{auto self=weak.lock();if(!self)return J{};
        return O({{L"type",S(L"layer")},{L"action",O({{L"op",S(L"select")},{L"id",N(self->id)},{L"mask",B(true)}})}});});
    link.Content(icon(L"link",data->theme(),10));link.Height(36);
    name=button(data,data->caption(L"layers",L"layer"),[weak]{if(auto self=weak.lock();self&&self->clickAllowed())self->action(rowSelection(self->id));});
    buttonFeedback(name,data->brush(L"text"),0,0);
    name.FontWeight(Windows::UI::Text::FontWeights::Normal());name.MinHeight(36);name.HorizontalAlignment(HorizontalAlignment::Stretch);name.HorizontalContentAlignment(HorizontalAlignment::Stretch);name.Padding({0});
    StackPanel caption;caption.Margin({6,0,0,0});title=label(data,L"");title.TextTrimming(TextTrimming::CharacterEllipsis);caption.Children().Append(title);
    AutomationProperties::SetAutomationId(title,L"layer-"+to_hstring(uint64_t(id))+L"-label");
    meta=label(data,L"");meta.Opacity(.55);
    AutomationProperties::SetAutomationId(meta,L"layer-"+to_hstring(uint64_t(id))+L"-meta");
    meta.TextTrimming(TextTrimming::CharacterEllipsis);caption.Children().Append(meta);caption.VerticalAlignment(VerticalAlignment::Center);name.Content(caption);
    Grid::SetColumn(name,9);body.Children().Append(name);
    edgeTag(rename,L"edge_contact",S(L"input"));rename.MinWidth(0);rename.MinHeight(data->controlHeight());rename.Height(data->controlHeight());rename.Padding({2,0,2,0});rename.Margin({6,0,2,0});rename.FontSize(data->textSize());
    rename.MaxLength(128);fieldColors(data,rename);rename.Visibility(Visibility::Collapsed);
    copyName(data,rename,data->copyCaption(L"layers",L"name"));Grid::SetColumn(rename,9);body.Children().Append(rename);
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
    root.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
        if(e.Key()==Windows::System::VirtualKey::F2){if(flag(self->model(),L"can_rename"))self->action(O({{L"op",S(L"begin_rename")},{L"id",N(self->id)}}));e.Handled(true);}
    }});
    for(FrameworkElement target:{FrameworkElement(name),FrameworkElement(content),FrameworkElement(mask)})target.KeyDown([weak](auto&& sender,KeyRoutedEventArgs const& e){
        if(e.Key()!=Windows::System::VirtualKey::Application&&!(e.Key()==Windows::System::VirtualKey::F10&&(GetKeyState(VK_SHIFT)&0x8000)))return;
        if(auto self=weak.lock()){
            auto anchor=sender.template as<FrameworkElement>();self->context(anchor==self->mask,anchor);e.Handled(true);
        }
    });
    lockImage.Width(12);lockImage.Height(12);lockImage.HorizontalAlignment(HorizontalAlignment::Left);lockImage.IsHitTestVisible(false);Grid::SetColumn(lockImage,11);body.Children().Append(lockImage);
    grip=pick(L"",12,[]{});grip.MinHeight(0);grip.Width(16);grip.Height(16);grip.Content(icon(L"grip",data->theme()));grip.Opacity(.55);
    copyName(data,grip,data->copyCaption(L"layers",L"move_layer"));
    for(auto const& item:std::array<std::pair<FrameworkElement,wchar_t const*>,8>{{{eye,L"visibility"},{check,L"selection"},{content,L"content"},
        {load,L"load"},{mask,L"mask"},{link,L"link"},{name,L"name"},{grip,L"drag"}}})
        AutomationProperties::SetAutomationId(item.first,L"layer-"+to_hstring(uint64_t(id))+L"-"+item.second);
    AutomationProperties::SetAutomationId(rename,L"layer-"+to_hstring(uint64_t(id))+L"-rename");
    AutomationProperties::SetAutomationId(texture,L"layer-texture-"+to_hstring(uint64_t(id)));
    AutomationProperties::SetAutomationId(texturePreview->root,L"layer-texture-"+to_hstring(uint64_t(id))+L"-preview");
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
    dropMark.IsHitTestVisible(false);dropMark.BorderBrush(accent(data));auto rowPadding=layerRowPadding(data);dropMark.Margin({-rowPadding.Left,-rowPadding.Top,-rowPadding.Right,-rowPadding.Bottom});
    Grid::SetColumnSpan(dropMark,13);body.Children().Append(dropMark);
    swipe(0);
}
double LayerRow::revealWidth()const{return std::min(std::max(1.,root.ActualWidth()),array(model(),L"swipe_actions").Size()*data->panelMetric(L"layer_swipe_action_width"));}
bool LayerRow::canSwipe()const{for(auto value:array(model(),L"swipe_actions"))if(flag(value.GetObject(),L"enabled"))return true;return false;}
void LayerRow::swipe(double offset,bool animate){
    swipeFrames.revoke();
    auto target=std::clamp(offset,object(model(),L"right_swipe").Size()?-data->panelMetric(L"layer_swipe_action_width"):0.,revealWidth());
    if(!animate||!root.IsLoaded()||!Windows::UI::ViewManagement::UISettings().AnimationsEnabled()||target==swipeOffset){positionSwipe(target);return;}
    auto from=swipeOffset;auto start=std::chrono::steady_clock::now();
    swipeFrames=CompositionTarget::Rendering(auto_revoke,[weak=weak_from_this(),from,target,start](auto&&,auto&&){if(auto self=weak.lock()){
        auto t=std::clamp(std::chrono::duration<double,std::milli>(std::chrono::steady_clock::now()-start).count()/150.,0.,1.);
        self->positionSwipe(from+(target-from)*(1-std::pow(1-t,3)));
        if(t>=1)self->swipeFrames.revoke();
    }});
}
void LayerRow::positionSwipe(double offset){
    swipeOffset=std::clamp(offset,object(model(),L"right_swipe").Size()?-data->panelMetric(L"layer_swipe_action_width"):0.,revealWidth());swipeTransform.X(-swipeOffset);
    auto width=revealWidth();swipeActions.Width(width);
    grip.Opacity(.55*(1-std::clamp(std::abs(swipeOffset)/(swipeOffset>0?std::max(1.,width):data->panelMetric(L"layer_swipe_action_width")),0.,1.)));
    RectangleGeometry clip;clip.Rect({float(width-std::max(0.,swipeOffset)),0,float(std::max(0.,swipeOffset)),float(root.ActualHeight())});swipeActions.Clip(clip);
    swipeActions.Visibility(swipeOffset>0?Visibility::Visible:Visibility::Collapsed);
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
void LayerRow::refresh(){
    if(!current())return;auto layer=model();auto view=owner.lock();bool full=!view||view->full;
    check.Visibility(full?Visibility::Visible:Visibility::Collapsed);body.ColumnDefinitions().GetAt(1).Width({full?data->panelMetric(L"control_height")+data->panelMetric(L"compound_spacing"):0.,GridUnitType::Pixel});
    body.Background(flag(layer,L"selected")?selected(data):clear());swipeActions.Background(data->brush(L"bg"));
    auto actions=array(layer,L"swipe_actions");
    auto key=actions.Stringify()+L":"+to_hstring(data->textSize());
    if(key!=swipeKey){swipeKey=key;
    swipeActions.Children().Clear();swipeActions.ColumnDefinitions().Clear();
    for(uint32_t i=0;i<actions.Size();++i){
        auto model=actions.GetObjectAt(i);auto key=std::wstring(str(model,L"id"));
        auto& pick=swipeButtons.try_emplace(key,nullptr).first->second;
        if(!pick){auto weak=weak_from_this();pick=button(data,L"",[weak,key]{if(auto self=weak.lock();self&&self->clickAllowed())for(auto value:array(self->model(),L"swipe_actions")){
            auto model=value.GetObject();if(str(model,L"id")==key&&flag(model,L"enabled")){self->swipe(0);self->action(object(model,L"action"));break;}
        }});pick.MinWidth(0);pick.Padding({4,0,4,0});pick.CornerRadius({0});pick.VerticalAlignment(VerticalAlignment::Stretch);pick.HorizontalAlignment(HorizontalAlignment::Stretch);
            TextBlock caption;caption.MaxLines(1);caption.TextTrimming(TextTrimming::CharacterEllipsis);caption.TextAlignment(TextAlignment::Center);pick.Content(caption);pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            AutomationProperties::SetAutomationId(pick,L"layer-"+to_hstring(uint64_t(id))+L"-swipe-"+hstring(key));}
        pick.Content().as<TextBlock>().Text(str(model,L"label"));pick.IsEnabled(flag(model,L"enabled"));AutomationProperties::SetName(pick,str(model,L"label"));tooltip(pick,str(model,L"label"));
        pick.Background(key==L"delete"?fill({255,192,28,40}):data->tint(L"text",key==L"duplicate"?61:41));
        pick.Foreground(key==L"delete"?fill({255,255,255,255}):data->brush(L"text"));
        pick.FontSize(data->textSize());
        ColumnDefinition column;column.Width({1,GridUnitType::Star});swipeActions.ColumnDefinitions().Append(column);
        Grid::SetColumn(pick,i);swipeActions.Children().Append(pick);
    }
    }
    if((swipeOffset>0&&!canSwipe())||(swipeOffset<0&&!object(layer,L"right_swipe").Size())||renaming)swipe(0);else positionSwipe(swipeOffset);
    title.Text(str(layer,L"label"));AutomationProperties::SetName(name,str(layer,L"label"));
    auto nextTitle=str(layer,L"label");if(nextTitle!=captionTitle){captionTitle=nextTitle;AutomationProperties::SetName(root,data->caption(O({{L"type",S(L"layer_row")},{L"title",S(nextTitle)}})));}
    auto shown=flag(layer,L"visible")&&!flag(layer,L"visibility_blocked")?L"eye":L"eye-hidden";
    eye.Opacity(flag(layer,L"visibility_blocked")?.35:1);
    auto icons=data->theme()+L":"+hstring(shown)+L":"+str(layer,L"selection_icon")+L":"+str(layer,L"content_icon")+L":"+
        to_hstring(flag(layer,L"pass_through"))+L":"+to_hstring(flag(layer,L"adjustment_effect"))+L":"+to_hstring(flag(layer,L"group"))+L":"+to_hstring(flag(layer,L"has_thumbnail"))+L":"+to_hstring(flag(layer,L"collapsed"))+L":"+to_hstring(flag(layer,L"locked"))+L":"+to_hstring(flag(layer,L"mask_linked"));
    if(icons!=iconKey){
        iconKey=icons;eye.Content(icon(shown,data->theme()));check.Content(icon(str(layer,L"selection_icon"),data->theme()));
        auto contentIcon=flag(layer,L"group")?(flag(layer,L"collapsed")?L"folder":L"folder-open"):str(layer,L"content_icon");
        bool preview=flag(layer,L"has_thumbnail"),symbol=!contentIcon.empty()&&!flag(layer,L"selection_layer");
        contentThumbnail.Visibility(preview?Visibility::Visible:Visibility::Collapsed);
        contentSymbol.Visibility(symbol?Visibility::Visible:Visibility::Collapsed);
        contentSymbol.Width(preview?14:24);contentSymbol.Height(preview?14:24);
        contentSymbol.Padding({preview?1.:0.});contentSymbol.CornerRadius({2,2,2,2});
        contentSymbol.HorizontalAlignment(preview?HorizontalAlignment::Right:HorizontalAlignment::Center);
        contentSymbol.VerticalAlignment(preview?VerticalAlignment::Bottom:VerticalAlignment::Center);
        contentSymbol.Margin(preview?Thickness{0,0,3,3}:Thickness{0});
        contentSymbol.Background(preview?data->brush(L"input"):clear());
        if(symbol){
            auto glyph=icon(contentIcon,data->theme(),preview?12:24);
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
    load.Visibility(full&&selectionLayer?Visibility::Visible:Visibility::Collapsed);
    body.ColumnDefinitions().GetAt(5).Width({full&&selectionLayer?32.:0.,GridUnitType::Pixel});
    if(selectionLayer){auto tip=str(layer,L"load_selection_tooltip");AutomationProperties::SetName(load,tip);CapyUi::tooltip(load,tip);}
    AutomationProperties::SetItemStatus(check,flag(layer,L"selected")?data->caption(L"search",L"selected"):data->caption(L"layers",L"unselected"));
    bool hasMask=full&&flag(layer,L"has_mask");
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
    auto paper=object(layer,L"paper");auto paperShown=full&&paper.Size()?Visibility::Visible:Visibility::Collapsed;
    texture.Visibility(paperShown);textureFrame.Visibility(paperShown);textureEdge.update(data,false);
    body.ColumnDefinitions().GetAt(8).Width({full&&paper.Size()?32.:0.,GridUnitType::Pixel});
    if(paper.Size()){texturePreview->show(paper.GetNamedValue(L"id",JsonValue::CreateNullValue()),str(paper,L"label"));AutomationProperties::SetHelpText(texture,str(paper,L"label"));}
    body.ColumnDefinitions().GetAt(2).Width({std::min(24.,num(layer,L"depth")*8),GridUnitType::Pixel});
    lockImage.Opacity(flag(layer,L"locked")||flag(layer,L"alpha_locked")?1:0);
    AutomationProperties::SetName(lockImage,flag(layer,L"locked")?data->caption(L"layers",L"locked"):flag(layer,L"alpha_locked")?data->caption(L"layers",L"alpha_locked"):L"");
    AutomationProperties::SetAccessibilityView(lockImage,flag(layer,L"locked")||flag(layer,L"alpha_locked")?Automation::Peers::AccessibilityView::Content:Automation::Peers::AccessibilityView::Raw);
    body.ColumnDefinitions().GetAt(11).Width({full&&flag(layer,L"can_drop_below")?14.:12.,GridUnitType::Pixel});
    body.ColumnDefinitions().GetAt(12).Width({full&&flag(layer,L"can_drop_below")?16.:0.,GridUnitType::Pixel});
    grip.Visibility(full&&flag(layer,L"can_drop_below")?Visibility::Visible:Visibility::Collapsed);
    grip.IsEnabled(flag(layer,L"drag_from_handle"));grip.IsHitTestVisible(flag(layer,L"drag_from_handle"));
    hstring details=str(layer,L"description");
    meta.Text(details);meta.Visibility(!full||details.empty()?Visibility::Collapsed:Visibility::Visible);
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
    for(bool isMask:{false,true}){
        if(isMask?!flag(layer,L"has_mask"):!flag(layer,L"has_thumbnail"))continue;
        auto item=thumbnail(layer,isMask);visible.push_back(item);
        bool current=false;auto source=LayerThumbnailSource(data->thumbnails,epoch,item,&current);auto shown=isMask?maskThumbnail:contentThumbnail;
        auto preview=isMask?maskPreview:contentPreview;
        if(preview.ImageSource()!=source)preview.ImageSource(source);AutomationProperties::SetItemStatus(shown,current?data->caption(L"header",L"ready"):data->caption(L"layers",L"pending"));
    }
}
}
