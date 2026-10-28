#pragma once
#include "PointerCompletion.h"
#include <winrt/Microsoft.UI.Xaml.Input.h>
#include <winrt/Microsoft.UI.Input.h>
#include <winrt/Microsoft.UI.Xaml.Controls.h>
#include <winrt/Microsoft.UI.Xaml.Controls.Primitives.h>
#include <winrt/Microsoft.UI.Xaml.Media.h>
#include <memory>
#include <vector>

namespace CapyUi {
class PointerGesture : public winrt::Microsoft::UI::Input::GestureRecognizer {
    std::shared_ptr<bool> motion=std::make_shared<bool>(false);
    static void ownHolding(winrt::Microsoft::UI::Xaml::DependencyObject const& node){
        using namespace winrt::Microsoft::UI::Xaml;
        if(node.try_as<Controls::TextBox>())return;
        if(auto element=node.try_as<UIElement>()){element.CanDrag(false);element.IsHoldingEnabled(false);}
        for(int i=0;i<Media::VisualTreeHelper::GetChildrenCount(node);++i)ownHolding(Media::VisualTreeHelper::GetChild(node,i));
    }
public:
    template<class Finish>
    static void OnTerminal(winrt::Microsoft::UI::Xaml::UIElement const& element,Finish finish){
        using namespace winrt::Microsoft::UI::Xaml;
        auto bind=[&](RoutedEvent const& event,PointerCompletion completion){
            element.AddHandler(event,winrt::box_value(Input::PointerEventHandler([finish,completion](auto&& sender,Input::PointerRoutedEventArgs const& e){
                finish(sender,e,completion);
            })),true);
        };
        bind(UIElement::PointerReleasedEvent(),PointerCompletion::Released);
        bind(UIElement::PointerCanceledEvent(),PointerCompletion::Canceled);
        bind(UIElement::PointerCaptureLostEvent(),PointerCompletion::CaptureLost);
    }
    static bool PrimaryContact(winrt::Microsoft::UI::Input::PointerPoint const& point){
        auto properties=point.Properties();
        return point.IsInContact()&&!properties.IsCanceled()&&!properties.IsRightButtonPressed()&&!properties.IsBarrelButtonPressed()&&
            (point.PointerDeviceType()!=winrt::Microsoft::UI::Input::PointerDeviceType::Mouse||properties.IsLeftButtonPressed());
    }
    static bool Captured(winrt::Microsoft::UI::Xaml::UIElement const& element,uint32_t id){
        if(auto captures=element.PointerCaptures())for(auto pointer:captures)if(pointer.PointerId()==id)return true;
        return false;
    }
    template<class Leave>
    static void OnLeaving(winrt::Microsoft::UI::Xaml::FrameworkElement const& element,Leave leave){
        using namespace winrt::Microsoft::UI::Xaml;
        element.AddHandler(UIElement::PointerExitedEvent(),winrt::box_value(Input::PointerEventHandler([leave](auto&& sender,Input::PointerRoutedEventArgs const& e){
            auto element=sender.template as<FrameworkElement>();
            if(Outside(element,e))leave(e);
        })),true);
    }
    static bool Outside(winrt::Microsoft::UI::Xaml::FrameworkElement const& element,winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs const& event){
        auto at=event.GetCurrentPoint(element).Position();
        return at.X<0||at.Y<0||at.X>=element.ActualWidth()||at.Y>=element.ActualHeight();
    }
    template<class Finish>
    static void OnCompleted(winrt::Microsoft::UI::Xaml::UIElement const& element,Finish finish,
        PointerCaptureLoss loss=PointerCaptureLoss::Cancel){
        OnTerminal(element,[finish,loss](auto&&,auto const& e,PointerCompletion completion){
            auto point=e.GetCurrentPoint(nullptr);
            finish(e,CanceledCompletion(completion,point.Properties().IsCanceled(),point.IsInContact(),loss));
        });
    }
    template<class Finish>
    static std::shared_ptr<PointerClick> Clicks(winrt::Microsoft::UI::Xaml::Controls::Primitives::ButtonBase const& element,Finish finish){
        using namespace winrt::Microsoft::UI::Xaml;
        auto clicks=std::make_shared<PointerClick>();
        element.AddHandler(UIElement::PointerPressedEvent(),winrt::box_value(Input::PointerEventHandler([clicks](auto&& sender,Input::PointerRoutedEventArgs const& e){
            auto point=e.GetCurrentPoint(nullptr);
            if(PrimaryContact(point)&&Captured(sender.template as<UIElement>(),point.PointerId()))clicks->Begin(point.PointerId());
        })),true);
        element.Unloaded([clicks,finish](auto&&,auto&&){if(auto id=clicks->Cancel())finish(*id,true);});
        element.AddHandler(UIElement::PreviewKeyDownEvent(),winrt::box_value(Input::KeyEventHandler([clicks](auto&&,auto&&){clicks->Reset();})),true);
        OnTerminal(element,[clicks,finish](auto&& sender,Input::PointerRoutedEventArgs const& e,PointerCompletion completion){
            auto point=e.GetCurrentPoint(nullptr);auto id=point.PointerId();
            bool cancel=CanceledCompletion(completion,point.Properties().IsCanceled(),point.IsInContact(),PointerCaptureLoss::CompleteLift);
            if(clicks->Complete(id,completion,cancel,Captured(sender.template as<UIElement>(),id)))finish(id,cancel);
        });
        return clicks;
    }
    static void OwnHolding(winrt::Microsoft::UI::Xaml::FrameworkElement const& element){
        ownHolding(element);
        element.Loaded([](auto&& sender,auto&&){ownHolding(sender.template as<winrt::Microsoft::UI::Xaml::DependencyObject>());});
    }
    PointerGesture(){
        Dragging([motion=motion](auto&&,auto const& event){
            if(event.DraggingState()==winrt::Microsoft::UI::Input::DraggingState::Started)*motion=true;
        });
        ManipulationStarted([motion=motion](auto&&,auto&&){*motion=true;});
    }
    void Begin(winrt::Microsoft::UI::Xaml::UIElement const& source,winrt::Microsoft::UI::Input::PointerPoint const& point,
        winrt::Microsoft::UI::Input::GestureSettings settings=winrt::Microsoft::UI::Input::GestureSettings::Hold){
        using Settings=winrt::Microsoft::UI::Input::GestureSettings;
        if(source)ownHolding(source);
        *motion=false;
        GestureSettings(settings|Settings::Tap|Settings::Drag|Settings::ManipulationTranslateX|Settings::ManipulationTranslateY);
        ProcessDownEvent(point);
    }
    void Move(winrt::Microsoft::UI::Xaml::UIElement const& source,winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs const& event){
        if(!event.IsGenerated())if(auto points=event.GetIntermediatePoints(source))ProcessMoveEvents(points);
    }
    bool Moved()const{return *motion;}
};
class ControlCapture:public std::enable_shared_from_this<ControlCapture>{
    using Element=winrt::Microsoft::UI::Xaml::FrameworkElement;
    using Node=winrt::Microsoft::UI::Xaml::DependencyObject;
    using View=winrt::Microsoft::UI::Xaml::UIElement;
    using Event=winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs;
    using Pointer=winrt::Microsoft::UI::Xaml::Input::Pointer;
    winrt::weak_ref<Element> surface;
    std::shared_ptr<PointerCancelRegistry> registry;
    std::map<uint32_t,Pointer> pointers;
    std::function<void(uint32_t,bool,Event const*)> finish;
    PointerCaptureLoss loss=PointerCaptureLoss::Cancel;
    static void captures(Node const& node,uint32_t id,std::vector<View>& result){
        using winrt::Microsoft::UI::Xaml::Media::VisualTreeHelper;
        if(auto view=node.try_as<View>();view&&PointerGesture::Captured(view,id))result.push_back(view);
        for(int i=0;i<VisualTreeHelper::GetChildrenCount(node);++i)captures(VisualTreeHelper::GetChild(node,i),id,result);
    }
    bool complete(uint32_t id,bool cancel,Event const* event,bool release=true){
        auto found=pointers.find(id);if(found==pointers.end())return false;
        auto pointer=found->second;pointers.erase(found);registry->Forget(id,this);
        std::vector<View> owners;if(release)if(auto view=surface.get())captures(view,id,owners);
        finish(id,cancel,event);
        for(auto const& owner:owners)if(PointerGesture::Captured(owner,id))owner.ReleasePointerCapture(pointer);
        return true;
    }
    bool completeAll(bool release){
        std::vector<uint32_t> ids;for(auto const& [id,pointer]:pointers)ids.push_back(id);
        for(auto id:ids)complete(id,true,nullptr,release);return !ids.empty();
    }
public:
    static std::shared_ptr<ControlCapture> Create(Element const& surface,std::shared_ptr<PointerCancelRegistry> registry,
        std::function<void(uint32_t,bool,Event const*)> finish,PointerCaptureLoss loss=PointerCaptureLoss::Cancel){
        using namespace winrt::Microsoft::UI::Xaml;
        auto self=std::make_shared<ControlCapture>();self->surface=winrt::make_weak(surface);self->registry=std::move(registry);self->finish=std::move(finish);self->loss=loss;
        surface.AddHandler(UIElement::PointerMovedEvent(),winrt::box_value(Input::PointerEventHandler([self](auto&&,Event const& event){
            auto point=event.GetCurrentPoint(nullptr);
            if(point.Properties().IsCanceled()&&self->complete(point.PointerId(),true,&event))event.Handled(true);
        })),true);
        PointerGesture::OnTerminal(surface,[self](auto&&,Event const& event,PointerCompletion completion){
            auto point=event.GetCurrentPoint(nullptr);auto id=point.PointerId();if(!self->Owns(id))return;
            if(completion==PointerCompletion::CaptureLost)if(auto surface=self->surface.get()){
                std::vector<View> owners;captures(surface,id,owners);
                if(!owners.empty()&&owners.front()!=event.OriginalSource().try_as<View>())return;
            }
            bool cancel=CanceledCompletion(completion,point.Properties().IsCanceled(),point.IsInContact(),self->loss);
            self->complete(id,cancel,&event);
        });
        surface.AddHandler(UIElement::KeyDownEvent(),winrt::box_value(Input::KeyEventHandler([self](auto&&,Input::KeyRoutedEventArgs const& event){
            if(event.Key()==winrt::Windows::System::VirtualKey::Escape&&self->Cancel())event.Handled(true);
        })),true);
        surface.Unloaded([self](auto&&,auto&&){self->Cancel();});return self;
    }
    ~ControlCapture(){for(auto const& [id,pointer]:pointers)registry->Forget(id,this);}
    bool Active()const{return !pointers.empty();}
    bool Owns(uint32_t id)const{return pointers.contains(id);}
    bool Begin(Event const& event,bool capture=true){
        auto view=surface.get();if(!view||!view.IsLoaded())return false;
        auto point=event.GetCurrentPoint(view);auto id=point.PointerId();
        if(!PointerGesture::PrimaryContact(point))return false;
        if(Owns(id))Cancel(id);
        if(capture&&!view.CapturePointer(event.Pointer()))return false;
        pointers.emplace(id,event.Pointer());
        registry->Observe(id,this,[weak=weak_from_this(),id]{if(auto self=weak.lock())self->Cancel(id);});return true;
    }
    bool Cancel(uint32_t id){return complete(id,true,nullptr);}
    bool Cancel(){return completeAll(true);}
    bool HandOff(){return completeAll(false);}
};
}
