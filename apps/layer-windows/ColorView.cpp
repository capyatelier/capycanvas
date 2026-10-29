#include "pch.h"
#include "ColorEditor.h"
#include "ColorView.h"
#include "SwatchFace.h"
#include "PointerGesture.h"
#include "NativeMenus.h"
#include "WorkspaceQuery.h"
#include "Checker.h"
#include "ColorWheel.h"
#include "ColorTouch.h"
#include "ColorIntensity.h"
#include <dwrite_2.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Input.h>
#include <array>
#include <map>
#include <robuffer.h>
#include <winrt/Windows.Storage.Streams.h>
#include <optional>
#include <fstream>
#include <iterator>

using namespace CapyUi;
using namespace CapyWheel;
using namespace winrt::Windows::Foundation;
namespace {

// Rasterize at the final glyph transform. The shared browser uses a grayscale
// reduction of DirectWrite's three-channel mask, with sRGB text correction.
// Match its font-cache precision and hinting before WinUI composites the image.
// See the Color panel rendering references in README.md.
struct GlyphRasterizer {
    com_ptr<IDWriteFactory2> factory;
    std::array<com_ptr<IDWriteFontFace>,2> faces;
    std::map<uint32_t,std::array<uint8_t,256>> tables;
    GlyphRasterizer(){
        check_hresult(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED,__uuidof(IDWriteFactory2),reinterpret_cast<::IUnknown**>(factory.put())));
        com_ptr<IDWriteFontCollection> fonts;check_hresult(factory->GetSystemFontCollection(fonts.put()));
        UINT index=0;BOOL found=FALSE;check_hresult(fonts->FindFamilyName(L"Segoe UI",&index,&found));check_bool(found);
        com_ptr<IDWriteFontFamily> family;check_hresult(fonts->GetFontFamily(index,family.put()));
        for(size_t i=0;i<faces.size();i++){
            com_ptr<IDWriteFont> font;check_hresult(family->GetFirstMatchingFont(i?DWRITE_FONT_WEIGHT_BOLD:DWRITE_FONT_WEIGHT_NORMAL,DWRITE_FONT_STRETCH_NORMAL,DWRITE_FONT_STYLE_NORMAL,font.put()));
            check_hresult(font->CreateFontFace(faces[i].put()));
        }
    }
    static float fontSize(double value){return std::floor(float(value)*100.f)/100.f;}
    static float linear(float x){return x<=.04045f?x/12.92f:std::pow((x+.055f)/1.055f,2.4f);}
    static float srgb(float x){return x<=.0031308f?12.92f*x:1.055f*std::pow(x,1.f/2.4f)-.055f;}
    static int canonical(int x){x>>=5;return (x<<5)|(x<<2)|(x>>1);}
    static std::array<uint8_t,256> coverage(winrt::Windows::UI::Color ink){
        int lum=(54*canonical(ink.R)+183*canonical(ink.G)+19*canonical(ink.B))>>8;
        float src=canonical(lum)/255.f,dst=1-src,ls=linear(src),ld=linear(dst);
        std::array<uint8_t,256> result;
        for(size_t i=0;i<result.size();i++){
            float a=float(i)/255.f;a+=a*(1-a)*ld;
            float corrected=(srgb(ls*a+ld*(1-a))-dst)/(src-dst);
            result[i]=uint8_t(std::clamp(std::lround(corrected*255),0l,255l));
        }
        return result;
    }
    void draw(Image const& target,std::wstring_view text,double font,bool bold,Matrix const& position,double scale,winrt::Windows::UI::Color ink,float opacity){
        auto relaxed=[&](double x){return std::floor(float(x*scale)*1024+.5f)/1024;};
        float a=relaxed(position.M11),b=relaxed(position.M12),c=relaxed(position.M21),d=relaxed(position.M22);
        float length=std::sqrt(c*c+d*d);bool rotated=b!=0||c!=0;
        auto quarter=[&](double x){return std::floor(float(x*scale)*4+.5f)/4;};
        DWRITE_MATRIX transform{a/length,b/length,c/length,d/length,quarter(position.OffsetX),rotated?quarter(position.OffsetY):std::floor(float(position.OffsetY*scale)+.5f)};
        auto face=faces[bold].get();auto count=uint32_t(text.size());float em=fontSize(font)*length;
        std::vector<UINT32> codes(text.begin(),text.end());std::vector<UINT16> glyphs(count);
        check_hresult(face->GetGlyphIndices(codes.data(),count,glyphs.data()));
        std::vector<DWRITE_GLYPH_METRICS> metrics(count);check_hresult(face->GetDesignGlyphMetrics(glyphs.data(),count,metrics.data()));
        DWRITE_FONT_METRICS fontMetrics;face->GetMetrics(&fontMetrics);std::vector<float> advances(count);
        std::vector<INT32> kerning(count);
        if(count>1)check_hresult(faces[bold].as<IDWriteFontFace1>()->GetKerningPairAdjustments(count,glyphs.data(),kerning.data()));
        double cursor=0;
        for(size_t i=0;i<count;i++){
            double width=fontSize(font)*(double(metrics[i].advanceWidth)+kerning[i])/fontMetrics.designUnitsPerEm;
            // Horizontal labels retain pair kerning and each glyph's quarter-pixel origin.
            advances[i]=count>1&&!rotated?quarter(position.OffsetX+cursor+width)-quarter(position.OffsetX+cursor):float(width)*length;
            cursor+=width;
        }
        DWRITE_GLYPH_RUN run{face,em,count,glyphs.data(),advances.data(),nullptr,FALSE,0};
        com_ptr<IDWriteGlyphRunAnalysis> analysis;
        auto mode=DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC;
        if(!rotated){
            mode=em>20?DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC:DWRITE_RENDERING_MODE_NATURAL;
            void const* table=nullptr;UINT32 bytes=0;void* context=nullptr;BOOL exists=FALSE;
            check_hresult(face->TryGetFontTable(DWRITE_MAKE_OPENTYPE_TAG('g','a','s','p'),&table,&bytes,&context,&exists));
            if(exists){
                auto data=static_cast<uint8_t const*>(table);
                auto word=[&](size_t i){return (unsigned(data[i])<<8)|data[i+1];};
                if(bytes>=4&&word(0)==1){
                    for(size_t i=4;i+3<bytes&&i<4+size_t(word(2))*4;i+=4){
                        if(std::lround(em)<=long(word(i))){mode=(word(i+2)&8)?DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC:DWRITE_RENDERING_MODE_NATURAL;break;}
                    }
                }
                face->ReleaseFontTable(context);
            }
        }
        check_hresult(factory->CreateGlyphRunAnalysis(&run,&transform,mode,
            DWRITE_MEASURING_MODE_NATURAL,rotated?DWRITE_GRID_FIT_MODE_DISABLED:DWRITE_GRID_FIT_MODE_ENABLED,
            DWRITE_TEXT_ANTIALIAS_MODE_CLEARTYPE,0,0,analysis.put()));
        RECT box{};check_hresult(analysis->GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1,&box));
        int width=box.right-box.left,height=box.bottom-box.top;
        if(width<=0||height<=0){target.Source(nullptr);return;}
        std::vector<uint8_t> samples(size_t(width)*height*3);
        check_hresult(analysis->CreateAlphaTexture(DWRITE_TEXTURE_CLEARTYPE_3x1,&box,samples.data(),uint32_t(samples.size())));
        // Transparent padding preserves interpolation at a fractional parent origin.
        Imaging::WriteableBitmap bitmap(width+2,height+2);uint8_t* pixels=nullptr;
        check_hresult(bitmap.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&pixels));
        std::fill_n(pixels,size_t(width+2)*(height+2)*4,uint8_t(0));
        auto [entry,inserted]=tables.try_emplace(uint32_t(ink.R)*65536+uint32_t(ink.G)*256+ink.B);
        if(inserted)entry->second=coverage(ink);auto const& table=entry->second;
        for(int y=0;y<height;y++)for(int x=0;x<width;x++){
            auto mask=&samples[(size_t(y)*width+x)*3];uint8_t alpha=uint8_t(std::lround(table[(mask[0]+mask[1]+mask[2])/3]*opacity));
            auto pixel=pixels+((size_t(y)+1)*(width+2)+x+1)*4;
            pixel[0]=uint8_t((ink.B*alpha+127)/255);pixel[1]=uint8_t((ink.G*alpha+127)/255);
            pixel[2]=uint8_t((ink.R*alpha+127)/255);pixel[3]=alpha;
        }
        bitmap.Invalidate();target.Source(bitmap);target.Width((width+2)/scale);target.Height((height+2)/scale);
        Canvas::SetLeft(target,(box.left-1)/scale);Canvas::SetTop(target,(box.top-1)/scale);
    }
};

Canvas colorIcon(hstring const& name,Brush const& ink){
    wchar_t executable[32768];auto length=GetModuleFileNameW(nullptr,executable,32768);
    auto path=std::filesystem::path(std::wstring(executable,length)).parent_path()/L"Assets"/L"color-icons"/(std::wstring(name)+L".xaml");
    std::ifstream file(path);
    if(!file)throw hresult_error(E_FAIL,L"Missing shared color icon");
    std::string xaml((std::istreambuf_iterator<char>(file)),std::istreambuf_iterator<char>());
    auto result=Markup::XamlReader::Load(to_hstring(xaml)).as<Canvas>();
    result.HorizontalAlignment(HorizontalAlignment::Center);result.VerticalAlignment(VerticalAlignment::Center);
    result.IsHitTestVisible(false);
    for(auto child:result.Children())child.as<Shapes::Path>().Stroke(ink);
    return result;
}

struct View:std::enable_shared_from_this<View>{
    std::shared_ptr<WorkspaceData> data;
    Grid root;Canvas stage,wheel,readoutBody;
    bool fitHeight=false;
    Image image;TextBlock drawError;
    std::array<Button,3> swatches;
    std::array<std::shared_ptr<PointerClick>,2> slotClicks;
    struct SlotHold {PointerGesture recognizer;std::optional<uint32_t> pointer;};
    std::array<SlotHold,2> slotHolds;
    std::array<SwatchFace,3> swatchFaces;
    std::array<bool,3> hovered{};
    Button edit;Shapes::Ellipse editFill;
    Canvas arc;Shapes::Path arcTrack;Slider intensity;std::shared_ptr<ColorIntensity> intensityArc;
    std::shared_ptr<ControlCapture> wheelCapture,arcCapture,touchCapture;hstring arcKey,arcContext;
    ColorTouches touches;
    bool layoutHdr=false;double frameWidth=0,frameHeight=0;
    uint64_t listener=0;
    std::array<Button,2> shapes;
    std::array<Canvas,2> shapeGlyphs{nullptr,nullptr};
    std::array<bool,2> shapeHovered{};
    Shapes::Ellipse swapFill;Image swapGlyph;
    Button swap,readout;
    Shapes::Path readoutHit;
    TextBlock labelMetrics;
    Image labelImage,gamutImage;hstring labelKey;
    Shapes::Path readoutFocus;
    struct Glyph {Image image;MatrixTransform transform;wchar_t scalar=0;std::array<double,9> key{};};
    std::vector<Glyph> glyphs,captionGlyphs;
    std::array<Border,3> chips;
    std::array<MatrixTransform,3> chipTransforms;
    TextBlock metrics;
    std::map<wchar_t,double> advances;
    double measuredFont=0;
    WheelImage drawing;
    hstring context,key,readoutKey,iconKey;
    uint32_t part=0;
    double side=0,panelSize=0,layoutScale=0;
    J layout;
    MenuFlyout menu{nullptr};OwnedPopup menuOwner;
    XamlRoot::Changed_revoker scaleChanged;
    CompositionTarget::SurfaceContentsLost_revoker contentsLost;
    explicit View(std::shared_ptr<WorkspaceData> source,bool fit):data(std::move(source)),fitHeight(fit){}
    ~View(){
        data->colorViews.erase(listener);
    }
    void editColor(hstring slot={}){
        if(slot.empty())slot=str(displayColors(data->state),L"paint_slot");
        EditColor(data,root,O({{L"slot",S(slot)}}),false,[data=data,slot](J color,std::optional<double> stops){
            auto action=O({{L"op",S(stops?L"set_slot_intensity":L"set_slot")},{L"slot",S(slot)},{L"color",color}});
            if(stops)action.Insert(L"stops",N(*stops));
            data->dispatch(O({{L"type",S(L"color")},{L"action",action}}));
        });
    }
    bool previewing()const{return WorkspaceData::previewing(data->colorPreview);}
    J model()const{return previewing()?object(data->colorPreview,L"view"):object(data->model,L"color_panel");}
    J paintColors()const{return previewing()?object(data->colorPreview,L"colors"):displayColors(data->state);}
    static J geometry(std::shared_ptr<WorkspaceData> const& data,double size,bool hdr){return colorUi(data->localization.get(),O({{L"type",S(L"layout")},{L"size",N(size)},{L"hdr",B(hdr)}})).GetObject();}
    void pickIntensity(Point position){
        if(arcContext.empty()||arcContext!=intensityContext())return;
        setIntensity(intensityArc->pick(position));
    }
    void setIntensity(double stops){sendEdit(O({{L"op",S(L"hdr_intensity")},{L"stops",N(stops)}}),!arcContext.empty()?arcContext:intensityContext());}
    bool transparentSlot()const{return str(displayColors(data->state),L"slot")==L"transparent";}
    void cancelArc(){if(arcCapture)arcCapture->Cancel();}
    void contact(hstring const& phase){data->dispatch(O({{L"type",S(L"color_contact")},{L"phase",S(phase)}}));}
    hstring editingContext()const{return object(data->model,L"color_wheel_context").Stringify();}
    hstring intensityContext()const{return object(data->model,L"color_edit_context").Stringify();}
    void sendEdit(J const& action,hstring const& expected){data->dispatch(O({{L"type",S(L"color_edit")},{L"context",J::Parse(expected)},{L"action",action}}));}
    void send(J const& action){data->dispatch(O({{L"type",S(L"color")},{L"action",action}}));}
    void showMenu(FrameworkElement const& anchor,J const& target,Point at){
        if(data->externalPopup||data->popupCount)return;
        menu=MenuFlyout();auto ticket=menuOwner.Begin(data,anchor,menu);
        if(!QueryWorkspace(data->query,O({{L"type",S(L"context")},{L"target",target}}),
            [weak=weak_from_this(),ticket,anchor=make_weak(anchor),at](J reply){if(auto self=weak.lock()){
                auto source=anchor.get();auto sections=array(object(reply,L"result"),L"sections");
                if(!source||!ticket.Current()||!sections.Size()){ticket.Cancel();return;}
                NativeMenuItems(self->menu.Items(),sections,self->data,[weak](J action){if(auto self=weak.lock())self->data->dispatch(action);});
                Primitives::FlyoutShowOptions options;options.Position(at);ticket.Show(source,options);
            }}))ticket.Cancel();
    }
    void finishPick(hstring const& phase){
        if(part)contact(phase);
        part=0;AutomationProperties::SetItemStatus(root,data->caption(L"header",L"ready"));
    }
    void cancel(){if(wheelCapture)wheelCapture->Cancel();if(touchCapture)touchCapture->Cancel();}
    bool joinsTouches(Microsoft::UI::Input::PointerPoint const& p)const{
        return p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch&&!touches.contacts.empty();
    }
    bool historyArea(Point at)const{
        if(side<1)return false;
        auto box=array(layout,L"wheel");double radius=side/2;
        if(std::hypot(at.X-box.GetNumberAt(0)-radius,at.Y-box.GetNumberAt(1)-radius)<=radius)return true;
        if(arc.Visibility()!=Visibility::Visible||transparentSlot())return false;
        A point;point.Append(N(at.X));point.Append(N(at.Y));
        return flag(colorUi(data->localization.get(),O({{L"type",S(L"arc")},{L"size",N(panelSize)},{L"point",point}})).GetObject(),L"hit");
    }
    void touch(PointerRoutedEventArgs const& e,hstring phase){
        auto p=e.GetCurrentPoint(stage);auto id=p.PointerId();
        if(p.PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Touch)return;
        if(phase==L"down"&&!touches.contacts.contains(id)){
            if(!historyArea(p.Position()))return;
            bool held=PointerGesture::Captured(root,id)||PointerGesture::Captured(arcTrack,id);
            if(!touchCapture->Begin(e,!held))return;
        }
        bool claimed=touches.claimed();
        if(touches.forward(*data,stage,p,phase,phase==L"down"&&historyArea(p.Position()))){
            if(!claimed){wheelCapture->HandOff();if(arcCapture)arcCapture->HandOff();}
            e.Handled(true);
        }
    }
    void pick(Point position){
        if(!part||context!=editingContext()||side<1)return;
        A location;location.Append(N(position.X));location.Append(N(position.Y));
        sendEdit(O({{L"op",S(L"pick_wheel")},{L"part",S(part==1?L"hue":L"field")},{L"point",location},{L"size",N(side)}}),context);
    }
    static void placeBox(FrameworkElement const& element,A const& box){
        Canvas::SetLeft(element,box.GetNumberAt(0));Canvas::SetTop(element,box.GetNumberAt(1));
        element.Width(box.GetNumberAt(2));element.Height(box.GetNumberAt(3));
    }
    Button control(hstring const& name,std::function<void()> action){
        auto result=button(data,name,std::move(action));result.Background(nullptr);result.UseSystemFocusVisuals(false);
        result.HorizontalContentAlignment(HorizontalAlignment::Stretch);result.VerticalContentAlignment(VerticalAlignment::Stretch);
        return result;
    }
    void init(){
        auto weak=weak_from_this();
        root.UseLayoutRounding(false);root.Background(clear());edgeTag(root,L"edge_contact",S(L"input"));root.MinWidth(128);root.MinHeight(128);copyName(data,root,data->copyCaption(L"color",L"controls"));AutomationProperties::SetAutomationId(root,L"color-controls");AutomationProperties::SetItemStatus(root,data->caption(L"header",L"ready"));
        root.Children().Append(stage);stage.HorizontalAlignment(HorizontalAlignment::Center);stage.VerticalAlignment(VerticalAlignment::Center);
        AutomationProperties::SetAutomationId(stage,L"color-panel");copyName(data,stage,data->copyCaption(L"color",L"picker"));
        wheel.Background(clear());copyName(data,image,data->copyCaption(L"color",L"wheel"));
        AutomationProperties::SetAutomationId(image,L"color-wheel");
        image.IsHitTestVisible(false);image.Stretch(Stretch::Fill);wheel.Children().Append(image);stage.Children().Append(wheel);
        drawError.Text(data->caption(L"color",L"wheel_unavailable"));drawError.TextWrapping(TextWrapping::Wrap);drawError.FontSize(12);
        data->copyView([target=make_weak(drawError),resolve=data->copyCaption(L"color",L"wheel_unavailable").current]{if(auto text=target.get()){text.Text(resolve());return true;}return false;});
        drawError.Foreground(data->brush(L"text"));drawError.IsHitTestVisible(false);drawError.Visibility(Visibility::Collapsed);wheel.Children().Append(drawError);
        data->copyView([weak]{if(auto self=weak.lock()){
            AutomationProperties::SetItemStatus(self->root,self->part?self->data->caption(L"color",L"picking"):self->data->caption(L"header",L"ready"));
            AutomationProperties::SetItemStatus(self->image,self->drawError.Visibility()==Visibility::Visible?self->data->caption(L"color",L"wheel_failed"):self->data->caption(L"header",L"ready"));return true;
        }return false;});
        root.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
        // The host owns capture; shared geometry limits it to wheel contacts.
        // Native buttons consume their own presses before this bubbles here.
        wheelCapture=captureControl(data,root,[weak](uint32_t,bool cancel,PointerRoutedEventArgs const* event){if(auto self=weak.lock()){
            cancel=cancel||self->context!=self->editingContext();
            if(!cancel&&event)self->pick(event->GetCurrentPoint(self->wheel).Position());
            self->finishPick(cancel?L"cancel":L"up");if(event)event->Handled(true);
        }});
        touchCapture=captureControl(data,root,[weak](uint32_t id,bool cancel,PointerRoutedEventArgs const* event){if(auto self=weak.lock()){
            if(event)self->touch(*event,cancel?L"cancel":L"up");
            else if(auto found=self->touches.contacts.find(id);found!=self->touches.contacts.end()){
                ColorTouches::send(*self->data,self->stage,id,L"cancel",found->second,0);self->touches.contacts.erase(found);
            }
        }},PointerCaptureLoss::CompleteLift);
        root.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->wheel);
            if(self->wheelCapture->Active()||!PointerGesture::PrimaryContact(p)||self->joinsTouches(p))return;
            auto at=p.Position();auto shape=str(self->model(),L"shape");
            auto part=capy_color_hit(at.X,at.Y,float(self->side),shape==L"circle"?2:shape==L"triangle"?1:0);
            if(part&&self->wheelCapture->Begin(e)){self->part=part;AutomationProperties::SetItemStatus(self->root,self->data->caption(L"color",L"picking"));self->contact(L"down");self->pick(at);e.Handled(true);}
        }});
        root.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(self->wheelCapture->Owns(e.Pointer().PointerId())){self->pick(e.GetCurrentPoint(self->wheel).Position());e.Handled(true);}
        }});
        root.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->touch(e,L"down");})),true);
        root.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->touchCapture->Owns(e.Pointer().PointerId()))self->touch(e,L"move");})),true);
        root.ContextRequested([weak](auto&&,ContextRequestedEventArgs const& e){if(auto self=weak.lock()){
            Point at{};
            if(!e.TryGetPosition(self->stage,at)){
                if(e.OriginalSource()!=self->intensity)return;
                auto middle=array(self->intensityArc->at(.5),L"point");at={float(middle.GetNumberAt(0)),float(middle.GetNumberAt(1))};
            }else if(!self->historyArea(at))return;
            e.Handled(true);
            if(!self->wheelCapture->Active()&&(!self->arcCapture||!self->arcCapture->Active())&&self->touches.contacts.empty())self->showMenu(self->stage,O({{L"kind",S(L"color_wheel")}}),at);
        }});
        root.ContextCanceled([weak](auto&&,auto&&){if(auto self=weak.lock())self->menuOwner.Cancel();});
        root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock()){
            self->scaleChanged=self->root.XamlRoot().Changed(auto_revoke,[weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
            self->refresh();
        }});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->touches.cancel(*self->data,self->stage);self->scaleChanged.revoke();}});
        contentsLost=CompositionTarget::SurfaceContentsLost(auto_revoke,[weak](auto&&,auto&&){if(auto self=weak.lock()){self->key=L"";self->drawing.surface=nullptr;self->refresh();}});
        listener=data->colorView([weak]{if(auto self=weak.lock())self->refresh();});
        drawing.start([weak]{if(auto self=weak.lock()){self->key=L"";self->data->queueColors();}});
        edit=control(data->caption(L"color",L"edit"),[weak]{if(auto self=weak.lock();self&&!self->transparentSlot())self->editColor();});
        Grid editContent;auto pencil=icon(L"pencil",data->theme());pencil.HorizontalAlignment(HorizontalAlignment::Center);pencil.VerticalAlignment(VerticalAlignment::Center);
        editContent.Children().Append(editFill);editContent.Children().Append(pencil);edit.Content(editContent);
        copyName(data,edit,data->copyCaption(L"color",L"edit"),true);AutomationProperties::SetAutomationId(edit,L"color-edit");
        stage.Children().Append(edit);
        const std::array<hstring,3> slots{L"background",L"foreground",L"transparent"};
        for(int i=0;i<3;i++){
            auto slot=slots[i];auto pick=control(slot,[weak,slot,i]{if(auto self=weak.lock()){
                if(i<2&&self->slotClicks[i]->SuppressClick())return;
                self->send(O({{L"op",S(L"select")},{L"slot",S(slot)}}));
            }});
            if(i<2)pick.DoubleTapped([weak,slot](auto&&,auto&&){if(auto self=weak.lock()){
                self->send(O({{L"op",S(L"select")},{L"slot",S(slot)}}));self->editColor(slot);
            }});
            Grid sample;sample.Background(nullptr);
            Shapes::Ellipse hit;hit.Fill(clear());sample.Children().Append(hit);
            sample.Children().Append(swatchFaces[i].root);
            pick.Content(sample);swatches[i]=pick;stage.Children().Append(pick);
            swatchFaces[i].feedback.Fill(buttonFeedbackBrush(pick,data->brush(L"text")));
            if(i<2){
                slotClicks[i]=PointerGesture::Clicks(pick,[](uint32_t,bool){});
                PointerGesture::OwnHolding(pick);
                pick.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak,i](auto&& sender,PointerRoutedEventArgs const& e){
                    if(auto self=weak.lock()){
                        auto source=sender.template as<Button>();auto point=e.GetCurrentPoint(source);auto& hold=self->slotHolds[i];
                        if(hold.pointer||!PointerGesture::PrimaryContact(point)||point.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse)return;
                        hold.pointer=point.PointerId();hold.recognizer.Begin(source,point);
                    }
                })),true);
                pick.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak,i](auto&& sender,PointerRoutedEventArgs const& e){
                    if(auto self=weak.lock();self&&self->slotHolds[i].pointer==e.Pointer().PointerId()){
                        auto& hold=self->slotHolds[i];
                        if(e.GetCurrentPoint(nullptr).Properties().IsCanceled()){self->slotClicks[i]->Suppress(e.Pointer().PointerId());hold.pointer.reset();hold.recognizer.CompleteGesture();}
                        else hold.recognizer.Move(sender.template as<Button>(),e);
                    }
                })),true);
                PointerGesture::OnCompleted(pick,[weak,i](PointerRoutedEventArgs const& e,bool cancel){if(auto self=weak.lock()){
                    auto& hold=self->slotHolds[i];if(hold.pointer!=e.Pointer().PointerId())return;
                    hold.pointer.reset();if(cancel)hold.recognizer.CompleteGesture();else hold.recognizer.ProcessUpEvent(e.GetCurrentPoint(self->swatches[i]));
                }});
                pick.Unloaded([weak,i](auto&&,auto&&){if(auto self=weak.lock()){
                    self->slotHolds[i].pointer.reset();self->slotHolds[i].recognizer.CompleteGesture();
                }});
                slotHolds[i].recognizer.Holding([weak,slot,i](auto&&,Microsoft::UI::Input::HoldingEventArgs const& e){
                    if(auto self=weak.lock();self&&self->slotHolds[i].pointer&&e.HoldingState()==Microsoft::UI::Input::HoldingState::Started){
                        self->slotClicks[i]->Suppress(*self->slotHolds[i].pointer);
                        self->showMenu(self->swatches[i],O({{L"kind",S(L"color_slot")},{L"slot",S(slot)}}),e.Position());
                    }
                });
                pick.ContextRequested([weak,slot](auto&& sender,ContextRequestedEventArgs const& e){
                    e.Handled(true);if(auto self=weak.lock()){
                        auto source=sender.template as<Button>();Point at{};if(!e.TryGetPosition(source,at))at={0,float(source.ActualHeight())};
                        self->showMenu(source,O({{L"kind",S(L"color_slot")},{L"slot",S(slot)}}),at);
                    }
                });
                pick.ContextCanceled([weak](auto&&,auto&&){if(auto self=weak.lock())self->menuOwner.Cancel();});
            }
            AutomationProperties::SetAutomationId(pick,L"color-"+slot);
            pick.PointerEntered([weak,i](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
                self->hovered[i]=e.Pointer().PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Touch;self->refresh();
            }});
            pick.PointerExited([weak,i](auto&&,auto&&){if(auto self=weak.lock()){self->hovered[i]=false;self->refresh();}});
            pick.GotFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
            pick.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
        }
        for(int i=0;i<2;i++){
            shapes[i]=control(data->caption(L"color",L"shape"),[weak,i]{if(auto self=weak.lock())self->send(O({{L"op",S(L"shape")},{L"shape",array(self->model(),L"other_shapes").GetAt(i)}}));});
            buttonFeedback(shapes[i],nullptr,0,0,nullptr,true);
            stage.Children().Append(shapes[i]);AutomationProperties::SetAutomationId(shapes[i],L"color-shape-"+to_hstring(i));
            shapes[i].PointerEntered([weak,i](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){self->shapeHovered[i]=e.Pointer().PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Touch;self->refresh();}});
            shapes[i].PointerExited([weak,i](auto&&,auto&&){if(auto self=weak.lock()){self->shapeHovered[i]=false;self->refresh();}});
        }
        swap=control(data->caption(L"color",L"swap"),[weak]{if(auto self=weak.lock())self->send(O({{L"op",S(L"swap")}}));});
        copyName(data,swap,data->copyCaption(L"color",L"swap"));
        stage.Children().Append(swap);AutomationProperties::SetAutomationId(swap,L"color-swap");
        Grid swapContent;swapGlyph=icon(L"color-swap",data->theme());swapGlyph.HorizontalAlignment(HorizontalAlignment::Center);swapGlyph.VerticalAlignment(VerticalAlignment::Center);
        swapContent.Children().Append(swapFill);swapContent.Children().Append(swapGlyph);swap.Content(swapContent);
        intensityArc=std::make_shared<ColorIntensity>();intensityArc->data=data;
        intensityArc->changed=[weak](double stops){if(auto self=weak.lock();self&&!self->transparentSlot())self->setIntensity(stops);};
        intensityArc->init(L"color-intensity");arc=intensityArc->root;arcTrack=intensityArc->track;intensity=intensityArc->access;
        arcCapture=captureControl(data,arcTrack,[weak](uint32_t,bool cancel,PointerRoutedEventArgs const* event){if(auto self=weak.lock()){
            cancel=cancel||self->arcContext!=self->intensityContext();
            if(!cancel&&event)self->pickIntensity(event->GetCurrentPoint(self->stage).Position());
            self->arcContext=L"";self->contact(cancel?L"cancel":L"up");if(event)event->Handled(true);
        }});
        arcTrack.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->stage);
            if(self->arcCapture->Active()||self->transparentSlot()||!PointerGesture::PrimaryContact(p)||self->joinsTouches(p))return;
            if(!self->arcCapture->Begin(e))return;
            self->arcContext=self->intensityContext();self->contact(L"down");self->pickIntensity(p.Position());e.Handled(true);
        }});
        arcTrack.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->arcCapture->Owns(e.Pointer().PointerId())){
            self->pickIntensity(e.GetCurrentPoint(self->stage).Position());e.Handled(true);
        }});
        stage.Children().Append(arc);
        readout=control(data->caption(L"color",L"switch_readout"),[weak]{if(auto self=weak.lock())self->send(O({{L"op",S(L"toggle_readout")}}));});
        buttonFeedback(readout,nullptr,0,0,nullptr,true);
        MenuFlyout editMenu;MenuFlyoutItem editItem;editItem.Text(data->caption(L"color",L"edit_menu"));
        data->copyView([target=make_weak(editItem),resolve=data->copyCaption(L"color",L"edit_menu").current]{if(auto item=target.get()){item.Text(resolve());return true;}return false;});
        AutomationProperties::SetAutomationId(editItem,L"edit-color");editItem.Click([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->transparentSlot())self->editColor();});
        editMenu.Items().Append(editItem);TrackPopup(editMenu,data);readout.ContextFlyout(editMenu);
        AutomationProperties::SetAutomationId(readout,L"color-readout");stage.Children().Append(readout);
        readoutHit.Fill(clear());readoutBody.Children().Append(readoutHit);
        labelMetrics.UseLayoutRounding(false);labelMetrics.FontFamily(FontFamily(L"Segoe UI"));labelMetrics.FontWeight(winrt::Windows::UI::Text::FontWeights::Bold());labelMetrics.IsHitTestVisible(false);AutomationProperties::SetAccessibilityView(labelMetrics,Automation::Peers::AccessibilityView::Raw);
        labelImage.IsHitTestVisible(false);labelImage.Stretch(Stretch::Fill);AutomationProperties::SetAccessibilityView(labelImage,Automation::Peers::AccessibilityView::Raw);readoutBody.Children().Append(labelImage);gamutImage.IsHitTestVisible(false);gamutImage.Stretch(Stretch::Fill);AutomationProperties::SetAccessibilityView(gamutImage,Automation::Peers::AccessibilityView::Raw);readoutBody.Children().Append(gamutImage);readoutFocus.IsHitTestVisible(false);
        readoutFocus.Opacity(.9);readoutFocus.StrokeThickness(1.5);readoutBody.Children().Append(readoutFocus);
        for(int i=0;i<3;i++){
            chips[i].IsHitTestVisible(false);chips[i].CornerRadius({2,2,2,2});chips[i].RenderTransform(chipTransforms[i]);
            readoutBody.Children().Append(chips[i]);
        }
        readout.Content(readoutBody);
        readout.GotFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
        readout.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
        metrics.UseLayoutRounding(false);metrics.FontFamily(FontFamily(L"Segoe UI"));metrics.IsHitTestVisible(false);
    }
    double advance(wchar_t c){
        if(auto found=advances.find(c);found!=advances.end())return found->second;
        metrics.Text(hstring(std::wstring(1,c)));metrics.Measure({1000,1000});return advances[c]=metrics.DesiredSize().Width;
    }
    static void at(MatrixTransform const& transform,double half,double radius,double angle,double x,double y){
        double a=angle+3.141592653589793/2,c=std::cos(a),s=std::sin(a);
        transform.Matrix({c,s,-s,c,half+radius*std::cos(angle)+c*x-s*y,half+radius*std::sin(angle)+s*x+c*y});
    }
    void updateReadout(J const& view){
        bool focused=readout.FocusState()==FocusState::Keyboard;
        auto next=view.Stringify()+layout.Stringify()+to_hstring(layoutScale)+data->theme()+(focused?L"/focus":L"");
        if(next==readoutKey)return;readoutKey=next;
        static thread_local GlyphRasterizer rasterizer;
        double half=array(layout,L"readout").GetNumberAt(2),radius=num(layout,L"readout_radius");
        // Match the shared canvas's integral backing size, including fractional DPI extents.
        double rasterScale=std::ceil(half*layoutScale)/half;
        double clipRadius=side*num(object(view,L"geometry"),L"outer")+2;
        PathFigure figure;figure.StartPoint({0,0});figure.IsClosed(true);figure.IsFilled(true);
        auto line=[&](float x,float y){LineSegment segment;segment.Point({x,y});figure.Segments().Append(segment);};
        line(float(half),0);line(float(half),float(half-clipRadius));
        ArcSegment bend;bend.Point({float(half-clipRadius),float(half)});bend.Size({float(clipRadius),float(clipRadius)});
        bend.SweepDirection(SweepDirection::Counterclockwise);figure.Segments().Append(bend);line(0,float(half));
        PathGeometry geometry;geometry.Figures().Append(figure);readoutHit.Data(geometry);
        auto ink=focused?accent(data):data->brush(L"text");
        double labelFont=std::clamp(panelSize*.044,9.,12.);
        labelMetrics.Text(str(view,L"readout_label"));labelMetrics.FontSize(GlyphRasterizer::fontSize(labelFont));
        labelMetrics.Measure({1000,1000});
        bool outOfGamut=flag(view,L"out_of_gamut");
        auto labelNext=labelMetrics.Text()+(outOfGamut?L"/gamut":L"")+L"/"+data->theme()+L"/"+to_hstring(labelFont)+L"/"+to_hstring(rasterScale)+(focused?L"/focus":L"");
        if(labelNext!=labelKey){
            rasterizer.draw(labelImage,std::wstring_view(labelMetrics.Text()),labelFont,true,Matrix{1,0,0,1,2,labelFont+1},rasterScale,ink.as<SolidColorBrush>().Color(),.9f);
            gamutImage.Visibility(outOfGamut?Visibility::Visible:Visibility::Collapsed);
            if(outOfGamut)rasterizer.draw(gamutImage,L"!",labelFont,true,Matrix{1,0,0,1,labelMetrics.DesiredSize().Width+6,labelFont+1},rasterScale,ink.as<SolidColorBrush>().Color(),.9f);
            labelKey=labelNext;
        }
        readoutFocus.Stroke(ink);readoutFocus.Visibility(focused?Visibility::Visible:Visibility::Collapsed);
        float w=labelMetrics.DesiredSize().Width+5,h=float(labelFont+4);
        PathFigure focusFigure;focusFigure.StartPoint({6,1});focusFigure.IsClosed(true);focusFigure.IsFilled(false);
        const std::array<Point,8> corners{{{w-4,1},{w+1,6},{w+1,h-4},{w-4,h+1},{6,h+1},{1,h-4},{1,6},{6,1}}};
        for(size_t i=0;i<corners.size();i+=2){
            LineSegment edge;edge.Point(corners[i]);focusFigure.Segments().Append(edge);
            ArcSegment corner;corner.Point(corners[i+1]);corner.Size({5,5});corner.SweepDirection(SweepDirection::Clockwise);focusFigure.Segments().Append(corner);
        }
        PathGeometry focusGeometry;focusGeometry.Figures().Append(focusFigure);readoutFocus.Data(focusGeometry);
        auto texts=array(view,L"readout_layout_text");bool rgb=str(view,L"readout")==L"rgb";
        double font=labelFont,digit=0,total=0;std::array<double,3> widths{};
        for(;;font-=.25){
            if(measuredFont!=font){advances.clear();measuredFont=font;metrics.FontSize(GlyphRasterizer::fontSize(font));}
            digit=0;for(wchar_t c=L'0';c<=L'9';c++)digit=std::max(digit,advance(c));
            total=0;
            for(int i=0;i<3;i++){
                widths[i]=rgb?font*.8+2:0;
                for(auto c:texts.GetStringAt(i))widths[i]+=(c==L' '||(c>=L'0'&&c<=L'9'))?digit:advance(c);
                total+=widths[i];
            }
            if(total+6<=radius*3.141592653589793/2-4||font<=8)break;
        }
        double chip=font*.8,gap=std::min(radius*.24,std::max(3.,(radius*3.141592653589793/2-4-total)*.5));
        double cursor=-(total+gap*2)*.5;size_t index=0;
        const std::array<winrt::Windows::UI::Color,3> rgbColors{{{255,237,79,92},{255,64,186,110},{255,74,143,250}}};
        for(int i=0;i<3;i++){
            double mid=-3*3.141592653589793/4+(cursor+widths[i]*.5)/radius;cursor+=widths[i]+gap;
            double along=-widths[i]*.5;
            chips[i].Visibility(rgb?Visibility::Visible:Visibility::Collapsed);
            if(rgb){chips[i].Width(chip);chips[i].Height(chip);chips[i].Background(fill(rgbColors[i]));at(chipTransforms[i],half,radius,mid+(along+chip*.5)/radius,-chip*.5,-font*.76);along+=chip+2;}
            for(auto c:texts.GetStringAt(i)){
                if(index==glyphs.size()){
                    Glyph glyph;glyph.image.IsHitTestVisible(false);glyph.image.Stretch(Stretch::Fill);AutomationProperties::SetAccessibilityView(glyph.image,Automation::Peers::AccessibilityView::Raw);
                    readoutBody.Children().Append(glyph.image);glyphs.push_back(glyph);
                }
                auto& glyph=glyphs[index++];glyph.image.Visibility(c==L' '?Visibility::Collapsed:Visibility::Visible);
                double cell=(c==L' '||(c>=L'0'&&c<=L'9'))?digit:advance(c);
                at(glyph.transform,half,radius,mid+(along+cell*.5)/radius,-advance(c)*.5,0);along+=cell;
                auto placement=glyph.transform.Matrix();auto color=ink.as<SolidColorBrush>().Color();
                std::array<double,9> stamp{font,placement.M11,placement.M12,placement.M21,placement.M22,placement.OffsetX,placement.OffsetY,rasterScale,double(color.R*65536+color.G*256+color.B)};
                if(c!=L' '&&(glyph.scalar!=c||glyph.key!=stamp)){
                    rasterizer.draw(glyph.image,std::wstring_view(&c,1),font,false,placement,rasterScale,color,.8f);glyph.scalar=c;glyph.key=stamp;
                }
            }
        }
        for(;index<glyphs.size();index++)glyphs[index].image.Visibility(Visibility::Collapsed);
        AutomationProperties::SetName(readout,str(view,L"readout_description"));
    }
    void updateArc(J const& view){
        bool hdr=flag(view,L"hdr")&&layoutHdr;
        intensityArc->present(view,panelSize,hdr,!transparentSlot());
        if(!hdr)return;
        double stops=num(view,L"intensity");
        auto ramps=array(view,L"intensity_ramp");auto markerColor=array(view,L"marker_color");
        auto next=to_hstring(stops)+ramps.Stringify()+markerColor.Stringify()+to_hstring(panelSize)+data->theme();
        if(next==arcKey)return;arcKey=next;
        wchar_t text[32];swprintf(text,32,L"%s%.2f EV",stops>=0?L"+":L"",stops);
        double font=num(object(layout,L"intensity_caption"),L"font_size");
        if(measuredFont!=font){advances.clear();measuredFont=font;metrics.FontSize(GlyphRasterizer::fontSize(font));}
        double digit=0;for(wchar_t c=L'0';c<=L'9';c++)digit=std::max(digit,advance(c));
        A cells;for(auto c:std::wstring_view(text))cells.Append(N((c==L' '||(c>=L'0'&&c<=L'9'))?digit:advance(c)));
        auto placements=colorUi(data->localization.get(),O({{L"type",S(L"intensity_caption")},{L"size",N(panelSize)},{L"advances",cells}})).GetArray();
        static thread_local GlyphRasterizer rasterizer;
        auto ink=data->brush(L"text").as<SolidColorBrush>().Color();size_t index=0;
        for(auto c:std::wstring_view(text)){
            if(index==captionGlyphs.size()){
                Glyph glyph;glyph.image.IsHitTestVisible(false);glyph.image.Stretch(Stretch::Fill);
                AutomationProperties::SetAccessibilityView(glyph.image,Automation::Peers::AccessibilityView::Raw);
                arc.Children().Append(glyph.image);captionGlyphs.push_back(glyph);
            }
            auto& glyph=captionGlyphs[index];auto glyphPoint=placements.GetArrayAt(uint32_t(index++));
            double angle=glyphPoint.GetNumberAt(2)*3.141592653589793/180,cs=std::cos(angle),sn=std::sin(angle),glyphX=-advance(c)*.5;
            Matrix position{cs,sn,-sn,cs,glyphPoint.GetNumberAt(0)+cs*glyphX,glyphPoint.GetNumberAt(1)+sn*glyphX};
            glyph.image.Visibility(c==L' '?Visibility::Collapsed:Visibility::Visible);
            if(c!=L' ')rasterizer.draw(glyph.image,std::wstring_view(&c,1),font,false,position,layoutScale,ink,.8f);
        }
        for(;index<captionGlyphs.size();index++)captionGlyphs[index].image.Visibility(Visibility::Collapsed);
    }
    void refresh(){
        auto view=model();if(!view.Size()||!root.XamlRoot())return;
        auto nextContext=editingContext();if(context!=nextContext){cancel();context=nextContext;}
        if(arcCapture&&arcCapture->Active()&&arcContext!=intensityContext())cancelArc();
        double scale=root.XamlRoot().RasterizationScale();
        bool hdr=flag(view,L"hdr");
        double available=std::floor(root.ActualWidth()*scale)/scale,height=fitHeight?root.ActualHeight():0;
        if(available<128)return;
        if(std::abs(frameWidth-available)>.01||std::abs(frameHeight-height)>.01||layoutScale!=scale||layoutHdr!=hdr){
            frameWidth=available;frameHeight=height;layoutScale=scale;layoutHdr=hdr;
            root.MinHeight(std::ceil(num(geometry(data,128,hdr),L"height")));
            double size=available;auto next=geometry(data,size,hdr);
            if(fitHeight&&num(next,L"height")>height){
                if(!hdr)size=std::floor(std::min(available,height)*scale)/scale;
                else{
                    int low=128,high=int(available);
                    while(low<high){int middle=(low+high+1)/2;if(num(geometry(data,middle,true),L"height")<=height)low=middle;else high=middle-1;}
                    size=low;
                }
                if(size<128)return;
                next=geometry(data,size,hdr);
            }
            cancel();cancelArc();panelSize=size;layout=next;arcKey=L"";
            double stageHeight=num(layout,L"height",size);
            if(!fitHeight)root.Height(stageHeight);stage.Width(panelSize);stage.Height(stageHeight);
            arc.Width(panelSize);arc.Height(stageHeight);
            auto box=array(layout,L"wheel");side=box.GetNumberAt(2);placeBox(wheel,box);
            // Browser canvas paint snaps the two layout edges to logical pixels.
            // Keep shared hit geometry while matching its final image placement.
            double inset=box.GetNumberAt(0),paintInset=std::round(inset),paintSide=std::round(inset+side)-paintInset;
            image.Width(paintSide);image.Height(paintSide);Canvas::SetLeft(image,paintInset-inset);Canvas::SetTop(image,paintInset-inset);
            drawError.Width(side);Canvas::SetTop(drawError,side*.4);
            placeBox(readout,array(layout,L"readout"));placeBox(swap,array(layout,L"swap"));placeBox(edit,array(layout,L"edit"));
            for(int i=0;i<2;i++)placeBox(shapes[i],array(layout,L"shapes").GetArrayAt(i));
            const std::array<wchar_t const*,3> slots{L"background",L"foreground",L"transparent"};
            for(int i=0;i<3;i++){
                auto swatchBox=array(layout,slots[i]);placeBox(swatches[i],swatchBox);
            }
        }
        auto icons=data->theme()+data->language()+array(view,L"other_shapes").Stringify();
        if(icons!=iconKey){
            iconKey=icons;swapGlyph.Source(icon(L"color-swap",data->theme()).Source());
            for(int i=0;i<2;i++){
                auto shape=array(view,L"other_shapes").GetStringAt(i);Grid content;Shapes::Ellipse hit;hit.Fill(clear());content.Children().Append(hit);
                auto glyph=colorIcon(shape,shapeHovered[i]?accent(data):data->brush(L"text"));
                RotateTransform rotation;rotation.CenterX(8);rotation.CenterY(8);rotation.Angle(array(layout,L"shape_rotations").GetNumberAt(i));
                for(auto child:glyph.Children())child.as<Shapes::Path>().Data().Transform(rotation);
                content.Children().Append(glyph);shapes[i].Content(content);shapeGlyphs[i]=glyph;
                auto title=data->caption(O({{L"type",S(L"color_shape")},{L"shape",S(shape)}}));
                AutomationProperties::SetName(shapes[i],title);tooltip(shapes[i],title);
            }
        }
        for(int i=0;i<2;i++)for(auto child:shapeGlyphs[i].Children())child.as<Shapes::Path>().Stroke(shapeHovered[i]?accent(data):data->brush(L"text"));
        swapFill.Fill(buttonFeedbackBrush(swap,data->brush(L"text"),31,41));
        bool editable=!transparentSlot();edit.IsEnabled(editable);edit.Opacity(editable?1.:.36);
        editFill.Fill(buttonFeedbackBrush(edit,data->brush(L"text"),31,41));
        updateArc(view);
        const std::array<hstring,3> slots{L"background",L"foreground",L"transparent"};
        auto front=str(view,L"front_swatch");
        for(int i=0;i<3;i++){
            auto swatch=find(array(view,L"swatches"),L"slot",slots[i]);
            int z=slots[i]==front?1:0;
            if(Canvas::GetZIndex(swatches[i])!=z)Canvas::SetZIndex(swatches[i],z);
            bool selected=flag(swatch,L"selected");
            swatchFaces[i].feedback.Fill(buttonFeedbackBrush(swatches[i],data->brush(L"text")));
            swatchFaces[i].Update(data,array(swatch,L"rgba"),array(layout,slots[i].c_str()).GetNumberAt(2),scale,selected,i==1,hovered[i]);
            AutomationProperties::SetName(swatches[i],str(swatch,L"label"));AutomationProperties::SetItemStatus(swatches[i],selected?data->caption(L"search",L"selected"):L"");
        }
        auto nextKey=view.Stringify()+to_hstring(side)+L"/"+to_hstring(scale);
        if(nextKey!=key){
            try{
                auto colors=paintColors();
                drawing.present(image,view,colors,side,scale);
                data->colorFieldPixels=drawing.fieldPixels;
                key=nextKey;drawError.Visibility(Visibility::Collapsed);AutomationProperties::SetItemStatus(image,data->caption(L"header",L"ready"));
            }catch(hresult_error const&){drawError.Visibility(Visibility::Visible);AutomationProperties::SetItemStatus(image,data->caption(L"color",L"wheel_failed"));}
        }
        updateReadout(view);
    }
};

}
double ColorPanelNaturalHeight(std::shared_ptr<WorkspaceData> const& data,double width,double scale){
    auto size=std::max(128.,std::floor(width*scale)/scale);
    return num(View::geometry(data,size,flag(object(data->model,L"color_panel"),L"hdr")),L"height",size);
}
FrameworkElement ColorPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,bool fitHeight){
    auto view=std::make_shared<View>(data,fitHeight);view->init();bindings.emplace_back([view]{view->refresh();});return view->root;
}
