#include "pch.h"
#include "ColorView.h"
#include <d2d1_3.h>
#include <d3d11.h>
#include <microsoft.ui.xaml.media.dxinterop.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Input.h>
#include <array>
#include <optional>

using namespace CapyUi;
using namespace Windows::Foundation;
namespace {
using Point2=D2D1_POINT_2F;
using Paint=D2D1_COLOR_F;
Point2 mix(Point2 a,Point2 b,float t){return {a.x+(b.x-a.x)*t,a.y+(b.y-a.y)*t};}
Paint paint(A const& value){
    return {float(value.GetNumberAt(0)),float(value.GetNumberAt(1)),float(value.GetNumberAt(2)),
        value.Size()>3?float(value.GetNumberAt(3)):1.f};
}
Paint mix(Paint a,Paint b,float t){return {a.r+(b.r-a.r)*t,a.g+(b.g-a.g)*t,a.b+(b.b-a.b)*t,1.f};}
Windows::UI::Color rgba(A const& value){
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
D2D1_GRADIENT_MESH_PATCH quad(std::array<Point2,4> const& corners,std::array<Paint,4> const& colors){
    std::array<Point2,16> points;
    for(int row=0;row<4;row++)for(int col=0;col<4;col++)
        points[row*4+col]=mix(mix(corners[0],corners[1],col/3.f),mix(corners[2],corners[3],col/3.f),row/3.f);
    return patch(points,colors);
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
// One GPU gradient image; marker motion is ordinary retained XAML geometry.
// The image is redrawn only when its hue, color space, extent or scale changes.
struct WheelImage {
    std::shared_ptr<Device> device;
    Imaging::SurfaceImageSource surface{nullptr};
    int pixels=0;
    void draw(Image const& image,J const& model,double size,double scale,bool reset=false){
        int next=std::max(1,int(std::ceil(size*scale)));
        if(reset){surface=nullptr;device.reset();}
        if(!surface||pixels!=next||!device||FAILED(device->d3d->GetDeviceRemovedReason())){
            device=Device::get();pixels=next;
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
            context->SetTransform(D2D1::Matrix3x2F::Scale(float(size*scale),float(size*scale))*
                D2D1::Matrix3x2F::Translation(float(offset.x),float(offset.y)));
            context->Clear(D2D1::ColorF(0,0,0,0));
            auto geometry=object(model,L"geometry");
            auto center=point(array(geometry,L"center"));
            float inner=float(num(geometry,L"inner")),outer=float(num(geometry,L"outer"));
            auto stops=array(model,L"hue_stops");
            std::vector<D2D1_GRADIENT_MESH_PATCH> patches;
            // Small curved patches interpolate the supplied hue stops in encoded
            // display RGB. Their internal radial edges have no antialiasing seams.
            constexpr int count=48;
            float start=float(num(model,L"hue_start_degrees")*3.141592653589793/180.);
            for(int segment=0;segment<count;segment++){
                float a=start+segment*2.f*3.141592653589793f/count;
                float b=start+(segment+1)*2.f*3.141592653589793f/count;
                float tangent=4.f/3.f*std::tan((b-a)/4.f);
                std::array<Point2,16> p;
                for(int row=0;row<4;row++){
                    float r=inner+(outer-inner)*row/3.f;
                    Point2 first{center.x+r*std::cos(a),center.y+r*std::sin(a)};
                    Point2 last{center.x+r*std::cos(b),center.y+r*std::sin(b)};
                    p[row*4]=first;p[row*4+1]={first.x-r*tangent*std::sin(a),first.y+r*tangent*std::cos(a)};
                    p[row*4+2]={last.x+r*tangent*std::sin(b),last.y-r*tangent*std::cos(b)};p[row*4+3]=last;
                }
                int sector=segment/8;float t=(segment%8)/8.f;
                auto c0=mix(paint(stops.GetArrayAt(sector)),paint(stops.GetArrayAt(sector+1)),t);
                auto c1=mix(paint(stops.GetArrayAt(sector)),paint(stops.GetArrayAt(sector+1)),t+1.f/8.f);
                auto piece=patch(p,{c0,c1,c0,c1});
                piece.leftEdgeMode=piece.rightEdgeMode=D2D1_PATCH_EDGE_MODE_ALIASED;patches.push_back(piece);
            }
            Paint white{1,1,1,1},black{0,0,0,1},hue=paint(array(model,L"hue_color"));
            if(str(model,L"space")!=L"hsv"){
                auto t=array(geometry,L"triangle");auto w=point(t.GetArrayAt(0)),k=point(t.GetArrayAt(1)),h=point(t.GetArrayAt(2));
                patches.push_back(quad({w,h,k,h},{white,hue,black,hue}));
            }
            com_ptr<ID2D1GradientMesh> mesh;
            check_hresult(context->CreateGradientMesh(patches.data(),uint32_t(patches.size()),mesh.put()));
            context->DrawGradientMesh(mesh.get());
            if(str(model,L"space")==L"hsv"){
                auto s=array(geometry,L"square");float x=float(s.GetNumberAt(0)),y=float(s.GetNumberAt(1)),w=float(s.GetNumberAt(2));
                auto gradient=[&](Point2 from,Point2 to,Paint first,Paint last){
                    D2D1_GRADIENT_STOP stops[]={{0,first},{1,last}};
                    com_ptr<ID2D1GradientStopCollection> collection;
                    check_hresult(context->CreateGradientStopCollection(stops,2,D2D1_GAMMA_2_2,D2D1_EXTEND_MODE_CLAMP,collection.put()));
                    com_ptr<ID2D1LinearGradientBrush> brush;
                    check_hresult(context->CreateLinearGradientBrush(D2D1::LinearGradientBrushProperties(from,to),collection.get(),brush.put()));
                    context->FillRectangle(D2D1::RectF(x,y,x+w,y+w),brush.get());
                };
                gradient({x,y},{x+w,y},white,hue);
                gradient({x,y},{x,y+w},Paint{0,0,0,0},black);
            }
            active=false;check_hresult(native->EndDraw());
        }catch(...){if(active)native->EndDraw();throw;}
    }
};
struct View:std::enable_shared_from_this<View>{
    std::shared_ptr<WorkspaceData> data;
    StackPanel root;
    Canvas wheel;
    Image image;
    std::array<Grid,2> markers;
    Grid actions,values;
    std::array<Button,3> swatches;
    std::array<SolidColorBrush,3> swatchColors;
    Button mode;
    TextBlock error;
    Bindings numbers;
    WheelImage drawing;
    hstring context,key;
    std::optional<uint32_t> pointer;
    uint32_t part=0;
    double side=0;
    XamlRoot::Changed_revoker scaleChanged;
    CompositionTarget::SurfaceContentsLost_revoker contentsLost;
    explicit View(std::shared_ptr<WorkspaceData> source):data(std::move(source)){}
    J model()const{return object(data->model,L"color_panel");}
    hstring editingContext()const{return str(model(),L"space")+L"/"+str(object(data->state,L"colors"),L"paint_slot");}
    void send(J const& action){data->dispatch(O({{L"type",S(L"color")},{L"action",action}}));}
    void cancel(){pointer.reset();part=0;wheel.ReleasePointerCaptures();}
    void pick(Point position){
        if(!pointer||context!=editingContext()||side<1)return;
        A location;location.Append(N(position.X));location.Append(N(position.Y));
        send(O({{L"op",S(L"pick")},{L"part",S(part==1?L"hue":L"field")},{L"point",location},{L"size",N(side)}}));
    }
    void init(){
        auto weak=weak_from_this();
        root.Spacing(6);AutomationProperties::SetName(root,L"Color controls");
        wheel.Background(clear());AutomationProperties::SetName(image,L"Color wheel");
        image.IsHitTestVisible(false);image.Stretch(Stretch::Fill);wheel.Children().Append(image);
        for(auto& marker:markers){
            marker.Width(10);marker.Height(10);marker.IsHitTestVisible(false);
            for(auto [width,colorValue]:{std::pair{3.,Windows::UI::Color{255,0,0,0}},std::pair{1.5,Windows::UI::Color{255,255,255,255}}}){
                Shapes::Ellipse ring;ring.Width(7);ring.Height(7);ring.StrokeThickness(width);ring.Stroke(fill(colorValue));marker.Children().Append(ring);
            }wheel.Children().Append(marker);
        }
        wheel.SizeChanged([weak](auto&&,SizeChangedEventArgs const& e){if(auto self=weak.lock()){
            double width=e.NewSize().Width;if(std::abs(width-self->side)>.01){self->cancel();self->side=width;self->wheel.Height(width);self->refresh();}
        }});
        wheel.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->wheel);
            if(self->pointer||!p.IsInContact()||(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed()))return;
            auto at=p.Position();uint32_t part=capy_color_hit(at.X,at.Y,float(self->side),str(self->model(),L"space")==L"hls"?1:0);
            if(part&&self->wheel.CapturePointer(e.Pointer())){self->pointer=p.PointerId();self->part=part;self->pick(at);e.Handled(true);}
        }});
        wheel.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(self->pointer==e.Pointer().PointerId()){self->pick(e.GetCurrentPoint(self->wheel).Position());e.Handled(true);}
        }});
        wheel.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(self->pointer==e.Pointer().PointerId()){self->pick(e.GetCurrentPoint(self->wheel).Position());self->cancel();e.Handled(true);}
        }});
        wheel.PointerCanceled([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        wheel.PointerCaptureLost([weak](auto&&,auto&&){if(auto self=weak.lock()){self->pointer.reset();self->part=0;}});
        root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock()){
            self->scaleChanged=self->root.XamlRoot().Changed(auto_revoke,[weak](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
            self->refresh();
        }});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->cancel();self->scaleChanged.revoke();}});
        contentsLost=CompositionTarget::SurfaceContentsLost(auto_revoke,[weak](auto&&,auto&&){if(auto self=weak.lock()){self->key=L"";self->refresh();}});
        root.Children().Append(wheel);
        actions.ColumnSpacing(2);
        for(int i=0;i<5;i++){ColumnDefinition column;column.Width({1,GridUnitType::Star});actions.ColumnDefinitions().Append(column);}
        for(int i=0;i<3;i++){
            auto swatch=array(model(),L"swatches").GetObjectAt(i);
            auto slot=str(swatch,L"slot");
            auto pick=button(data,str(swatch,L"label"),[weak,slot]{if(auto self=weak.lock())self->send(O({{L"op",S(L"select")},{L"slot",S(slot)}}));});
            pick.Height(26);pick.HorizontalAlignment(HorizontalAlignment::Stretch);pick.Padding({3,3,3,3});pick.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            Grid sample;sample.Height(20);sample.IsHitTestVisible(false);
            Canvas checker;
            checker.SizeChanged([weakChecker=make_weak(checker)](auto&&,SizeChangedEventArgs const& e){
                if(auto checker=weakChecker.get()){
                    checker.Children().Clear();
                    for(int y=0;y<4;y++)for(int x=0;x<int(std::ceil(e.NewSize().Width/5));x++){
                        Shapes::Rectangle cell;cell.Width(std::min(5.,e.NewSize().Width-x*5.));cell.Height(5);
                        cell.Fill(fill((x+y)%2?Windows::UI::Color{255,140,140,140}:Windows::UI::Color{255,204,204,204}));
                        Canvas::SetLeft(cell,x*5);Canvas::SetTop(cell,y*5);checker.Children().Append(cell);
                    }
                }
            });
            sample.Children().Append(checker);
            Shapes::Rectangle overlay;swatchColors[i]=fill(rgba(array(swatch,L"rgba")));overlay.Fill(swatchColors[i]);sample.Children().Append(overlay);
            pick.Content(sample);Grid::SetColumn(pick,i);actions.Children().Append(pick);swatches[i]=pick;
        }
        auto swap=button(data,L"Swap foreground and background",[weak]{if(auto self=weak.lock())self->send(O({{L"op",S(L"swap")}}));});
        swap.Content(icon(L"swap",data->theme()));swap.Height(26);swap.HorizontalAlignment(HorizontalAlignment::Stretch);Grid::SetColumn(swap,3);actions.Children().Append(swap);
        mode=button(data,L"Switch HSV square / HLS triangle",[weak]{if(auto self=weak.lock())self->send(O({{L"op",S(L"toggle_space")}}));});
        mode.Height(26);mode.HorizontalAlignment(HorizontalAlignment::Stretch);Grid::SetColumn(mode,4);actions.Children().Append(mode);
        root.Children().Append(actions);root.Children().Append(values);
        error=label(data,L"");error.TextWrapping(TextWrapping::Wrap);error.Visibility(Visibility::Collapsed);root.Children().Append(error);
    }
    void rebuildValues(){
        numbers.clear();values.Children().Clear();values.ColumnDefinitions().Clear();values.ColumnSpacing(2);
        auto weak=weak_from_this();auto components=array(model(),L"components");
        for(int i=0;i<3;i++){
            ColumnDefinition column;column.Width({1,GridUnitType::Star});values.ColumnDefinitions().Append(column);
            auto component=components.GetObjectAt(i);
            StackPanel cell;auto title=label(data,str(component,L"label"));title.Opacity(.55);title.HorizontalAlignment(HorizontalAlignment::Center);cell.Children().Append(title);
            cell.Children().Append(number(data,str(component,L"name"),object(component,L"numeric"),
                [weak,i]{if(auto self=weak.lock())return num(array(self->model(),L"components").GetObjectAt(i),L"value");return 0.;},
                [weak,i,expected=context](double value){if(auto self=weak.lock();self&&self->editingContext()==expected)
                    self->send(O({{L"op",S(L"component")},{L"index",N(i)},{L"value",N(value)}}));},numbers,nullptr,true));
            Grid::SetColumn(cell,i);values.Children().Append(cell);
        }
        Shapes::Polygon symbol;symbol.Width(14);symbol.Height(20);symbol.Stroke(data->brush(L"text"));symbol.StrokeThickness(1.2);
        Windows::Foundation::Collections::IVector<Point> points=symbol.Points();
        if(str(model(),L"space")==L"hsv"){points.Append({2,16});points.Append({7,4});points.Append({12,16});}
        else{points.Append({2,4});points.Append({12,4});points.Append({12,16});points.Append({2,16});}
        mode.Content(symbol);
        AutomationProperties::SetItemStatus(mode,str(model(),L"space"));
    }
    void refresh(){
        auto view=model();if(!view.Size())return;
        auto nextContext=editingContext();
        if(context!=nextContext){cancel();context=nextContext;rebuildValues();}
        bool previous=std::exchange(data->updating,true);
        struct Reset{bool& flag;bool previous;~Reset(){flag=previous;}} reset{data->updating,previous};
        for(auto const& bind:numbers)bind();
        auto colors=array(view,L"swatches");
        for(int i=0;i<3;i++){auto swatch=colors.GetObjectAt(i);swatchColors[i].Color(rgba(array(swatch,L"rgba")));
            swatches[i].Background(flag(swatch,L"selected")?selected():clear());
            AutomationProperties::SetItemStatus(swatches[i],flag(swatch,L"selected")?L"Selected":L"");}
        if(side<1||!root.XamlRoot())return;
        image.Width(side);image.Height(side);
        double scale=root.XamlRoot().RasterizationScale();
        auto nextKey=O({{L"space",S(str(view,L"space"))},{L"hue",array(view,L"hue_color")},{L"size",N(side)},{L"scale",N(scale)}}).Stringify();
        if(nextKey!=key){
            try{
                try{drawing.draw(image,view,side,scale);}
                catch(hresult_error const& exception){
                    auto code=exception.code();
                    if(code!=DXGI_ERROR_DEVICE_REMOVED&&code!=DXGI_ERROR_DEVICE_RESET&&code!=D2DERR_RECREATE_TARGET&&code!=E_SURFACE_CONTENTS_LOST)throw;
                    drawing.draw(image,view,side,scale,true);
                }
                key=nextKey;error.Visibility(Visibility::Collapsed);
            }catch(hresult_error const&){error.Text(L"Color wheel could not be drawn. Color fields remain available.");error.Visibility(Visibility::Visible);}
        }
        int i=0;for(auto name:{L"hue_marker",L"field_marker"}){
            auto p=array(view,name);Canvas::SetLeft(markers[i],p.GetNumberAt(0)*side-5);Canvas::SetTop(markers[i],p.GetNumberAt(1)*side-5);i++;
        }
    }
};
}
FrameworkElement ColorPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<View>(data);view->init();bindings.emplace_back([view]{view->refresh();});return view->root;
}
