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
    ContentControl overview;
    Grid actions;
    double inset,spacing,controlHeight,aspect=1;
    NavigatorLayout(ContentControl const& image,Grid const& buttons,double padding,double gap,double height):
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
    ContentControl overview;
    Shapes::Path surround,backing;
    Bindings bindings;
    std::shared_ptr<ControlCapture> capture;
    std::optional<std::array<double,17>> geometry;
    bool visible=true;
    Impl(std::shared_ptr<WorkspaceData> state,std::function<void()> changed):
        data(std::move(state)),layoutChanged(std::move(changed)){}
    void send(wchar_t const* phase,Point at){
        data->dispatch(O({{L"type",S(L"navigator")},{L"phase",S(phase)},
            {L"position",coordinates({at.X,at.Y})},
            {L"viewport",coordinates({overview.ActualWidth(),overview.ActualHeight()})}}));
    }
    void cancel(){
        if(capture)capture->Cancel();
    }
    void init(){
        auto weak=weak_from_this();
        root.Background(data->brush(L"panel"));
        data->chromeFrames->observers.emplace(ChromeFrames::Id(root),[weak]{if(auto self=weak.lock())self->refreshGeometry();});
        AutomationProperties::SetName(root,data->caption(L"color",L"navigator_panel"));
        surround.Fill(data->brush(L"bg"));surround.IsHitTestVisible(false);
        root.Children().Append(surround);
        backing.Fill(data->brush(L"panel"));backing.IsHitTestVisible(false);root.Children().Append(backing);
        Grid inputSurface;inputSurface.Background(clear());overview.Content(inputSurface);
        overview.HorizontalContentAlignment(HorizontalAlignment::Stretch);overview.VerticalContentAlignment(VerticalAlignment::Stretch);edgeTag(overview,L"edge_contact",S(L"input"));
        overview.IsTabStop(true);overview.UseSystemFocusVisuals(false);
        overview.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetName(overview,data->caption(L"color",L"navigator_overview"));
        AutomationProperties::SetAutomationId(overview,L"navigator-overview");
        actions=NavigationButtons(data,L"navigator",[data=data]{return array(data->state,L"commands");},bindings);
        for(auto const& bind:bindings)bind();
        content=make_self<NavigatorLayout>(overview,actions,data->panelMetric(L"inset"),data->panelMetric(L"control_spacing"),data->panelMetric(L"control_height"));
        root.Children().Append(content.as<Panel>());updateAspect();
        capture=captureControl(data,overview,[weak](uint32_t,bool cancel,PointerRoutedEventArgs const* event){if(auto self=weak.lock()){
            self->send(cancel?L"cancel":L"up",event&&!cancel?event->GetCurrentPoint(self->overview).Position():Point{});
            if(event)event->Handled(true);
        }});
        overview.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->overview);
            if(self->capture->Active())return;
            if(self->overview.ActualWidth()<=8||self->overview.ActualHeight()<=8)return;
            if(self->capture->Begin(e)){self->overview.Focus(FocusState::Pointer);self->send(L"down",p.Position());e.Handled(true);}
        }});
        overview.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(self->capture->Owns(e.Pointer().PointerId())){self->send(L"move",e.GetCurrentPoint(self->overview).Position());e.Handled(true);}
        }});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->refreshGeometry();self->layoutChanged();}});
        root.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->refreshGeometry();});
        root.LayoutUpdated([weak](auto&&,auto&&){if(auto self=weak.lock())self->refreshGeometry();});
    }
    void refreshGeometry(){
        double width=root.ActualWidth(),height=root.ActualHeight();
        if(!root.IsLoaded()||width<=0||height<=0){root.Background(data->brush(L"panel"));surround.Data(nullptr);backing.Data(nullptr);geometry.reset();return;}
        auto tabs=array(data->state,L"tabs");auto document=tabs.Size()?tabs.GetObjectAt(0):J{};
        float bounds[4]{};
        bool valid=capy_navigator_image(float(overview.ActualWidth()),float(overview.ActualHeight()),
            uint32_t(num(document,L"width")),uint32_t(num(document,L"height")),bounds);
        auto origin=overview.TransformToVisual(root).TransformPoint({});
        auto position=root.TransformToVisual(nullptr).TransformPoint({});
        Rect hole{};auto acknowledged=presentedPlacement(root);auto image=array(acknowledged,L"bounds");auto acknowledgedClip=array(acknowledged,L"clip");float pixels[4]{};
        auto shown=data->chromeFrames->presented.document;
        if(image.Size()==4&&acknowledgedClip.Size()==4&&capy_navigator_image(float(image.GetNumberAt(2)),float(image.GetNumberAt(3)),uint32_t(num(shown,L"width")),uint32_t(num(shown,L"height")),pixels)){
            hole=intersect({float(image.GetNumberAt(0))+pixels[0],float(image.GetNumberAt(1))+pixels[1],pixels[2],pixels[3]},
                {float(acknowledgedClip.GetNumberAt(0)),float(acknowledgedClip.GetNumberAt(1)),float(acknowledgedClip.GetNumberAt(2)),float(acknowledgedClip.GetNumberAt(3))});
            hole=intersect(hole,{origin.X,origin.Y,float(overview.ActualWidth()),float(overview.ActualHeight())});
            if(valid)hole=intersect(hole,{origin.X+bounds[0],origin.Y+bounds[1],bounds[2],bounds[3]});
        }
        std::array<double,17> next{width,height,origin.X,origin.Y,overview.ActualWidth(),overview.ActualHeight(),bounds[0],bounds[1],bounds[2],bounds[3],double(valid),position.X,position.Y,hole.X,hole.Y,hole.Width,hole.Height};
        if(geometry==next)return;
        cancel();geometry=next;
        auto shape=[&](Rect outer){
            GeometryGroup geometry;geometry.FillRule(FillRule::EvenOdd);
            RectangleGeometry rectangle;rectangle.Rect(outer);geometry.Children().Append(rectangle);
            if(hole.Width>0&&hole.Height>0){RectangleGeometry cutout;cutout.Rect(hole);geometry.Children().Append(cutout);}
            return geometry;
        };
        RectangleGeometry clip;clip.Rect({0,0,float(width),float(height)});root.Clip(clip);
        bool ready=hole.Width>0&&hole.Height>0;root.Background(ready?Brush(nullptr):data->brush(L"panel"));
        surround.Data(ready?shape({origin.X,origin.Y,float(overview.ActualWidth()),float(overview.ActualHeight())}):Geometry(nullptr));
        backing.Data(valid?shape({origin.X+bounds[0],origin.Y+bounds[1],bounds[2],bounds[3]}):Geometry(nullptr));
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
    J placement(UIElement const& reference,Rect clip,int order,bool capture)const{
        if(!visible||!root.IsLoaded()||overview.ActualWidth()<=8||overview.ActualHeight()<=8)return {};
        auto at=overview.TransformToVisual(reference).TransformPoint({});
        Rect bounds{at.X,at.Y,float(overview.ActualWidth()),float(overview.ActualHeight())};
        auto body=root.TransformToVisual(reference).TransformPoint({});
        clip=intersect(intersect(clip,{body.X,body.Y,float(root.ActualWidth()),float(root.ActualHeight())}),bounds);
        if(clip.Width<=0||clip.Height<=0)return {};
        auto result=O({{L"bounds",coordinates({bounds.X,bounds.Y,bounds.Width,bounds.Height})},
            {L"clip",coordinates({clip.X,clip.Y,clip.Width,clip.Height})},{L"order",N(order)}});
        if(capture){data->chromeFrames->Track(root);data->chromeFrames->current.views.insert_or_assign(ChromeFrames::Id(root),result);}
        return result;
    }
    J presentedPlacement(UIElement const& reference)const{
        auto slot=data->chromeFrames->View(root);auto workspace=data->chromeFrames->root.get();
        if(!slot.Size()||!workspace||num(slot,L"opacity",1)<=0)return {};
        auto result=J::Parse(slot.Stringify());auto transform=workspace.TransformToVisual(reference);
        for(auto field:{L"bounds",L"clip"}){auto a=array(slot,field);if(a.Size()!=4)return {};
            auto b=transform.TransformBounds({float(a.GetNumberAt(0)),float(a.GetNumberAt(1)),float(a.GetNumberAt(2)),float(a.GetNumberAt(3))});
            result.Insert(field,coordinates({b.X,b.Y,b.Width,b.Height}));
        }
        return result;
    }
};
NavigatorView::NavigatorView(std::shared_ptr<WorkspaceData> data,std::function<void()> changed):
    impl(std::make_shared<Impl>(std::move(data),std::move(changed))){impl->init();}
NavigatorView::~NavigatorView(){impl->cancel();impl->data->chromeFrames->observers.erase(ChromeFrames::Id(impl->root));}
FrameworkElement NavigatorView::Root()const{return impl->root;}
double NavigatorView::NaturalHeight(double width)const{return impl->naturalImage(width)+impl->fixedHeight();}
double NavigatorView::FixedHeight()const{return impl->fixedHeight();}
void NavigatorView::Apply(bool visible){impl->apply(visible);}
J NavigatorView::Placement(UIElement const& reference,Rect clip,int order,bool capture)const{return impl->placement(reference,clip,order,capture);}
J NavigatorView::PresentedPlacement(UIElement const& reference)const{return impl->presentedPlacement(reference);}
