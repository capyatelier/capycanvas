#pragma once
#include "NativeInputReceipt.h"
#include <windows.h>
#include <winrt/Microsoft.UI.Content.h>
#include <winrt/Microsoft.UI.Input.h>
#include <winrt/Microsoft.UI.Xaml.h>
#include <winrt/Microsoft.UI.Xaml.Automation.h>
#include <winrt/Microsoft.UI.Xaml.Input.h>
#include <winrt/Microsoft.UI.Xaml.Media.h>
#include <winrt/Windows.Graphics.h>
#include <functional>
#include <map>

namespace CapyUi {
struct NativeInputSemantic {
    bool canvas=false;
    std::optional<uint64_t> record;
    std::wstring part;
};
class NativeInputObserver {
    using Node=winrt::Microsoft::UI::Xaml::DependencyObject;
    using View=winrt::Microsoft::UI::Xaml::UIElement;
    using Event=winrt::Microsoft::UI::Xaml::Input::PointerRoutedEventArgs;
    using Point=winrt::Windows::Foundation::Point;
    using XamlRoot=winrt::Microsoft::UI::Xaml::XamlRoot;
public:
    using Semantic=std::function<NativeInputSemantic(Node const&,Point)>;
    struct Pending {
        std::shared_ptr<NativeInputJournal> journal;NativeInputDown down;
        void Record(NativeInputAdmission admitted){if(auto output=std::exchange(journal,{})){down.admitted=admitted;output->Record(std::move(down));}}
    };
private:
    struct Owner {std::weak_ptr<NativeInputJournal> journal;std::function<bool()> enabled;Semantic semantic;};
    inline static thread_local std::map<uintptr_t,Owner> owners;
public:
    static std::vector<std::wstring> Path(Node node){
        std::vector<std::wstring> result;
        for(;node;node=winrt::Microsoft::UI::Xaml::Media::VisualTreeHelper::GetParent(node))
            if(auto element=node.try_as<View>())result.emplace_back(winrt::Microsoft::UI::Xaml::Automation::AutomationProperties::GetAutomationId(element));
        return result;
    }
    static uintptr_t Bind(XamlRoot const& root,std::shared_ptr<NativeInputJournal> const& journal,std::function<bool()> enabled,Semantic semantic){
        auto key=reinterpret_cast<uintptr_t>(root.as<::IUnknown>().get());owners[key]={journal,std::move(enabled),std::move(semantic)};return key;
    }
    static void Unbind(uintptr_t key,std::shared_ptr<NativeInputJournal> const& journal){
        auto found=owners.find(key);if(found!=owners.end()&&found->second.journal.lock()==journal)owners.erase(found);
    }
    static Pending Prepare(View const& receiver,Event const& event,NativeInputRoute route=NativeInputRoute::Routed){
        if(owners.empty()||!receiver||!receiver.XamlRoot())return {};
        auto root=receiver.XamlRoot();auto key=reinterpret_cast<uintptr_t>(root.as<::IUnknown>().get());auto found=owners.find(key);
        if(found==owners.end()||!found->second.enabled())return {};
        auto output=found->second.journal.lock();if(!output){owners.erase(found);return {};}
        auto converter=root.CoordinateConverter();if(!converter)return {};
        auto point=event.GetCurrentPoint(root.Content().as<View>());auto screen=converter.ConvertLocalToScreen(point.Position());
        auto source=event.OriginalSource().try_as<Node>();NativeInputDown receipt;
        receipt.pointer=point.PointerId();receipt.time=point.Timestamp();receipt.screen={screen.X,screen.Y};receipt.path=Path(source);
        using Device=winrt::Microsoft::UI::Input::PointerDeviceType;
        receipt.device=point.PointerDeviceType()==Device::Mouse?L"mouse":point.PointerDeviceType()==Device::Pen?L"pen":L"touch";
        receipt.route=route;
        if(found->second.semantic){auto target=found->second.semantic(source,point.Position());receipt.record=target.record;receipt.part=std::move(target.part);}
        return {std::move(output),std::move(receipt)};
    }
    static void Down(View const& receiver,Event const& event,NativeInputAdmission admitted,NativeInputRoute route=NativeInputRoute::Routed){
        auto pending=Prepare(receiver,event,route);pending.Record(admitted);
    }
};
}
