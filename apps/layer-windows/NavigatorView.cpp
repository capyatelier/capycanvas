#include "pch.h"
#include "NavigatorView.h"
#include "NavigationControls.h"
#include "PointerGesture.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <optional>

using namespace CapyUi;
using Windows::Foundation::Point;
using Windows::Foundation::Rect;
namespace {
A coordinates(std::initializer_list<double> values){A result;for(auto v:values)result.Append(N(v));return result;}
Rect intersect(Rect a,Rect b){
    float x=std::max(a.X,b.X),y=std::max(a.Y,b.Y);
    return {x,y,std::max(0.f,std::min(a.X+a.Width,b.X+b.Width)-x),
        std::max(0.f,std::min(a.Y+a.Height,b.Y+b.Height)-y)};
}
struct NavigatorLayout:PanelT<NavigatorLayout>{
    Canvas overview;
    Grid actions;
    double inset,spacing,controlHeight,aspect=1;
    NavigatorLayout(Canvas const& image,Grid const& buttons,double padding,double gap,double height):
        overview(image),actions(buttons),inset(padding),spacing(gap),controlHeight(height){
        Children().Append(overview);Children().Append(actions);
    }
    double fixedHeight()const{return 2*inset+spacing+controlHeight;}
    double naturalImage(double width)const{return std::max(0.,width-2*inset)*aspect;}
    double imageHeight(Windows::Foundation::Size available)const{
        double natural=naturalImage(available.Width);
        return std::isfinite(available.Height)?std::clamp(double(available.Height)-fixedHeight(),0.,natural):natural;
    }
    Windows::Foundation::Size MeasureOverride(Windows::Foundation::Size available){
        actions.Measure({std::max(0.f,available.Width-float(2*inset)),float(controlHeight)});
        float width=std::isfinite(available.Width)?available.Width:actions.DesiredSize().Width+float(2*inset);
        double height=imageHeight({width,available.Height});
        overview.Measure({std::max(0.f,width-float(2*inset)),float(height)});
        return {width,float(height+fixedHeight())};
    }
    Windows::Foundation::Size ArrangeOverride(Windows::Foundation::Size finalSize){
        float width=std::max(0.f,finalSize.Width-float(2*inset));double height=imageHeight(finalSize);
        overview.Arrange({float(inset),float(inset),width,float(height)});
        actions.Arrange({float(inset),float(inset+height+spacing),width,float(controlHeight)});
        return finalSize;
    }
    void Aspect(double value){if(aspect!=value){aspect=value;InvalidateMeasure();}}
};
}
struct NavigatorView::Impl : std::enable_shared_from_this<Impl> {
    std::shared_ptr<WorkspaceData> data;
    std::function<void()> layoutChanged;
    Grid root,actions;
    com_ptr<NavigatorLayout> content;
    Canvas overview;
    Shapes::Path surround;
    Bindings bindings;
    std::optional<uint32_t> pointer;
    std::optional<std::array<double,13>> geometry;
    bool visible=true;
    Impl(std::shared_ptr<WorkspaceData> state,std::function<void()> changed):
        data(std::move(state)),layoutChanged(std::move(changed)){}
    void send(wchar_t const* phase,Point at){
        data->dispatch(O({{L"type",S(L"navigator")},{L"phase",S(phase)},
            {L"position",coordinates({at.X,at.Y})},
            {L"viewport",coordinates({overview.ActualWidth(),overview.ActualHeight()})}}));
    }
    void cancel(){
        if(pointer){pointer.reset();send(L"cancel",{});overview.ReleasePointerCaptures();}
    }
    void init(){
        auto weak=weak_from_this();
        AutomationProperties::SetName(root,data->caption(L"color",L"navigator_panel"));
        surround.Fill(data->brush(L"bg"));surround.IsHitTestVisible(false);
        root.Children().Append(surround);
        overview.Background(clear());edgeTag(overview,L"edge_contact",S(L"input"));
        overview.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetName(overview,data->caption(L"color",L"navigator_overview"));
        AutomationProperties::SetAutomationId(overview,L"navigator-overview");
        actions=NavigationButtons(data,L"navigator",[data=data]{return array(data->state,L"commands");},bindings);
        for(auto const& bind:bindings)bind();
        content=make_self<NavigatorLayout>(overview,actions,data->panelMetric(L"inset"),data->panelMetric(L"control_spacing"),data->panelMetric(L"control_height"));
        root.Children().Append(content.as<Panel>());updateAspect();
        overview.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->overview);
            if(self->pointer||!PointerGesture::PrimaryContact(p))return;
            if(self->overview.ActualWidth()<=8||self->overview.ActualHeight()<=8)return;
            if(self->overview.CapturePointer(e.Pointer())){self->pointer=p.PointerId();self->send(L"down",p.Position());e.Handled(true);}
        }});
        overview.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(self->pointer==e.Pointer().PointerId()){self->send(L"move",e.GetCurrentPoint(self->overview).Position());e.Handled(true);}
        }});
        PointerGesture::OnCompleted(overview,[weak](PointerRoutedEventArgs const& e,bool cancel){if(auto self=weak.lock();self&&self->pointer==e.Pointer().PointerId()){
            if(cancel)self->cancel();else{
                self->pointer.reset();self->send(L"up",e.GetCurrentPoint(self->overview).Position());self->overview.ReleasePointerCapture(e.Pointer());
            }e.Handled(true);
        }});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->cancel();self->layoutChanged();}});
        root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->refreshGeometry();});
        root.LayoutUpdated([weak](auto&&,auto&&){if(auto self=weak.lock())self->refreshGeometry();});
    }
    void refreshGeometry(){
        double width=root.ActualWidth(),height=root.ActualHeight();
        if(!root.IsLoaded()||width<=0||height<=0)return;
        auto tabs=array(data->state,L"tabs");auto document=tabs.Size()?tabs.GetObjectAt(0):J{};
        float bounds[4]{};
        bool valid=capy_navigator_image(float(overview.ActualWidth()),float(overview.ActualHeight()),
            uint32_t(num(document,L"width")),uint32_t(num(document,L"height")),bounds);
        auto origin=overview.TransformToVisual(root).TransformPoint({});
        auto position=root.TransformToVisual(nullptr).TransformPoint({});
        std::array<double,13> next{width,height,origin.X,origin.Y,overview.ActualWidth(),overview.ActualHeight(),bounds[0],bounds[1],bounds[2],bounds[3],double(valid),position.X,position.Y};
        if(geometry==next)return;
        cancel();geometry=next;
        auto shape=[&](Rect outer){
            GeometryGroup geometry;geometry.FillRule(FillRule::EvenOdd);
            RectangleGeometry rectangle;rectangle.Rect(outer);geometry.Children().Append(rectangle);
            if(valid){RectangleGeometry hole;hole.Rect({origin.X+bounds[0],origin.Y+bounds[1],bounds[2],bounds[3]});geometry.Children().Append(hole);}
            return geometry;
        };
        RectangleGeometry clip;clip.Rect({0,0,float(width),float(height)});root.Clip(clip);
        surround.Data(shape({origin.X,origin.Y,float(overview.ActualWidth()),float(overview.ActualHeight())}));
        layoutChanged();
    }
    void updateAspect(){
        auto tabs=array(data->state,L"tabs");auto document=tabs.Size()?tabs.GetObjectAt(0):J{};
        content->Aspect(capy_navigator_aspect(uint32_t(num(document,L"width")),uint32_t(num(document,L"height"))));
    }
    double naturalImage(double width)const{return content->naturalImage(width);}
    double fixedHeight()const{return content->fixedHeight();}
    void apply(bool show){
        visible=show;if(!visible)cancel();
        for(auto const& bind:bindings)bind();
        updateAspect();
    }
    J placement(UIElement const& reference,Rect clip,int order)const{
        if(!visible||!root.IsLoaded()||overview.ActualWidth()<=8||overview.ActualHeight()<=8)return {};
        auto at=overview.TransformToVisual(reference).TransformPoint({});
        Rect bounds{at.X,at.Y,float(overview.ActualWidth()),float(overview.ActualHeight())};
        auto body=root.TransformToVisual(reference).TransformPoint({});
        clip=intersect(intersect(clip,{body.X,body.Y,float(root.ActualWidth()),float(root.ActualHeight())}),bounds);
        if(clip.Width<=0||clip.Height<=0)return {};
        return O({{L"bounds",coordinates({bounds.X,bounds.Y,bounds.Width,bounds.Height})},
            {L"clip",coordinates({clip.X,clip.Y,clip.Width,clip.Height})},{L"order",N(order)}});
    }
};
NavigatorView::NavigatorView(std::shared_ptr<WorkspaceData> data,std::function<void()> changed):
    impl(std::make_shared<Impl>(std::move(data),std::move(changed))){impl->init();}
NavigatorView::~NavigatorView(){impl->cancel();}
FrameworkElement NavigatorView::Root()const{return impl->root;}
double NavigatorView::NaturalHeight(double width)const{return impl->naturalImage(width)+impl->fixedHeight();}
double NavigatorView::FixedHeight()const{return impl->fixedHeight();}
void NavigatorView::Apply(bool visible){impl->apply(visible);}
J NavigatorView::Placement(UIElement const& reference,Rect clip,int order)const{return impl->placement(reference,clip,order);}
