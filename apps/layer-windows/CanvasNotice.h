#pragma once
#include "UiControls.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceShadow.h"
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>

namespace CapyUi {
struct CanvasNotice:std::enable_shared_from_this<CanvasNotice>{
    static constexpr double Margin=12,BarReach=72,MaxWidth=720,Offscreen=-100000;
    static constexpr uint32_t TimeoutMs=4000;
    std::shared_ptr<WorkspaceData> data;
    Canvas host{nullptr};
    Grid frame;
    Shapes::Path surface;
    Grid row;
    TextBlock text{nullptr};
    Button action{nullptr};
    WorkspaceShadow shadow;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timeout{nullptr};
    std::optional<double> shown;
    J work,status,bar;
    bool visible=false;

    void init(Canvas const& root){
        host=root;auto weak=weak_from_this();
        text=label(data,L"");text.TextWrapping(TextWrapping::Wrap);text.VerticalAlignment(VerticalAlignment::Center);
        text.IsHitTestVisible(false);AutomationProperties::SetAutomationId(text,L"canvas-notice-text");
        action=button(data,L"",[weak]{if(auto self=weak.lock())self->accept();});
        action.Padding({12,5,12,5});action.MinHeight(34);action.Margin({0,-6,-8,-6});action.VerticalAlignment(VerticalAlignment::Center);
        action.AllowFocusOnInteraction(false);action.IsTabStop(false);action.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(action,L"canvas-notice-action");
        for(auto width:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){ColumnDefinition column;column.Width(width);row.ColumnDefinitions().Append(column);}
        row.ColumnSpacing(12);row.Padding({14,8,14,8});Grid::SetColumn(action,1);
        row.Children().Append(text);row.Children().Append(action);
        surface.IsHitTestVisible(false);frame.Children().Append(surface);frame.Children().Append(row);
        frame.Background(nullptr);
        AutomationProperties::SetAutomationId(frame,L"canvas-notice");AutomationProperties::SetName(frame,L"Canvas notice");
        AutomationProperties::SetLiveSetting(frame,Automation::Peers::AutomationLiveSetting::Polite);
        Canvas::SetZIndex(frame,900);
        timeout=host.DispatcherQueue().CreateTimer();timeout.IsRepeating(false);timeout.Interval(std::chrono::milliseconds(TimeoutMs));
        timeout.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->expire();});
        attach();present();
    }
    void attach(){
        uint32_t index;
        if(!host.Children().IndexOf(shadow.Root(),index))host.Children().Append(shadow.Root());
        if(!host.Children().IndexOf(frame,index))host.Children().Append(frame);
    }
    void Publish(J const& state){
        auto notice=object(state,L"notice");
        if(!notice.Size()){Hide();shown.reset();return;}
        auto id=num(notice,L"id");
        if(shown==id)return;
        shown=id;
        text.Text(str(notice,L"text"));
        auto offer=object(notice,L"action");
        action.Content(box_value(str(offer,L"label")));AutomationProperties::SetName(action,str(offer,L"label"));
        action.Visibility(offer.Size()?Visibility::Visible:Visibility::Collapsed);
        surface.Fill(data->brush(L"panel"));text.Foreground(data->brush(L"text"));
        visible=true;present();
        timeout.Stop();timeout.Start();
        if(auto peer=Automation::Peers::FrameworkElementAutomationPeer::FromElement(frame))
            peer.RaiseAutomationEvent(Automation::Peers::AutomationEvents::LiveRegionChanged);
    }
    void Place(J const& layout){work=object(layout,L"work_area");status=object(layout,L"status");present();}
    void Bar(J const& bounds){bar=bounds;present();}
    void Hide(){
        timeout.Stop();
        if(visible){visible=false;present();}
    }
    void accept(){
        if(!shown||!visible)return;
        Hide();
        data->dispatch(O({{L"type",S(L"notice")},{L"id",N(*shown)},{L"accept",B(true)}}));
    }
    void expire(){
        if(!shown||!visible)return;
        visible=false;present();
        data->dispatch(O({{L"type",S(L"notice")},{L"id",N(*shown)},{L"accept",B(false)}}));
    }
    void present(){
        if(!visible||!work.Size()){
            Canvas::SetLeft(frame,Offscreen);frame.Opacity(0);frame.IsHitTestVisible(false);
            shadow.Layout({float(Offscreen),0,1,1},899,false);
            return;
        }
        double areaBottom=num(work,L"y")+num(work,L"height");
        double floor=num(status,L"height")>0?std::min(areaBottom,num(status,L"y")):areaBottom;
        double y=bar.Size()&&num(bar,L"y")+num(bar,L"height")>floor-BarReach?num(bar,L"y")-Margin:floor-Margin;
        double limit=std::min(MaxWidth,std::max(0.,num(work,L"width")-2*Margin));
        frame.MaxWidth(limit);frame.Width(std::numeric_limits<double>::quiet_NaN());frame.Height(std::numeric_limits<double>::quiet_NaN());
        frame.Measure({float(limit),INFINITY});
        auto size=frame.DesiredSize();
        float width=std::min(float(limit),std::ceil(size.Width)+2);
        frame.Measure({width,INFINITY});
        float height=std::ceil(frame.DesiredSize().Height);
        frame.Width(width);frame.Height(height);
        double left=std::round(num(work,L"x")+num(work,L"width")/2-width/2),top=std::round(y-height);
        Canvas::SetLeft(frame,left);Canvas::SetTop(frame,top);
        std::array<float,4> radii{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
        surface.Data(squircleRectangle(width,height,radii));
        shadow.Shape(squircleRectangle(width,height,radii),width,height,8,2,.27f);shadow.Cut(radii);
        shadow.Layout({float(left),float(top),width,height},899,true);
        frame.Opacity(1);frame.IsHitTestVisible(true);
    }
};
}
