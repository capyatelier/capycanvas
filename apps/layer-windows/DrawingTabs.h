#pragma once
#include "UiControls.h"
#include "NativeMenus.h"
#include "WorkspaceRowDrag.h"
#include "WorkspaceGeometry.h"
#include <winrt/Microsoft.UI.Input.h>
#include <winrt/Microsoft.UI.Composition.h>
#include <winrt/Windows.UI.ViewManagement.h>
#include <optional>
#include <set>
#include <string_view>

namespace CapyUi {
struct DrawingTabs:std::enable_shared_from_this<DrawingTabs>{
    using K=Windows::System::VirtualKey;
    std::shared_ptr<WorkspaceData> data;Grid root;TextBlock plain;StackPanel strip;Button selector;Flyout popup;Grid listSurface;ListView list;
    struct Tab{Grid box;Button select,close;TextBlock text;MenuFlyout menu;};
    struct Row{ListViewItem item;Button grip,remove;TextBlock text,location;MenuFlyout menu;};
    std::map<uint64_t,Tab> tabs;std::map<uint64_t,Row> rows;std::unique_ptr<WorkspaceRowDrag> rowDrag;
    struct SlideCopy{uint64_t id=0;Border copy;double target=0;Microsoft::UI::Composition::Vector3KeyFrameAnimation animation{nullptr};};
    Canvas overlay;std::vector<SlideCopy> copies;A slideOrder,slideHits;J slideClip,slide;bool animateSlide=true;
    std::optional<uint32_t> pointer;uint64_t source=0;Windows::Foundation::Point origin{},position{};
    Microsoft::UI::Input::GestureRecognizer hold;bool recognizing=false,dragging=false,held=false,updating=false,showing=false;double slopX=4,slopY=4;
    uint64_t selectedModel=0;
    hstring epoch;J model(){return object(data->model,L"windows_tabs");}
    bool available(){return flag(model(),L"available")&&!flag(object(data->model,L"header"),L"editing");}
    bool single(){return array(model(),L"tabs").Size()<2;}
    hstring plainTitle(){
        auto all=array(data->state,L"tabs");if(!all.Size())return L"Capy Canvas";auto tab=all.GetObjectAt(0);
        return str(tab,L"title")+L" · "+to_hstring(int64_t(num(tab,L"width")))+L" × "+to_hstring(int64_t(num(tab,L"height")));
    }
    static hstring marked(J const& spec){return (flag(spec,L"modified")?hstring(L"• "):hstring())+str(spec,L"title");}
    std::optional<uint64_t> neighbor(uint64_t id,K key){
        std::vector<uint64_t> ids;for(auto v:array(model(),L"tabs"))ids.push_back(uint64_t(num(v.GetObject(),L"id")));
        auto at=std::find(ids.begin(),ids.end(),id);if(at==ids.end()||ids.empty())return std::nullopt;auto index=size_t(at-ids.begin());
        if(key==K::Home)return ids.front();if(key==K::End)return ids.back();
        return ids[(index+(key==K::Right?1:ids.size()-1))%ids.size()];
    }
    void tabKey(uint64_t id,KeyRoutedEventArgs const& e){
        if(!available()||pointer)return;auto key=e.Key();
        bool control=GetKeyState(VK_CONTROL)&0x8000,shift=GetKeyState(VK_SHIFT)&0x8000;
        if(control&&shift&&(key==K::Left||key==K::Right)){send(O({{L"op",S(L"step")},{L"id",N(double(id))},{L"forward",B(key==K::Right)}}));e.Handled(true);return;}
        if(!control&&!shift&&key==K::Delete){close(id);e.Handled(true);return;}
        if(control||shift||(key!=K::Left&&key!=K::Right&&key!=K::Home&&key!=K::End))return;
        if(auto next=neighbor(id,key)){select(*next);if(tabs.contains(*next))tabs.at(*next).select.Focus(FocusState::Keyboard);}e.Handled(true);
    }
    void send(J action){data->document(to_string(O({{L"operation",S(L"tabs")},{L"action",action}}).Stringify()));}
    void select(uint64_t id){if(!available())return;popup.Hide();send(O({{L"op",S(L"select")},{L"id",N(double(id))}}));}
    void close(uint64_t id){popup.Hide();send(O({{L"op",S(L"close")},{L"id",N(double(id))}}));}
    MenuFlyout menu(uint64_t id){
        MenuFlyout menu;TrackPopup(menu,data);auto weak=weak_from_this();
        auto add=[&](hstring name,J action){MenuFlyoutItem item;item.Text(name);if(name==L"Undo tab order")AutomationProperties::SetAutomationId(item,L"drawing-order-undo");if(name==L"Redo tab order")AutomationProperties::SetAutomationId(item,L"drawing-order-redo");item.Click([weak,action](auto&&,auto&&){if(auto self=weak.lock())self->send(action);});menu.Items().Append(item);return item;};
        auto retry=add(L"Retry drawing storage",O({{L"op",S(L"retry_storage")}}));
        add(L"Move left / up",O({{L"op",S(L"step")},{L"id",N(double(id))},{L"forward",B(false)}}));
        add(L"Move right / down",O({{L"op",S(L"step")},{L"id",N(double(id))},{L"forward",B(true)}}));
        auto undo=add(L"Undo tab order",O({{L"op",S(L"history")},{L"redo",B(false)}}));auto redo=add(L"Redo tab order",O({{L"op",S(L"history")},{L"redo",B(true)}}));
        menu.Opening([weak,retry,undo,redo](auto&&,auto&&){if(auto self=weak.lock()){auto m=self->model();retry.Visibility(str(m,L"storage_error").empty()?Visibility::Collapsed:Visibility::Visible);undo.IsEnabled(flag(m,L"can_undo"));redo.IsEnabled(flag(m,L"can_redo"));}});
        MenuFlyoutItem item;item.Text(L"Close drawing");item.Click([weak,id](auto&&,auto&&){if(auto self=weak.lock())self->close(id);});menu.Items().Append(item);return menu;
    }
    A hits(){A result;for(auto value:array(model(),L"tabs")){auto id=uint64_t(num(value.GetObject(),L"id"));auto it=tabs.find(id);if(it==tabs.end())continue;auto box=it->second.box;
        auto b=box.TransformToVisual(root).TransformBounds({0,0,float(box.ActualWidth()),float(box.ActualHeight())});
        if(b.Width>0&&b.Height>0)result.Append(O({{L"id",N(double(id))},{L"bounds",O({{L"x",N(b.X)},{L"y",N(b.Y)},{L"width",N(b.Width)},{L"height",N(b.Height)}})}}));}return result;}
    A point(Windows::Foundation::Point value){A result;result.Append(N(value.X));result.Append(N(value.Y));return result;}
    J slideRequest(){return O({{L"id",N(double(source))},{L"hits",slideHits},{L"clip",slideClip},{L"press",point(origin)},{L"point",point(position)}});}
    J slideAt(){
        auto request=slideRequest();request.Insert(L"order",slideOrder);auto json=to_string(request.Stringify());
        std::unique_ptr<char,decltype(&capy_string_free)> reply(capy_document_tab_slide(json.c_str()),capy_string_free);
        if(!reply||std::string_view(reply.get())=="null")return J{};return J::Parse(to_hstring(reply.get()));}
    void beginSlide(){
        slideOrder=A{};for(auto v:array(model(),L"tabs"))slideOrder.Append(N(num(v.GetObject(),L"id")));slideHits=hits();
        auto bounds=strip.TransformToVisual(root).TransformBounds({0,0,float(strip.ActualWidth()),float(strip.ActualHeight())});
        slideClip=rectangle(bounds);slide=J{};copies.clear();overlay.Children().Clear();
        if(slideHits.Size()!=slideOrder.Size()||bounds.Width<=0){slideHits=A{};return;}
        animateSlide=Windows::UI::ViewManagement::UISettings().AnimationsEnabled();
        overlay.Width(bounds.Width);overlay.Height(bounds.Height);overlay.Margin({bounds.X,bounds.Y,0,0});
        RectangleGeometry mask;mask.Rect({0,0,bounds.Width,bounds.Height});overlay.Clip(mask);
        auto selectedId=uint64_t(num(model(),L"selected"));
        for(auto value:slideHits){
            auto hit=value.GetObject();auto id=uint64_t(num(hit,L"id"));auto b=object(hit,L"bounds");auto& tab=tabs.at(id);
            Grid content;content.ColumnDefinitions().Append(ColumnDefinition());ColumnDefinition tail;tail.Width({28,GridUnitType::Pixel});content.ColumnDefinitions().Append(tail);
            auto title=label(data,tab.text.Text());title.TextTrimming(TextTrimming::CharacterEllipsis);title.Margin({8,0,2,0});title.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(title);
            auto mark=label(data,L"×");mark.HorizontalAlignment(HorizontalAlignment::Center);mark.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(mark,1);content.Children().Append(mark);
            SlideCopy copy;copy.id=id;copy.copy.Child(content);copy.copy.CornerRadius({17,17,17,17});
            copy.copy.Background(id==selectedId?data->glass(L"document_tab"):id==source?data->tint(L"text",18):clear());
            copy.copy.Width(num(b,L"width"));copy.copy.Height(num(b,L"height"));
            Canvas::SetLeft(copy.copy,num(b,L"x")-bounds.X);Canvas::SetTop(copy.copy,num(b,L"y")-bounds.Y);Canvas::SetZIndex(copy.copy,id==source?2:0);
            overlay.Children().Append(copy.copy);copies.push_back(copy);tab.box.Opacity(0);
        }
        overlay.Visibility(Visibility::Visible);
    }
    void endSlide(){
        for(auto& copy:copies)if(copy.animation)copy.copy.StopAnimation(copy.animation);
        copies.clear();overlay.Children().Clear();overlay.Visibility(Visibility::Collapsed);slideHits=A{};slide=J{};
        for(auto& [id,tab]:tabs)tab.box.Opacity(1);
    }
    void updateSlide(){
        if(!slideHits.Size())return;slide=slideAt();if(!slide.Size())return;
        auto offsets=array(slide,L"offsets");if(offsets.Size()!=copies.size())return;
        auto scale=root.XamlRoot()?root.XamlRoot().RasterizationScale():1.;
        for(uint32_t i=0;i<copies.size();++i){
            auto& copy=copies[i];bool grabbed=copy.id==source;
            double target=grabbed?std::round((num(object(slide,L"bounds"),L"x")-num(object(slideHits.GetObjectAt(i),L"bounds"),L"x"))*scale)/scale:offsets.GetNumberAt(i);
            if(target==copy.target)continue;copy.target=target;
            if(!grabbed&&animateSlide){
                auto compositor=CompositionTarget::GetCompositorForCurrentThread();
                auto shift=compositor.CreateVector3KeyFrameAnimation();shift.Target(L"Translation");
                shift.InsertExpressionKeyFrame(0,L"this.StartingValue");
                shift.InsertKeyFrame(1,{float(target),0,0},compositor.CreateCubicBezierEasingFunction({.215f,.61f},{.355f,1.f}));
                shift.Duration(std::chrono::milliseconds(120));copy.animation=shift;copy.copy.StartAnimation(shift);
            }else{
                if(copy.animation){copy.copy.StopAnimation(copy.animation);copy.animation=nullptr;}
                copy.copy.Translation({float(target),0,0});
            }
        }
        AutomationProperties::SetItemStatus(overlay,slide.Stringify());
    }
    void cancel(){if(!pointer)return;pointer.reset();dragging=false;held=false;endSlide();root.ReleasePointerCaptures();
        if(recognizing){recognizing=false;hold.CompleteGesture();}if(tabs.contains(source))tabs.at(source).menu.Hide();AutomationProperties::SetItemStatus(root,L"Ready");}
    void move(PointerRoutedEventArgs const& e){if(pointer!=e.Pointer().PointerId())return;position=e.GetCurrentPoint(root).Position();
        if(recognizing)hold.ProcessMoveEvents(e.GetIntermediatePoints(root));
        if(!dragging&&(std::abs(position.X-origin.X)>slopX||std::abs(position.Y-origin.Y)>slopY)){dragging=true;held=false;if(tabs.contains(source))tabs.at(source).menu.Hide();beginSlide();}
        if(dragging){updateSlide();AutomationProperties::SetItemStatus(root,L"Dragging");}e.Handled(true);
    }
    void activateItem(winrt::Windows::Foundation::IInspectable const& value){for(auto const& [id,row]:rows)if(value==row.item||value==row.item.Content()){select(id);return;}}
    void show(FrameworkElement anchor=nullptr){refresh();popup.ShowAt(anchor?anchor:selector);}
    void styleTab(Tab& t){
        t.box.CornerRadius({17,17,17,17});t.select.CornerRadius({17,0,0,17});t.close.CornerRadius({0,17,17,0});
        for(auto const& part:{t.select,t.close}){
            part.Background(clear());
            part.Resources().Insert(box_value(L"ButtonBackgroundPointerOver"),data->tint(L"text",18));
            part.Resources().Insert(box_value(L"ButtonBackgroundPressed"),data->tint(L"text",41));
        }
    }
    void paintTab(Tab& t,bool chosen){t.box.Background(chosen?data->glass(L"document_tab"):clear());}
    void layout(){auto count=array(model(),L"tabs").Size();bool one=count<2,compact=!one&&capy_document_tabs_compact(float(root.ActualWidth()),count);
        plain.Visibility(one?Visibility::Visible:Visibility::Collapsed);
        strip.Visibility(one||compact?Visibility::Collapsed:Visibility::Visible);selector.Visibility(!one&&compact?Visibility::Visible:Visibility::Collapsed);
        root.Background(headerSurface(data));root.CornerRadius({6,6,6,6});root.Margin(one||compact?Thickness{}:Thickness{0,1,0,1});
        if(!compact){double width=std::max(0.,(root.ActualWidth()-6.*(std::max(1u,count)-1))/std::max(1u,count));for(auto& [id,t]:tabs)t.box.Width(width);}}
    void refresh(){
        if(pointer&&(!available()||epoch!=to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")))))cancel();
        if(pointer&&slideHits.Size()){A current;for(auto v:array(model(),L"tabs"))current.Append(N(num(v.GetObject(),L"id")));if(current.Stringify()!=slideOrder.Stringify())cancel();}
        updating=true;auto weak=weak_from_this();auto m=model();auto selectedId=uint64_t(num(m,L"selected"));bool selectionChanged=selectedId!=selectedModel;selectedModel=selectedId;std::vector<hstring> order;uint32_t index=0;std::set<uint64_t> live;
        for(auto v:array(m,L"tabs")){auto spec=v.GetObject();auto id=uint64_t(num(spec,L"id"));live.insert(id);auto label=marked(spec);auto key=to_hstring(id);order.push_back(key);
            if(!tabs.contains(id)){
                Tab t;t.box.ColumnDefinitions().Append(ColumnDefinition());ColumnDefinition tail;tail.Width({28,GridUnitType::Pixel});t.box.ColumnDefinitions().Append(tail);
                t.select=button(data,label,[weak,id]{if(auto self=weak.lock())self->select(id);});t.select.HorizontalAlignment(HorizontalAlignment::Stretch);t.select.HorizontalContentAlignment(HorizontalAlignment::Stretch);t.select.Padding({8,0,2,0});t.select.MinWidth(0);t.select.Height(34);
                t.text=CapyUi::label(data,label);t.text.TextTrimming(TextTrimming::CharacterEllipsis);t.select.Content(t.text);AutomationProperties::SetAutomationId(t.select,L"drawing-tab-"+key);
                t.close=button(data,L"Close drawing",[weak,id]{if(auto self=weak.lock())self->close(id);});t.close.Content(box_value(L"×"));t.close.Padding({0});t.close.MinWidth(0);t.close.Height(34);Grid::SetColumn(t.close,1);AutomationProperties::SetAutomationId(t.close,L"drawing-close-"+key);
                t.menu=menu(id);t.select.KeyDown([weak,id](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock())self->tabKey(id,e);});t.select.ContextRequested([weak,id](auto&&,ContextRequestedEventArgs const& e){e.Handled(true);if(auto self=weak.lock();self&&self->available()&&!self->pointer)self->tabs.at(id).menu.ShowAt(self->tabs.at(id).select);});t.select.IsHoldingEnabled(false);t.select.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak,id](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
                    auto p=e.GetCurrentPoint(self->root);if(p.Properties().IsMiddleButtonPressed()){self->close(id);e.Handled(true);return;}
                    if(!self->available()||self->pointer||!p.IsInContact()||p.Properties().IsRightButtonPressed()||p.Properties().IsBarrelButtonPressed())return;
                    self->tabs.at(id).select.Focus(FocusState::Pointer);self->tabs.at(id).select.CancelDirectManipulations();self->tabs.at(id).select.ReleasePointerCapture(e.Pointer());if(!self->root.CapturePointer(e.Pointer()))return;AutomationProperties::SetItemStatus(self->root,L"Pressed");self->source=id;self->pointer=p.PointerId();self->origin=self->position=p.Position();self->epoch=to_hstring(uint64_t(num(object(self->data->state,L"document_file"),L"epoch")));
                    auto dpi=GetDpiForWindow(GetForegroundWindow());self->slopX=std::max(2.,double(GetSystemMetricsForDpi(SM_CXDRAG,dpi))*96./std::max(96u,dpi));self->slopY=std::max(2.,double(GetSystemMetricsForDpi(SM_CYDRAG,dpi))*96./std::max(96u,dpi));
                    if(p.PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Mouse){self->recognizing=true;self->hold.ProcessDownEvent(p);}e.Handled(true);
                }})),true);t.box.Children().Append(t.select);t.box.Children().Append(t.close);styleTab(t);tabs.emplace(id,t);
                Row r;r.item.Tag(box_value(key));AutomationProperties::SetAutomationId(r.item,L"drawing-row-"+key);Grid row;row.ColumnDefinitions().Append(ColumnDefinition());ColumnDefinition first;first.Width({28,GridUnitType::Pixel});row.ColumnDefinitions().InsertAt(0,first);ColumnDefinition lastColumn;lastColumn.Width({28,GridUnitType::Pixel});row.ColumnDefinitions().Append(lastColumn);auto title=str(spec,L"title");
                r.grip=button(data,L"Reorder drawing",[]{});r.grip.Content(panelGrip(data->theme()));r.grip.Padding({0});r.grip.MinWidth(0);AutomationProperties::SetAutomationId(r.grip,L"drawing-grip-"+key);
                r.remove=button(data,L"Close "+title,[weak,id]{if(auto self=weak.lock())self->close(id);});r.remove.Content(box_value(L"×"));r.remove.Padding({0});r.remove.MinWidth(0);Grid::SetColumn(r.remove,2);AutomationProperties::SetAutomationId(r.remove,L"drawing-row-close-"+key);r.menu=menu(id);
                StackPanel labels;r.text=CapyUi::label(data,label);r.location=CapyUi::label(data,L"");r.location.FontSize(11);r.location.TextTrimming(TextTrimming::CharacterEllipsis);labels.Children().Append(r.text);labels.Children().Append(r.location);Grid::SetColumn(labels,1);row.Children().Append(r.grip);row.Children().Append(labels);row.Children().Append(r.remove);r.item.Content(row);
                rowDrag->Attach(r.item,r.grip,r.remove,r.menu,key);rows.emplace(id,r);
            }
            auto& t=tabs.at(id);t.text.Text(label);t.text.Foreground(data->brush(L"text"));paintTab(t,id==uint64_t(num(m,L"selected")));t.select.IsEnabled(available());t.close.IsEnabled(available());ToolTipService::SetToolTip(t.select,box_value(str(spec,L"location",label)));AutomationProperties::SetName(t.select,label);
            auto& r=rows.at(id);r.text.Text(label);r.location.Text(str(spec,L"location"));r.item.IsEnabled(available());r.remove.IsEnabled(available());
            AutomationProperties::SetName(r.remove,L"Close "+str(spec,L"title"));ToolTipService::SetToolTip(r.remove,box_value(L"Close "+str(spec,L"title")));
            auto children=strip.Children();uint32_t at;if(index>=children.Size()||children.GetAt(index)!=t.box){if(children.IndexOf(t.box,at))children.RemoveAt(at);children.InsertAt(index,t.box);}
            auto items=list.Items();if(index>=items.Size()||items.GetAt(index)!=r.item){if(items.IndexOf(r.item,at))items.RemoveAt(at);items.InsertAt(index,r.item);}++index;
            if(id==uint64_t(num(m,L"selected"))){selector.Content(box_value(label+L" ▾"));if(!showing||selectionChanged)list.SelectedItem(r.item);}
        }
        while(strip.Children().Size()>index)strip.Children().RemoveAtEnd();while(list.Items().Size()>index)list.Items().RemoveAtEnd();
        std::erase_if(tabs,[&](auto const& p){return !live.contains(p.first);});std::erase_if(rows,[&](auto const& p){return !live.contains(p.first);});rowDrag->Refresh(order);selector.IsEnabled(available());
        auto error=str(m,L"storage_error");if(!error.empty())ToolTipService::SetToolTip(selector,box_value(error+L". Use Drawing options → Retry drawing storage."));else ToolTipService::SetToolTip(selector,box_value(L"Choose a drawing"));
        AutomationProperties::SetHelpText(root,error);if(plain.Text()!=plainTitle())plain.Text(plainTitle());plain.Foreground(data->brush(L"text"));layout();updating=false;
    }
    void init(){
        auto weak=weak_from_this();root.Tag(O({{L"header_source",O({{L"kind",S(L"native")}})},{L"native_keys",B(true)}}));root.Background(clear());strip.Orientation(Orientation::Horizontal);
        plain.FontWeight(Windows::UI::Text::FontWeights::SemiBold());plain.FontSize(data->textSize());plain.TextTrimming(TextTrimming::CharacterEllipsis);plain.TextWrapping(TextWrapping::NoWrap);
        plain.TextAlignment(TextAlignment::Center);plain.VerticalAlignment(VerticalAlignment::Center);plain.Padding({8,0,8,0});plain.IsHitTestVisible(false);AutomationProperties::SetAutomationId(plain,L"drawing-title");
        root.Children().Append(plain);root.Children().Append(strip);root.Children().Append(selector);selector.MinWidth(0);selector.HorizontalAlignment(HorizontalAlignment::Stretch);selector.Height(34);AutomationProperties::SetAutomationId(selector,L"drawing-selector");AutomationProperties::SetItemStatus(selector,L"Closed");AutomationProperties::SetName(root,L"Drawings");AutomationProperties::SetAutomationId(root,L"drawing-tabs");
        selector.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->show();});root.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->layout();});
        strip.Spacing(6);overlay.HorizontalAlignment(HorizontalAlignment::Left);overlay.VerticalAlignment(VerticalAlignment::Top);overlay.IsHitTestVisible(false);overlay.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(overlay,L"drawing-tab-slide");AutomationProperties::SetName(overlay,L"Drawing tab slide");root.Children().Append(overlay);
        AutomationProperties::SetAutomationId(list,L"drawing-list");
        list.IsItemClickEnabled(true);list.SelectionMode(ListViewSelectionMode::Single);list.MaxHeight(420);listSurface.Children().Append(list);listSurface.Width(360);popup.Content(listSurface);TrackPopup(popup,data);
        rowDrag=std::make_unique<WorkspaceRowDrag>(list,listSurface,[weak]{auto s=weak.lock();return s&&s->available();},[weak](hstring id,std::optional<hstring> before){if(auto s=weak.lock())s->send(O({{L"op",S(L"reorder")},{L"id",N(double(std::stoull(id.c_str())))},{L"before",before?N(double(std::stoull(before->c_str()))):JsonValue::CreateNullValue()}}));},[weak](hstring id){if(auto s=weak.lock())s->select(std::stoull(id.c_str()));},[]{});
        popup.Closed([weak](auto&&,auto&&){if(auto s=weak.lock()){s->showing=false;AutomationProperties::SetItemStatus(s->selector,L"Closed");s->rowDrag->Cancel();}});popup.Opened([weak](auto&&,auto&&){if(auto s=weak.lock()){s->showing=true;AutomationProperties::SetItemStatus(s->selector,L"Open");}});
        list.ItemClick([weak](auto&&,ItemClickEventArgs const& e){if(auto s=weak.lock();s&&!s->updating&&!s->rowDrag->SuppressClick()){s->activateItem(e.ClickedItem());}});
        list.PreviewKeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto s=weak.lock()){
            if(e.Key()==Windows::System::VirtualKey::Escape&&s->rowDrag->Escape()){e.Handled(true);return;}
            bool control=GetKeyState(VK_CONTROL)&0x8000,shift=GetKeyState(VK_SHIFT)&0x8000;auto key=e.Key();
            if(key==K::Delete||(control&&shift&&(key==K::Up||key==K::Down)))
                for(auto const& [id,row]:s->rows)if(row.item==Input::FocusManager::GetFocusedElement(s->list.XamlRoot())){
                    e.Handled(true);if(!s->available())return;
                    if(key==K::Delete)s->close(id);else s->send(O({{L"op",S(L"step")},{L"id",N(double(id))},{L"forward",B(key==K::Down)}}));return;
                }
            if(e.Key()!=Windows::System::VirtualKey::Enter)return;
            for(auto node=Input::FocusManager::GetFocusedElement(s->list.XamlRoot()).try_as<DependencyObject>();node&&node!=s->list;node=VisualTreeHelper::GetParent(node)){
                if(node.try_as<Button>())return;
                for(auto const& [id,row]:s->rows)if(node==row.item){e.Handled(true);s->select(id);return;}
            }
        }});
        hold.GestureSettings(Microsoft::UI::Input::GestureSettings::Hold);hold.Holding([weak](auto&&,Microsoft::UI::Input::HoldingEventArgs const& e){if(auto s=weak.lock();s&&s->pointer&&!s->dragging&&e.HoldingState()==Microsoft::UI::Input::HoldingState::Started){s->held=true;if(s->tabs.contains(s->source)){Primitives::FlyoutShowOptions options;options.ShowMode(Primitives::FlyoutShowMode::Transient);s->tabs.at(s->source).menu.ShowAt(s->tabs.at(s->source).select,options);}}});
        root.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){if(auto s=weak.lock())s->move(e);})),true);
        root.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){if(auto s=weak.lock();s&&s->pointer==e.Pointer().PointerId()){
            if(e.GetCurrentPoint(s->root).Properties().IsCanceled()){s->cancel();e.Handled(true);return;}
            s->position=e.GetCurrentPoint(s->root).Position();bool dragged=s->dragging,held=s->held;auto id=s->source;
            if(dragged)s->updateSlide();bool attached=dragged&&s->slide.Size()&&flag(s->slide,L"attached");auto request=s->slideRequest();
            MenuFlyout menu=s->tabs.contains(id)?s->tabs.at(id).menu:nullptr;s->cancel();
            if(attached){request.Insert(L"op",S(L"slide"));s->send(request);}
            else if(held&&menu)menu.ShowAt(s->tabs.at(id).select);else if(!dragged)s->select(id);e.Handled(true);
        }})),true);
        auto cancel=[weak](auto&&,PointerRoutedEventArgs const& e){if(auto s=weak.lock();s&&s->pointer==e.Pointer().PointerId())s->cancel();};root.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler(cancel)),true);root.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler(cancel)),true);
        root.LostFocus([weak](auto&&,auto&&){if(auto s=weak.lock();s&&s->pointer&&!s->held)s->cancel();});
        root.Unloaded([weak](auto&&,auto&&){if(auto s=weak.lock()){s->cancel();s->rowDrag->Cancel();}});
        root.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(e.Key()==Windows::System::VirtualKey::Escape)if(auto s=weak.lock();s&&s->pointer){s->cancel();e.Handled(true);}});
        refresh();
    }
};
}
