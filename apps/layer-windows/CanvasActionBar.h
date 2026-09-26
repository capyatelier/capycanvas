#pragma once
#include "UiControls.h"
#include "NativeMenus.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceQuery.h"
#include "WorkspaceShadow.h"

namespace CapyUi {
struct CanvasActionBar:std::enable_shared_from_this<CanvasActionBar>{
    static constexpr double Gap=4,Inset=6,Offscreen=-100000;
    std::shared_ptr<WorkspaceData> data;
    std::function<void()> changed;
    Canvas host{nullptr};
    Border frame;
    Grid content;
    Shapes::Path surface;
    StackPanel row,items,completion;
    TextBlock caption{nullptr};
    Button more{nullptr};
    MenuFlyout menu{nullptr};
    WorkspaceShadow shadow;
    std::vector<FrameworkElement> fields;
    Microsoft::UI::Dispatching::DispatcherQueueTimer reappear{nullptr};
    J view,bounds;
    hstring schema,theme;
    uint64_t request=0;
    size_t shown=0;
    bool contact=false,dragging=false,suppressed=false;

    void init(Canvas const& root){
        host=root;auto weak=weak_from_this();
        row.Orientation(Orientation::Horizontal);row.Spacing(Gap);row.Padding({Inset,Inset,Inset,Inset});
        for(auto panel:{items,completion}){panel.Orientation(Orientation::Horizontal);panel.Spacing(Gap);}
        caption=label(data,L"");caption.VerticalAlignment(VerticalAlignment::Center);caption.Margin({8,0,8,0});caption.Opacity(.8);
        more=button(data,L"More",[weak]{if(auto self=weak.lock())self->openMenu();});
        more.MinHeight(40);more.Width(40);more.AllowFocusOnInteraction(false);more.IsTabStop(false);
        AutomationProperties::SetAutomationId(more,L"canvas-bar-more");tooltip(more,L"More");
        row.Children().Append(caption);row.Children().Append(items);row.Children().Append(more);row.Children().Append(completion);
        surface.IsHitTestVisible(false);content.Children().Append(surface);content.Children().Append(row);
        frame.Child(content);frame.Background(clear());
        AutomationProperties::SetAutomationId(frame,L"canvas-action-bar");AutomationProperties::SetName(frame,L"Canvas actions");
        Canvas::SetZIndex(frame,180);
        reappear=host.DispatcherQueue().CreateTimer();reappear.IsRepeating(false);
        reappear.Interval(std::chrono::milliseconds(uint32_t(num(data->catalog,L"canvas_bar_reappear_ms",180))));
        reappear.Tick([weak](auto&&,auto&&){if(auto self=weak.lock()){self->suppressed=false;self->place();}});
        attach();present(false);
    }
    void attach(){
        uint32_t index;
        if(!host.Children().IndexOf(shadow.Root(),index))host.Children().Append(shadow.Root());
        if(!host.Children().IndexOf(frame,index))host.Children().Append(frame);
    }
    J context()const{return object(view,L"context");}
    bool nearObject()const{return str(view,L"placement")==L"near_object";}
    void send(J const& action){
        data->dispatch(O({{L"type",S(L"canvas_bar_edit")},{L"context",context()},{L"action",action}}));
    }
    static J command(J const& item){return object(object(object(item,L"option"),L"Action"),L"state");}
    static J choice(J const& item){return object(object(item,L"option"),L"Choice");}
    StackPanel labelled(hstring const& glyph,hstring const& text,bool menu=false){
        StackPanel inner;inner.Orientation(Orientation::Horizontal);inner.Spacing(6);
        if(!glyph.empty())inner.Children().Append(icon(glyph,data->theme()));
        auto caption=label(data,text);caption.VerticalAlignment(VerticalAlignment::Center);caption.FontWeight(Windows::UI::Text::FontWeights::Normal());
        inner.Children().Append(caption);
        if(menu)inner.Children().Append(icon(L"chevron-down",data->theme(),12));
        return inner;
    }
    template<typename T> void compact(T const& control,hstring const& id,hstring const& name){
        control.MinHeight(40);control.Padding({12,0,12,0});control.AllowFocusOnInteraction(false);control.IsTabStop(false);
        AutomationProperties::SetAutomationId(control,id);AutomationProperties::SetName(control,name);tooltip(control,name);
    }
    FrameworkElement segments(J const& spec){
        auto id=str(spec,L"id");auto list=array(spec,L"items");auto weak=weak_from_this();
        StackPanel group;group.Orientation(Orientation::Horizontal);
        AutomationProperties::SetAutomationId(group,L"canvas-bar-choice-"+id);AutomationProperties::SetName(group,str(spec,L"label"));
        for(uint32_t i=0;i<list.Size();++i){
            auto entry=list.GetObjectAt(i);auto action=object(entry,L"action");
            auto toggle=button<Primitives::ToggleButton>(data,L"",[weak,action]{if(auto self=weak.lock())self->send(action);});
            toggle.Content(labelled(str(entry,L"icon"),str(entry,L"label")));
            compact(toggle,L"canvas-bar-choice-"+id+L"-"+to_hstring(i),str(entry,L"label"));
            double left=i==0?6:0,right=i+1==list.Size()?6:0;toggle.CornerRadius({left,right,right,left});
            group.Children().Append(toggle);
        }
        return group;
    }
    FrameworkElement dropdown(J const& spec){
        auto id=str(spec,L"id");auto weak=weak_from_this();
        Button result=button(data,L"",[weak,id]{if(auto self=weak.lock())self->openChoice(id);});
        compact(result,L"canvas-bar-choice-"+id,str(spec,L"label"));
        return result;
    }
    FrameworkElement field(J const& item,bool finishing){
        if(auto spec=choice(item);spec.Size())return flag(spec,L"segmented")?segments(spec):dropdown(spec);
        auto option=object(object(item,L"option"),L"Action");auto state=object(option,L"state");auto id=str(state,L"id");
        auto weak=weak_from_this();
        auto activate=[weak,id]{if(auto self=weak.lock())self->send(O({{L"type",S(L"invoke")},{L"command",S(id)}}));};
        Primitives::ButtonBase result=flag(option,L"checkable")?Primitives::ButtonBase(button<Primitives::ToggleButton>(data,L"",activate)):Primitives::ButtonBase(button(data,L"",activate));
        StackPanel inner;inner.Orientation(Orientation::Horizontal);inner.Spacing(6);
        if(auto name=str(state,L"icon");!name.empty())inner.Children().Append(icon(name,data->theme()));
        auto text=label(data,str(item,L"label"));text.VerticalAlignment(VerticalAlignment::Center);text.FontWeight(Windows::UI::Text::FontWeights::Normal());inner.Children().Append(text);
        result.Content(inner);result.MinHeight(40);result.Padding({12,0,12,0});
        result.AllowFocusOnInteraction(false);result.IsTabStop(false);
        if(finishing&&(id==L"apply_transform"||id==L"complete_selection")){
            result.Background(accent(data));text.Foreground(data->brush(L"accent_foreground"));
            text.FontWeight(Windows::UI::Text::FontWeights::SemiBold());
        }
        AutomationProperties::SetAutomationId(result,L"canvas-bar-"+id);AutomationProperties::SetName(result,str(state,L"label"));
        tooltip(result,str(state,L"tooltip"));
        return result;
    }
    void update(FrameworkElement const& element,J const& item){
        if(auto spec=choice(item);spec.Size()){
            auto list=array(spec,L"items");
            if(auto group=element.try_as<StackPanel>()){
                for(uint32_t i=0;i<list.Size()&&i<group.Children().Size();++i)
                    group.Children().GetAt(i).as<Primitives::ToggleButton>().IsChecked(flag(list.GetObjectAt(i),L"selected"));
            }else if(auto picker=element.try_as<Button>()){
                for(auto value:list)if(auto entry=value.GetObject();flag(entry,L"selected"))picker.Content(labelled(str(entry,L"icon"),str(entry,L"label"),true));
            }
            return;
        }
        auto button=element.as<Primitives::ButtonBase>();auto state=command(item);
        button.IsEnabled(flag(state,L"enabled"));
        if(auto toggle=button.try_as<Primitives::ToggleButton>())toggle.IsChecked(flag(state,L"selected"));
    }
    hstring signature(J const& next)const{
        std::wstring key=std::wstring(object(next,L"context").Stringify())+L"|"+std::wstring(str(next,L"label"));
        for(auto list:{L"items",L"completion"})for(auto value:array(next,list)){
            auto item=value.GetObject();key+=L"|"+std::wstring(str(item,L"label"))+L":"+std::wstring(str(command(item),L"id"));
            if(auto spec=choice(item);spec.Size()){
                key+=L":"+std::wstring(str(spec,L"id"))+(flag(spec,L"segmented")?L":s":L":d");
                for(auto entry:array(spec,L"items"))key+=L","+std::wstring(str(entry.GetObject(),L"label"));
            }
        }
        return hstring(key);
    }
    void rebuild(){
        if(menu)menu.Hide();
        items.Children().Clear();completion.Children().Clear();fields.clear();
        caption.Text(str(view,L"label"));caption.Visibility(str(view,L"label").empty()?Visibility::Collapsed:Visibility::Visible);
        for(auto [list,panel,finishing]:{std::tuple{L"items",items,false},std::tuple{L"completion",completion,true}})
            for(auto value:array(view,list)){auto made=field(value.GetObject(),finishing);panel.Children().Append(made);fields.push_back(made);}
        surface.Fill(data->glass(L"panel"));
        more.Content(icon(L"more",data->theme()));
    }
    void Apply(J const& state){
        auto value=state.GetNamedValue(L"canvas_bar",JsonValue::CreateNullValue());
        if(value.ValueType()!=JsonValueType::Object){
            if(view.Size()){view=J{};schema=L"";++request;bounds=J{};if(menu)menu.Hide();present(false);}
            return;
        }
        auto next=value.GetObject();auto key=signature(next);
        view=next;
        if(key!=schema||theme!=data->theme()){schema=key;theme=data->theme();rebuild();}
        uint32_t index=0;
        for(auto list:{L"items",L"completion"})for(auto item:array(view,list))if(index<fields.size())update(fields[index++],item.GetObject());
        place();
    }
    void place(){
        if(!view.Size()){present(false);return;}
        std::vector<double> widths;
        double height=0;
        auto measure=[&](FrameworkElement const& element){element.Measure({INFINITY,INFINITY});height=std::max(height,double(element.DesiredSize().Height));return double(element.DesiredSize().Width);};
        for(auto const& item:fields){item.Visibility(Visibility::Visible);widths.push_back(measure(item));}
        auto count=array(view,L"items").Size();
        A itemWidths,completionWidths;
        for(size_t i=0;i<widths.size();++i)(i<count?itemWidths:completionWidths).Append(N(widths[i]));
        auto query=O({{L"type",S(L"canvas_bar_layout")},{L"measure",O({{L"context",context()},
            {L"label",N(caption.Visibility()==Visibility::Visible?measure(caption)+16:0)},
            {L"items",itemWidths},{L"completion",completionWidths},{L"more",N(measure(more))},
            {L"height",N(height+2*Inset)},{L"gap",N(Gap)},{L"padding",N(Inset)}})}});
        auto serial=++request;auto weak=weak_from_this();
        QueryWorkspace(data->query,query,[weak,serial](J reply){
            auto self=weak.lock();if(!self||serial!=self->request)return;
            auto result=object(reply,L"result");
            self->bounds=object(result,L"bounds");self->shown=size_t(num(result,L"items"));
            for(size_t i=0;i<self->fields.size()&&i<array(self->view,L"items").Size();++i)
                self->fields[i].Visibility(i<self->shown?Visibility::Visible:Visibility::Collapsed);
            self->present(self->bounds.Size()!=0);
        });
    }
    void present(bool placed){
        bool visible=placed&&view.Size()&&!suppressed;
        if(visible){
            float width=float(num(bounds,L"width")),height=float(num(bounds,L"height"));
            frame.Width(width);frame.Height(height);Canvas::SetLeft(frame,num(bounds,L"x"));Canvas::SetTop(frame,num(bounds,L"y"));
            std::array<float,4> radii{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
            surface.Data(squircleRectangle(width,height,radii));
            frame.CornerRadius({SurfaceRadius*CornerFit,SurfaceRadius*CornerFit,SurfaceRadius*CornerFit,SurfaceRadius*CornerFit});
            shadow.Shape(squircleRectangle(width,height,radii),width,height,12,2,.16f);shadow.Cut(radii);
            shadow.Layout(rectangle(bounds),179,true);
        }else{
            Canvas::SetLeft(frame,Offscreen);shadow.Layout({float(Offscreen),0,1,1},179,false);
        }
        frame.Opacity(visible?1:0);frame.IsHitTestVisible(visible);
        auto facts=visible?IJsonValue(bounds):IJsonValue(JsonValue::CreateNullValue());
        if(!data->chrome.HasKey(L"canvas_bar")||data->chrome.GetNamedValue(L"canvas_bar").Stringify()!=facts.Stringify()){
            data->chrome.Insert(L"canvas_bar",facts);if(changed)changed();
        }
    }
    void suppress(bool hidden){
        reappear.Stop();
        if(hidden){suppressed=true;if(menu)menu.Hide();present(false);return;}
        if(suppressed&&!contact&&!dragging)reappear.Start();
    }
    void Contact(bool active){
        contact=active;
        if(!active||nearObject())suppress(active);
    }
    void Dragging(bool active){
        if(dragging==active)return;
        dragging=active;suppress(active);
    }
    void Defer(){if(view.Size()&&nearObject()&&!contact&&!dragging){suppress(true);suppress(false);}}
    void AppendGlass(A& regions,UIElement const& reference)const{
        appendGlass(regions,frame,reference,{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius},true);
    }
    void openChoice(hstring const& id){
        if(!view.Size())return;
        auto weak=weak_from_this();
        QueryWorkspace(data->query,O({{L"type",S(L"canvas_bar_choice_menu")},{L"context",context()},{L"id",S(id)}}),[weak,id](J reply){
            auto self=weak.lock();if(!self||!self->view.Size())return;
            auto model=object(reply,L"result");if(!array(model,L"sections").Size())return;
            FrameworkElement anchor{nullptr};
            for(auto const& element:self->fields)if(AutomationProperties::GetAutomationId(element)==L"canvas-bar-choice-"+id)anchor=element;
            if(!anchor)return;
            if(self->menu)self->menu.Hide();
            self->menu=MenuFlyout();TrackPopup(self->menu,self->data);
            NativeMenuItems(self->menu.Items(),array(model,L"sections"),self->data,[data=self->data](J action){data->dispatch(action);});
            Primitives::FlyoutShowOptions options;options.Placement(Primitives::FlyoutPlacementMode::TopEdgeAlignedLeft);
            self->menu.ShowAt(anchor,options);
        });
    }
    void openMenu(){
        if(!view.Size())return;
        auto weak=weak_from_this();
        QueryWorkspace(data->query,O({{L"type",S(L"canvas_bar_menu")},{L"context",context()},{L"shown",N(double(shown))}}),[weak](J reply){
            auto self=weak.lock();if(!self||!self->view.Size())return;
            auto model=object(reply,L"result");if(!array(model,L"sections").Size())return;
            if(self->menu)self->menu.Hide();
            self->menu=MenuFlyout();TrackPopup(self->menu,self->data);
            NativeMenuItems(self->menu.Items(),array(model,L"sections"),self->data,[data=self->data](J action){data->dispatch(action);});
            Primitives::FlyoutShowOptions options;options.Placement(Primitives::FlyoutPlacementMode::TopEdgeAlignedRight);
            self->menu.ShowAt(self->more,options);
        });
    }
};
}
