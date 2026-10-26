#include "pch.h"
#include "TextureDialog.h"
#include "PickerDialog.h"
#include "BitmapUpload.h"
#include "WorkspaceGeometry.h"
#include "NativeMenus.h"
#include <deque>
#include <set>

namespace CapyUi {
struct PaperPreviewCache : std::enable_shared_from_this<PaperPreviewCache> {
    struct Entry {std::shared_ptr<BitmapUpload> upload;bool ready=false;};
    struct Shown {ImageSource source{nullptr};bool pending=false;};
    PreviewTransport transport;
    Microsoft::UI::Dispatching::DispatcherQueue dispatcher{Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread()};
    Microsoft::UI::Dispatching::DispatcherQueueTimer retry{nullptr};
    std::map<hstring,Entry> entries;
    std::deque<hstring> queue;
    std::set<hstring> missing;
    struct Viewer {std::weak_ptr<void> owner;std::function<void()> refresh;};
    std::vector<Viewer> viewers;
    size_t viewerCleanup=64;
    hstring library;
    bool busy=false;
    ~PaperPreviewCache(){if(retry)retry.Stop();for(auto& [id,entry]:entries)entry.upload->Close();}
    void watch(std::weak_ptr<void> owner,std::function<void()> refresh){
        if(viewers.size()>=viewerCleanup){std::erase_if(viewers,[](auto const& item){return item.owner.expired();});viewerCleanup=std::max(size_t(64),viewers.size()*2);}
        viewers.push_back({std::move(owner),std::move(refresh)});
    }
    Shown find(hstring const& id,hstring const& generation){
        if(generation!=library){library=generation;missing.clear();}
        if(auto found=entries.find(id);found!=entries.end())return {found->second.ready?found->second.upload->Image():nullptr,!found->second.ready};
        if(missing.contains(id))return {};
        entries.emplace(id,Entry{BitmapUpload::Create()});queue.push_back(id);send();
        return {nullptr,true};
    }
    void send(){
        if(busy||queue.empty()||!transport)return;
        std::vector<hstring> batch;A ids;
        while(!queue.empty()&&batch.size()<8){batch.push_back(queue.front());ids.Append(S(queue.front()));queue.pop_front();}
        busy=true;auto weak=weak_from_this();auto target=dispatcher;
        if(transport(CanvasQueryKind::Papers,to_string(O({{L"ids",ids}}).Stringify()),[weak,target,batch](PreviewPacket packet){
            target.TryEnqueue([weak,packet=std::move(packet),batch]{if(auto self=weak.lock())self->receive(packet,batch);});
        }))return;
        busy=false;queue.insert(queue.begin(),batch.begin(),batch.end());
        if(!retry){
            retry=dispatcher.CreateTimer();retry.IsRepeating(false);retry.Interval(std::chrono::milliseconds(120));
            retry.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->send();});
        }
        retry.Start();
    }
    void receive(PreviewPacket const& packet,std::vector<hstring> const& batch){
        busy=false;std::set<hstring> delivered;
        try{
            auto status=packet?J::Parse(to_hstring(NativeString(capy_preview_metadata(packet.get())).get())):J{};
            auto images=array(status,L"images");int size=int(num(status,L"size"));
            if(size>0&&size<=512&&images.Size()<=batch.size()){
                size_t bytes=size_t(size)*size_t(size)*4*images.Size();auto weak=weak_from_this();
                for(auto value:images){
                    auto image=value.GetObject();auto id=str(image,L"id");auto found=entries.find(id);
                    if(found==entries.end()||found->second.ready||std::find(batch.begin(),batch.end(),id)==batch.end()||!delivered.insert(id).second)continue;
                    std::weak_ptr<BitmapUpload> expected=found->second.upload;
                    found->second.upload->Submit({BitmapUpload::ReadPacket(packet),size_t(num(image,L"offset")),bytes,size,size,false,true,
                        [weak,id,expected]{auto self=weak.lock();if(!self)return false;auto found=self->entries.find(id);return found!=self->entries.end()&&found->second.upload==expected.lock();},
                        [weak,id,expected](bool accepted){if(auto self=weak.lock())self->loaded(id,expected,accepted);}});
                }
            }
        }catch(hresult_error const&){}
        bool forgotten=false;
        for(auto const& id:batch)if(!delivered.contains(id)){forget(id);forgotten=true;}
        if(forgotten)notify();
        send();
    }
    void loaded(hstring const& id,std::weak_ptr<BitmapUpload> const& expected,bool accepted){
        auto found=entries.find(id);if(found==entries.end()||found->second.upload!=expected.lock())return;
        if(accepted)found->second.ready=true;else forget(id);
        notify();
    }
    void forget(hstring const& id){
        auto found=entries.find(id);if(found==entries.end())return;
        found->second.upload->Close();entries.erase(found);missing.insert(id);
    }
    void notify(){
        auto current=viewers;
        for(auto const& item:current)if(!item.owner.expired())item.refresh();
        std::erase_if(viewers,[](auto const& item){return item.owner.expired();});
    }
};
namespace {
std::shared_ptr<PaperPreviewCache> const& paperPreviews(std::shared_ptr<WorkspaceData> const& data){
    if(!data->papers){data->papers=std::make_shared<PaperPreviewCache>();data->papers->transport=data->query;}
    return data->papers;
}
PathGeometry squircleShape(double size,double inset=0){
    auto radius=float(size/2);auto geometry=squircleRectangle(float(size),float(size),{radius,radius,radius,radius});
    if(inset){TranslateTransform offset;offset.X(inset);offset.Y(inset);geometry.Transform(offset);}
    return geometry;
}
Shapes::Path squircle(double size){
    Shapes::Path shape;shape.Width(size);shape.Height(size);shape.Data(squircleShape(size));shape.IsHitTestVisible(false);return shape;
}
}
std::shared_ptr<PaperThumbnail> PaperThumbnail::Create(std::shared_ptr<WorkspaceData> const& data,double size,double glyph,bool well){
    auto result=std::make_shared<PaperThumbnail>();result->data=data;result->glyphSize=glyph;
    auto& root=result->root;root.Width(size);root.Height(size);root.IsHitTestVisible(false);root.IsTabStop(false);
    root.HorizontalAlignment(HorizontalAlignment::Center);root.VerticalAlignment(VerticalAlignment::Center);
    result->face.Width(size);result->face.Height(size);root.Content(result->face);
    result->well=squircle(size);result->picture=squircle(size);
    result->well.Fill(well?data->tint(L"text",15):clear());
    result->pixels.Stretch(Stretch::Fill);result->picture.Fill(result->pixels);
    result->glyph.HorizontalAlignment(HorizontalAlignment::Center);result->glyph.VerticalAlignment(VerticalAlignment::Center);
    for(UIElement part:{UIElement(result->well),UIElement(result->picture),UIElement(result->glyph)})result->face.Children().Append(part);
    paperPreviews(data)->watch(result,[weak=std::weak_ptr<PaperThumbnail>(result)]{if(auto viewer=weak.lock())viewer->refresh();});
    return result;
}
PaperPreview paperPreview(std::shared_ptr<WorkspaceData> const& data,hstring const& id){
    auto found=paperPreviews(data)->find(id,str(object(data->model,L"windows_papers"),L"generation"));
    return {found.source,found.pending};
}
void watchPaperPreviews(std::shared_ptr<WorkspaceData> const& data,std::weak_ptr<void> owner,std::function<void()> refresh){
    paperPreviews(data)->watch(std::move(owner),std::move(refresh));
}
void PaperThumbnail::show(V const& id,hstring const& label){shown=id;AutomationProperties::SetName(root,label);refresh();}
void PaperThumbnail::refresh(){
    PaperPreview image;
    if(shown&&shown.ValueType()==JsonValueType::String)image=paperPreview(data,shown.GetString());
    if(pixels.ImageSource()!=image.source)pixels.ImageSource(image.source);
    picture.Visibility(image.source?Visibility::Visible:Visibility::Collapsed);
    glyph.Visibility(image.source?Visibility::Collapsed:Visibility::Visible);
    if(!image.source&&theme!=data->theme()){theme=data->theme();setIcon(glyph,placeholder,theme,glyphSize);}
    AutomationProperties::SetItemStatus(root,image.source?data->caption(L"header",L"ready"):image.pending?data->caption(L"layers",L"pending"):hstring());
}

namespace {
constexpr double TileFrame=102,TilePreview=96,TileColumnGap=12,TileRowGap=14,TextureDialogHeight=600;
struct TexturePicker {J paper;uint32_t selected=0;hstring key;bool enabled=false;};
std::optional<TexturePicker> texturePicker(J const& state,double layer,double epoch){
    if(num(object(state,L"document_file"),L"epoch")!=epoch)return std::nullopt;
    auto view=object(state,L"layer_properties");auto shown=view.GetNamedValue(L"layer",JsonValue::CreateNullValue());
    if(shown.ValueType()!=JsonValueType::Number||shown.GetNumber()!=layer)return std::nullopt;
    for(auto value:array(view,L"controls")){
        auto control=value.GetObject();auto paper=object(control,L"paper");
        if(str(object(control,L"kind"),L"kind")==L"choice"&&paper.Size())
            return TexturePicker{paper,uint32_t(num(object(control,L"value"),L"value")),str(control,L"key"),flag(view,L"enabled")};
    }
    return std::nullopt;
}
}
hstring textureTileId(J const& tile){return tile.GetNamedValue(L"id",JsonValue::CreateNullValue()).Stringify();}

std::shared_ptr<TextureGrid> TextureGrid::Create(std::shared_ptr<WorkspaceData> const& data,hstring const& prefix){
    auto result=std::make_shared<TextureGrid>();result->data=data;result->prefix=prefix;
    auto& grid=result->grid;
    grid.ColumnSpacing(TileColumnGap);grid.RowSpacing(TileRowGap);grid.XYFocusKeyboardNavigation(XYFocusKeyboardNavigationMode::Enabled);
    AutomationProperties::SetAutomationId(grid,prefix+L"-grid");
    grid.SizeChanged([weak=result->weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->arrange();});
    return result;
}
TextureGrid::Tile TextureGrid::make(V const& id,hstring const& text,hstring const& suffix,hstring const& glyph,std::function<void()> click){
    Tile result;
    result.node=button(data,text,std::move(click));
    result.node.HorizontalAlignment(HorizontalAlignment::Stretch);result.node.VerticalAlignment(VerticalAlignment::Top);
    result.node.HorizontalContentAlignment(HorizontalAlignment::Stretch);result.node.Padding({0,0,0,0});
    for(auto state:{L"ButtonBackgroundPointerOver",L"ButtonBackgroundPressed"})result.node.Resources().Insert(box_value(hstring(state)),clear());
    Grid frame;frame.Width(TileFrame);frame.Height(TileFrame);frame.HorizontalAlignment(HorizontalAlignment::Center);
    result.hover=squircle(TileFrame);result.hover.Fill(clear());
    result.picture=PaperThumbnail::Create(data,TilePreview,48,true);result.picture->placeholder=glyph;result.picture->show(id,text);
    result.ring=squircle(TileFrame);
    GeometryGroup edge;edge.FillRule(FillRule::EvenOdd);edge.Children().Append(squircleShape(TileFrame));edge.Children().Append(squircleShape(TileFrame-4,2));
    result.ring.Data(edge);result.ring.Fill(accent(data));result.ring.Opacity(0);
    for(UIElement part:{UIElement(result.hover),UIElement(result.picture->root),UIElement(result.ring)})frame.Children().Append(part);
    AutomationProperties::SetAutomationId(result.picture->root,prefix+L"-preview-"+suffix);
    result.caption=label(data,text,false,false);result.caption.FontSize(data->textSize()*.82);result.caption.LineHeight(0);result.caption.LineStackingStrategy(LineStackingStrategy::MaxHeight);
    result.caption.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());result.caption.TextAlignment(TextAlignment::Center);result.caption.HorizontalAlignment(HorizontalAlignment::Stretch);
    result.caption.TextWrapping(TextWrapping::Wrap);result.caption.TextTrimming(TextTrimming::CharacterEllipsis);result.caption.MaxLines(2);
    StackPanel content;content.Spacing(6);content.Children().Append(frame);content.Children().Append(result.caption);result.node.Content(content);
    AutomationProperties::SetAutomationId(result.node,prefix+L"-tile-"+suffix);tooltip(result.node,text);
    auto hover=result.hover;
    result.node.PointerEntered([hover,data=data](auto&&,auto&&){hover.Fill(data->tint(L"text",26));});
    auto leave=[hover](auto&&,auto&&){hover.Fill(clear());};
    result.node.PointerExited(leave);result.node.PointerCanceled(leave);result.node.PointerCaptureLost(leave);
    return result;
}
void TextureGrid::trail(hstring const& suffix,hstring const& glyph,std::function<void()> click){
    trailing=make(JsonValue::CreateNullValue(),L"",suffix,glyph,std::move(click));
    AutomationProperties::SetAutomationId(trailing->node,prefix+L"-"+suffix);
    grid.Children().Append(trailing->node);columns=0;arrange();
}
void TextureGrid::rename(Tile const& target,hstring const& text){
    target.caption.Text(text);tooltip(target.node,text);AutomationProperties::SetName(target.node,text);AutomationProperties::SetName(target.picture->root,text);
}
void TextureGrid::rebuild(A const& list){
    auto weak=weak_from_this();
    grid.Children().Clear();tiles.clear();
    for(uint32_t index=0;index<list.Size();++index){
        auto item=list.GetObjectAt(index);
        auto tile=make(item.GetNamedValue(L"id",JsonValue::CreateNullValue()),str(item,L"label"),to_hstring(index),L"layer-paper-symbolic",
            [weak,index]{if(auto self=weak.lock();self&&self->picked)self->picked(index);});
        tile.id=textureTileId(item);
        tile.node.DoubleTapped([weak,index](auto&&,DoubleTappedRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->activated){e.Handled(true);self->activated(index);}});
        if(auto actions=array(item,L"actions");actions.Size()){
            MenuFlyout menu;TrackPopup(menu,data);AutomationProperties::SetAutomationId(menu,L"paper-tile-menu");
            for(auto value:actions){
                auto spec=value.GetObject();auto action=object(spec,L"action");MenuFlyoutItem command;command.Text(str(spec,L"label"));
                AutomationProperties::SetAutomationId(command,L"paper-"+str(action,L"op")+L"-"+to_hstring(index));
                command.Click([weak,action](auto&&,auto&&){if(auto self=weak.lock();self&&self->act)self->act(action);});menu.Items().Append(command);
            }
            tile.node.ContextFlyout(menu);
        }
        grid.Children().Append(tile.node);tiles.push_back(std::move(tile));
    }
    if(trailing)grid.Children().Append(trailing->node);
    columns=0;arrange();
}
void TextureGrid::arrange(){
    std::vector<Button> nodes;for(auto const& tile:tiles)nodes.push_back(tile.node);
    if(trailing)nodes.push_back(trailing->node);
    double width=grid.ActualWidth();if(width<=0||nodes.empty())return;
    int next=std::clamp(int((width+TileColumnGap)/(TileFrame+TileColumnGap)),1,6);
    if(next!=columns){
        columns=next;grid.ColumnDefinitions().Clear();
        for(int i=0;i<columns;++i){ColumnDefinition column;column.Width({1,GridUnitType::Star});grid.ColumnDefinitions().Append(column);}
    }
    auto rows=int((nodes.size()+columns-1)/columns);
    while(int(grid.RowDefinitions().Size())<rows){RowDefinition row;row.Height({1,GridUnitType::Auto});grid.RowDefinitions().Append(row);}
    while(int(grid.RowDefinitions().Size())>rows)grid.RowDefinitions().RemoveAtEnd();
    for(size_t i=0;i<nodes.size();++i){Grid::SetColumn(nodes[i],int(i)%columns);Grid::SetRow(nodes[i],int(i)/columns);}
}
void TextureGrid::mark(hstring const& id){
    for(auto const& tile:tiles){
        bool on=tile.id==id;tile.ring.Opacity(on?1:0);
        AutomationProperties::SetItemStatus(tile.node,on?data->caption(L"search",L"selected"):hstring());
    }
}
bool TextureGrid::focus(hstring const& id){
    for(auto const& tile:tiles)if(tile.id==id){
        if(!tile.node.Focus(FocusState::Programmatic))return false;
        BringIntoViewOptions options;options.AnimationDesired(false);options.VerticalAlignmentRatio(.5);tile.node.StartBringIntoView(options);return true;
    }
    return false;
}

namespace {
struct TextureDialog;
std::vector<std::shared_ptr<TextureDialog>>& openDialogs(){thread_local std::vector<std::shared_ptr<TextureDialog>> dialogs;return dialogs;}

struct TextureDialog : std::enable_shared_from_this<TextureDialog> {
    std::shared_ptr<WorkspaceData> data;
    double layer=0,epoch=0;
    ContentDialog dialog{nullptr};
    Grid root,footer;
    std::shared_ptr<TextureGrid> tiles;
    StackPanel page;
    ScrollViewer scroll;
    TextBlock title;
    Button import{nullptr},cancel{nullptr},apply{nullptr};
    J paper;
    hstring shown,pending;
    std::optional<J> renaming,importing;
    uint64_t listener=0;
    bool chosen=false,accepting=false,finished=false,focusPending=false;
    XamlRoot::Changed_revoker resized;

    void init(XamlRoot const& xaml){
        auto weak=weak_from_this();
        dialog=pickerDialog(data,xaml,root,scroll,footer,L"texture-dialog");
        pickerTitle(data,title,L"texture-title");
        tiles=TextureGrid::Create(data,L"texture");
        tiles->picked=[weak](uint32_t index){if(auto self=weak.lock();self&&index<self->tiles->tiles.size()){self->pending=self->tiles->tiles[index].id;self->tiles->mark(self->pending);}};
        tiles->act=[weak](J action){if(auto self=weak.lock())self->act(action);};
        page.Padding({PickerPadding,PickerPadding,PickerPadding,PickerPadding});page.Spacing(14);
        page.Children().Append(title);page.Children().Append(tiles->grid);
        scroll.Content(page);scroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);mouseScrollbars(scroll);
        pickerFooter(data,footer,{GridUnitType::Auto,GridUnitType::Star,GridUnitType::Auto,GridUnitType::Auto});
        import=pickerButton(data,L"texture-import",0,false,[weak]{if(auto self=weak.lock()){self->importing=object(self->paper,L"import");self->close(false);}});
        cancel=pickerButton(data,L"texture-cancel",2,false,[weak]{if(auto self=weak.lock())self->close(false);});
        apply=pickerButton(data,L"texture-apply",3,true,[weak]{if(auto self=weak.lock())self->close(true);});
        import.Visibility(Visibility::Collapsed);
        for(auto const& node:{import,cancel,apply})footer.Children().Append(node);
        root.Height(TextureDialogHeight);
        root.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock();self&&e.Key()==Windows::System::VirtualKey::Enter){e.Handled(true);self->close(true);}});
        resized=xaml.Changed(auto_revoke,[weak](auto&&,auto&&){if(auto self=weak.lock())fitPicker(self->root);});
        dialog.Opened([weak](auto&&,auto&&){if(auto self=weak.lock()){fitPicker(self->root);self->focusPending=true;self->focus();}});
        dialog.Closed([weak](auto&&,auto&&){if(auto self=weak.lock()){
            self->data->popup(false);
            if(auto action=std::exchange(self->renaming,std::nullopt))self->rename(*action);else self->finish();
        }});
        listener=data->colorView([weak]{if(auto self=weak.lock())self->update();});
        data->copyView([weak]{if(auto self=weak.lock();self&&!self->finished){self->relabel();return true;}return false;});
        relabel();fitPicker(root);update();
    }
    bool current()const{return num(object(data->state,L"document_file"),L"epoch")==epoch;}
    void relabel(){
        auto text=data->caption(L"paper",L"choose");
        title.Text(text);AutomationProperties::SetName(dialog,text);AutomationProperties::SetName(root,text);AutomationProperties::SetName(tiles->grid,text);
        pickerLabel(cancel,data->common(L"cancel"));pickerLabel(apply,data->caption(L"paper",L"use_texture"));
        if(auto action=object(paper,L"import");action.Size())pickerLabel(import,str(action,L"label"));
    }
    void update(){
        if(finished)return;
        dialogSurface(data,dialog);
        auto picker=texturePicker(data->state,layer,epoch);if(!picker)return;
        if(auto key=picker->paper.Stringify();key!=shown){
            shown=key;paper=picker->paper;tiles->rebuild(array(paper,L"tiles"));
            auto const& built=tiles->tiles;
            bool kept=chosen&&std::any_of(built.begin(),built.end(),[this](TextureGrid::Tile const& tile){return tile.id==pending;});
            if(!kept&&picker->selected<built.size())pending=built[picker->selected].id;
            chosen=true;
            auto action=object(paper,L"import");import.Visibility(action.Size()?Visibility::Visible:Visibility::Collapsed);relabel();
        }
        tiles->mark(pending);focus();
    }
    void focus(){
        if(focusPending&&tiles->focus(pending))focusPending=false;
    }
    void act(J action){
        if(finished||!current())return;
        if(str(action,L"op")==L"rename_paper"){renaming=action;dialog.Hide();return;}
        data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",action}}),to_hstring(uint64_t(epoch)));
    }
    fire_and_forget rename(J action){
        auto lifetime=shared_from_this();
        ContentDialog prompt;prompt.XamlRoot(dialog.XamlRoot());prompt.Title(box_value(data->caption(L"paper",L"rename_title")));
        prompt.PrimaryButtonText(data->common(L"save"));prompt.CloseButtonText(data->common(L"cancel"));prompt.DefaultButton(ContentDialogButton::Primary);
        dialogSurface(data,prompt);
        AutomationProperties::SetAutomationId(prompt,L"paper-rename-dialog");
        TextBox input;input.Header(box_value(data->common(L"name")));input.Text(str(action,L"name"));input.MaxLength(64);AutomationProperties::SetAutomationId(input,L"paper-rename-name");
        auto validate=[source=data,prompt=make_weak(prompt),input=make_weak(input)]{auto view=prompt.get();auto entry=input.get();if(view&&entry)view.IsPrimaryButtonEnabled(source->caption(O({{L"type",S(L"paper_name_error")},{L"name",S(entry.Text())}})).empty());};
        input.TextChanged([validate](auto&&,auto&&){validate();});validate();prompt.Content(input);
        input.Loaded([weak=make_weak(input)](auto&&,auto&&){if(auto input=weak.get()){input.Focus(FocusState::Programmatic);input.SelectAll();}});
        data->popup(true);bool saved=false;
        try{saved=co_await prompt.ShowAsync()==ContentDialogResult::Primary;}catch(hresult_error const&){}
        data->popup(false);
        if(saved&&texturePicker(data->state,layer,epoch)){
            action.Insert(L"name",S(input.Text()));data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",action}}),to_hstring(uint64_t(epoch)));
        }
        present();
    }
    void present(){
        if(finished)return;
        data->popup(true);focusPending=true;
        try{dialog.ShowAsync();}catch(hresult_error const&){data->popup(false);finish();}
    }
    void close(bool accept){accepting=accept;dialog.Hide();}
    void finish(){
        if(std::exchange(finished,true))return;
        if(listener)data->colorViews.erase(listener);
        resized.revoke();
        auto picker=texturePicker(data->state,layer,epoch);
        auto epochText=to_hstring(uint64_t(epoch));
        if(importing&&importing->Size()&&picker)data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",object(*importing,L"action")}}),epochText);
        else if(accepting&&(!picker||!picker->enabled))data->dispatch(O({{L"windows_failure",S(L"action_failed")}}));
        else if(accepting){
            auto list=array(picker->paper,L"tiles");
            for(uint32_t index=0;index<list.Size();++index){
                if(textureTileId(list.GetObjectAt(index))!=pending)continue;
                if(index!=picker->selected)data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",O({{L"op",S(L"set")},{L"layer",N(layer)},{L"key",S(picker->key)},
                    {L"value",O({{L"kind",S(L"choice")},{L"value",N(index)}})}})}}),epochText);
                break;
            }
        }
        auto& open=openDialogs();auto self=shared_from_this();
        open.erase(std::remove(open.begin(),open.end(),self),open.end());
    }
};
}

void ChooseTexture(std::shared_ptr<WorkspaceData> const& data,UIElement const& owner,double layer){
    auto xaml=owner.XamlRoot();if(!xaml||!openDialogs().empty())return;
    auto dialog=std::make_shared<TextureDialog>();dialog->data=data;dialog->layer=layer;
    dialog->epoch=num(object(data->state,L"document_file"),L"epoch");
    dialog->init(xaml);openDialogs().push_back(dialog);dialog->present();
}
}
