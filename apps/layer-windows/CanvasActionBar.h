#pragma once
#include "UiControls.h"
#include "NativeMenus.h"
#include "WorkspaceGeometry.h"
#include "WorkspaceQuery.h"
#include "WorkspaceShadow.h"

namespace CapyUi {
struct CanvasActionBar:std::enable_shared_from_this<CanvasActionBar>{
    static constexpr double Offscreen=-100000;
    struct Field {FrameworkElement slot{nullptr},element{nullptr};};
    struct Prepared {StackPanel row;TextBlock caption{nullptr};Button more{nullptr};std::vector<Field> fields;J view,sizes;hstring schema,theme;};
    std::shared_ptr<WorkspaceData> data;
    std::function<void()> changed;
    Canvas host{nullptr};
    Border frame;
    Grid content;
    Shapes::Path surface;
    StackPanel row;
    TextBlock caption{nullptr};
    Button more{nullptr};
    MenuFlyout menu{nullptr};
    OwnedPopup menuOwner;
    WorkspaceShadow shadow;
    std::vector<Field> fields;
    std::shared_ptr<Prepared> pending;
    Microsoft::UI::Dispatching::DispatcherQueueTimer reappear{nullptr};
    J view,bounds,sizes;
    hstring schema,theme,placement;
    uint64_t request=0;
    size_t shown=0;
    double tile=36,gap=2,glyph=16;
    bool contact=false,dragging=false,suppressed=false;

    void init(Canvas const& root){
        host=root;auto weak=weak_from_this();
        auto style=toolbarUi(data->localization.get(),O({{L"type",S(L"style")},{L"style",S(L"small")}})).GetObject();
        tile=array(style,L"size").GetNumberAt(1);gap=num(style,L"gap",gap);glyph=num(style,L"icon",glyph);
        surface.IsHitTestVisible(false);content.Children().Append(surface);content.Children().Append(row);
        frame.Child(content);frame.Background(clear());
        AutomationProperties::SetAutomationId(frame,L"canvas-action-bar");copyName(data,frame,data->copyCaption(L"color",L"canvas_actions"));
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
    hstring accentTheme()const{
        auto ink=data->brush(L"accent_foreground").Color();
        return 299*ink.R+587*ink.G+114*ink.B>128000?L"dark":L"light";
    }
    void quiet(Control const& control){control.AllowFocusOnInteraction(false);control.IsTabStop(false);}
    void tiled(Control const& control){
        double r=tile*.5*CornerFit;
        control.Width(tile);control.Height(tile);control.Padding({0,0,0,0});control.CornerRadius({r,r,r,r});
        control.HorizontalContentAlignment(HorizontalAlignment::Stretch);control.VerticalContentAlignment(VerticalAlignment::Stretch);buttonColors(data,control.as<Primitives::ButtonBase>(),20,control.try_as<Primitives::ToggleButton>()?Brush(selected(data)):Brush(nullptr),.45);quiet(control);
    }
    void worded(Control const& control,double padding=10){
        control.Height(data->controlHeight());control.Padding({padding,0,padding,0});control.CornerRadius(data->controlCorner());
        control.VerticalAlignment(VerticalAlignment::Center);buttonColors(data,control.as<Primitives::ButtonBase>(),20,control.try_as<Primitives::ToggleButton>()?Brush(selected(data)):Brush(nullptr),.45);quiet(control);
    }
    FrameworkElement face(hstring const& name,hstring const& iconTheme,bool group){
        Grid result;auto image=icon(name,iconTheme,glyph);
        image.HorizontalAlignment(HorizontalAlignment::Center);image.VerticalAlignment(VerticalAlignment::Center);result.Children().Append(image);
        if(group)result.Children().Append(ToolGroupMarker(data));
        return result;
    }
    StackPanel labelled(hstring const& name,hstring const& text,bool menu=false,hstring const& iconTheme={}){
        StackPanel inner;inner.Orientation(Orientation::Horizontal);inner.Spacing(6);
        if(!name.empty())inner.Children().Append(icon(name,iconTheme.empty()?data->theme():iconTheme,glyph));
        auto words=label(data,text,true);words.VerticalAlignment(VerticalAlignment::Center);
        inner.Children().Append(words);
        if(menu)inner.Children().Append(icon(L"chevron-down",data->theme(),glyph));
        return inner;
    }
    FrameworkElement spaced(FrameworkElement const& element){element.Margin({ControlClearance,ControlClearance,ControlClearance,ControlClearance});return element;}
    FrameworkElement segments(J const& spec){
        auto id=str(spec,L"id");auto list=array(spec,L"items");auto weak=weak_from_this();
        auto group=segmentRow(data);group.Background(data->tint(L"text",26));group.VerticalAlignment(VerticalAlignment::Center);
        AutomationProperties::SetAutomationId(group,L"canvas-bar-choice-"+id);AutomationProperties::SetName(group,str(spec,L"label"));
        for(uint32_t i=0;i<list.Size();++i){
            auto entry=list.GetObjectAt(i);auto action=object(entry,L"action");
            auto toggle=button<Primitives::ToggleButton>(data,L"",[weak,action]{if(auto self=weak.lock()){self->send(action);self->restore();}});
            toggle.Content(labelled(str(entry,L"icon"),str(entry,L"label")));worded(toggle,8);segmentColors(data,toggle);buttonColors(data,toggle,20,selected(data),.45);toggle.CornerRadius({0,0,0,0});
            AutomationProperties::SetAutomationId(toggle,L"canvas-bar-choice-"+id+L"-"+to_hstring(i));AutomationProperties::SetName(toggle,str(entry,L"label"));tooltip(toggle,str(entry,L"label"));
            group.Children().Append(toggle);
        }
        return spaced(group);
    }
    FrameworkElement dropdown(J const& spec){
        auto id=str(spec,L"id");auto weak=weak_from_this();
        Button result=button(data,L"",[weak,id]{if(auto self=weak.lock())self->openChoice(id);});worded(result);
        AutomationProperties::SetAutomationId(result,L"canvas-bar-choice-"+id);AutomationProperties::SetName(result,str(spec,L"label"));tooltip(result,str(spec,L"label"));
        return spaced(result);
    }
    FrameworkElement menuField(J const& item){
        auto id=str(item,L"menu");auto weak=weak_from_this();bool iconOnly=flag(item,L"icon_only");
        Button result=button(data,L"",[weak,id]{if(auto self=weak.lock())self->openChoice(id);});
        if(iconOnly){tiled(result);result.Content(face(str(item,L"icon"),data->theme(),true));}
        else{worded(result);result.Content(labelled(str(item,L"icon"),str(item,L"label"),true));}
        AutomationProperties::SetAutomationId(result,L"canvas-bar-menu-"+id);AutomationProperties::SetName(result,str(item,L"label"));
        auto shell=explainable(result);
        return iconOnly?FrameworkElement(shell):spaced(shell);
    }
    FrameworkElement field(J const& item){
        if(!str(item,L"menu").empty())return menuField(item);
        if(auto spec=choice(item);spec.Size())return flag(spec,L"segmented")?segments(spec):dropdown(spec);
        auto option=object(object(item,L"option"),L"Action");auto state=object(option,L"state");auto id=str(state,L"id");
        bool iconOnly=flag(item,L"icon_only"),accented=flag(item,L"accent");
        auto weak=weak_from_this();
        auto activate=[weak,id]{if(auto self=weak.lock()){self->send(O({{L"type",S(L"invoke")},{L"command",S(id)}}));self->restore();}};
        Primitives::ButtonBase result=flag(option,L"checkable")?Primitives::ButtonBase(button<Primitives::ToggleButton>(data,L"",activate)):Primitives::ButtonBase(button(data,L"",activate));
        auto iconTheme=accented?accentTheme():data->theme();
        if(iconOnly){tiled(result);result.Content(face(str(state,L"icon"),iconTheme,false));}
        else{
            worded(result);auto inner=labelled(str(state,L"icon"),str(item,L"label"),false,iconTheme);result.Content(inner);
            if(accented)inner.Children().GetAt(inner.Children().Size()-1).as<TextBlock>().Foreground(data->brush(L"accent_foreground"));
        }
        if(accented){
            result.Background(accent(data));
        }
        AutomationProperties::SetAutomationId(result,L"canvas-bar-"+id);AutomationProperties::SetName(result,str(state,L"label"));
        auto shell=explainable(result);
        return iconOnly?FrameworkElement(shell):spaced(shell);
    }
    FrameworkElement divided(FrameworkElement const& element){
        StackPanel group;group.Orientation(Orientation::Horizontal);group.Spacing(gap);
        Border slot;slot.Width(DividerSlot);slot.Height(tile);slot.Child(dividerLine(data,false));
        group.Children().Append(slot);group.Children().Append(element);
        return group;
    }
    void update(FrameworkElement const& element,J const& item){
        if(!str(item,L"menu").empty()){
            auto state=command(item);bool enabled=!state.Size()||flag(state,L"enabled");auto button=explained(element);
            explain(element.as<Grid>(),button,enabled,str(item,L"label"),str(state,L"disabled_reason"));
            return;
        }
        if(auto spec=choice(item);spec.Size()){
            auto list=array(spec,L"items");
            if(auto group=element.try_as<StackPanel>()){
                for(uint32_t i=0;i<list.Size()&&i<group.Children().Size();++i){
                    auto entry=list.GetObjectAt(i);auto toggle=group.Children().GetAt(i).as<Primitives::ToggleButton>();
                    toggle.IsChecked(flag(entry,L"selected"));tooltip(toggle,str(entry,L"disabled_reason",str(entry,L"label")));
                }
            }else if(auto picker=element.try_as<Button>()){
                J chosen=list.Size()?list.GetObjectAt(0):J{};
                for(auto value:list)if(flag(value.GetObject(),L"selected"))chosen=value.GetObject();
                if(chosen.Size()){
                    auto name=str(chosen,L"icon"),text=str(chosen,L"label"),key=name+L"|"+text+L"|"+data->theme();auto tag=picker.Tag().try_as<J>();
                    if(str(tag?tag:J{},L"canvas_bar_value")!=key){
                        if(auto content=picker.Content().try_as<StackPanel>()){
                            auto image=content.Children().GetAt(0).as<Image>();if(!name.empty())setIcon(image,name,data->theme(),glyph);image.Visibility(name.empty()?Visibility::Collapsed:Visibility::Visible);
                            content.Children().GetAt(1).as<TextBlock>().Text(text);
                        }else{StackPanel body;body.Orientation(Orientation::Horizontal);body.Spacing(6);Image image;if(!name.empty())setIcon(image,name,data->theme(),glyph);image.Visibility(name.empty()?Visibility::Collapsed:Visibility::Visible);body.Children().Append(image);auto caption=label(data,text,true);caption.VerticalAlignment(VerticalAlignment::Center);body.Children().Append(caption);body.Children().Append(icon(L"chevron-down",data->theme(),glyph));picker.Content(body);}
                        edgeTag(picker,L"canvas_bar_value",S(key));
                    }
                }
            }
            return;
        }
        auto button=explained(element);auto state=command(item);bool enabled=flag(state,L"enabled");
        explain(element.as<Grid>(),button,enabled,str(state,L"tooltip"),str(state,L"disabled_reason"));
        if(auto toggle=button.try_as<Primitives::ToggleButton>())toggle.IsChecked(flag(state,L"selected"));
    }
    void restore(std::vector<Field> const& target,J const& model){
        uint32_t index=0;
        for(auto list:{L"items",L"completion"})for(auto item:array(model,list))if(index<target.size())update(target[index++].element,item.GetObject());
    }
    void restore(){restore(fields,view);}
    hstring signature(J const& next)const{
        std::wstring key=std::wstring(object(next,L"context").Stringify())+L"|"+std::wstring(str(next,L"label"));
        for(auto list:{L"items",L"completion"})for(auto value:array(next,list)){
            auto item=value.GetObject();key+=L"|"+std::wstring(str(item,L"label"))+L":"+std::wstring(str(command(item),L"id"))
                +L":"+std::wstring(str(item,L"menu"))+L":"+std::wstring(str(item,L"icon"))+(flag(item,L"accent")?L":a":L"")
                +(flag(item,L"icon_only")?L":i":L"")+(flag(item,L"separated")?L":s":L"");
            if(auto spec=choice(item);spec.Size()){
                key+=L":"+std::wstring(str(spec,L"id"))+(flag(spec,L"segmented")?L":s":L":d");
                for(auto entry:array(spec,L"items"))key+=L","+std::wstring(str(entry.GetObject(),L"label"));
            }
        }
        return hstring(key);
    }
    std::shared_ptr<Prepared> prepare(J const& model,hstring const& key){
        auto next=std::make_shared<Prepared>();next->row.XamlRoot(host.XamlRoot());next->view=model;next->schema=key;next->theme=data->theme();
        next->row.Orientation(Orientation::Horizontal);next->row.Spacing(gap);
        next->caption=label(data,str(model,L"label"));next->caption.VerticalAlignment(VerticalAlignment::Center);next->caption.Margin({12,0,8,0});next->caption.Opacity(.8);
        next->caption.Visibility(str(model,L"label").empty()?Visibility::Collapsed:Visibility::Visible);next->row.Children().Append(next->caption);
        next->more=button(data,L"",[weak=weak_from_this()]{if(auto self=weak.lock())self->openMenu();});tiled(next->more);
        AutomationProperties::SetAutomationId(next->more,L"canvas-bar-more");copyName(data,next->more,data->copyCommon(L"more"),true);
        auto append=[&](wchar_t const* list){
            for(auto value:array(model,list)){
                auto item=value.GetObject();auto made=field(item);
                Field entry{flag(item,L"separated")?divided(made):made,made};
                next->row.Children().Append(entry.slot);next->fields.push_back(entry);
            }
        };
        append(L"items");next->row.Children().Append(next->more);append(L"completion");
        next->more.Content(face(L"more",data->theme(),false));return next;
    }
    void Apply(J const& state){
        auto value=state.GetNamedValue(L"canvas_bar",JsonValue::CreateNullValue());
        if(value.ValueType()!=JsonValueType::Object){
            if(view.Size()||pending){view=J{};pending.reset();schema=L"";placement=L"";++request;bounds=J{};menuOwner.Cancel();present(false);}
            return;
        }
        auto next=value.GetObject();auto key=signature(next);
        if(key==schema&&theme==data->theme()){pending.reset();view=next;restore();}
        else{
            if(!pending||pending->schema!=key||pending->theme!=data->theme())pending=prepare(next,key);
            else if(pending->view.Stringify()!=next.Stringify())pending->sizes=J{};
            pending->view=next;restore(pending->fields,next);
        }
        auto located=next.Stringify()+L"|"+object(data->model,L"layout").Stringify()+L"|"+data->theme();
        if(located!=placement){placement=located;place();}
    }
    J measure(std::shared_ptr<Prepared> const& prepared={}){
        auto const& measuredFields=prepared?prepared->fields:fields;auto measuredView=prepared?prepared->view:view;
        auto measuredCaption=prepared?prepared->caption:caption;auto measuredMore=prepared?prepared->more:more;
        std::vector<double> widths;
        double height=0;
        auto extent=[&](FrameworkElement const& element){element.Measure({INFINITY,INFINITY});height=std::max(height,double(element.DesiredSize().Height));return double(element.DesiredSize().Width);};
        for(auto const& item:measuredFields){item.slot.Visibility(Visibility::Visible);widths.push_back(extent(item.slot));}
        auto count=array(measuredView,L"items").Size();
        A itemWidths,completionWidths;
        for(size_t i=0;i<widths.size();++i)(i<count?itemWidths:completionWidths).Append(N(widths[i]));
        return O({{L"label",N(measuredCaption.Visibility()==Visibility::Visible?extent(measuredCaption):0)},
            {L"items",itemWidths},{L"completion",completionWidths},{L"more",N(extent(measuredMore))},
            {L"height",N(std::max(height,tile))},{L"gap",N(gap)},{L"padding",N(0)}});
    }
    void install(std::shared_ptr<Prepared> const& prepared,J const& result){
        if(prepared){
            menuOwner.Cancel();view=prepared->view;sizes=prepared->sizes;schema=prepared->schema;theme=prepared->theme;
            row=prepared->row;caption=prepared->caption;more=prepared->more;fields=prepared->fields;pending.reset();
            content.Children().SetAt(1,row);surface.Fill(data->glass(L"panel"));restore();
        }
        bounds=object(result,L"bounds");shown=size_t(num(result,L"items"));
        for(size_t i=0;i<fields.size()&&i<array(view,L"items").Size();++i)fields[i].slot.Visibility(i<shown?Visibility::Visible:Visibility::Collapsed);
        present(bounds.Size()!=0);
    }
    void place(){
        auto prepared=pending;auto model=prepared?prepared->view:view;
        if(!model.Size()){present(false);return;}
        auto& dimensions=prepared?prepared->sizes:sizes;if(!dimensions.Size())dimensions=measure(prepared);
        auto measured=J::Parse(dimensions.Stringify());measured.Insert(L"context",object(model,L"context"));
        auto query=O({{L"type",S(L"canvas_bar_layout")},{L"measure",measured}});
        auto serial=++request;auto weak=weak_from_this();
        QueryWorkspace(data->query,query,[weak,serial,prepared](J reply){
            auto self=weak.lock();if(!self||serial!=self->request)return;
            self->install(prepared,object(reply,L"result"));
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
            shadow.Shape(squircleRectangle(width,height,radii),width,height,8,2,.16f);shadow.Cut(radii);
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
        if(hidden){suppressed=true;menuOwner.Cancel();present(false);return;}
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
    FrameworkElement anchorFor(hstring const& id)const{
        for(auto const& entry:fields){
            auto const& element=entry.element;
            if(AutomationProperties::GetAutomationId(element)==L"canvas-bar-choice-"+id)return element;
            if(auto shell=element.try_as<Grid>();shell&&AutomationProperties::GetAutomationId(explained(shell))==L"canvas-bar-menu-"+id)return shell;
        }
        return nullptr;
    }
    void openPopup(J const& request,FrameworkElement const& anchor,Primitives::FlyoutPlacementMode placement){
        if(!view.Size()||!anchor)return;
        menuOwner.Cancel();menu=MenuFlyout();auto ticket=menuOwner.Begin(data,anchor,menu);
        auto weak=weak_from_this();
        if(!QueryWorkspace(data->query,request,[weak,ticket,placement,source=make_weak(anchor)](J reply){
            auto self=weak.lock();if(!self||!ticket.Current())return;
            auto sections=array(object(reply,L"result"),L"sections");if(!sections.Size()){ticket.Cancel();return;}
            NativeMenuItems(self->menu.Items(),sections,self->data,[data=self->data](J action){data->dispatch(action);});
            Primitives::FlyoutShowOptions options;options.Placement(placement);ticket.Show(source.get(),options);
        }))ticket.Cancel();
    }
    void openChoice(hstring const& id){
        openPopup(O({{L"type",S(L"canvas_bar_choice_menu")},{L"context",context()},{L"id",S(id)}}),anchorFor(id),Primitives::FlyoutPlacementMode::TopEdgeAlignedLeft);
    }
    void openMenu(){
        openPopup(O({{L"type",S(L"canvas_bar_menu")},{L"context",context()},{L"shown",N(double(shown))}}),more,Primitives::FlyoutPlacementMode::TopEdgeAlignedRight);
    }
};
}
