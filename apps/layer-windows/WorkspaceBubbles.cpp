#include "pch.h"
#include "BubblePresentation.h"
#include "NativeJson.h"
#include "ToolView.h"
#include "Checker.h"
#include "SwatchFace.h"
#include "ColorEditor.h"
#include <numbers>
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>
#include <winrt/Microsoft.UI.Xaml.Hosting.h>
#include <winrt/Microsoft.UI.Xaml.Documents.h>
#include <winrt/Microsoft.UI.Xaml.Markup.h>

using namespace CapyBubbles;
using Microsoft::UI::Xaml::Hosting::ElementCompositionPreview;
namespace {
void remove(Canvas const& root,UIElement const& item){uint32_t at;if(root.Children().IndexOf(item,at))root.Children().RemoveAt(at);}
Geometry outline(Size size,double radius,bool squircle){
    std::array<float,4> radii; radii.fill(float(radius));
    return squircle?squircleRectangle(size.Width,size.Height,radii):roundedRectangle(size.Width,size.Height,radii);
}
PathGeometry colorOutline(J const& color){
    auto bounds=rectangle(object(color,L"bounds"));PathGeometry geometry;PathFigure figure;bool first=true;
    for(auto value:array(color,L"outline")){
        auto a=value.GetObject();auto center=point(array(a,L"center"));auto radius=num(a,L"radius"),start=num(a,L"start"),sweep=num(a,L"sweep");
        auto at=[&](double fraction){auto angle=start+sweep*fraction;return Point{float(center.X+radius*std::cos(angle)-bounds.X),float(center.Y+radius*std::sin(angle)-bounds.Y)};};
        if(first){figure.StartPoint(at(0));first=false;}else{LineSegment line;line.Point(at(0));figure.Segments().Append(line);}
        int steps=int(std::ceil(std::abs(sweep)/(std::numbers::pi/2)));
        for(int i=1;i<=steps;++i){ArcSegment segment;segment.Point(at(double(i)/steps));segment.Size({float(radius),float(radius)});segment.SweepDirection(sweep>=0?SweepDirection::Clockwise:SweepDirection::Counterclockwise);figure.Segments().Append(segment);}
    }
    figure.IsClosed(true);geometry.Figures().Append(figure);return geometry;
}
Geometry surface(Record const& record,Part const& part,bool hit=false){
    auto color=object(record.packet,L"color");
    if(part.key==L"face"&&color.Size())return colorOutline(color);
    return outline(part.size,hit&&part.key==L"face"&&str(record.view,L"kind")==L"layers"?0:part.radius,part.squircle);
}
void place(FrameworkElement const& view,Point center,Size size){
    view.Width(size.Width);view.Height(size.Height);Canvas::SetLeft(view,center.X-size.Width/2);Canvas::SetTop(view,center.Y-size.Height/2);
}
Shapes::Path shape(Geometry const& geometry,Brush const& fill){Shapes::Path p;p.Data(geometry);p.Fill(fill);p.IsHitTestVisible(false);return p;}
PathGeometry arc(double radius,double start,double sweep){
    auto at=[&](double degrees){auto angle=degrees*std::numbers::pi/180;return Point{float(radius*std::cos(angle)),float(radius*std::sin(angle))};};
    PathGeometry geometry;PathFigure figure;figure.StartPoint(at(start));figure.IsFilled(false);
    ArcSegment segment;segment.Point(at(start+sweep));segment.Size({float(radius),float(radius)});segment.IsLargeArc(std::abs(sweep)>180);segment.SweepDirection(sweep>=0?SweepDirection::Clockwise:SweepDirection::Counterclockwise);
    figure.Segments().Append(segment);geometry.Figures().Append(figure);return geometry;
}
void dot(Canvas const& root,Point center,double radius,Brush const& ink,Brush const& edge=nullptr){
    Shapes::Ellipse circle;circle.Fill(ink);if(edge){circle.Stroke(edge);circle.StrokeThickness(1);}place(circle,center,{float(radius*2),float(radius*2)});circle.IsHitTestVisible(false);root.Children().Append(circle);
}
void iconAt(Canvas const& root,std::shared_ptr<WorkspaceData> const& data,hstring const& name,Point center,double size){auto image=icon(name,data->theme(),size);place(image,center,{float(size),float(size)});image.IsHitTestVisible(false);root.Children().Append(image);}
void previewAt(Canvas const& root,std::shared_ptr<WorkspaceData> const& data,int id,Point center,Size size,double opacity=1){Image image;image.Source(BrushPreview(id,data->theme()));image.Stretch(Stretch::Fill);image.Opacity(opacity);image.IsHitTestVisible(false);place(image,center,size);root.Children().Append(image);}
void retire(Part& part){
    if(part.retired||!part.inspector)return;part.retired=true;
    auto visual=ElementCompositionPreview::GetElementVisual(part.inspector->root);auto compositor=visual.Compositor();
    auto surface=compositor.CreateVisualSurface();surface.SourceVisual(visual);surface.SourceSize({part.size.Width,part.size.Height});
    auto snapshot=compositor.CreateSpriteVisual();snapshot.Size({part.size.Width,part.size.Height});snapshot.Brush(compositor.CreateSurfaceBrush(surface));
    part.art.Children().Clear();ElementCompositionPreview::SetElementChildVisual(part.art,snapshot);
}
void shadow(Record const& record,Part& part){
    if(part.key==L"face"&&str(record.view,L"kind")==L"layers"){part.shadow.Children().Clear();return;}
    bool bead=prefix(part.key,L"bead:")&&!part.glass;auto reach=bead?3.:record.lifted?12.:8.;auto drop=bead?1.:record.lifted?4.5:2.;auto opacity=bead?.13:record.lifted?.16:.08;
    auto color=part.key==L"face"?object(record.packet,L"color"):J{};
    auto key=to_hstring(part.size.Width)+L":"+to_hstring(part.size.Height)+L":"+to_hstring(part.radius)+L":"+to_hstring(reach)+array(color,L"outline").Stringify();
    if(part.shadowKey==key)return;part.shadowKey=key;part.shadow.Children().Clear();
    if(color.Size()){
        ElementCompositionPreview::GetElementVisual(part.shadow).Clip(nullptr);
        if(!part.colorShadow)part.colorShadow=std::make_unique<WorkspaceShadow>();
        auto bounds=rectangle(object(color,L"bounds"));std::vector<Point> contour;
        for(auto value:array(color,L"outline")){auto a=value.GetObject();auto center=point(array(a,L"center"));auto radius=num(a,L"radius"),start=num(a,L"start"),sweep=num(a,L"sweep");auto steps=std::max(1,int(std::ceil(std::abs(sweep)*radius)));
            for(int i=0;i<=steps;++i){auto angle=start+sweep*i/steps;contour.push_back({float(center.X+radius*std::cos(angle)-bounds.X),float(center.Y+radius*std::sin(angle)-bounds.Y)});}}
        part.colorShadow->Shape(colorOutline(color),part.size.Width,part.size.Height,float(reach),float(drop),float(opacity));
        part.colorShadow->Cut(std::move(contour));part.colorShadow->Layout({0,0,part.size.Width,part.size.Height},0,true);part.shadow.Children().Append(part.colorShadow->Root());return;
    }
    part.colorShadow.reset();
    auto width=double(part.size.Width),height=double(part.size.Height),radius=std::min(width,height)/2;
    if(part.key==L"card")radius=std::min(radius,part.radius);
    ClipOutside(part.shadow,part.size.Width,part.size.Height,{float(radius),float(radius),float(radius),float(radius)},false);
    auto stops=[&](auto const& brush,double edge,double end,bool reverse=false){
        for(int i=0;i<=4;++i){auto t=(reverse?4-i:i)*.25;GradientStop stop;stop.Offset(edge+(end-edge)*t);stop.Color({uint8_t(std::round(255*opacity*(1-t)*(1-t))),0,0,0});brush.GradientStops().Append(stop);}
    };
    auto patch=[&](Brush const& brush,double x,double y,double width,double height){
        Shapes::Rectangle area;area.Fill(brush);area.IsHitTestVisible(false);area.Width(width);area.Height(height);Canvas::SetLeft(area,x);Canvas::SetTop(area,y);part.shadow.Children().Append(area);
    };
    auto extent=radius+reach;
    for(auto right:{false,true})for(auto bottom:{false,true}){
        RadialGradientBrush gradient;Point center{right?0.f:1.f,bottom?0.f:1.f};gradient.Center(center);gradient.GradientOrigin(center);gradient.RadiusX(1);gradient.RadiusY(1);stops(gradient,radius/extent,1);
        patch(gradient,right?width-radius:-reach,(bottom?height-radius:-reach)+drop,extent,extent);
    }
    if(width>2*radius){LinearGradientBrush gradient;gradient.StartPoint({0,0});gradient.EndPoint({0,1});auto edge=reach/(height+2*reach);stops(gradient,edge,0,true);stops(gradient,1-edge,1);patch(gradient,radius,drop-reach,width-2*radius,height+2*reach);}
    if(height>2*radius){LinearGradientBrush gradient;gradient.StartPoint({0,0});gradient.EndPoint({1,0});auto edge=reach/(width+2*reach);stops(gradient,edge,0,true);stops(gradient,1-edge,1);patch(gradient,-reach,radius+drop,width+2*reach,height-2*radius);}
}
ControlTemplate bubbleTemplate(){
    static auto value=Markup::XamlReader::Load(L"<ControlTemplate xmlns='http://schemas.microsoft.com/winfx/2006/xaml/presentation' TargetType='Button'><ContentPresenter Content='{TemplateBinding Content}'/></ControlTemplate>").as<ControlTemplate>();return value;
}
Button bubbleButton(){
    Button button;button.Template(bubbleTemplate());button.Padding({0});button.BorderThickness({0});button.Background(nullptr);button.MinWidth(0);button.MinHeight(0);button.UseSystemFocusVisuals(true);return button;
}
Size textSize(std::shared_ptr<WorkspaceData> const& data,hstring const& text,bool count=false,bool tabular=false){auto metrics=label(data,text,false,false);if(count)metrics.FontWeight(Windows::UI::Text::FontWeights::SemiBold());if(tabular)Documents::Typography::SetNumeralAlignment(metrics,FontNumeralAlignment::Tabular);metrics.Measure({INFINITY,INFINITY});return metrics.DesiredSize();}
}
void Record::place(){
    Canvas::SetLeft(hints,reach);Canvas::SetTop(hints,reach);
    root.Width(reach*2);root.Height(reach*2);transform.TranslateX(center.X-reach);transform.TranslateY(center.Y-reach);transform.CenterX(reach);transform.CenterY(reach);transform.ScaleX(scale());transform.ScaleY(scale());root.Opacity(alpha());
}
void Part::place(Point at,Size extent,double alpha,bool animate){
    if(at==toCenter&&extent==toSize&&alpha==toStrength){if(!animate){layout.snap(1);center=at;size=extent;strength=alpha;}return;}
    fromCenter=center;toCenter=at;fromSize=size;toSize=extent;fromStrength=strength;toStrength=alpha;
    layout.snap(animate&&size.Width>0?0:1);layout.to(1,220);
    if(layout.value==1){center=at;size=extent;strength=alpha;}
}
bool Part::step(double now,bool reduced){
    bool moving=layout.step(now,reduced);auto t=float(layout.value);
    center={fromCenter.X+(toCenter.X-fromCenter.X)*t,fromCenter.Y+(toCenter.Y-fromCenter.Y)*t};
    size={fromSize.Width+(toSize.Width-fromSize.Width)*t,fromSize.Height+(toSize.Height-fromSize.Height)*t};strength=fromStrength+(toStrength-fromStrength)*t;
    return appearance.step(now,reduced)||moving;
}
WorkspaceBubbles::Impl::~Impl(){ForgetLayerThumbnails(data->thumbnails,this);disposed=true;frames.revoke();contentsLost.revoke();if(holdTimer)holdTimer.Stop();if(thumbnailTimer)thumbnailTimer.Stop();dismiss();data->colorViews.erase(colorListener);}
void WorkspaceBubbles::Impl::init(){
    stamps.IsHitTestVisible(false);Canvas::SetZIndex(stamps,-1);layer.Children().Append(stamps);
    contactReach.Fill(clear());contactReach.Visibility(Visibility::Collapsed);Canvas::SetZIndex(contactReach,0);layer.Children().Append(contactReach);
    auto weak=weak_from_this();holdTimer=workspace.DispatcherQueue().CreateTimer();holdTimer.Interval(std::chrono::milliseconds(16));holdTimer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->hold();});
    thumbnailTimer=workspace.DispatcherQueue().CreateTimer();thumbnailTimer.Interval(std::chrono::milliseconds(120));thumbnailTimer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->thumbnails();});
    colorListener=data->colorView([weak]{if(auto self=weak.lock())for(auto const& [id,record]:self->records)self->refresh(record);});
    contentsLost=CompositionTarget::SurfaceContentsLost(auto_revoke,[weak](auto&&,auto&&){if(auto self=weak.lock())for(auto const& [id,record]:self->records){
        for(auto const& [key,p]:record->parts){if(p->wheel){p->wheel->surface=nullptr;p->wheelKey=L"";}if(p->intensity)p->intensity->image->invalidate();}self->refresh(record);
    }});
    workspace.Loaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->thumbnails();});
    workspace.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock()){self->thumbnailTimer.Stop();ForgetLayerThumbnails(self->data->thumbnails,self.get());}});
    workspace.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->wake();});
}
void WorkspaceBubbles::Impl::reattach(){for(auto const& [id,record]:records){uint32_t at;if(!layer.Children().IndexOf(record->root,at))layer.Children().Append(record->root);}}
void WorkspaceBubbles::Impl::apply(){
    layer.Visibility(flag(data->state,L"workspace_ready")?Visibility::Visible:Visibility::Collapsed);
    auto nextEpoch=uint64_t(num(object(data->state,L"document_file"),L"epoch"));auto packets=array(data->model,L"bubbles");std::set<uint32_t> alive;bool replaced=epoch!=nextEpoch;
    for(auto value:packets){auto packet=value.GetObject(),view=object(packet,L"view");auto id=uint32_t(num(view,L"id"));alive.insert(id);
        if(auto found=records.find(id);found!=records.end()&&found->second->source!=object(view,L"source").Stringify())replaced=true;}
    if(records.size()!=alive.size())replaced=true;
    if(epoch!=nextEpoch){dismiss();for(auto const& [id,record]:records)record->clearStamp();}
    if(contact&&(epoch!=nextEpoch||[&]{for(auto value:packets){auto v=object(value.GetObject(),L"view");if(num(v,L"id")==contact->record->id)return object(v,L"source").Stringify()!=contact->record->source;}return contact->mode!=L"moving"&&!(contact->paired&&contact->lifted);}()))cancel(true);
    if(candidate&&(epoch!=nextEpoch||!current(*candidate)))cancel(true);
    if(replaced){++owners;lastRing.reset();}epoch=nextEpoch;
    for(auto it=records.begin();it!=records.end();)if(!alive.contains(it->first)){remove(layer,it->second->root);remove(stamps,it->second->stampImage);it=records.erase(it);}else ++it;
    int order=0;for(auto value:packets){auto packet=value.GetObject(),view=object(packet,L"view");auto id=uint32_t(num(view,L"id"));auto& record=records[id];
        if(record&&record->source!=object(view,L"source").Stringify()){remove(layer,record->root);remove(stamps,record->stampImage);record.reset();}
        if(!record){record=std::make_shared<Record>();record->id=id;record->source=object(view,L"source").Stringify();record->root.RenderTransform(record->transform);record->root.Visibility(Visibility::Collapsed);layer.Children().Append(record->root);}
        record->order=order++;record->stampDirty=true;update(record,packet);refresh(record);
    }
    if(menuId&&(!records.contains(menuId)||records.at(menuId)->source!=menuSource))dismiss();
    reattach();thumbnails();
    if(records.empty()){active=false;frames.revoke();if(replaced&&changed)changed();}
    else wake();
}
void WorkspaceBubbles::Impl::wake(){
    if(records.empty())return;
    ++wakes;active=true;if(frames)return;
    frames=CompositionTarget::Rendering(auto_revoke,[weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->frame();});
}
void WorkspaceBubbles::Impl::frame(){
    if(disposed||!workspace.IsLoaded()){frames.revoke();return;}if(busy)return;
    if(!active){frames.revoke();return;}
    auto now=milliseconds();auto reduced=!winrt::Windows::UI::ViewManagement::UISettings().AnimationsEnabled();auto ticket=owners,wake=wakes;busy=true;
    auto request=JsonUnsignedFields(to_string(O({{L"type",S(L"bubble_motion")},{L"reduced",B(reduced)}}).Stringify()),
        {{"now_ns",uint64_t(std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now().time_since_epoch()).count())}});
    if(!QueryWorkspace(data->query,std::move(request),[weak=weak_from_this(),ticket,wake,now,reduced](J reply){if(auto self=weak.lock()){
        self->busy=false;if(self->disposed)return;auto motion=object(reply,L"result");if(ticket==self->owners&&motion.Size())self->step(motion,now,reduced,wake);
    }}))busy=false;
}
void WorkspaceBubbles::Impl::step(J const& motion,double now,bool reduced,uint64_t requestedWake){
    active=requestedWake!=wakes||flag(motion,L"moving")||contact||candidate&&candidate->moving;
    auto fade=array(motion,L"fade");bool down=fade.Size()&&fade.GetBooleanAt(0);active=active||down;
    std::set<uint32_t> faded;if(fade.Size()>1)for(auto id:fade.GetArrayAt(1))faded.insert(uint32_t(id.GetNumber()));
    if(down){restoreAt=-1;for(auto const& [id,r]:records){r->faded=faded.contains(id);r->fade.to(r->faded?.2:1,140);}}
    else if(stroke)restoreAt=now+180;stroke=down;
    if(restoreAt>=0){if(now>=restoreAt){restoreAt=-1;for(auto const& [id,r]:records){r->faded=false;r->fade.to(1,140);}}else active=true;}
    for(auto value:array(motion,L"placements")){auto p=value.GetObject();auto found=records.find(uint32_t(num(p,L"id")));if(found==records.end())continue;auto& r=*found->second;
        r.center=point(array(p,L"center"));r.lifted=flag(p,L"lifted");r.homing=flag(p,L"homing");r.root.Visibility(Visibility::Visible);}
    if(contact&&!contact->paired)contact->record->hovered=hit(*contact->record,contact->last);
    for(auto const& [id,r]:records){active=r->fade.step(now,reduced)||active;
        for(auto it=r->parts.begin();it!=r->parts.end();){auto& p=*it->second;active=p.step(now,reduced)||active;
            if(p.appearance.value==0&&p.appearance.target==0){remove(r->root,p.root);it=r->parts.erase(it);continue;}
            draw(r,it->second);++it;}
        r->place();r->root.IsHitTestVisible(!r->faded);Canvas::SetZIndex(r->root,(r->lifted?800:700)+r->order);
        stamp(r);
    }
    contactSurface();if(changed)changed();
}
void WorkspaceBubbles::Impl::update(std::shared_ptr<Record> const& r,J const& packet){
    if(r->packet.Size()&&r->packet.Stringify()==packet.Stringify())return;
    auto prior=r->view;bool sameDiameter=prior.Size()&&num(prior,L"diameter")==num(object(packet,L"view"),L"diameter");r->packet=packet;r->view=object(packet,L"view");r->reach=num(packet,L"reach")+16;r->place();auto v=r->view;std::set<hstring> live;int order=0;
    bool tweenBeads=sameDiameter&&!(contact&&contact->record==r&&contact->mode==L"beads"&&contact->moved);
    auto part=[&](hstring const& key,J const& value,Point at,Size size,double radius,bool squircle=false,bool glass=true,double strength=1,double from=.6,double duration=180,double delay=0){
        live.insert(key);auto& p=r->parts[key];if(!p){p=std::make_shared<Part>(key,(key==L"face"||key==L"badge")&&!r->parts.contains(L"card")?1:0);
            p->root.RenderTransform(p->transform);p->shadow.IsHitTestVisible(false);p->selection.IsHitTestVisible(false);p->hover.IsHitTestVisible(false);p->decorations.IsHitTestVisible(false);p->art.IsHitTestVisible(key==L"card"||key==L"pill");p->root.Children().Append(p->shadow);p->root.Children().Append(p->ink);p->root.Children().Append(p->decorations);p->ink.Children().Append(p->background);p->ink.Children().Append(p->selection);p->ink.Children().Append(p->hover);p->ink.Children().Append(p->art);r->root.Children().Append(p->root);
        }
        if(p->retired){ElementCompositionPreview::SetElementChildVisual(p->art,nullptr);p->art.Children().Append(p->cardHost);p->retired=false;p->contentDirty=true;p->appearance.snap(0);}
        if(key==L"halo"&&p->appearance.target==0)p->appearance.snap(0);
        p->contentDirty=p->contentDirty||p->data.Stringify()!=value.Stringify();p->order=++order;p->data=value;p->radius=radius;p->squircle=squircle;p->glass=glass;p->fromScale=from;
        bool tween=tweenBeads&&prefix(key,L"bead:");
        if(tween&&!glass&&(p->appearance.value!=1||p->appearance.target==0)){
            auto initial=p->size.Width==0;p->place(initial?at:p->center,initial?size:p->size,p->strength*p->appearance.value,false);p->appearance.snap(1);
        }
        p->place(at,size,strength,tween);p->appearance.to(1,duration,delay);return p;
    };
    auto face=point(array(v,L"face"));bool card=object(v,L"card").Size();
    J faceData;for(auto name:{L"kind",L"icon",L"label",L"preview",L"variants",L"dots",L"buttons",L"selected"})faceData.Insert(name,v.GetNamedValue(name,JsonValue::CreateNullValue()));
    auto colorPresentation=object(packet,L"color");auto colorBounds=rectangle(object(colorPresentation,L"bounds"));faceData.Insert(L"color",object(v,L"color"));
    if(!card)part(L"face",faceData,colorPresentation.Size()?Point{colorBounds.X+colorBounds.Width/2,colorBounds.Y+colorBounds.Height/2}:Point{},colorPresentation.Size()?Size{colorBounds.Width,colorBounds.Height}:Size{face.X,face.Y},str(v,L"kind")==L"layers"?20:face.Y/2,str(v,L"kind")==L"layers",true,flag(v,L"enabled")?1:.45,.6,180,r->parts.contains(L"card")?150:0);
    if(!card&&str(v,L"kind")==L"layers"){auto text=to_hstring(uint32_t(num(v,L"layers")));auto metrics=textSize(data,text,true);auto bounds=ui(data,O({{L"type",S(L"count_badge")},{L"text",point({metrics.Width,std::max(22.f,metrics.Height)})}})).GetArray();auto size=point(bounds.GetArrayAt(1));part(L"badge",O({{L"text",S(text)}}),point(bounds.GetArrayAt(0)),{size.X,size.Y},size.Y/2,false,true,1,.6,180,r->parts.contains(L"card")?150:0);}
    if(array(v,L"rings").Size()){auto rings=O({{L"rings",array(v,L"rings")},{L"_geometry",array(packet,L"rings")}});auto reach=num(v,L"halo",num(v,L"diameter"))/2+40;part(L"halo",rings,{},Size{float(reach*2),float(reach*2)},0,false,false,1,.85,160);}
    if(!str(v,L"readout").empty()){auto metrics=textSize(data,str(v,L"readout"),false,true);part(L"readout",O({{L"text",S(str(v,L"readout"))}}),point(array(packet,L"readout")),{metrics.Width+14,metrics.Height+4},(metrics.Height+4)/2,false,true,1,1,0);}
    uint32_t fresh=0;auto chooser=array(v,L"chooser");for(uint32_t i=0;i<chooser.Size();++i){auto item=chooser.GetObjectAt(i);bool brush=str(item,L"kind")==L"brush";auto key=(brush?L"brush:":L"item:")+to_hstring(i);auto found=r->parts.find(key);
        if(found!=r->parts.end()&&found->second->appearance.target==0){auto old=found->second;r->parts.erase(found);old->order+=1000;r->parts[L"leaving:"+key+L":"+to_hstring(milliseconds())]=old;}
        else if(found!=r->parts.end()&&(str(found->second->data,L"kind")!=str(item,L"kind")||str(found->second->data,L"label")!=str(item,L"label"))){remove(r->root,found->second->root);r->parts.erase(found);}
        auto wait=r->parts.contains(key)?0:fresh++*(brush?16:14);part(key,item,point(array(item,L"center")),brush?Size{152,48}:Size{44,44},brush?24:22,false,true,flag(item,L"enabled")?1:.45,.6,180,wait);
        if(!brush){auto metrics=textSize(data,str(item,L"label"));auto tag=rectangle(ui(data,O({{L"type",S(L"chooser_tag")},{L"center",array(item,L"center")},{L"width",N(std::min(96.,double(metrics.Width)+18))}})).GetObject());part(L"tag:"+to_hstring(i),item,{tag.X+tag.Width/2,tag.Y+tag.Height/2},{tag.Width,metrics.Height+6},9,false,true,1,1,0);}}
    auto beads=array(v,L"beads"),colors=array(packet,L"bead_colors");for(uint32_t i=0;i<beads.Size();++i){auto bead=beads.GetObjectAt(i),placement=object(bead,L"placement");auto size=float(num(placement,L"size"));auto color=colors.GetAt(i);
        auto content=O({{L"kind",S(str(object(placement,L"bead"),L"kind"))},{L"label",S(str(bead,L"label"))},{L"selected",B(flag(bead,L"selected"))},{L"rgba",color}});
        bool palette=color.ValueType()==JsonValueType::Null;
        auto p=part(L"bead:"+object(placement,L"bead").Stringify(),content,point(array(placement,L"center")),{size,size},size/2,false,palette,palette?1:num(placement,L"alpha"),1,220);
        if(palette||!tweenBeads)p->appearance.snap(1);}
    if(!str(v,L"pill").empty()){auto pill=rectangle(object(packet,L"pill"));part(L"pill",O({{L"text",S(str(v,L"pill"))}}),{pill.X+pill.Width/2,pill.Y+pill.Height/2},{pill.Width,pill.Height},pill.Height/2,false,true,1,1,0);}
    if(card){auto spec=object(v,L"card");auto size=point(array(spec,L"size"));part(L"card",spec,{},Size{size.X,size.Y},18,true,true,1,.6,200);}
    r->hints.Children().Clear();r->hints.IsHitTestVisible(false);Canvas::SetZIndex(r->hints,order+1);
    uint32_t index;if(!r->root.Children().IndexOf(r->hints,index))r->root.Children().Append(r->hints);
    for(auto value:array(v,L"hints")){auto hint=value.GetArray();dot(r->hints,point(hint.GetArrayAt(0)),2.5,data->tint(L"text",uint8_t(std::round(255*.35*hint.GetNumberAt(1)))));}
    for(auto it=r->parts.begin();it!=r->parts.end();){auto const& [key,p]=*it;
        if(!live.contains(key)){
            bool bead=prefix(key,L"bead:");
            bool immediate=card&&(key==L"face"||key==L"badge")||key==L"pill"||key==L"readout"||prefix(key,L"tag:")||bead&&(p->glass||!tweenBeads)||chooser.Size()&&(prefix(key,L"item:")||prefix(key,L"brush:"));
            if(immediate){remove(r->root,p->root);it=r->parts.erase(it);continue;}
            if(p->appearance.target>0){if(bead)p->place(p->center,p->size,p->strength,false);else p->appearance.snap(1);}
            p->appearance.to(0,bead?220:key==L"card"?150:120);p->background.IsHitTestVisible(false);if(p->accessible)p->accessible.IsHitTestVisible(false);if(key==L"card")retire(*p);
        }++it;
    }
}
void WorkspaceBubbles::Impl::draw(std::shared_ptr<Record> const& r,std::shared_ptr<Part> const& p){
    if(p->theme!=data->theme()){p->theme=data->theme();p->contentDirty=true;p->wheelKey=L"";p->surfaceKey=L"";}
    p->root.Width(p->size.Width);p->root.Height(p->size.Height);p->ink.Width(p->size.Width);p->ink.Height(p->size.Height);p->art.Width(p->size.Width);p->art.Height(p->size.Height);
    p->transform.CenterX(p->size.Width/2);p->transform.CenterY(p->size.Height/2);p->transform.TranslateX(r->reach+p->center.X-p->size.Width/2);p->transform.TranslateY(r->reach+p->center.Y-p->size.Height/2);
    bool item=prefix(p->key,L"item:")||prefix(p->key,L"brush:");auto index=item?std::stod(to_string(p->key).substr(to_string(p->key).find(':')+1)):-1;
    auto marked=[&](V const& hit){return item?hit&&hit.ValueType()==JsonValueType::Object&&num(hit.GetObject(),L"item",-1)==index:kind(hit)==L"face"&&p->key==L"face"&&str(r->view,L"kind")!=L"color"&&str(r->view,L"kind")!=L"history";};
    bool hovered=marked(r->hovered),pressed=r->pressed&&contact&&contact->record==r;
    p->hot=item&&hovered&&contact&&contact->record==r&&contact->mode==L"choose"&&p->appearance.target>0;p->transform.ScaleX(p->scale());p->transform.ScaleY(p->scale());p->root.Opacity(p->appearance.value);Canvas::SetZIndex(p->root,p->order);
    p->ink.Opacity(p->strength);
    p->shadow.Opacity(prefix(p->key,L"bead:")&&!p->glass?p->strength:1);
    p->root.IsHitTestVisible(p->appearance.target>0&&!r->faded);if(p->accessible)p->accessible.Visibility(p->appearance.target>0?Visibility::Visible:Visibility::Collapsed);bool selected=flag(p->data,L"selected")&&(p->key==L"face"||item||prefix(p->key,L"bead:")&&p->glass);
    p->selection.Fill(selected?CapyUi::selected(data):clear());if(!item||p->appearance.target>0)p->hover.Fill(data->tint(L"text",pressed&&marked(contact->hit)?36:hovered?18:0));
    if(p->key==L"face"&&str(r->view,L"kind")==L"history")for(uint32_t i=0;i<2;++i){
        bool over=r->hovered&&r->hovered.ValueType()==JsonValueType::Object&&num(r->hovered.GetObject(),L"half",-1)==i;
        bool down=pressed&&contact->hit&&contact->hit.ValueType()==JsonValueType::Object&&num(contact->hit.GetObject(),L"half",-1)==i;
        p->halves[i].Fill(data->tint(L"text",down?36:over?18:0));
    }
    bool tag=prefix(p->key,L"tag:");if(tag){auto suffix=std::wstring(p->key).substr(4);auto found=r->parts.find(L"item:"+hstring(suffix));
        bool shown=found!=r->parts.end()&&found->second->appearance.value==1&&(flag(p->data,L"shown")||found->second->hot);
        p->root.Visibility(shown?Visibility::Visible:Visibility::Collapsed);}
    auto geometryKey=to_hstring(p->size.Width)+L":"+to_hstring(p->size.Height)+L":"+to_hstring(p->radius)+L":"+to_hstring(p->squircle);
    geometryKey=geometryKey+(p->glass?L":glass":L":clear")+(p->key==L"face"?array(object(r->packet,L"color"),L"outline").Stringify():hstring{});
    if(p->key!=L"halo"&&p->surfaceKey!=geometryKey){p->surfaceKey=geometryKey;for(auto path:{p->background,p->selection,p->hover})path.Data(surface(*r,*p));
        p->background.Fill(p->glass?data->glass(L"panel"):clear());p->background.IsHitTestVisible(false);p->selection.IsHitTestVisible(false);p->contentDirty=true;
        if(p->key==L"card")ClipSurface(p->art,p->size.Width,p->size.Height,{18,18,18,18},true);
        if(p->accessible){auto hit=p->accessible.Content().as<Shapes::Path>();hit.Data(surface(*r,*p,true));p->accessible.Width(p->size.Width);p->accessible.Height(p->size.Height);}
    }
    if(p->key!=L"halo"){shadow(*r,*p);}
    if(p->contentDirty&&!p->retired){p->contentDirty=false;content(r,p);}
    if(p->inspector&&!p->retired){auto& inspector=*p->inspector;bool open=flag(p->data,L"properties");
        p->cardHost.Width(p->size.Width);p->cardHost.Height(p->size.Height);
        if(inspector.animating){auto t=winrt::Windows::UI::ViewManagement::UISettings().AnimationsEnabled()?std::clamp(std::chrono::duration<double>(Clock::now()-inspector.started).count()/.2,0.,1.):1.;inspector.position=inspector.startPosition+(inspector.targetPosition-inspector.startPosition)*(1-std::pow(1-t,3));inspector.animating=t<1;active=active||inspector.animating;}
        inspector.place(p->layers->Root(),p->size.Width,p->size.Height,data->panelMetric(L"disclosure_height"),open);
        if(!inspector.animating&&!open&&inspector.anchor.Size()&&p->layers->layers->restoreAnchor(inspector.anchor))inspector.anchor=J{};
    }
}
void WorkspaceBubbles::Impl::content(std::shared_ptr<Record> const& r,std::shared_ptr<Part> const& p){
    auto weak=weak_from_this();std::weak_ptr<Record> owner=r;std::weak_ptr<Part> part=p;auto v=r->view;auto size=p->size;Point center{size.Width/2,size.Height/2};
    if(p->key==L"card"){
        p->root.Background(clear());
        if(!p->inspector){
            auto notify=[weak,owner]{if(auto self=weak.lock())if(auto r=owner.lock())self->wake();};
            p->layers=std::make_unique<PanelBody>(data,find(array(data->model,L"panels"),L"id",L"layers"),J{},notify);
            p->inspector=std::make_unique<LayerInspector>(data,find(array(data->model,L"panels"),L"id",L"properties"),nullptr,notify,[weak,owner]{if(auto self=weak.lock())if(auto r=owner.lock())self->send(O({{L"type",S(L"properties")},{L"id",N(r->id)}}));});
            auto& inspector=*p->inspector;inspector.pages.Children().Append(p->layers->Root());inspector.pages.Children().Append(inspector.properties->Root());
            p->cardHost=ContentControl();p->cardHost.IsTabStop(false);p->cardHost.HorizontalContentAlignment(HorizontalAlignment::Stretch);p->cardHost.VerticalContentAlignment(VerticalAlignment::Stretch);p->cardHost.Content(inspector.root);p->art.Children().Append(p->cardHost);
            AutomationProperties::SetAutomationId(p->cardHost,L"bubble-card-"+to_hstring(r->id));AutomationProperties::SetName(p->cardHost,str(v,L"label"));AutomationProperties::SetAutomationId(inspector.footer,L"bubble-properties-"+to_hstring(r->id));
            p->root.LayoutUpdated([weak,owner,part](auto&&,auto&&){auto self=weak.lock();auto r=owner.lock();auto p=part.lock();if(!self||!r||!p||p->retired||!object(r->view,L"card").Size())return;
                auto request=p->inspector->sizingRequest(*p->layers);if(num(request,L"properties_height")<=0)return;
                auto key=request.Stringify()+L":"+to_hstring(array(self->data->state,L"layers").Size())+L":"+to_hstring(self->epoch)+L":"+to_hstring(self->workspace.ActualHeight());if(key==p->inspector->sizingKey)return;p->inspector->sizingKey=key;
                auto epoch=self->epoch;
                if(!QueryWorkspace(self->data->query,request,[weak,owner,part,key,epoch](J reply){auto self=weak.lock();auto r=owner.lock();auto p=part.lock();
                    if(!self||!r||!p||self->epoch!=epoch||p->retired||p->inspector->sizingKey!=key||!object(r->view,L"card").Size())return;
                    auto found=self->records.find(r->id);if(found==self->records.end()||found->second!=r)return;
                    auto heights=array(object(reply,L"result"),L"heights");if(heights.Size()==2)self->send(O({{L"type",S(L"measure_card")},{L"id",N(r->id)},{L"heights",heights}}));
                }))p->inspector->sizingKey=L"";
            });
        }
        auto& inspector=*p->inspector;bool open=flag(p->data,L"properties");auto target=open?1.:0.;
        if(inspector.targetPosition!=target){if(open)inspector.anchor=p->layers->layers->scrollAnchor();inspector.startPosition=inspector.position;inspector.targetPosition=target;inspector.started=Clock::now();inspector.animating=true;}
        bool full=flag(p->data,L"full");p->layers->layers->full=full;p->layers->Apply(true);inspector.properties->Apply(open);inspector.refresh(open,str(p->data,L"disclosure"),!full);return;
    }
    if(p->key==L"halo"){
        auto rings=array(p->data,L"rings"),geometry=array(p->data,L"_geometry");
        if(p->rings.size()!=rings.Size()){
            p->art.Children().Clear();p->rings.clear();p->rings.resize(rings.Size());
            for(auto& art:p->rings){
                for(auto path:{art.body,art.track,art.value}){path.StrokeStartLineCap(PenLineCap::Round);path.StrokeEndLineCap(PenLineCap::Round);path.IsHitTestVisible(false);art.root.Children().Append(path);}
                art.thumb.StrokeThickness(1);art.thumb.IsHitTestVisible(false);art.glyph.IsHitTestVisible(false);art.root.Children().Append(art.thumb);art.root.Children().Append(art.glyph);p->art.Children().Append(art.root);
            }
        }
        for(uint32_t i=0;i<rings.Size();++i){auto& art=p->rings[i];auto ring=rings.GetObjectAt(i),shape=object(ring,L"arc");auto radius=num(shape,L"radius"),start=num(shape,L"start"),sweep=num(shape,L"sweep");auto span=21/radius*180/std::numbers::pi*(sweep<0?-1:1);
            art.root.Opacity(flag(ring,L"enabled")?1:.4);art.root.Width(size.Width);art.root.Height(size.Height);
            auto arcAt=[&](double start,double sweep){auto path=arc(radius,start,sweep);TranslateTransform shift;shift.X(center.X);shift.Y(center.Y);path.Transform(shift);return path;};
            auto key=shape.Stringify()+L":"+to_hstring(size.Width)+L":"+to_hstring(size.Height);bool geometryChanged=art.geometryKey!=key;
            if(geometryChanged){art.geometryKey=key;art.body.Data(arcAt(start-span,sweep+span));art.track.Data(arcAt(start,sweep));}
            auto fill=num(ring,L"fill");if(geometryChanged||art.fill!=fill){art.fill=fill;art.value.Data(arcAt(start,sweep*fill));}art.value.Visibility(fill>.004?Visibility::Visible:Visibility::Collapsed);
            art.body.Stroke(data->glass(L"panel"));art.body.StrokeThickness(num(shape,L"band"));art.body.IsHitTestVisible(flag(ring,L"enabled"));
            art.track.Stroke(data->tint(L"text",46));art.track.StrokeThickness(4);art.value.Stroke(data->tint(L"text",128));art.value.StrokeThickness(4);
            auto places=geometry.GetObjectAt(i);auto thumb=point(array(places,L"thumb")),mark=point(array(places,L"icon"));auto diameter=flag(ring,L"active")?15.f:14.f;
            art.thumb.Fill(data->brush(L"thumb"));art.thumb.Stroke(data->tint(L"text",153));place(art.thumb,{center.X+thumb.X,center.Y+thumb.Y},{diameter,diameter});
            auto iconKey=data->theme()+L":"+str(ring,L"icon");if(art.iconKey!=iconKey){art.iconKey=iconKey;art.glyph.Source(icon(str(ring,L"icon"),data->theme(),16).Source());}place(art.glyph,{center.X+mark.X,center.Y+mark.Y},{16,16});
        }p->art.IsHitTestVisible(true);return;
    }
    bool history=p->key==L"face"&&str(v,L"kind")==L"history";
    if(!history||!p->historyHost)p->art.Children().Clear();p->decorations.Children().Clear();
    if(p->key==L"face"){
        auto type=str(v,L"kind");
        if(type==L"color"){
            if(!p->wheel){p->wheel=std::make_unique<CapyWheel::WheelImage>();p->wheel->start([weak,owner,part]{if(auto self=weak.lock())if(auto r=owner.lock())if(auto p=part.lock()){p->wheelKey=L"";self->refresh(r);}});}
            auto color=object(r->packet,L"color");if(color.Size()){auto bounds=rectangle(object(color,L"bounds"));center={-bounds.X,-bounds.Y};}
            auto side=float(num(v,L"diameter")-14);p->wheelImage.IsHitTestVisible(false);place(p->wheelImage,center,{side,side});AutomationProperties::SetAutomationId(p->wheelImage,L"bubble-wheel-"+to_hstring(r->id));p->art.Children().Append(p->wheelImage);
            if(color.Size()){
                if(!p->intensity){
                    p->intensity=std::make_unique<IntensityArt>();auto& art=*p->intensity;art.root.IsHitTestVisible(false);
                    art.image=std::make_unique<CapyWheel::IntensityImage>();art.image->start([weak,owner,part]{if(auto self=weak.lock())if(auto r=owner.lock())if(auto p=part.lock())self->intensity(r,p);});art.root.Children().Append(art.ramp);
                    art.edge.Stroke(fill({128,0,0,0}));art.edge.StrokeThickness(4);art.marker.Stroke(fill({255,255,255,255}));art.marker.StrokeThickness(2);art.focus.StrokeThickness(2);
                    for(auto const& element:{UIElement(art.zero),UIElement(art.edge),UIElement(art.marker),UIElement(art.focus)})art.root.Children().Append(element);
                    auto slider=art.value;slider.Width(1);slider.Height(1);slider.Opacity(0);slider.IsHitTestVisible(false);slider.UseSystemFocusVisuals(false);slider.IsThumbToolTipEnabled(false);slider.StepFrequency(.01);slider.SmallChange(.1);slider.LargeChange(1);p->root.Children().Append(slider);
                    AutomationProperties::SetAutomationId(slider,L"bubble-intensity-"+to_hstring(r->id));
                    slider.ValueChanged([weak,owner,part](auto&&,Primitives::RangeBaseValueChangedEventArgs const& e){if(auto self=weak.lock())if(auto r=owner.lock())if(auto p=part.lock();p&&p->intensity&&!p->intensity->syncing&&p->intensity->value.IsEnabled())self->send(O({{L"type",S(L"set_intensity")},{L"id",N(r->id)},{L"stops",N(e.NewValue())}}));});
                    auto focused=[weak,owner,part](auto&&,auto&&){if(auto self=weak.lock())if(auto r=owner.lock())if(auto p=part.lock())self->intensity(r,p);};slider.GotFocus(focused);slider.LostFocus(focused);
                }
                p->art.Children().Append(p->intensity->root);intensity(r,p);
            }else if(p->intensity){remove(p->root,p->intensity->value);p->intensity.reset();}
        }else if(type==L"history"){
            p->art.IsHitTestVisible(true);
            if(!p->historyHost){p->historyHost=ContentControl();p->historyHost.IsTabStop(false);p->historyHost.Content(Canvas());p->art.Children().Append(p->historyHost);}
            place(p->historyHost,center,size);AutomationProperties::SetAutomationId(p->historyHost,L"tool-bubble-"+to_hstring(r->id));AutomationProperties::SetName(p->historyHost,str(v,L"label"));
            auto buttons=array(v,L"buttons"),positions=array(r->packet,L"history");auto group=p->historyHost.Content().as<Canvas>();group.Width(size.Width);group.Height(size.Height);
            for(uint32_t i=0;i<buttons.Size();++i){auto spec=buttons.GetObjectAt(i);auto offset=point(positions.GetArrayAt(i));auto& button=p->historyButtons[i];
                if(!button){button=bubbleButton();Canvas face;face.Width(size.Height);face.Height(size.Height);Point half{size.Height/2,size.Height/2};p->halves[i].IsHitTestVisible(true);place(p->halves[i],half,{size.Height,size.Height});face.Children().Append(p->halves[i]);auto glyph=icon(str(spec,L"icon"),data->theme(),20);glyph.IsHitTestVisible(false);place(glyph,half,{20,20});face.Children().Append(glyph);button.Content(face);group.Children().Append(button);
                    AutomationProperties::SetAutomationId(button,L"bubble-history-"+to_hstring(r->id)+L"-"+to_hstring(i));
                    button.Click([weak,owner,i](auto&&,auto&&){if(auto self=weak.lock())if(auto r=owner.lock();r&&!self->contact)self->send(O({{L"type",S(L"tap")},{L"id",N(r->id)},{L"position",array(r->packet,L"history").GetArrayAt(i)}}));});
                }
                place(button,{center.X+offset.X,center.Y+offset.Y},{size.Height,size.Height});button.IsEnabled(flag(spec,L"enabled"));AutomationProperties::SetName(button,str(spec,L"label"));tooltip(button,str(spec,L"label"));
                auto glyph=button.Content().as<Canvas>().Children().GetAt(1).as<Image>();setIcon(glyph,str(spec,L"icon"),data->theme(),20);glyph.Opacity(flag(spec,L"enabled")?1:.35);
            }
        }else if(type==L"layers"){
            auto edge=shape(squircleRectangle(48,48,{14,14,14,14}),fill({71,128,128,128}));place(edge,center,{48,48});p->art.Children().Append(edge);
            auto back=shape(squircleRectangle(46,46,{13,13,13,13}),fill({255,255,255,255}));place(back,center,{46,46});p->art.Children().Append(back);
            p->thumbnailBrush.Stretch(Stretch::UniformToFill);p->thumbnailFace=shape(squircleRectangle(46,46,{13,13,13,13}),p->thumbnailBrush);place(p->thumbnailFace,center,{46,46});p->art.Children().Append(p->thumbnailFace);
        }else{
            bool brush=v.GetNamedValue(L"preview",JsonValue::CreateNullValue()).ValueType()==JsonValueType::Number;
            if(brush)previewAt(p->decorations,data,int(num(v,L"preview")),{center.X,center.Y+11},{32,8},.78);
            iconAt(p->art,data,str(v,L"icon"),{center.X,center.Y+(brush?-6:type==L"medium"?-4:0)},20);
            if(flag(v,L"variants"))iconAt(p->art,data,L"tool-group",{size.Width-17,size.Height-17},16);
            auto dots=array(r->packet,L"dots"),chosenDots=array(v,L"dots");for(uint32_t i=0;i<dots.Size();++i){auto at=point(dots.GetArrayAt(i));dot(p->decorations,{center.X+at.X,center.Y+at.Y},1.75,data->tint(L"text",chosenDots.GetBooleanAt(i)?230:77));}
        }
    }else if(p->key==L"pill"){
        p->art.IsHitTestVisible(true);for(auto direction:{-1,1}){auto button=CapyUi::button(data,L"",[weak,owner,direction]{if(auto self=weak.lock())if(auto r=owner.lock())self->send(O({{L"type",S(L"palette")},{L"id",N(r->id)},{L"step",N(direction)}}));});CapyUi::buttonFeedback(button,data->brush(L"text"),18,36);button.Padding({4,0,4,0});button.BorderThickness({0});button.CornerRadius({15,15,15,15});button.Background(clear());button.Content(icon(direction<0?L"go-previous":L"go-next",data->theme()));place(button,{direction<0?16.f:size.Width-16,15},{32,30});p->art.Children().Append(button);}
        auto button=CapyUi::button(data,str(p->data,L"text"),[weak,owner,part]{if(auto self=weak.lock())if(auto r=owner.lock())if(auto p=part.lock())self->showMenu(r,{r->center.X+p->center.X,r->center.Y+p->center.Y},true);});CapyUi::buttonFeedback(button,data->brush(L"text"),18,36);button.Padding({4,0,4,0});button.BorderThickness({0});button.CornerRadius({15,15,15,15});button.Background(clear());auto text=label(data,str(p->data,L"text"));text.FontWeight(Windows::UI::Text::FontWeights::Bold());text.TextAlignment(TextAlignment::Center);text.TextTrimming(TextTrimming::CharacterEllipsis);text.MaxLines(1);button.Content(text);place(button,center,{size.Width-68,30});p->art.Children().Append(button);return;
    }else if(p->key==L"badge"||p->key==L"readout"||prefix(p->key,L"tag:")){
        auto text=label(data,str(p->data,prefix(p->key,L"tag:")?L"label":L"text"));if(p->key==L"badge")text.FontWeight(Windows::UI::Text::FontWeights::SemiBold());if(p->key==L"readout")Documents::Typography::SetNumeralAlignment(text,FontNumeralAlignment::Tabular);text.TextAlignment(TextAlignment::Center);text.TextTrimming(TextTrimming::CharacterEllipsis);text.MaxLines(1);text.IsHitTestVisible(false);text.Measure({INFINITY,INFINITY});auto padding=p->key==L"badge"?5:prefix(p->key,L"tag:")?9:7;place(text,center,{size.Width-float(padding*2),text.DesiredSize().Height});p->art.Children().Append(text);return;
    }else if(prefix(p->key,L"bead:")){
        if(p->glass)iconAt(p->art,data,L"palette",center,13);
        else{SwatchFace swatch;swatch.Update(data,array(p->data,L"rgba"),size.Width,workspace.XamlRoot()?workspace.XamlRoot().RasterizationScale():1,flag(p->data,L"selected"),str(p->data,L"kind")==L"foreground",false);p->art.Children().Append(swatch.root);}
    }else{
        if(p->data.GetNamedValue(L"preview",JsonValue::CreateNullValue()).ValueType()==JsonValueType::Number){previewAt(p->decorations,data,int(num(p->data,L"preview")),{center.X,13},{size.Width*.68f,14});auto text=label(data,str(p->data,L"label"));text.TextAlignment(TextAlignment::Center);text.TextTrimming(TextTrimming::CharacterEllipsis);text.MaxLines(1);text.Opacity(.8);text.IsHitTestVisible(false);text.Measure({INFINITY,INFINITY});auto height=text.DesiredSize().Height;place(text,{center.X,size.Height-5-height/2},{size.Width-16,height});p->art.Children().Append(text);}
        else iconAt(p->art,data,str(p->data,L"icon"),center,18);
    }
    bool interactive=p->key==L"face"||prefix(p->key,L"bead:")||prefix(p->key,L"item:")||prefix(p->key,L"brush:");
    if(interactive&&!p->accessible){p->accessible=bubbleButton();auto region=shape(surface(*r,*p,true),clear());region.IsHitTestVisible(true);p->accessible.Content(region);p->root.Children().Append(p->accessible);
        p->accessible.Click([weak,owner,part](auto&&,auto&&){auto self=weak.lock();auto r=owner.lock();auto p=part.lock();if(!self||!r||!p||self->contact||str(r->view,L"kind")==L"history")return;auto type=prefix(p->key,L"bead:")?L"beads":prefix(p->key,L"item:")||prefix(p->key,L"brush:")?L"choose":L"tap";
            auto action=O({{L"type",S(type)},{L"id",N(r->id)},{L"position",point(p->center)}});if(std::wstring_view(type)!=L"tap"){action.Insert(L"phase",S(L"down"));self->send(action);action.Insert(L"phase",S(L"up"));}self->send(action);
        });
    }
    if(p->accessible){if(p->key==L"face")AutomationProperties::SetAutomationId(p->accessible,(history?L"bubble-hit-":L"tool-bubble-")+to_hstring(r->id));p->accessible.IsTabStop(!history);AutomationProperties::SetAccessibilityView(p->accessible,history?Automation::Peers::AccessibilityView::Raw:Automation::Peers::AccessibilityView::Content);p->accessible.Width(size.Width);p->accessible.Height(size.Height);p->accessible.Content().as<Shapes::Path>().Data(surface(*r,*p,true));auto name=p->key==L"face"?str(v,L"label"):str(p->data,L"label");AutomationProperties::SetName(p->accessible,name);}
}
void WorkspaceBubbles::Impl::intensity(std::shared_ptr<Record> const& r,std::shared_ptr<Part> const& p){
    auto color=object(r->packet,L"color");
    for(auto value:array(data->colorPreview,L"bubble_colors")){auto entry=value.GetArray();if(entry.GetNumberAt(0)==r->id){color=entry.GetObjectAt(1);break;}}
    auto spec=object(color,L"intensity");if(!p->intensity||!spec.Size())return;
    auto& art=*p->intensity;auto geometry=object(spec,L"geometry");auto bounds=rectangle(object(color,L"bounds"));Point origin{-bounds.X,-bounds.Y};
    auto range=array(spec,L"range");auto marker=num(geometry,L"marker_radius");
    auto key=geometry.Stringify()+range.Stringify()+object(color,L"bounds").Stringify();
    if(art.geometryKey!=key){
        art.geometryKey=key;art.root.Width(p->size.Width);art.root.Height(p->size.Height);
        auto zero=array(spec,L"zero");auto from=point(zero.GetArrayAt(0)),to=point(zero.GetArrayAt(1));
        art.zero.X1(origin.X+from.X);art.zero.Y1(origin.Y+from.Y);art.zero.X2(origin.X+to.X);art.zero.Y2(origin.Y+to.Y);art.zero.StrokeThickness(1);
    }
    double size=std::max(p->size.Width,p->size.Height),scale=workspace.XamlRoot()?workspace.XamlRoot().RasterizationScale():1.;
    art.ramp.Width(size);art.ramp.Height(size);art.image->present(art.ramp,spec,origin,size,scale);
    auto at=point(array(spec,L"point"));at={at.X+origin.X,at.Y+origin.Y};
    place(art.edge,at,{float(marker*2+4),float(marker*2+4)});place(art.marker,at,{float(marker*2+2),float(marker*2+2)});place(art.focus,at,{float(marker*2+10),float(marker*2+10)});
    art.marker.Fill(fill(CapyWheel::rgba(array(spec,L"marker"))));art.zero.Stroke(data->tint(L"text",230));art.focus.Stroke(data->tint(L"text",230));
    art.focus.Visibility(art.value.FocusState()==FocusState::Keyboard?Visibility::Visible:Visibility::Collapsed);
    art.syncing=true;art.value.Minimum(range.GetNumberAt(0));art.value.Maximum(range.GetNumberAt(1));art.value.Value(num(spec,L"stops"));art.syncing=false;
    art.value.IsEnabled(flag(spec,L"enabled"));art.value.IsTabStop(flag(spec,L"enabled"));Canvas::SetLeft(art.value,at.X);Canvas::SetTop(art.value,at.Y);
    AutomationProperties::SetName(art.value,str(spec,L"label"));AutomationProperties::SetItemStatus(art.value,str(spec,L"text"));
}
void WorkspaceBubbles::Impl::refresh(std::shared_ptr<Record> const& r){
    for(auto const& [key,p]:r->parts){if(p->retired)continue;draw(r,p);
        if(p->wheel){auto preview=data->colorPreview;auto model=object(preview,L"view");if(!model.Size())model=object(data->model,L"color_panel");auto colors=object(preview,L"colors");if(!colors.Size())colors=displayColors(data->state);auto side=num(r->view,L"diameter")-14,scale=workspace.XamlRoot()?workspace.XamlRoot().RasterizationScale():1.;auto next=model.Stringify()+colors.Stringify()+to_hstring(side)+L":"+to_hstring(scale);
            if(p->wheelKey!=next&&side>0){try{p->wheel->present(p->wheelImage,model,colors,side,scale);p->wheelKey=next;}catch(hresult_error const& error){OutputDebugStringW(error.message().c_str());}}
        }
        intensity(r,p);
        if(p->inspector){p->layers->Apply(true);p->inspector->properties->Apply(flag(p->data,L"properties"));p->inspector->refresh(flag(p->data,L"properties"),str(p->data,L"disclosure"),!flag(p->data,L"full"));}
    }
}
void WorkspaceBubbles::Impl::thumbnails(){
    if(!workspace.IsLoaded()||!workspace.XamlRoot()||!workspace.XamlRoot().IsHostVisible()){ForgetLayerThumbnails(data->thumbnails,this);return;}
    auto documentEpoch=CapyLayers::epochOf(data);std::vector<LayerThumbnail> wanted;
    for(auto const& [id,r]:records){auto found=r->parts.find(L"face");if(str(r->view,L"kind")!=L"layers"||found==r->parts.end())continue;auto p=found->second;auto row=CapyLayers::panelRow(data->state,num(r->view,L"layer",-1));if(!flag(row,L"has_thumbnail")){p->thumbnailBrush.ImageSource(nullptr);continue;}
        LayerThumbnail target{to_hstring(uint64_t(num(row,L"id"))),to_hstring(uint64_t(num(row,L"id"))),to_hstring(uint64_t(num(row,L"paint_revision"))),false};wanted.push_back(target);p->thumbnailBrush.ImageSource(LayerThumbnailSource(data->thumbnails,documentEpoch,target));}
    if(wanted.empty()){ForgetLayerThumbnails(data->thumbnails,this);thumbnailTimer.Stop();}
    else{auto interval=RefreshLayerThumbnails(data->thumbnails,this,documentEpoch,wanted);if(thumbnailTimer.Interval()!=interval)thumbnailTimer.Interval(interval);if(!thumbnailTimer.IsRunning())thumbnailTimer.Start();}
}
void WorkspaceBubbles::Impl::stamp(std::shared_ptr<Record> const& r){
    auto spec=object(r->view,L"stamp");if(!spec.Size()){r->stampImage.Visibility(Visibility::Collapsed);r->stampImage.Source(nullptr);if(r->stamp.Size()||r->stampBusy||r->stampPending)r->clearStamp();return;}
    auto rgba=array(r->packet,L"stamp_color");if(rgba.Size()!=4)return;
    auto ink=CapyWheel::rgba(rgba);auto key=rgba.Stringify();if(key!=r->stampColor){r->stampColor=key;r->clearStamp();}
    auto at=point(array(spec,L"center"));auto diameter=float(num(spec,L"diameter"));auto position=Point{at.X+r->center.X,at.Y+r->center.Y};
    auto extent=num(r->stamp,L"extent",1);place(r->stampImage,position,{diameter*float(extent),diameter*float(extent)});r->stampImage.IsHitTestVisible(false);r->stampImage.Stretch(Stretch::Fill);uint32_t index;if(!stamps.Children().IndexOf(r->stampImage,index))stamps.Children().Append(r->stampImage);r->stampImage.Visibility(r->stampImage.Source()?Visibility::Visible:Visibility::Collapsed);
    if(r->stampBusy||!r->stampDirty&&!r->stampPending)return;r->stampDirty=false;r->stampBusy=true;auto generation=r->stampGeneration;auto query=O({{L"type",S(L"bubble_stamp")}});if(r->stamp.Size())query.Insert(L"known_request",N(num(r->stamp,L"request_id")));
    if(!RequestStamp(data->query,query,ink,[weak=weak_from_this(),owner=std::weak_ptr(r),generation](J result,Imaging::WriteableBitmap image){if(auto self=weak.lock())if(auto r=owner.lock();r&&r->stampGeneration==generation&&object(r->view,L"stamp").Size()){
        r->stampBusy=false;r->stampPending=flag(result,L"pending");if(image){r->stamp=result;r->stampImage.Source(image);}if(image||r->stampPending)self->wake();
    }})){r->stampBusy=false;r->stampDirty=true;}
}
void WorkspaceBubbles::Impl::glass(A& boxes,A& arcs,A& colors,UIElement const& reference)const{
    if(!workspace.IsLoaded())return;auto origin=workspace.TransformToVisual(reference).TransformPoint({0,0});
    for(auto const& [id,r]:records)for(auto const& [key,p]:r->parts){if(p->root.Visibility()!=Visibility::Visible)continue;auto strength=r->alpha()*p->appearance.value*p->strength;auto scale=r->scale()*p->scale();if(strength<=0)continue;
        if(p->key==L"halo"){for(auto value:array(p->data,L"rings")){auto ring=value.GetObject(),a=object(ring,L"arc");auto radius=num(a,L"radius"),sign=num(a,L"sweep")<0?-1.:1.;A arc;for(double v:std::array<double,7>{origin.X+r->center.X,origin.Y+r->center.Y,radius*scale,num(a,L"band")*scale,num(a,L"start")*std::numbers::pi/180-21/radius*sign,num(a,L"sweep")*std::numbers::pi/180+21/radius*sign,strength*(flag(ring,L"enabled")?1:.4)})arc.Append(N(v));arcs.Append(arc);}}
        else if(p->key==L"face"&&object(r->packet,L"color").Size()){
            auto geometry=array(object(r->packet,L"color"),L"glass");A color;auto shift=r->scale()*(1-p->scale());
            for(double v:{origin.X+r->center.X+p->center.X*shift,origin.Y+r->center.Y+p->center.Y*shift,geometry.GetNumberAt(0)*scale,geometry.GetNumberAt(1)*scale,geometry.GetNumberAt(2)*scale,geometry.GetNumberAt(3),strength})color.Append(N(v));colors.Append(color);
        }else if(p->glass){auto width=p->size.Width*scale,height=p->size.Height*scale,radius=p->radius*scale;A box;for(double v:{origin.X+r->center.X+p->center.X*r->scale()-width/2,origin.Y+r->center.Y+p->center.Y*r->scale()-height/2,width,height,radius,radius,radius,radius,p->squircle?1.:0.,strength})box.Append(N(v));boxes.Append(box);}
    }
}
WorkspaceBubbles::WorkspaceBubbles(std::shared_ptr<WorkspaceData> data,Canvas workspace,std::function<void()> changed):impl(std::make_shared<Impl>()){impl->data=std::move(data);impl->workspace=workspace;impl->changed=std::move(changed);impl->init();}
WorkspaceBubbles::~WorkspaceBubbles(){impl->cancel();}
void WorkspaceBubbles::Attach(UIElement const& root,std::function<void()> cancel){impl->attach(root,std::move(cancel));}
void WorkspaceBubbles::Apply(){impl->apply();}
void WorkspaceBubbles::Reattach(){impl->reattach();}
void WorkspaceBubbles::Wake(){impl->wake();}
bool WorkspaceBubbles::Cancel(){return impl->cancel();}
void WorkspaceBubbles::Glass(A& boxes,A& arcs,A& colors,UIElement const& reference)const{impl->glass(boxes,arcs,colors,reference);}
A WorkspaceBubbles::InputRegions(UIElement const& reference)const{
    A result;if(!impl->layer.IsLoaded())return result;
    for(auto const& [id,r]:impl->records)if(!r->faded)for(auto const& [key,p]:r->parts)if(p->root.IsHitTestVisible()&&p->root.Visibility()==Visibility::Visible){
        auto common=impl->inputRoot.as<FrameworkElement>();auto bounds=common.TransformToVisual(reference).TransformBounds(visibleBounds(p->root,common));if(bounds.Width>0&&bounds.Height>0)result.Append(O({{L"x",N(bounds.X)},{L"y",N(bounds.Y)},{L"width",N(bounds.Width)},{L"height",N(bounds.Height)}}));
    }return result;
}
