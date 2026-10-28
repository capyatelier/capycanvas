#pragma once
#include "UiControls.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceShadow.h"
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>

namespace CapyUi {
struct NoticeAnchor{double x,y,width,minText;};
inline NoticeAnchor noticeAnchor(J const& placement,J const& bar){
    constexpr double Margin=12;
    double y=num(placement,L"y");
    if(bar.Size()&&num(bar,L"y")+num(bar,L"height")>num(placement,L"bar_floor"))y=std::min(y,num(bar,L"y")-Margin);
    return {num(placement,L"x"),y,num(placement,L"max_width"),num(placement,L"min_text_width")};
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
    StackPanel actions;
    std::vector<Button> choices;
    double arrangedWidth=-1,gap=2;
    bool reflow=true;
    WorkspaceShadow shadow;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timeout{nullptr};
    std::optional<double> shown;
    J placement,bar;
    bool visible=false;

    void init(Canvas const& root){
        host=root;auto weak=weak_from_this();
        auto style=toolbarUi(data->localization.get(),O({{L"type",S(L"style")},{L"style",S(L"small")}})).GetObject();
        gap=num(style,L"gap",gap);
        text=label(data,L"");text.TextWrapping(TextWrapping::Wrap);text.VerticalAlignment(VerticalAlignment::Center);
        text.IsHitTestVisible(false);AutomationProperties::SetAutomationId(text,L"canvas-notice-text");
        actions.Orientation(Orientation::Horizontal);actions.Spacing(gap);actions.HorizontalAlignment(HorizontalAlignment::Right);
        actions.VerticalAlignment(VerticalAlignment::Center);actions.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(actions,L"canvas-notice-actions");
        for(auto width:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){ColumnDefinition column;column.Width(width);row.ColumnDefinitions().Append(column);}
        for(auto height:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){RowDefinition line;line.Height(height);row.RowDefinitions().Append(line);}
        row.MinHeight(array(style,L"size").GetNumberAt(1));Grid::SetColumn(actions,1);
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
        if(changed)text.Text(message);
        if(auto name=str(notice,L"name");AutomationProperties::GetName(frame)!=name)AutomationProperties::SetName(frame,name);
        auto offers=array(notice,L"actions");
        auto key=[](J const& offer){return str(offer,L"id")+(flag(offer,L"accent")?L":accent":L"");};
        bool rebuild=offers.Size()!=choices.size();
        for(uint32_t i=0;!rebuild&&i<offers.Size();++i)rebuild=key(offers.GetObjectAt(i))!=str(choices[i].Tag().as<J>(),L"notice_offer");
        if(rebuild){
            actions.Children().Clear();choices.clear();auto weak=weak_from_this();
            for(auto value:offers){
                auto offer=value.GetObject();auto token=str(offer,L"id");
                auto choice=button(data,L"",[weak,token]{if(auto self=weak.lock())self->accept(token);});
                auto caption=label(data,L"",true);caption.VerticalAlignment(VerticalAlignment::Center);choice.Content(caption);edgeTag(choice,L"notice_offer",S(key(offer)));
                choice.Height(data->controlHeight());choice.Padding({10,0,10,0});choice.CornerRadius(data->controlCorner());
                choice.AllowFocusOnInteraction(false);
                if(flag(offer,L"accent")){
                    choice.Background(accent(data));caption.Foreground(data->brush(L"accent_foreground"));
                }
                AutomationProperties::SetAutomationId(choice,L"canvas-notice-action-"+token);
                auto shell=explainable(choice);shell.VerticalAlignment(VerticalAlignment::Center);
                shell.Margin({ControlClearance,ControlClearance,ControlClearance,ControlClearance});
                actions.Children().Append(shell);choices.push_back(choice);
            }
            actions.Visibility(offers.Size()?Visibility::Visible:Visibility::Collapsed);changed=true;
        }
        for(uint32_t i=0;i<offers.Size();++i){
            auto offer=offers.GetObjectAt(i);auto choice=choices[i];bool enabled=flag(offer,L"enabled");
            auto caption=choice.Content().as<TextBlock>();auto name=str(offer,L"label");
            if(caption.Text()!=name){caption.Text(name);AutomationProperties::SetName(choice,name);changed=true;}
            explain(actions.Children().GetAt(i).as<Grid>(),choice,enabled,name,str(offer,L"reason"));choice.Opacity(enabled?1:.36);
        }
        reflow|=changed;
        if(shown==id){if(changed)present();return;}
        shown=id;
        surface.Fill(data->glass(L"panel"));
        visible=true;present();
        timeout.Stop();timeout.Start();
        if(auto peer=Automation::Peers::FrameworkElementAutomationPeer::FromElement(frame))
            peer.RaiseAutomationEvent(Automation::Peers::AutomationEvents::LiveRegionChanged);
    }
    void Place(J const& layout){placement=object(layout,L"notice");present();}
    void Bar(J const& bounds){bar=bounds;present();}
    void AppendGlass(A& regions,UIElement const& reference)const{
        appendGlass(regions,frame,reference,{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius},true);
    }
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
        for(auto const& choice:choices)choice.IsTabStop(visible&&placement.Size());
        if(!visible||!placement.Size()){
            Canvas::SetLeft(frame,Offscreen);frame.Opacity(0);frame.IsHitTestVisible(false);
            shadow.Layout({float(Offscreen),0,1,1},899,false);
            return;
        }
        auto anchor=noticeAnchor(placement,bar);double limit=anchor.width;
        if(reflow||arrangedWidth!=limit){
            reflow=false;arrangedWidth=limit;bool offered=!choices.empty();double spacing=offered?gap:0.;
            auto natural=[&](Thickness padding){text.Margin(padding);text.Measure({INFINITY,INFINITY});return double(text.DesiredSize().Width);};
            actions.Measure({INFINITY,INFINITY});
            double message=natural({12,6,offered?8.:12.,6}),offers=offered?actions.DesiredSize().Width:0.;
            bool stacked=offered&&message+offers+gap>limit&&limit-offers-gap<anchor.minText;
            if(stacked)message=natural({12,10,12,2});
            row.Width(std::min(limit,std::ceil(stacked?std::max(message,offers):message+spacing+offers)+1));
            row.ColumnDefinitions().GetAt(1).Width(stacked?GridLength{0,GridUnitType::Pixel}:GridLength{1,GridUnitType::Auto});
            Grid::SetColumn(actions,stacked?0:1);Grid::SetRow(actions,stacked?1:0);
            row.ColumnSpacing(stacked?0:spacing);row.RowSpacing(stacked?gap:0);
        }
        float width=float(row.Width());
        row.Measure({width,INFINITY});
        float height=std::ceil(row.DesiredSize().Height);
        frame.Width(width);frame.Height(height);
        double left=std::round(anchor.x-width/2),top=std::round(anchor.y-height);
        Canvas::SetLeft(frame,left);Canvas::SetTop(frame,top);
        std::array<float,4> radii{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
        surface.Data(squircleRectangle(width,height,radii));
        shadow.Shape(squircleRectangle(width,height,radii),width,height,12,2,.16f);shadow.Cut(radii);
        shadow.Layout({float(left),float(top),width,height},899,true);
        frame.Opacity(1);frame.IsHitTestVisible(true);
    }
};
}
