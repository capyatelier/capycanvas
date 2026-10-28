#pragma once
#include "WorkspaceBubbles.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceQuery.h"
#include "IntensityImage.h"
#include "LayerInspector.h"
#include "NativeMenus.h"
#include "StampPreview.h"
#include "WorkspaceShadow.h"
#include "PointerGesture.h"
#include "ColorTouch.h"
#include <chrono>
#include <set>
#include <winrt/Windows.UI.ViewManagement.h>

namespace CapyBubbles {
using namespace CapyUi;
using Clock=std::chrono::steady_clock;
using winrt::Windows::Foundation::Size;
inline bool prefix(hstring const& value,std::wstring_view start){return std::wstring_view(value).starts_with(start);}
inline double milliseconds(){return std::chrono::duration<double,std::milli>(Clock::now().time_since_epoch()).count();}
inline A point(Point value){A a;a.Append(N(value.X));a.Append(N(value.Y));return a;}
inline Point point(A const& a){return a.Size()==2?Point{float(a.GetNumberAt(0)),float(a.GetNumberAt(1))}:Point{};}
inline double distance(Point a,Point b={}){return std::hypot(a.X-b.X,a.Y-b.Y);}
inline Point relative(Point a,Point b){return {a.X-b.X,a.Y-b.Y};}
inline V ui(std::shared_ptr<WorkspaceData> const& data,J const& request){
    NativeString raw(capy_bubble_ui(data->localization.get(),to_string(request.Stringify()).c_str()),capy_string_free);
    if(!raw)throw hresult_invalid_argument();return JsonValue::Parse(to_hstring(raw.get()));
}
inline hstring kind(V const& value){return value&&value.ValueType()==JsonValueType::String?value.GetString():L"";}
struct Tween {
    double value=0,target=0,from=0,start=-1,duration=0,delay=0;
    bool smooth=false;
    explicit Tween(double initial=0,bool smooth=false):value(initial),target(initial),from(initial),smooth(smooth){}
    void snap(double next){value=target=from=next;start=-1;}
    void to(double next,double ms,double wait=0){if(target==next)return;from=value;target=next;duration=ms;delay=wait;start=-1;}
    bool step(double now,bool reduced){if(value==target)return false;if(start<0)start=now;
        auto t=reduced||duration==0?1.:std::clamp((now-start-delay)/duration,0.,1.);value=from+(target-from)*(smooth?t*t*(3-2*t):1-std::pow(1-t,3));return t<1.;}
};
struct RingArt {
    Canvas root;
    Shapes::Path body,track,value;
    Shapes::Ellipse thumb;
    Image glyph;
    hstring geometryKey,iconKey;
    double fill=-1;
};
struct IntensityArt {
    Canvas root;
    Slider value;
    Shapes::Line zero;
    Shapes::Ellipse edge,marker,focus;
    Image ramp;
    std::unique_ptr<CapyWheel::IntensityImage> image;
    hstring geometryKey;
    bool syncing=false;
};
struct Part {
    hstring key,shadowKey,surfaceKey,theme;
    J data;
    Canvas root,ink,art,decorations,shadow;
    Shapes::Path background,selection,hover;
    Button accessible{nullptr};
    CompositeTransform transform;
    Point center{},fromCenter{},toCenter{};
    Size size{},fromSize{},toSize{};
    Tween appearance,layout{1};
    double fromScale=.6,radius=0,strength=1,fromStrength=1,toStrength=1;
    int order=0;
    bool squircle=false,glass=true,hot=false,retired=false,contentDirty=true;
    std::unique_ptr<CapyWheel::WheelImage> wheel;
    std::unique_ptr<IntensityArt> intensity;
    std::unique_ptr<WorkspaceShadow> colorShadow;
    Image wheelImage;
    std::array<Shapes::Ellipse,2> halves;
    ContentControl historyHost{nullptr};
    std::array<Button,2> historyButtons{nullptr,nullptr};
    std::vector<RingArt> rings;
    ImageBrush thumbnailBrush;
    Shapes::Path thumbnailFace;
    hstring wheelKey;
    std::unique_ptr<PanelBody> layers;
    std::unique_ptr<LayerInspector> inspector;
    ContentControl cardHost{nullptr};
    explicit Part(hstring name,double initial):key(name),appearance(initial){}
    double scale()const{return (fromScale+(1-fromScale)*appearance.value)*(hot?(prefix(key,L"brush:")?1.06:1.12):1.);}
    void place(Point,Size,double,bool);
    bool step(double,bool);
};
struct Record {
    uint32_t id=0;
    int order=0;
    J packet,view;
    hstring source;
    Canvas root;
    Canvas hints;
    CompositeTransform transform;
    std::map<hstring,std::shared_ptr<Part>> parts;
    Point center{};
    Tween fade{1,true};
    bool lifted=false,homing=false,faded=false,pressed=false;
    double pinch=1,reach=0;
    V hovered{nullptr};
    uint64_t stampGeneration=0;
    bool stampBusy=false,stampDirty=true,stampPending=false;
    J stamp;
    Image stampImage;
    hstring stampColor;
    void clearStamp(){++stampGeneration;stampBusy=stampPending=false;stampDirty=true;stamp=J{};stampImage.Source(nullptr);stampImage.Visibility(Visibility::Collapsed);}
    void place();
    double scale()const{return pinch*(homing?.82:lifted?1.04:1.);}
    double alpha()const{return fade.value*(homing?.55:1.);}
};
struct Contact {
    std::shared_ptr<Record> record;
    V hit{nullptr};
    Input::Pointer pointer{nullptr};
    Microsoft::UI::Input::PointerDeviceType device=Microsoft::UI::Input::PointerDeviceType::Mouse;
    hstring mode=L"pending";
    Point start{},last{},midpoint{},pairOrigin{};
    J grab;
    uint32_t wheel=0;
    double started=0,spread=1,scale=1;
    bool held=false,paired=false,lifted=false,pairEnded=false,colorStarted=false,moved=false;
    bool recognizing=false;
    bool nativeDrags=false;
    double holdAt=-1;
    PointerGesture recognizer;
    std::vector<weak_ref<UIElement>> path;
    std::map<uint32_t,Point> fingers;
};
struct Candidate {
    Input::Pointer pointer{nullptr};
    Point last{};
    Rect bounds{};
    J grab;
    hstring identity;
    weak_ref<FrameworkElement> source;
    std::vector<weak_ref<UIElement>> path;
    bool moving=false;
    uint64_t epoch=0;
};
}

struct WorkspaceBubbles::Impl:std::enable_shared_from_this<WorkspaceBubbles::Impl> {
    std::shared_ptr<CapyUi::WorkspaceData> data;
    CapyUi::Canvas workspace{nullptr};
    CapyUi::Canvas layer;
    CapyUi::Canvas stamps;
    CapyUi::Shapes::Ellipse contactReach;
    CapyUi::UIElement inputRoot{nullptr};
    std::function<void()> changed,cancelSources;
    std::map<uint32_t,std::shared_ptr<CapyBubbles::Record>> records;
    std::shared_ptr<CapyBubbles::Contact> contact;
    std::optional<CapyBubbles::Candidate> candidate;
    CapyUi::CompositionTarget::Rendering_revoker frames;
    winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer holdTimer{nullptr};
    winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer thumbnailTimer{nullptr};
    CapyUi::CompositionTarget::SurfaceContentsLost_revoker contentsLost;
    uint64_t colorListener=0;
    CapyUi::MenuFlyout menu{nullptr};
    CapyUi::OwnedPopup menuOwner;
    CapyUi::J menuRequest;
    uint32_t menuId=0;
    winrt::hstring menuSource;
    bool keepMenu=false;
    uint64_t epoch=0,owners=0,wakes=0,menuGeneration=0;
    bool busy=false,active=false,stroke=false,releasing=false,disposed=false,hovering=false;
    bool pointerPen=false,pointerHidden=false;
    double restoreAt=-1;
    std::optional<std::tuple<uint32_t,double,CapyUi::Point>> lastRing;
    CapyUi::ColorTouches colorTouches;
    ~Impl();
    void init();
    void attach(CapyUi::UIElement const&,std::function<void()>);
    void apply();
    void reattach();
    void wake();
    void frame();
    void step(CapyUi::J const&,double,bool,uint64_t);
    void update(std::shared_ptr<CapyBubbles::Record> const&,CapyUi::J const&);
    void refresh(std::shared_ptr<CapyBubbles::Record> const&);
    void draw(std::shared_ptr<CapyBubbles::Record> const&,std::shared_ptr<CapyBubbles::Part> const&);
    void content(std::shared_ptr<CapyBubbles::Record> const&,std::shared_ptr<CapyBubbles::Part> const&);
    void stamp(std::shared_ptr<CapyBubbles::Record> const&);
    void thumbnails();
    void glass(CapyUi::A&,CapyUi::A&,CapyUi::A&,CapyUi::UIElement const&)const;
    void intensity(std::shared_ptr<CapyBubbles::Record> const&,std::shared_ptr<CapyBubbles::Part> const&);
    void send(CapyUi::J const&);
    void drag(CapyUi::J const&,winrt::hstring const&,CapyUi::Point);
    CapyUi::V hit(CapyBubbles::Record const&,CapyUi::Point)const;
    std::shared_ptr<CapyBubbles::Record> target(CapyUi::Point)const;
    void down(winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs const&);
    bool join(winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs const&);
    void colorTouch(winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs const&,winrt::hstring const&,std::shared_ptr<CapyBubbles::Record> const&);
    void move(winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs const&);
    void up(winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs const&,bool);
    bool cancel(bool abandon=false);
    void restore(std::shared_ptr<CapyBubbles::Contact> const&);
    void begin(std::shared_ptr<CapyBubbles::Contact> const&,CapyUi::Point);
    void pick(std::shared_ptr<CapyBubbles::Contact> const&,CapyUi::Point);
    void finishColor(std::shared_ptr<CapyBubbles::Contact> const&,bool);
    void hold();
    void contactSurface();
    bool capture(CapyUi::Input::Pointer const&,std::vector<winrt::weak_ref<CapyUi::UIElement>> const&);
    void dismiss();
    void showMenu(std::shared_ptr<CapyBubbles::Record> const&,CapyUi::Point,bool palettes=false);
    CapyUi::J source(CapyUi::J const&)const;
    bool current(CapyBubbles::Candidate const&)const;
};
