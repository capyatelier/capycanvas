#pragma once
#include "UiControls.h"
#include <array>

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
    auto length=num(value,L"length"),depth=num(value,L"depth"),r0=radii.GetNumberAt(0),r1=radii.GetNumberAt(1);
    constexpr double k=.5522848;
    PathFigure figure;figure.StartPoint(at(0,0));figure.IsClosed(true);figure.IsFilled(true);
    auto line=[&](double x,double y){LineSegment segment;segment.Point(at(x,y));figure.Segments().Append(segment);};
    auto curve=[&](double x1,double y1,double x2,double y2,double x3,double y3){
        BezierSegment segment;segment.Point1(at(x1,y1));segment.Point2(at(x2,y2));segment.Point3(at(x3,y3));figure.Segments().Append(segment);
    };
    line(length,0);line(length,depth-r1);curve(length,depth-r1+r1*k,length+r1-r1*k,depth,length+r1,depth);
    line(-r0,depth);curve(-r0+r0*k,depth,0,depth-r0+r0*k,0,depth-r0);
    geometry.Figures().Append(figure);return geometry;
}
}
