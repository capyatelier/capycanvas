#pragma once
#include "UiControls.h"
#include <array>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

namespace CapyUi {
using winrt::Windows::Foundation::Point;
using winrt::Windows::Foundation::Rect;
inline Rect rectangle(J const& value){
    return {float(num(value,L"x")),float(num(value,L"y")),float(num(value,L"width")),float(num(value,L"height"))};
}
inline J rectangle(Rect value){
    return O({{L"x",N(value.X)},{L"y",N(value.Y)},{L"width",N(value.Width)},{L"height",N(value.Height)}});
}
inline V toolbarUi(CapyLocalization  localization,J const& request){
    NativeString result(capy_toolbar_ui(localization,to_string(request.Stringify()).c_str()),capy_string_free);
    if(!result)throw hresult_invalid_argument(L"Toolbar request failed");
    auto value=JsonValue::Parse(to_hstring(result.get()));
    if(value.ValueType()==JsonValueType::Object&&value.GetObject().HasKey(L"error"))
        throw hresult_invalid_argument(value.GetObject().GetNamedString(L"error"));
    return value;
}
inline Rect intersect(Rect a,Rect b){
    float x=std::max(a.X,b.X),y=std::max(a.Y,b.Y);
    return {x,y,std::max(0.f,std::min(a.X+a.Width,b.X+b.Width)-x),std::max(0.f,std::min(a.Y+a.Height,b.Y+b.Height)-y)};
}
inline Rect visibleBounds(FrameworkElement const& element,FrameworkElement const& reference){
    if(!element.IsLoaded())return {};
    auto bounds=element.TransformToVisual(reference).TransformBounds({0,0,float(element.ActualWidth()),float(element.ActualHeight())});
    auto parent=element.as<DependencyObject>();
    while(parent){
        if(auto item=parent.try_as<FrameworkElement>()){
            if(item.Visibility()!=Visibility::Visible)return {};
            auto box=item.TransformToVisual(reference).TransformBounds({0,0,float(item.ActualWidth()),float(item.ActualHeight())});
            bounds=intersect(bounds,box);
        }
        if(parent==reference)return bounds;
        parent=VisualTreeHelper::GetParent(parent);
    }
    return {};
}
inline Rect allocatedBounds(FrameworkElement const& element,UIElement const& reference){
    if(!element||!element.IsLoaded()||element.Visibility()!=Visibility::Visible||element.ActualWidth()<=0||element.ActualHeight()<=0)return {};
    return element.TransformToVisual(reference).TransformBounds({0,0,float(element.ActualWidth()),float(element.ActualHeight())});
}
inline int visualOrder(Canvas const& root,FrameworkElement const& element){
    uint32_t index=0;root.Children().IndexOf(element,index);
    return Canvas::GetZIndex(element)*16384+int(index);
}
inline J panelStructure(J const& panel){
    A keys,controls;
    for(auto value:array(panel,L"controls")){auto control=value.GetObject();controls.Append(O({{L"control",control.GetNamedValue(L"control")},{L"visible_in_panel",B(flag(control,L"visible_in_panel",true))}}));}
    for(auto value:array(panel,L"tiles")){auto tile=value.GetObject();keys.Append(O({{L"id",N(num(tile,L"id"))},{L"control",object(tile,L"control")},{L"slider_style",object(tile,L"slider_style")}}));}
    return O({{L"id",S(str(panel,L"id"))},{L"controls",controls},{L"style",S(str(panel,L"tile_style"))},{L"tiles",keys}});
}
inline bool appendGlass(A& regions,FrameworkElement const& element,UIElement const& reference,std::array<double,4> radii,bool squircle=false){
    if(!element||!element.IsLoaded()||element.ActualWidth()<=0||element.ActualHeight()<=0)return false;
    double opacity=1.;
    for(auto node=element.as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node))
        if(auto item=node.try_as<UIElement>()){if(item.Visibility()!=Visibility::Visible||item.Opacity()<=0)return false;opacity*=item.Opacity();}
    auto box=element.TransformToVisual(reference).TransformBounds({0,0,float(element.ActualWidth()),float(element.ActualHeight())});
    A region;for(double v:{double(box.X),double(box.Y),double(box.Width),double(box.Height),radii[0],radii[1],radii[2],radii[3],squircle?1.:0.})region.Append(N(v));
    region.Append(N(opacity));regions.Append(region);return true;
}
inline std::array<double,4> cornerRadii(CornerRadius const& r){return {r.TopLeft,r.TopRight,r.BottomRight,r.BottomLeft};}
inline void appendConnection(A& connections,J const& connection,UIElement const& workspace,UIElement const& reference){
    if(!connection.Size())return;auto origin=workspace.TransformToVisual(reference).TransformPoint({0,0});
    auto moved=J::Parse(connection.Stringify());auto bounds=object(moved,L"bounds");
    bounds.Insert(L"x",N(num(bounds,L"x")+origin.X));bounds.Insert(L"y",N(num(bounds,L"y")+origin.Y));
    moved.Insert(L"bounds",bounds);connections.Append(moved);
}
inline hstring drawerFacing(std::shared_ptr<WorkspaceData> const& data,J const& anchor){
    for(auto value:data->drawerSources){
        auto source=value.GetObject();auto candidate=object(source,L"anchor");bool same=str(candidate,L"kind")==str(anchor,L"kind");
        for(auto key:{L"panel"})same=same&&str(candidate,key)==str(anchor,key);
        for(auto key:{L"tile",L"id",L"column"})same=same&&num(candidate,key,-1)==num(anchor,key,-1);
        if(same)return str(source,L"direction");
    }
    return L"";
}
inline CornerRadius facingCorners(double r,hstring const& direction){
    CornerRadius c{r,r,r,r};
    if(direction==L"top")c.TopLeft=c.TopRight=0;else if(direction==L"bottom")c.BottomLeft=c.BottomRight=0;
    else if(direction==L"left")c.TopLeft=c.BottomLeft=0;else if(direction==L"right")c.TopRight=c.BottomRight=0;
    return c;
}
inline std::array<bool,4> sourceCorners(std::shared_ptr<WorkspaceData> const& data,J const& source,J const& container){
    std::array<bool,4> result{};
    auto request=O({{L"type",S(L"drawer_source_corners")},{L"anchor",object(source,L"bounds")},{L"direction",S(str(source,L"direction"))},{L"container",container}});
    NativeString reply(capy_toolbar_ui(data->localization.get(),to_string(request.Stringify()).c_str()),capy_string_free);
    if(!reply)return result;auto value=JsonValue::Parse(to_hstring(reply.get()));if(value.ValueType()!=JsonValueType::Array)return result;
    auto list=value.GetArray();for(uint32_t i=0;i<4&&i<list.Size();++i)result[i]=list.GetBooleanAt(i);
    return result;
}
struct AutomaticTab{Button tab{nullptr};TextBlock name{nullptr};std::wstring key;};
inline bool automaticTabs(std::shared_ptr<WorkspaceData> const& data,double group){
    return str(object(data->model,L"windows_tab_styles"),to_hstring(uint32_t(group)).c_str())==L"automatic";
}
inline void fitAutomaticTabs(std::shared_ptr<WorkspaceData> const& data,std::vector<AutomaticTab> const& tabs,double available,std::map<std::wstring,double>& widths){
    if(tabs.empty()||available<=0)return;
    A pairs;
    for(auto const& t:tabs){
        auto found=widths.find(t.key);
        if(found==widths.end()){
            auto shown=t.name.Visibility();t.name.Visibility(Visibility::Visible);
            t.tab.Measure({std::numeric_limits<float>::infinity(),std::numeric_limits<float>::infinity()});
            found=widths.emplace(t.key,t.tab.DesiredSize().Width).first;t.name.Visibility(shown);
        }
        A pair;pair.Append(N(found->second));pair.Append(N(36));pairs.Append(pair);
    }
    auto request=O({{L"type",S(L"automatic_tab_names")},{L"available",N(available)},{L"widths",pairs}});
    NativeString reply(capy_toolbar_ui(data->localization.get(),to_string(request.Stringify()).c_str()),capy_string_free);
    if(!reply)return;auto names=JsonValue::Parse(to_hstring(reply.get()));if(names.ValueType()!=JsonValueType::Array)return;
    auto list=names.GetArray();
    for(uint32_t i=0;i<tabs.size()&&i<list.Size();++i)tabs[i].name.Visibility(list.GetBooleanAt(i)?Visibility::Visible:Visibility::Collapsed);
}
inline PathGeometry roundedRectangle(float width,float height,std::array<float,4> radii){
    for(auto& radius:radii)radius=std::min(radius,std::max(0.f,std::min(width,height)*.5f));
    auto [tl,tr,br,bl]=radii;
    PathFigure figure;figure.StartPoint({tl,0});figure.IsClosed(true);figure.IsFilled(true);
    auto line=[&](Point p){LineSegment segment;segment.Point(p);figure.Segments().Append(segment);};
    auto arc=[&](Point p,float radius){
        if(!radius){line(p);return;}
        ArcSegment segment;segment.Point(p);segment.Size({radius,radius});segment.SweepDirection(SweepDirection::Clockwise);
        figure.Segments().Append(segment);
    };
    line({width-tr,0});arc({width,tr},tr);line({width,height-br});arc({width-br,height},br);
    line({bl,height});arc({0,height-bl},bl);line({0,tl});arc({tl,0},tl);
    PathGeometry geometry;geometry.Figures().Append(figure);return geometry;
}
constexpr float SurfaceRadius=18;
constexpr double DividerSlot=8,DividerLength=18,ControlClearance=5;
inline void orientDivider(FrameworkElement const& line,bool horizontal){
    line.Width(horizontal?DividerLength:1);line.Height(horizontal?1:DividerLength);
}
inline Border dividerLine(std::shared_ptr<WorkspaceData> const& data,bool horizontal){
    Border line;line.Background(data->brush(L"settings_secondary"));line.Opacity(.3);line.IsHitTestVisible(false);
    line.HorizontalAlignment(HorizontalAlignment::Center);line.VerticalAlignment(VerticalAlignment::Center);
    orientDivider(line,horizontal);return line;
}
inline void squircleCorner(PointCollection const& points,Point center,Point start,Point end){
    for(int i=0;i<=24;++i){
        double angle=i*3.14159265358979/48,along=std::sqrt(std::cos(angle)),across=std::sqrt(std::sin(angle));
        points.Append({float(center.X+start.X*along+end.X*across),float(center.Y+start.Y*along+end.Y*across)});
    }
}
inline std::vector<Point> squircleOutline(float width,float height,std::array<float,4> radii){
    for(auto& radius:radii)radius=std::min(radius,std::max(0.f,std::min(width,height)*.5f));
    auto [tl,tr,br,bl]=radii;
    PointCollection points;
    squircleCorner(points,{width-tr,tr},{0,-tr},{tr,0});
    squircleCorner(points,{width-br,height-br},{br,0},{0,br});
    squircleCorner(points,{bl,height-bl},{0,bl},{-bl,0});
    squircleCorner(points,{tl,tl},{-tl,0},{0,-tl});
    std::vector<Point> outline(points.Size());points.GetMany(0,outline);return outline;
}
inline PathGeometry polygon(std::vector<Point> const& points){
    PathGeometry geometry;if(points.empty())return geometry;
    PathFigure figure;figure.StartPoint(points.front());figure.IsClosed(true);figure.IsFilled(true);
    PolyLineSegment outline;auto line=outline.Points();for(auto point=points.begin()+1;point!=points.end();++point)line.Append(*point);
    figure.Segments().Append(outline);geometry.Figures().Append(figure);return geometry;
}
inline PathGeometry squircleRectangle(float width,float height,std::array<float,4> radii){return polygon(squircleOutline(width,height,radii));}
inline std::vector<Point> clipColumns(std::vector<Point> points,float left,float right){
    for(auto [edge,side]:{std::pair{left,1.f},std::pair{right,-1.f}}){
        std::vector<Point> kept;
        for(size_t i=0;i<points.size();++i){
            auto a=points[i],b=points[(i+1)%points.size()];bool inside=(a.X-edge)*side>=0;
            if(inside)kept.push_back(a);
            if(inside!=((b.X-edge)*side>=0))kept.push_back({edge,a.Y+(edge-a.X)/(b.X-a.X)*(b.Y-a.Y)});
        }
        points=std::move(kept);
    }
    return points;
}
// The 6-DIP shoulders join the active tab to its panel. Coordinates include
// both overhangs so a Path never has to arrange a negative geometry origin.
inline std::vector<Point> panelTabOutline(float width,float height,std::array<float,2> bottom={-6,-6}){
    auto [left,right]=bottom;float pad=std::max(0.f,-left),r=std::min({SurfaceRadius,width*.5f,height-6});
    PointCollection points;
    squircleCorner(points,{pad+left,height-std::abs(left)},{0,std::abs(left)},{-left,0});
    squircleCorner(points,{pad+r,r},{-r,0},{0,-r});
    squircleCorner(points,{pad+width-r,r},{0,-r},{r,0});
    squircleCorner(points,{pad+width-right,height-std::abs(right)},{right,0},{0,std::abs(right)});
    std::vector<Point> outline(points.Size());points.GetMany(0,outline);return outline;
}
inline PathGeometry panelTabShape(float width,float height){return polygon(panelTabOutline(width,height));}
inline void selectPanelTabShell(Grid const& shell,bool selected,Brush const& fill=nullptr,J const& presentation=J{}){
    shell.Tag(box_value(selected?L"active-panel-tab-shell":L"panel-tab-shell"));Canvas::SetZIndex(shell,selected?1:0);
    auto background=shell.Children().GetAt(0).as<Canvas>();auto shape=background.Children().GetAt(0).as<Microsoft::UI::Xaml::Shapes::Path>();
    shape.Fill(selected?fill:unbox_value_or<bool>(background.Tag(),false)?shape.Tag().try_as<Brush>():Brush(nullptr));
    if(presentation.Size()){
        auto bottom=array(presentation,L"hover_bottom");
        shape.Resources().Insert(box_value(L"TabHoverBottom"),box_value(Thickness{bottom.GetNumberAt(0),0,bottom.GetNumberAt(1),0}));
    }
    auto key=box_value(L"TabHoverBottom");auto corners=shape.Resources().HasKey(key)?unbox_value<Thickness>(shape.Resources().Lookup(key)):Thickness{};
    std::array<float,2> bottom=selected?std::array<float,2>{-6,-6}:std::array<float,2>{float(corners.Left),float(corners.Right)};
    Canvas::SetLeft(shape,std::min(0.f,bottom[0]));
    auto content=shell.Children().GetAt(1).as<FrameworkElement>();float width=float(content.ActualWidth()),height=float(content.ActualHeight());
    if(width>0&&height>0){shape.Width(width-std::min(0.f,bottom[0])-std::min(0.f,bottom[1]));shape.Height(height);shape.Data(polygon(panelTabOutline(width,height,bottom)));}
}
inline Grid panelTabShell(FrameworkElement const& content,Brush const& fill,Brush const& hover=nullptr){
    if(auto tab=content.try_as<Button>())for(auto state:{L"ButtonBackgroundPointerOver",L"ButtonBackgroundPressed"})
        tab.Resources().Insert(box_value(hstring(state)),clear());
    Grid shell;shell.Tag(box_value(fill?L"active-panel-tab-shell":L"panel-tab-shell"));
    Canvas background;shell.Children().Append(background);
    Microsoft::UI::Xaml::Shapes::Path shape;shape.Tag(hover);shape.Fill(fill);shape.IsHitTestVisible(false);
    shape.Stretch(Stretch::Fill);background.Children().Append(shape);shell.Children().Append(content);
    content.SizeChanged([weak=make_weak(shell)](auto&&,auto&&){if(auto shell=weak.get()){
        auto shape=shell.Children().GetAt(0).as<Canvas>().Children().GetAt(0).as<Microsoft::UI::Xaml::Shapes::Path>();
        selectPanelTabShell(shell,unbox_value_or<hstring>(shell.Tag(),L"")==L"active-panel-tab-shell",shape.Fill());
    }});
    shell.PointerEntered([weak=make_weak(shell),hover](auto&&,auto&&){if(auto shell=weak.get()){
        auto background=shell.Children().GetAt(0).as<Canvas>();background.Tag(box_value(true));
        if(unbox_value_or<hstring>(shell.Tag(),L"")!=L"active-panel-tab-shell")background.Children().GetAt(0).as<Microsoft::UI::Xaml::Shapes::Path>().Fill(hover);
    }});
    shell.PointerExited([weak=make_weak(shell)](auto&&,auto&&){if(auto shell=weak.get()){
        auto background=shell.Children().GetAt(0).as<Canvas>();background.Tag(box_value(false));
        if(unbox_value_or<hstring>(shell.Tag(),L"")!=L"active-panel-tab-shell")background.Children().GetAt(0).as<Microsoft::UI::Xaml::Shapes::Path>().Fill(nullptr);
    }});
    return shell;
}
// Same joined outline as the shared-layout Android expansion. The two
// native child frames clip their own content; this path paints the join.
inline PathGeometry expansionShape(J const& value){
    auto preview=rectangle(object(value,L"preview")),configuration=rectangle(object(value,L"configuration"));
    auto bounds=rectangle(object(value,L"bounds"));float width=bounds.Width,height=bounds.Height;
    if(configuration.Y<=0)return roundedRectangle(width,height,{8,8,8,8});
    float left=preview.X,right=left+preview.Width,top=configuration.Y;
    float radius=std::min({8.f,height*.5f,width*.5f});
    PathFigure figure;figure.StartPoint({left+radius,0});figure.IsClosed(true);figure.IsFilled(true);
    auto line=[&](float x,float y){LineSegment segment;segment.Point({x,y});figure.Segments().Append(segment);};
    auto curve=[&](float x1,float y1,float x2,float y2){
        QuadraticBezierSegment segment;segment.Point1({x1,y1});segment.Point2({x2,y2});figure.Segments().Append(segment);
    };
    line(right-radius,0);curve(right,0,right,radius);
    if(right<width){line(right,top);line(width-radius,top);curve(width,top,width,top+radius);}
    line(width,height-radius);curve(width,height,width-radius,height);
    line(radius,height);curve(0,height,0,height-radius);
    if(left>0){
        line(0,top+radius);curve(0,top,radius,top);
        if(flag(value,L"concave_join")){line(left-radius,top);curve(left,top,left,top-radius);}
        else line(left,top);
    }
    line(left,radius);curve(left,0,left+radius,0);
    PathGeometry geometry;geometry.Figures().Append(figure);return geometry;
}
inline PathGeometry drawerBridge(J const& value){
    auto transform=array(value,L"transform"),radii=array(value,L"radii");
    PathGeometry geometry;
    if(transform.Size()!=6||radii.Size()!=2)return geometry;
    auto at=[&](double x,double y){return Point{
        float(transform.GetNumberAt(0)*x+transform.GetNumberAt(2)*y+transform.GetNumberAt(4)),
        float(transform.GetNumberAt(1)*x+transform.GetNumberAt(3)*y+transform.GetNumberAt(5))};};
    auto length=float(num(value,L"length")),depth=float(num(value,L"depth")),r0=float(radii.GetNumberAt(0)),r1=float(radii.GetNumberAt(1));
    PathFigure figure;figure.StartPoint(at(0,0));figure.IsClosed(true);figure.IsFilled(true);
    PointCollection local;local.Append({length,0});
    squircleCorner(local,{length+r1,depth-r1},{-r1,0},{0,r1});
    squircleCorner(local,{-r0,depth-r0},{0,r0},{r0,0});
    PolyLineSegment outline;for(auto p:local)outline.Points().Append(at(p.X,p.Y));
    figure.Segments().Append(outline);geometry.Figures().Append(figure);return geometry;
}
}
