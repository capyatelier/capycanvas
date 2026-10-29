#pragma once
#include "UiControls.h"
#include "NativeMenus.h"
#include "WorkspaceRowDrag.h"
#include "WorkspaceGeometry.h"
#include "ExternalImages.h"
#include "NativePresentation.h"
#include <winrt/Microsoft.UI.Input.h>
#include <winrt/Microsoft.UI.Composition.h>
#include <winrt/Windows.UI.ViewManagement.h>
#include <optional>
#include <set>
#include <string_view>

namespace CapyUi {
struct DrawingTabs:std::enable_shared_from_this<DrawingTabs>{
    using K=Windows::System::VirtualKey;
    std::shared_ptr<WorkspaceData> data;Grid root;TextBlock plain,selectorText;StackPanel strip;Button selector;Flyout popup;Grid listSurface;ListView list;
    struct Tab{Grid box;Border shade,separator;Button select,close;TextBlock text;MenuFlyout menu;bool chosen=false,hovered=false,pressed=false;};
    struct Row{ListViewItem item;Button grip,remove;TextBlock text,location;MenuFlyout menu;hstring title,locationSource;};
    std::map<uint64_t,Tab> tabs;std::map<uint64_t,Row> rows;std::unique_ptr<WorkspaceRowDrag> rowDrag;
    struct SlideCopy{uint64_t id=0;Border copy;double target=0;Microsoft::UI::Composition::Vector3KeyFrameAnimation animation{nullptr};};
    Canvas overlay;std::vector<SlideCopy> copies;A slideOrder,slideHits;J slideClip,slide;bool animateSlide=true;
    NativePresentation presentation;bool finishing=false;uint64_t contactGeneration=0;
    std::optional<uint32_t> pointer;Input::Pointer contact{nullptr};uint64_t source=0;Windows::Foundation::Point origin{},position{};
    PointerGesture hold;bool recognizing=false,dragging=false,held=false,releasing=false,updating=false,showing=false;
    uint64_t selectedModel=0;
    hstring epoch,plainSource,plainCaption,storageError;bool storageTooltipReady=false;J model(){return object(data->model,L"windows_tabs");}
    bool available(){return flag(model(),L"available")&&!flag(object(data->model,L"header"),L"editing");}
    bool single(){return array(model(),L"tabs").Size()<2;}
    J headerSize(){auto header=object(data->model,L"header");return find(array(header,L"sizes"),L"id",str(object(header,L"model"),L"size"));}
    double tile(){return num(headerSize(),L"tile",36);}
    double gap(){return num(headerSize(),L"gap",2);}
    double corner(){return tile()*.5*CornerFit;}
    hstring plainTitle(){
        auto all=array(data->state,L"tabs");if(!all.Size())return str(data->catalog,L"app_name");auto tab=all.GetObjectAt(0);
        auto request=O({{L"type",S(L"drawing_title")},{L"title",S(str(tab,L"title"))},{L"width",N(num(tab,L"width"))},{L"height",N(num(tab,L"height"))}});
        auto requestSource=request.Stringify()+L"/"+to_hstring(data->localizationGeneration);if(requestSource!=plainSource){plainSource=requestSource;plainCaption=data->caption(request);}return plainCaption;
    }
    static hstring marked(J const& spec){return (flag(spec,L"modified")?hstring(L"• "):hstring())+str(spec,L"title");}
    std::optional<uint64_t> neighbor(uint64_t id,K key){
        std::vector<uint64_t> ids;for(auto v:array(model(),L"tabs"))ids.push_back(uint64_t(num(v.GetObject(),L"id")));
        auto at=std::find(ids.begin(),ids.end(),id);if(at==ids.end()||ids.empty())return std::nullopt;auto index=size_t(at-ids.begin());
        if(key==K::Home)return ids.front();if(key==K::End)return ids.back();
        return ids[(index+(key==K::Right?1:ids.size()-1))%ids.size()];
    }
    void tabKey(uint64_t id,KeyRoutedEventArgs const& e){
        if(!available()||pointer||finishing)return;auto key=e.Key();
        bool control=GetKeyState(VK_CONTROL)&0x8000,shift=GetKeyState(VK_SHIFT)&0x8000;
        if(control&&shift&&(key==K::Left||key==K::Right)){send(O({{L"op",S(L"step")},{L"id",N(double(id))},{L"forward",B(key==K::Right)}}));e.Handled(true);return;}
        if(!control&&!shift&&key==K::Delete){close(id);e.Handled(true);return;}
        if(control||shift||(key!=K::Left&&key!=K::Right&&key!=K::Home&&key!=K::End))return;
        if(auto next=neighbor(id,key)){select(*next);if(tabs.contains(*next))tabs.at(*next).select.Focus(FocusState::Keyboard);}e.Handled(true);
    }
    void send(J action,std::function<void()> ready={}){
        auto json=to_string(O({{L"operation",S(L"tabs")},{L"action",action}}).Stringify());
        if(ready)data->commit(std::move(json),true,std::move(ready));else data->document(std::move(json));
    }
    void select(uint64_t id){if(!available()||finishing)return;popup.Hide();send(O({{L"op",S(L"select")},{L"id",N(double(id))}}));}
    void close(uint64_t id){if(finishing)return;popup.Hide();send(O({{L"op",S(L"close")},{L"id",N(double(id))}}));}
    MenuFlyout menu(uint64_t id){
        MenuFlyout menu;TrackPopup(menu,data);auto weak=weak_from_this();
        menu.Opening([weak,owner=make_weak(menu),id](auto&&,auto&&){if(auto self=weak.lock())if(auto menu=owner.get()){
            A rows;auto model=self->model();
            auto add=[&](wchar_t const* key,wchar_t const* identifier,J action,bool enabled=true){rows.Append(O({{L"label",S(self->data->caption(L"header",key))},{L"enabled",B(enabled)},{L"action",O({{L"command",S(identifier)},{L"action",action}})}}));};
            if(!str(model,L"storage_error").empty())add(L"retry_storage",L"drawing-retry-storage",O({{L"op",S(L"retry_storage")}}));
            add(L"move_left_up",L"drawing-move-left",O({{L"op",S(L"step")},{L"id",N(double(id))},{L"forward",B(false)}}));
            add(L"move_right_down",L"drawing-move-right",O({{L"op",S(L"step")},{L"id",N(double(id))},{L"forward",B(true)}}));
            add(L"undo_order",L"drawing-order-undo",O({{L"op",S(L"history")},{L"redo",B(false)}}),flag(model,L"can_undo"));
            add(L"redo_order",L"drawing-order-redo",O({{L"op",S(L"history")},{L"redo",B(true)}}),flag(model,L"can_redo"));
            add(L"close_drawing",L"drawing-close",O({{L"op",S(L"close")},{L"id",N(double(id))}}));
            A sections;sections.Append(rows);NativeMenuItems(menu.Items(),sections,self->data,[weak](J value){if(auto self=weak.lock()){
                auto action=object(value,L"action");if(str(action,L"op")==L"close")self->close(uint64_t(num(action,L"id")));else self->send(action);
            }});
        }});return menu;
    }
    A hits(){A result;for(auto value:array(model(),L"tabs")){auto id=uint64_t(num(value.GetObject(),L"id"));auto it=tabs.find(id);if(it==tabs.end())continue;auto box=it->second.box;
        auto b=box.TransformToVisual(root).TransformBounds({0,0,float(box.ActualWidth()),float(box.ActualHeight())});
        if(b.Width>0&&b.Height>0)result.Append(O({{L"id",N(double(id))},{L"bounds",O({{L"x",N(b.X)},{L"y",N(b.Y)},{L"width",N(b.Width)},{L"height",N(b.Height)}})}}));}return result;}
    A point(Windows::Foundation::Point value){A result;result.Append(N(value.X));result.Append(N(value.Y));return result;}
    J slideRequest(){return O({{L"id",N(double(source))},{L"hits",slideHits},{L"clip",slideClip},{L"press",point(origin)},{L"point",point(position)}});}
    J slideAt(){
        auto request=slideRequest();request.Insert(L"order",slideOrder);auto json=to_string(request.Stringify());
        NativeString reply(capy_document_tab_slide(json.c_str()),capy_string_free);
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
            Grid content;double inset=tile()/2;
            auto title=label(data,tab.text.Text());title.TextTrimming(TextTrimming::CharacterEllipsis);title.Margin({inset+16,0,inset+16,0});title.HorizontalAlignment(HorizontalAlignment::Center);title.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(title);
            auto mark=icon(L"close",data->theme());mark.Width(16);mark.HorizontalAlignment(HorizontalAlignment::Right);mark.VerticalAlignment(VerticalAlignment::Center);mark.Margin({0,0,inset-8,0});content.Children().Append(mark);
            SlideCopy copy;copy.id=id;copy.copy.Child(content);copy.copy.CornerRadius({corner(),corner(),corner(),corner()});
            copy.copy.Background(id==selectedId?data->glass(L"document_tab"):id==source?data->tint(L"text",18):clear());
            copy.copy.Width(num(b,L"width"));copy.copy.Height(num(b,L"height"));
            Canvas::SetLeft(copy.copy,num(b,L"x")-bounds.X);Canvas::SetTop(copy.copy,num(b,L"y")-bounds.Y);Canvas::SetZIndex(copy.copy,id==source?2:0);
            overlay.Children().Append(copy.copy);copies.push_back(copy);tab.box.Opacity(0);
        }
        overlay.Visibility(Visibility::Visible);
    }
    void endSlide(){
        presentation.Cancel();finishing=false;
        AutomationProperties::SetItemStatus(root,data->caption(L"header",L"ready"));
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
    bool claim(){
        if(!pointer||!contact||!tabs.contains(source))return false;
        if(PointerGesture::Captured(root,*pointer))return true;
        releasing=true;auto button=tabs.at(source).select;button.CancelDirectManipulations();button.ReleasePointerCapture(contact);
        bool accepted=root.CapturePointer(contact);releasing=false;if(!accepted)cancel();return accepted;
    }
    void release(bool keepSlide=false){if(!pointer)return;pointer.reset();contact=nullptr;dragging=false;held=false;dragCursor(root,false,false);if(!keepSlide)endSlide();root.ReleasePointerCaptures();
        if(recognizing){recognizing=false;hold.CompleteGesture();}if(!keepSlide)AutomationProperties::SetItemStatus(root,data->caption(L"header",L"ready"));}
    void cancel(){if(!pointer&&!finishing)return;auto previous=contact;bool captured=pointer.has_value();Button button=pointer&&tabs.contains(source)?tabs.at(source).select:nullptr;MenuFlyout menu=button?tabs.at(source).menu:nullptr;
        ++contactGeneration;release();if(!captured)endSlide();if(button&&previous)button.ReleasePointerCapture(previous);if(menu)menu.Hide();}
    bool cancelAll(){bool active=pointer.has_value()||finishing;cancel();return (rowDrag&&rowDrag->Cancel())||active;}
    void move(PointerRoutedEventArgs const& e){if(pointer!=e.Pointer().PointerId())return;position=e.GetCurrentPoint(root).Position();
        if(recognizing)hold.Move(root,e);
        if(!dragging&&hold.Moved()){if(!claim())return;dragging=true;held=false;dragCursor(root,e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch,true);if(tabs.contains(source))tabs.at(source).menu.Hide();beginSlide();}
        if(dragging){updateSlide();AutomationProperties::SetItemStatus(root,data->caption(L"header",L"dragging"));}if(dragging||held)e.Handled(true);
    }
    void up(PointerRoutedEventArgs const& e){if(pointer!=e.Pointer().PointerId())return;
        auto point=e.GetCurrentPoint(root);
        position=point.Position();bool dragged=dragging,wasHeld=held;
        if(dragged)updateSlide();bool attached=dragged&&slide.Size()&&flag(slide,L"attached");auto request=slideRequest();
        release(attached);
        if(attached){
            finishing=true;auto serial=contactGeneration;request.Insert(L"op",S(L"slide"));
            send(request,[weak=weak_from_this(),serial]{if(auto self=weak.lock();self&&self->contactGeneration==serial){
                self->presentation.AfterLayout([weak,serial]{if(auto self=weak.lock();self&&self->contactGeneration==serial)self->endSlide();});
            }});
        }
        if(dragged||wasHeld)e.Handled(true);
    }
    void activateItem(winrt::Windows::Foundation::IInspectable const& value){for(auto const& [id,row]:rows)if(value==row.item||value==row.item.Content()){select(id);return;}}
    void show(FrameworkElement anchor=nullptr){refresh();popup.ShowAt(anchor?anchor:selector);}
    void styleTab(Tab& t){
        t.box.CornerRadius({corner(),corner(),corner(),corner()});t.select.CornerRadius({corner(),corner(),corner(),corner()});t.close.CornerRadius({12*CornerFit,12*CornerFit,12*CornerFit,12*CornerFit});
        for(auto const& part:{t.select,t.close}){
            part.Background(clear());
            buttonFeedback(part,data->brush(L"text"),part==t.close?18:0,part==t.close?41:0);
        }
    }
    void paintTab(Tab& t,bool chosen){t.chosen=chosen;t.box.Background(chosen?data->glass(L"document_tab"):clear());t.shade.Background(data->tint(L"text",t.pressed?(chosen?23:41):t.hovered?(chosen?8:18):0));}
    void separators(){bool previousIdle=false;for(auto value:array(model(),L"tabs")){auto id=uint64_t(num(value.GetObject(),L"id"));auto found=tabs.find(id);if(found==tabs.end())continue;auto& tab=found->second;bool idle=!tab.chosen&&!tab.hovered&&!tab.pressed;tab.separator.Visibility(previousIdle&&idle?Visibility::Visible:Visibility::Collapsed);previousIdle=idle;}}
    void hover(uint64_t id,bool active){auto found=tabs.find(id);if(found==tabs.end())return;found->second.hovered=active;paintTab(found->second,found->second.chosen);separators();}
    void layout(){auto count=array(model(),L"tabs").Size();bool one=count<2,compact=!one&&capy_document_tabs_compact(float(root.ActualWidth()),count);
        plain.Visibility(one?Visibility::Visible:Visibility::Collapsed);
        strip.Visibility(one||compact?Visibility::Collapsed:Visibility::Visible);selector.Visibility(!one&&compact?Visibility::Visible:Visibility::Collapsed);
        root.Background(headerSurface(data));root.CornerRadius({corner(),corner(),corner(),corner()});strip.Spacing(gap());
        for(auto& [id,t]:tabs){t.box.CornerRadius({corner(),corner(),corner(),corner()});t.select.CornerRadius({corner(),corner(),corner(),corner()});t.shade.CornerRadius({corner(),corner(),corner(),corner()});}
        double inset=tile()/2;
        for(auto& [id,t]:tabs){t.select.Padding({inset+16,0,inset+16,0});t.separator.Margin({0,tile()*.1,0,tile()*.1});t.close.Margin({0,0,inset-12,0});}
        if(!compact){double width=std::max(0.,(root.ActualWidth()-gap()*(std::max(1u,count)-1))/std::max(1u,count));for(auto& [id,t]:tabs)t.box.Width(width);}}
    void refresh(){
        if((pointer||finishing)&&(!available()||!findId(array(model(),L"tabs"),double(source)).Size()||epoch!=to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")))))cancel();
        if(pointer&&slideHits.Size()){A current;for(auto v:array(model(),L"tabs"))current.Append(N(num(v.GetObject(),L"id")));if(current.Stringify()!=slideOrder.Stringify())cancel();}
        updating=true;auto weak=weak_from_this();auto m=model();auto selectedId=uint64_t(num(m,L"selected"));bool selectionChanged=selectedId!=selectedModel;selectedModel=selectedId;std::vector<hstring> order;uint32_t index=0;std::set<uint64_t> live;
        for(auto v:array(m,L"tabs")){auto spec=v.GetObject();auto id=uint64_t(num(spec,L"id"));live.insert(id);auto label=marked(spec);auto key=to_hstring(id);order.push_back(key);
            if(!tabs.contains(id)){
                Tab t;t.shade.IsHitTestVisible(false);t.shade.CornerRadius({corner(),corner(),corner(),corner()});t.box.Children().Append(t.shade);t.separator.Width(1);t.separator.HorizontalAlignment(HorizontalAlignment::Left);t.separator.IsHitTestVisible(false);t.separator.Background(data->line(L"border"));t.box.Children().Append(t.separator);
                t.select=button(data,label,[weak,id]{if(auto self=weak.lock();self&&!self->dragging&&!self->held)self->select(id);});t.select.HorizontalAlignment(HorizontalAlignment::Stretch);t.select.VerticalAlignment(VerticalAlignment::Stretch);t.select.HorizontalContentAlignment(HorizontalAlignment::Center);t.select.MinWidth(0);t.select.FontWeight(Windows::UI::Text::FontWeights::Normal());
                t.text=CapyUi::label(data,label);t.text.TextTrimming(TextTrimming::CharacterEllipsis);t.select.Content(t.text);AutomationProperties::SetAutomationId(t.select,L"drawing-tab-"+key);
                t.close=button(data,data->copyCaption(L"header",L"close_drawing"),[weak,id]{if(auto self=weak.lock())self->close(id);});t.close.Content(icon(L"close",data->theme()));t.close.Padding({0,0,0,0});t.close.MinWidth(0);t.close.Width(24);t.close.Height(24);t.close.HorizontalAlignment(HorizontalAlignment::Right);t.close.VerticalAlignment(VerticalAlignment::Center);AutomationProperties::SetAutomationId(t.close,L"drawing-close-"+key);
                t.menu=menu(id);t.select.KeyDown([weak,id](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock())self->tabKey(id,e);});t.select.ContextRequested([weak,id](auto&&,ContextRequestedEventArgs const& e){e.Handled(true);if(auto self=weak.lock();self&&self->available()&&!self->pointer)self->tabs.at(id).menu.ShowAt(self->tabs.at(id).select);});PointerGesture::OwnHolding(t.select);t.select.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak,id](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
                    auto p=e.GetCurrentPoint(self->root);if(p.Properties().IsMiddleButtonPressed()){self->close(id);e.Handled(true);return;}
                    if(self->pointer){if(self->pointer!=p.PointerId())self->cancel();return;}
                    if(!self->available()||self->finishing||!PointerGesture::PrimaryContact(p))return;
                    ++self->contactGeneration;AutomationProperties::SetItemStatus(self->root,self->data->caption(L"header",L"pressed"));self->source=id;self->pointer=p.PointerId();self->contact=e.Pointer();self->origin=self->position=p.Position();self->epoch=to_hstring(uint64_t(num(object(self->data->state,L"document_file"),L"epoch")));
                    self->recognizing=true;self->hold.Begin(self->tabs.at(id).select,p,p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse?Microsoft::UI::Input::GestureSettings::None:Microsoft::UI::Input::GestureSettings::Hold);
                }})),true);t.box.Children().Append(t.select);t.box.Children().Append(t.close);
                t.box.PointerEntered([weak,id](auto&&,auto&&){if(auto self=weak.lock())self->hover(id,true);});t.box.PointerExited([weak,id](auto&&,auto&&){if(auto self=weak.lock())self->hover(id,false);});
                t.box.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak,id](auto&&,auto&&){if(auto self=weak.lock()){auto& tab=self->tabs.at(id);tab.pressed=true;self->paintTab(tab,tab.chosen);self->separators();}})),true);
                for(auto event:{UIElement::PointerReleasedEvent(),UIElement::PointerCanceledEvent(),UIElement::PointerCaptureLostEvent()})t.box.AddHandler(event,box_value(PointerEventHandler([weak,id](auto&&,auto&&){if(auto self=weak.lock()){auto& tab=self->tabs.at(id);tab.pressed=false;self->paintTab(tab,tab.chosen);self->separators();}})),true);
                styleTab(t);tabs.emplace(id,t);
                Row r;r.item.Tag(box_value(key));AutomationProperties::SetAutomationId(r.item,L"drawing-row-"+key);Grid row;row.ColumnDefinitions().Append(ColumnDefinition());ColumnDefinition first;first.Width({28,GridUnitType::Pixel});row.ColumnDefinitions().InsertAt(0,first);ColumnDefinition lastColumn;lastColumn.Width({28,GridUnitType::Pixel});row.ColumnDefinitions().Append(lastColumn);auto title=str(spec,L"title");
                r.grip=button(data,data->copyCaption(L"header",L"reorder_drawing"),[]{});r.grip.Content(panelGrip(data->theme()));r.grip.Padding({0,0,0,0});r.grip.MinWidth(0);AutomationProperties::SetAutomationId(r.grip,L"drawing-grip-"+key);
                r.remove=button(data,data->copyCaption(L"header",L"close_drawing"),[weak,id]{if(auto self=weak.lock())self->close(id);});r.remove.Content(icon(L"close",data->theme()));r.remove.Padding({0,0,0,0});r.remove.MinWidth(0);Grid::SetColumn(r.remove,2);AutomationProperties::SetAutomationId(r.remove,L"drawing-row-close-"+key);r.menu=menu(id);
                StackPanel labels;r.text=CapyUi::label(data,label);r.location=CapyUi::label(data,L"");r.location.FontSize(data->textSize()/1.2);opticalText(r.location,r.location.FontSize());r.location.TextTrimming(TextTrimming::CharacterEllipsis);labels.Children().Append(r.text);labels.Children().Append(r.location);Grid::SetColumn(labels,1);row.Children().Append(r.grip);row.Children().Append(labels);row.Children().Append(r.remove);r.item.Content(row);
                rowDrag->Attach(r.item,r.grip,r.remove,nullptr,r.menu,key);rows.emplace(id,r);
            }
            auto& t=tabs.at(id);t.text.Text(label);t.text.Foreground(data->brush(L"text"));paintTab(t,id==uint64_t(num(m,L"selected")));t.select.IsEnabled(available());t.close.IsEnabled(available());tooltip(t.select,str(spec,L"location",label));AutomationProperties::SetName(t.select,label);
            auto& r=rows.at(id);r.text.Text(label);auto location=str(spec,L"location");if(r.locationSource!=location){r.locationSource=location;r.location.Text(data->caption(O({{L"type",S(L"drawing_location")},{L"location",S(location)}})));AutomationProperties::SetHelpText(r.location,location);}r.item.IsEnabled(available());r.remove.IsEnabled(available());
            auto title=str(spec,L"title");if(r.title!=title){r.title=title;auto close=data->caption(O({{L"type",S(L"close_drawing")},{L"title",S(title)}}));AutomationProperties::SetName(r.remove,close);tooltip(r.remove,close);AutomationProperties::SetName(t.close,close);tooltip(t.close,close);}
            auto children=strip.Children();uint32_t at;if(index>=children.Size()||children.GetAt(index)!=t.box){if(children.IndexOf(t.box,at))children.RemoveAt(at);children.InsertAt(index,t.box);}
            auto items=list.Items();if(index>=items.Size()||items.GetAt(index)!=r.item){if(items.IndexOf(r.item,at))items.RemoveAt(at);items.InsertAt(index,r.item);}++index;
            if(id==uint64_t(num(m,L"selected"))){selectorText.Text(label);if(!showing||selectionChanged)list.SelectedItem(r.item);}
        }
        while(strip.Children().Size()>index)strip.Children().RemoveAtEnd();while(list.Items().Size()>index)list.Items().RemoveAtEnd();
        std::erase_if(tabs,[&](auto const& p){return !live.contains(p.first);});std::erase_if(rows,[&](auto const& p){return !live.contains(p.first);});rowDrag->Refresh(order);selector.IsEnabled(available());
        auto error=str(m,L"storage_error");if(!storageTooltipReady||storageError!=error){storageTooltipReady=true;storageError=error;tooltip(selector,error.empty()?data->caption(L"header",L"choose_drawing"):data->caption(O({{L"type",S(L"drawing_storage")},{L"detail",S(error)}})));}
        AutomationProperties::SetHelpText(root,error);if(plain.Text()!=plainTitle())plain.Text(plainTitle());plain.Foreground(data->brush(L"text"));separators();layout();updating=false;
    }
    void init(){
        selector=button(data,L"",[]{});selectorText=label(data,L"");selectorText.TextTrimming(TextTrimming::CharacterEllipsis);selectorText.VerticalAlignment(VerticalAlignment::Center);
        Grid face;face.ColumnSpacing(6);ColumnDefinition text;text.Width({1,GridUnitType::Star});ColumnDefinition arrow;arrow.Width({1,GridUnitType::Auto});face.ColumnDefinitions().Append(text);face.ColumnDefinitions().Append(arrow);face.Children().Append(selectorText);auto chevron=icon(L"chevron-down",data->theme());Grid::SetColumn(chevron,1);face.Children().Append(chevron);selector.Content(face);
        buttonColors(data,selector);
        auto weak=weak_from_this();root.Tag(O({{L"header_source",O({{L"kind",S(L"native")}})},{L"native_keys",B(true)}}));root.Background(clear());strip.Orientation(Orientation::Horizontal);
        plain=label(data,L"");plain.FontWeight(Windows::UI::Text::FontWeights::SemiBold());plain.FontSize(data->textSize());plain.TextTrimming(TextTrimming::CharacterEllipsis);plain.TextWrapping(TextWrapping::NoWrap);
        plain.TextAlignment(TextAlignment::Center);plain.VerticalAlignment(VerticalAlignment::Center);plain.Padding({8,0,8,0});plain.IsHitTestVisible(false);AutomationProperties::SetAutomationId(plain,L"drawing-title");
        root.Children().Append(plain);root.Children().Append(strip);root.Children().Append(selector);selector.MinWidth(0);selector.HorizontalAlignment(HorizontalAlignment::Stretch);selector.VerticalAlignment(VerticalAlignment::Stretch);AutomationProperties::SetAutomationId(selector,L"drawing-selector");AutomationProperties::SetItemStatus(selector,data->caption(L"header",L"closed"));AutomationProperties::SetName(root,data->caption(L"header",L"drawings"));AutomationProperties::SetAutomationId(root,L"drawing-tabs");
        selector.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->show();});root.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->layout();});
        root.AllowDrop(true);
        root.DragOver([weak](auto&&,DragEventArgs const& event){if(auto self=weak.lock();self&&fileDrag(event)){
            using winrt::Windows::ApplicationModel::DataTransfer::DataPackageOperation;
            auto command=find(array(self->data->state,L"commands"),L"id",L"open_document");bool open=flag(command,L"enabled");
            event.AcceptedOperation(open?DataPackageOperation::Copy:DataPackageOperation::None);event.DragUIOverride().Caption(open?str(object(object(self->data->catalog,L"bootstrap"),L"recovery"),L"open"):str(command,L"disabled_reason"));event.Handled(true);
        }});
        root.Drop([weak](auto&&,DragEventArgs const& event){if(auto self=weak.lock();self&&fileDrag(event))
            receiveFileDrop(event,O({{L"operation",S(L"open_paths")}}),[weak](std::string json){if(auto self=weak.lock())self->data->document(std::move(json));});});
        overlay.HorizontalAlignment(HorizontalAlignment::Left);overlay.VerticalAlignment(VerticalAlignment::Top);overlay.IsHitTestVisible(false);overlay.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(overlay,L"drawing-tab-slide");AutomationProperties::SetName(overlay,data->caption(L"header",L"slide"));root.Children().Append(overlay);
        AutomationProperties::SetAutomationId(list,L"drawing-list");
        list.IsItemClickEnabled(true);list.SelectionMode(ListViewSelectionMode::Single);list.MaxHeight(420);listSurface.Children().Append(list);listSurface.Width(360);popup.Content(listSurface);TrackPopup(popup,data);
        rowDrag=std::make_unique<WorkspaceRowDrag>(list,listSurface,[weak]{auto s=weak.lock();return s&&s->available();},[weak](hstring id,std::optional<hstring> before){if(auto s=weak.lock())s->send(O({{L"op",S(L"reorder")},{L"id",N(double(std::stoull(id.c_str())))},{L"before",before?N(double(std::stoull(before->c_str()))):JsonValue::CreateNullValue()}}));},[]{});
        popup.Closed([weak](auto&&,auto&&){if(auto s=weak.lock()){s->showing=false;AutomationProperties::SetItemStatus(s->selector,s->data->caption(L"header",L"closed"));s->rowDrag->Cancel();}});popup.Opened([weak,data=data](auto&&,auto&&){if(auto s=weak.lock()){s->showing=true;AutomationProperties::SetItemStatus(s->selector,data->caption(L"header",L"open"));}});
        list.ItemClick([weak](auto&&,ItemClickEventArgs const& e){if(auto s=weak.lock();s&&!s->updating&&!s->rowDrag->SuppressClick()){s->activateItem(e.ClickedItem());}});
        ShortcutKeys(list,[weak](auto&&,KeyRoutedEventArgs const& e){if(auto s=weak.lock()){
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
        hold.Holding([weak](auto&&,Microsoft::UI::Input::HoldingEventArgs const& e){if(auto s=weak.lock();s&&s->pointer&&!s->dragging&&e.HoldingState()==Microsoft::UI::Input::HoldingState::Started&&s->claim()){s->held=true;if(s->tabs.contains(s->source)){Primitives::FlyoutShowOptions options;options.ShowMode(Primitives::FlyoutShowMode::Transient);s->tabs.at(s->source).menu.ShowAt(s->tabs.at(s->source).select,options);}}});
        root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){if(auto s=weak.lock();s&&s->pointer&&s->pointer!=e.Pointer().PointerId())s->cancel();})),true);
        root.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){if(auto s=weak.lock())s->move(e);})),true);
        PointerGesture::OnLeaving(root,[weak](PointerRoutedEventArgs const& e){if(auto s=weak.lock();s&&s->pointer==e.Pointer().PointerId()&&!s->held&&!s->dragging){
            s->release();
        }});
        PointerGesture::OnCompleted(root,[weak](PointerRoutedEventArgs const& e,bool cancel){if(auto s=weak.lock();s&&!s->releasing&&s->pointer==e.Pointer().PointerId()){
            if(cancel)s->cancel();else s->up(e);
        }},PointerCaptureLoss::CompleteLift);
        root.LostFocus([weak](auto&&,auto&&){if(auto s=weak.lock();s&&s->pointer&&!s->held&&!s->releasing)s->cancel();});
        root.Unloaded([weak](auto&&,auto&&){if(auto s=weak.lock()){s->cancel();s->rowDrag->Cancel();}});
        root.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(e.Key()==Windows::System::VirtualKey::Escape)if(auto s=weak.lock();s&&s->pointer){s->cancel();e.Handled(true);}});
        refresh();
    }
};
}
