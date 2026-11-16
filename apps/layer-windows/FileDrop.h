#pragma once
#include "NativeJson.h"
#include <cstdio>
#include <cwchar>
#include <functional>
#include <winrt/Windows.ApplicationModel.DataTransfer.h>
#include <winrt/Windows.Storage.h>

namespace CapyUi {
inline bool drawingFile(hstring const& path){auto dot=std::wstring_view(path).rfind(L'.');return dot!=std::wstring_view::npos&&_wcsicmp(path.c_str()+dot,L".capy")==0;}
enum class DropKind{Images,Drawings,Mixed};
enum class DropTarget{Canvas,Tabs};
inline DropKind fileDropKind(A const& paths){
    uint32_t drawings=0;for(auto path:paths)drawings+=drawingFile(path.GetString());
    return !drawings?DropKind::Images:drawings==paths.Size()?DropKind::Drawings:DropKind::Mixed;
}
inline bool fileDropAllowed(J const& state,DropKind kind,DropTarget target){
    if(target==DropTarget::Canvas&&kind==DropKind::Mixed)return false;
    auto command=target==DropTarget::Tabs||kind==DropKind::Drawings?L"open_document":L"import_image";
    return flag(find(array(state,L"commands"),L"id",command),L"enabled");
}
inline winrt::Windows::Foundation::IAsyncOperation<A> fileDropPaths(winrt::Windows::ApplicationModel::DataTransfer::DataPackageView content){
    using namespace winrt::Windows::ApplicationModel::DataTransfer;
    A paths;if(!content.Contains(StandardDataFormats::StorageItems()))co_return paths;
    for(auto item:co_await content.GetStorageItemsAsync())if(auto file=item.try_as<winrt::Windows::Storage::StorageFile>())paths.Append(JsonValue::CreateStringValue(file.Path()));
    co_return paths;
}
inline J imageDrop(J const& state){
    auto file=object(state,L"document_file"),layer=object(object(state,L"layer_tools"),L"editing_layer");J action;
    action.Insert(L"operation",JsonValue::CreateStringValue(L"drop_images"));
    action.Insert(L"epoch",JsonValue::CreateNumberValue(num(file,L"epoch")));
    action.Insert(L"revision",JsonValue::CreateNumberValue(num(file,L"revision")));
    action.Insert(L"active_layer",JsonValue::CreateNumberValue(num(layer,L"id")));return action;
}
inline J canvasFileDrop(J const& state,winrt::Windows::Foundation::Point point,double scale){
    auto action=imageDrop(state);J screen;
    screen.Insert(L"x",JsonValue::CreateNumberValue(point.X*scale));screen.Insert(L"y",JsonValue::CreateNumberValue(point.Y*scale));
    action.Insert(L"screen",screen);return action;
}
inline J drawingTabsFileDrop(){J action;action.Insert(L"operation",JsonValue::CreateStringValue(L"open_paths"));return action;}
inline winrt::Windows::Foundation::IAsyncOperation<winrt::Windows::ApplicationModel::DataTransfer::DataPackageOperation>
receiveFileDrop(winrt::Windows::ApplicationModel::DataTransfer::DataPackageView content,J action,std::function<void(std::string)> send){
    using namespace winrt::Windows::ApplicationModel::DataTransfer;
    auto paths=co_await fileDropPaths(content);
    if(!paths.Size())co_return DataPackageOperation::None;
    action.Insert(L"paths",paths);send(winrt::to_string(action.Stringify()));co_return DataPackageOperation::Copy;
}
inline winrt::Windows::Foundation::IAsyncOperation<winrt::Windows::ApplicationModel::DataTransfer::DataPackage> fileDropPackage(A paths){
    using namespace winrt::Windows::ApplicationModel::DataTransfer;
    auto files=winrt::single_threaded_vector<winrt::Windows::Storage::IStorageItem>();
    for(auto value:paths)files.Append(co_await winrt::Windows::Storage::StorageFile::GetFileFromPathAsync(value.GetString()));
    DataPackage content;content.RequestedOperation(DataPackageOperation::Copy);content.SetStorageItems(files);co_return content;
}
inline std::string fileDropError(winrt::hresult_error const& error){
    char code[16];std::snprintf(code,sizeof code," [0x%08X]",uint32_t(error.code().value));return winrt::to_string(error.message())+code;
}
}
