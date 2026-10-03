#pragma once
#include "CanvasNotice.h"

namespace CapyUi {
struct PreviewPanel:std::enable_shared_from_this<PreviewPanel>{
    struct Kind{wchar_t const* draft;wchar_t const* type;wchar_t const* radius;wchar_t const* apply;wchar_t const* cancel;wchar_t const* identity;wchar_t const* name;};
    static constexpr Kind SelectionRefine{L"selection_resize",L"selection",L"resize_radius",L"apply_resize",L"cancel_resize",L"kind",L"selection-refine"};
    static constexpr Kind FrequencySeparation{L"frequency_separation",L"frequency_separation",L"radius",L"apply",L"cancel",L"label",L"frequency-separation"};
    static constexpr double Width=360,Offscreen=-100000;
    std::shared_ptr<WorkspaceData> data;
    Kind kind{};
    Canvas host{nullptr};
    ContentControl frame;
    Grid layers;
    Shapes::Path surface;
    StackPanel body;
    TextBlock title{nullptr};
    Border slot;
    Button apply{nullptr},cancel{nullptr};
    WorkspaceShadow shadow;
    Bindings binds;
    NumericAdmissions admissions;
    hstring built;
    J work,status,bar;
    std::optional<std::array<double,4>> shown;
    std::function<void()> moved;
    weak_ref<Control> previous;

    J view()const{return object(object(data->state,L"layer_tools"),kind.draft);}
    void send(J const& action)const{data->dispatch(O({{L"type",S(kind.type)},{L"action",action}}));}
    hstring id(wchar_t const* part)const{return hstring(kind.name)+L"-"+part;}
    void init(Canvas const& root){
        host=root;auto weak=weak_from_this();
        title=label(data,L"",true);body.Children().Append(title);body.Children().Append(slot);
        apply=button(data,data->copyCommon(L"apply"),[weak]{if(auto self=weak.lock())self->accept();});
        cancel=button(data,data->copyCommon(L"cancel"),[weak]{if(auto self=weak.lock())self->dismiss();});
        for(auto action:{cancel,apply}){action.Padding({12,5,12,5});action.MinHeight(34);action.AllowFocusOnInteraction(false);}
        AutomationProperties::SetAutomationId(apply,id(L"apply"));AutomationProperties::SetAutomationId(cancel,id(L"cancel"));
        StackPanel actions;actions.Orientation(Orientation::Horizontal);actions.Spacing(8);actions.HorizontalAlignment(HorizontalAlignment::Right);
        actions.Children().Append(cancel);actions.Children().Append(apply);body.Children().Append(actions);
        body.Spacing(10);body.Padding({16,14,16,12});
        surface.IsHitTestVisible(false);layers.Children().Append(surface);layers.Children().Append(body);
        frame.Content(layers);frame.UseSystemFocusVisuals(false);
        frame.HorizontalContentAlignment(HorizontalAlignment::Stretch);frame.VerticalContentAlignment(VerticalAlignment::Stretch);
        AutomationProperties::SetAutomationId(frame,id(L"panel"));
        frame.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){
            auto self=weak.lock();if(!self||composingKey(e))return;
            if(e.Key()==Windows::System::VirtualKey::Enter){e.Handled(true);self->accept();}
            else if(e.Key()==Windows::System::VirtualKey::Escape){e.Handled(true);self->dismiss();}
        });
        Canvas::SetZIndex(frame,900);
        attach();present();
    }
    void attach(){
        uint32_t index;
        if(!host.Children().IndexOf(shadow.Root(),index))host.Children().Append(shadow.Root());
        if(!host.Children().IndexOf(frame,index))host.Children().Append(frame);
    }
    bool commit(bool revert){
        bool admitted=true;
        for(auto const& admit:admissions)admitted=admit(revert)&&admitted;
        return admitted;
    }
    void accept(){if(view().Size()&&commit(false))send(O({{L"op",S(kind.apply)}}));}
    void dismiss(){if(view().Size()){commit(true);send(O({{L"op",S(kind.cancel)}}));}}
    void Cancel(){if(view().Size())send(O({{L"op",S(kind.cancel)}}));}
    void build(J const& draft){
        binds.clear();admissions.clear();auto weak=weak_from_this();
        NumberPresentation presentation;presentation.title=[weak]{if(auto self=weak.lock())return str(self->view(),L"label");return hstring();};
        slot.Child(number(data,str(draft,L"label"),object(draft,L"numeric"),
            [weak]{if(auto self=weak.lock())return num(self->view(),L"radius");return 0.;},
            [weak](double value){if(auto self=weak.lock();self&&!self->data->updating)self->send(O({{L"op",S(self->kind.radius)},{L"radius",N(value)}}));},
            binds,nullptr,false,id(L"value"),false,presentation,&admissions));
    }
    void Publish(J const& state){
        auto draft=object(object(state,L"layer_tools"),kind.draft);
        if(!draft.Size()){
            if(!built.empty()){built=L"";binds.clear();admissions.clear();restoreFocus();slot.Child(nullptr);present();}
            return;
        }
        bool opening=built.empty();
        title.Text(str(draft,L"title"));AutomationProperties::SetName(frame,str(draft,L"title"));
        if(auto key=str(draft,kind.identity)+object(draft,L"numeric").Stringify()+data->theme();key!=built){built=key;build(draft);}
        surface.Fill(data->brush(L"panel"));title.Foreground(data->brush(L"text"));
        apply.Background(accent(data));apply.Foreground(data->brush(L"accent_foreground"));
        for(auto const& bind:binds)bind();
        present();
        if(opening){previous=FocusManager::GetFocusedElement(host.XamlRoot()).try_as<Control>();frame.Focus(FocusState::Programmatic);}
    }
    void restoreFocus(){
        auto focus=FocusManager::GetFocusedElement(host.XamlRoot()).try_as<DependencyObject>();bool inside=false;
        for(auto node=focus;node&&!inside;node=VisualTreeHelper::GetParent(node))inside=node==frame;
        if(auto target=previous.get();inside&&target&&target.IsLoaded())target.Focus(FocusState::Programmatic);
        previous=nullptr;
    }
    void Place(J const& layout){work=object(layout,L"work_area");status=object(layout,L"status");present();}
    void Bar(J const& bounds){if(bounds.Size()||built.empty())bar=bounds;present();}
    J Bounds()const{
        if(!shown)return J{};
        auto [x,y,width,height]=*shown;
        return O({{L"x",N(x)},{L"y",N(y)},{L"width",N(width)},{L"height",N(height)}});
    }
    void present(){
        auto before=shown;
        if(built.empty()||!work.Size()){
            frame.Visibility(Visibility::Collapsed);frame.IsTabStop(false);
            shadow.Layout({float(Offscreen),0,1,1},899,false);shown.reset();
        }else{
            auto anchor=noticeAnchor(work,status,bar);
            frame.Visibility(Visibility::Visible);frame.IsTabStop(true);
            float width=float(std::min(Width,anchor.width));
            frame.Width(width);frame.Height(std::numeric_limits<double>::quiet_NaN());
            frame.Measure({width,INFINITY});
            float height=std::ceil(frame.DesiredSize().Height);frame.Height(height);
            double left=std::round(anchor.x-width/2),top=std::round(anchor.y-height);
            Canvas::SetLeft(frame,left);Canvas::SetTop(frame,top);
            std::array<float,4> radii{SurfaceRadius,SurfaceRadius,SurfaceRadius,SurfaceRadius};
            surface.Data(squircleRectangle(width,height,radii));
            shadow.Shape(squircleRectangle(width,height,radii),width,height,8,2,.27f);shadow.Cut(radii);
            shadow.Layout({float(left),float(top),width,height},899,true);
            shown=std::array<double,4>{left,top,double(width),double(height)};
        }
        if(shown!=before&&moved)moved();
    }
};
}
