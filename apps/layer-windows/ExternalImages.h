#pragma once
#include "UiControls.h"
#include <winrt/Windows.ApplicationModel.DataTransfer.h>
#include "FileDrop.h"
namespace CapyUi {
inline fire_and_forget classifyDrop(DragEventArgs event,std::shared_ptr<DropKind> kind){
    auto deferral=event.GetDeferral();*kind=DropKind::Images;
    try{
        *kind=fileDropKind(co_await fileDropPaths(event.DataView()));
    }catch(hresult_error const&){}
    deferral.Complete();
}
inline hstring dropCaption(std::shared_ptr<WorkspaceData> const& data,DropKind kind){return data->caption(L"color",kind==DropKind::Drawings?L"drop_drawings":kind==DropKind::Mixed?L"drop_mixed":L"drop_images");}
inline bool fileDrag(DragEventArgs const& event){return event.DataView().Contains(winrt::Windows::ApplicationModel::DataTransfer::StandardDataFormats::StorageItems());}
inline fire_and_forget receiveFileDrop(DragEventArgs event,J action,std::function<void(std::string)> send,std::function<void(std::string)> report){
    auto deferral=event.GetDeferral();event.Handled(true);
    try{event.AcceptedOperation(co_await receiveFileDrop(event.DataView(),action,std::move(send)));}
    catch(hresult_error const& error){
        event.AcceptedOperation(winrt::Windows::ApplicationModel::DataTransfer::DataPackageOperation::None);deferral.Complete();
        if(report)report(fileDropError(error));else throw;co_return;
    }
    deferral.Complete();
}
}
