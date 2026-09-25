#include "pch.h"
#include "ToolbarComponents.h"
#include "WorkspaceGeometry.h"
#include "NativeMenus.h"
#include "RangeControl.h"
#include <robuffer.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Xaml.Media.Imaging.h>
#include <optional>
#include <limits>

using namespace CapyUi;
namespace Shapes=Microsoft::UI::Xaml::Shapes;
namespace NativeInput=Microsoft::UI::Input;
using winrt::Windows::Foundation::Point;

namespace {
V toolbarUi(J const& request){
    std::unique_ptr<char,decltype(&capy_string_free)> result(capy_toolbar_ui(to_string(request.Stringify()).c_str()),capy_string_free);
    if(!result)throw hresult_invalid_argument(L"Toolbar request failed");
    auto value=JsonValue::Parse(to_hstring(result.get()));
    if(value.ValueType()==JsonValueType::Object&&value.GetObject().HasKey(L"error"))
        throw hresult_invalid_argument(value.GetObject().GetNamedString(L"error"));
    return value;
}
J copy(J const& value){return J::Parse(value.Stringify());}
double textWidth(std::shared_ptr<WorkspaceData> const& data,hstring const& text){
    auto measure=label(data,text);measure.Measure({std::numeric_limits<float>::infinity(),32});
    return measure.DesiredSize().Width;
}
hstring eights(hstring const& text){
    std::wstring value=text.c_str();for(auto& ch:value)if(ch>=L'0'&&ch<=L'9')ch=L'8';return hstring(value);
}
bool inside(winrt::Windows::Foundation::IInspectable const& original,UIElement const& ancestor){
    for(auto node=original.try_as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node))
        if(node==ancestor)return true;
    return false;
}
winrt::Windows::UI::Color alpha(winrt::Windows::UI::Color value,uint8_t a){value.A=a;return value;}
bool insideTrack(double along,double across,double length,double narrow){
    if(along<0){double a=along/2.25,b=across/narrow;return a*a+b*b<=1;}
    if(along>length-3){double a=(along-(length-3))/3,b=across/8;return a*a+b*b<=1;}
    return std::abs(across)<=narrow+(8-narrow)*along/std::max(1.,length-3);
}
Imaging::WriteableBitmap trackRaster(double width,double height,double scale,bool vertical,bool opacity,winrt::Windows::UI::Color ink){
    int pw=std::max(1,int(std::ceil(width*scale))),ph=std::max(1,int(std::ceil(height*scale)));
    Imaging::WriteableBitmap result(pw,ph);uint8_t* bytes=nullptr;
    check_hresult(result.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&bytes));
    double length=(vertical?height:width)-12,narrow=opacity?8:2.5;
    for(int y=0;y<ph;y++)for(int x=0;x<pw;x++){
        double coverage=0;
        for(int sy=0;sy<4;sy++)for(int sx=0;sx<4;sx++){
            double px=(x+(sx+.5)/4)/scale,py=(y+(sy+.5)/4)/scale;
            double along=vertical?length+6-py:px-6,across=vertical?px-14:py-14;
            if(!insideTrack(along,across,length,narrow))continue;
            double a=.22;
            if(opacity){
                bool cell=((int(std::floor(along/4))+int(std::floor(across/4)))%2+2)%2==0;
                double t=std::clamp(along/std::max(1.,length),0.,1.)*.65;
                a=1-(1-.08)*(1-(cell?.2:0))*(1-t);
            }
            coverage+=a;
        }
        double a=coverage/16;auto p=bytes+(size_t(y)*pw+x)*4;
        p[0]=uint8_t(std::lround(ink.B*a));p[1]=uint8_t(std::lround(ink.G*a));p[2]=uint8_t(std::lround(ink.R*a));p[3]=uint8_t(std::lround(255*a));
    }
    result.Invalidate();return result;
}
}

struct ToolbarComponent::Impl:std::enable_shared_from_this<Impl>{
    struct Field {
        FrameworkElement row{nullptr};
        std::function<void(J const&)> update;
        std::function<void()> orient;
        std::function<winrt::Windows::Foundation::Size()> natural;
        std::function<void()> dispose;
        int segmented=0;bool action=false,interval=false;
    };
    struct Contact {uint32_t id;Point start;bool moved=false;};
    std::shared_ptr<WorkspaceData> data;
    std::weak_ptr<WorkspaceGestures> gestures;
    hstring panelId,tileStyle,tileLabel;double tileId=0,iconSize=16;
    J control,style,item,model;
    Canvas root;Border blank;Button more{nullptr};
    std::wstring schema,boundsKey;
    std::vector<Field> fields;
    bool vertical=false,measured=false,standalone=true,text=true,sliders=true;
    double width=0,height=0;
    uint64_t transient=0;

    Canvas sliderRow{nullptr},track{nullptr},marks{nullptr};
    Button cap{nullptr};Border thumb{nullptr};Image trackImage{nullptr};Slider access{nullptr};
    J field,spec;double current=0,level=0;bool enabled=false,opacity=false,settingAccess=false;
    std::optional<Contact> contact;
    std::wstring trackKey;hstring marksKey;

    Primitives::Popup preview{nullptr};
    Canvas previewBody{nullptr},stampLayer{nullptr};Image stampImage{nullptr};Shapes::Rectangle fade{nullptr};
    TextBlock caption{nullptr};Grid captionSlot{nullptr};Button bookmark{nullptr};Border previewFrame{nullptr};double bookmarkIcon=0;
    J stamp;uint64_t stampRequest=0;std::optional<bool> bookmarkSelected;
    UIElement outsideHost{nullptr};winrt::Windows::Foundation::IInspectable outside{nullptr};
    UIElement editHost{nullptr};winrt::Windows::Foundation::IInspectable editOutside{nullptr};

    Flyout editor{nullptr};

    void init(J const& panel,J const& tile){
        panelId=str(panel,L"id");tileId=num(tile,L"id");tileStyle=str(panel,L"tile_style",L"small");
        iconSize=num(panel,L"tile_icon_size",16);tileLabel=str(tile,L"label");
        control=object(tile,L"control");standalone=str(control,L"kind")!=L"tool_options";
        auto preferences=object(control,L"style");text=flag(preferences,L"text",true);sliders=flag(preferences,L"sliders",true);
        style=toolbarUi(O({{L"type",S(L"style")},{L"style",S(tileStyle)}})).GetObject();
        item=O({{L"kind",S(L"tile")},{L"panel",S(panelId)},{L"tile",N(tileId)}});
        root.Background(clear());
        AutomationProperties::SetAutomationId(root,L"toolbar-component-"+to_hstring(uint32_t(tileId)));
        AutomationProperties::SetName(root,tileLabel);
        auto weak=weak_from_this();
        if(!standalone){
            blank.Background(clear());root.Children().Append(blank);
            if(auto source=gestures.lock())source->Source(blank,J{},item);
            more=button(data,L"More tool options",[weak]{if(auto self=weak.lock())self->activate();});
            more.Content(icon(L"more",data->theme(),iconSize));
            ToolTipService::SetToolTip(more,box_value(L"More tool options"));
            AutomationProperties::SetAutomationId(more,L"toolbar-more-"+to_hstring(uint32_t(tileId)));
            root.Children().Append(more);
            if(auto source=gestures.lock())source->Source(more,drag(),item,false,{},WorkspaceGestures::Pickup::Hold);
        }
        transient=data->transient([weak]{if(auto self=weak.lock())return self->closePopup();return false;});
        Update(tile);
    }
    ~Impl(){
        if(data)data->transients.erase(transient);
        for(auto& f:fields)if(f.dispose)f.dispose();
        closePopup();unwatchEdit();
    }
    void watchEdit(FrameworkElement const& owner){
        unwatchEdit();
        if(!root.XamlRoot())return;
        editHost=root.XamlRoot().Content();
        editOutside=box_value(PointerEventHandler([weak=weak_from_this(),view=make_weak(owner)](auto&&,PointerRoutedEventArgs const& e){
            auto self=weak.lock();auto field=view.get();
            if(self&&field&&!inside(e.OriginalSource(),field)&&self->more)self->more.Focus(FocusState::Programmatic);
        }));
        if(editHost)editHost.AddHandler(UIElement::PointerPressedEvent(),editOutside,true);
    }
    void unwatchEdit(){
        if(editHost&&editOutside)editHost.RemoveHandler(UIElement::PointerPressedEvent(),editOutside);
        editHost=nullptr;editOutside=nullptr;
    }
    J drag()const{return O({{L"type",S(L"tile_drag")},{L"item",item}});}
    void activate()const{
        if(auto source=gestures.lock();source&&source->SuppressClick())return;
        data->dispatch(O({{L"type",S(L"activate_tile")},{L"panel",S(panelId)},{L"tile",N(tileId)}}));
    }
    void send(J const& action)const{
        data->dispatch(O({{L"type",S(L"toolbar_edit")},{L"context",object(model,L"context")},{L"action",action}}));
    }
    bool closePopup(){
        bool closed=false;
        if(preview){preview.IsOpen(false);preview=nullptr;closed=true;}
        if(outsideHost&&outside)outsideHost.RemoveHandler(UIElement::PointerPressedEvent(),outside);
        outsideHost=nullptr;outside=nullptr;++stampRequest;
        if(editor){editor.Hide();editor=nullptr;closed=true;}
        return closed;
    }
    std::wstring schemaKey(J const& component)const{
        auto key=copy(component);
        if(auto value=object(key,L"numeric");value.Size())value.SetNamedValue(L"value",N(0));
        if(key.HasKey(L"bookmarks"))key.Remove(L"bookmarks");
        for(auto option:array(key,L"options")){
            auto entry=option.GetObject();
            if(auto value=object(entry,L"Numeric");value.Size())value.SetNamedValue(L"value",N(0));
            if(auto range=object(entry,L"Range");range.Size())
                for(auto bound:array(range,L"bounds"))bound.GetObject().SetNamedValue(L"value",N(0));
            if(auto choice=object(entry,L"Choice");choice.Size())
                for(auto choiceItem:array(choice,L"items"))choiceItem.GetObject().SetNamedValue(L"selected",B(false));
            if(auto action=object(entry,L"Action");action.Size()){
                auto state=object(action,L"state");state.SetNamedValue(L"selected",B(false));state.SetNamedValue(L"enabled",B(true));
            }
        }
        return key.Stringify().c_str();
    }
    void Update(J const& tile){
        auto value=object(tile,L"component");if(!value.Size())return;
        auto nextSchema=schemaKey(value);model=value;
        if(nextSchema!=schema){
            closePopup();
            for(auto& f:fields){if(f.dispose)f.dispose();uint32_t index;if(root.Children().IndexOf(f.row,index))root.Children().RemoveAt(index);}
            fields.clear();schema=nextSchema;
            if(standalone)buildSlider(tile);
            else for(auto option:array(model,L"options")){
                auto entry=option.GetObject();
                if(entry.HasKey(L"Range"))fields.push_back(rangeField(object(entry,L"Range")));
                else if(entry.HasKey(L"Numeric"))fields.push_back(numericField(object(entry,L"Numeric")));
                else if(entry.HasKey(L"Choice"))fields.push_back(choiceField(object(entry,L"Choice")));
                else fields.push_back(actionField(object(entry,L"Action")));
                root.Children().Append(fields.back().row);
            }
            measured=false;
        }
        if(standalone)updateSlider();
        else{
            auto options=array(model,L"options");
            for(uint32_t i=0;i<std::min<uint32_t>(options.Size(),uint32_t(fields.size()));++i)fields[i].update(options.GetObjectAt(i));
        }
        if(!measured){layout();measured=true;}
    }
    void Layout(J const& bounds,bool axisVertical){
        auto key=std::to_wstring(num(bounds,L"width"))+L"x"+std::to_wstring(num(bounds,L"height"))+(axisVertical?L"v":L"h");
        if(key==boundsKey)return;
        boundsKey=key;width=num(bounds,L"width");height=num(bounds,L"height");vertical=axisVertical;
        RectangleGeometry clip;clip.Rect({0,0,float(width),float(height)});root.Clip(clip);
        closePopup();layout();
    }
    void layout(){
        if(!model.Size()||width<=0||height<=0)return;
        for(auto& f:fields)if(f.orient)f.orient();
        if(standalone){layoutSlider();return;}
        blank.Width(width);blank.Height(height);
        auto size=array(style,L"size");double tileW=size.GetNumberAt(0),tileH=size.GetNumberAt(1);
        A sizes;
        for(auto& f:fields){
            A pair;
            if(f.segmented){
                bool stacked=vertical&&width<tileW*f.segmented;
                pair.Append(N(vertical?width:tileW*f.segmented));pair.Append(N(vertical?tileH*(stacked?f.segmented:1):24));
            }else if(!f.interval&&(vertical||f.action)){pair.Append(N(tileW));pair.Append(N(tileH));}
            else{auto extent=f.natural();pair.Append(N(extent.Width));pair.Append(N(extent.Height));}
            sizes.Append(pair);
        }
        auto geometry=toolbarUi(O({{L"type",S(L"options_layout")},{L"width",N(width)},{L"height",N(height)},
            {L"axis",S(vertical?L"vertical":L"horizontal")},{L"sizes",sizes},{L"button",size},{L"gap",N(vertical?num(style,L"gap",2):10)}})).GetObject();
        place(more,object(geometry,L"more"));
        auto placed=array(geometry,L"fields");
        for(size_t i=0;i<fields.size();++i){
            auto value=i<placed.Size()?placed.GetAt(uint32_t(i)):JsonValue::CreateNullValue();
            bool shown=value.ValueType()==JsonValueType::Object;
            if(!shown&&fields[i].row.FocusState()!=FocusState::Unfocused)more.Focus(FocusState::Programmatic);
            fields[i].row.Visibility(shown?Visibility::Visible:Visibility::Collapsed);
            if(!shown)continue;
            auto bounds=value.GetObject();
            if(fields[i].segmented&&!vertical)bounds=O({{L"x",N(num(bounds,L"x"))},{L"y",N(num(bounds,L"y")+(num(bounds,L"height")-24)/2)},{L"width",N(num(bounds,L"width"))},{L"height",N(24)}});
            place(fields[i].row,bounds);
        }
    }

    void buildSlider(J const& tile){
        auto weak=weak_from_this();
        field=object(model,L"numeric");
        opacity=str(control,L"kind")==L"brush_opacity_slider";
        if(!field.Size())field=O({{L"id",S(opacity?L"opacity":L"size")},{L"label",S(str(tile,L"label"))},
            {L"numeric",toolbarUi(O({{L"type",S(L"slider_spec")},{L"control",control}})).GetObject()},{L"value",N(.5)}});
        spec=object(field,L"numeric");current=num(field,L"value");
        Field result;
        sliderRow=Canvas();sliderRow.Background(clear());result.row=sliderRow;
        cap=button(data,str(field,L"label"),[weak]{
            auto self=weak.lock();if(!self)return;
            if(auto source=self->gestures.lock();source&&source->SuppressClick())return;
            self->show();
        });
        cap.Background(clear());cap.Content(nullptr);ToolTipService::SetToolTip(cap,box_value(str(field,L"label")));
        AutomationProperties::SetAutomationId(cap,L"slider-cap-"+to_hstring(uint32_t(tileId)));
        if(auto source=gestures.lock())source->Source(cap,drag(),item,false,{},WorkspaceGestures::Pickup::Hold);
        track=Canvas();track.Background(clear());track.ManipulationMode(ManipulationModes::None);
        trackImage=Image();trackImage.IsHitTestVisible(false);trackImage.Stretch(Stretch::Fill);track.Children().Append(trackImage);
        marks=Canvas();marks.IsHitTestVisible(false);track.Children().Append(marks);
        thumb=Border();thumb.CornerRadius({3.24,3.24,3.24,3.24});thumb.BorderThickness({1,1,1,1});
        thumb.BorderBrush(fill(alpha(color(str(object(data->state,L"palette"),L"text")),153)));
        thumb.Background(data->brush(L"thumb"));thumb.IsHitTestVisible(false);track.Children().Append(thumb);
        access=Slider();access.Minimum(0);access.Maximum(1);access.StepFrequency(.001);access.SmallChange(.01);access.LargeChange(.1);
        access.Opacity(0);access.IsHitTestVisible(false);access.IsThumbToolTipEnabled(false);
        AutomationProperties::SetName(access,str(field,L"label"));
        AutomationProperties::SetAutomationId(access,L"component-slider-"+to_hstring(uint32_t(tileId)));
        access.ValueChanged([weak](auto&&,Primitives::RangeBaseValueChangedEventArgs const& e){
            if(auto self=weak.lock();self&&!self->settingAccess&&!self->data->updating&&self->enabled)
                self->change(num(numeric(self->spec,self->current,O({{L"type",S(L"position")},{L"position",N(e.NewValue())}})),L"value"));
        });
        track.Children().Append(access);
        track.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->pressed(e);});
        track.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->moved(e);});
        track.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->released(e);});
        track.PointerCanceled([weak](auto&&,auto&&){if(auto self=weak.lock())self->lost();});
        track.PointerCaptureLost([weak](auto&&,auto&&){if(auto self=weak.lock())self->lost();});
        sliderRow.Children().Append(cap);sliderRow.Children().Append(track);
        fields.push_back(std::move(result));root.Children().Append(sliderRow);
    }
    double travel()const{return std::max(1.,(vertical?track.Height():track.Width())-12);}
    double position(PointerRoutedEventArgs const& e)const{
        auto p=e.GetCurrentPoint(track).Position();
        double at=vertical?1-(p.Y-6)/travel():(p.X-6)/travel();
        return std::clamp(at,0.,1.);
    }
    void change(double value){
        if(value==current)return;
        send(O({{L"type",S(L"set_tool_setting")},{L"id",S(str(field,L"id"))},{L"value",N(value)}}));
    }
    void pick(PointerRoutedEventArgs const& e,bool snap){
        double at=position(e);
        if(snap){
            A values;for(auto mark:array(model,L"bookmarks"))values.Append(N(num(mark.GetObject(),L"value")));
            change(toolbarUi(O({{L"type",S(L"slider_bookmark_value")},{L"control",control},{L"values",values},
                {L"position",N(at)},{L"travel",N(travel())}})).GetNumber());
        }else change(num(numeric(spec,current,O({{L"type",S(L"position")},{L"position",N(at)}})),L"value"));
    }
    void pressed(PointerRoutedEventArgs const& e){
        auto p=e.GetCurrentPoint(track);
        if(!enabled||contact)return;
        if(p.PointerDeviceType()==NativeInput::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed())return;
        if(!track.CapturePointer(e.Pointer()))return;
        contact=Contact{p.PointerId(),p.Position()};e.Handled(true);
        pick(e,true);show();
    }
    void moved(PointerRoutedEventArgs const& e){
        if(!contact||contact->id!=e.Pointer().PointerId())return;
        auto p=e.GetCurrentPoint(track).Position();e.Handled(true);
        if(!contact->moved&&std::hypot(p.X-contact->start.X,p.Y-contact->start.Y)<3)return;
        contact->moved=true;pick(e,false);
    }
    void released(PointerRoutedEventArgs const& e){
        if(!contact||contact->id!=e.Pointer().PointerId())return;
        bool dragged=contact->moved;contact.reset();e.Handled(true);
        track.ReleasePointerCapture(e.Pointer());
        if(dragged)closePopup();
    }
    void lost(){if(contact){contact.reset();closePopup();}}
    void updateSlider(){
        auto next=object(model,L"numeric");enabled=next.Size()!=0;
        if(enabled)current=num(next,L"value");
        auto shown=numeric(spec,current,O({{L"type",S(L"format")}}));level=num(shown,L"fill");
        settingAccess=true;access.Value(level);settingAccess=false;
        access.IsEnabled(enabled);AutomationProperties::SetItemStatus(access,str(shown,L"text"));
        sliderRow.Opacity(enabled?1:.4);
        paintSlider();paintPreview();
    }
    void layoutSlider(){
        auto geometry=toolbarUi(O({{L"type",S(L"slider_layout")},{L"width",N(width)},{L"height",N(height)},
            {L"axis",S(vertical?L"vertical":L"horizontal")}})).GetArray();
        sliderRow.Width(width);sliderRow.Height(height);
        place(cap,geometry.GetObjectAt(0));auto bounds=geometry.GetObjectAt(1);
        double x=num(bounds,L"x"),y=num(bounds,L"y"),w=num(bounds,L"width"),h=num(bounds,L"height");
        if(vertical){Canvas::SetLeft(track,(width-28)/2);Canvas::SetTop(track,y+2);track.Width(28);track.Height(std::max(0.,h-4));}
        else{Canvas::SetLeft(track,x+2);Canvas::SetTop(track,(height-28)/2);track.Width(std::max(0.,w-4));track.Height(28);}
        access.Width(track.Width());access.Height(track.Height());
        access.Orientation(vertical?Orientation::Vertical:Orientation::Horizontal);
        paintTrack();paintSlider();
    }
    void paintTrack(){
        double w=track.Width(),h=track.Height();
        if(!(w>0)||!(h>0))return;
        double scale=root.XamlRoot()?root.XamlRoot().RasterizationScale():1.;
        auto ink=color(str(object(data->state,L"palette"),L"text"));
        auto key=std::to_wstring(w)+L"x"+std::to_wstring(h)+L"@"+std::to_wstring(scale)+(vertical?L"v":L"h")+std::to_wstring(ink.R)+L"."+std::to_wstring(ink.G)+L"."+std::to_wstring(ink.B);
        if(key!=trackKey){trackKey=key;trackImage.Source(trackRaster(w,h,scale,vertical,opacity,ink));}
        trackImage.Width(w);trackImage.Height(h);
    }
    void paintSlider(){
        double span=travel();
        if(vertical){thumb.Width(28);thumb.Height(12);Canvas::SetLeft(thumb,0);Canvas::SetTop(thumb,span*(1-level));}
        else{thumb.Width(12);thumb.Height(28);Canvas::SetLeft(thumb,span*level);Canvas::SetTop(thumb,0);}
        bool follows=false;for(auto value:array(model,L"bookmarks"))follows=follows||flag(value.GetObject(),L"selected");
        auto key=array(model,L"bookmarks").Stringify()+to_hstring(span)+(vertical?L"v":L"h")+(follows?to_hstring(level):L"");
        if(key==marksKey)return;
        marksKey=key;marks.Children().Clear();
        for(auto value:array(model,L"bookmarks")){
            auto mark=value.GetObject();bool chosen=flag(mark,L"selected");double at=chosen?level:num(mark,L"fill");
            Border line;line.CornerRadius({1,1,1,1});line.Background(data->brush(chosen?L"panel":L"text"));
            if(vertical){line.Width(14);line.Height(2);Canvas::SetLeft(line,7);Canvas::SetTop(line,6+span*(1-at)-1);}
            else{line.Width(2);line.Height(14);Canvas::SetLeft(line,6+span*at-1);Canvas::SetTop(line,7);}
            marks.Children().Append(line);
        }
    }
    void show(){
        if(!enabled)return;
        if(preview&&preview.IsOpen()){paintPreview();return;}
        closePopup();
        auto request=++stampRequest;auto weak=weak_from_this();
        auto dispatcher=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread();
        if(!data->query||!dispatcher)return;
        auto json=to_string(O({{L"type",S(L"toolbar_stamp")},{L"context",object(model,L"context")}}).Stringify());
        data->query(CanvasQueryKind::Workspace,json,[dispatcher,weak,request](PreviewPacket packet){
            dispatcher.TryEnqueue([weak,request,packet=std::move(packet)]{
                auto self=weak.lock();if(!self||request!=self->stampRequest||!packet)return;
                try{
                    auto reply=J::Parse(to_hstring(capy_preview_metadata(packet.get())));
                    auto result=object(reply,L"result");if(!result.Size())return;
                    size_t count=0;auto bytes=capy_preview_bytes(packet.get(),&count);
                    self->openPreview(result,bytes,count);
                }catch(hresult_error const& error){OutputDebugStringW(error.message().c_str());}
            });
        });
    }
    void openPreview(J const& result,uint8_t const* mask,size_t count){
        auto size=int(num(result,L"size"));
        if(size<=0||count!=size_t(size)*size_t(size)||!root.XamlRoot())return;
        stamp=result;
        Imaging::WriteableBitmap bitmap(size,size);uint8_t* bytes=nullptr;
        check_hresult(bitmap.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&bytes));
        auto ink=color(str(object(data->state,L"palette"),L"text"));
        for(size_t i=0;i<count;++i){
            auto a=mask[i];auto p=bytes+i*4;
            p[0]=uint8_t(ink.B*a/255);p[1]=uint8_t(ink.G*a/255);p[2]=uint8_t(ink.R*a/255);p[3]=a;
        }
        bitmap.Invalidate();
        auto weak=weak_from_this();
        previewFrame=Border();previewFrame.Background(data->brush(L"panel"));previewFrame.CornerRadius({8,8,8,8});
        previewFrame.BorderThickness({0,0,0,0});
        previewFrame.RequestedTheme(data->theme()==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        previewFrame.Shadow(ThemeShadow());previewFrame.Translation({0,0,16});
        previewBody=Canvas();previewFrame.Child(previewBody);
        stampLayer=Canvas();stampImage=Image();stampImage.Source(bitmap);stampImage.Stretch(Stretch::Fill);
        stampLayer.Children().Append(stampImage);previewBody.Children().Append(stampLayer);
        fade=Shapes::Rectangle();fade.IsHitTestVisible(false);previewBody.Children().Append(fade);
        caption=label(data,L"");caption.VerticalAlignment(VerticalAlignment::Center);caption.TextWrapping(TextWrapping::NoWrap);
        captionSlot=Grid();captionSlot.Children().Append(caption);previewBody.Children().Append(captionSlot);
        bookmark=button(data,L"Bookmark this value",[weak]{
            if(auto self=weak.lock())self->send(O({{L"type",S(L"toggle_slider_bookmark")},{L"control",self->control}}));
        });
        bookmarkIcon=0;
        AutomationProperties::SetAutomationId(bookmark,L"slider-bookmark-"+to_hstring(uint32_t(tileId)));
        previewBody.Children().Append(bookmark);bookmarkSelected.reset();
        preview=Primitives::Popup();preview.XamlRoot(root.XamlRoot());preview.Child(previewFrame);
        preview.ShouldConstrainToRootBounds(true);
        outsideHost=root.XamlRoot().Content();
        outside=box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&!inside(e.OriginalSource(),self->sliderRow))self->closePopup();
        }));
        if(outsideHost)outsideHost.AddHandler(UIElement::PointerPressedEvent(),outside,true);
        preview.IsOpen(true);paintPreview();
    }
    void paintPreview(){
        if(!preview||!stamp.Size())return;
        auto geometry=toolbarUi(O({{L"type",S(L"slider_preview")},{L"control",control},{L"style",S(tileStyle)},{L"value",N(current)},
            {L"length",N(std::max(width,height))},{L"extent",N(num(stamp,L"extent"))}})).GetObject();
        double side=num(geometry,L"side");
        previewFrame.Width(side);previewFrame.Height(side);
        auto box=object(geometry,L"stamp"),view=object(geometry,L"viewport");
        RectangleGeometry clip;clip.Rect(rectangle(view));stampLayer.Clip(clip);
        stampImage.Width(num(box,L"width"));stampImage.Height(num(box,L"height"));
        Canvas::SetLeft(stampImage,num(box,L"x"));Canvas::SetTop(stampImage,num(box,L"y"));
        stampImage.Opacity(num(geometry,L"opacity",1));
        double header=num(geometry,L"header_fade");
        fade.Visibility(header>0?Visibility::Visible:Visibility::Collapsed);
        if(header>0){
            auto panel=color(str(object(data->state,L"palette"),L"panel"));
            LinearGradientBrush gradient;gradient.StartPoint({0,0});gradient.EndPoint({0,1});
            GradientStop top;top.Color(alpha(panel,uint8_t(std::lround(255*num(geometry,L"header_fade_opacity",.4)))));top.Offset(0);GradientStop bottom;bottom.Color(alpha(panel,0));bottom.Offset(1);
            gradient.GradientStops().Append(top);gradient.GradientStops().Append(bottom);
            fade.Fill(gradient);fade.Width(side);fade.Height(header);
        }
        double radius=num(geometry,L"radius")*.54;previewFrame.CornerRadius({radius,radius,radius,radius});
        caption.Text(str(geometry,L"text"));place(captionSlot,object(geometry,L"caption"));
        place(bookmark,object(geometry,L"bookmark"));bookmark.CornerRadius({radius,radius,radius,radius});
        double glyph=num(geometry,L"icon",16);
        bool chosen=false;for(auto mark:array(model,L"bookmarks"))chosen=chosen||flag(mark.GetObject(),L"selected");
        if(chosen!=bookmarkSelected||glyph!=bookmarkIcon){
            bookmarkSelected=chosen;bookmarkIcon=glyph;bookmark.Content(icon(chosen?L"minus":L"plus",data->theme(),glyph));
            auto tip=chosen?L"Remove bookmark":L"Bookmark this value";
            ToolTipService::SetToolTip(bookmark,box_value(tip));AutomationProperties::SetName(bookmark,tip);
        }
        auto anchor=root.TransformToVisual(nullptr).TransformBounds({0,0,float(root.ActualWidth()),float(root.ActualHeight())});
        auto window=root.XamlRoot().Size();
        double x=vertical?(anchor.X+anchor.Width+side+8<=window.Width?anchor.X+anchor.Width+8:anchor.X-side-8):anchor.X;
        double y=vertical?anchor.Y+(anchor.Height-side)/2:(anchor.Y+anchor.Height+side+8<=window.Height?anchor.Y+anchor.Height+8:anchor.Y-side-8);
        preview.HorizontalOffset(std::max(6.,std::min(x,window.Width-side-6)));
        preview.VerticalOffset(std::max(6.,std::min(y,window.Height-side-6)));
    }

    Field numericField(J const& option){
        auto weak=weak_from_this();
        auto id=str(option,L"id");auto settingSpec=object(option,L"numeric");
        auto info=toolbarUi(O({{L"type",S(L"numeric_info")},{L"id",S(id)},{L"control",settingSpec},{L"compact",B(true)},{L"units",B(true)}})).GetObject();
        struct State {double value=0;bool units=true,scrubbed=false;std::optional<Contact> contact;double startFill=0;};
        auto state=std::make_shared<State>();state->value=num(option,L"value");
        auto set=[weak,id](double value){if(auto self=weak.lock())self->send(O({{L"type",S(L"set_tool_setting")},{L"id",S(id)},{L"value",N(value)}}));};
        auto reset=[weak,id]{if(auto self=weak.lock())self->send(O({{L"type",S(L"reset_tool_setting")},{L"id",S(id)}}));};
        NumberPresentation presentation;
        for(auto sample:array(info,L"samples"))presentation.widthSamples.push_back(sample.GetString());
        presentation.resolve=[state](J const& control,double value,J const& operation){
            return toolbarUi(O({{L"type",S(L"number")},{L"request",O({{L"control",control},{L"value",N(value)},{L"operation",operation}})},
                {L"compact",B(true)},{L"units",B(state->units)}})).GetObject();
        };
        auto title=str(option,L"label");
        auto bindings=std::make_shared<Bindings>();
        auto numberView=number(data,title,settingSpec,[state]{return state->value;},set,*bindings,nullptr,false,L"toolbar-setting-"+id,true,presentation);
        auto header=numberView.Children().GetAt(0).as<Grid>();auto inlineTrack=header.Children().GetAt(0).as<Slider>();
        Grid row;row.ColumnSpacing(4);
        ColumnDefinition lead;lead.Width({1,GridUnitType::Auto});row.ColumnDefinitions().Append(lead);
        ColumnDefinition rest;rest.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(rest);
        auto name=label(data,title);name.VerticalAlignment(VerticalAlignment::Center);
        Border glyph;glyph.Background(clear());glyph.Child(icon(str(info,L"icon"),data->theme()));glyph.VerticalAlignment(VerticalAlignment::Center);
        ToolTipService::SetToolTip(glyph,box_value(title));
        for(FrameworkElement node:{FrameworkElement(name),FrameworkElement(glyph)})node.DoubleTapped([reset](auto&&,auto&&){reset();});
        row.Children().Append(name);row.Children().Append(glyph);Grid::SetColumn(numberView,1);row.Children().Append(numberView);
        numberView.VerticalAlignment(VerticalAlignment::Center);
        numberView.GotFocus([weak,view=make_weak(numberView)](auto&&,RoutedEventArgs const& e){
            if(auto self=weak.lock();self&&e.OriginalSource().try_as<TextBox>())if(auto owner=view.get())self->watchEdit(owner);
        });
        numberView.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->unwatchEdit();});
        AutomationProperties::SetAutomationId(row,L"toolbar-field-"+id);
        Button face=button(data,title,[]{});face.Background(clear());face.ManipulationMode(ManipulationModes::None);
        face.HorizontalContentAlignment(HorizontalAlignment::Stretch);face.VerticalContentAlignment(VerticalAlignment::Stretch);
        AutomationProperties::SetAutomationId(face,L"toolbar-face-"+id);
        auto faceValue=label(data,L"");faceValue.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());
        auto faceLabel=label(data,title);faceLabel.TextTrimming(TextTrimming::CharacterEllipsis);
        auto faceIcon=icon(str(info,L"icon"),data->theme());
        Grid faceContent;faceContent.RowSpacing(1);faceContent.ColumnSpacing(6);
        faceContent.Children().Append(faceIcon);faceContent.Children().Append(faceLabel);faceContent.Children().Append(faceValue);
        face.Content(faceContent);
        Grid cell;cell.Children().Append(row);cell.Children().Append(face);
        auto labeled=flag(style,L"labeled");
        auto updateFace=[weak,state,settingSpec,faceValue,labeled]{
            auto self=weak.lock();if(!self)return;
            auto format=[&](bool units){return str(toolbarUi(O({{L"type",S(L"number")},{L"request",O({{L"control",settingSpec},{L"value",N(state->value)},
                {L"operation",O({{L"type",S(L"format")}})}})},{L"compact",B(true)},{L"units",B(units)}})).GetObject(),L"text");};
            auto text=format(self->tileStyle!=L"small");
            double available=std::min(array(self->style,L"size").GetNumberAt(0),self->width>0?self->width:1e9)-(labeled?38:4);
            if(textWidth(self->data,eights(text))+2>available)text=format(false);
            faceValue.Text(text);
            faceValue.FontSize(self->data->textSize()*(self->tileStyle==L"small"&&text.size()>=4?.9:1.));
        };
        face.Click([weak,state,settingSpec,title,set](winrt::Windows::Foundation::IInspectable const& sender,auto&&){
            auto self=weak.lock();if(!self)return;
            if(std::exchange(state->scrubbed,false))return;
            self->closePopup();
            Bindings bindings;StackPanel content;content.MinWidth(220);content.MaxWidth(320);
            content.Children().Append(number(self->data,title,settingSpec,[state]{return state->value;},set,bindings));
            for(auto const& bind:bindings)bind();
            self->editor=Flyout();self->editor.Content(content);TrackPopup(self->editor,self->data);
            self->editor.ShowAt(sender.as<FrameworkElement>());
        });
        face.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([state,settingSpec](winrt::Windows::Foundation::IInspectable const& sender,PointerRoutedEventArgs const& e){
            auto p=e.GetCurrentPoint(sender.as<UIElement>());
            if(p.PointerDeviceType()==NativeInput::PointerDeviceType::Mouse)return;
            state->scrubbed=false;state->contact=Contact{p.PointerId(),p.Position()};
            state->startFill=num(numeric(settingSpec,state->value,O({{L"type",S(L"format")}})),L"fill");
        })),true);
        face.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([state,settingSpec,set](winrt::Windows::Foundation::IInspectable const& sender,PointerRoutedEventArgs const& e){
            if(!state->contact||state->contact->id!=e.Pointer().PointerId())return;
            auto p=e.GetCurrentPoint(sender.as<UIElement>()).Position();
            if(!state->contact->moved&&std::abs(p.Y-state->contact->start.Y)<8)return;
            state->contact->moved=true;
            auto next=numeric(settingSpec,state->value,O({{L"type",S(L"position")},{L"position",N(state->startFill+(state->contact->start.Y-p.Y)/200)}}));
            if(num(next,L"value")!=state->value)set(num(next,L"value"));
        })),true);
        auto finish=[state](winrt::Windows::Foundation::IInspectable const&,PointerRoutedEventArgs const& e){
            if(state->contact&&state->contact->id==e.Pointer().PointerId()){state->scrubbed=state->contact->moved;state->contact.reset();}
        };
        face.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler(finish)),true);
        face.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler(finish)),true);
        face.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler(finish)),true);
        face.PointerWheelChanged([state,settingSpec,set](auto&&,PointerRoutedEventArgs const& e){
            auto delta=e.GetCurrentPoint(nullptr).Properties().MouseWheelDelta();if(!delta)return;
            e.Handled(true);
            auto next=numeric(settingSpec,state->value,O({{L"type",S(L"step")},{L"steps",N(delta>0?1:-1)}}));
            if(num(next,L"value")!=state->value)set(num(next,L"value"));
        });
        Field result;result.row=cell;
        result.update=[state,updateFace,bindings](J const& option){
            state->value=num(object(option,L"Numeric"),L"value");
            for(auto const& bind:*bindings)bind();
            updateFace();
        };
        result.orient=[weak,state,name,glyph,numberView,inlineTrack,face,faceLabel,faceIcon,faceValue,labeled,updateFace]{
            auto self=weak.lock();if(!self)return;
            state->units=!self->vertical||self->tileStyle!=L"small";
            name.Visibility(!self->vertical&&self->text?Visibility::Visible:Visibility::Collapsed);
            glyph.Visibility(!self->vertical&&!self->text?Visibility::Visible:Visibility::Collapsed);
            numberView.Visibility(self->vertical?Visibility::Collapsed:Visibility::Visible);
            face.Visibility(self->vertical?Visibility::Visible:Visibility::Collapsed);
            inlineTrack.Visibility(self->sliders?Visibility::Visible:Visibility::Collapsed);
            faceLabel.Visibility(labeled?Visibility::Visible:Visibility::Collapsed);
            auto content=face.Content().as<Grid>();content.RowDefinitions().Clear();content.ColumnDefinitions().Clear();
            if(labeled){
                ColumnDefinition iconColumn;iconColumn.Width({16,GridUnitType::Pixel});content.ColumnDefinitions().Append(iconColumn);
                ColumnDefinition textColumn;textColumn.Width({1,GridUnitType::Star});content.ColumnDefinitions().Append(textColumn);
                for(int i=0;i<2;++i){RowDefinition line;line.Height({1,GridUnitType::Star});content.RowDefinitions().Append(line);}
                content.Padding({8,4,8,4});Grid::SetRowSpan(faceIcon,2);Grid::SetColumn(faceLabel,1);Grid::SetColumn(faceValue,1);Grid::SetRow(faceValue,1);
                faceValue.HorizontalAlignment(HorizontalAlignment::Left);faceIcon.VerticalAlignment(VerticalAlignment::Center);
            }else{
                for(int i=0;i<2;++i){RowDefinition line;line.Height({1,GridUnitType::Auto});content.RowDefinitions().Append(line);}
                content.Padding({0,0,0,0});content.VerticalAlignment(VerticalAlignment::Center);
                Grid::SetRowSpan(faceIcon,1);Grid::SetRow(faceValue,1);Grid::SetColumn(faceValue,0);
                faceValue.HorizontalAlignment(HorizontalAlignment::Center);faceIcon.HorizontalAlignment(HorizontalAlignment::Center);
            }
            updateFace();
        };
        result.natural=[weak,title,info]{
            auto self=weak.lock();if(!self)return winrt::Windows::Foundation::Size{};
            double value=0;for(auto sample:array(info,L"samples"))value=std::max(value,textWidth(self->data,eights(sample.GetString())));
            double lead=self->text?textWidth(self->data,title):16;
            return winrt::Windows::Foundation::Size{float(lead+4+value+14+(self->sliders?60:0)),24.f};
        };
        return result;
    }
    Field rangeField(J const& interval){
        auto weak=weak_from_this();auto bounds=array(interval,L"bounds");
        std::array<hstring,2> ids{str(bounds.GetObjectAt(0),L"id"),str(bounds.GetObjectAt(1),L"id")};
        auto range=RangeControl::Create(data,bounds.GetObjectAt(0),bounds.GetObjectAt(1),str(interval,L"label"),L"toolbar",sliders,
            [weak,ids](int index,double value){if(auto self=weak.lock())
                self->send(O({{L"type",S(L"set_tool_setting")},{L"id",S(ids[index])},{L"value",N(value)}}));});
        Field result;result.row=range->root;result.interval=true;
        result.update=[range](J const& option){
            auto current=array(object(option,L"Range"),L"bounds");
            range->Update(num(current.GetObjectAt(0),L"value"),num(current.GetObjectAt(1),L"value"));
        };
        result.dispose=[range]{range->Dispose();};
        result.natural=[weak,range]{
            auto self=weak.lock();if(!self)return winrt::Windows::Foundation::Size{};
            range->root.Measure({std::numeric_limits<float>::infinity(),std::numeric_limits<float>::infinity()});
            auto desired=range->root.DesiredSize();
            return winrt::Windows::Foundation::Size{std::max(desired.Width,self->sliders?280.f:0.f),std::max(desired.Height,24.f)};
        };
        return result;
    }
    Field choiceField(J const& choice){
        auto weak=weak_from_this();
        bool segmented=flag(choice,L"segmented");auto id=str(choice,L"id");auto items=array(choice,L"items");
        Field result;
        if(segmented){
            Grid row;row.Background(data->brush(L"input"));row.CornerRadius({6,6,6,6});
            AutomationProperties::SetName(row,str(choice,L"label"));AutomationProperties::SetAutomationId(row,L"toolbar-choice-"+id);
            std::vector<Button> buttons;std::vector<hstring> glyphs;
            for(uint32_t i=0;i<items.Size();++i){
                auto entry=items.GetObjectAt(i);auto action=object(entry,L"action");
                auto pick=button(data,str(entry,L"label"),[weak,action]{if(auto self=weak.lock())self->send(action);});
                pick.CornerRadius({0,0,0,0});pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.VerticalAlignment(VerticalAlignment::Stretch);
                glyphs.push_back(str(entry,L"icon"));ToolTipService::SetToolTip(pick,box_value(str(entry,L"label")));
                AutomationProperties::SetAutomationId(pick,L"toolbar-segment-"+id+L"-"+to_hstring(i));
                row.Children().Append(pick);buttons.push_back(pick);
            }
            result.row=row;result.segmented=int(items.Size());
            result.update=[buttons,data=data](J const& option){
                auto current=array(object(option,L"Choice"),L"items");
                for(uint32_t i=0;i<std::min<uint32_t>(current.Size(),uint32_t(buttons.size()));++i){
                    bool chosen=flag(current.GetObjectAt(i),L"selected");
                    buttons[i].Background(chosen?selected(data):clear());AutomationProperties::SetItemStatus(buttons[i],chosen?L"Selected":L"");
                }
            };
            result.orient=[weak,row,buttons,glyphs]{
                auto self=weak.lock();if(!self)return;
                double tileW=array(self->style,L"size").GetNumberAt(0);
                bool stacked=self->vertical&&self->width<tileW*double(buttons.size());
                row.RowDefinitions().Clear();row.ColumnDefinitions().Clear();
                for(size_t i=0;i<buttons.size();++i){
                    if(stacked){RowDefinition line;line.Height({1,GridUnitType::Star});row.RowDefinitions().Append(line);Grid::SetRow(buttons[i],int(i));Grid::SetColumn(buttons[i],0);}
                    else{ColumnDefinition cell;cell.Width({1,GridUnitType::Star});row.ColumnDefinitions().Append(cell);Grid::SetColumn(buttons[i],int(i));Grid::SetRow(buttons[i],0);}
                    double first=i==0?6:0,last=i+1==buttons.size()?6:0;
                    buttons[i].CornerRadius(stacked?CornerRadius{first,first,last,last}:CornerRadius{first,last,last,first});
                    buttons[i].Content(icon(glyphs[i],self->data->theme(),self->vertical?self->iconSize:16));
                }
            };
            return result;
        }
        auto pick=button(data,str(choice,L"label"),[]{});
        pick.Background(data->brush(L"input"));pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        pick.Padding({6,0,6,0});AutomationProperties::SetAutomationId(pick,L"toolbar-choice-"+id);
        ToolTipService::SetToolTip(pick,box_value(str(choice,L"label")));
        auto labelText=std::make_shared<J>(choice);auto shown=std::make_shared<hstring>();
        pick.Click([weak,labelText](winrt::Windows::Foundation::IInspectable const& sender,auto&&){
            auto self=weak.lock();if(!self)return;
            MenuFlyout menu;TrackPopup(menu,self->data);
            for(auto value:array(*labelText,L"items")){
                auto entry=value.GetObject();auto action=object(entry,L"action");
                RadioMenuFlyoutItem option;option.Text(str(entry,L"label"));option.IsChecked(flag(entry,L"selected"));
                option.GroupName(L"toolbar-choice");
                option.Click([weak,action](auto&&,auto&&){if(auto self=weak.lock())self->send(action);});
                menu.Items().Append(option);
            }
            menu.ShowAt(sender.as<FrameworkElement>());
        });
        result.row=pick;
        result.update=[weak,pick,labelText,shown](J const& option){
            auto self=weak.lock();if(!self)return;
            *labelText=object(option,L"Choice");auto current=array(*labelText,L"items");
            J chosen=current.Size()?current.GetObjectAt(0):J{};
            for(auto value:current)if(flag(value.GetObject(),L"selected"))chosen=value.GetObject();
            auto key=str(chosen,L"label")+L"|"+str(chosen,L"icon");
            if(key==*shown)return;
            *shown=key;
            Grid content;content.ColumnSpacing(6);
            for(auto width:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){
                ColumnDefinition column;column.Width(width);content.ColumnDefinitions().Append(column);
            }
            auto glyph=icon(str(chosen,L"icon"),self->data->theme());content.Children().Append(glyph);
            auto name=label(self->data,str(chosen,L"label"));name.TextTrimming(TextTrimming::CharacterEllipsis);
            name.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(name,1);content.Children().Append(name);
            auto chevron=icon(L"chevron-down",self->data->theme(),12);Grid::SetColumn(chevron,2);content.Children().Append(chevron);
            pick.Content(content);AutomationProperties::SetItemStatus(pick,str(chosen,L"label"));
        };
        result.orient=[weak,pick]{
            auto self=weak.lock();if(!self)return;
            auto content=pick.Content().try_as<Grid>();if(!content)return;
            bool compact=self->vertical&&!flag(self->style,L"labeled");
            content.Children().GetAt(1).as<FrameworkElement>().Visibility(compact?Visibility::Collapsed:Visibility::Visible);
            content.Children().GetAt(2).as<FrameworkElement>().Visibility(self->vertical?Visibility::Collapsed:Visibility::Visible);
            pick.Background(self->vertical?clear():self->data->brush(L"input"));
            pick.HorizontalContentAlignment(compact?HorizontalAlignment::Center:HorizontalAlignment::Stretch);
        };
        result.natural=[]{return winrt::Windows::Foundation::Size{168,24};};
        return result;
    }
    Field actionField(J const& action){
        auto weak=weak_from_this();
        auto state=object(action,L"state");auto command=str(state,L"id");bool checkable=flag(action,L"checkable");
        auto pick=button(data,str(state,L"label"),[weak,command]{
            if(auto self=weak.lock())self->send(O({{L"type",S(L"invoke")},{L"command",S(command)}}));
        });
        auto glyph=str(state,L"icon");pick.Content(icon(glyph.empty()?L"settings":glyph,data->theme(),iconSize));
        ToolTipService::SetToolTip(pick,box_value(str(state,L"tooltip")));
        AutomationProperties::SetAutomationId(pick,L"toolbar-action-"+command);
        Field result;result.row=pick;result.action=true;
        result.update=[pick,checkable,data=data](J const& option){
            auto current=object(object(option,L"Action"),L"state");bool enabled=flag(current,L"enabled");
            pick.IsEnabled(enabled);pick.Opacity(enabled?1:.36);
            bool chosen=checkable&&flag(current,L"selected");
            pick.Background(chosen?selected(data):clear());AutomationProperties::SetItemStatus(pick,chosen?L"Selected":L"");
        };
        return result;
    }
};

ToolbarComponent::ToolbarComponent(std::shared_ptr<WorkspaceData> data,J const& panel,J const& tile,
    std::shared_ptr<WorkspaceGestures> const& gestures):impl(std::make_shared<Impl>()){
    impl->data=std::move(data);impl->gestures=gestures;impl->init(panel,tile);
}
ToolbarComponent::~ToolbarComponent()=default;
FrameworkElement ToolbarComponent::Root()const{return impl->root;}
void ToolbarComponent::Update(J const& tile){impl->Update(tile);}
void ToolbarComponent::Layout(J const& bounds,bool vertical){impl->Layout(bounds,vertical);}
