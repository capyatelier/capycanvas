#include "pch.h"
#include "ColorLibraryView.h"
#include "ColorView.h"
#include "NativeMenus.h"
#include <d2d1_3.h>
#include <d3d11.h>
#include <dwrite_2.h>
#include <microsoft.ui.xaml.media.dxinterop.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Input.h>
#include <array>
#include <map>
#include <robuffer.h>
#include <winrt/Windows.Storage.Streams.h>
#include <optional>
#include <fstream>
#include <mutex>
#include <thread>
#include <iterator>

using namespace CapyUi;
using namespace winrt::Windows::Foundation;
namespace {
using Point2=D2D1_POINT_2F;
using Paint=D2D1_COLOR_F;
Paint paint(A const& value){
    return {float(value.GetNumberAt(0)),float(value.GetNumberAt(1)),float(value.GetNumberAt(2)),
        value.Size()>3?float(value.GetNumberAt(3)):1.f};
}
winrt::Windows::UI::Color rgba(A const& value){
    auto p=paint(value);return {uint8_t(std::round(p.a*255)),uint8_t(std::round(p.r*255)),
        uint8_t(std::round(p.g*255)),uint8_t(std::round(p.b*255))};
}
Point2 point(A const& value){return {float(value.GetNumberAt(0)),float(value.GetNumberAt(1))};}

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

struct Device {
    com_ptr<ID3D11Device> d3d;
    com_ptr<ID2D1Device2> d2d;
    Device(){
        check_hresult(D3D11CreateDevice(nullptr,D3D_DRIVER_TYPE_HARDWARE,nullptr,D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr,0,D3D11_SDK_VERSION,d3d.put(),nullptr,nullptr));
        com_ptr<ID2D1Factory3> factory;
        D2D1_FACTORY_OPTIONS options{};
        check_hresult(D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED,__uuidof(ID2D1Factory3),&options,factory.put_void()));
        check_hresult(factory->CreateDevice(d3d.as<IDXGIDevice>().get(),d2d.put()));
    }
    static std::shared_ptr<Device> get(){
        static thread_local std::weak_ptr<Device> cached;
        auto device=cached.lock();
        if(!device||FAILED(device->d3d->GetDeviceRemovedReason())){device=std::make_shared<Device>();cached=device;}
        return device;
    }
};
struct FieldRequest {hstring key;uint32_t pixels=0;float hue=0;uint32_t projection=0,rgbSpace=0;bool hdr=false;std::string mapped;uint64_t epoch=0;};
struct FieldResult {hstring key;uint32_t pixels=0;uint64_t epoch=0;std::vector<uint8_t> bytes;};
bool rasterField(FieldRequest const& request,std::vector<uint8_t>& bytes){
    bytes.resize(size_t(request.pixels)*request.pixels*4);
    return request.hdr?capy_color_mapped_field(request.pixels,request.mapped.c_str(),bytes.data(),bytes.size())
        :capy_color_raster(request.pixels,request.hue,request.projection,request.rgbSpace,false,bytes.data(),bytes.size());
}
struct FieldWorker : std::enable_shared_from_this<FieldWorker> {
    Microsoft::UI::Dispatching::DispatcherQueue queue{Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread()};
    std::function<void(FieldResult)> deliver;
    std::mutex mutex;std::optional<FieldRequest> pending;bool running=false;hstring active;uint64_t activeEpoch=0;
    void submit(FieldRequest request){
        std::lock_guard lock(mutex);
        if(running&&active==request.key&&activeEpoch==request.epoch){pending.reset();return;}
        pending=std::move(request);
        if(!running){running=true;std::thread([self=shared_from_this()]{self->run();}).detach();}
    }
    void cancel(){std::lock_guard lock(mutex);pending.reset();}
    void run(){
        for(;;){
            FieldRequest request;
            {std::lock_guard lock(mutex);if(!pending){running=false;active=L"";return;}
                request=std::move(*pending);pending.reset();active=request.key;activeEpoch=request.epoch;}
            auto result=std::make_shared<FieldResult>(FieldResult{request.key,request.pixels,request.epoch,{}});
            if(!rasterField(request,result->bytes))continue;
            queue.TryEnqueue([weak=weak_from_this(),result]{if(auto self=weak.lock();self&&self->deliver)self->deliver(std::move(*result));});
        }
    }
};
// Retain the static hue brush and shared field bitmap independently.
// Marker motion redraws their image without rerasterizing the color field.
struct WheelImage {
    std::shared_ptr<Device> device;
    Imaging::SurfaceImageSource surface{nullptr};
    int pixels=0;
    com_ptr<ID2D1ImageBrush> ring;
    com_ptr<ID2D1Bitmap> field;
    hstring ringShape,fieldKey,fieldLayout;
    std::shared_ptr<FieldWorker> worker;
    std::optional<FieldResult> ready;
    uint64_t epoch=0,synchronous=0;bool previewed=false;
    void install(ID2D1DeviceContext2* context,std::vector<uint8_t> const& bytes,uint32_t side,hstring const& key){
        field=nullptr;check_hresult(context->CreateBitmap(D2D1::SizeU(side,side),bytes.data(),side*4,
            D2D1::BitmapProperties(D2D1::PixelFormat(DXGI_FORMAT_R8G8B8A8_UNORM,D2D1_ALPHA_MODE_PREMULTIPLIED)),field.put()));
        fieldKey=key;
    }
    void draw(Image const& image,J const& model,J const& state,double size,double scale,bool previewing,bool reset=false){
        int next=std::max(1,int(std::ceil(size*scale)));
        if(reset){surface=nullptr;device.reset();}
        if(!surface||pixels!=next||!device||FAILED(device->d3d->GetDeviceRemovedReason())){
            device=Device::get();pixels=next;ring=nullptr;field=nullptr;
            surface=Imaging::SurfaceImageSource(pixels,pixels,false);
            check_hresult(surface.as<ISurfaceImageSourceNativeWithD2D>()->SetDevice(device->d2d.get()));
            image.Source(surface);
        }
        auto native=surface.as<ISurfaceImageSourceNativeWithD2D>();
        com_ptr<ID2D1DeviceContext> base;
        POINT offset{};
        check_hresult(native->BeginDraw(RECT{0,0,pixels,pixels},__uuidof(ID2D1DeviceContext),base.put_void(),&offset));
        bool active=true;
        try{
            auto context=base.as<ID2D1DeviceContext2>();
            context->SetDpi(96,96);
            context->SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            context->SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
            context->SetTransform(D2D1::Matrix3x2F::Scale(float(pixels),float(pixels))*
                D2D1::Matrix3x2F::Translation(float(offset.x),float(offset.y)));
            context->Clear(D2D1::ColorF(0,0,0,0));
            auto geometry=object(model,L"geometry");
            auto center=point(array(geometry,L"center"));
            float inner=float(num(geometry,L"inner")),outer=float(num(geometry,L"outer"));
            auto shape=str(model,L"shape");uint32_t projection=shape==L"circle"?2:shape==L"triangle"?1:0;
            auto working=str(model,L"rgb_space",L"Srgb");uint32_t rgbSpace=working==L"DisplayP3"?1:working==L"AdobeRgb"?2:working==L"ProPhoto"?3:0;
            auto ringKey=shape+L"/"+working+L"/"+to_hstring(pixels);
            if(!ring||ringShape!=ringKey){
                std::vector<uint8_t> bytes(size_t(pixels)*pixels*4);
                if(!capy_color_raster(pixels,0,projection,rgbSpace,true,bytes.data(),bytes.size()))throw hresult_invalid_argument(L"Invalid shared hue guide");
                com_ptr<ID2D1Bitmap> bitmap;check_hresult(context->CreateBitmap(D2D1::SizeU(pixels,pixels),bytes.data(),pixels*4,
                    D2D1::BitmapProperties(D2D1::PixelFormat(DXGI_FORMAT_R8G8B8A8_UNORM,D2D1_ALPHA_MODE_PREMULTIPLIED)),bitmap.put()));
                ring=nullptr;check_hresult(context->CreateImageBrush(bitmap.get(),D2D1::ImageBrushProperties(D2D1::RectF(0,0,float(pixels),float(pixels))),
                    D2D1::BrushProperties(1,D2D1::Matrix3x2F::Scale(1.f/pixels,1.f/pixels)),ring.put()));ringShape=ringKey;
            }
            Paint white{1,1,1,1};
            uint32_t fieldPixels=uint32_t(pixels);float hueValue=float(array(model,L"wheel_components").GetNumberAt(0));
            bool hdr=flag(model,L"hdr");auto rendition=object(model,L"rendition").Stringify();
            auto layoutKey=ringKey+(hdr?L"/hdr"+rendition:hstring{});
            if(layoutKey!=fieldLayout||previewed!=previewing){
                if(layoutKey!=fieldLayout)field=nullptr;
                fieldLayout=layoutKey;previewed=previewing;++epoch;ready.reset();if(worker)worker->cancel();
            }
            if(ready&&ready->epoch==epoch&&ready->pixels==fieldPixels)install(context.get(),ready->bytes,fieldPixels,ready->key);
            ready.reset();
            auto wanted=ringKey+L"/"+to_hstring(hueValue);if(hdr)wanted=wanted+state.Stringify()+rendition;
            if(!field||fieldKey!=wanted){
                FieldRequest request{wanted,fieldPixels,hueValue,projection,rgbSpace,hdr,
                    hdr?to_string(O({{L"state",state},{L"rendition",object(model,L"rendition")}}).Stringify()):std::string{},epoch};
                if(previewing&&field&&worker)worker->submit(std::move(request));
                else{
                    std::vector<uint8_t> bytes;
                    if(!rasterField(request,bytes))throw hresult_invalid_argument(L"Invalid shared color field");
                    install(context.get(),bytes,fieldPixels,wanted);++synchronous;
                }
            }
            com_ptr<ID2D1Factory> factory;context->GetFactory(factory.put());com_ptr<ID2D1Geometry> clip;
            if(projection==0){auto square=array(geometry,L"square");float x=float(square.GetNumberAt(0)),y=float(square.GetNumberAt(1)),w=float(square.GetNumberAt(2));
                float radius=float(std::min(6.,size*.02)/size);com_ptr<ID2D1RoundedRectangleGeometry> rounded;
                check_hresult(factory->CreateRoundedRectangleGeometry(D2D1::RoundedRect(D2D1::RectF(x,y,x+w,y+w),radius,radius),rounded.put()));clip=rounded;
            }else if(projection==2){com_ptr<ID2D1EllipseGeometry> ellipse;float radius=float(num(geometry,L"disc_radius"));
                check_hresult(factory->CreateEllipseGeometry(D2D1::Ellipse(center,radius,radius),ellipse.put()));clip=ellipse;}
            if(clip)context->PushLayer(D2D1::LayerParameters(D2D1::InfiniteRect(),clip.get()),nullptr);
            context->DrawBitmap(field.get(),D2D1::RectF(0,0,1,1),1,D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
            if(clip)context->PopLayer();
            // Stroke one ellipse, as in the reference canvas, to keep both rims consistent.
            context->DrawEllipse(D2D1::Ellipse(center,(inner+outer)/2,(inner+outer)/2),ring.get(),outer-inner);
            float radius=float(std::clamp(size*.04,6.,10.)/size);
            com_ptr<ID2D1SolidColorBrush> brush;check_hresult(context->CreateSolidColorBrush(white,brush.put()));
            for(auto const& entry:{std::pair{L"wheel_hue_marker",L"wheel_hue_color"},std::pair{L"wheel_marker",L"marker_color"}}){
                auto p=point(array(model,entry.first));auto ellipse=D2D1::Ellipse(p,radius,radius);
                brush->SetColor(paint(array(model,entry.second)));context->FillEllipse(ellipse,brush.get());
                brush->SetColor(Paint{0,0,0,.5f});context->DrawEllipse(ellipse,brush.get(),float(4/size));
                brush->SetColor(white);context->DrawEllipse(ellipse,brush.get(),float(2/size));
            }
            active=false;check_hresult(native->EndDraw());
        }catch(...){if(active)native->EndDraw();throw;}
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
    std::array<Shapes::Ellipse,3> swatchEdges,swatchChecks,swatchPaint;
    std::array<bool,3> hovered{};
    std::array<Button,2> quick;
    std::array<Shapes::Ellipse,2> quickEdges,quickPaint;
    std::array<bool,2> quickHovered{};
    Button edit;Shapes::Ellipse editFill;bool editHovered=false;
    Canvas arc;Shapes::Path arcTrack;std::vector<Shapes::Line> ramp;Shapes::Ellipse markerShadow,marker;TextBlock caption;
    Slider intensity;bool syncingIntensity=false;
    std::optional<uint32_t> arcPointer;double arcOriginal=0;hstring arcKey;
    bool layoutHdr=false;double frameWidth=0,frameHeight=0;
    uint64_t listener=0;
    std::array<Button,2> shapes;
    std::array<Canvas,2> shapeGlyphs{nullptr,nullptr};
    std::array<bool,2> shapeHovered{};
    Shapes::Ellipse swapFill;Image swapGlyph;bool swapHovered=false;
    Button swap,readout;
    Shapes::Path readoutHit;
    TextBlock labelMetrics;
    Image labelImage;hstring labelKey;
    Shapes::Path readoutFocus;
    struct Glyph {Image image;MatrixTransform transform;wchar_t scalar=0;std::array<double,9> key{};};
    std::vector<Glyph> glyphs;
    std::array<Border,3> chips;
    std::array<MatrixTransform,3> chipTransforms;
    TextBlock metrics;
    std::map<wchar_t,double> advances;
    double measuredFont=0;
    WheelImage drawing;
    hstring context,key,readoutKey,iconKey;
    std::optional<uint32_t> pointer;
    uint32_t part=0;
    double side=0,panelSize=0,layoutScale=0;
    J layout;
    XamlRoot::Changed_revoker scaleChanged;
    CompositionTarget::SurfaceContentsLost_revoker contentsLost;
    explicit View(std::shared_ptr<WorkspaceData> source,bool fit):data(std::move(source)),fitHeight(fit){}
    ~View(){
        data->colorViews.erase(listener);
        if(drawing.worker){drawing.worker->deliver=nullptr;drawing.worker->cancel();}
    }
    std::shared_ptr<ColorLibraryView> libraryView;
    Flyout colorFlyout;
    void editColor(FrameworkElement const& anchor){
        libraryView=std::make_shared<ColorLibraryView>();libraryView->data=data;libraryView->init();
        ScrollViewer scroll;scroll.Content(libraryView->root);scroll.MaxHeight(std::max(200.,root.XamlRoot().Size().Height-100.));
        colorFlyout.Content(scroll);colorFlyout.ShowAt(anchor);
    }
    bool previewing()const{return WorkspaceData::previewing(data->colorPreview);}
    J model()const{return previewing()?object(data->colorPreview,L"view"):object(data->model,L"color_panel");}
    J paintColors()const{return previewing()?object(data->colorPreview,L"colors"):displayColors(data->state);}
    static J geometry(double size,bool hdr){return colorUi(O({{L"type",S(L"layout")},{L"size",N(size)},{L"hdr",B(hdr)}})).GetObject();}
    J arcAt(double fraction)const{return colorUi(O({{L"type",S(L"arc")},{L"size",N(panelSize)},{L"fraction",N(fraction)}})).GetObject();}
    void pickIntensity(Point position){
        A point;point.Append(N(position.X));point.Append(N(position.Y));
        auto hit=colorUi(O({{L"type",S(L"arc")},{L"size",N(panelSize)},{L"point",point}})).GetObject();
        auto fraction=hit.GetNamedValue(L"fraction",JsonValue::CreateNullValue());
        if(fraction.ValueType()==JsonValueType::Number)setIntensity(-2+8*fraction.GetNumber());
    }
    void setIntensity(double stops){send(O({{L"op",S(L"hdr_intensity")},{L"stops",N(std::clamp(stops,-2.,6.))}}));}
    bool transparentSlot()const{return str(displayColors(data->state),L"slot")==L"transparent";}
    void endArc(bool restore){
        if(!arcPointer)return;arcPointer.reset();arcTrack.ReleasePointerCaptures();
        if(restore)setIntensity(arcOriginal);
    }
    hstring editingContext()const{return str(model(),L"shape")+L"/"+str(displayColors(data->state),L"paint_slot");}
    void send(J const& action){data->dispatch(O({{L"type",S(L"color")},{L"action",action}}));}
    void cancel(){pointer.reset();part=0;root.ReleasePointerCaptures();AutomationProperties::SetItemStatus(root,L"Ready");}
    void pick(Point position){
        if(!pointer||context!=editingContext()||side<1)return;
        A location;location.Append(N(position.X));location.Append(N(position.Y));
        send(O({{L"op",S(L"pick_wheel")},{L"part",S(part==1?L"hue":L"field")},{L"point",location},{L"size",N(side)}}));
    }
    static void placeBox(FrameworkElement const& element,A const& box){
        Canvas::SetLeft(element,box.GetNumberAt(0));Canvas::SetTop(element,box.GetNumberAt(1));
        element.Width(box.GetNumberAt(2));element.Height(box.GetNumberAt(3));
    }
    Button control(hstring const& name,std::function<void()> action){
        auto result=button(data,name,std::move(action));result.Background(nullptr);result.UseSystemFocusVisuals(false);
        for(auto resource:{L"ButtonBackground",L"ButtonBackgroundPointerOver",L"ButtonBackgroundPressed",L"ButtonBackgroundDisabled"})
            result.Resources().Insert(box_value(resource),nullptr);
        result.HorizontalContentAlignment(HorizontalAlignment::Stretch);result.VerticalContentAlignment(VerticalAlignment::Stretch);
        return result;
    }
    static Imaging::WriteableBitmap checker(double logical,double scale,winrt::Windows::UI::Color light,winrt::Windows::UI::Color dark){
        int size=int(std::ceil(logical*scale));
        Imaging::WriteableBitmap result(size,size);uint8_t* bytes=nullptr;
        check_hresult(result.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&bytes));
        for(int y=0;y<size;y++)for(int x=0;x<size;x++){
            // Match the shared repeating conic gradient, including its quadrant boundaries.
            double dx=std::fmod((x+.5)/scale,10.)-5,dy=std::fmod((y+.5)/scale,10.)-5;
            auto value=dx==0||dx*dy<0?dark:light;auto p=bytes+(y*size+x)*4;
            p[0]=value.B;p[1]=value.G;p[2]=value.R;p[3]=255;
        }
        result.Invalidate();return result;
    }
    void init(){
        auto weak=weak_from_this();
        root.UseLayoutRounding(false);root.Background(clear());root.MinWidth(128);root.MinHeight(128);AutomationProperties::SetName(root,L"Color controls");AutomationProperties::SetAutomationId(root,L"color-controls");AutomationProperties::SetItemStatus(root,L"Ready");
        root.Children().Append(stage);stage.HorizontalAlignment(HorizontalAlignment::Center);stage.VerticalAlignment(VerticalAlignment::Center);
        AutomationProperties::SetAutomationId(stage,L"color-panel");AutomationProperties::SetName(stage,L"Color picker");
        wheel.Background(clear());AutomationProperties::SetName(image,L"Color wheel");
        AutomationProperties::SetAutomationId(image,L"color-wheel");
        image.IsHitTestVisible(false);image.Stretch(Stretch::Fill);wheel.Children().Append(image);stage.Children().Append(wheel);
        drawError.Text(L"Color wheel unavailable");drawError.TextWrapping(TextWrapping::Wrap);drawError.FontSize(12);
        drawError.Foreground(data->brush(L"text"));drawError.IsHitTestVisible(false);drawError.Visibility(Visibility::Collapsed);wheel.Children().Append(drawError);
        root.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
        // The host owns capture; shared geometry limits it to wheel contacts.
        // Native buttons consume their own presses before this bubbles here.
        root.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->wheel);
            if(self->pointer||!p.IsInContact()||(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed()))return;
            auto at=p.Position();auto shape=str(self->model(),L"shape");
            auto part=capy_color_hit(at.X,at.Y,float(self->side),shape==L"circle"?2:shape==L"triangle"?1:0);
            if(part&&self->root.CapturePointer(e.Pointer())){self->pointer=p.PointerId();self->part=part;AutomationProperties::SetItemStatus(self->root,L"Picking");self->pick(at);e.Handled(true);}
        }});
        root.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(self->pointer==e.Pointer().PointerId()){self->pick(e.GetCurrentPoint(self->wheel).Position());e.Handled(true);}
        }});
        root.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(self->pointer==e.Pointer().PointerId()){self->cancel();e.Handled(true);}
        }});
        root.PointerCanceled([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())if(self->pointer==e.Pointer().PointerId())self->cancel();});
        root.PointerCaptureLost([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())if(self->pointer==e.Pointer().PointerId()){self->pointer.reset();self->part=0;AutomationProperties::SetItemStatus(self->root,L"Ready");}});
        root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock()){
            self->scaleChanged=self->root.XamlRoot().Changed(auto_revoke,[weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
            self->refresh();
        }});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->cancel();self->scaleChanged.revoke();}});
        contentsLost=CompositionTarget::SurfaceContentsLost(auto_revoke,[weak](auto&&,auto&&){if(auto self=weak.lock()){self->key=L"";self->drawing.surface=nullptr;self->refresh();}});
        listener=data->colorView([weak]{if(auto self=weak.lock())self->refresh();});
        drawing.worker=std::make_shared<FieldWorker>();
        drawing.worker->deliver=[weak](FieldResult result){if(auto self=weak.lock()){self->drawing.ready=std::move(result);self->key=L"";self->refresh();}};
        edit=control(L"Edit Color",[weak]{if(auto self=weak.lock();self&&!self->transparentSlot())self->editColor(self->edit);});
        Grid editContent;auto pencil=icon(L"pencil",data->theme());pencil.HorizontalAlignment(HorizontalAlignment::Center);pencil.VerticalAlignment(VerticalAlignment::Center);
        editContent.Children().Append(editFill);editContent.Children().Append(pencil);edit.Content(editContent);
        ToolTipService::SetToolTip(edit,box_value(L"Edit Color\u2026"));AutomationProperties::SetAutomationId(edit,L"color-edit");
        edit.PointerEntered([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){self->editHovered=e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse;self->refresh();}});
        edit.PointerExited([weak](auto&&,auto&&){if(auto self=weak.lock()){self->editHovered=false;self->refresh();}});
        stage.Children().Append(edit);
        for(int i=0;i<2;i++){
            bool white=i==0;
            quick[i]=control(white?L"Paint with white":L"Paint with black",[weak,white]{if(auto self=weak.lock())self->send(O({{L"op",S(L"quick_color")},{L"white",B(white)}}));});
            Grid sample;sample.Background(nullptr);sample.Children().Append(quickEdges[i]);sample.Children().Append(quickPaint[i]);
            quickPaint[i].Margin({1,1,1,1});quick[i].Content(sample);stage.Children().Append(quick[i]);
            AutomationProperties::SetAutomationId(quick[i],white?L"color-white":L"color-black");
            quick[i].PointerEntered([weak,i](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
                self->quickHovered[i]=e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse;self->refresh();
            }});
            quick[i].PointerExited([weak,i](auto&&,auto&&){if(auto self=weak.lock()){self->quickHovered[i]=false;self->refresh();}});
        }
        // Background is inserted first so the larger foreground owns their overlap.
        const std::array<hstring,3> slots{L"background",L"foreground",L"transparent"};
        for(int i=0;i<3;i++){
            auto slot=slots[i];auto pick=control(slot,[weak,slot]{if(auto self=weak.lock())self->send(O({{L"op",S(L"select")},{L"slot",S(slot)}}));});
            if(i<2)pick.DoubleTapped([weak,slot](auto&&,auto&&){if(auto self=weak.lock()){
                self->send(O({{L"op",S(L"select")},{L"slot",S(slot)}}));self->editColor(self->edit);
            }});
            Grid sample;sample.Background(nullptr);
            sample.Children().Append(swatchEdges[i]);sample.Children().Append(swatchChecks[i]);sample.Children().Append(swatchPaint[i]);
            pick.Content(sample);swatches[i]=pick;stage.Children().Append(pick);
            if(i<2){
                MenuFlyout menu;MenuFlyoutItem item;item.Text(L"Swap foreground and background");AutomationProperties::SetAutomationId(item,L"color-swatch-swap");
                item.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->send(O({{L"op",S(L"swap")}}));});
                menu.Items().Append(item);TrackPopup(menu,data);pick.ContextFlyout(menu);
            }
            AutomationProperties::SetAutomationId(pick,L"color-"+slot);
            pick.PointerEntered([weak,i](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
                self->hovered[i]=e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse;self->refresh();
            }});
            pick.PointerExited([weak,i](auto&&,auto&&){if(auto self=weak.lock()){self->hovered[i]=false;self->refresh();}});
            pick.GotFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
            pick.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
        }
        for(int i=0;i<2;i++){
            shapes[i]=control(L"Color shape",[weak,i]{if(auto self=weak.lock())self->send(O({{L"op",S(L"shape")},{L"shape",array(self->model(),L"other_shapes").GetAt(i)}}));});
            stage.Children().Append(shapes[i]);AutomationProperties::SetAutomationId(shapes[i],L"color-shape-"+to_hstring(i));
            shapes[i].PointerEntered([weak,i](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){self->shapeHovered[i]=e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse;self->refresh();}});
            shapes[i].PointerExited([weak,i](auto&&,auto&&){if(auto self=weak.lock()){self->shapeHovered[i]=false;self->refresh();}});
        }
        swap=control(L"Swap foreground and background",[weak]{if(auto self=weak.lock())self->send(O({{L"op",S(L"swap")}}));});
        stage.Children().Append(swap);AutomationProperties::SetAutomationId(swap,L"color-swap");
        Grid swapContent;swapGlyph=icon(L"color-swap",data->theme());swapGlyph.HorizontalAlignment(HorizontalAlignment::Center);swapGlyph.VerticalAlignment(VerticalAlignment::Center);
        swapContent.Children().Append(swapFill);swapContent.Children().Append(swapGlyph);swap.Content(swapContent);
        swap.PointerEntered([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){self->swapHovered=e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse;self->refresh();}});
        swap.PointerExited([weak](auto&&,auto&&){if(auto self=weak.lock()){self->swapHovered=false;self->refresh();}});
        arc.Background(nullptr);arc.Visibility(Visibility::Collapsed);
        arcTrack.Stroke(clear());arcTrack.StrokeStartLineCap(PenLineCap::Round);arcTrack.StrokeEndLineCap(PenLineCap::Round);
        arc.Children().Append(arcTrack);
        markerShadow.IsHitTestVisible(false);marker.IsHitTestVisible(false);
        markerShadow.Stroke(fill({128,0,0,0}));markerShadow.StrokeThickness(4);marker.Stroke(fill({255,255,255,255}));marker.StrokeThickness(2);
        caption.IsHitTestVisible(false);caption.FontFamily(FontFamily(L"Segoe UI"));
        arcTrack.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->stage);
            if(self->arcPointer||self->transparentSlot()||(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed()))return;
            if(!self->arcTrack.CapturePointer(e.Pointer()))return;
            self->arcPointer=p.PointerId();self->arcOriginal=num(self->model(),L"intensity");self->pickIntensity(p.Position());e.Handled(true);
        }});
        arcTrack.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->arcPointer==e.Pointer().PointerId()){
            self->pickIntensity(e.GetCurrentPoint(self->stage).Position());e.Handled(true);
        }});
        arcTrack.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->arcPointer==e.Pointer().PointerId()){
            self->pickIntensity(e.GetCurrentPoint(self->stage).Position());self->endArc(false);e.Handled(true);
        }});
        arcTrack.PointerCanceled([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->arcPointer==e.Pointer().PointerId())self->endArc(true);});
        arcTrack.PointerCaptureLost([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->arcPointer==e.Pointer().PointerId())self->endArc(true);});
        arcTrack.DoubleTapped([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->transparentSlot())self->setIntensity(0);});
        intensity.Minimum(-2);intensity.Maximum(6);intensity.StepFrequency(.01);intensity.SmallChange(.1);intensity.LargeChange(1);
        intensity.Width(1);intensity.Height(1);intensity.Opacity(0);intensity.IsHitTestVisible(false);intensity.Visibility(Visibility::Collapsed);
        AutomationProperties::SetName(intensity,L"Color intensity");AutomationProperties::SetAutomationId(intensity,L"color-intensity");
        intensity.ValueChanged([weak](auto&&,Primitives::RangeBaseValueChangedEventArgs const& e){if(auto self=weak.lock();self&&!self->syncingIntensity&&!self->transparentSlot())self->setIntensity(e.NewValue());});
        intensity.PreviewKeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock();self&&e.Key()==winrt::Windows::System::VirtualKey::Home){self->setIntensity(0);e.Handled(true);}});
        stage.Children().Append(arc);stage.Children().Append(intensity);
        readout=control(L"Switch color readout",[weak]{if(auto self=weak.lock())self->send(O({{L"op",S(L"toggle_readout")}}));});
        MenuFlyout editMenu;MenuFlyoutItem editItem;editItem.Text(L"Edit color and palettes…");
        AutomationProperties::SetAutomationId(editItem,L"edit-color-palettes");editItem.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->editColor(self->readout);});
        editMenu.Items().Append(editItem);TrackPopup(editMenu,data);readout.ContextFlyout(editMenu);
        AutomationProperties::SetAutomationId(readout,L"color-readout");stage.Children().Append(readout);
        readoutHit.Fill(clear());readoutBody.Children().Append(readoutHit);
        labelMetrics.UseLayoutRounding(false);labelMetrics.FontFamily(FontFamily(L"Segoe UI"));labelMetrics.FontWeight(winrt::Windows::UI::Text::FontWeights::Bold());labelMetrics.IsHitTestVisible(false);AutomationProperties::SetAccessibilityView(labelMetrics,Automation::Peers::AccessibilityView::Raw);
        labelImage.IsHitTestVisible(false);labelImage.Stretch(Stretch::Fill);AutomationProperties::SetAccessibilityView(labelImage,Automation::Peers::AccessibilityView::Raw);readoutBody.Children().Append(labelImage);readoutFocus.IsHitTestVisible(false);
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
        auto labelNext=labelMetrics.Text()+L"/"+data->theme()+L"/"+to_hstring(labelFont)+L"/"+to_hstring(rasterScale)+(focused?L"/focus":L"");
        if(labelNext!=labelKey){
            rasterizer.draw(labelImage,std::wstring_view(labelMetrics.Text()),labelFont,true,Matrix{1,0,0,1,2,labelFont+1},rasterScale,ink.as<SolidColorBrush>().Color(),.9f);labelKey=labelNext;
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
        arc.Visibility(hdr?Visibility::Visible:Visibility::Collapsed);intensity.Visibility(hdr?Visibility::Visible:Visibility::Collapsed);
        if(!hdr)return;
        double stops=num(view,L"intensity");
        auto ramps=array(view,L"intensity_ramp");auto markerColor=array(view,L"marker_color");
        auto next=to_hstring(stops)+ramps.Stringify()+markerColor.Stringify()+to_hstring(panelSize)+data->theme();
        if(next==arcKey)return;arcKey=next;
        for(size_t i=0;i<ramp.size()&&i<ramps.Size();i++)ramp[i].Stroke(fill(rgba(ramps.GetArrayAt(uint32_t(i)))));
        auto point=array(arcAt((stops+2)/8),L"point");double x=point.GetNumberAt(0),y=point.GetNumberAt(1);
        for(auto const& dot:{markerShadow,marker}){Canvas::SetLeft(dot,x-dot.Width()/2);Canvas::SetTop(dot,y-dot.Height()/2);}
        marker.Fill(fill(rgba(markerColor)));
        wchar_t text[32];swprintf(text,32,L"%s%.2f EV",stops>=0?L"+":L"",stops);
        caption.Text(text);caption.Foreground(data->brush(L"text"));caption.Measure({1000,1000});
        auto placement=array(layout,L"intensity_caption");
        Canvas::SetLeft(caption,placement.GetNumberAt(0)-caption.DesiredSize().Width/2);
        Canvas::SetTop(caption,placement.GetNumberAt(1)-caption.BaselineOffset());
        syncingIntensity=true;intensity.Value(stops);syncingIntensity=false;
        wchar_t value[32];swprintf(value,32,L"%.1f EV",stops);AutomationProperties::SetItemStatus(intensity,value);
    }
    void refresh(){
        auto view=model();if(!view.Size()||!root.XamlRoot())return;
        if(libraryView&&libraryView->root.IsLoaded())libraryView->refresh();
        auto nextContext=editingContext();if(context!=nextContext){cancel();context=nextContext;}
        double scale=root.XamlRoot().RasterizationScale();
        bool hdr=flag(view,L"hdr");
        double available=std::floor(root.ActualWidth()*scale)/scale,height=fitHeight?root.ActualHeight():0;
        if(available<128)return;
        if(std::abs(frameWidth-available)>.01||std::abs(frameHeight-height)>.01||layoutScale!=scale||layoutHdr!=hdr){
            frameWidth=available;frameHeight=height;layoutScale=scale;layoutHdr=hdr;
            root.MinHeight(std::ceil(num(geometry(128,hdr),L"height")));
            double size=available;auto next=geometry(size,hdr);
            if(fitHeight&&num(next,L"height")>height){
                if(!hdr)size=std::floor(std::min(available,height)*scale)/scale;
                else{
                    int low=128,high=int(available);
                    while(low<high){int middle=(low+high+1)/2;if(num(geometry(middle,true),L"height")<=height)low=middle;else high=middle-1;}
                    size=low;
                }
                if(size<128)return;
                next=geometry(size,hdr);
            }
            cancel();endArc(true);panelSize=size;layout=next;arcKey=L"";
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
            for(int i=0;i<2;i++)placeBox(quick[i],array(layout,i==0?L"white":L"black"));
            if(hdr){
                auto start=arcAt(0),end=arcAt(1);auto shape=object(start,L"geometry");
                double radius=num(shape,L"radius"),width=num(shape,L"width");
                PathFigure figure;auto from=array(start,L"point"),to=array(end,L"point");
                figure.StartPoint({float(from.GetNumberAt(0)),float(from.GetNumberAt(1))});figure.IsClosed(false);figure.IsFilled(false);
                ArcSegment segment;segment.Point({float(to.GetNumberAt(0)),float(to.GetNumberAt(1))});segment.Size({float(radius),float(radius)});
                segment.SweepDirection(SweepDirection::Counterclockwise);figure.Segments().Append(segment);
                PathGeometry track;track.Figures().Append(figure);arcTrack.Data(track);arcTrack.StrokeThickness(width);
                auto path=array(start,L"path");
                for(auto const& line:ramp){uint32_t at;if(arc.Children().IndexOf(line,at))arc.Children().RemoveAt(at);}
                ramp.clear();
                for(uint32_t i=1;i<path.Size();i++){
                    Shapes::Line line;auto a=path.GetArrayAt(i-1),b=path.GetArrayAt(i);
                    line.X1(a.GetNumberAt(0));line.Y1(a.GetNumberAt(1));line.X2(b.GetNumberAt(0));line.Y2(b.GetNumberAt(1));
                    line.StrokeThickness(width);line.StrokeStartLineCap(PenLineCap::Round);line.StrokeEndLineCap(PenLineCap::Round);
                    line.IsHitTestVisible(false);arc.Children().InsertAt(uint32_t(ramp.size()),line);ramp.push_back(line);
                }
                double markerRadius=num(shape,L"marker_radius");
                markerShadow.Width(markerRadius*2+4);markerShadow.Height(markerRadius*2+4);
                marker.Width(markerRadius*2+2);marker.Height(markerRadius*2+2);
                for(auto const& dot:{markerShadow,marker}){uint32_t at;if(!arc.Children().IndexOf(dot,at))arc.Children().Append(dot);}
                uint32_t at;if(!arc.Children().IndexOf(caption,at))arc.Children().Append(caption);
                caption.FontSize(array(layout,L"intensity_caption").GetNumberAt(2));
            }
            for(int i=0;i<2;i++)placeBox(shapes[i],array(layout,L"shapes").GetArrayAt(i));
            const std::array<wchar_t const*,3> slots{L"background",L"foreground",L"transparent"};
            for(int i=0;i<3;i++){
                auto swatchBox=array(layout,slots[i]);placeBox(swatches[i],swatchBox);
                double pad=i==1?3:1;swatchChecks[i].Margin({pad,pad,pad,pad});swatchPaint[i].Margin({pad,pad,pad,pad});
                ImageBrush pixels;pixels.ImageSource(checker(swatchBox.GetNumberAt(2)-2*pad,scale,color(str(object(data->state,L"palette"),L"checker_light")),color(str(object(data->state,L"palette"),L"checker_dark"))));pixels.Stretch(Stretch::Fill);swatchChecks[i].Fill(pixels);
            }
        }
        auto icons=data->theme()+array(view,L"other_shapes").Stringify();
        if(icons!=iconKey){
            iconKey=icons;swapGlyph.Source(icon(L"color-swap",data->theme()).Source());
            for(int i=0;i<2;i++){
                auto shape=array(view,L"other_shapes").GetStringAt(i);Grid content;Shapes::Ellipse hit;hit.Fill(clear());content.Children().Append(hit);
                auto glyph=colorIcon(shape,shapeHovered[i]?accent(data):data->brush(L"text"));
                RotateTransform rotation;rotation.CenterX(8);rotation.CenterY(8);rotation.Angle(array(layout,L"shape_rotations").GetNumberAt(i));
                for(auto child:glyph.Children())child.as<Shapes::Path>().Data().Transform(rotation);
                content.Children().Append(glyph);shapes[i].Content(content);shapeGlyphs[i]=glyph;
                auto title=L"Use "+hstring(shape==L"circle"?L"Okhsv":shape==L"triangle"?L"HLS":L"HSV")+L" "+shape;
                AutomationProperties::SetName(shapes[i],title);ToolTipService::SetToolTip(shapes[i],box_value(title));
            }
        }
        for(int i=0;i<2;i++)for(auto child:shapeGlyphs[i].Children())child.as<Shapes::Path>().Stroke(shapeHovered[i]?accent(data):data->brush(L"text"));
        auto swapInk=color(str(object(data->state,L"palette"),L"text"));swapInk.A=swapHovered?31:0;swapFill.Fill(fill(swapInk));
        bool editable=!transparentSlot();edit.IsEnabled(editable);edit.Opacity(editable?1.:.36);
        auto editInk=color(str(object(data->state,L"palette"),L"text"));editInk.A=editHovered&&editable?31:0;editFill.Fill(fill(editInk));
        auto quickColors=array(view,L"quick_colors");
        for(int i=0;i<2;i++){
            J preset;for(auto value:quickColors)if(flag(value.GetObject(),L"white")==(i==0))preset=value.GetObject();
            bool chosen=flag(preset,L"selected");auto ring=color(str(object(data->state,L"palette"),L"text"));
            if(!(chosen||quickHovered[i]))ring.A=64;
            quickEdges[i].Fill(data->brush(L"panel"));quickEdges[i].Stroke(fill(ring));quickEdges[i].StrokeThickness(chosen||quickHovered[i]?2:1);
            quickPaint[i].Fill(fill(rgba(array(preset,L"rgba"))));
            auto name=str(preset,L"label");AutomationProperties::SetName(quick[i],name);ToolTipService::SetToolTip(quick[i],box_value(name));
            AutomationProperties::SetItemStatus(quick[i],chosen?L"Selected":L"");
        }
        updateArc(view);
        const std::array<hstring,3> slots{L"background",L"foreground",L"transparent"};
        for(int i=0;i<3;i++){
            auto swatch=find(array(view,L"swatches"),L"slot",slots[i]);
            swatchEdges[i].Fill(data->brush(L"panel"));auto stroke=color(str(object(data->state,L"palette"),L"text"));
            bool selected=flag(swatch,L"selected");
            if(!(selected||hovered[i]))stroke.A=64;
            swatchEdges[i].Stroke(fill(stroke));swatchEdges[i].StrokeThickness(selected||hovered[i]?2:1);
            swatchPaint[i].Fill(fill(rgba(array(swatch,L"rgba"))));
            AutomationProperties::SetName(swatches[i],str(swatch,L"label"));AutomationProperties::SetItemStatus(swatches[i],selected?L"Selected":L"");
        }
        bool preview=previewing();
        auto nextKey=view.Stringify()+to_hstring(side)+L"/"+to_hstring(scale)+(preview?L"/preview":L"");
        if(nextKey!=key){
            try{
                auto colors=paintColors();auto rasters=drawing.synchronous;
                struct Count{WheelImage const& drawing;uint64_t before;std::shared_ptr<WorkspaceData> const& data;
                    ~Count(){data->colorFields+=drawing.synchronous-before;}} count{drawing,rasters,data};
                try{drawing.draw(image,view,colors,side,scale,preview);}
                catch(hresult_error const& exception){
                    auto code=exception.code();
                    if(code!=DXGI_ERROR_DEVICE_REMOVED&&code!=DXGI_ERROR_DEVICE_RESET&&code!=D2DERR_RECREATE_TARGET&&code!=E_SURFACE_CONTENTS_LOST)throw;
                    drawing.draw(image,view,colors,side,scale,preview,true);
                }
                key=nextKey;drawError.Visibility(Visibility::Collapsed);AutomationProperties::SetItemStatus(image,L"Ready");
            }catch(hresult_error const&){drawError.Visibility(Visibility::Visible);AutomationProperties::SetItemStatus(image,L"Color wheel could not be drawn");}
        }
        updateReadout(view);
    }
};

}
double ColorPanelNaturalHeight(std::shared_ptr<WorkspaceData> const& data,double width,double scale){
    auto size=std::max(128.,std::floor(width*scale)/scale);
    return num(View::geometry(size,flag(object(data->model,L"color_panel"),L"hdr")),L"height",size);
}
FrameworkElement ColorPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,bool fitHeight){
    auto view=std::make_shared<View>(data,fitHeight);view->init();bindings.emplace_back([view]{view->refresh();});return view->root;
}
