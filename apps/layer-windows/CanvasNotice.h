#pragma once
#include "UiControls.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceShadow.h"
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>
#include <winrt/Microsoft.UI.Xaml.Documents.h>

namespace CapyUi {
struct NoticeAnchor{double x,y,width;};
inline NoticeAnchor noticeAnchor(J const& work,J const& status,J const& bar){
    constexpr double Margin=12,BarReach=72,MaxWidth=720;
    double areaBottom=num(work,L"y")+num(work,L"height");
    double floor=num(status,L"height")>0?std::min(areaBottom,num(status,L"y")):areaBottom;
    double y=bar.Size()&&num(bar,L"y")+num(bar,L"height")>floor-BarReach?num(bar,L"y")-Margin:floor-Margin;
    return {num(work,L"x")+num(work,L"width")/2,y,std::min(MaxWidth,std::max(0.,num(work,L"width")-2*Margin))};
}
struct CanvasNotice:std::enable_shared_from_this<CanvasNotice>{
    static constexpr double Offscreen=-100000;
    static constexpr uint32_t TimeoutMs=4000;
    std::shared_ptr<WorkspaceData> data;
    Canvas host{nullptr};
    Grid frame;
    Shapes::Path surface;
    Grid row;
    TextBlock text{nullptr};
    RichTextBlock actions;
    Microsoft::UI::Xaml::Documents::Paragraph actionLine;
    std::vector<Button> choices;
    double arrangedWidth=-1;
    bool reflow=true;
    WorkspaceShadow shadow;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timeout{nullptr};
    std::optional<double> shown;
    J work,status,bar;
    bool visible=false;

    void init(Canvas const& root){
        host=root;auto weak=weak_from_this();
        text=label(data,L"");text.TextWrapping(TextWrapping::Wrap);text.VerticalAlignment(VerticalAlignment::Center);
        text.IsHitTestVisible(false);AutomationProperties::SetAutomationId(text,L"canvas-notice-text");
        actions.FontSize(data->textSize());actions.FontFamily(FontFamily(L"Segoe UI"));inheritLanguage(actions,data);
        actions.IsTextSelectionEnabled(false);actions.TextWrapping(TextWrapping::Wrap);actions.Blocks().Append(actionLine);
        actions.VerticalAlignment(VerticalAlignment::Center);actions.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(actions,L"canvas-notice-actions");
        for(auto width:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){ColumnDefinition column;column.Width(width);row.ColumnDefinitions().Append(column);}
        for(int i=0;i<2;++i){RowDefinition line;line.Height({1,GridUnitType::Auto});row.RowDefinitions().Append(line);}
        row.ColumnSpacing(12);row.Padding({14,8,14,8});Grid::SetColumn(actions,1);
        row.Children().Append(text);row.Children().Append(actions);
        surface.IsHitTestVisible(false);frame.Children().Append(surface);frame.Children().Append(row);
        frame.Background(nullptr);
        AutomationProperties::SetAutomationId(frame,L"canvas-notice");
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
        auto message=str(notice,L"text");bool changed=text.Text()!=message;
        if(changed){text.Text(message);AutomationProperties::SetName(frame,message);}
        auto offers=array(notice,L"actions");
        bool rebuild=offers.Size()!=choices.size();
        for(uint32_t i=0;!rebuild&&i<offers.Size();++i)rebuild=str(offers.GetObjectAt(i),L"id")!=unbox_value<hstring>(choices[i].Tag());
        if(rebuild){
            actionLine.Inlines().Clear();choices.clear();auto weak=weak_from_this();
            for(auto value:offers){
                auto token=str(value.GetObject(),L"id");
                auto choice=button(data,L"",[weak,token]{if(auto self=weak.lock())self->accept(token);});
                TextBlock caption;choice.Content(caption);choice.Tag(box_value(token));
                choice.Padding({12,5,12,5});choice.MinHeight(34);choice.Margin({0,-6,-8,-6});
                choice.AllowFocusOnInteraction(false);choice.IsTabStop(false);
                AutomationProperties::SetAutomationId(choice,L"canvas-notice-action-"+token);
                Microsoft::UI::Xaml::Documents::InlineUIContainer content;content.Child(choice);
                actionLine.Inlines().Append(content);choices.push_back(choice);
            }
            actions.Visibility(offers.Size()?Visibility::Visible:Visibility::Collapsed);changed=true;
        }
        for(uint32_t i=0;i<offers.Size();++i){
            auto offer=offers.GetObjectAt(i);auto choice=choices[i];
            auto caption=choice.Content().as<TextBlock>();auto name=str(offer,L"label"),reason=str(offer,L"reason");
            if(caption.Text()!=name){caption.Text(name);AutomationProperties::SetName(choice,name);changed=true;}
            if(choice.IsEnabled()!=flag(offer,L"enabled"))choice.IsEnabled(flag(offer,L"enabled"));
            if(AutomationProperties::GetHelpText(choice)!=reason){
                AutomationProperties::SetHelpText(choice,reason);ToolTipService::SetToolTip(choice,reason.empty()?nullptr:box_value(reason));
            }
        }
        reflow|=changed;
        if(shown==id){if(changed)present();return;}
        shown=id;
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
    void accept(hstring const& token){
        if(!shown||!visible)return;
        Hide();
        data->dispatch(O({{L"type",S(L"notice")},{L"id",N(*shown)},{L"accept",B(true)},{L"action",S(token)}}));
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
        auto anchor=noticeAnchor(work,status,bar);double limit=anchor.width;
        if(reflow||arrangedWidth!=limit){
            reflow=false;arrangedWidth=limit;double gap=choices.empty()?0.:12.,minimum=0;
            for(auto const& choice:choices){choice.Measure({INFINITY,INFINITY});minimum=std::max(minimum,double(choice.DesiredSize().Width));}
            actions.Measure({INFINITY,INFINITY});text.Measure({INFINITY,INFINITY});
            double message=text.DesiredSize().Width,offers=choices.empty()?0.:actions.DesiredSize().Width;
            double content=std::min(std::max(0.,limit-28),message+offers+gap),available=std::max(0.,content-gap);
            double actionWidth=message+offers>0?std::clamp(available*offers/(message+offers),std::min(minimum,available),available):0.;
            text.Measure({float(available-actionWidth),INFINITY});actions.Measure({float(actionWidth),INFINITY});
            double inlineHeight=std::max(text.DesiredSize().Height,actions.DesiredSize().Height);
            text.Measure({float(content),INFINITY});actions.Measure({float(content),INFINITY});
            bool stacked=!choices.empty()&&text.DesiredSize().Height+actions.DesiredSize().Height+gap<inlineHeight;
            row.ColumnDefinitions().GetAt(0).Width({stacked?content:available-actionWidth,GridUnitType::Pixel});
            row.ColumnDefinitions().GetAt(1).Width({stacked?0.:actionWidth,GridUnitType::Pixel});
            Grid::SetColumn(actions,stacked?0:1);Grid::SetRow(actions,stacked?1:0);
            row.ColumnSpacing(stacked?0:gap);row.RowSpacing(stacked?gap:0);row.Width(content+28);
        }
        frame.MaxWidth(limit);frame.Width(std::numeric_limits<double>::quiet_NaN());frame.Height(std::numeric_limits<double>::quiet_NaN());
        row.Measure({float(limit),INFINITY});
        auto size=row.DesiredSize();
        float width=std::min(float(limit),std::ceil(size.Width)+2);
        row.Measure({width,INFINITY});
        float height=std::ceil(row.DesiredSize().Height);
        frame.Width(width);frame.Height(height);
        double left=std::round(anchor.x-width/2),top=std::round(anchor.y-height);
        Canvas::SetLeft(frame,left);Canvas::SetTop(frame,top);
        std::array<float,4> radii{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
        surface.Data(squircleRectangle(width,height,radii));
        shadow.Shape(squircleRectangle(width,height,radii),width,height,8,2,.27f);shadow.Cut(radii);
        shadow.Layout({float(left),float(top),width,height},899,true);
        frame.Opacity(1);frame.IsHitTestVisible(true);
    }
};
}
