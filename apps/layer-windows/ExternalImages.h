#pragma once
#include "UiControls.h"
#include <winrt/Windows.ApplicationModel.DataTransfer.h>
#include <winrt/Windows.Storage.h>
namespace CapyUi {
inline bool fileDrag(DragEventArgs const& event){return event.DataView().Contains(winrt::Windows::ApplicationModel::DataTransfer::StandardDataFormats::StorageItems());}
inline J imageDrop(J const& state){
    auto file=object(state,L"document_file");auto layer=object(object(state,L"layer_tools"),L"editing_layer");
    return O({{L"operation",S(L"drop_images")},{L"epoch",N(num(file,L"epoch"))},{L"revision",N(num(file,L"revision"))},{L"active_layer",N(num(layer,L"id"))}});
}
inline fire_and_forget receiveImageDrop(DragEventArgs event,J action,std::function<void(std::string)> send){
    auto deferral=event.GetDeferral();event.Handled(true);A paths;
    try{auto items=co_await event.DataView().GetStorageItemsAsync();for(auto item:items)if(auto file=item.try_as<winrt::Windows::Storage::StorageFile>())paths.Append(S(file.Path()));}
    catch(hresult_error const&){event.AcceptedOperation(winrt::Windows::ApplicationModel::DataTransfer::DataPackageOperation::None);}
    action.Insert(L"paths",paths);send(to_string(action.Stringify()));deferral.Complete();
}
}
