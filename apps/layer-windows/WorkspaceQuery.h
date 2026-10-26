#pragma once
#include "UiControls.h"

namespace CapyUi {
// Reply runs on the requesting UI thread. Capture weak view owners in reply;
// the canvas owner may discard the callback during shutdown.
inline bool QueryWorkspace(PreviewTransport const& transport,J const& request,
    std::function<void(J)> reply){
    auto dispatcher=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread();
    if(!transport||!dispatcher)return false;
    return transport(CanvasQueryKind::Workspace,to_string(request.Stringify()),
        [dispatcher,reply=std::move(reply)](PreviewPacket packet){
            J result;
            if(packet)try{result=J::Parse(to_hstring(NativeString(capy_preview_metadata(packet.get())).get()));}
                catch(hresult_error const& error){OutputDebugStringW(error.message().c_str());}
            dispatcher.TryEnqueue([reply,result=std::move(result)]{reply(result);});
        });
}
inline void actionTooltip(std::shared_ptr<WorkspaceData> const& data,FrameworkElement const& owner,Control const& target,std::function<J()> action){
    tooltip(owner,AutomationProperties::GetName(target));
    owner.PointerEntered([weak=std::weak_ptr<WorkspaceData>(data),tip=make_weak(owner),named=make_weak(target),action=std::move(action)](auto&&,PointerRoutedEventArgs const& e){
        if(e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch)return;
        auto data=weak.lock();auto target=named.get();if(!data||!target)return;
        auto request=action();if(!request.Size())return;
        auto name=AutomationProperties::GetName(target);
        QueryWorkspace(data->query,O({{L"type",S(L"action_tooltip")},{L"label",S(name)},{L"action",request}}),[tip,named,name](J reply){
            auto owner=tip.get();auto target=named.get();auto text=str(reply,L"result");
            if(owner&&target&&!text.empty()&&AutomationProperties::GetName(target)==name){auto reason=AutomationProperties::GetHelpText(target);tooltip(owner,reason.empty()?text:reason);}
        });
    });
}
inline void actionTooltip(std::shared_ptr<WorkspaceData> const& data,Control const& target,std::function<J()> action){actionTooltip(data,target,target,std::move(action));}
}
