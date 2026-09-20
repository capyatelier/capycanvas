#pragma once
#include "UiControls.h"
#include "NativeMenus.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Input.h>
#include <robuffer.h>
#include <optional>

namespace CapyUi {
// Capture, focus and native drawing stay here. Geometry and edits stay in Rust.
struct ProofDial:std::enable_shared_from_this<ProofDial>{
    std::shared_ptr<WorkspaceData> data;
    ContentControl root;Canvas canvas;Viewbox view;
    Shapes::Ellipse field,marker;std::array<Shapes::Polyline,2> arcs;
    std::array<Shapes::Ellipse,2> handles;std::array<TextBlock,4> texts;std::array<Image,4> icons;
    Button reset;std::optional<uint32_t> pointer;winrt::Windows::Foundation::Point origin{},last{};
    hstring epoch,identity,key;uint8_t part=0;static constexpr float side=256;
    J form()const{return object(data->model,L"windows_proof_form");}
    J geometry(std::optional<winrt::Windows::Foundation::Point> point={})const{
        J request=O({{L"size",N(side)},{L"recipe",object(form(),L"rendition")}});
        if(point){A p;p.Append(N(point->X));p.Append(N(point->Y));request.Insert(L"point",p);}
        auto json=to_string(request.Stringify());std::unique_ptr<char,decltype(&capy_string_free)> reply(capy_proof_dial(json.c_str()),capy_string_free);
        return reply?J::Parse(to_hstring(reply.get())):J{};
    }
    void send(J const& action,hstring const& at){data->dispatch(O({{L"windows_proof_action",action},{L"windows_epoch",S(at)}}));}
    void control(uint8_t selected,J const& edit){send(O({{L"type",S(L"control")},{L"part",N(selected)},{L"edit",edit}}),to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch"))));}
    void contact(hstring const& phase){A start,point;start.Append(N(origin.X));start.Append(N(origin.Y));point.Append(N(last.X));point.Append(N(last.Y));
        send(O({{L"type",S(L"dial")},{L"phase",S(phase)},{L"size",N(side)},{L"origin",start},{L"point",point}}),epoch);
    }
    std::vector<std::pair<weak_ref<UIElement>,ManipulationModes>> scrollModes;
    void releaseScroll(){for(auto const& [weak,mode]:scrollModes)if(auto content=weak.get())content.ManipulationMode(mode);scrollModes.clear();}
    void claimScroll(){
        root.CancelDirectManipulations();
        for(auto node=VisualTreeHelper::GetParent(root);node;node=VisualTreeHelper::GetParent(node))if(auto scroll=node.try_as<ScrollViewer>())
            if(auto content=scroll.Content().try_as<UIElement>()){auto mode=content.ManipulationMode();scrollModes.emplace_back(make_weak(content),mode);content.ManipulationMode(mode&~ManipulationModes::System);}
    }
    void finish(bool cancel){if(!pointer)return;pointer.reset();contact(cancel?L"cancel":L"up");root.ReleasePointerCaptures();releaseScroll();AutomationProperties::SetItemStatus(root,L"Ready");}
    static void circle(Shapes::Ellipse const& e,A const& point,double radius){e.Width(radius*2);e.Height(radius*2);Canvas::SetLeft(e,point.GetNumberAt(0)-radius);Canvas::SetTop(e,point.GetNumberAt(1)-radius);}
    void refresh(){
        auto current=array(form(),L"identity").Stringify();if(current!=identity){finish(true);identity=current;}
        auto next=object(form(),L"rendition").Stringify()+data->theme();if(next==key)return;key=next;
        auto g=geometry();if(!g.Size()||g.HasKey(L"error"))return;
        auto ink=data->brush(L"text");circle(field,array(g,L"center"),num(g,L"radius"));circle(marker,array(g,L"marker"),num(g,L"marker_radius"));
        marker.Stroke(ink);marker.Fill(clear());marker.StrokeThickness(2);
        auto tracks=array(g,L"arcs");for(uint32_t i=0;i<2;++i){auto track=tracks.GetObjectAt(i);auto spec=object(track,L"geometry");PointCollection points;
            for(auto item:array(track,L"path")){auto p=item.GetArray();points.Append({float(p.GetNumberAt(0)),float(p.GetNumberAt(1))});}
            arcs[i].Points(points);arcs[i].Stroke(data->brush(L"border"));arcs[i].StrokeThickness(num(spec,L"width"));
            circle(handles[i],array(track,L"point"),num(spec,L"marker_radius"));handles[i].Fill(ink);
        }
        auto readouts=array(g,L"readouts"),values=array(g,L"percentages"),names=array(g,L"icons");
        for(uint32_t i=0;i<4;++i){auto r=readouts.GetObjectAt(i);auto at=array(r,L"text"),box=array(r,L"icon");
            texts[i].Text(to_hstring(int(std::lround(values.GetNumberAt(i))))+L"%");texts[i].Foreground(ink);texts[i].FontSize(num(g,L"text_size"));texts[i].Width(52);texts[i].TextAlignment(TextAlignment::Center);
            Canvas::SetLeft(texts[i],at.GetNumberAt(0)-26);Canvas::SetTop(texts[i],at.GetNumberAt(1)-num(g,L"text_size"));
            icons[i].Source(icon(names.GetStringAt(i),data->theme()).Source());icons[i].Width(box.GetNumberAt(2));icons[i].Height(box.GetNumberAt(3));Canvas::SetLeft(icons[i],box.GetNumberAt(0));Canvas::SetTop(icons[i],box.GetNumberAt(1));
        }
        auto box=array(g,L"reset");reset.Width(box.GetNumberAt(2));reset.Height(box.GetNumberAt(3));Canvas::SetLeft(reset,box.GetNumberAt(0));Canvas::SetTop(reset,box.GetNumberAt(1));reset.Content(icon(L"reset",data->theme()));
    }
    void init(){
        auto weak=weak_from_this();root.IsTabStop(true);root.MinWidth(128);root.MaxWidth(side);root.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetAutomationId(root,L"proof-dial");AutomationProperties::SetName(root,L"SDR appearance dial");AutomationProperties::SetHelpText(root,L"Drag balance and contrast; arcs adjust brightness and color intensity. Arrow keys adjust the selected control. Home resets it; Escape cancels a drag.");
        canvas.Width(side);canvas.Height(side);canvas.Background(clear());view.Child(canvas);view.Stretch(Stretch::Uniform);root.Content(view);
        // One shared immutable illustration; native bitmap storage is BGRA.
        Imaging::WriteableBitmap bitmap(256,256);uint8_t* bytes=nullptr;check_hresult(bitmap.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&bytes));
        if(!capy_proof_texture(256,bytes,256*256*4))throw hresult_invalid_argument(L"Proof texture failed");for(size_t i=0;i<256*256*4;i+=4)std::swap(bytes[i],bytes[i+2]);bitmap.Invalidate();
        ImageBrush image;image.ImageSource(bitmap);image.Stretch(Stretch::Fill);field.Fill(image);field.IsHitTestVisible(false);canvas.Children().Append(field);
        for(uint32_t i=0;i<2;++i){arcs[i].IsHitTestVisible(false);handles[i].IsHitTestVisible(false);canvas.Children().Append(arcs[i]);canvas.Children().Append(handles[i]);}
        marker.IsHitTestVisible(false);canvas.Children().Append(marker);
        for(uint32_t i=0;i<4;++i){texts[i].IsHitTestVisible(false);icons[i].IsHitTestVisible(false);canvas.Children().Append(texts[i]);canvas.Children().Append(icons[i]);}
        reset.Padding({0});reset.MinWidth(0);reset.MinHeight(0);reset.Background(clear());reset.BorderThickness({0});AutomationProperties::SetAutomationId(reset,L"proof-dial-reset");AutomationProperties::SetName(reset,L"Reset SDR appearance");
        reset.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->control(3,O({{L"type",S(L"reset")}}));});canvas.Children().Append(reset);
        root.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->canvas);if(self->pointer||!p.IsInContact()||(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed()))return;
            auto g=self->geometry(p.Position());auto hit=g.GetNamedValue(L"hit",JsonValue::CreateNullValue());if(hit.ValueType()!=JsonValueType::Number||hit.GetNumber()==3)return;
            self->root.Focus(FocusState::Pointer);self->claimScroll();if(!self->root.CapturePointer(e.Pointer())){self->releaseScroll();return;}
            self->pointer=p.PointerId();self->part=uint8_t(hit.GetNumber());self->origin=self->last=p.Position();self->epoch=to_hstring(uint64_t(num(object(self->data->state,L"document_file"),L"epoch")));
            self->contact(L"down");AutomationProperties::SetItemStatus(self->root,L"Adjusting");e.Handled(true);
        }});
        root.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())if(self->pointer==e.Pointer().PointerId()){self->last=e.GetCurrentPoint(self->canvas).Position();self->contact(L"move");e.Handled(true);}});
        root.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())if(self->pointer==e.Pointer().PointerId()){self->last=e.GetCurrentPoint(self->canvas).Position();self->finish(e.GetCurrentPoint(self->canvas).Properties().IsCanceled());e.Handled(true);}});
        auto cancel=[weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())if(self->pointer==e.Pointer().PointerId())self->finish(true);};
        root.PointerCanceled(cancel);root.PointerCaptureLost(cancel);
        root.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->finish(true);});root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->finish(true);});
        root.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock()){
            using winrt::Windows::System::VirtualKey;auto key=e.Key();if(key==VirtualKey::Escape){self->finish(true);e.Handled(true);return;}
            if(self->pointer)return;
            if(key==VirtualKey::Home){self->control(self->part,O({{L"type",S(L"reset")}}));e.Handled(true);}
            else if(key==VirtualKey::Left||key==VirtualKey::Right||key==VirtualKey::Up||key==VirtualKey::Down){self->control(self->part,O({{L"type",S(L"step")},{L"axis",N(key==VirtualKey::Up||key==VirtualKey::Down?1:0)},{L"steps",N(key==VirtualKey::Right||key==VirtualKey::Up?1:-1)}}));e.Handled(true);}
        }});
        MenuFlyout menu;TrackPopup(menu,data);std::array<hstring,4> names{L"Reset balance and contrast",L"Reset brightness",L"Reset color intensity",L"Reset all"};
        for(uint8_t i=0;i<4;++i){MenuFlyoutItem item;item.Text(names[i]);item.Click([weak,i](auto&&,auto&&){if(auto self=weak.lock())self->control(i,O({{L"type",S(L"reset")}}));});menu.Items().Append(item);}root.ContextFlyout(menu);
        refresh();
    }
};
inline ContentControl ProofDialControl(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){auto view=std::make_shared<ProofDial>();view->data=data;view->init();bindings.emplace_back([view]{view->refresh();});return view->root;}
}
