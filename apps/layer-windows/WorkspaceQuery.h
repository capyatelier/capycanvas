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
            dispatcher.TryEnqueue([reply,packet=std::move(packet)]{
                J result;
                if(packet)try{result=J::Parse(to_hstring(capy_preview_metadata(packet.get())));}
                    catch(hresult_error const& error){OutputDebugStringW(error.message().c_str());}
                reply(result);
            });
        });
}
inline void actionTooltip(std::shared_ptr<WorkspaceData> const& data,Control const& target,std::function<J()> action){
    tooltip(target,AutomationProperties::GetName(target));
    target.PointerEntered([weak=std::weak_ptr<WorkspaceData>(data),owner=make_weak(target),action=std::move(action)](auto&&,PointerRoutedEventArgs const& e){
        if(e.Pointer().PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Touch)return;
        auto data=weak.lock();auto target=owner.get();if(!data||!target)return;
        auto request=action();if(!request.Size())return;
        auto name=AutomationProperties::GetName(target);
        QueryWorkspace(data->query,O({{L"type",S(L"action_tooltip")},{L"label",S(name)},{L"action",request}}),[owner,name](J reply){
            auto target=owner.get();auto text=str(reply,L"result");
            if(target&&!text.empty()&&AutomationProperties::GetName(target)==name)tooltip(target,text);
        });
    });
}
}
