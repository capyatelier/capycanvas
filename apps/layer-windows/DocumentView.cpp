#include "pch.h"
#include "DocumentView.h"
#include "BitmapUpload.h"
#include "UiControls.h"
#include "WorkspaceQuery.h"
#include "ExportForm.h"
#include "ProofForm.h"
#include "NewDrawingDialog.h"
#include <winrt/Microsoft.Windows.Storage.Pickers.h>
#include <microsoft.ui.xaml.window.h>
#include <array>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <robuffer.h>
#include <winrt/Windows.ApplicationModel.DataTransfer.h>
#include <winrt/Windows.Storage.h>
#include <winrt/Windows.Storage.Streams.h>

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace CapyUi;
namespace Pickers=winrt::Microsoft::Windows::Storage::Pickers;

struct DocumentView::Impl : std::enable_shared_from_this<Impl> {
    static constexpr wchar_t ClipNonce[]=L"art.capycanvas.clip.nonce";
    static int depthIndex(hstring const& value){return value==L"F32"?3:value==L"F16"?2:value==L"U16"?1:0;}
    static hstring depthValue(int index){return std::array<hstring,4>{L"U8",L"U16",L"F16",L"F32"}.at(index);}
    Dispatch send,report;
    PreviewTransport query;
    hstring workflowStamp,recoveryStamp,drawingStamp,packageStamp;
    bool busyDialog=false,busyCompleted=false;
    std::function<void()> changed;
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    NativeLocalization localization;
    std::function<void()> presentationChanged,workflowChanged;
    J catalog,model,proofDraft;
    std::unique_ptr<NewDrawingDialog> newDrawing;
    hstring proofProfileId;
    uint32_t proofRequest=0;
    bool proofManaging=false;
    Window window{nullptr};
    ContentDialog dialog{nullptr};
    winrt::Windows::Foundation::IAsyncOperation<Pickers::PickFileResult> picker{nullptr};
    winrt::Windows::Foundation::IAsyncOperation<winrt::Windows::Foundation::Collections::IVectorView<Pickers::PickFileResult>> multiplePicker{nullptr};
    uint32_t handled=0;
    uint32_t interpreted=0;
    bool showing=false,stopping=false;
    std::vector<std::shared_ptr<BitmapUpload>> previewUploads;
    void clearPreviews(){for(auto& upload:previewUploads)upload->Close();previewUploads.clear();}
    ~Impl(){clearPreviews();}


    hstring recovery(wchar_t const* key) const {return str(object(object(catalog,L"bootstrap"),L"recovery"),key);}
    hstring common(wchar_t const* key) const {return str(object(object(catalog,L"bootstrap"),L"common"),key);}
    hstring native(wchar_t const* group,wchar_t const* key) const {return str(object(object(catalog,L"native_copy"),group),key);}
    hstring caption(J const& request) const {
        auto input=to_string(request.Stringify());NativeString raw(capy_native_caption(localization.get(),input.c_str()),capy_string_free);
        if(!raw)throw hresult_error(E_OUTOFMEMORY);auto value=J::Parse(to_hstring(raw.get()));if(value.HasKey(L"error"))throw hresult_invalid_argument(str(value,L"error"));return str(value,L"text");
    }
    LocalizedCopy featureText(wchar_t const* group,wchar_t const* key)const{
        auto resolve=[source=std::weak_ptr<WorkspaceData>(data),group=std::wstring(group),key=std::wstring(key)]{
            if(auto data=source.lock())return str(object(object(object(object(data->model,L"windows_document"),L"details"),L"feature_copy"),group.c_str()),key.c_str());return hstring();
        };return {resolve(),resolve};
    }
    LocalizedCopy interpretationText(wchar_t const* key)const{
        auto resolve=[source=std::weak_ptr<WorkspaceData>(data),key=std::wstring(key)]{
            if(auto data=source.lock())return str(object(object(data->model,L"windows_document"),L"copy"),key.c_str());return hstring();
        };return {resolve(),resolve};
    }
    LocalizedCopy documentText(wchar_t const* key,bool details=true)const{
        auto resolve=[source=std::weak_ptr<WorkspaceData>(data),key=std::wstring(key),details]{
            if(auto data=source.lock()){auto document=object(data->model,L"windows_document");return str(details?object(document,L"details"):document,key.c_str());}return hstring();
        };return {resolve(),resolve};
    }
    LocalizedCopy documentRow(wchar_t const* key,uint32_t index)const{
        auto resolve=[source=std::weak_ptr<WorkspaceData>(data),key=std::wstring(key),index]{
            if(auto data=source.lock()){auto rows=array(object(object(data->model,L"windows_document"),L"details"),key.c_str());if(index<rows.Size()){auto row=rows.GetArrayAt(index);return row.GetStringAt(0)+L"\n"+row.GetStringAt(1);}}return hstring();
        };return {resolve(),resolve};
    }
    static hstring profileLabel(J const& entry){return str(entry,L"name")+L" · "+str(entry,L"state")+(entry.HasKey(L"issue")?L" · "+str(entry,L"issue"):L"");}
    static A profileEntries(J const& details){return array(details,details.HasKey(L"profile_library")?L"profile_library":L"profiles");}
    template<class Control> void copyHeader(Control const& control,LocalizedCopy const& copy){
        auto caption=label(data,hstring(copy));control.Header(caption);AutomationProperties::SetName(control,copy);
        data->copyView([weak=make_weak(control),caption,resolve=copy.current]{auto control=weak.get();if(!control)return false;auto value=resolve();caption.Text(value);AutomationProperties::SetName(control,value);return true;});
    }
    template<class Control> void copyContent(Control const& control,LocalizedCopy const& copy){
        control.FontFamily(FontFamily(L"Segoe UI"));control.FontSize(data->textSize());control.Foreground(data->brush(L"text"));buttonCaption(control,hstring(copy));
        data->copyView([weak=make_weak(control),resolve=copy.current]{auto control=weak.get();if(!control)return false;buttonCaption(control,resolve());return true;});
    }
    fire_and_forget importPaper(J envelope){
        auto lifetime=shared_from_this();auto id=num(envelope,L"id");showing=true;changed();
        J response=O({{L"operation",S(L"paper_import")},{L"id",N(id)},{L"path",JsonValue::CreateNullValue()}});
        try{
            Pickers::FileOpenPicker open(window.AppWindow().Id());open.CommitButtonText(native(L"paper",L"import_accept"));
            for(auto suffix:array(object(object(catalog,L"native_copy"),L"paper"),L"extensions"))open.FileTypeFilter().Append(suffix.GetString());
            picker=open.PickSingleFileAsync();auto selected=co_await picker;if(selected)response.Insert(L"path",S(selected.Path()));
        }catch(hresult_canceled const&){}
        catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        picker=nullptr;if(!stopping)send(to_string(response.Stringify()));showing=false;changed();
    }
    fire_and_forget show(J envelope) {
        auto lifetime=shared_from_this();
        auto request=object(object(envelope,L"kind"),L"request");
        auto type=str(request,L"type");
        if(type==L"export"||type==L"place"||type==L"paste"||type==L"copy"||type==L"change_color"||type==L"color_history"||type==L"properties"||type==L"repair_source_profile"||type==L"rasterize_source"){
            send(to_string(O({{L"operation",S(L"workflow_begin")},{L"id",N(num(envelope,L"id"))}}).Stringify()));co_return;
        }
        if(type==L"new"){
            if(!newDrawing)newDrawing=std::make_unique<NewDrawingDialog>(data,send,window,changed);
            newDrawing->Open(envelope);co_return;
        }
        auto id=num(envelope,L"id");
        auto file=object(object(model,L"state"),L"document_file");
        // Approval applies to the document shown when this dialog opened.
        J response=O({{L"operation",S(L"cancel")},{L"id",N(id)}});A queued;
        if(type==L"confirm_close")response=O({{L"operation",S(L"respond_close")},
            {L"id",N(id)},{L"epoch",N(num(file,L"epoch"))},
            {L"revision",N(num(file,L"revision"))},{L"decision",S(L"cancel")}});
        showing=true;changed();
        try {
            // A taskbar close can request a decision while the owner is minimized.
            // Restore only when this request needs UI, preserving background saves.
            if(type!=L"save"||str(object(request,L"location"),L"uri").empty()){
                HWND handle=nullptr;
                check_hresult(window.as<IWindowNative>()->get_WindowHandle(&handle));
                // SW_RESTORE retains the maximized state from before minimization.
                if(IsIconic(handle))ShowWindow(handle,SW_RESTORE);
            }
            auto options=object(model,L"document_options");
            if(type==L"confirm_close") {
                dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialogSurface(data,dialog);dialogLayout(data,dialog);
                AutomationProperties::SetAutomationId(dialog,L"document-dialog");
                dialog.CloseButtonText(str(options,L"cancel_label"));
                dialog.DefaultButton(ContentDialogButton::Primary);
                auto size=window.Content().XamlRoot().Size();
                StackPanel body;body.Spacing(12);body.Width(std::max(180.,std::min(380.,double(size.Width)-96)));
                presentationChanged=[this,id]{auto options=object(model,L"document_options");auto current=object(object(findId(array(object(model,L"state"),L"requests"),id),L"kind"),L"request");dialog.Title(box_value(str(current,L"title")));dialog.PrimaryButtonText(str(options,L"save_label"));dialog.SecondaryButtonText(str(options,L"discard_label"));dialog.CloseButtonText(str(options,L"cancel_label"));};
                dialog.Title(box_value(str(request,L"title")));
                dialog.PrimaryButtonText(str(options,L"save_label"));
                dialog.SecondaryButtonText(str(options,L"discard_label"));
                auto text=label(data,str(options,L"unsaved_description"));text.TextWrapping(TextWrapping::Wrap);
                body.Children().Append(text);
                ScrollViewer scroll;scroll.MaxHeight(std::max(180.,double(size.Height)-220));scroll.Content(body);dialog.Content(scroll);
                auto choice=co_await dialog.ShowAsync();
                response.Insert(L"decision",S(choice==ContentDialogResult::Primary?L"save":
                    choice==ContentDialogResult::Secondary?L"discard":L"cancel"));
            } else if(type==L"save"||type==L"open") {
                auto path=str(object(request,L"location"),L"uri");
                if(path.empty()) {
                    auto extension=L"."+str(options,L"extension");
                    if(type==L"open") {
                        Pickers::FileOpenPicker open(window.AppWindow().Id());
                        open.CommitButtonText(str(options,L"open_label"));
                        open.FileTypeFilter().Append(extension);
                        for(auto ext:array(options,L"photo_extensions"))open.FileTypeFilter().Append(L"."+ext.GetString());
                        multiplePicker=open.PickMultipleFilesAsync();auto selected=co_await multiplePicker;multiplePicker=nullptr;
                        for(uint32_t i=0;selected&&i<selected.Size();++i){if(i)queued.Append(S(selected.GetAt(i).Path()));else path=selected.GetAt(i).Path();}
                    } else {
                        Pickers::FileSavePicker save(window.AppWindow().Id());
                        save.CommitButtonText(str(options,L"save_label"));
                        save.DefaultFileExtension(extension);save.SuggestedFileName(str(request,L"name"));
                        save.FileTypeChoices().Insert(str(options,L"filter_label"),single_threaded_vector<hstring>({extension}));
                        picker=save.PickSaveFileAsync();
                        auto selected=co_await picker;
                        if(selected)path=selected.Path();
                    }
                }
                if(!path.empty()) {
                    response=O({{L"operation",S(type)},{L"id",N(id)},{L"path",S(path)}});
                    if(type==L"open") {
                        response.Insert(L"epoch",N(num(file,L"epoch")));
                        response.Insert(L"revision",N(num(file,L"revision")));
                    }
                }
            } else if(type==L"import_lookup") {
                auto copy=object(object(model,L"windows_request_copy"),to_hstring(uint64_t(id)).c_str());auto filter=array(copy,L"filter");
                if(filter.Size()!=2)throw hresult_invalid_argument();
                Pickers::FileOpenPicker open(window.AppWindow().Id());
                open.CommitButtonText(str(copy,L"accept_label"));open.SuggestedStartLocation(Pickers::PickerLocationId::Downloads);
                open.FileTypeFilter().Append(L"."+filter.GetStringAt(1));
                picker=open.PickSingleFileAsync();auto selected=co_await picker;picker=nullptr;
                if(selected)response=O({{L"operation",S(L"import_lookup")},{L"id",N(id)},{L"path",S(selected.Path())}});
            } else {
                response=O({{L"operation",S(L"failure")},{L"id",N(id)},
                    {L"error",S(recovery(L"unsupported_operation"))}});
            }
        } catch(hresult_canceled const&) {
            // Picker cancellation is a normal response and never acknowledges a save.
        } catch(hresult_error const& failure) {
            if(!stopping) {
                OutputDebugStringW(failure.message().c_str());auto message=recovery(type==L"confirm_close"?L"unsaved_dialog_failed":L"document_dialog_failed");
                if(type==L"confirm_close")report(to_string(message));
                else response=O({{L"operation",S(L"failure")},{L"id",N(id)},{L"error",S(message)}});
            }
        } catch(std::exception const&) {
            if(!stopping) {
                if(type==L"confirm_close")report(to_string(recovery(L"unsaved_dialog_failed")));
                else response=O({{L"operation",S(L"failure")},{L"id",N(id)},
                    {L"error",S(recovery(L"document_dialog_failed"))}});
            }
        }
        picker=nullptr;clearPreviews();dialog=nullptr;presentationChanged={};
        if(!stopping)send(to_string(response.Stringify()));
        if(!stopping&&queued.Size())send(to_string(O({{L"operation",S(L"open_paths")},{L"paths",queued}}).Stringify()));
        showing=false;
        changed();
    }
    fire_and_forget interpret(J request){
        auto lifetime=shared_from_this();showing=true;changed();auto id=num(request,L"id");
        J profile;bool accepted=false;
        try {
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialogSurface(data,dialog);dialogLayout(data,dialog);dialog.Title(box_value(str(object(request,L"copy"),L"interpret_title")));
            dialog.PrimaryButtonText(recovery(L"open"));dialog.CloseButtonText(common(L"cancel"));
            StackPanel body;body.Spacing(12);auto info=label(data,interpretationText(L"interpret_help"));info.TextWrapping(TextWrapping::Wrap);body.Children().Append(info);
            ComboBox space;choiceColors(data,space,ControlSurface::Dialog);AutomationProperties::SetAutomationId(space,L"document-interpret-profile");copyHeader(space,interpretationText(L"interpret_as"));auto spaces=array(request,L"spaces");
            A choices;for(auto value:spaces){comboOption(space,value.GetArray().GetStringAt(1));choices.Append(O({{L"Builtin",value.GetArray().GetAt(0)}}));}
            for(auto item:array(request,L"profiles")){auto entry=item.GetObject();if(entry.HasKey(L"issue"))continue;comboOption(space,str(entry,L"name"),L"document-interpret-profile-"+str(entry,L"id"));choices.Append(O({{L"library",S(str(entry,L"id"))}}));}
            space.SelectedIndex(0);body.Children().Append(space);dialog.Content(body);
            presentationChanged=[this,space,choices]{
                auto current=object(model,L"windows_document"),copy=object(current,L"copy");
                dialog.Title(box_value(str(copy,L"interpret_title")));dialog.PrimaryButtonText(recovery(L"open"));dialog.CloseButtonText(common(L"cancel"));space.Language(data->language());
                for(uint32_t i=0;i<choices.Size();++i){auto choice=choices.GetObjectAt(i);auto id=str(choice,L"library");
                    if(!id.empty()){auto entry=find(array(current,L"profiles"),L"id",id);if(entry.Size())comboOptionText(space,i,str(entry,L"name"));}
                    else for(auto item:array(current,L"spaces")){auto entry=item.GetArray();if(entry.GetAt(0).Stringify()==choice.GetNamedValue(L"Builtin").Stringify())comboOptionText(space,i,entry.GetStringAt(1));}
                }
            };
            accepted=co_await dialog.ShowAsync()==ContentDialogResult::Primary;
            if(accepted)profile=choices.GetObjectAt(space.SelectedIndex());
        }catch(hresult_error const& e){if(!stopping)report(to_string(e.message()));}
        clearPreviews();dialog=nullptr;presentationChanged={};if(!stopping)send(to_string(O({{L"operation",S(L"interpret")},{L"id",N(id)},
            {L"profile",accepted?V(profile):JsonValue::CreateNullValue()}}).Stringify()));
        showing=false;changed();
    }
    fire_and_forget copied(J request){
        auto lifetime=shared_from_this();auto id=num(request,L"id");auto details=object(request,L"details");
        J action=O({{L"op",S(L"cancel")}});
        try{
            using namespace winrt::Windows::ApplicationModel::DataTransfer;
            auto file=co_await winrt::Windows::Storage::StorageFile::GetFileFromPathAsync(str(details,L"file"));
            auto bytes=co_await winrt::Windows::Storage::FileIO::ReadBufferAsync(file);
            winrt::Windows::Storage::Streams::InMemoryRandomAccessStream png;co_await png.WriteAsync(bytes);png.Seek(0);
            if(stopping)co_return;
            DataPackage package;package.RequestedOperation(DataPackageOperation::Copy);
            package.SetData(L"PNG",png);package.SetData(ClipNonce,box_value(str(details,L"nonce")));
            package.SetBitmap(winrt::Windows::Storage::Streams::RandomAccessStreamReference::CreateFromStream(png));
            Clipboard::SetContent(package);action=O({{L"op",S(L"clip_published")},{L"publication",N(capy_clipboard_publication())}});
        }catch(hresult_error const&){if(!stopping)report(to_string(str(object(details,L"delivery"),L"clipboard_unavailable")));}
        if(!stopping)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",action}}).Stringify()));
    }
    fire_and_forget pickImages(J request){
        auto lifetime=shared_from_this();showing=true;changed();A paths,alternatives;J own;auto id=num(request,L"id");
        try{
            auto details=object(request,L"details");
            if(str(request,L"kind")==L"paste"){
                using namespace winrt::Windows::ApplicationModel::DataTransfer;
                auto content=Clipboard::GetContent();
                auto nonce=str(details,L"clip_nonce");
                try {
                    if(!nonce.empty()&&content.Contains(ClipNonce)&&unbox_value_or<hstring>(co_await content.GetDataAsync(ClipNonce),L"")==nonce)
                        own=O({{L"op",S(L"paste_clip")},{L"nonce",S(nonce)}});
                }catch(hresult_error const&){}
                if(!own.Size()){
                    try {
                        if(content.Contains(StandardDataFormats::StorageItems())){
                            auto items=co_await content.GetStorageItemsAsync();for(auto item:items)if(auto file=item.try_as<winrt::Windows::Storage::StorageFile>())paths.Append(S(file.Path()));
                        }
                    }catch(hresult_error const&){}
                    for(bool bitmap:{false,true}){
                        try {
                            winrt::Windows::Storage::Streams::IRandomAccessStream input{nullptr};
                            if(bitmap&&content.Contains(StandardDataFormats::Bitmap()))input=co_await (co_await content.GetBitmapAsync()).OpenReadAsync();
                            if(!bitmap&&content.Contains(L"PNG"))input=(co_await content.GetDataAsync(L"PNG")).try_as<winrt::Windows::Storage::Streams::IRandomAccessStream>();
                            if(!input)continue;
                            if(input.Size()>512ull*1024*1024)throw hresult_error(E_OUTOFMEMORY);
                            auto target=object(details,L"clipboard");auto folder=co_await winrt::Windows::Storage::StorageFolder::GetFolderFromPathAsync(str(target,L"folder"));
                            auto file=co_await folder.CreateFileAsync(str(target,bitmap?L"bitmap_name":L"name"),winrt::Windows::Storage::CreationCollisionOption::ReplaceExisting);
                            auto output=co_await file.OpenAsync(winrt::Windows::Storage::FileAccessMode::ReadWrite);
                            co_await winrt::Windows::Storage::Streams::RandomAccessStream::CopyAsync(input,output);co_await output.FlushAsync();output.Close();input.Close();
                            A candidate;candidate.Append(S(file.Path()));if(!paths.Size())paths=candidate;else alternatives.Append(candidate);
                        }catch(hresult_error const&){}
                        if(stopping)co_return;
                    }
                    if(!paths.Size())report(to_string(str(object(details,L"delivery"),L"clipboard_empty")));
                }
            }else{
                Pickers::FileOpenPicker open(window.AppWindow().Id());open.CommitButtonText(recovery(L"import"));
                for(auto extension:array(details,L"extensions"))open.FileTypeFilter().Append(L"."+extension.GetString());
                multiplePicker=open.PickMultipleFilesAsync();auto selected=co_await multiplePicker;for(auto file:selected)paths.Append(S(file.Path()));
            }
        }catch(hresult_canceled const&){}catch(hresult_error const& e){if(!stopping)report(to_string(e.message()));}
        multiplePicker=nullptr;
        if(!stopping)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},
            {L"action",own.Size()?own:paths.Size()?O({{L"op",S(L"read_images")},{L"paths",paths},{L"alternatives",alternatives}}):O({{L"op",S(L"cancel")}})}}).Stringify()));
        showing=false;changed();
    }
    void preview(Image const& image,uint32_t id,uint32_t index,std::function<bool()> accepted={},std::shared_ptr<BitmapUpload> upload={}){
        auto queue=window.DispatcherQueue();
        if(!upload)upload=BitmapUpload::Create();
        if(std::find(previewUploads.begin(),previewUploads.end(),upload)==previewUploads.end())previewUploads.push_back(upload);
        query(CanvasQueryKind::Document,to_string(O({{L"id",N(id)},{L"index",N(index)}}).Stringify()),[queue,image,id,accepted,upload,weak=weak_from_this()](PreviewPacket packet){
            queue.TryEnqueue([packet,image,id,accepted,upload,weak]{
                auto self=weak.lock();if(!self||self->stopping||!packet||(accepted&&!accepted()))return;
                try{
                    auto meta=J::Parse(to_hstring(NativeString(capy_preview_metadata(packet.get())).get()));if(uint32_t(num(meta,L"id"))!=id)return;
                    int width=int(num(meta,L"width")),height=int(num(meta,L"height"));if(width<=0||height<=0||width>1024||height>1024)return;
                    auto length=capy_preview_length(packet.get());if(length!=size_t(width)*height*4)return;
                    auto current=[weak,accepted,expected=std::weak_ptr<BitmapUpload>(upload)]{auto self=weak.lock();auto upload=expected.lock();
                        return self&&upload&&!self->stopping&&(!accepted||accepted())&&
                            std::find(self->previewUploads.begin(),self->previewUploads.end(),upload)!=self->previewUploads.end();};
                    upload->Submit({BitmapUpload::ReadPacket(packet),0,length,width,height,true,true,current,
                        [owner=make_weak(image),source=std::weak_ptr<BitmapUpload>(upload),current](bool applied){if(applied&&current())if(auto image=owner.get())if(auto upload=source.lock())image.Source(upload->Image());}});
                }catch(std::exception const&){}catch(hresult_error const&){}
            });
        });
    }
    fire_and_forget package(J request){
        auto lifetime=shared_from_this();showing=true;changed();auto id=uint32_t(num(request,L"id"));auto summary=object(request,L"summary");hstring destination,action=L"close";
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialogSurface(data,dialog);dialogLayout(data,dialog);
            AutomationProperties::SetAutomationId(dialog,L"package-view");dialog.Title(box_value(str(summary,L"status")));dialog.PrimaryButtonText(str(summary,L"copy_original"));dialog.SecondaryButtonText(flag(object(summary,L"capabilities"),L"export")?str(summary,L"export_preview"):L"");dialog.CloseButtonText(str(summary,L"close"));
            StackPanel body;body.Spacing(12);body.MaxWidth(640);
            auto text=[body,data=data](hstring value){auto words=label(data,value);words.TextWrapping(TextWrapping::Wrap);body.Children().Append(words);};text(str(summary,L"reason"));
            for(auto value:array(summary,L"outputs"))text(str(value.GetObject(),L"name"));
            if(flag(object(summary,L"capabilities"),L"view")){Image image;image.MaxHeight(384);image.Stretch(Stretch::Uniform);body.Children().Append(image);preview(image,id,0);}
            dialog.Content(body);presentationChanged=[this]{auto summary=object(object(model,L"windows_document"),L"summary");dialog.Title(box_value(str(summary,L"status")));dialog.PrimaryButtonText(str(summary,L"copy_original"));dialog.SecondaryButtonText(flag(object(summary,L"capabilities"),L"export")?str(summary,L"export_preview"):L"");dialog.CloseButtonText(str(summary,L"close"));};
            auto result=co_await dialog.ShowAsync();clearPreviews();dialog=nullptr;presentationChanged={};
            if((result==ContentDialogResult::Primary||result==ContentDialogResult::Secondary)&&!stopping){
                bool exporting=result==ContentDialogResult::Secondary;Pickers::FileSavePicker save(window.AppWindow().Id());
                if(exporting){
                    std::wstring name(str(request,L"name"));auto extension=name.find_last_of(L'.');if(extension!=std::wstring::npos&&extension!=0)name.resize(extension);
                    save.DefaultFileExtension(L".png");save.SuggestedFileName(hstring(name));save.FileTypeChoices().Insert(str(summary,L"export_preview"),single_threaded_vector<hstring>({L".png"}));
                }else{
                    save.DefaultFileExtension(L".capy");save.SuggestedFileName(str(request,L"name"));save.FileTypeChoices().Insert(str(object(catalog,L"delivery"),L"drawing_type"),single_threaded_vector<hstring>({L".capy"}));
                }
                picker=save.PickSaveFileAsync();auto selected=co_await picker;picker=nullptr;
                if(selected){destination=selected.Path();action=exporting?L"export_preview":L"copy_original";}
            }
        }catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        clearPreviews();dialog=nullptr;presentationChanged={};
        if(!stopping)send(to_string(O({{L"operation",S(L"package")},{L"id",N(id)},{L"action",S(action)},{L"path",action!=L"close"?S(destination):JsonValue::CreateNullValue()}}).Stringify()));
        showing=false;changed();
    }
    winrt::Windows::Foundation::IAsyncOperation<Pickers::PickFileResult> exportDestination(J const& details){
        Pickers::FileSavePicker save(window.AppWindow().Id());auto extension=L"."+str(details,L"extension");save.DefaultFileExtension(extension);save.SuggestedFileName(str(details,L"suggested_name"));
        save.FileTypeChoices().Insert(str(details,L"format_name"),single_threaded_vector<hstring>({extension}));return save.PickSaveFileAsync();
    }
    fire_and_forget colorWorkflow(J request){
        auto lifetime=shared_from_this();showing=true;changed();auto id=uint32_t(num(request,L"id"));auto kind=str(request,L"kind");
        auto details=object(request,L"details");auto spaces=array(request,L"spaces");A profiles;
        J response=O({{L"op",S(L"cancel")}});
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialogSurface(data,dialog);dialogLayout(data,dialog);dialog.CloseButtonText(common(L"cancel"));dialog.PrimaryButtonText(common(L"apply"));dialog.IsPrimaryButtonEnabled(false);
            AutomationProperties::SetAutomationId(dialog,L"document-workflow");
            StackPanel body;body.Spacing(12);body.MaxWidth(std::max(200.,std::min(540.,double(window.Content().XamlRoot().Size().Width)-120)));body.HorizontalAlignment(HorizontalAlignment::Stretch);
            ComboBox space,depth,intent;for(auto box:{space,depth,intent})choiceColors(data,box,ControlSurface::Dialog);auto copy=compactCheck(data,label(data,L"")),dither=compactCheck(data,label(data,L""));auto status=label(data,L"");status.TextWrapping(TextWrapping::Wrap);
            auto text=[&](LocalizedCopy const& value){auto label=CapyUi::label(data,value);label.TextWrapping(TextWrapping::Wrap);body.Children().Append(label);return label;};
            if(kind==L"depth"){
                copyHeader(depth,featureText(L"color",L"depth"));for(auto key:{L"depth_8",L"depth_16",L"depth_float16",L"depth_float32"})comboOption(depth,str(object(object(details,L"feature_copy"),L"color"),key));depth.SelectedIndex(depthIndex(str(object(details,L"color"),L"depth")));body.Children().Append(depth);
                copyContent(dither,featureText(L"color",L"dither_stochastic"));body.Children().Append(dither);
            }else if(kind!=L"rasterize"){
                copyHeader(space,featureText(L"color",kind==L"repair"?L"current_source":L"space"));AutomationProperties::SetAutomationId(space,L"document-profile-choice");
                for(auto item:spaces){auto pair=item.GetArray();comboOption(space,pair.GetStringAt(1));profiles.Append(O({{L"Builtin",pair.GetAt(0)}}));}
                if(kind==L"repair")for(auto item:array(details,L"profiles")){auto entry=item.GetObject();if(entry.HasKey(L"issue"))continue;comboOption(space,str(entry,L"name"));profiles.Append(O({{L"library",S(str(entry,L"id"))}}));}
                space.SelectedIndex(0);for(uint32_t i=0;i<spaces.Size();++i)if(spaces.GetArrayAt(i).GetStringAt(0)==str(object(details,L"color"),L"space"))space.SelectedIndex(i);body.Children().Append(space);
            }
            if(kind==L"convert"){
                copyHeader(intent,featureText(L"export",L"intent"));for(auto key:{L"relative",L"perceptual",L"saturation",L"absolute"})comboOption(intent,str(object(object(details,L"feature_copy"),L"export"),key));intent.SelectedIndex(0);body.Children().Append(intent);
                copyContent(copy,featureText(L"color",L"create_flattened"));body.Children().Append(copy);
            }
            if(kind==L"assign")text(featureText(L"color",L"assign_native_help"));
            if(kind==L"repair"){AutomationProperties::SetAutomationId(text(featureText(L"color",L"current_source")),L"document-source-current-label");AutomationProperties::SetAutomationId(text(documentText(L"source_profile")),L"document-source-profile");}
            if(kind==L"rasterize")text(featureText(L"color",L"source_baked_help"));
            std::array<Image,3> images;std::array<TextBlock,3> headings;
            std::array<std::shared_ptr<BitmapUpload>,3> uploads;for(uint32_t i=0;i<3;++i){uploads[i]=BitmapUpload::Create();}
            for(uint32_t i=0;i<3;++i){headings[i].TextWrapping(TextWrapping::Wrap);body.Children().Append(headings[i]);images[i].MaxHeight(210);images[i].Stretch(Stretch::Uniform);images[i].Visibility(Visibility::Collapsed);body.Children().Append(images[i]);}
            body.Children().Append(status);ScrollViewer scroll;scroll.HorizontalScrollMode(ScrollMode::Disabled);scroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);scroll.HorizontalContentAlignment(HorizontalAlignment::Stretch);scroll.Content(body);scroll.MaxHeight(std::max(180.,double(window.Content().XamlRoot().Size().Height)-220));dialog.Content(scroll);
            auto pending=std::make_shared<bool>(true);auto seen=std::make_shared<hstring>();auto generation=std::make_shared<uint64_t>(0);
            auto prepare=[this,id,kind,space,depth,intent,copy,dither,spaces,profiles,pending,generation]{
                if(stopping)return;*pending=true;++*generation;dialog.IsPrimaryButtonEnabled(false);
                V choice=JsonValue::CreateNullValue();
                if(kind==L"assign")choice=O({{L"Assign",spaces.GetArrayAt(space.SelectedIndex()).GetAt(0)}});
                if(kind==L"convert")choice=O({{L"Convert",O({{L"space",spaces.GetArrayAt(space.SelectedIndex()).GetAt(0)},{L"options",O({{L"intent",S(std::array<hstring,4>{L"RelativeColorimetric",L"Perceptual",L"Saturation",L"AbsoluteColorimetric"}[intent.SelectedIndex()])},{L"black_point_compensation",B(false)}})}})}});
                if(kind==L"depth")choice=O({{L"Depth",O({{L"depth",S(depthValue(depth.SelectedIndex()))},{L"dither",S(depth.SelectedIndex()==0&&dither.IsChecked().Value()?L"Stochastic8":L"None")}})}});
                if(kind==L"repair")choice=profiles.GetAt(space.SelectedIndex());
                send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",O({{L"op",S(L"prepare")},{L"choice",choice},{L"copy",B(kind==L"convert"&&copy.IsChecked().Value())}})}}).Stringify()));
            };
            for(auto control:{space,depth,intent})control.SelectionChanged([prepare](auto&&,auto&&){prepare();});copy.Click([prepare](auto&&,auto&&){prepare();});dither.Click([prepare](auto&&,auto&&){prepare();});
            workflowChanged=[this,id,kind,pending,seen,generation,images,headings,status,depth,intent,uploads]{
                auto current=object(model,L"windows_document");if(uint32_t(num(current,L"id"))!=id)return;
                if(str(current,L"type")==L"workflow_busy"){*pending=true;dialog.IsPrimaryButtonEnabled(false);return;}
                auto serial=to_hstring(uint32_t(num(current,L"serial")))+data->language();if(serial==*seen)return;*seen=serial;
                auto details=object(current,L"details"),feature=object(details,L"feature_copy"),copy=object(feature,L"color");
                dialog.Title(box_value(str(copy,kind==L"assign"?L"assign_title":kind==L"convert"?L"convert_title":kind==L"depth"?L"depth_title":kind==L"repair"?L"repair_title":L"rasterize_title")));
                dialog.PrimaryButtonText(flag(details,L"copy")?str(copy,L"create_copy"):common(L"apply"));dialog.CloseButtonText(common(L"cancel"));status.Text(str(current,L"error"));
                if(kind==L"depth"){std::array<hstring,4> labels{str(copy,L"depth_8"),str(copy,L"depth_16"),str(copy,L"depth_float16"),str(copy,L"depth_float32")};for(uint32_t i=0;i<labels.size();++i)comboOptionText(depth,i,labels[i]);}
                if(kind==L"convert"){auto output=object(feature,L"export");std::array<hstring,4> labels{str(output,L"relative"),str(output,L"perceptual"),str(output,L"saturation"),str(output,L"absolute")};for(uint32_t i=0;i<labels.size();++i)comboOptionText(intent,i,labels[i]);}
                bool ready=str(current,L"stage")==L"preview"&&status.Text().empty();*pending=!ready;dialog.IsPrimaryButtonEnabled(ready&&!flag(details,L"range_blocked"));
                if(!ready)return;
                hstring message=data->copyCaption(O({{L"type",S(L"inspection_clipped")},{L"count",N(num(details,L"clipped_channels"))}}));
                if(flag(details,L"range_blocked"))message=message+L"\n"+str(object(feature,L"export"),L"outside_range");
                if(flag(details,L"adds_layer"))message=message+L"\n"+str(copy,L"adds_layer");status.Text(message);
                auto sdr=flag(details,L"has_sdr_preview");auto token=++*generation;
                for(uint32_t i=0;i<3;++i){bool visible=i<(sdr?3u:2u);images[i].Visibility(visible?Visibility::Visible:Visibility::Collapsed);headings[i].Visibility(visible?Visibility::Visible:Visibility::Collapsed);if(!visible)continue;
                    headings[i].Text(i==0?str(copy,L"before"):i==2?str(object(feature,L"export"),L"sdr_base"):sdr?str(object(feature,L"export"),L"preview_sdr"):str(copy,L"after"));
                    preview(images[i],id,i,[generation,token]{return *generation==token;},uploads[i]);
                }
            };
            dialog.PrimaryButtonClick([pending](auto&&,ContentDialogButtonClickEventArgs const& event){if(*pending)event.Cancel(true);});
            auto shown=dialog.ShowAsync();prepare();auto result=co_await shown;++*generation;
            workflowChanged={};auto current=object(model,L"windows_document");workflowStamp=to_hstring(id)+L"/"+to_hstring(uint32_t(num(current,L"serial")));
            if(result==ContentDialogResult::Primary&&!*pending)response=O({{L"op",S(flag(object(current,L"details"),L"copy")?L"open_copy":L"commit")}});
        }catch(hresult_canceled const&){}catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        workflowChanged={};clearPreviews();dialog=nullptr;
        if(!stopping)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",response}}).Stringify()));showing=false;changed();
    }
    fire_and_forget workflow(J request){
        auto lifetime=shared_from_this();auto id=uint32_t(num(request,L"id"));
        auto workflowKind=str(request,L"kind");
        if(workflowKind==L"assign"||workflowKind==L"convert"||workflowKind==L"depth"||workflowKind==L"repair"||workflowKind==L"rasterize"){colorWorkflow(request);co_return;}
        if(str(request,L"kind")==L"copy"&&str(request,L"stage")==L"commit"){copied(request);co_return;}
        if(str(request,L"stage")==L"commit"){
            send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",O({{L"op",S(L"commit")}})}}).Stringify()));co_return;
        }
        showing=true;changed();
        auto kind=str(request,L"kind"),stage=str(request,L"stage");auto details=object(request,L"details");
        if((kind==L"place"||kind==L"paste")&&stage==L"options"){showing=false;pickImages(request);co_return;}
        if(kind==L"export"&&stage==L"repeat_pick"){
            J chosen=O({{L"op",S(L"cancel")}});
            try{picker=exportDestination(details);auto selected=co_await picker;if(selected)chosen=O({{L"op",S(L"export_write")},{L"path",S(selected.Path())}});}
            catch(hresult_canceled const&){}catch(hresult_error const& e){if(!stopping)report(to_string(e.message()));}
            picker=nullptr;if(!stopping)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",chosen}}).Stringify()));
            showing=false;changed();co_return;
        }
        J action=O({{L"op",S(L"cancel")}});
        auto scripted=std::make_shared<J>();std::shared_ptr<ExportFormView> exportForm;std::shared_ptr<ProofFormView> proofForm;ComboBox profileList;choiceColors(data,profileList,ControlSurface::Dialog);TextBox profileSearch;A profileChoices;std::function<void()> updateProfileVisibility,filterProfiles,workflowApplied;
        if(kind==L"proof"&&proofRequest!=id){proofRequest=id;proofDraft=J();proofProfileId=L"";proofManaging=false;}
        bool library=kind==L"profiles"||(kind==L"proof"&&proofManaging);
        auto feature=object(details,L"feature_copy"),colorCopy=object(feature,L"color"),exportCopy=object(feature,L"export"),profileCopy=object(feature,L"profile"),proofCopy=object(feature,L"proof");
        auto colorText=[&](wchar_t const* key){return str(colorCopy,key);};
        auto exportText=[&](wchar_t const* key){return str(exportCopy,key);};
        auto profileText=[&](wchar_t const* key){return str(profileCopy,key);};
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialogSurface(data,dialog);dialogLayout(data,dialog);dialog.CloseButtonText(common(L"cancel"));
            AutomationProperties::SetAutomationId(dialog,L"document-workflow");
            auto title=library?profileText(L"library_title"):kind==L"proof"?str(proofCopy,L"title"):kind==L"export"?exportText(L"title"):kind==L"assign"?colorText(L"assign_title"):kind==L"convert"?colorText(L"convert_title"):kind==L"depth"?colorText(L"depth_title"):
                kind==L"place"||kind==L"paste"?profileText(L"interpret_title"):kind==L"repair"?colorText(L"repair_title"):kind==L"rasterize"?colorText(L"rasterize_title"):str(details,L"title");
            dialog.Title(box_value(title));
            StackPanel body;body.Spacing(12);body.MaxWidth(std::max(200.,std::min(540.,double(window.Content().XamlRoot().Size().Width)-120)));body.HorizontalAlignment(HorizontalAlignment::Stretch);
            auto text=[&](auto const& value){auto item=label(data,value);item.TextWrapping(TextWrapping::Wrap);body.Children().Append(item);return item;};
            ComboBox space;choiceColors(data,space,ControlSurface::Dialog);
            auto spaces=array(request,L"spaces");
            auto error=text(documentText(L"error",false));AutomationProperties::SetAutomationId(error,L"document-workflow-error");error.Visibility(str(request,L"error").empty()?Visibility::Collapsed:Visibility::Visible);
            if(stage==L"error"&&kind!=L"proof"){
                dialog.CloseButtonText(common(L"close"));
            }else if(library){
                AutomationProperties::SetAutomationId(text(featureText(L"profile",L"library_help")),L"profile-library-help");
                AutomationProperties::SetAutomationId(profileList,L"document-profile-library");auto entries=profileEntries(details);copyHeader(profileList,featureText(L"profile",L"library_title"));profileList.HorizontalAlignment(HorizontalAlignment::Stretch);
                auto search=profileSearch;fieldColors(data,search,ControlSurface::Dialog);search.PlaceholderText(native(L"header",L"search"));AutomationProperties::SetAutomationId(search,L"profile-library-search");search.Visibility(entries.Size()>7?Visibility::Visible:Visibility::Collapsed);body.Children().Append(search);
                for(auto item:entries){auto entry=item.GetObject();comboOption(profileList,profileLabel(entry),L"document-profile-"+str(entry,L"id"));}
                auto searchSerial=std::make_shared<uint64_t>(0);
                auto filter=std::make_shared<std::shared_ptr<std::function<void(uint32_t)>>>();
                filterProfiles=[weak=weak_from_this(),owner=make_weak(dialog),search,profileList,searchSerial,filter,id]{
                    auto self=weak.lock();if(!self)return;auto entries=profileEntries(object(object(self->model,L"windows_document"),L"details"));
                    auto query=search.Text();auto serial=++*searchSerial;
                    *filter=std::make_shared<std::function<void(uint32_t)>>();
                    **filter=[weak,owner,query,profileList,entries,searchSerial,serial,next=std::weak_ptr<std::shared_ptr<std::function<void(uint32_t)>>>(filter),id](uint32_t index){
                        auto self=weak.lock();if(!self||!self->showing||(!owner.get()||self->dialog!=owner.get())||*searchSerial!=serial||index>=entries.Size())return;
                        auto entry=entries.GetObjectAt(index);auto item=profileList.Items().GetAt(index).as<ComboBoxItem>();
                        bool queued=QueryWorkspace(self->data->query,O({{L"type",S(L"profile_library_matches")},{L"entry",object(entry,L"query_entry")},{L"query",S(query)}}),[weak,owner,searchSerial,serial,item,profileList,index,next,id](J reply){
                            if(auto self=weak.lock();self&&self->showing&&owner.get()&&self->dialog==owner.get()&&*searchSerial==serial&&uint32_t(num(object(self->model,L"windows_document"),L"id"))==id){
                                bool matched=flag(reply,L"result");item.Visibility(matched?Visibility::Visible:Visibility::Collapsed);
                                if(!matched&&profileList.SelectedIndex()==int32_t(index))profileList.SelectedIndex(-1);
                                self->dialog.IsSecondaryButtonEnabled(profileList.SelectedIndex()>=0);
                                if(auto continuation=next.lock();continuation&&*continuation)(**continuation)(index+1);
                            }
                        });
                        if(!queued){auto timer=self->window.DispatcherQueue().CreateTimer();timer.IsRepeating(false);timer.Interval(std::chrono::milliseconds(16));timer.Tick([next,index](auto&& timer,auto&&){timer.Stop();if(auto continuation=next.lock();continuation&&*continuation)(**continuation)(index);});timer.Start();}
                    };
                    (**filter)(0);
                };
                search.TextChanged([filterProfiles](auto&&,auto&&){filterProfiles();});
                Button visibility;buttonColors(data,visibility);visibility.HorizontalAlignment(HorizontalAlignment::Left);AutomationProperties::SetAutomationId(visibility,L"profile-visibility");
                updateProfileVisibility=[this,button=make_weak(visibility),list=make_weak(profileList)]{auto target=button.get();auto box=list.get();if(!target||!box)return;
                    auto entries=profileEntries(object(object(model,L"windows_document"),L"details"));auto index=box.SelectedIndex();
                    bool valid=index>=0&&uint32_t(index)<entries.Size();bool shown=!valid||flag(entries.GetObjectAt(uint32_t(index)),L"visible",true);auto copy=object(object(object(object(model,L"windows_document"),L"details"),L"feature_copy"),L"profile");
                    auto text=str(copy,shown?L"hide":L"show");buttonCaption(target,text);AutomationProperties::SetName(target,text);target.IsEnabled(valid);
                };
                profileList.SelectionChanged([this,profileList,updateProfileVisibility](auto&&,auto&&){updateProfileVisibility();if(dialog)dialog.IsSecondaryButtonEnabled(profileList.SelectedIndex()>=0);});
                visibility.Click([this,id,list=make_weak(profileList)](auto&&,auto&&){
                    auto box=list.get();auto index=box?box.SelectedIndex():-1;auto entries=profileEntries(object(object(model,L"windows_document"),L"details"));if(index<0||uint32_t(index)>=entries.Size())return;auto entry=entries.GetObjectAt(uint32_t(index));
                    send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",O({{L"op",S(L"profile_visibility")},{L"id",S(str(entry,L"id"))},{L"visible",B(!flag(entry,L"visible",true))}})}}).Stringify()));
                });
                if(entries.Size())profileList.SelectedIndex(0);updateProfileVisibility();
                body.Children().Append(profileList);body.Children().Append(visibility);
                dialog.PrimaryButtonText(profileText(L"add_profile_dialog"));dialog.SecondaryButtonText(profileText(L"remove"));dialog.IsSecondaryButtonEnabled(entries.Size()!=0);dialog.CloseButtonText(common(L"done"));
            }else if(kind==L"proof"){
                proofForm=std::make_shared<ProofFormView>(data);proofForm->root.Language(data->language());proofForm->init(details,proofDraft,proofProfileId);body.Children().Append(proofForm->root);
                for(auto box:{proofForm->profile,proofForm->intent,proofForm->simulation})choiceColors(data,box,ControlSurface::Dialog);
                presentationChanged=[this,proofForm]{
                    auto details=object(object(model,L"windows_document"),L"details"),copy=object(object(details,L"feature_copy"),L"proof");
                    proofForm->relocalize(localization,details,data->language());dialog.Title(box_value(str(copy,L"title")));dialog.PrimaryButtonText(common(L"apply"));dialog.CloseButtonText(common(L"cancel"));
                };
                StackPanel buttons;buttons.Orientation(Orientation::Horizontal);buttons.Spacing(8);
                auto add=[&](LocalizedCopy title,hstring op){
                    auto button=CapyUi::button(data,title,[]{});
                    button.Click([this,proofForm,scripted,op](auto&&,auto&&){
                        proofDraft=proofForm->current();proofProfileId=proofForm->profileId;
                        *scripted=O({{L"op",S(op)}});dialog.Hide();
                    });buttons.Children().Append(button);
                };
                add(featureText(L"profile",L"add_profile_dialog"),L"proof_import");add(featureText(L"profile",L"manage"),L"proof_manage");body.Children().Append(buttons);
                dialog.PrimaryButtonText(common(L"apply"));
            }else if(kind==L"export"&&stage==L"options"){
                exportForm=std::make_shared<ExportFormView>(data);exportForm->root.Language(data->language());exportForm->init(details);exportForm->validation.Foreground(data->brush(L"error"));
                presentationChanged=[this,exportForm]{
                    auto details=object(object(model,L"windows_document"),L"details"),copy=object(object(details,L"feature_copy"),L"export");
                    exportForm->relocalize(localization,details,data->language());exportForm->validation.Foreground(data->brush(L"error"));
                    dialog.Title(box_value(str(copy,L"title")));dialog.PrimaryButtonText(str(copy,L"preview"));dialog.CloseButtonText(common(L"cancel"));
                };
                auto presetUpdating=std::make_shared<bool>(false);
                auto presets=object(details,L"presets");ComboBox preset;choiceColors(data,preset,ControlSurface::Dialog);copyHeader(preset,featureText(L"export",L"preset"));preset.HorizontalAlignment(HorizontalAlignment::Stretch);
                for(auto name:array(presets,L"names"))comboOption(preset,name.GetString());auto selectedPreset=presets.GetNamedValue(L"index",JsonValue::CreateNullValue());preset.SelectedIndex(selectedPreset.ValueType()==JsonValueType::Number?int(selectedPreset.GetNumber()):0);body.Children().Append(preset);
                body.Children().Append(exportForm->root);TextBox name;fieldColors(data,name,ControlSurface::Dialog);AutomationProperties::SetAutomationId(name,L"export-preset-name");copyHeader(name,featureText(L"export",L"preset_name"));name.MaxLength(64);body.Children().Append(name);
                auto operate=[this,id,exportForm](J operation){
                    send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",O({{L"op",S(L"export_preset")},{L"action",operation},{L"profile_id",exportForm->profileId.empty()?JsonValue::CreateNullValue():S(exportForm->profileId)}})}}).Stringify()));
                };
                preset.SelectionChanged([preset,operate,presetUpdating](auto&&,auto&&){if(!*presetUpdating&&preset.SelectedIndex()>=0)operate(O({{L"type",S(L"get")},{L"index",N(preset.SelectedIndex())}}));});
                StackPanel buttons;buttons.Orientation(Orientation::Horizontal);buttons.Spacing(6);
                auto add=[&](LocalizedCopy title,std::function<void()> invoke){auto button=CapyUi::button(data,title,[]{});button.Click([invoke,exportForm](auto&&,auto&&){try{invoke();}catch(hresult_error const& error){exportForm->validation.Text(error.message());}});buttons.Children().Append(button);return button;};
                add(featureText(L"export",L"save_preset"),[name,operate,exportForm]{operate(O({{L"type",S(L"save")},{L"name",S(name.Text())},{L"recipe",exportForm->current()}}));});
                add(data->copyCommon(L"update"),[preset,operate,exportForm]{operate(O({{L"type",S(L"update")},{L"index",N(preset.SelectedIndex())},{L"recipe",exportForm->current()}}));});
                auto remove=add(data->copyCommon(preset.SelectedIndex()<4?L"reset":L"remove"),[preset,operate]{operate(O({{L"type",S(preset.SelectedIndex()<4?L"reset":L"remove")},{L"index",N(preset.SelectedIndex())}}));});body.Children().Append(buttons);
                auto formChanged=presentationChanged;
                presentationChanged=[this,formChanged,preset,presetUpdating,remove]{
                    formChanged();*presetUpdating=true;
                    auto selected=preset.SelectedIndex();auto names=array(object(object(object(model,L"windows_document"),L"details"),L"presets"),L"names");
                    for(uint32_t i=0;i<names.Size();++i){if(i<preset.Items().Size())comboOptionText(preset,i,names.GetStringAt(i));else comboOption(preset,names.GetStringAt(i));}
                    while(preset.Items().Size()>names.Size())preset.Items().RemoveAtEnd();
                    preset.SelectedIndex(selected>=0&&uint32_t(selected)<names.Size()?selected:names.Size()?0:-1);*presetUpdating=false;
                    buttonCaption(remove,common(preset.SelectedIndex()<4?L"reset":L"remove"));
                };
                workflowApplied=[this,exportForm,preset,presetUpdating,remove]{
                    auto details=object(object(model,L"windows_document"),L"details");exportForm->apply(details);
                    *presetUpdating=true;auto selected=object(details,L"presets").GetNamedValue(L"index",JsonValue::CreateNullValue());
                    preset.SelectedIndex(selected.ValueType()==JsonValueType::Number?int32_t(selected.GetNumber()):0);*presetUpdating=false;
                    buttonCaption(remove,common(preset.SelectedIndex()<4?L"reset":L"remove"));
                };
                dialog.PrimaryButtonText(exportText(L"preview"));
                dialog.PrimaryButtonClick([exportForm](auto&&,ContentDialogButtonClickEventArgs const& e){try{exportForm->current();}catch(hresult_error const& error){exportForm->validation.Text(error.message());e.Cancel(true);}});
            }else if(kind==L"properties"){
                presentationChanged=[this]{auto details=object(object(model,L"windows_document"),L"details");dialog.Title(box_value(str(details,L"title")));dialog.CloseButtonText(str(details,L"done"));dialog.PrimaryButtonText(str(details,L"proof"));};
                for(uint32_t i=0;i<array(details,L"rows").Size();++i)text(documentRow(L"rows",i));
                if(array(details,L"sources").Size())text(documentText(L"source_images"));
                for(uint32_t i=0;i<array(details,L"sources").Size();++i)text(documentRow(L"sources",i));dialog.CloseButtonText(str(details,L"done"));dialog.PrimaryButtonText(str(details,L"proof"));
            }else if(stage==L"preview"){
                text(featureText(L"color",flag(details,L"copy")?L"clipped_comparison":L"compare_before_apply"));
                auto sdr=flag(details,L"has_sdr_preview");
                for(uint32_t i=0;i<(sdr?3u:2u);++i){text(i==2?featureText(L"export",L"sdr_base"):i?(sdr?featureText(L"export",L"preview_sdr"):featureText(L"color",L"after")):featureText(L"color",L"before"));Image image;image.MaxHeight(210);image.Stretch(Stretch::Uniform);body.Children().Append(image);preview(image,id,i);}
                text(data->copyCaption(O({{L"type",S(L"inspection_clipped")},{L"count",N(num(details,L"clipped_channels"))}})));
                if(flag(details,L"range_blocked"))text(featureText(L"export",L"outside_range"));
                if(flag(details,L"adds_layer"))text(featureText(L"color",L"adds_layer"));
                dialog.PrimaryButtonText(kind==L"export"?exportText(L"export"):common(L"apply"));dialog.IsPrimaryButtonEnabled(!flag(details,L"range_blocked"));
            }else{
                AutomationProperties::SetAutomationId(space,L"document-profile-choice");copyHeader(space,featureText(L"color",L"current_source"));
                for(auto entry:spaces){comboOption(space,entry.GetArray().GetStringAt(1));profileChoices.Append(O({{L"Builtin",entry.GetArray().GetAt(0)}}));}
                for(auto item:array(details,L"profiles")){auto entry=item.GetObject();if(entry.HasKey(L"issue"))continue;comboOption(space,str(entry,L"name"),L"document-profile-choice-"+str(entry,L"id"));profileChoices.Append(O({{L"library",S(str(entry,L"id"))}}));}
                space.SelectedIndex(0);body.Children().Append(space);dialog.PrimaryButtonText(colorText(L"add_source"));
            }
            ScrollViewer scroll;scroll.HorizontalScrollMode(ScrollMode::Disabled);scroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);scroll.HorizontalContentAlignment(HorizontalAlignment::Stretch);scroll.Content(body);scroll.MaxHeight(std::max(180.,double(window.Content().XamlRoot().Size().Height)-220));dialog.Content(scroll);
            auto projected=presentationChanged;
            presentationChanged=[this,projected,kind,stage,library,space,profileList,profileSearch,profileChoices,updateProfileVisibility,filterProfiles]{
                if(projected){projected();return;}
                auto current=object(model,L"windows_document"),details=object(current,L"details"),feature=object(details,L"feature_copy");
                auto color=[&](wchar_t const* key){return str(object(feature,L"color"),key);};
                auto output=[&](wchar_t const* key){return str(object(feature,L"export"),key);};
                auto profile=[&](wchar_t const* key){return str(object(feature,L"profile"),key);};
                auto title=library?profile(L"library_title"):kind==L"assign"?color(L"assign_title"):kind==L"convert"?color(L"convert_title"):kind==L"depth"?color(L"depth_title"):kind==L"place"||kind==L"paste"?profile(L"interpret_title"):kind==L"repair"?color(L"repair_title"):kind==L"rasterize"?color(L"rasterize_title"):str(details,L"title");
                dialog.Title(box_value(title));dialog.CloseButtonText(common(stage==L"error"?L"close":library?L"done":L"cancel"));
                if(library){profileSearch.PlaceholderText(native(L"header",L"search"));profileSearch.Language(data->language());dialog.PrimaryButtonText(profile(L"add_profile_dialog"));dialog.SecondaryButtonText(profile(L"remove"));
                    auto selected=profileList.SelectedIndex();auto profiles=profileEntries(details);
                    for(uint32_t i=0;i<profiles.Size();++i){auto entry=profiles.GetObjectAt(i);if(i<profileList.Items().Size())comboOptionText(profileList,i,profileLabel(entry));else comboOption(profileList,profileLabel(entry));AutomationProperties::SetAutomationId(profileList.Items().GetAt(i).as<ComboBoxItem>(),L"document-profile-"+str(entry,L"id"));}
                    while(profileList.Items().Size()>profiles.Size())profileList.Items().RemoveAtEnd();
                    profileList.SelectedIndex(selected>=0&&uint32_t(selected)<profiles.Size()?selected:-1);if(updateProfileVisibility)updateProfileVisibility();if(filterProfiles)filterProfiles();
                }
                else if(stage==L"preview")dialog.PrimaryButtonText(kind==L"export"?output(L"export"):common(L"apply"));
                else if(stage!=L"error")dialog.PrimaryButtonText(color(L"add_source"));
                for(auto control:{space,profileList})control.Language(data->language());
                for(uint32_t i=0;i<profileChoices.Size();++i){auto choice=profileChoices.GetObjectAt(i);auto id=str(choice,L"library");
                    if(!id.empty()){auto entry=find(array(details,L"profiles"),L"id",id);if(entry.Size())comboOptionText(space,i,str(entry,L"name"));}
                    else for(auto item:array(current,L"spaces")){auto entry=item.GetArray();if(entry.GetAt(0).Stringify()==choice.GetNamedValue(L"Builtin").Stringify())comboOptionText(space,i,entry.GetStringAt(1));}
                }
            };
            if(library||(kind==L"export"&&stage==L"options")){
                auto serial=std::make_shared<uint32_t>(uint32_t(num(request,L"serial")));
                workflowChanged=[this,id,kind,stage,serial,workflowApplied,error]{
                    auto current=object(model,L"windows_document");auto next=uint32_t(num(current,L"serial"));
                    if(uint32_t(num(current,L"id"))!=id||str(current,L"kind")!=kind||next==*serial)return;
                    if(str(current,L"stage")!=stage&&str(current,L"stage")!=L"error")return;
                    *serial=next;workflowStamp=to_hstring(id)+L"/"+to_hstring(next);
                    if(presentationChanged)presentationChanged();
                    auto message=str(current,L"error");if(message.empty()&&workflowApplied)workflowApplied();
                    error.Text(documentText(L"error",false));error.Visibility(message.empty()?Visibility::Collapsed:Visibility::Visible);dialog.IsPrimaryButtonEnabled(message.empty());
                };
            }
            auto result=co_await dialog.ShowAsync();workflowChanged={};
            if(library&&result==ContentDialogResult::Secondary&&profileList.SelectedIndex()>=0)
                action=O({{L"op",S(L"profile_remove")},{L"id",S(str(profileEntries(object(object(model,L"windows_document"),L"details")).GetObjectAt(profileList.SelectedIndex()),L"id"))}});
            if(result==ContentDialogResult::Primary){
                if(library){
                    action=O({{L"op",S(L"describe")}});
                    Pickers::FileOpenPicker open(window.AppWindow().Id());open.FileTypeFilter().Append(L".icc");open.FileTypeFilter().Append(L".icm");picker=open.PickSingleFileAsync();
                    auto selected=co_await picker;if(selected)action=O({{L"op",S(L"profile_import")},{L"path",S(selected.Path())}});
                }else if(kind==L"properties"){
                    action=O({{L"op",S(L"appearance")}});
                }else if(kind==L"proof"){
                    proofDraft=proofForm->current();proofProfileId=proofForm->profileId;
                    action=O({{L"op",S(L"proof_options")},{L"settings",proofDraft},{L"profile_id",proofProfileId.empty()?JsonValue::CreateNullValue():S(proofProfileId)}});
                }else if(kind==L"export"&&stage==L"options"){
                    action=O({{L"op",S(L"export_options")},{L"recipe",exportForm->current()},{L"profile_id",exportForm->profileId.empty()?JsonValue::CreateNullValue():S(exportForm->profileId)}});
                }else if(kind==L"export"&&stage==L"preview"){
                    picker=exportDestination(details);
                    auto selected=co_await picker;if(selected)action=O({{L"op",S(L"export_write")},{L"path",S(selected.Path())}});
                }else if(stage==L"preview"){
                    action=O({{L"op",S(L"commit")}});
                }else{
                    action=O({{L"op",S(L"interpret_image")},{L"profile",profileChoices.GetAt(space.SelectedIndex())}});
                }
            }
            if(kind==L"proof"&&library&&result==ContentDialogResult::None){proofManaging=false;action=O({{L"op",S(L"describe")}});}
            if(str(*scripted,L"op")==L"proof_manage"){proofManaging=true;*scripted=O({{L"op",S(L"describe")}});}
            if(str(*scripted,L"op")==L"proof_import"){
                *scripted=O({{L"op",S(L"describe")}});
                Pickers::FileOpenPicker open(window.AppWindow().Id());open.FileTypeFilter().Append(L".icc");open.FileTypeFilter().Append(L".icm");picker=open.PickSingleFileAsync();
                auto selected=co_await picker;if(selected)*scripted=O({{L"op",S(L"profile_import")},{L"path",S(selected.Path())}});
            }
        }catch(hresult_canceled const&){}catch(hresult_error const& e){if(!stopping)report(to_string(e.message()));}
        workflowChanged={};clearPreviews();dialog=nullptr;picker=nullptr;presentationChanged={};
        if(scripted->Size())action=*scripted;
        if(!stopping)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",action}}).Stringify()));
        showing=false;changed();
    }
    fire_and_forget recovering(J state){
        auto lifetime=shared_from_this();showing=true;changed();hstring action=L"keep_open";
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialogSurface(data,dialog);dialogLayout(data,dialog);dialog.Title(box_value(recovery(L"attention")));
            dialog.PrimaryButtonText(recovery(L"retry"));dialog.CloseButtonText(common(L"keep_open"));
            auto text=label(data,str(state,L"error"));text.MaxWidth(420);text.TextWrapping(TextWrapping::Wrap);dialog.Content(text);
            presentationChanged=[this,text]{dialog.Title(box_value(recovery(L"attention")));dialog.PrimaryButtonText(recovery(L"retry"));dialog.CloseButtonText(common(L"keep_open"));text.Text(str(object(model,L"windows_recovery"),L"error"));};
            if(co_await dialog.ShowAsync()==ContentDialogResult::Primary)action=L"retry";
        }catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        clearPreviews();dialog=nullptr;presentationChanged={};if(!stopping)send(to_string(O({{L"operation",S(L"recovery")},{L"action",O({{L"op",S(action)}})}}).Stringify()));showing=false;changed();
    }
    fire_and_forget recoverDrawing(double id){
        auto lifetime=shared_from_this();showing=true;changed();hstring action=L"later";
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialogSurface(data,dialog);dialogLayout(data,dialog);
            auto text=label(data,L"");text.MaxWidth(420);text.TextWrapping(TextWrapping::Wrap);dialog.Content(text);
            presentationChanged=[this,text]{dialog.Title(box_value(recovery(L"title")));dialog.PrimaryButtonText(recovery(L"retry"));dialog.SecondaryButtonText(recovery(L"discard"));dialog.CloseButtonText(recovery(L"later"));text.Text(recovery(L"explanation"));};
            presentationChanged();dialog.DefaultButton(ContentDialogButton::Close);
            auto result=co_await dialog.ShowAsync();
            if(result==ContentDialogResult::Primary)action=L"restore_drawing";else if(result==ContentDialogResult::Secondary)action=L"discard";
        }catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        clearPreviews();dialog=nullptr;presentationChanged={};if(!stopping)send(to_string(O({{L"operation",S(L"recovery")},{L"action",O({{L"op",S(action)},{L"id",N(id)}})}}).Stringify()));showing=false;changed();
    }
    hstring busyTitle(J const& state){auto title=str(state,L"title");return title.empty()?str(object(catalog,L"bootstrap"),L"preparing_document"):title;}
    fire_and_forget working(J state){
        auto lifetime=shared_from_this();showing=true;busyDialog=true;busyCompleted=false;changed();
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialogSurface(data,dialog);dialogLayout(data,dialog);dialog.Title(box_value(busyTitle(state)));dialog.CloseButtonText(common(L"cancel"));
            ProgressRing progress;progress.IsActive(true);progress.Width(48);progress.Height(48);dialog.Content(progress);co_await dialog.ShowAsync();
        }catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        clearPreviews();dialog=nullptr;presentationChanged={};
        if(!stopping&&!busyCompleted&&str(state,L"type")==L"package_busy")send(to_string(O({{L"operation",S(L"package")},{L"id",N(num(state,L"id"))},{L"action",S(L"close")},{L"path",JsonValue::CreateNullValue()}}).Stringify()));
        else if(!stopping&&!busyCompleted&&str(state,L"type")==L"opening_busy")send(to_string(O({{L"operation",S(L"cancel")},{L"id",N(num(state,L"id"))}}).Stringify()));
        else if(!stopping&&!busyCompleted)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(num(state,L"id"))},{L"action",O({{L"op",S(L"cancel")}})}}).Stringify()));
        busyDialog=false;showing=false;changed();
    }
    void apply(J const& snapshot,bool blocked) {
        data->model=snapshot;
        bool relocalize=data->adoptLocalization(snapshot);
        if(relocalize){catalog=data->catalog;localization=data->localization;}
        model=snapshot;data->state=object(snapshot,L"state");data->refreshPalette();
        if(dialog)dialogSurface(data,dialog);
        if(newDrawing)newDrawing->Apply();
        if(workflowChanged)workflowChanged();
        if(relocalize&&presentationChanged)presentationChanged();
        if(relocalize&&dialog){
            dialog.Language(data->language());
            if(busyDialog){dialog.Title(box_value(busyTitle(object(model,L"windows_document"))));dialog.CloseButtonText(common(L"cancel"));}
        }
        auto document=object(model,L"windows_document");
        if(!document.Size())workflowStamp=L"";
        if(busyDialog&&str(object(model,L"windows_document"),L"type")!=L"workflow_busy"&&str(object(model,L"windows_document"),L"type")!=L"opening_busy"&&str(object(model,L"windows_document"),L"type")!=L"package_busy"){busyCompleted=true;if(dialog)dialog.Hide();}
        if(stopping||blocked||showing||(newDrawing&&newDrawing->IsOpen()))return;
        auto recovery=object(model,L"windows_recovery");auto failure=str(recovery,L"error");
        if(failure.empty())recoveryStamp=L"";
        if(!flag(recovery,L"busy")&&!failure.empty()){
            auto stamp=failure+L"/"+(flag(recovery,L"closing")?L"close":L"open");if(stamp!=recoveryStamp){recoveryStamp=stamp;recovering(recovery);return;}
        }
        auto drawing=recovery.GetNamedValue(L"drawing",JsonValue::CreateNullValue());
        if(drawing.ValueType()!=JsonValueType::Number)drawingStamp=L"";
        else if(!flag(recovery,L"busy")&&failure.empty()){
            auto stamp=to_hstring(uint64_t(drawing.GetNumber()));if(stamp!=drawingStamp){drawingStamp=stamp;recoverDrawing(drawing.GetNumber());return;}
        }
        if(str(document,L"type")==L"workflow_busy"||str(document,L"type")==L"opening_busy"||str(document,L"type")==L"package_busy"){handled=uint32_t(num(document,L"id"));working(document);return;}
        if(str(document,L"type")==L"package"){
            auto stamp=to_hstring(uint32_t(num(document,L"id")))+L"/"+to_hstring(uint32_t(num(document,L"serial")));if(stamp!=packageStamp){packageStamp=stamp;package(document);}return;
        }
        if(str(document,L"type")==L"workflow"){
            auto stamp=to_hstring(uint32_t(num(document,L"id")))+L"/"+to_hstring(uint32_t(num(document,L"serial")));
            if(stamp!=workflowStamp){workflowStamp=stamp;workflow(document);}return;
        }
        if(str(document,L"type")==L"interpret"&&uint32_t(num(document,L"id"))!=interpreted){interpreted=uint32_t(num(document,L"id"));interpret(document);return;}
        for(auto value:array(object(model,L"state"),L"requests")) {
            auto envelope=value.GetObject();
            if(str(object(envelope,L"kind"),L"type")==L"soft_proof_setup"){
                auto id=uint32_t(num(envelope,L"id"));if(id!=handled){handled=id;send(to_string(O({{L"operation",S(L"workflow_begin")},{L"id",N(id)}}).Stringify()));}break;
            }
            if(str(object(envelope,L"kind"),L"type")==L"paper"&&str(object(object(envelope,L"kind"),L"request"),L"type")==L"import"){
                auto id=uint32_t(num(envelope,L"id"));if(id!=handled){handled=id;importPaper(envelope);}break;
            }
            if(str(object(envelope,L"kind"),L"type")!=L"document")continue;
            auto id=uint32_t(num(envelope,L"id"));
            if(id!=handled){handled=id;show(envelope);}
            break;
        }
    }
};
DocumentView::DocumentView(Dispatch send,Dispatch action,Json catalog,NativeLocalization localization,Window window,std::function<void()> changed,PreviewTransport query,Dispatch report)
    :impl(std::make_shared<Impl>()) {
    impl->data->catalog=catalog;impl->data->localization=localization;impl->localization=localization;impl->send=std::move(send);impl->catalog=catalog;impl->window=window;
    impl->query=std::move(query);impl->changed=std::move(changed);impl->report=std::move(report);
    impl->data->send=std::move(action);impl->data->query=impl->query;
}
DocumentView::~DocumentView()=default;
void DocumentView::Apply(Json const& snapshot,bool blocked){impl->apply(snapshot,blocked);}
bool DocumentView::IsOpen()const{return impl->showing||(impl->newDrawing&&impl->newDrawing->IsOpen());}
void DocumentView::Hide(){
    impl->stopping=true;impl->clearPreviews();
    if(impl->newDrawing)impl->newDrawing->Hide();
    if(impl->dialog)impl->dialog.Hide();
    if(impl->picker)impl->picker.Cancel();
    if(impl->multiplePicker)impl->multiplePicker.Cancel();
}
