#include "../FileDrop.h"
#include <filesystem>
#include <fstream>
#include <iostream>

using namespace winrt;
using namespace Windows::ApplicationModel::DataTransfer;
using namespace Windows::Storage;
using namespace CapyUi;
static void check(bool value,char const* message){if(!value)throw std::runtime_error(message);}
int wmain(int argc,wchar_t** argv){
    init_apartment();check(argc==2,"File drop checks require an owned output directory");
    auto directory=std::filesystem::path(argv[1])/L"file-drop-data";check(std::filesystem::create_directory(directory),"Fresh file drop output required");
    try{
        auto first=directory/L"photo \u65e5\u672c\u8a9e.png",second=directory/L"saved drawing.CAPY",folder=directory/L"folder.capy";
        std::ofstream(first,std::ios::binary)<<"photo";std::ofstream(second,std::ios::binary)<<"drawing";std::filesystem::create_directory(folder);
        auto photo=StorageFile::GetFileFromPathAsync(first.wstring()).get();auto drawing=StorageFile::GetFileFromPathAsync(second.wstring()).get();auto subfolder=StorageFolder::GetFolderFromPathAsync(folder.wstring()).get();
        auto items=single_threaded_vector<IStorageItem>();items.Append(photo);items.Append(subfolder);items.Append(drawing);
        DataPackage data;data.SetStorageItems(items);
        auto state=J::Parse(LR"({"document_file":{"epoch":19,"revision":23},"layer_tools":{"editing_layer":{"id":7}}})");
        for(bool open:{false,true})for(bool image:{false,true}){
            auto presented=J::Parse(LR"({"commands":[{"id":"open_document","enabled":false},{"id":"import_image","enabled":false}]})");
            auto commands=presented.GetNamedArray(L"commands");commands.GetObjectAt(0).Insert(L"enabled",JsonValue::CreateBooleanValue(open));commands.GetObjectAt(1).Insert(L"enabled",JsonValue::CreateBooleanValue(image));
            for(auto kind:{DropKind::Images,DropKind::Drawings,DropKind::Mixed}){
                bool canvas=kind==DropKind::Images?image:kind==DropKind::Drawings?open:false;
                check(fileDropAllowed(presented,kind,DropTarget::Canvas)==canvas,"Canvas drop ignored its presented command eligibility");
                check(fileDropAllowed(presented,kind,DropTarget::Tabs)==open,"Tab drop ignored its presented command eligibility");
            }
        }
        check(!fileDropAllowed(J{},DropKind::Images,DropTarget::Canvas)&&!fileDropAllowed(J{},DropKind::Drawings,DropTarget::Tabs),"Missing presented commands accepted a drop");
        check(fileDropKind(fileDropPaths(data.GetView()).get())==DropKind::Mixed,"Real StorageItems were not classified as mixed files");
        DataPackage single;auto one=single_threaded_vector<IStorageItem>();one.Append(photo);single.SetStorageItems(one);
        check(fileDropKind(fileDropPaths(single.GetView()).get())==DropKind::Images,"Real image StorageItem was not classified as an image");
        one=single_threaded_vector<IStorageItem>();one.Append(drawing);single.SetStorageItems(one);
        check(fileDropKind(fileDropPaths(single.GetView()).get())==DropKind::Drawings,"Real drawing StorageItem was not classified as a drawing");
        uint32_t calls=0;J request;
        auto send=[&](std::string text){++calls;request=J::Parse(to_hstring(text));};
        for(double scale:{1.,1.5,2.}){
            auto action=canvasFileDrop(state,{12.5f,31.25f},scale);
            check(receiveFileDrop(data.GetView(),action,send).get()==DataPackageOperation::Copy,"StorageFiles were not accepted");
            check(request.GetNamedString(L"operation")==L"drop_images"&&num(request,L"epoch")==19&&num(request,L"revision")==23&&num(request,L"active_layer")==7,"Canvas drop lost its original document target");
            auto at=request.GetNamedObject(L"screen");check(num(at,L"x")==12.5*scale&&num(at,L"y")==31.25*scale,"Canvas drop changed DIP to pixel mapping");
            auto paths=request.GetNamedArray(L"paths");check(paths.Size()==2&&paths.GetStringAt(0)==photo.Path()&&paths.GetStringAt(1)==drawing.Path(),"StorageFile filtering, Unicode or order changed");
        }
        auto layerAction=imageDrop(state);A insertion;insertion.Append(JsonValue::CreateNumberValue(41));insertion.Append(JsonValue::CreateNumberValue(.75));layerAction.Insert(L"layer",insertion);
        check(receiveFileDrop(data.GetView(),layerAction,send).get()==DataPackageOperation::Copy,"Layer drop did not accept StorageFiles");
        auto layer=request.GetNamedArray(L"layer");check(layer.GetNumberAt(0)==41&&layer.GetNumberAt(1)==.75&&!request.HasKey(L"screen")&&num(request,L"active_layer")==7,"Layer drop changed its insertion target or canvas state");
        check(receiveFileDrop(data.GetView(),drawingTabsFileDrop(),send).get()==DataPackageOperation::Copy,"Tab drop did not accept StorageFiles");
        check(request.Size()==2&&request.GetNamedString(L"operation")==L"open_paths"&&request.GetNamedArray(L"paths").Size()==2,"Tab drop changed its command or added a canvas target");
        check(drawingFile(drawing.Path())&&!drawingFile(photo.Path())&&!drawingFile(L"art.capy.png"),"Drawing file extension mapping changed");
        A paths;paths.Append(JsonValue::CreateStringValue(photo.Path()));paths.Append(JsonValue::CreateStringValue(drawing.Path()));
        auto generated=fileDropPackage(paths).get();check(generated.GetView().Contains(StandardDataFormats::StorageItems()),"Smoke package does not contain real StorageItems");
        check(receiveFileDrop(generated.GetView(),drawingTabsFileDrop(),send).get()==DataPackageOperation::Copy,"Smoke package bypassed StorageItems delivery");
        check(request.GetNamedArray(L"paths").GetStringAt(0)==photo.Path()&&request.GetNamedArray(L"paths").GetStringAt(1)==drawing.Path(),"Smoke package changed path order");
        auto before=calls;DataPackage text;text.SetText(L"not files");
        check(receiveFileDrop(text.GetView(),drawingTabsFileDrop(),send).get()==DataPackageOperation::None&&calls==before,"Non-storage data was dispatched as files");
        DataPackage folders;auto onlyFolder=single_threaded_vector<IStorageItem>();onlyFolder.Append(subfolder);folders.SetStorageItems(onlyFolder);
        check(receiveFileDrop(folders.GetView(),drawingTabsFileDrop(),send).get()==DataPackageOperation::None&&calls==before,"A StorageFolder was dispatched as a file");
        auto malformed=[](){DataPackage data;data.SetDataProvider(StandardDataFormats::StorageItems(),[](DataProviderRequest request){request.SetData(box_value(L"wrong storage type"));});return data;};
        auto direct=malformed(),routed=malformed();hresult original=0,propagated=0;
        try{direct.GetView().GetStorageItemsAsync().get();}catch(hresult_error const& error){original=error.code();}
        try{receiveFileDrop(routed.GetView(),drawingTabsFileDrop(),send).get();}catch(hresult_error const& error){propagated=error.code();}
        check(original.value!=0,"Malformed StorageItems did not produce an API error");
        if(propagated.value!=original.value||calls!=before)std::cerr<<"StorageItems HRESULT expected=0x"<<std::hex<<uint32_t(original.value)<<" actual=0x"<<uint32_t(propagated.value)<<std::dec<<" dispatches="<<calls<<" expected="<<before<<"\n";
        check(propagated.value==original.value&&calls==before,"StorageItems HRESULT was swallowed or changed");
        A missing;missing.Append(JsonValue::CreateStringValue((directory/L"missing.png").wstring()));propagated=0;
        try{fileDropPackage(missing).get();}catch(hresult_error const& error){propagated=error.code();}
        check(propagated.value!=0,"A missing smoke StorageFile did not fail");
        auto message=fileDropError(hresult_access_denied(L"File drop original failure"));
        check(message.find("File drop original failure")!=std::string::npos&&message.find("0x80070005")!=std::string::npos,"Native drop report lost the original HRESULT or message");
        std::filesystem::remove_all(directory);std::cout<<"Real DataPackage file filtering, order, target mapping and errors passed\n";
    }catch(hresult_error const& error){std::filesystem::remove_all(directory);std::cerr<<fileDropError(error)<<"\n";return 1;}
    catch(std::exception const& error){std::filesystem::remove_all(directory);std::cerr<<error.what()<<"\n";return 1;}
    return 0;
}
