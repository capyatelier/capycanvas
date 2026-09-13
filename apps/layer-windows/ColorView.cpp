#include "pch.h"
#include "ColorView.h"
#include "NativeMenus.h"
#include <d2d1_3.h>
#include <d3d11.h>
#include <dwrite.h>
#include <microsoft.ui.xaml.media.dxinterop.h>
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
using namespace winrt::Windows::Foundation;
namespace {
using Point2=D2D1_POINT_2F;
using Paint=D2D1_COLOR_F;
Paint paint(A const& value){
    return {float(value.GetNumberAt(0)),float(value.GetNumberAt(1)),float(value.GetNumberAt(2)),
        value.Size()>3?float(value.GetNumberAt(3)):1.f};
}
Paint mix(Paint a,Paint b,float t){return {a.r+(b.r-a.r)*t,a.g+(b.g-a.g)*t,a.b+(b.b-a.b)*t,1.f};}
winrt::Windows::UI::Color rgba(A const& value){
    auto p=paint(value);return {uint8_t(std::round(p.a*255)),uint8_t(std::round(p.r*255)),
        uint8_t(std::round(p.g*255)),uint8_t(std::round(p.b*255))};
}
Point2 point(A const& value){return {float(value.GetNumberAt(0)),float(value.GetNumberAt(1))};}
D2D1_GRADIENT_MESH_PATCH patch(std::array<Point2,16> const& p,std::array<Paint,4> const& c){
    return {p[0],p[1],p[2],p[3],p[4],p[5],p[6],p[7],
        p[8],p[9],p[10],p[11],p[12],p[13],p[14],p[15],
        c[0],c[1],c[2],c[3],D2D1_PATCH_EDGE_MODE_ANTIALIASED,D2D1_PATCH_EDGE_MODE_ANTIALIASED,
        D2D1_PATCH_EDGE_MODE_ANTIALIASED,D2D1_PATCH_EDGE_MODE_ANTIALIASED};
}

// Keep glyphs as vectors until their final rotation and position.
// This avoids resampling the raster of each tiny rotated TextBlock.
struct GlyphOutline : winrt::implements<GlyphOutline,ID2D1SimplifiedGeometrySink> {
    PathGeometry geometry;
    PathFigure figure{nullptr};
    HRESULT error=S_OK;
    template<class F> void append(F action) noexcept {
        if(FAILED(error))return;
        try{action();}catch(...){error=to_hresult();}
    }
    void __stdcall SetFillMode(D2D1_FILL_MODE mode) noexcept override {
        append([&]{geometry.FillRule(mode==D2D1_FILL_MODE_WINDING?FillRule::Nonzero:FillRule::EvenOdd);});
    }
    void __stdcall SetSegmentFlags(D2D1_PATH_SEGMENT) noexcept override {}
    void __stdcall BeginFigure(Point2 p,D2D1_FIGURE_BEGIN begin) noexcept override {
        append([&]{figure=PathFigure();figure.StartPoint({p.x,p.y});figure.IsFilled(begin==D2D1_FIGURE_BEGIN_FILLED);geometry.Figures().Append(figure);});
    }
    void __stdcall AddLines(Point2 const* points,UINT count) noexcept override {
        append([&]{for(UINT i=0;i<count;i++){LineSegment line;line.Point({points[i].x,points[i].y});figure.Segments().Append(line);}});
    }
    void __stdcall AddBeziers(D2D1_BEZIER_SEGMENT const* curves,UINT count) noexcept override {
        append([&]{for(UINT i=0;i<count;i++){auto const& c=curves[i];BezierSegment curve;
            curve.Point1({c.point1.x,c.point1.y});curve.Point2({c.point2.x,c.point2.y});curve.Point3({c.point3.x,c.point3.y});figure.Segments().Append(curve);}});
    }
    void __stdcall EndFigure(D2D1_FIGURE_END end) noexcept override {
        append([&]{figure.IsClosed(end==D2D1_FIGURE_END_CLOSED);});
    }
    HRESULT __stdcall Close() noexcept override {return error;}
};
PathGeometry glyphOutline(wchar_t scalar,double size){
    static thread_local auto face=[]{
        com_ptr<IDWriteFactory> factory;
        check_hresult(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED,__uuidof(IDWriteFactory),reinterpret_cast<::IUnknown**>(factory.put())));
        com_ptr<IDWriteFontCollection> fonts;check_hresult(factory->GetSystemFontCollection(fonts.put()));
        UINT index=0;BOOL found=FALSE;check_hresult(fonts->FindFamilyName(L"Segoe UI",&index,&found));check_bool(found);
        com_ptr<IDWriteFontFamily> family;check_hresult(fonts->GetFontFamily(index,family.put()));
        com_ptr<IDWriteFont> font;check_hresult(family->GetFirstMatchingFont(DWRITE_FONT_WEIGHT_NORMAL,DWRITE_FONT_STRETCH_NORMAL,DWRITE_FONT_STYLE_NORMAL,font.put()));
        com_ptr<IDWriteFontFace> result;check_hresult(font->CreateFontFace(result.put()));return result;
    }();
    UINT32 code=scalar;UINT16 glyph=0;check_hresult(face->GetGlyphIndices(&code,1,&glyph));
    auto sink=winrt::make_self<GlyphOutline>();
    check_hresult(face->GetGlyphRunOutline(float(size),&glyph,nullptr,nullptr,1,false,false,sink.get()));
    check_hresult(sink->Close());return sink->geometry;
}

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
// Retain the static hue mesh and shared field bitmap independently.
// Marker motion redraws their image without rerasterizing the color field.
struct WheelImage {
    std::shared_ptr<Device> device;
    Imaging::SurfaceImageSource surface{nullptr};
    int pixels=0;
    com_ptr<ID2D1GradientMesh> ring;
    com_ptr<ID2D1Bitmap> field;
    hstring ringShape,fieldKey;
    void draw(Image const& image,J const& model,double size,double scale,bool reset=false){
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
            context->SetTransform(D2D1::Matrix3x2F::Scale(float(size*scale),float(size*scale))*
                D2D1::Matrix3x2F::Translation(float(offset.x),float(offset.y)));
            context->Clear(D2D1::ColorF(0,0,0,0));
            auto geometry=object(model,L"geometry");
            auto center=point(array(geometry,L"center"));
            float inner=float(num(geometry,L"inner")),outer=float(num(geometry,L"outer"));
            auto shape=str(model,L"shape");uint32_t projection=shape==L"circle"?2:shape==L"triangle"?1:0;
            if(!ring||ringShape!=shape){
                auto raw=capy_color_hue_stops(projection);
                if(!raw)throw hresult_invalid_argument(L"Invalid shared hue guide");
                std::unique_ptr<char,decltype(&capy_string_free)> owned(raw,capy_string_free);
                auto stops=A::Parse(to_hstring(raw));
                std::vector<D2D1_GRADIENT_MESH_PATCH> patches;
                float start=float(num(model,L"wheel_hue_start_degrees")*3.141592653589793/180.);
                for(uint32_t stop=0;stop+1<stops.Size();stop++){
                    auto firstStop=stops.GetObjectAt(stop),lastStop=stops.GetObjectAt(stop+1);
                    float from=float(num(firstStop,L"offset")),to=float(num(lastStop,L"offset"));
                    auto firstColor=paint(array(firstStop,L"color")),lastColor=paint(array(lastStop,L"color"));
                    int segments=std::max(1,int(std::ceil((to-from)*48)));
                    for(int segment=0;segment<segments;segment++){
                        float t=float(segment)/segments,u=float(segment+1)/segments;
                        float a=start+(from+(to-from)*t)*2.f*3.141592653589793f;
                        float b=start+(from+(to-from)*u)*2.f*3.141592653589793f;
                        float tangent=4.f/3.f*std::tan((b-a)/4.f);
                        std::array<Point2,16> p;
                        for(int row=0;row<4;row++){
                            float apron=2.f/pixels;
                            float radius=inner-apron+(outer-inner+2*apron)*row/3.f;
                            Point2 first{center.x+radius*std::cos(a),center.y+radius*std::sin(a)};
                            Point2 last{center.x+radius*std::cos(b),center.y+radius*std::sin(b)};
                            p[row*4]=first;p[row*4+1]={first.x-radius*tangent*std::sin(a),first.y+radius*tangent*std::cos(a)};
                            p[row*4+2]={last.x+radius*tangent*std::sin(b),last.y-radius*tangent*std::cos(b)};p[row*4+3]=last;
                        }
                        auto c0=mix(firstColor,lastColor,t),c1=mix(firstColor,lastColor,u);
                        auto piece=patch(p,{c0,c1,c0,c1});
                        piece.topEdgeMode=piece.bottomEdgeMode=piece.leftEdgeMode=piece.rightEdgeMode=D2D1_PATCH_EDGE_MODE_ALIASED;patches.push_back(piece);
                    }
                }
                ring=nullptr;check_hresult(context->CreateGradientMesh(patches.data(),uint32_t(patches.size()),ring.put()));
                ringShape=shape;
            }
            Paint white{1,1,1,1},black{0,0,0,1},hue=paint(array(model,L"wheel_hue_color"));
            com_ptr<ID2D1Factory> factory;context->GetFactory(factory.put());
            if(projection==0){
                auto s=array(geometry,L"square");float x=float(s.GetNumberAt(0)),y=float(s.GetNumberAt(1)),w=float(s.GetNumberAt(2));
                com_ptr<ID2D1RoundedRectangleGeometry> clip;
                float radius=float(std::min(6.,size*.02)/size);
                check_hresult(factory->CreateRoundedRectangleGeometry(D2D1::RoundedRect(D2D1::RectF(x,y,x+w,y+w),radius,radius),clip.put()));
                context->PushLayer(D2D1::LayerParameters(D2D1::InfiniteRect(),clip.get()),nullptr);
                auto gradient=[&](Point2 from,Point2 to,Paint first,Paint last){
                    D2D1_GRADIENT_STOP stops[]={{0,first},{1,last}};
                    com_ptr<ID2D1GradientStopCollection> collection;
                    check_hresult(context->CreateGradientStopCollection(stops,2,D2D1_GAMMA_2_2,D2D1_EXTEND_MODE_CLAMP,collection.put()));
                    com_ptr<ID2D1LinearGradientBrush> brush;
                    check_hresult(context->CreateLinearGradientBrush(D2D1::LinearGradientBrushProperties(from,to),collection.get(),brush.put()));
                    context->FillRectangle(D2D1::RectF(x,y,x+w,y+w),brush.get());
                };
                gradient({x,y},{x+w,y},white,hue);gradient({x,y},{x,y+w},Paint{0,0,0,0},black);
                context->PopLayer();
            }else{
                uint32_t fieldPixels=uint32_t(projection==2?std::ceil(size):pixels);
                float hueValue=float(array(model,L"wheel_components").GetNumberAt(0));
                auto wanted=shape+L"/"+to_hstring(fieldPixels)+L"/"+to_hstring(hueValue);
                if(!field||fieldKey!=wanted){
                    std::vector<uint8_t> bytes(size_t(fieldPixels)*fieldPixels*4);
                    if(!capy_color_field(fieldPixels,hueValue,projection,bytes.data(),bytes.size()))
                        throw hresult_invalid_argument(L"Invalid shared color field");
                    field=nullptr;check_hresult(context->CreateBitmap(D2D1::SizeU(fieldPixels,fieldPixels),bytes.data(),fieldPixels*4,
                        D2D1::BitmapProperties(D2D1::PixelFormat(DXGI_FORMAT_R8G8B8A8_UNORM,D2D1_ALPHA_MODE_PREMULTIPLIED)),field.put()));
                    fieldKey=wanted;
                }
                if(projection==2){
                    com_ptr<ID2D1EllipseGeometry> clip;
                    float radius=float(num(geometry,L"disc_radius"));
                    check_hresult(factory->CreateEllipseGeometry(D2D1::Ellipse(center,radius,radius),clip.put()));
                    context->PushLayer(D2D1::LayerParameters(D2D1::InfiniteRect(),clip.get()),nullptr);
                }
                context->DrawBitmap(field.get(),D2D1::RectF(0,0,1,1),1,D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
                if(projection==2)context->PopLayer();
            }
            // A single analytic annulus clip gives the mesh a continuous rim.
            com_ptr<ID2D1EllipseGeometry> outside,inside;
            check_hresult(factory->CreateEllipseGeometry(D2D1::Ellipse(center,outer,outer),outside.put()));
            check_hresult(factory->CreateEllipseGeometry(D2D1::Ellipse(center,inner,inner),inside.put()));
            ID2D1Geometry* rims[]={outside.get(),inside.get()};com_ptr<ID2D1GeometryGroup> rim;
            check_hresult(factory->CreateGeometryGroup(D2D1_FILL_MODE_ALTERNATE,rims,2,rim.put()));
            context->PushLayer(D2D1::LayerParameters(D2D1::InfiniteRect(),rim.get()),nullptr);
            context->DrawGradientMesh(ring.get());context->PopLayer();
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
    std::array<Button,2> shapes;
    std::array<Canvas,2> shapeGlyphs{nullptr,nullptr};
    std::array<bool,2> shapeHovered{};
    Shapes::Ellipse swapFill;bool swapHovered=false;
    Button swap,readout;
    Shapes::Path readoutHit;
    TextBlock readoutLabel;
    Shapes::Path readoutFocus;
    struct Glyph {Shapes::Path path;MatrixTransform transform;wchar_t scalar=0;double font=0;};
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
    J model()const{return object(data->model,L"color_panel");}
    hstring editingContext()const{return str(model(),L"shape")+L"/"+str(object(data->state,L"colors"),L"paint_slot");}
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
    static Imaging::WriteableBitmap checker(double logical,double scale){
        int size=int(std::ceil(logical*scale));
        Imaging::WriteableBitmap result(size,size);uint8_t* bytes=nullptr;
        check_hresult(result.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&bytes));
        for(int y=0;y<size;y++)for(int x=0;x<size;x++){
            auto value=uint8_t((int((x+.5)/scale/5)+int((y+.5)/scale/5))%2?204:140);auto p=bytes+(y*size+x)*4;
            p[0]=p[1]=p[2]=value;p[3]=255;
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
        // Background is inserted first so the larger foreground owns their overlap.
        const std::array<hstring,3> slots{L"background",L"foreground",L"transparent"};
        for(int i=0;i<3;i++){
            auto slot=slots[i];auto pick=control(slot,[weak,slot]{if(auto self=weak.lock())self->send(O({{L"op",S(L"select")},{L"slot",S(slot)}}));});
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
        Grid swapContent;swapContent.Children().Append(swapFill);swapContent.Children().Append(colorIcon(L"swap",data->brush(L"text")));swap.Content(swapContent);
        swap.PointerEntered([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){self->swapHovered=e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse;self->refresh();}});
        swap.PointerExited([weak](auto&&,auto&&){if(auto self=weak.lock()){self->swapHovered=false;self->refresh();}});
        readout=control(L"Color readout",[weak]{if(auto self=weak.lock())self->send(O({{L"op",S(L"toggle_readout")}}));});
        AutomationProperties::SetAutomationId(readout,L"color-readout");stage.Children().Append(readout);
        readoutHit.Fill(clear());readoutBody.Children().Append(readoutHit);
        readoutLabel.FontFamily(FontFamily(L"Segoe UI"));readoutLabel.FontWeight(winrt::Windows::UI::Text::FontWeights::Bold());readoutLabel.IsHitTestVisible(false);AutomationProperties::SetAccessibilityView(readoutLabel,Automation::Peers::AccessibilityView::Raw);
        readoutBody.Children().Append(readoutLabel);readoutFocus.IsHitTestVisible(false);
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
        auto next=view.Stringify()+layout.Stringify()+data->theme()+(focused?L"/focus":L"");
        if(next==readoutKey)return;readoutKey=next;
        double half=array(layout,L"readout").GetNumberAt(2),radius=num(layout,L"readout_radius");
        double clipRadius=side*num(object(view,L"geometry"),L"outer")+2;
        PathFigure figure;figure.StartPoint({0,0});figure.IsClosed(true);figure.IsFilled(true);
        auto line=[&](float x,float y){LineSegment segment;segment.Point({x,y});figure.Segments().Append(segment);};
        line(float(half),0);line(float(half),float(half-clipRadius));
        ArcSegment arc;arc.Point({float(half-clipRadius),float(half)});arc.Size({float(clipRadius),float(clipRadius)});
        arc.SweepDirection(SweepDirection::Counterclockwise);figure.Segments().Append(arc);line(0,float(half));
        PathGeometry geometry;geometry.Figures().Append(figure);readoutHit.Data(geometry);
        auto ink=focused?fill(color(L"#3584e4")):data->brush(L"text");
        double labelFont=std::clamp(panelSize*.044,9.,12.);
        readoutLabel.Text(str(view,L"readout_label"));readoutLabel.FontSize(labelFont);readoutLabel.Foreground(ink);readoutLabel.Opacity(.9);
        readoutLabel.Measure({1000,1000});Canvas::SetLeft(readoutLabel,2);Canvas::SetTop(readoutLabel,labelFont+1-readoutLabel.BaselineOffset());
        readoutFocus.Stroke(ink);readoutFocus.Visibility(focused?Visibility::Visible:Visibility::Collapsed);
        float w=readoutLabel.DesiredSize().Width+5,h=float(labelFont+4);
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
            if(measuredFont!=font){advances.clear();measuredFont=font;metrics.FontSize(font);}
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
                    Glyph glyph;glyph.path.IsHitTestVisible(false);AutomationProperties::SetAccessibilityView(glyph.path,Automation::Peers::AccessibilityView::Raw);glyph.path.Opacity(.8);
                    readoutBody.Children().Append(glyph.path);glyphs.push_back(glyph);
                }
                auto& glyph=glyphs[index++];glyph.path.Visibility(c==L' '?Visibility::Collapsed:Visibility::Visible);
                if(glyph.scalar!=c||glyph.font!=font){
                    glyph.scalar=c;glyph.font=font;glyph.path.Data(glyphOutline(c,font));glyph.path.Data().Transform(glyph.transform);
                }
                glyph.path.Fill(ink);
                double cell=(c==L' '||(c>=L'0'&&c<=L'9'))?digit:advance(c);
                at(glyph.transform,half,radius,mid+(along+cell*.5)/radius,-advance(c)*.5,0);along+=cell;
            }
        }
        for(;index<glyphs.size();index++)glyphs[index].path.Visibility(Visibility::Collapsed);
        AutomationProperties::SetName(readout,str(view,L"readout_description"));
    }
    void refresh(){
        auto view=model();if(!view.Size()||!root.XamlRoot())return;
        auto nextContext=editingContext();if(context!=nextContext){cancel();context=nextContext;}
        double scale=root.XamlRoot().RasterizationScale();
        double extent=fitHeight?std::min(root.ActualWidth(),root.ActualHeight()):root.ActualWidth();
        double nextSize=std::floor(extent*scale)/scale;
        if(nextSize<128)return;
        if(std::abs(panelSize-nextSize)>.01||layoutScale!=scale){
            layoutScale=scale;
            cancel();panelSize=nextSize;if(!fitHeight)root.Height(panelSize);stage.Width(panelSize);stage.Height(panelSize);
            std::unique_ptr<char,decltype(&capy_string_free)> raw(capy_color_layout(float(panelSize)),capy_string_free);
            if(!raw)return;layout=J::Parse(to_hstring(raw.get()));
            auto box=array(layout,L"wheel");side=box.GetNumberAt(2);placeBox(wheel,box);
            // Browser canvas paint snaps the two layout edges to logical pixels.
            // Keep shared hit geometry while matching its final image placement.
            double inset=box.GetNumberAt(0),paintInset=std::round(inset),paintSide=std::round(inset+side)-paintInset;
            image.Width(paintSide);image.Height(paintSide);Canvas::SetLeft(image,paintInset-inset);Canvas::SetTop(image,paintInset-inset);
            drawError.Width(side);Canvas::SetTop(drawError,side*.4);
            placeBox(readout,array(layout,L"readout"));placeBox(swap,array(layout,L"swap"));
            for(int i=0;i<2;i++)placeBox(shapes[i],array(layout,L"shapes").GetArrayAt(i));
            const std::array<wchar_t const*,3> slots{L"background",L"foreground",L"transparent"};
            for(int i=0;i<3;i++){
                auto swatchBox=array(layout,slots[i]);placeBox(swatches[i],swatchBox);
                double pad=i==1?3:1;swatchChecks[i].Margin({pad,pad,pad,pad});swatchPaint[i].Margin({pad,pad,pad,pad});
                ImageBrush pixels;pixels.ImageSource(checker(swatchBox.GetNumberAt(2)-2*pad,scale));pixels.Stretch(Stretch::Fill);swatchChecks[i].Fill(pixels);
            }
        }
        auto icons=data->theme()+array(view,L"other_shapes").Stringify();
        if(icons!=iconKey){
            iconKey=icons;
            for(int i=0;i<2;i++){
                auto shape=array(view,L"other_shapes").GetStringAt(i);Grid content;Shapes::Ellipse hit;hit.Fill(clear());content.Children().Append(hit);
                auto glyph=colorIcon(shape,shapeHovered[i]?fill(color(L"#3584e4")):data->brush(L"text"));
                RotateTransform rotation;rotation.CenterX(8);rotation.CenterY(8);rotation.Angle(array(layout,L"shape_rotations").GetNumberAt(i));
                for(auto child:glyph.Children())child.as<Shapes::Path>().Data().Transform(rotation);
                content.Children().Append(glyph);shapes[i].Content(content);shapeGlyphs[i]=glyph;
                auto title=L"Use "+hstring(shape==L"circle"?L"Okhsv":shape==L"triangle"?L"HLS":L"HSV")+L" "+shape;
                AutomationProperties::SetName(shapes[i],title);ToolTipService::SetToolTip(shapes[i],box_value(title));
            }
        }
        for(int i=0;i<2;i++)for(auto child:shapeGlyphs[i].Children())child.as<Shapes::Path>().Stroke(shapeHovered[i]?fill(color(L"#3584e4")):data->brush(L"text"));
        auto swapInk=color(str(object(data->state,L"palette"),L"text"));swapInk.A=swapHovered?31:0;swapFill.Fill(fill(swapInk));
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
        auto nextKey=view.Stringify()+to_hstring(side)+L"/"+to_hstring(scale);
        if(nextKey!=key){
            try{
                try{drawing.draw(image,view,side,scale);}
                catch(hresult_error const& exception){
                    auto code=exception.code();
                    if(code!=DXGI_ERROR_DEVICE_REMOVED&&code!=DXGI_ERROR_DEVICE_RESET&&code!=D2DERR_RECREATE_TARGET&&code!=E_SURFACE_CONTENTS_LOST)throw;
                    drawing.draw(image,view,side,scale,true);
                }
                key=nextKey;drawError.Visibility(Visibility::Collapsed);AutomationProperties::SetItemStatus(image,L"Ready");
            }catch(hresult_error const&){drawError.Visibility(Visibility::Visible);AutomationProperties::SetItemStatus(image,L"Color wheel could not be drawn");}
        }
        updateReadout(view);
    }
};

}
FrameworkElement ColorPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,bool fitHeight){
    auto view=std::make_shared<View>(data,fitHeight);view->init();bindings.emplace_back([view]{view->refresh();});return view->root;
}
