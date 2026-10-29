#pragma once
#include "UiControls.h"
#include "NativeMenus.h"
#include "PointerGesture.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Input.h>
#include "BitmapUpload.h"
#include <optional>

namespace CapyUi {
// Capture, focus and native drawing stay here. Geometry and edits stay in Rust.
struct ProofDial:std::enable_shared_from_this<ProofDial>{
    std::shared_ptr<WorkspaceData> data;
    ContentControl root;Canvas canvas;Viewbox view;
    Shapes::Ellipse field,marker,markerHalo,focusRing;ImageBrush fieldImage;std::shared_ptr<BitmapUpload> upload=BitmapUpload::Create();std::array<Shapes::Polyline,2> arcs;
    std::array<Shapes::Ellipse,2> handles,handleHalos;bool initialized=false;hstring paintedTheme;std::array<TextBlock,4> texts;std::array<Image,4> icons;
    Button reset;std::array<MenuFlyoutItem,4> resetItems;std::shared_ptr<ControlCapture> capture;winrt::Windows::Foundation::Point origin{},last{};
    std::optional<winrt::Windows::System::VirtualKey> heldKey;uint64_t pressedAt=0;winrt::Windows::Foundation::Point pressedPoint{};uint8_t pressedPart=0;
    hstring epoch,identity,key;uint8_t part=0;static constexpr float side=256;
    ~ProofDial(){upload->Close();}
    J form()const{return object(data->model,L"windows_proof_form");}
    J geometry(std::optional<winrt::Windows::Foundation::Point> point={})const{
        J request=O({{L"size",N(side)},{L"recipe",object(form(),L"rendition")}});
        if(point){A p;p.Append(N(point->X));p.Append(N(point->Y));request.Insert(L"point",p);}
        auto json=to_string(request.Stringify());NativeString reply(capy_proof_dial(json.c_str()),capy_string_free);
        return reply?J::Parse(to_hstring(reply.get())):J{};
    }
    void send(J const& action,hstring const& at){data->dispatch(O({{L"windows_proof_action",action},{L"windows_epoch",S(at)}}));}
    void control(uint8_t selected,J const& edit,hstring const& phase=L""){auto action=O({{L"type",S(L"control")},{L"part",N(selected)},{L"edit",edit}});if(!phase.empty())action.Insert(L"phase",S(phase));send(action,heldKey?epoch:to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch"))));}
    void finishKey(bool cancel){if(!heldKey)return;control(part,O({{L"type",S(L"step")},{L"axis",N(0)},{L"steps",N(0)}}),cancel?L"cancel":L"up");heldKey.reset();}
    void contact(hstring const& phase){A start,point;start.Append(N(origin.X));start.Append(N(origin.Y));point.Append(N(last.X));point.Append(N(last.Y));
        send(O({{L"type",S(L"dial")},{L"phase",S(phase)},{L"size",N(side)},{L"origin",start},{L"point",point}}),epoch);
    }
    void cancel(){finishKey(true);if(capture)capture->Cancel();}
    static void circle(Shapes::Ellipse const& e,A const& point,double radius){e.Width(radius*2);e.Height(radius*2);Canvas::SetLeft(e,point.GetNumberAt(0)-radius);Canvas::SetTop(e,point.GetNumberAt(1)-radius);}
    void refresh(){
        auto copy=object(form(),L"copy");auto title=str(copy,L"balance_contrast");
        AutomationProperties::SetName(root,title);AutomationProperties::SetHelpText(root,title);root.Language(data->language());
        AutomationProperties::SetName(reset,str(copy,L"reset_sdr"));tooltip(reset,str(copy,L"reset_sdr"));
        AutomationProperties::SetItemStatus(root,data->caption(L"header",capture&&capture->Active()?L"dragging":L"ready"));
        std::array<hstring,4> labels{data->caption(L"color",L"reset_balance_contrast"),data->caption(L"color",L"reset_brightness"),data->caption(L"color",L"reset_highlight_color"),str(copy,L"reset_sdr")};
        for(uint32_t i=0;i<labels.size();++i){resetItems[i].Text(labels[i]);AutomationProperties::SetName(resetItems[i],labels[i]);}
        auto current=array(form(),L"identity").Stringify();if(current!=identity){cancel();identity=current;}
        auto next=object(form(),L"rendition").Stringify()+data->theme()+to_hstring(root.FocusState()==FocusState::Keyboard);if(next==key)return;key=next;
        auto g=geometry();if(!g.Size()||g.HasKey(L"error"))return;
        bool setup=!initialized;initialized=true;bool themeChanged=paintedTheme!=data->theme();paintedTheme=data->theme();
        auto ink=data->brush(L"text");if(setup)circle(field,array(g,L"center"),num(g,L"radius"));circle(marker,array(g,L"marker"),num(g,L"marker_radius"));circle(markerHalo,array(g,L"marker"),num(g,L"marker_radius"));
        auto tracks=array(g,L"arcs");for(uint32_t i=0;i<2;++i){auto track=tracks.GetObjectAt(i);auto spec=object(track,L"geometry");if(setup){PointCollection points;
            for(auto item:array(track,L"path")){auto p=item.GetArray();points.Append({float(p.GetNumberAt(0)),float(p.GetNumberAt(1))});}
            arcs[i].Points(points);LinearGradientBrush gradient;gradient.MappingMode(BrushMappingMode::Absolute);gradient.StartPoint({points.GetAt(0).X,0});gradient.EndPoint({points.GetAt(points.Size()-1).X,0});
            auto stop=[&](double offset,Windows::UI::Color color){GradientStop item;item.Offset(offset);item.Color(color);gradient.GradientStops().Append(item);};
            if(i==0){stop(0,{255,10,10,10});stop(.5,{255,140,140,140});stop(1,{255,255,255,255});}else{stop(0,{255,242,242,242});stop(1,{255,38,140,217});}
            arcs[i].Stroke(gradient);arcs[i].StrokeThickness(num(spec,L"width"));arcs[i].StrokeStartLineCap(PenLineCap::Round);arcs[i].StrokeEndLineCap(PenLineCap::Round);}
            circle(handles[i],array(track,L"point"),num(spec,L"marker_radius"));circle(handleHalos[i],array(track,L"point"),num(spec,L"marker_radius"));
        }
        auto selected=part==0?array(g,L"marker"):part<=2?array(tracks.GetObjectAt(part-1),L"point"):array(g,L"marker");double radius=part==0?num(g,L"marker_radius"):part<=2?num(object(tracks.GetObjectAt(part-1),L"geometry"),L"marker_radius"):num(g,L"marker_radius");circle(focusRing,selected,radius+3);focusRing.Visibility(root.FocusState()==FocusState::Keyboard?Visibility::Visible:Visibility::Collapsed);
        auto readouts=array(g,L"readouts"),values=array(g,L"percentages"),names=array(g,L"icons");
        for(uint32_t i=0;i<4;++i){auto r=readouts.GetObjectAt(i);auto at=array(r,L"text"),box=array(r,L"icon");
            auto text=to_hstring(int(std::lround(values.GetNumberAt(i))))+L"%";
            if(texts[i].Text()!=text)texts[i].Text(text);texts[i].Foreground(ink);texts[i].FontSize(num(g,L"text_size"));texts[i].Width(52);texts[i].TextAlignment(TextAlignment::Center);
            Canvas::SetLeft(texts[i],at.GetNumberAt(0)-26);Canvas::SetTop(texts[i],at.GetNumberAt(1)-num(g,L"text_size"));
            if(themeChanged)icons[i].Source(icon(names.GetStringAt(i),data->theme()).Source());icons[i].Width(box.GetNumberAt(2));icons[i].Height(box.GetNumberAt(3));Canvas::SetLeft(icons[i],box.GetNumberAt(0));Canvas::SetTop(icons[i],box.GetNumberAt(1));
        }
        auto box=array(g,L"reset");reset.Width(box.GetNumberAt(2));reset.Height(box.GetNumberAt(3));Canvas::SetLeft(reset,box.GetNumberAt(0));Canvas::SetTop(reset,box.GetNumberAt(1));if(themeChanged)reset.Content(icon(L"reset",data->theme()));
    }
    void init(){
        auto weak=weak_from_this();root.IsTabStop(true);edgeTag(root,L"edge_contact",S(L"input"));root.MinWidth(128);root.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetAutomationId(root,L"proof-dial");
        canvas.Width(side);canvas.Height(side);canvas.Background(clear());view.Child(canvas);view.Stretch(Stretch::Uniform);root.Content(view);
        field.Fill(fieldImage);field.IsHitTestVisible(false);canvas.Children().Append(field);auto bytes=std::make_shared<std::vector<uint8_t>>();
        upload->Submit({[bytes]{bytes->resize(256*256*4);if(!capy_proof_texture(256,bytes->data(),bytes->size()))bytes->clear();return std::span<uint8_t const>(*bytes);},0,256*256*4,256,256,true,true,[weak]{return !weak.expired();},[weak](bool accepted){if(auto self=weak.lock();self&&accepted)self->fieldImage.ImageSource(self->upload->Image());}});
        auto halo=[](Shapes::Ellipse const& ring){ring.Stroke(fill({166,0,0,0}));ring.StrokeThickness(4);ring.Fill(clear());ring.IsHitTestVisible(false);};halo(markerHalo);
        for(auto const& handle:handles){handle.Stroke(fill({255,255,255,255}));handle.StrokeThickness(2);handle.Fill(clear());}
        for(uint32_t i=0;i<2;++i){arcs[i].IsHitTestVisible(false);handles[i].IsHitTestVisible(false);halo(handleHalos[i]);canvas.Children().Append(arcs[i]);canvas.Children().Append(handleHalos[i]);canvas.Children().Append(handles[i]);}
        marker.Stroke(fill({255,255,255,255}));marker.Fill(clear());marker.StrokeThickness(2);marker.IsHitTestVisible(false);canvas.Children().Append(markerHalo);canvas.Children().Append(marker);focusRing.Stroke(fill({166,255,255,255}));focusRing.StrokeThickness(1);focusRing.IsHitTestVisible(false);canvas.Children().Append(focusRing);
        for(uint32_t i=0;i<4;++i){texts[i]=label(data,L"",false,false);texts[i].Opacity(.62);texts[i].IsHitTestVisible(false);icons[i].Opacity(.62);icons[i].IsHitTestVisible(false);canvas.Children().Append(texts[i]);canvas.Children().Append(icons[i]);}
        reset.Padding({0});reset.MinWidth(0);reset.MinHeight(0);reset.Background(clear());reset.BorderThickness({0});AutomationProperties::SetAutomationId(reset,L"proof-dial-reset");
        buttonColors(data,reset);
        reset.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->control(3,O({{L"type",S(L"reset")}}));});canvas.Children().Append(reset);
        capture=captureControl(data,root,[weak](uint32_t,bool cancel,PointerRoutedEventArgs const* event){if(auto self=weak.lock()){
            if(event&&!cancel)self->last=event->GetCurrentPoint(self->canvas).Position();
            if(cancel)self->pressedAt=0;
            self->contact(cancel?L"cancel":L"up");AutomationProperties::SetItemStatus(self->root,self->data->caption(L"header",L"ready"));
            if(event)event->Handled(true);
        }});
        root.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->canvas);if(self->capture->Active()||!PointerGesture::PrimaryContact(p))return;
            auto g=self->geometry(p.Position());auto hit=g.GetNamedValue(L"hit",JsonValue::CreateNullValue());if(hit.ValueType()!=JsonValueType::Number||hit.GetNumber()==3)return;
            self->finishKey(true);self->root.Focus(FocusState::Pointer);
            auto at=p.Position();auto selected=uint8_t(hit.GetNumber());bool repeated=self->pressedAt&&p.Timestamp()-self->pressedAt<=uint64_t(GetDoubleClickTime())*1000&&selected==self->pressedPart&&std::hypot(at.X-self->pressedPoint.X,at.Y-self->pressedPoint.Y)<=8;
            self->pressedAt=repeated?0:p.Timestamp();self->pressedPoint=at;self->pressedPart=selected;
            if(repeated){self->part=selected;self->control(selected,O({{L"type",S(L"reset")}}));e.Handled(true);return;}
            if(!self->capture->Begin(e))return;
            self->part=uint8_t(hit.GetNumber());self->origin=self->last=p.Position();self->epoch=to_hstring(uint64_t(num(object(self->data->state,L"document_file"),L"epoch")));
            self->contact(L"down");AutomationProperties::SetItemStatus(self->root,self->data->caption(L"header",L"dragging"));e.Handled(true);
        }});
        root.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->capture->Owns(e.Pointer().PointerId())){self->last=e.GetCurrentPoint(self->canvas).Position();self->contact(L"move");e.Handled(true);}});
        root.GotFocus([weak](auto&&,auto&&){if(auto self=weak.lock()){self->key=L"";self->refresh();}});root.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock()){self->cancel();self->key=L"";self->refresh();}});root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->finishKey(true);});
        root.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
            using winrt::Windows::System::VirtualKey;auto key=e.Key();if(key==VirtualKey::Escape){self->cancel();e.Handled(true);return;}
            if(self->capture->Active())return;
            if(key==VirtualKey::Home){self->finishKey(false);self->control(self->part,O({{L"type",S(L"reset")}}));e.Handled(true);}
            else if(key==VirtualKey::Left||key==VirtualKey::Right||key==VirtualKey::Up||key==VirtualKey::Down){if(self->heldKey&&*self->heldKey!=key)self->finishKey(false);
                bool first=!self->heldKey;if(first){self->heldKey=key;self->epoch=to_hstring(uint64_t(num(object(self->data->state,L"document_file"),L"epoch")));}
                int amount=(GetKeyState(VK_SHIFT)&0x8000)?10:1;
                self->control(self->part,O({{L"type",S(L"step")},{L"axis",N(key==VirtualKey::Up||key==VirtualKey::Down?1:0)},{L"steps",N((key==VirtualKey::Right||key==VirtualKey::Up?1:-1)*amount)}}),first?L"down":L"move");e.Handled(true);}
        }});
        root.KeyUp([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->heldKey==e.Key()){self->finishKey(false);e.Handled(true);}});
        MenuFlyout menu;TrackPopup(menu,data);
        for(uint8_t i=0;i<4;++i){auto item=resetItems[i];AutomationProperties::SetAutomationId(item,L"proof-dial-reset-"+to_hstring(i));item.Click([weak,i](auto&&,auto&&){if(auto self=weak.lock())self->control(i,O({{L"type",S(L"reset")}}));});menu.Items().Append(item);}root.ContextFlyout(menu);
        refresh();
    }
};
inline ContentControl ProofDialControl(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){auto view=std::make_shared<ProofDial>();view->data=data;view->init();bindings.emplace_back([view]{view->refresh();});return view->root;}
}
