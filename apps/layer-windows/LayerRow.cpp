#include "pch.h"
#include "LayersView.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <array>

using namespace CapyLayers;
namespace CapyLayers {
namespace {
double renameTarget(std::shared_ptr<WorkspaceData> const& data){
    auto value=object(data->state,L"layer_tools").GetNamedValue(L"rename_layer",JsonValue::CreateNullValue());
    return value.ValueType()==JsonValueType::Number?value.GetNumber():-1;
}
void corners(Canvas const& canvas){
    canvas.Width(30);canvas.Height(30);canvas.IsHitTestVisible(false);
    for(auto corner:std::array<std::array<double,4>,4>{{{1,1,1,1},{29,1,-1,1},{1,29,1,-1},{29,29,-1,-1}}})
        for(int pass=0;pass<2;pass++)for(int axis=0;axis<2;axis++){
            Shapes::Line line;line.X1(corner[0]);line.Y1(corner[1]);
            line.X2(corner[0]+(axis==0?corner[2]*6:0));line.Y2(corner[1]+(axis==1?corner[3]*6:0));
            line.Stroke(fill(pass?Windows::UI::Color{255,255,255,255}:Windows::UI::Color{255,0,0,0}));
            line.StrokeThickness(pass?1:3);canvas.Children().Append(line);
        }
}
LayerThumbnail thumbnail(J const& layer,bool mask){
    return {to_hstring(uint64_t(num(layer,L"id"))),to_hstring(uint64_t(num(layer,mask?L"mask_id":L"id"))),
        to_hstring(uint64_t(num(layer,mask?L"mask_revision":L"paint_revision"))),mask};
}
}
J LayerRow::model()const{return findId(array(data->state,L"layers"),id);}
bool LayerRow::current()const{return epoch==epochOf(data)&&model().Size()!=0;}
void LayerRow::action(J operation){if(!data->updating&&current())data->dispatchDocument(layerAction(operation),epoch);}
void LayerRow::context(bool isMask,UIElement const& anchor){if(current())if(auto view=owner.lock())view->context(id,isMask,anchor);}
void LayerRow::init(){
    auto weak=weak_from_this();root.Child(body);root.MinHeight(40);root.Padding({6,2,6,2});root.BorderThickness({0});
    root.BorderBrush(fill({255,53,132,228}));body.ColumnSpacing(0);body.VerticalAlignment(VerticalAlignment::Center);
    AutomationProperties::SetAutomationId(root,L"layer-row-"+to_hstring(uint64_t(id)));
    AutomationProperties::SetName(root,L"Layer row");
    // Include the two-DIP gaps only beside visible flex items. Empty mask and
    // indentation columns must not add their own gaps.
    for(auto width:{26.,26.,0.,5.,32.,14.,32.,-1.,14.,12.}){
        ColumnDefinition column;column.Width({width<0?1:width,width<0?GridUnitType::Star:GridUnitType::Pixel});
        body.ColumnDefinitions().Append(column);
    }
    auto pick=[&](hstring const& title,int column,std::function<void()> action){
        auto control=button(data,title,std::move(action));control.Width(column==4||column==6?30:column==5||column==9?12:24);
        control.Height(30);control.HorizontalAlignment(HorizontalAlignment::Left);Grid::SetColumn(control,column);body.Children().Append(control);return control;
    };
    eye=pick(L"Layer visibility",0,[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"visibility")},{L"id",N(self->id)},{L"value",B(!flag(self->model(),L"visible"))}}));});
    check=pick(L"Select layer without changing drawing target",1,[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"toggle_selection")},{L"id",N(self->id)}}));});
    for(auto control:{eye,check}){control.ClearValue(FrameworkElement::HeightProperty());control.MinHeight(24);control.VerticalAlignment(VerticalAlignment::Stretch);}
    Grid::SetColumn(indent,2);body.Children().Append(indent);clip.Width(3);clip.Height(28);clip.CornerRadius({1,1,1,1});clip.Background(fill({255,233,153,165}));clip.HorizontalAlignment(HorizontalAlignment::Left);
    Grid::SetColumn(clip,3);body.Children().Append(clip);
    content=pick(L"Edit layer content",4,[weak]{if(auto self=weak.lock()){
        auto layer=self->model();self->action(flag(layer,L"group")?O({{L"op",S(L"collapse")},{L"id",N(self->id)}}):
            O({{L"op",S(L"select")},{L"id",N(self->id)},{L"mask",B(false)}}));
    }});
    mask=pick(L"Edit layer mask",6,[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"select")},{L"id",N(self->id)},{L"mask",B(true)}}));});
    for(auto pair:{std::pair{contentImage,contentTile},std::pair{maskImage,maskTile}}){
        pair.first.Width(28);pair.first.Height(28);pair.first.Stretch(Stretch::Fill);pair.first.IsHitTestVisible(false);pair.second.Children().Append(pair.first);
    }
    corners(contentCorners);corners(maskCorners);contentTile.Children().Append(contentCorners);maskTile.Children().Append(maskCorners);
    content.Content(contentTile);mask.Content(maskTile);
    content.CornerRadius({3,3,3,3});mask.CornerRadius({3,3,3,3});
    link=pick(L"Link layer mask",5,[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"link_mask")},{L"id",N(self->id)},{L"value",B(!flag(self->model(),L"mask_linked"))}}));});
    link.Content(icon(L"link",data->theme(),12));
    name=button(data,L"Layer",[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"select")},{L"id",N(self->id)},{L"mask",B(false)}}));});
    name.MinHeight(36);name.HorizontalAlignment(HorizontalAlignment::Stretch);name.HorizontalContentAlignment(HorizontalAlignment::Stretch);
    name.FontWeight(Windows::UI::Text::FontWeights::Normal());name.Padding({0});name.Margin({6,0,2,0});
    StackPanel caption;title=label(data,L"");title.TextTrimming(TextTrimming::CharacterEllipsis);caption.Children().Append(title);
    title.LineHeight(20);AutomationProperties::SetAutomationId(title,L"layer-"+to_hstring(uint64_t(id))+L"-label");
    meta=label(data,L"");meta.Opacity(.55);meta.LineHeight(20);
    AutomationProperties::SetAutomationId(meta,L"layer-"+to_hstring(uint64_t(id))+L"-meta");
    meta.TextTrimming(TextTrimming::CharacterEllipsis);caption.Children().Append(meta);name.Content(caption);
    Grid::SetColumn(name,7);body.Children().Append(name);
    rename.MinWidth(0);rename.MinHeight(24);rename.Height(24);rename.Padding({2,0,2,0});rename.Margin({6,0,2,0});rename.FontSize(data->textSize());
    rename.MaxLength(256);rename.Background(data->brush(L"input"));rename.Visibility(Visibility::Collapsed);
    AutomationProperties::SetName(rename,L"Layer name");Grid::SetColumn(rename,7);body.Children().Append(rename);
    rename.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
        if(e.Key()==Windows::System::VirtualKey::Enter){self->commit(false);e.Handled(true);}
        if(e.Key()==Windows::System::VirtualKey::Escape){self->commit(true);e.Handled(true);}
    }});
    rename.TextChanging([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating)self->committing=false;});
    rename.LosingFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->commit(false);});
    rename.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->commit(false);});
    name.DoubleTapped([weak](auto&&,DoubleTappedRoutedEventArgs const& e){if(auto self=weak.lock())self->action(O({{L"op",S(L"begin_rename")},{L"id",N(self->id)}}));e.Handled(true);});
    name.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
        if(e.Key()==Windows::System::VirtualKey::F2){self->action(O({{L"op",S(L"begin_rename")},{L"id",N(self->id)}}));e.Handled(true);}
        else if(e.Key()==Windows::System::VirtualKey::Application||(e.Key()==Windows::System::VirtualKey::F10&&(GetKeyState(VK_SHIFT)&0x8000))){
            self->context(false,self->name);e.Handled(true);
        }
    }});
    lockImage.Width(12);lockImage.Height(12);lockImage.HorizontalAlignment(HorizontalAlignment::Left);lockImage.IsHitTestVisible(false);Grid::SetColumn(lockImage,8);body.Children().Append(lockImage);
    grip=pick(L"Drag layer",9,[]{});grip.Height(12);grip.Content(icon(L"grip",data->theme(),12));grip.Opacity(.6);
    for(auto item:{std::pair{eye,L"visibility"},std::pair{check,L"selection"},std::pair{content,L"content"},
        std::pair{mask,L"mask"},std::pair{link,L"link"},std::pair{name,L"name"},std::pair{grip,L"drag"}})
        AutomationProperties::SetAutomationId(item.first,L"layer-"+to_hstring(uint64_t(id))+L"-"+item.second);
    AutomationProperties::SetAutomationId(rename,L"layer-"+to_hstring(uint64_t(id))+L"-rename");
    AutomationProperties::SetName(contentImage,L"Layer preview");AutomationProperties::SetName(maskImage,L"Layer mask preview");
    AutomationProperties::SetAutomationId(contentImage,L"layer-"+to_hstring(uint64_t(id))+L"-thumbnail");
    AutomationProperties::SetAutomationId(maskImage,L"layer-"+to_hstring(uint64_t(id))+L"-mask-thumbnail");
    root.RightTapped([weak](auto&&,RightTappedRoutedEventArgs const& e){if(auto self=weak.lock())self->context(false,self->root);e.Handled(true);});
    mask.RightTapped([weak](auto&&,RightTappedRoutedEventArgs const& e){if(auto self=weak.lock())self->context(true,self->mask);e.Handled(true);});
    dragSource(name);dragSource(content);dragSource(mask);dragSource(grip);
    dropMark.IsHitTestVisible(false);dropMark.BorderBrush(fill({255,53,132,228}));dropMark.Margin({-6,-2,-6,-2});
    Grid::SetColumnSpan(dropMark,10);body.Children().Append(dropMark);
    root.AllowDrop(true);
    auto over=[weak](auto&&,DragEventArgs const& e){if(auto self=weak.lock()){
        auto view=self->owner.lock();auto layer=self->model();
        if(!self->current()||!view||!view->dragCurrent()||*view->dragged==self->id){e.AcceptedOperation(Windows::ApplicationModel::DataTransfer::DataPackageOperation::None);return;}
        float fraction=flag(layer,L"can_drop_below")?std::clamp(e.GetPosition(self->root).Y/float(std::max(1.,self->root.ActualHeight())),0.f,1.f):0.f;
        bool into=flag(layer,L"group")&&fraction>=.25f&&fraction<.75f;
        self->highlight(into?3:fraction<.5f?1:2);
        e.AcceptedOperation(Windows::ApplicationModel::DataTransfer::DataPackageOperation::Move);
        e.DragUIOverride().Caption((into?L"Move into ":fraction<.5f?L"Move above ":L"Move below ")+str(layer,L"label"));
        e.Handled(true);
    }};
    root.DragEnter(over);root.DragOver(over);
    root.DragLeave([weak](auto&&,auto&&){if(auto self=weak.lock())self->highlight(0);});
    root.Drop([weak](auto&&,DragEventArgs const& e){if(auto self=weak.lock()){
        auto view=self->owner.lock();if(!view||!view->dragCurrent()||!self->current())return;
        float fraction=flag(self->model(),L"can_drop_below")?std::clamp(e.GetPosition(self->root).Y/float(std::max(1.,self->root.ActualHeight())),0.f,1.f):0.f;
        self->action(O({{L"op",S(L"drop")},{L"id",N(*view->dragged)},{L"target",N(self->id)},{L"fraction",N(fraction)}}));
        view->clearDrag();e.AcceptedOperation(Windows::ApplicationModel::DataTransfer::DataPackageOperation::Move);e.Handled(true);
    }});
}
void LayerRow::dragSource(UIElement const& source){
    auto weak=weak_from_this();source.CanDrag(true);
    source.DragStarting([weak](auto&&,DragStartingEventArgs const& e){
        auto self=weak.lock();auto view=self?self->owner.lock():nullptr;
        if(!self||!view||!self->current()||self->renaming||!flag(self->model(),L"can_drop_below")||flag(self->model(),L"locked")){e.Cancel(true);return;}
        view->dragged=self->id;view->dragEpoch=self->epoch;
        // This format carries no document pixels, names, or filesystem paths.
        e.Data().SetData(L"CapyCanvas.InternalLayer",box_value(L"layer"));
        e.AllowedOperations(Windows::ApplicationModel::DataTransfer::DataPackageOperation::Move);
    });
    source.DropCompleted([view=owner](auto&&,auto&&){if(auto self=view.lock())self->clearDrag();});
}
void LayerRow::commit(bool cancel){
    if(!renaming||committing||data->updating||!current())return;
    if(renameTarget(data)!=id)return;
    committing=true;
    action(cancel?O({{L"op",S(L"cancel_rename")}}):O({{L"op",S(L"rename")},{L"id",N(id)},{L"name",S(rename.Text())}}));
}
void LayerRow::highlight(int position){
    // Keep row geometry stable throughout the drag.
    dropMark.BorderThickness(position==3?Thickness{2,2,2,2}:position==1?Thickness{0,2,0,0}:position==2?Thickness{0,0,0,2}:Thickness{0});
    AutomationProperties::SetHelpText(root,position==3?L"Move into group":position==1?L"Move above layer":position==2?L"Move below layer":L"");
}
void LayerRow::refresh(){
    if(!current())return;auto layer=model();
    root.Background(flag(layer,L"selected")?selected():clear());
    title.Text(str(layer,L"label"));AutomationProperties::SetName(name,str(layer,L"label"));
    AutomationProperties::SetName(root,str(layer,L"label")+L" layer row");
    auto shown=flag(layer,L"visible")?L"eye":L"eye-hidden";
    auto icons=hstring(shown)+L":"+str(layer,L"selection_icon")+L":"+str(layer,L"content_icon")+L":"+
        to_hstring(flag(layer,L"group"))+L":"+to_hstring(flag(layer,L"collapsed"))+L":"+to_hstring(flag(layer,L"locked"));
    if(icons!=iconKey){
        iconKey=icons;eye.Content(icon(shown,data->theme()));check.Content(icon(str(layer,L"selection_icon"),data->theme()));
        auto contentIcon=flag(layer,L"group")?(flag(layer,L"collapsed")?L"folder":L"folder-open"):str(layer,L"content_icon");
        if(!contentIcon.empty())contentImage.Source(icon(contentIcon,data->theme(),28).Source());else contentImage.Source(nullptr);
        lockImage.Source(icon(flag(layer,L"locked")?L"lock":L"alpha-lock",data->theme(),12).Source());
    }
    AutomationProperties::SetName(eye,flag(layer,L"visible")?L"Hide layer":L"Show layer");
    AutomationProperties::SetItemStatus(check,flag(layer,L"selected")?L"Selected":L"Unselected");
    bool hasMask=flag(layer,L"has_mask"),group=flag(layer,L"group");
    content.Background(group?clear():data->brush(L"input"));mask.Background(data->brush(L"input"));
    contentCorners.Visibility(flag(layer,L"editing")&&!flag(layer,L"mask_selected")?Visibility::Visible:Visibility::Collapsed);
    maskCorners.Visibility(flag(layer,L"mask_selected")?Visibility::Visible:Visibility::Collapsed);
    mask.Visibility(hasMask?Visibility::Visible:Visibility::Collapsed);link.Visibility(hasMask?Visibility::Visible:Visibility::Collapsed);
    body.ColumnDefinitions().GetAt(5).Width({hasMask?14.:0.,GridUnitType::Pixel});
    body.ColumnDefinitions().GetAt(6).Width({hasMask?32.:0.,GridUnitType::Pixel});
    link.Opacity(flag(layer,L"mask_linked")?1:.35);link.IsEnabled(!flag(layer,L"locked"));
    maskImage.Opacity(flag(layer,L"mask_enabled")?1:.4);
    body.ColumnDefinitions().GetAt(2).Width({std::min(24.,num(layer,L"depth")*8),GridUnitType::Pixel});
    clip.Opacity(flag(layer,L"clipped")?1:0);lockImage.Opacity(flag(layer,L"locked")||flag(layer,L"alpha_locked")?1:0);
    body.ColumnDefinitions().GetAt(8).Width({flag(layer,L"can_drop_below")?14.:12.,GridUnitType::Pixel});
    body.ColumnDefinitions().GetAt(9).Width({flag(layer,L"can_drop_below")?12.:0.,GridUnitType::Pixel});
    grip.Visibility(flag(layer,L"can_drop_below")?Visibility::Visible:Visibility::Collapsed);
    grip.Opacity(flag(layer,L"can_drop_below")?.6:0);grip.IsEnabled(flag(layer,L"can_drop_below")&&!flag(layer,L"locked"));
    hstring details=num(layer,L"blend")?str(layer,L"blend_label"):L"";
    if(num(layer,L"opacity",1)<1){if(!details.empty())details=details+L" · ";details=details+to_hstring(int(std::round(num(layer,L"opacity")*100)))+L"%";}
    meta.Text(details);meta.Visibility(details.empty()?Visibility::Collapsed:Visibility::Visible);
    bool editing=renameTarget(data)==id;
    if(editing&&!renaming){renaming=true;committing=false;rename.Text(str(layer,L"label"));rename.Visibility(Visibility::Visible);name.Visibility(Visibility::Collapsed);
        rename.Focus(FocusState::Programmatic);rename.SelectAll();}
    else if(!editing&&renaming){renaming=false;committing=false;rename.Visibility(Visibility::Collapsed);name.Visibility(Visibility::Visible);}
}
void LayerRow::thumbnails(std::vector<LayerThumbnail>& visible){
    if(!current())return;auto layer=model();
    for(bool isMask:{false,true}){
        if(isMask?!flag(layer,L"has_mask"):(flag(layer,L"group")||!str(layer,L"content_icon").empty()))continue;
        auto item=thumbnail(layer,isMask);visible.push_back(item);
        auto source=LayerThumbnailSource(data->thumbnails,epoch,item);auto image=isMask?maskImage:contentImage;
        if(image.Source()!=source)image.Source(source);AutomationProperties::SetItemStatus(image,source?L"Ready":L"Pending");
    }
}
}
