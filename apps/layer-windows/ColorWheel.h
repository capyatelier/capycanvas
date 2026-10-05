#pragma once
#include "UiControls.h"
#include <d2d1_3.h>
#include <d3d11.h>
#include <microsoft.ui.xaml.media.dxinterop.h>
#include <mutex>
#include <thread>

namespace CapyWheel {
using namespace CapyUi;
using Point2=D2D1_POINT_2F;
using Paint=D2D1_COLOR_F;
inline Paint paint(A const& value){
    return {float(value.GetNumberAt(0)),float(value.GetNumberAt(1)),float(value.GetNumberAt(2)),
        value.Size()>3?float(value.GetNumberAt(3)):1.f};
}
inline winrt::Windows::UI::Color rgba(A const& value){
    auto p=paint(value);return {uint8_t(std::round(p.a*255)),uint8_t(std::round(p.r*255)),
        uint8_t(std::round(p.g*255)),uint8_t(std::round(p.b*255))};
}
inline Point2 point(A const& value){return {float(value.GetNumberAt(0)),float(value.GetNumberAt(1))};}

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
struct FieldRequest {hstring key;uint32_t pixels=0;float hue=0;uint32_t projection=0,rgbSpace=0;bool hdr=false,guide=false;std::string mapped;uint64_t epoch=0;};
struct FieldResult {hstring key;uint32_t pixels=0;uint64_t epoch=0;std::vector<uint8_t> bytes;};
inline bool rasterField(FieldRequest const& request,std::vector<uint8_t>& bytes){
    bytes.resize(size_t(request.pixels)*request.pixels*4);
    return request.hdr?capy_color_mapped_field(request.pixels,request.mapped.c_str(),bytes.data(),bytes.size())
        :capy_color_raster(request.pixels,request.hue,request.projection,request.rgbSpace,request.guide,bytes.data(),bytes.size());
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
    hstring ringKey,ringLayout,fieldKey,fieldLayout;
    std::shared_ptr<FieldWorker> ringWorker,worker;
    std::optional<FieldResult> ringReady,ready;
    uint64_t ringEpoch=0,epoch=0;uint32_t fieldPixels=0;
    static com_ptr<ID2D1Bitmap> bitmap(ID2D1DeviceContext2* context,FieldResult const& result){
        com_ptr<ID2D1Bitmap> bitmap;check_hresult(context->CreateBitmap(D2D1::SizeU(result.pixels,result.pixels),result.bytes.data(),result.pixels*4,
            D2D1::BitmapProperties(D2D1::PixelFormat(DXGI_FORMAT_R8G8B8A8_UNORM,D2D1_ALPHA_MODE_PREMULTIPLIED)),bitmap.put()));
        return bitmap;
    }
    void draw(Image const& image,J const& model,J const& state,double size,double scale,bool reset=false){
        int next=std::max(1,int(std::ceil(size*scale)));
        if(reset){surface=nullptr;device.reset();}
        if(!device||FAILED(device->d3d->GetDeviceRemovedReason())){
            device=Device::get();surface=nullptr;ring=nullptr;field=nullptr;ringKey=fieldKey=L"";
        }
        if(!surface||pixels!=next){
            pixels=next;
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
            auto shapeKey=shape+L"/"+working;
            if(shapeKey!=ringLayout){ringLayout=shapeKey;ring=nullptr;ringKey=L"";++ringEpoch;ringReady.reset();ringWorker->cancel();}
            if(ringReady&&ringReady->epoch==ringEpoch){
                auto bitmap=WheelImage::bitmap(context.get(),*ringReady);float side=float(ringReady->pixels);
                ring=nullptr;check_hresult(context->CreateImageBrush(bitmap.get(),D2D1::ImageBrushProperties(D2D1::RectF(0,0,side,side)),
                    D2D1::BrushProperties(1,D2D1::Matrix3x2F::Scale(1/side,1/side)),ring.put()));ringKey=ringReady->key;
            }
            ringReady.reset();
            auto wantedRing=shapeKey+L"/"+to_hstring(pixels);
            if(ringKey!=wantedRing)ringWorker->submit(FieldRequest{wantedRing,uint32_t(pixels),0,projection,rgbSpace,false,true,{},ringEpoch});
            Paint white{1,1,1,1};
            float hueValue=float(array(model,L"wheel_components").GetNumberAt(0));
            bool hdr=flag(model,L"hdr");auto rendition=object(model,L"rendition").Stringify();
            auto layoutKey=shapeKey+(hdr?L"/hdr"+rendition:hstring{});
            if(layoutKey!=fieldLayout){fieldLayout=layoutKey;field=nullptr;fieldKey=L"";++epoch;ready.reset();worker->cancel();}
            if(ready&&ready->epoch==epoch){field=WheelImage::bitmap(context.get(),*ready);fieldKey=ready->key;fieldPixels=ready->pixels;}
            ready.reset();
            auto wanted=layoutKey+L"/"+to_hstring(pixels)+L"/"+to_hstring(hueValue);if(hdr)wanted=wanted+state.Stringify();
            if(fieldKey!=wanted)worker->submit(FieldRequest{wanted,uint32_t(pixels),hueValue,projection,rgbSpace,hdr,false,
                hdr?to_string(O({{L"state",state},{L"rendition",object(model,L"rendition")}}).Stringify()):std::string{},epoch});
            com_ptr<ID2D1Factory> factory;context->GetFactory(factory.put());com_ptr<ID2D1Geometry> clip;
            if(projection==0){auto square=array(geometry,L"square");float x=float(square.GetNumberAt(0)),y=float(square.GetNumberAt(1)),w=float(square.GetNumberAt(2));
                float radius=float(std::min(6.,size*.02)/size);com_ptr<ID2D1RoundedRectangleGeometry> rounded;
                check_hresult(factory->CreateRoundedRectangleGeometry(D2D1::RoundedRect(D2D1::RectF(x,y,x+w,y+w),radius,radius),rounded.put()));clip=rounded;
            }else if(projection==2){com_ptr<ID2D1EllipseGeometry> ellipse;float radius=float(num(geometry,L"disc_radius"));
                check_hresult(factory->CreateEllipseGeometry(D2D1::Ellipse(center,radius,radius),ellipse.put()));clip=ellipse;}
            if(field){
                if(clip)context->PushLayer(D2D1::LayerParameters(D2D1::InfiniteRect(),clip.get()),nullptr);
                context->DrawBitmap(field.get(),D2D1::RectF(0,0,1,1),1,D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
                if(clip)context->PopLayer();
            }
            // Stroke one ellipse, as in the reference canvas, to keep both rims consistent.
            if(ring)context->DrawEllipse(D2D1::Ellipse(center,(inner+outer)/2,(inner+outer)/2),ring.get(),outer-inner);
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
}
