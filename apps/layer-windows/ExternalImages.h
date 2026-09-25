#pragma once
#include "UiControls.h"
#include <winrt/Windows.ApplicationModel.DataTransfer.h>
#include <winrt/Windows.Storage.h>
namespace CapyUi {
enum class DropKind{Images,Drawings,Mixed};
inline bool drawingFile(hstring const& path){auto dot=std::wstring_view(path).rfind(L'.');return dot!=std::wstring_view::npos&&_wcsicmp(path.c_str()+dot,L".capy")==0;}
inline fire_and_forget classifyDrop(DragEventArgs event,std::shared_ptr<DropKind> kind){
    auto deferral=event.GetDeferral();*kind=DropKind::Images;
    try{
        uint32_t drawings=0,count=0;
        for(auto item:co_await event.DataView().GetStorageItemsAsync())if(auto file=item.try_as<winrt::Windows::Storage::StorageFile>()){++count;drawings+=drawingFile(file.Path());}
        *kind=drawings==0?DropKind::Images:drawings==count?DropKind::Drawings:DropKind::Mixed;
    }catch(hresult_error const&){}
    deferral.Complete();
}
inline hstring dropCaption(DropKind kind){return kind==DropKind::Drawings?L"Open drawing":kind==DropKind::Mixed?L"Open drawings or place images, not both":L"Place images on canvas";}
inline fire_and_forget receiveDrawingDrop(DragEventArgs event,std::function<void(std::string)> send){
    auto deferral=event.GetDeferral();event.Handled(true);A paths;
    try{for(auto item:co_await event.DataView().GetStorageItemsAsync())if(auto file=item.try_as<winrt::Windows::Storage::StorageFile>();file&&drawingFile(file.Path()))paths.Append(S(file.Path()));}
    catch(hresult_error const&){event.AcceptedOperation(winrt::Windows::ApplicationModel::DataTransfer::DataPackageOperation::None);}
    if(paths.Size())send(to_string(O({{L"operation",S(L"open_paths")},{L"paths",paths}}).Stringify()));
    deferral.Complete();
}
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
