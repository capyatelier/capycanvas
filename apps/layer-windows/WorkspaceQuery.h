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
}
