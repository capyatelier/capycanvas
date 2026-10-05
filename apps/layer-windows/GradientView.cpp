#include "pch.h"
#include "EffectControls.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Xaml.Media.Imaging.h>
#include <winrt/Windows.Storage.Streams.h>
#include <robuffer.h>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <optional>
using namespace CapyEffects;
namespace {
struct RampRequest {std::shared_ptr<CapyLocalization> localization;std::string request;hstring key;int width=0,height=0;winrt::Windows::UI::Color light{},dark{};double cell=1;};
struct RampResult {hstring key;int width=0,height=0;std::vector<uint8_t> bytes;};
bool rasterRamp(RampRequest const& request,std::vector<uint8_t>& bytes){
    std::unique_ptr<char,decltype(&capy_string_free)> raw(capy_color_ui(request.localization.get(),request.request.c_str()),capy_string_free);
    if(!raw)return false;
    char const* cursor=std::strstr(raw.get(),"\"argb\":[");if(!cursor)return false;cursor+=8;
    size_t count=size_t(request.width)*request.height;bytes.resize(count*4);
    for(size_t i=0;i<count;++i){
        char* end=nullptr;auto pixel=uint32_t(std::strtoul(cursor,&end,10));if(end==cursor)return false;cursor=end+(*end==','?1:0);
        double alpha=(pixel>>24)/255.;
        size_t x=i%size_t(request.width),y=i/size_t(request.width);
        auto under=(size_t(x/request.cell)+size_t(y/request.cell))%2?request.dark:request.light;
        auto mix=[alpha](uint32_t ink,uint8_t base){return uint8_t(std::lround(ink*alpha+base*(1-alpha)));};
        auto out=bytes.data()+i*4;
        out[0]=mix(pixel&255,under.B);out[1]=mix(pixel>>8&255,under.G);out[2]=mix(pixel>>16&255,under.R);out[3]=255;
    }
    return true;
}
struct RampWorker:std::enable_shared_from_this<RampWorker> {
    Microsoft::UI::Dispatching::DispatcherQueue queue{Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread()};
    std::function<void(RampResult)> deliver;
    std::mutex mutex;std::optional<RampRequest> pending;bool running=false;
    void submit(RampRequest request){
        std::lock_guard lock(mutex);pending=std::move(request);
        if(!running){running=true;std::thread([self=shared_from_this()]{self->run();}).detach();}
    }
    void run(){
        for(;;){
            RampRequest request;
            {std::lock_guard lock(mutex);if(!pending){running=false;return;}request=std::move(*pending);pending.reset();}
            auto result=std::make_shared<RampResult>(RampResult{request.key,request.width,request.height,{}});
            if(!rasterRamp(request,result->bytes))continue;
            queue.TryEnqueue([weak=weak_from_this(),result]{if(auto self=weak.lock();self&&self->deliver)self->deliver(std::move(*result));});
        }
    }
};
struct RampView:std::enable_shared_from_this<RampView> {
    std::shared_ptr<WorkspaceData> data;std::function<J()> gradient;double height=32;
    Border root;Image image;Imaging::WriteableBitmap bitmap{nullptr};
    std::shared_ptr<RampWorker> worker=std::make_shared<RampWorker>();
    hstring requested,shown;
    void init(){
        root.Height(height);root.CornerRadius({4,4,4,4});root.Background(data->brush(L"input"));
        image.Stretch(Stretch::Fill);image.IsHitTestVisible(false);root.Child(image);
        auto weak=weak_from_this();
        worker->deliver=[weak](RampResult result){if(auto self=weak.lock())self->install(std::move(result));};
        root.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
    }
    void refresh(){
        auto value=gradient();auto xaml=root.XamlRoot();if(!value.Size()||!xaml)return;
        double scale=xaml.RasterizationScale();
        int width=std::min(2048,int(std::lround(root.ActualWidth()*scale))),pixels=std::min(64,int(std::lround(height*scale)));
        if(width<1||pixels<1)return;
        auto panel=object(data->model,L"color_panel");auto palette=object(data->state,L"palette");
        auto request=O({{L"type",S(L"gradient")},{L"gradient",value},{L"document_space",S(str(panel,L"rgb_space",L"Srgb"))},
            {L"rendition",panel.GetNamedValue(L"rendition",JsonValue::CreateNullValue())},
            {L"image",O({{L"size",values({double(width),double(pixels)})},{L"depth",S(str(object(data->model,L"proof_panel"),L"depth",L"U8"))}})}});
        auto light=str(palette,L"checker_light"),dark=str(palette,L"checker_dark");
        auto key=request.Stringify()+light+dark+to_hstring(scale);
        if(key==requested)return;requested=key;
        worker->submit({data->localization,to_string(request.Stringify()),key,width,pixels,color(light),color(dark),5*scale});
    }
    void install(RampResult result){
        if(result.key==shown)return;
        if(!bitmap||bitmap.PixelWidth()!=result.width||bitmap.PixelHeight()!=result.height){bitmap=Imaging::WriteableBitmap(result.width,result.height);image.Source(bitmap);}
        uint8_t* destination=nullptr;
        check_hresult(bitmap.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&destination));
        std::memcpy(destination,result.bytes.data(),result.bytes.size());bitmap.Invalidate();shown=result.key;
    }
};
struct EditorView:std::enable_shared_from_this<EditorView> {
    std::shared_ptr<WorkspaceData> data;GradientSource source;
    StackPanel root;Grid top,strip,bottom;ContentControl focus;Canvas dots;ComboBox interpolation;ContentControl positionGate;
    Button reverse{nullptr},reset{nullptr},remove{nullptr},bucket{nullptr},colorPick{nullptr};
    Bindings fields;
    struct Contact{uint32_t id;J owner;int index;double x,width,position;};
    std::optional<Contact> contact;std::optional<std::pair<J,int>> held;
    int selected=0;bool syncing=false;hstring owner,drawn,choices;
    J control()const{return source.control();}
    J value()const{return object(object(control(),L"value"),L"value");}
    A stops()const{return array(value(),L"stops");}
    J stop()const{auto all=stops();return all.Size()?all.GetObjectAt(uint32_t(std::clamp(selected,0,int(all.Size())-1))):J{};}
    J gradient()const{return object(control(),L"gradient");}
    J target()const{return object(gradient(),L"destination");}
    hstring context()const{return owner+L"/"+to_hstring(selected)+L"/"+to_hstring(stops().Size());}
    void send(J const& edit,hstring const& phase={},J const& destination={})const{
        source.send(O({{L"op",S(L"gradient")},{L"target",destination.Size()?destination:target()},{L"edit",edit}}),phase);
    }
    static J stopEdit(V const& index,double position,V const& color=JsonValue::CreateNullValue(),bool erase=false){
        return O({{L"kind",S(L"stop")},{L"index",index},{L"position",N(position)},{L"color",color},{L"remove",B(erase)}});
    }
    static J positionEdit(int index,J const& operation){return O({{L"kind",S(L"position")},{L"index",N(index)},{L"operation",operation}});}
    static J settle(){return O({{L"type",S(L"step")},{L"steps",N(0)}});}
    void cancel(){
        J destination;
        if(contact){destination=contact->owner;contact.reset();focus.ReleasePointerCaptures();}
        else if(held){destination=held->first;held.reset();}
        if(destination.Size())send(O({{L"kind",S(L"reset")}}),L"cancel",destination);
    }
    void removeSelected(){
        int index=selected,count=int(stops().Size());if(index<=0||index+1>=count)return;
        selected=std::max(0,index-1);send(stopEdit(N(index),0,JsonValue::CreateNullValue(),true));
    }
    Button iconButton(wchar_t const* glyph,hstring const& suffix,std::function<void()> action){
        auto pick=button(data,L"",std::move(action));pick.Width(32);pick.Height(32);pick.Content(icon(glyph,data->theme(),20));
        AutomationProperties::SetAutomationId(pick,source.id+L"-"+suffix);return pick;
    }
    void name(Button const& pick,hstring const& text){if(AutomationProperties::GetName(pick)!=text){AutomationProperties::SetName(pick,text);tooltip(pick,text);}}
    void column(Grid const& row,GridLength width){ColumnDefinition definition;definition.Width(width);row.ColumnDefinitions().Append(definition);}
    void init(){
        auto weak=weak_from_this();root.Spacing(6);
        top.ColumnSpacing(6);column(top,{1,GridUnitType::Star});column(top,{1,GridUnitType::Auto});column(top,{1,GridUnitType::Auto});
        interpolation.HorizontalAlignment(HorizontalAlignment::Stretch);interpolation.MinWidth(0);interpolation.MinHeight(32);
        interpolation.FontSize(data->textSize());AutomationProperties::SetAutomationId(interpolation,source.id+L"-interpolation");
        interpolation.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->syncing&&!self->data->updating){
            auto modes=array(self->gradient(),L"interpolations");auto index=self->interpolation.SelectedIndex();
            if(index>=0&&uint32_t(index)<modes.Size())self->send(O({{L"kind",S(L"interpolation")},{L"value",S(modes.GetArrayAt(uint32_t(index)).GetStringAt(0))}}));
        }});
        auto open=std::make_shared<bool>(false);
        interpolation.DropDownOpened([data=data,open](auto&&,auto&&){if(!std::exchange(*open,true))data->popup(true);});
        interpolation.DropDownClosed([data=data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
        interpolation.Unloaded([data=data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
        reverse=iconButton(L"flip-horizontal",L"reverse",[weak]{if(auto self=weak.lock()){self->selected=int(self->stops().Size())-1-self->selected;self->send(O({{L"kind",S(L"reverse")}}));}});
        reset=iconButton(L"reset",L"reset",[weak]{if(auto self=weak.lock())self->send(O({{L"kind",S(L"reset")}}));});
        Grid::SetColumn(reverse,1);Grid::SetColumn(reset,2);
        for(FrameworkElement part:{FrameworkElement(interpolation),FrameworkElement(reverse),FrameworkElement(reset)})top.Children().Append(part);
        strip.Height(44);
        auto ramp=GradientRamp(data,[weak]{if(auto self=weak.lock())return self->value();return J{};},32,fields);
        ramp.Margin({6,0,6,0});ramp.VerticalAlignment(VerticalAlignment::Top);strip.Children().Append(ramp);
        dots.IsHitTestVisible(false);strip.Children().Append(dots);
        focus.Content(strip);focus.IsTabStop(true);focus.UseSystemFocusVisuals(true);focus.Background(clear());focus.ManipulationMode(ManipulationModes::None);
        focus.HorizontalContentAlignment(HorizontalAlignment::Stretch);focus.VerticalContentAlignment(VerticalAlignment::Stretch);
        AutomationProperties::SetAutomationId(focus,source.id+L"-gradient");
        strip.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->drawDots();});
        focus.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->down(e);});
        focus.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->move(e);});
        focus.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->up(e);});
        auto lost=[weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->contact&&self->contact->id==e.Pointer().PointerId())self->cancel();};
        focus.PointerCanceled(lost);focus.PointerCaptureLost(lost);
        focus.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock())self->key(e,true);});
        focus.KeyUp([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock())self->key(e,false);});
        focus.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        focus.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        bottom.ColumnSpacing(6);column(bottom,{1,GridUnitType::Star});for(int i=0;i<3;++i)column(bottom,{1,GridUnitType::Auto});
        NumberPresentation presentation;presentation.title=data->copyCaption(L"color",L"position").current;
        presentation.identity=[weak]{if(auto self=weak.lock())return self->context();return hstring();};
        positionGate.Content(number(data,presentation.title(),object(data->catalog,L"opacity"),
            [weak]{if(auto self=weak.lock())return num(self->stop(),L"position");return 0.;},
            [weak](double position){if(auto self=weak.lock())self->send(positionEdit(self->selected,O({{L"type",S(L"value")},{L"value",N(position)}})));},
            fields,nullptr,true,source.id+L"-position",false,presentation));
        positionGate.IsTabStop(false);positionGate.HorizontalContentAlignment(HorizontalAlignment::Stretch);positionGate.VerticalAlignment(VerticalAlignment::Center);
        remove=iconButton(L"minus",L"remove",[weak]{if(auto self=weak.lock())self->removeSelected();});
        colorPick=CompactColorField(data,source.id,[weak]{if(auto self=weak.lock())return self->data->caption(L"color",L"color");return hstring();},
            [weak]{if(auto self=weak.lock())return object(self->stop(),L"color");return J{};},
            [weak](J color){if(auto self=weak.lock())self->send(stopEdit(N(self->selected),num(self->stop(),L"position"),color));},
            fields,[weak]{if(auto self=weak.lock())return self->context();return hstring();});
        colorPick.VerticalAlignment(VerticalAlignment::Center);
        bucket=iconButton(L"fill",L"use-color",[weak]{if(auto self=weak.lock())self->send(O({{L"kind",S(L"use_current_color")},{L"index",N(self->selected)}}));});
        Grid::SetColumn(remove,1);Grid::SetColumn(colorPick,2);Grid::SetColumn(bucket,3);
        for(FrameworkElement part:{FrameworkElement(positionGate),FrameworkElement(remove),FrameworkElement(colorPick),FrameworkElement(bucket)})bottom.Children().Append(part);
        for(FrameworkElement part:{FrameworkElement(top),FrameworkElement(focus),FrameworkElement(bottom)})root.Children().Append(part);
    }
    void down(PointerRoutedEventArgs const& e){
        auto p=e.GetCurrentPoint(focus);
        if(contact||!p.IsInContact()||!source.enabled())return;
        if(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed())return;
        double width=focus.ActualWidth()-12;if(width<=0)return;
        cancel();
        double at=std::clamp((p.Position().X-6)/width,0.,1.);auto all=stops();int existing=-1,below=0;
        for(uint32_t i=0;i<all.Size();++i){
            double position=num(all.GetObjectAt(i),L"position");
            if(existing<0&&std::abs(position-at)*width<8)existing=int(i);
            if(position<at)++below;
        }
        if(existing<0&&!flag(gradient(),L"can_add"))return;
        focus.Focus(FocusState::Pointer);
        if(!focus.CapturePointer(e.Pointer()))return;
        int index=existing>=0?existing:below;selected=index;
        contact=Contact{p.PointerId(),target(),index,p.Position().X,width,existing>=0?num(all.GetObjectAt(uint32_t(existing)),L"position"):at};
        send(stopEdit(existing>=0?N(existing):JsonValue::CreateNullValue(),contact->position),L"down",contact->owner);
        e.Handled(true);drawDots();
    }
    void move(PointerRoutedEventArgs const& e){
        if(!contact||contact->id!=e.Pointer().PointerId())return;
        double x=e.GetCurrentPoint(focus).Position().X;
        send(stopEdit(N(contact->index),std::clamp(contact->position+(x-contact->x)/contact->width,0.,1.)),L"move",contact->owner);e.Handled(true);
    }
    void up(PointerRoutedEventArgs const& e){
        if(!contact||contact->id!=e.Pointer().PointerId())return;
        if(e.GetCurrentPoint(focus).Properties().IsCanceled()){cancel();return;}
        move(e);auto finished=*contact;contact.reset();
        send(positionEdit(finished.index,settle()),L"up",finished.owner);
        focus.ReleasePointerCapture(e.Pointer());e.Handled(true);
    }
    void key(KeyRoutedEventArgs const& e,bool pressed){
        using K=winrt::Windows::System::VirtualKey;auto pressedKey=e.Key();
        if(composingKey(e)||!source.enabled())return;
        bool step=pressedKey==K::Left||pressedKey==K::Right;
        if(!pressed){
            if(step&&held){auto finished=*held;held.reset();send(positionEdit(finished.second,settle()),L"up",finished.first);e.Handled(true);}
            return;
        }
        if(pressedKey==K::Escape&&(contact||held)){cancel();e.Handled(true);}
        else if(pressedKey==K::Delete||pressedKey==K::Back){cancel();removeSelected();e.Handled(true);}
        else if(step){
            auto phase=held?L"move":L"down";if(!held)held=std::pair{target(),selected};
            bool shift=(GetKeyState(VK_SHIFT)&0x8000)!=0;
            send(positionEdit(held->second,O({{L"type",S(L"step")},{L"steps",N((pressedKey==K::Left?-1:1)*(shift?10:1))}})),phase,held->first);
            e.Handled(true);
        }
    }
    void drawDots(){
        double width=std::max(0.,focus.ActualWidth()-12);auto all=stops();
        auto next=all.Stringify()+L"/"+to_hstring(selected)+L"/"+to_hstring(width)+data->theme();
        if(next==drawn)return;drawn=next;dots.Children().Clear();
        for(uint32_t i=0;i<all.Size();++i){
            double radius=int(i)==selected?4:2.5;Shapes::Ellipse dot;dot.Width(radius*2);dot.Height(radius*2);dot.Fill(data->brush(L"text"));
            Canvas::SetLeft(dot,6+num(all.GetObjectAt(i),L"position")*width-radius);Canvas::SetTop(dot,39-radius);dots.Children().Append(dot);
        }
    }
    void refresh(){
        if(!control().Size())return;
        auto destination=target();auto field=[&destination](wchar_t const* name){return destination.GetNamedValue(name,JsonValue::CreateNullValue());};
        auto nextOwner=O({{L"document",object(data->state,L"document_file").GetNamedValue(L"epoch",JsonValue::CreateNullValue())},
            {L"kind",field(L"kind")},{L"layer",field(L"layer")},{L"key",field(L"key")}}).Stringify();
        if(nextOwner!=owner){cancel();selected=0;owner=nextOwner;}
        auto all=stops();if(all.Size()<2)return;
        selected=std::clamp(selected,0,int(all.Size())-1);
        auto modes=array(gradient(),L"interpolations");
        if(auto next=modes.Stringify();next!=choices){
            choices=next;syncing=true;interpolation.Items().Clear();
            for(auto mode:modes)comboOption(interpolation,mode.GetArray().GetStringAt(1));
            syncing=false;
        }
        int chosen=-1;auto mix=str(value(),L"interpolation");
        for(uint32_t i=0;i<modes.Size();++i)if(modes.GetArrayAt(i).GetStringAt(0)==mix)chosen=int(i);
        if(interpolation.SelectedIndex()!=chosen){syncing=true;interpolation.SelectedIndex(chosen);syncing=false;}
        auto caption=str(gradient(),L"interpolation_label");AutomationProperties::SetName(interpolation,caption);tooltip(interpolation,caption);
        name(reverse,str(gradient(),L"reverse_label"));name(reset,data->caption(L"color",L"reset_gradient"));
        name(remove,data->caption(L"color",L"remove_stop"));name(bucket,data->caption(L"color",L"use_selected"));
        AutomationProperties::SetName(focus,data->caption(L"color",L"add_stop"));tooltip(focus,data->caption(L"color",L"add_stop"));
        bool enabled=source.enabled(),interior=selected>0&&selected+1<int(all.Size());
        for(Control control:{Control(interpolation),Control(reverse),Control(reset),Control(focus),Control(bucket),Control(colorPick)})control.IsEnabled(enabled);
        positionGate.IsEnabled(enabled&&interior);remove.IsEnabled(enabled&&interior);
        if(!enabled)cancel();
        for(auto const& bind:fields)bind();
        drawDots();
    }
};
}
FrameworkElement CapyEffects::GradientRamp(std::shared_ptr<WorkspaceData> const& data,std::function<J()> gradient,double height,Bindings& bindings){
    auto view=std::make_shared<RampView>();view->data=data;view->gradient=std::move(gradient);view->height=height;view->init();
    bindings.emplace_back([view]{view->refresh();});return view->root;
}
FrameworkElement CapyEffects::GradientEditor(std::shared_ptr<WorkspaceData> const& data,GradientSource source,Bindings& bindings){
    auto view=std::make_shared<EditorView>();view->data=data;view->source=std::move(source);view->init();
    bindings.emplace_back([view]{view->refresh();});return view->root;
}
FrameworkElement CapyEffects::GradientField(std::shared_ptr<Property> const& property,Bindings& bindings){
    return GradientEditor(property->data,{[property]{return property->model();},
        [property](J action,hstring phase){property->action(std::move(action),phase);},
        [property]{return flag(property->view(),L"enabled");},property->id()},bindings);
}
