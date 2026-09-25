#pragma once
#include "UiControls.h"
#include <array>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

namespace CapyUi {
using Windows::Foundation::Point;
using Windows::Foundation::Rect;
inline Rect rectangle(J const& value){
    return {float(num(value,L"x")),float(num(value,L"y")),float(num(value,L"width")),float(num(value,L"height"))};
}
inline J rectangle(Rect value){
    return O({{L"x",N(value.X)},{L"y",N(value.Y)},{L"width",N(value.Width)},{L"height",N(value.Height)}});
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
inline int visualOrder(Canvas const& root,FrameworkElement const& element){
    uint32_t index=0;root.Children().IndexOf(element,index);
    return Canvas::GetZIndex(element)*16384+int(index);
}
inline J panelStructure(J const& panel){
    A keys;
    for(auto value:array(panel,L"tiles")){auto tile=value.GetObject();keys.Append(O({{L"id",N(num(tile,L"id"))},{L"control",object(tile,L"control")}}));}
    return O({{L"id",S(str(panel,L"id"))},{L"controls",array(panel,L"controls")},{L"style",S(str(panel,L"tile_style"))},{L"tiles",keys}});
}
inline bool appendGlass(A& regions,FrameworkElement const& element,UIElement const& reference,std::array<double,4> radii,bool squircle=false){
    if(!element||!element.IsLoaded()||element.ActualWidth()<=0||element.ActualHeight()<=0)return false;
    for(auto node=element.as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node))
        if(auto item=node.try_as<UIElement>();item&&(item.Visibility()!=Visibility::Visible||item.Opacity()<=0))return false;
    auto box=element.TransformToVisual(reference).TransformBounds({0,0,float(element.ActualWidth()),float(element.ActualHeight())});
    A region;for(double v:{double(box.X),double(box.Y),double(box.Width),double(box.Height),radii[0],radii[1],radii[2],radii[3],squircle?1.:0.})region.Append(N(v));
    regions.Append(region);return true;
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
inline std::array<bool,4> sourceCorners(J const& source,J const& container){
    std::array<bool,4> result{};
    auto request=O({{L"type",S(L"drawer_source_corners")},{L"anchor",object(source,L"bounds")},{L"direction",S(str(source,L"direction"))},{L"container",container}});
    std::unique_ptr<char,decltype(&capy_string_free)> reply(capy_toolbar_ui(to_string(request.Stringify()).c_str()),capy_string_free);
    if(!reply)return result;auto value=JsonValue::Parse(to_hstring(reply.get()));if(value.ValueType()!=JsonValueType::Array)return result;
    auto list=value.GetArray();for(uint32_t i=0;i<4&&i<list.Size();++i)result[i]=list.GetBooleanAt(i);
    return result;
}
struct AutomaticTab{Button tab{nullptr};TextBlock name{nullptr};std::wstring key;};
inline bool automaticTabs(std::shared_ptr<WorkspaceData> const& data,double group){
    return str(object(data->model,L"windows_tab_styles"),to_hstring(uint32_t(group)).c_str())==L"automatic";
}
inline void fitAutomaticTabs(std::vector<AutomaticTab> const& tabs,double available,std::map<std::wstring,double>& widths){
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
    std::unique_ptr<char,decltype(&capy_string_free)> reply(capy_toolbar_ui(to_string(request.Stringify()).c_str()),capy_string_free);
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
constexpr float SurfaceRadius=18,CornerFit=.54f;
inline void squircleCorner(PointCollection const& points,Point center,Point start,Point end){
    for(int i=0;i<=24;++i){
        double angle=i*3.14159265358979/48,along=std::sqrt(std::cos(angle)),across=std::sqrt(std::sin(angle));
        points.Append({float(center.X+start.X*along+end.X*across),float(center.Y+start.Y*along+end.Y*across)});
    }
}
inline PathGeometry squircleRectangle(float width,float height,std::array<float,4> radii){
    for(auto& radius:radii)radius=std::min(radius,std::max(0.f,std::min(width,height)*.5f));
    auto [tl,tr,br,bl]=radii;
    PathFigure figure;figure.StartPoint({tl,0});figure.IsClosed(true);figure.IsFilled(true);
    PolyLineSegment outline;auto points=outline.Points();
    squircleCorner(points,{width-tr,tr},{0,-tr},{tr,0});
    squircleCorner(points,{width-br,height-br},{br,0},{0,br});
    squircleCorner(points,{bl,height-bl},{0,bl},{-bl,0});
    squircleCorner(points,{tl,tl},{-tl,0},{0,-tl});
    figure.Segments().Append(outline);PathGeometry geometry;geometry.Figures().Append(figure);return geometry;
}
// The 6-DIP shoulders join the active tab to its panel. Coordinates include
// both overhangs so a Path never has to arrange a negative geometry origin.
inline PathGeometry panelTabShape(float width,float height){
    constexpr float foot=6;float r=std::min({SurfaceRadius,width*.5f,height-foot});
    PathFigure figure;figure.StartPoint({0,height});figure.IsClosed(true);figure.IsFilled(true);
    PolyLineSegment outline;auto points=outline.Points();
    squircleCorner(points,{0,height-foot},{0,foot},{foot,0});
    squircleCorner(points,{foot+r,r},{-r,0},{0,-r});
    squircleCorner(points,{foot+width-r,r},{0,-r},{r,0});
    squircleCorner(points,{2*foot+width,height-foot},{-foot,0},{0,foot});
    figure.Segments().Append(outline);PathGeometry geometry;geometry.Figures().Append(figure);return geometry;
}
inline Grid panelTabShell(FrameworkElement const& content,Brush const& fill){
    Grid shell;shell.Tag(box_value(fill?L"active-panel-tab-shell":L"panel-tab-shell"));
    Canvas background;shell.Children().Append(background);
    if(fill){
        Microsoft::UI::Xaml::Shapes::Path shape;shape.Fill(fill);shape.IsHitTestVisible(false);
        shape.Stretch(Stretch::Fill);Canvas::SetLeft(shape,-6);background.Children().Append(shape);
        content.SizeChanged([weak=make_weak(shape)](auto&&,SizeChangedEventArgs const& event){
            auto size=event.NewSize();if(auto shape=weak.get();shape&&size.Width>0&&size.Height>0){
                shape.Width(size.Width+12);shape.Height(size.Height);shape.Data(panelTabShape(size.Width,size.Height));
            }
        });
    }
    shell.Children().Append(content);return shell;
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
