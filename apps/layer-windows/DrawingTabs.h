#pragma once
#include "UiControls.h"
#include "NativeMenus.h"
#include "WorkspaceRowDrag.h"
#include <winrt/Microsoft.UI.Input.h>
#include <optional>
#include <set>
#include <string_view>

namespace CapyUi {
struct DrawingTabs:std::enable_shared_from_this<DrawingTabs>{
    std::shared_ptr<WorkspaceData> data;Grid root;StackPanel strip;Button selector;Flyout popup;Grid listSurface;ListView list;
    struct Tab{Grid box;Button select,close;TextBlock text;MenuFlyout menu;};
    struct Row{ListViewItem item;Button grip,more;TextBlock text,location;MenuFlyout menu;};
    std::map<uint64_t,Tab> tabs;std::map<uint64_t,Row> rows;std::unique_ptr<WorkspaceRowDrag> rowDrag;
    Border hint;std::optional<uint32_t> pointer;uint64_t source=0;Windows::Foundation::Point origin{},position{};
    Microsoft::UI::Input::GestureRecognizer hold;bool recognizing=false,dragging=false,held=false,updating=false,showing=false;double slopX=4,slopY=4;
    uint64_t selectedModel=0;
    hstring epoch;J model(){return object(data->model,L"windows_tabs");}
    bool available(){return flag(model(),L"available")&&!flag(object(data->model,L"header"),L"editing");}
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
    J target(){A order,point;for(auto v:array(model(),L"tabs"))order.Append(N(num(v.GetObject(),L"id")));point.Append(N(position.X));point.Append(N(position.Y));
        auto request=O({{L"order",order},{L"hits",hits()},{L"point",point},{L"vertical",B(false)}});auto json=to_string(request.Stringify());
        std::unique_ptr<char,decltype(&capy_string_free)> reply(capy_document_tab_drop(json.c_str()),capy_string_free);
        if(!reply||std::string_view(reply.get())=="null")return J{};return J::Parse(to_hstring(reply.get()));}
    void cancel(){if(!pointer)return;pointer.reset();dragging=false;held=false;hint.Visibility(Visibility::Collapsed);root.ReleasePointerCaptures();
        if(recognizing){recognizing=false;hold.CompleteGesture();}if(tabs.contains(source)){tabs.at(source).box.Opacity(1);tabs.at(source).menu.Hide();}AutomationProperties::SetItemStatus(root,L"Ready");}
    void move(PointerRoutedEventArgs const& e){if(pointer!=e.Pointer().PointerId())return;position=e.GetCurrentPoint(root).Position();
        if(recognizing)hold.ProcessMoveEvents(e.GetIntermediatePoints(root));
        if(!dragging&&(std::abs(position.X-origin.X)>slopX||std::abs(position.Y-origin.Y)>slopY)){dragging=true;held=false;if(tabs.contains(source)){tabs.at(source).menu.Hide();tabs.at(source).box.Opacity(.55);}}
        if(dragging){auto drop=target();hint.Visibility(drop.Size()?Visibility::Visible:Visibility::Collapsed);if(drop.Size()){
            auto before=drop.GetNamedValue(L"before",JsonValue::CreateNullValue());double x=root.ActualWidth();
            for(auto v:hits()){auto h=v.GetObject();auto b=object(h,L"bounds");if(before.ValueType()==JsonValueType::Number&&num(h,L"id")==before.GetNumber()){x=num(b,L"x");break;}if(before.ValueType()==JsonValueType::Null)x=num(b,L"x")+num(b,L"width");}
            hint.Margin({x-1,2,0,2});}
            AutomationProperties::SetItemStatus(root,L"Dragging");}e.Handled(true);
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
    void paintTab(Tab& t,bool chosen){t.box.Background(chosen?data->brush(L"panel"):clear());}
    void layout(){auto count=array(model(),L"tabs").Size();bool compact=capy_document_tabs_compact(float(root.ActualWidth()),count);
        strip.Visibility(compact?Visibility::Collapsed:Visibility::Visible);selector.Visibility(compact?Visibility::Visible:Visibility::Collapsed);
        root.Background(headerSurface(data));root.CornerRadius({6,6,6,6});root.Margin(compact?Thickness{}:Thickness{0,1,0,1});
        if(!compact){double width=std::max(0.,(root.ActualWidth()-6.*(std::max(1u,count)-1))/std::max(1u,count));for(auto& [id,t]:tabs)t.box.Width(width);}}
    void refresh(){
        if(pointer&&(!available()||epoch!=to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")))))cancel();
        updating=true;auto weak=weak_from_this();auto m=model();auto selectedId=uint64_t(num(m,L"selected"));bool selectionChanged=selectedId!=selectedModel;selectedModel=selectedId;std::vector<hstring> order;uint32_t index=0;std::set<uint64_t> live;
        for(auto v:array(m,L"tabs")){auto spec=v.GetObject();auto id=uint64_t(num(spec,L"id"));live.insert(id);auto label=str(spec,L"title")+(flag(spec,L"modified")?L" •":L"");auto key=to_hstring(id);order.push_back(key);
            if(!tabs.contains(id)){
                Tab t;t.box.ColumnDefinitions().Append(ColumnDefinition());ColumnDefinition tail;tail.Width({28,GridUnitType::Pixel});t.box.ColumnDefinitions().Append(tail);
                t.select=button(data,label,[weak,id]{if(auto self=weak.lock())self->select(id);});t.select.HorizontalAlignment(HorizontalAlignment::Stretch);t.select.HorizontalContentAlignment(HorizontalAlignment::Stretch);t.select.Padding({8,0,2,0});t.select.MinWidth(0);t.select.Height(34);
                t.text=CapyUi::label(data,label);t.text.TextTrimming(TextTrimming::CharacterEllipsis);t.select.Content(t.text);AutomationProperties::SetAutomationId(t.select,L"drawing-tab-"+key);
                t.close=button(data,L"Close drawing",[weak,id]{if(auto self=weak.lock())self->close(id);});t.close.Content(box_value(L"×"));t.close.Padding({0});t.close.MinWidth(0);t.close.Height(34);Grid::SetColumn(t.close,1);AutomationProperties::SetAutomationId(t.close,L"drawing-close-"+key);
                t.menu=menu(id);t.select.ContextRequested([weak,id](auto&&,ContextRequestedEventArgs const& e){e.Handled(true);if(auto self=weak.lock();self&&self->available()&&!self->pointer)self->tabs.at(id).menu.ShowAt(self->tabs.at(id).select);});t.select.IsHoldingEnabled(false);t.select.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak,id](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
                    auto p=e.GetCurrentPoint(self->root);if(p.Properties().IsMiddleButtonPressed()){self->close(id);e.Handled(true);return;}
                    if(!self->available()||self->pointer||!p.IsInContact()||p.Properties().IsRightButtonPressed()||p.Properties().IsBarrelButtonPressed())return;
                    self->tabs.at(id).select.Focus(FocusState::Pointer);self->tabs.at(id).select.CancelDirectManipulations();self->tabs.at(id).select.ReleasePointerCapture(e.Pointer());if(!self->root.CapturePointer(e.Pointer()))return;AutomationProperties::SetItemStatus(self->root,L"Pressed");self->source=id;self->pointer=p.PointerId();self->origin=self->position=p.Position();self->epoch=to_hstring(uint64_t(num(object(self->data->state,L"document_file"),L"epoch")));
                    auto dpi=GetDpiForWindow(GetForegroundWindow());self->slopX=std::max(2.,double(GetSystemMetricsForDpi(SM_CXDRAG,dpi))*96./std::max(96u,dpi));self->slopY=std::max(2.,double(GetSystemMetricsForDpi(SM_CYDRAG,dpi))*96./std::max(96u,dpi));
                    if(p.PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Mouse){self->recognizing=true;self->hold.ProcessDownEvent(p);}e.Handled(true);
                }})),true);t.box.Children().Append(t.select);t.box.Children().Append(t.close);styleTab(t);tabs.emplace(id,t);
                Row r;r.item.Tag(box_value(key));AutomationProperties::SetAutomationId(r.item,L"drawing-row-"+key);Grid row;row.ColumnDefinitions().Append(ColumnDefinition());ColumnDefinition first;first.Width({28,GridUnitType::Pixel});row.ColumnDefinitions().InsertAt(0,first);ColumnDefinition lastColumn;lastColumn.Width({28,GridUnitType::Pixel});row.ColumnDefinitions().Append(lastColumn);
                r.grip=button(data,L"Reorder drawing",[]{});r.grip.Content(panelGrip(data->theme()));r.grip.Padding({0});r.grip.MinWidth(0);AutomationProperties::SetAutomationId(r.grip,L"drawing-grip-"+key);
                r.more=button(data,L"Drawing options",[]{});r.more.Content(box_value(L"⋮"));r.more.Padding({0});r.more.MinWidth(0);Grid::SetColumn(r.more,2);r.menu=menu(id);r.more.Flyout(r.menu);
                StackPanel labels;r.text=CapyUi::label(data,label);r.location=CapyUi::label(data,L"");r.location.FontSize(11);r.location.TextTrimming(TextTrimming::CharacterEllipsis);labels.Children().Append(r.text);labels.Children().Append(r.location);Grid::SetColumn(labels,1);row.Children().Append(r.grip);row.Children().Append(labels);row.Children().Append(r.more);r.item.Content(row);
                rowDrag->Attach(r.item,r.grip,r.more,r.menu,key);rows.emplace(id,r);
            }
            auto& t=tabs.at(id);t.text.Text(label);t.text.Foreground(data->brush(L"text"));paintTab(t,id==uint64_t(num(m,L"selected")));t.select.IsEnabled(available());t.close.IsEnabled(available());ToolTipService::SetToolTip(t.select,box_value(str(spec,L"location",label)));AutomationProperties::SetName(t.select,label);
            auto& r=rows.at(id);r.text.Text(label);r.location.Text(str(spec,L"location"));r.item.IsEnabled(available());
            auto children=strip.Children();uint32_t at;if(index>=children.Size()||children.GetAt(index)!=t.box){if(children.IndexOf(t.box,at))children.RemoveAt(at);children.InsertAt(index,t.box);}
            auto items=list.Items();if(index>=items.Size()||items.GetAt(index)!=r.item){if(items.IndexOf(r.item,at))items.RemoveAt(at);items.InsertAt(index,r.item);}++index;
            if(id==uint64_t(num(m,L"selected"))){selector.Content(box_value(label+L" ▾"));if(!showing||selectionChanged)list.SelectedItem(r.item);}
        }
        while(strip.Children().Size()>index)strip.Children().RemoveAtEnd();while(list.Items().Size()>index)list.Items().RemoveAtEnd();
        std::erase_if(tabs,[&](auto const& p){return !live.contains(p.first);});std::erase_if(rows,[&](auto const& p){return !live.contains(p.first);});rowDrag->Refresh(order);selector.IsEnabled(available());
        auto error=str(m,L"storage_error");if(!error.empty())ToolTipService::SetToolTip(selector,box_value(error+L". Use Drawing options → Retry drawing storage."));else ToolTipService::SetToolTip(selector,box_value(L"Choose a drawing"));
        AutomationProperties::SetHelpText(root,error);layout();updating=false;
    }
    void init(){
        auto weak=weak_from_this();root.Tag(O({{L"header_source",O({{L"kind",S(L"native")}})}}));root.Background(clear());strip.Orientation(Orientation::Horizontal);root.Children().Append(strip);root.Children().Append(selector);selector.MinWidth(0);selector.HorizontalAlignment(HorizontalAlignment::Stretch);selector.Height(34);AutomationProperties::SetAutomationId(selector,L"drawing-selector");AutomationProperties::SetItemStatus(selector,L"Closed");AutomationProperties::SetName(root,L"Drawings");AutomationProperties::SetAutomationId(root,L"drawing-tabs");
        selector.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->show();});root.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->layout();});
        hint.Width(2);hint.Background(accent(data));strip.Spacing(6);hint.HorizontalAlignment(HorizontalAlignment::Left);hint.IsHitTestVisible(false);hint.Visibility(Visibility::Collapsed);root.Children().Append(hint);
        AutomationProperties::SetAutomationId(list,L"drawing-list");
        list.IsItemClickEnabled(true);list.SelectionMode(ListViewSelectionMode::Single);list.MaxHeight(420);listSurface.Children().Append(list);listSurface.Width(360);popup.Content(listSurface);TrackPopup(popup,data);
        rowDrag=std::make_unique<WorkspaceRowDrag>(list,listSurface,[weak]{auto s=weak.lock();return s&&s->available();},[weak](hstring id,std::optional<hstring> before){if(auto s=weak.lock())s->send(O({{L"op",S(L"reorder")},{L"id",N(double(std::stoull(id.c_str())))},{L"before",before?N(double(std::stoull(before->c_str()))):JsonValue::CreateNullValue()}}));},[weak](hstring id){if(auto s=weak.lock())s->select(std::stoull(id.c_str()));},[]{});
        popup.Closed([weak](auto&&,auto&&){if(auto s=weak.lock()){s->showing=false;AutomationProperties::SetItemStatus(s->selector,L"Closed");s->rowDrag->Cancel();}});popup.Opened([weak](auto&&,auto&&){if(auto s=weak.lock()){s->showing=true;AutomationProperties::SetItemStatus(s->selector,L"Open");}});
        list.ItemClick([weak](auto&&,ItemClickEventArgs const& e){if(auto s=weak.lock();s&&!s->updating&&!s->rowDrag->SuppressClick()){s->activateItem(e.ClickedItem());}});
        list.PreviewKeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto s=weak.lock()){
            if(e.Key()==Windows::System::VirtualKey::Escape&&s->rowDrag->Escape()){e.Handled(true);return;}
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
            s->position=e.GetCurrentPoint(s->root).Position();bool dragged=s->dragging,held=s->held;auto id=s->source;auto drop=s->target();auto hits=s->hits();auto position=s->position;MenuFlyout menu=s->tabs.contains(id)?s->tabs.at(id).menu:nullptr;s->cancel();
            if(dragged&&drop.Size()){A point;point.Append(N(position.X));point.Append(N(position.Y));s->send(O({{L"op",S(L"drop")},{L"id",N(double(id))},{L"hits",hits},{L"point",point},{L"vertical",B(false)}}));}
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
