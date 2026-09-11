#include "pch.h"
#include "NavigatorView.h"
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
}
struct NavigatorView::Impl : std::enable_shared_from_this<Impl> {
    std::shared_ptr<WorkspaceData> data;
    std::function<void()> layoutChanged;
    Grid root,content,actions;
    Canvas overview;
    Shapes::Path background,surround;
    Bindings bindings;
    std::optional<uint32_t> pointer;
    hstring geometryKey;
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
        AutomationProperties::SetName(root,L"Navigator panel");
        background.Fill(data->brush(L"panel"));background.IsHitTestVisible(false);
        surround.Fill(data->brush(L"bg"));surround.IsHitTestVisible(false);
        root.Children().Append(background);root.Children().Append(surround);
        content.Margin({8,8,8,8});content.RowSpacing(4);content.VerticalAlignment(VerticalAlignment::Top);
        RowDefinition imageRow;imageRow.Height({1,GridUnitType::Auto});content.RowDefinitions().Append(imageRow);
        RowDefinition buttonsRow;buttonsRow.Height({32,GridUnitType::Pixel});content.RowDefinitions().Append(buttonsRow);
        // The path behind the controls leaves only the shared image bounds clear.
        // Transparent children alone cannot reveal a surface through an opaque panel.
        overview.Background(clear());overview.Height(220);
        overview.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetName(overview,L"Navigator overview");
        AutomationProperties::SetAutomationId(overview,L"navigator-overview");
        content.Children().Append(overview);
        for(int i=0;i<6;i++){ColumnDefinition column;column.Width({1,GridUnitType::Star});actions.ColumnDefinitions().Append(column);}
        int column=0;
        for(auto id:{L"zoom_out",L"zoom_in",L"rotate_left",L"rotate_right",L"flip_horizontal",L"flip_vertical"}){
            auto command=find(array(data->state,L"commands"),L"id",id);
            auto pick=button(data,str(command,L"label"),[data=data,id]{data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(id)}}));});
            pick.Content(icon(str(command,L"icon"),data->theme()));pick.Height(32);
            pick.HorizontalAlignment(HorizontalAlignment::Stretch);
            ToolTipService::SetToolTip(pick,box_value(str(command,L"label")));
            AutomationProperties::SetAutomationId(pick,hstring(L"navigator-")+id);
            Grid::SetColumn(pick,column++);actions.Children().Append(pick);
            bindings.emplace_back([data=data,id,pick]{auto command=find(array(data->state,L"commands"),L"id",id);
                pick.IsEnabled(flag(command,L"enabled"));pick.Background(flag(command,L"selected")?selected():clear());
                AutomationProperties::SetItemStatus(pick,flag(command,L"selected")?L"Selected":L"");});
        }
        Grid::SetRow(actions,1);content.Children().Append(actions);root.Children().Append(content);
        overview.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->overview);
            if(self->pointer||!p.IsInContact()||(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed()))return;
            if(self->overview.ActualWidth()<=8||self->overview.ActualHeight()<=8)return;
            if(self->overview.CapturePointer(e.Pointer())){self->pointer=p.PointerId();self->send(L"down",p.Position());e.Handled(true);}
        }});
        overview.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(self->pointer==e.Pointer().PointerId()){self->send(L"move",e.GetCurrentPoint(self->overview).Position());e.Handled(true);}
        }});
        overview.PointerReleased([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            if(self->pointer==e.Pointer().PointerId()){
                self->pointer.reset();self->send(L"up",e.GetCurrentPoint(self->overview).Position());
                self->overview.ReleasePointerCapture(e.Pointer());e.Handled(true);
            }
        }});
        overview.PointerCanceled([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        overview.PointerCaptureLost([weak](auto&&,auto&&){if(auto self=weak.lock())self->cancel();});
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->cancel();self->layoutChanged();}});
        root.LayoutUpdated([weak](auto&&,auto&&){if(auto self=weak.lock()){self->refreshGeometry();self->layoutChanged();}});
    }
    void refreshGeometry(){
        double width=root.ActualWidth(),height=root.ActualHeight();
        if(!root.IsLoaded()||width<=0||height<=0)return;
        double imageHeight=std::clamp(height-52.,0.,220.);
        if(overview.Height()!=imageHeight){overview.Height(imageHeight);return;}
        auto tabs=array(data->state,L"tabs");auto document=tabs.Size()?tabs.GetObjectAt(0):J{};
        float bounds[4]{};
        bool valid=capy_navigator_image(float(overview.ActualWidth()),float(overview.ActualHeight()),
            uint32_t(num(document,L"width")),uint32_t(num(document,L"height")),bounds);
        auto origin=overview.TransformToVisual(root).TransformPoint({});
        auto next=O({{L"root",coordinates({width,height})},{L"overview",coordinates({origin.X,origin.Y,overview.ActualWidth(),overview.ActualHeight()})},
            {L"image",coordinates({bounds[0],bounds[1],bounds[2],bounds[3]})},{L"valid",B(valid)}}).Stringify();
        if(next==geometryKey)return;
        cancel();geometryKey=next;
        auto shape=[&](Rect outer){
            GeometryGroup geometry;geometry.FillRule(FillRule::EvenOdd);
            RectangleGeometry rectangle;rectangle.Rect(outer);geometry.Children().Append(rectangle);
            if(valid){RectangleGeometry hole;hole.Rect({origin.X+bounds[0],origin.Y+bounds[1],bounds[2],bounds[3]});geometry.Children().Append(hole);}
            return geometry;
        };
        RectangleGeometry clip;clip.Rect({0,0,float(width),float(height)});root.Clip(clip);
        background.Data(shape({0,0,float(width),float(height)}));
        surround.Data(shape({origin.X,origin.Y,float(overview.ActualWidth()),float(overview.ActualHeight())}));
    }
    void apply(bool show){
        visible=show;if(!visible)cancel();
        for(auto const& bind:bindings)bind();
        refreshGeometry();
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
void NavigatorView::Apply(bool visible){impl->apply(visible);}
J NavigatorView::Placement(UIElement const& reference,Rect clip,int order)const{return impl->placement(reference,clip,order);}
