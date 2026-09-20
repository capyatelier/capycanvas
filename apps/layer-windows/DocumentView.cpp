#include "pch.h"
#include "DocumentView.h"
#include "UiControls.h"
#include "ExportForm.h"
#include "ProofForm.h"
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
    static int depthIndex(hstring const& value){return value==L"F32"?3:value==L"F16"?2:value==L"U16"?1:0;}
    static hstring depthValue(int index){return std::array<hstring,4>{L"U8",L"U16",L"F16",L"F32"}.at(index);}
    Dispatch send,report;
    PreviewTransport query;
    hstring workflowStamp,recoveryStamp;
    bool busyDialog=false,busyCompleted=false,recoveryProgress=false;
    std::function<void()> changed;
    J catalog,model,proofDraft;
    hstring proofProfileId;
    uint32_t proofRequest=0;
    bool proofManaging=false;
    Window window{nullptr};
    ContentDialog dialog{nullptr};
    winrt::Windows::Foundation::IAsyncOperation<Pickers::PickFileResult> picker{nullptr};
    winrt::Windows::Foundation::IAsyncOperation<winrt::Windows::Foundation::Collections::IVectorView<Pickers::PickFileResult>> multiplePicker{nullptr};
    uint32_t handled=0;
    hstring handledImport;
    uint32_t interpreted=0;
    bool showing=false,stopping=false;

    fire_and_forget show(J envelope) {
        auto lifetime=shared_from_this();
        auto request=object(object(envelope,L"kind"),L"request");
        auto type=str(request,L"type");
        if(type==L"export"||type==L"place"||type==L"paste"||type==L"change_color"||type==L"color_history"||type==L"properties"||type==L"repair_source_profile"||type==L"rasterize_source"){
            send(to_string(O({{L"operation",S(L"workflow_begin")},{L"id",N(num(envelope,L"id"))}}).Stringify()));co_return;
        }
        auto id=num(envelope,L"id");
        auto file=object(object(model,L"state"),L"document_file");
        // Approval applies to the document shown when this dialog opened.
        J response=O({{L"operation",S(L"cancel")},{L"id",N(id)}});
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
            if(type==L"new"||type==L"confirm_close") {
                dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());
                AutomationProperties::SetAutomationId(dialog,L"document-dialog");
                dialog.RequestedTheme(str(object(model,L"state"),L"theme")==L"dark"?ElementTheme::Dark:ElementTheme::Light);
                dialog.CloseButtonText(str(options,L"cancel_label"));
                dialog.DefaultButton(ContentDialogButton::Primary);
                auto size=window.Content().XamlRoot().Size();
                StackPanel body;body.Spacing(12);body.Width(std::max(180.,std::min(380.,double(size.Width)-96)));
                std::shared_ptr<std::array<uint32_t,2>> extent=std::make_shared<std::array<uint32_t,2>>();
                J creationDraft;TextBox presetName;CheckBox remember;
                if(type==L"new") {
                    auto spec=object(catalog,L"new_document");
                    dialog.Title(box_value(str(spec,L"title")));
                    dialog.PrimaryButtonText(str(spec,L"accept"));
                    auto creation=object(options,L"creation");creationDraft=J::Parse(object(creation,L"options").Stringify());
                    auto labels=array(spec,L"labels"),defaults=array(creationDraft,L"extent");
                    ComboBox preset,space,depth,background;
                    AutomationProperties::SetAutomationId(depth,L"document-depth");preset.Header(box_value(L"Preset"));space.Header(box_value(L"Working RGB"));depth.Header(box_value(L"Precision"));background.Header(box_value(L"Background"));
                    auto presets=array(creation,L"presets"),spaces=array(creation,L"spaces");
                    for(auto item:presets)preset.Items().Append(box_value(str(item.GetObject(),L"name")));
                    for(auto item:spaces)space.Items().Append(box_value(item.GetArray().GetStringAt(1)));
                    for(auto name:{L"8-bit SDR",L"16-bit SDR",L"16-bit float HDR",L"32-bit float HDR"})depth.Items().Append(box_value(name));
                    for(auto name:{L"White",L"Transparent"})background.Items().Append(box_value(name));
                    for(auto control:{preset,space,depth,background}){control.HorizontalAlignment(HorizontalAlignment::Stretch);body.Children().Append(control);}
                    std::array<TextBox,2> entries;
                    for(uint32_t i=0;i<2;++i) {
                        entries[i].Header(box_value(labels.GetStringAt(i)));
                        entries[i].Text(to_hstring(uint32_t(defaults.GetNumberAt(i))));entries[i].MaxLength(512);
                        AutomationProperties::SetName(entries[i],labels.GetStringAt(i));
                        AutomationProperties::SetAutomationId(entries[i],i==0?L"document-width":L"document-height");
                        body.Children().Append(entries[i]);
                    }
                    auto load=[creationDraft,entries,space,depth,background,spaces](J value){
                        creationDraft.Insert(L"color",object(value,L"color"));creationDraft.Insert(L"background",S(str(value,L"background")));
                        auto extent=array(value,L"extent");for(uint32_t i=0;i<2;++i)entries[i].Text(to_hstring(uint32_t(extent.GetNumberAt(i))));
                        for(uint32_t i=0;i<spaces.Size();++i)if(spaces.GetArrayAt(i).GetStringAt(0)==str(object(value,L"color"),L"space"))space.SelectedIndex(i);
                        depth.SelectedIndex(depthIndex(str(object(value,L"color"),L"depth")));background.SelectedIndex(str(value,L"background")==L"Transparent"?1:0);
                    };
                    load(creationDraft);
                    preset.SelectionChanged([preset,presets,load](auto&&,auto&&){if(preset.SelectedIndex()>=0)load(object(presets.GetObjectAt(preset.SelectedIndex()),L"options"));});
                    auto custom=array(object(object(object(model,L"state"),L"settings"),L"new_document"),L"presets").Size();
                    auto builtins=presets.Size()-custom;Button remove;remove.Content(box_value(L"Delete selected preset"));remove.IsEnabled(false);body.Children().Append(remove);
                    preset.SelectionChanged([preset,remove,builtins](auto&&,auto&&){remove.IsEnabled(preset.SelectedIndex()>=int(builtins));});
                    remove.Click([this,preset,presets,builtins,id](auto&&,auto&&){auto index=preset.SelectedIndex();if(index<int(builtins))return;
                        send(to_string(O({{L"operation",S(L"new_preferences")},{L"id",N(id)},{L"action",O({{L"type",S(L"remove")},{L"index",N(index-builtins)}})}}).Stringify()));
                        preset.SelectedIndex(-1);presets.RemoveAt(index);preset.Items().RemoveAt(index);
                    });
                    presetName.Header(box_value(L"Save as preset (optional)"));presetName.MaxLength(64);body.Children().Append(presetName);
                    remember.Content(box_value(L"Use these choices by default"));body.Children().Append(remember);
                    TextBlock error;error.TextWrapping(TextWrapping::Wrap);error.Visibility(Visibility::Collapsed);
                    AutomationProperties::SetAutomationId(error,L"document-error");body.Children().Append(error);
                    dialog.PrimaryButtonClick([entries,defaults,spec,extent,error,creationDraft,space,spaces,depth,background](auto&&,ContentDialogButtonClickEventArgs const& e) {
                        try {
                            for(uint32_t i=0;i<2;++i) {
                                auto result=numeric(object(spec,L"numeric"),defaults.GetNumberAt(i),
                                    O({{L"type",S(L"expression")},{L"text",S(entries[i].Text())}}));
                                (*extent)[i]=uint32_t(num(result,L"value"));
                            }
                            A dimensions;for(auto value:*extent)dimensions.Append(N(value));creationDraft.Insert(L"extent",dimensions);
                            creationDraft.Insert(L"color",O({{L"space",spaces.GetArrayAt(space.SelectedIndex()).GetAt(0)},{L"depth",S(depthValue(depth.SelectedIndex()))}}));
                            creationDraft.Insert(L"background",S(background.SelectedIndex()==1?L"Transparent":L"White"));
                        } catch(hresult_error const& failure) {
                            e.Cancel(true);error.Text(failure.message());error.Visibility(Visibility::Visible);
                        }
                    });
                } else {
                    dialog.Title(box_value(str(request,L"title")));
                    dialog.PrimaryButtonText(str(options,L"save_label"));
                    dialog.SecondaryButtonText(str(options,L"discard_label"));
                    TextBlock text;text.Text(str(options,L"unsaved_description"));text.TextWrapping(TextWrapping::Wrap);
                    body.Children().Append(text);
                }
                ScrollViewer scroll;scroll.MaxHeight(std::max(180.,double(size.Height)-220));scroll.Content(body);dialog.Content(scroll);
                auto choice=co_await dialog.ShowAsync();
                if(type==L"new"&&choice==ContentDialogResult::Primary)
                    response=O({{L"operation",S(L"create")},{L"id",N(id)},
                        {L"epoch",N(num(file,L"epoch"))},{L"revision",N(num(file,L"revision"))},
                        {L"options",creationDraft},{L"preset",S(presetName.Text())},{L"defaults",B(remember.IsChecked().Value())}});
                if(type==L"confirm_close")response.Insert(L"decision",S(choice==ContentDialogResult::Primary?L"save":
                    choice==ContentDialogResult::Secondary?L"discard":L"cancel"));
            } else if(type==L"save"||type==L"open"||type==L"export") {
                auto path=str(object(request,L"location"),L"uri");
                if(path.empty()) {
                    bool exporting=type==L"export";
                    auto extension=L"."+str(options,exporting?L"export_extension":L"extension");
                    if(type==L"open") {
                        Pickers::FileOpenPicker open(window.AppWindow().Id());
                        open.CommitButtonText(str(options,L"open_label"));
                        open.FileTypeFilter().Append(extension);
                        for(auto ext:{L".png",L".jpg",L".jpeg",L".jpe",L".tif",L".tiff",L".webp",L".bmp",L".dib",L".gif",L".exr",L".avif",L".heic",L".heif",L".hif"})open.FileTypeFilter().Append(ext);
                        picker=open.PickSingleFileAsync();
                    } else {
                        Pickers::FileSavePicker save(window.AppWindow().Id());
                        save.CommitButtonText(str(options,exporting?L"export_label":L"save_label"));
                        save.DefaultFileExtension(extension);save.SuggestedFileName(str(request,L"name"));
                        save.FileTypeChoices().Insert(str(options,exporting?L"export_filter_label":L"filter_label"),single_threaded_vector<hstring>({extension}));
                        picker=save.PickSaveFileAsync();
                    }
                    auto selected=co_await picker;
                    if(selected)path=selected.Path();
                }
                if(!path.empty()) {
                    response=O({{L"operation",S(type)},{L"id",N(id)},{L"path",S(path)}});
                    if(type==L"open") {
                        response.Insert(L"epoch",N(num(file,L"epoch")));
                        response.Insert(L"revision",N(num(file,L"revision")));
                    }
                }
            } else {
                response=O({{L"operation",S(L"failure")},{L"id",N(id)},
                    {L"error",S(L"This document operation is not available yet")}});
            }
        } catch(hresult_canceled const&) {
            // Picker cancellation is a normal response and never acknowledges a save.
        } catch(hresult_error const& failure) {
            if(!stopping) {
                auto message=L"Windows could not show the document dialog ("+to_hstring(failure.code().value)+L").";
                if(type==L"confirm_close")report(to_string(message));
                else response=O({{L"operation",S(L"failure")},{L"id",N(id)},{L"error",S(message)}});
            }
        } catch(std::exception const&) {
            if(!stopping) {
                if(type==L"confirm_close")report("Windows could not show the unsaved changes dialog.");
                else response=O({{L"operation",S(L"failure")},{L"id",N(id)},
                    {L"error",S(L"Windows could not show the document dialog.")}});
            }
        }
        picker=nullptr;dialog=nullptr;
        if(!stopping)send(to_string(response.Stringify()));
        showing=false;
        changed();
    }
    fire_and_forget showImport(J request) {
        auto lifetime=shared_from_this();
        auto response=O({{L"operation",S(L"import_image")},{L"id",S(str(request,L"id"))},
            {L"path",JsonValue::CreateNullValue()}});
        showing=true;changed();
        try {
            Pickers::FileOpenPicker open(window.AppWindow().Id());
            open.CommitButtonText(L"Import image");
            for(auto extension:{L".png",L".jpg",L".jpeg",L".bmp",L".gif",L".tif",L".tiff",L".jxr",L".webp",L".heic",L".heif"})
                open.FileTypeFilter().Append(extension);
            picker=open.PickSingleFileAsync();
            auto selected=co_await picker;
            if(selected)response.Insert(L"path",S(selected.Path()));
        } catch(hresult_canceled const&) {
        } catch(hresult_error const& failure) {
            if(!stopping)report("Windows could not show the image picker ("+std::to_string(failure.code().value)+").");
        } catch(std::exception const&) {
            if(!stopping)report("Windows could not show the image picker.");
        }
        picker=nullptr;
        if(!stopping)send(to_string(response.Stringify()));
        showing=false;changed();
    }
    fire_and_forget interpret(J request){
        auto lifetime=shared_from_this();showing=true;changed();auto id=num(request,L"id");
        J profile;bool accepted=false;
        try {
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.Title(box_value(L"Interpret untagged image"));
            dialog.PrimaryButtonText(L"Open image");dialog.CloseButtonText(L"Cancel");
            StackPanel body;body.Spacing(12);TextBlock info;info.Text(L"This image has no embedded color profile. Choose how its original values should be interpreted.");info.TextWrapping(TextWrapping::Wrap);body.Children().Append(info);
            ComboBox space;space.Header(box_value(L"Source RGB profile"));auto spaces=array(request,L"spaces");
            A choices;for(auto value:spaces){space.Items().Append(box_value(value.GetArray().GetStringAt(1)));choices.Append(O({{L"Builtin",value.GetArray().GetAt(0)}}));}
            for(auto item:array(request,L"profiles")){auto entry=item.GetObject();if(entry.HasKey(L"issue"))continue;space.Items().Append(box_value(str(entry,L"name")));choices.Append(O({{L"library",S(str(entry,L"id"))}}));}
            space.SelectedIndex(0);body.Children().Append(space);dialog.Content(body);
            accepted=co_await dialog.ShowAsync()==ContentDialogResult::Primary;
            if(accepted)profile=choices.GetObjectAt(space.SelectedIndex());
        }catch(hresult_error const& e){if(!stopping)report(to_string(e.message()));}
        dialog=nullptr;if(!stopping)send(to_string(O({{L"operation",S(L"interpret")},{L"id",N(id)},
            {L"profile",accepted?V(profile):JsonValue::CreateNullValue()}}).Stringify()));
        showing=false;changed();
    }
    fire_and_forget pickImages(J request){
        auto lifetime=shared_from_this();showing=true;changed();A paths;auto id=num(request,L"id");
        try{
            auto details=object(request,L"details");
            if(str(request,L"kind")==L"paste"){
                using namespace winrt::Windows::ApplicationModel::DataTransfer;
                auto content=Clipboard::GetContent();
                if(content.Contains(StandardDataFormats::StorageItems())){
                    auto items=co_await content.GetStorageItemsAsync();for(auto item:items)if(auto file=item.try_as<winrt::Windows::Storage::StorageFile>())paths.Append(S(file.Path()));
                }else if(content.Contains(StandardDataFormats::Bitmap())){
                    auto reference=co_await content.GetBitmapAsync();auto input=co_await reference.OpenReadAsync();
                    auto target=object(details,L"clipboard");auto folder=co_await winrt::Windows::Storage::StorageFolder::GetFolderFromPathAsync(str(target,L"folder"));
                    auto file=co_await folder.CreateFileAsync(str(target,L"name"),winrt::Windows::Storage::CreationCollisionOption::ReplaceExisting);
                    auto output=co_await file.OpenAsync(winrt::Windows::Storage::FileAccessMode::ReadWrite);
                    co_await winrt::Windows::Storage::Streams::RandomAccessStream::CopyAsync(input,output);co_await output.FlushAsync();output.Close();input.Close();paths.Append(S(file.Path()));
                }else report("The clipboard has no image or image files.");
            }else{
                Pickers::FileOpenPicker open(window.AppWindow().Id());open.CommitButtonText(L"Import images");
                for(auto extension:array(details,L"extensions"))open.FileTypeFilter().Append(L"."+extension.GetString());
                multiplePicker=open.PickMultipleFilesAsync();auto selected=co_await multiplePicker;for(auto file:selected)paths.Append(S(file.Path()));
            }
        }catch(hresult_canceled const&){}catch(hresult_error const& e){if(!stopping)report(to_string(e.message()));}
        multiplePicker=nullptr;
        if(!stopping)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},
            {L"action",paths.Size()?O({{L"op",S(L"read_images")},{L"paths",paths}}):O({{L"op",S(L"cancel")}})}}).Stringify()));
        showing=false;changed();
    }
    void preview(Image const& image,uint32_t id,uint32_t index){
        auto queue=window.DispatcherQueue();
        query(CanvasQueryKind::Document,to_string(O({{L"id",N(id)},{L"index",N(index)}}).Stringify()),[queue,image,id](PreviewPacket packet){
            queue.TryEnqueue([packet,image,id]{if(!packet)return;
                try{auto meta=J::Parse(to_hstring(capy_preview_metadata(packet.get())));if(uint32_t(num(meta,L"id"))!=id)return;
                    uint32_t width=uint32_t(num(meta,L"width")),height=uint32_t(num(meta,L"height"));size_t length=0;auto bytes=capy_preview_bytes(packet.get(),&length);
                    if(!width||!height||width>512||height>384||length!=size_t(width)*height*4)return;
                    Imaging::WriteableBitmap bitmap(width,height);uint8_t* output=nullptr;check_hresult(bitmap.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&output));
                    for(size_t i=0;i<length;i+=4){auto a=bytes[i+3];output[i]=uint8_t((uint32_t(bytes[i+2])*a+127)/255);output[i+1]=uint8_t((uint32_t(bytes[i+1])*a+127)/255);output[i+2]=uint8_t((uint32_t(bytes[i])*a+127)/255);output[i+3]=a;}
                    bitmap.Invalidate();image.Source(bitmap);
                }catch(hresult_error const&){}
            });
        });
    }
    fire_and_forget workflow(J request){
        auto lifetime=shared_from_this();auto id=uint32_t(num(request,L"id"));
        if(str(request,L"stage")==L"commit"){
            send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",O({{L"op",S(L"commit")}})}}).Stringify()));co_return;
        }
        showing=true;changed();
        auto kind=str(request,L"kind"),stage=str(request,L"stage");auto details=object(request,L"details");
        if((kind==L"place"||kind==L"paste")&&stage==L"options"){showing=false;pickImages(request);co_return;}
        J action=O({{L"op",S(L"cancel")}});
        auto scripted=std::make_shared<J>();std::shared_ptr<ExportFormView> exportForm;std::shared_ptr<ProofFormView> proofForm;ComboBox profileList;A profileChoices;
        if(kind==L"proof"&&proofRequest!=id){proofRequest=id;proofDraft=J();proofProfileId=L"";proofManaging=false;}
        bool library=kind==L"profiles"||(kind==L"proof"&&proofManaging);
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.CloseButtonText(L"Cancel");
            dialog.RequestedTheme(str(object(model,L"state"),L"theme")==L"dark"?ElementTheme::Dark:ElementTheme::Light);
            AutomationProperties::SetAutomationId(dialog,L"document-workflow");
            auto title=library?L"ICC profile library":kind==L"proof"?L"Proof Setup":kind==L"export"?L"Export image":kind==L"assign"?L"Assign working RGB":kind==L"convert"?L"Convert color space":kind==L"depth"?L"Change bit depth":
                kind==L"place"||kind==L"paste"?L"Interpret untagged image":kind==L"repair"?L"Repair source profile":kind==L"rasterize"?L"Rasterize source":kind==L"histogram"?L"Histogram":L"Document properties";
            dialog.Title(box_value(title));
            StackPanel body;body.Spacing(10);body.Width(std::max(200.,std::min(540.,double(window.Content().XamlRoot().Size().Width)-120)));
            auto text=[&](hstring value){TextBlock label;label.Text(value);label.TextWrapping(TextWrapping::Wrap);body.Children().Append(label);};
            ComboBox space,depth,intent;CheckBox copy,dither,blackPoint;
            auto spaces=array(request,L"spaces");
            if(!str(request,L"error").empty())text(str(request,L"error"));
            if(stage==L"error"&&kind!=L"proof"){
                dialog.CloseButtonText(L"Close");
            }else if(library){
                text(L"Imported profiles are kept in app storage. Removing a profile does not alter existing drawings or saved export recipes.");
                auto entries=array(details,L"profiles");profileList.Header(box_value(L"Profiles"));profileList.HorizontalAlignment(HorizontalAlignment::Stretch);
                for(auto item:entries){auto entry=item.GetObject();profileList.Items().Append(box_value(str(entry,L"name")+L" · "+str(entry,L"channels")+(entry.HasKey(L"issue")?L" · "+str(entry,L"issue"):L"")));}
                if(entries.Size())profileList.SelectedIndex(0);body.Children().Append(profileList);
                dialog.PrimaryButtonText(L"Import profile…");dialog.SecondaryButtonText(L"Remove selected");dialog.IsSecondaryButtonEnabled(entries.Size()!=0);dialog.CloseButtonText(L"Done");
            }else if(kind==L"proof"){
                proofForm=std::make_shared<ProofFormView>();proofForm->init(details,proofDraft,proofProfileId);body.Children().Append(proofForm->root);
                StackPanel buttons;buttons.Orientation(Orientation::Horizontal);buttons.Spacing(8);
                auto add=[&](hstring title,hstring op){
                    Button button;button.Content(box_value(title));AutomationProperties::SetName(button,title);
                    button.Click([this,proofForm,scripted,op](auto&&,auto&&){
                        proofDraft=proofForm->current();proofProfileId=proofForm->profileId;
                        *scripted=O({{L"op",S(op)}});dialog.Hide();
                    });buttons.Children().Append(button);
                };
                add(L"Add Profile…",L"proof_import");add(L"Manage Profiles…",L"proof_manage");body.Children().Append(buttons);
                dialog.PrimaryButtonText(L"Apply");
            }else if(kind==L"export"&&stage==L"options"){
                exportForm=std::make_shared<ExportFormView>();exportForm->init(details);
                auto presets=object(details,L"presets");ComboBox preset;preset.Header(box_value(L"Export preset"));preset.HorizontalAlignment(HorizontalAlignment::Stretch);
                for(auto name:array(presets,L"names"))preset.Items().Append(box_value(name.GetString()));auto selectedPreset=presets.GetNamedValue(L"index",JsonValue::CreateNullValue());preset.SelectedIndex(selectedPreset.ValueType()==JsonValueType::Number?int(selectedPreset.GetNumber()):0);body.Children().Append(preset);
                body.Children().Append(exportForm->root);TextBox name;name.Header(box_value(L"Preset name"));name.MaxLength(64);body.Children().Append(name);
                auto operate=[this,scripted,exportForm](J operation){
                    *scripted=O({{L"op",S(L"export_preset")},{L"action",operation},{L"profile_id",exportForm->profileId.empty()?JsonValue::CreateNullValue():S(exportForm->profileId)}});dialog.Hide();
                };
                preset.SelectionChanged([preset,operate](auto&&,auto&&){if(preset.SelectedIndex()>=0)operate(O({{L"type",S(L"get")},{L"index",N(preset.SelectedIndex())}}));});
                StackPanel buttons;buttons.Orientation(Orientation::Horizontal);buttons.Spacing(6);
                auto add=[&](hstring title,std::function<void()> invoke){Button button;button.Content(box_value(title));button.Click([invoke,exportForm](auto&&,auto&&){try{invoke();}catch(hresult_error const& error){exportForm->validation.Text(error.message());}});buttons.Children().Append(button);};
                add(L"Save preset",[name,operate,exportForm]{operate(O({{L"type",S(L"save")},{L"name",S(name.Text())},{L"recipe",exportForm->current()}}));});
                add(L"Update",[preset,operate,exportForm]{operate(O({{L"type",S(L"update")},{L"index",N(preset.SelectedIndex())},{L"recipe",exportForm->current()}}));});
                add(L"Remove / reset",[preset,operate]{operate(O({{L"type",S(preset.SelectedIndex()<4?L"reset":L"remove")},{L"index",N(preset.SelectedIndex())}}));});body.Children().Append(buttons);
                dialog.PrimaryButtonText(L"Preview export");
                dialog.PrimaryButtonClick([exportForm](auto&&,ContentDialogButtonClickEventArgs const& e){try{exportForm->current();}catch(hresult_error const& error){exportForm->validation.Text(error.message());e.Cancel(true);}});
            }else if(kind==L"properties"){
                for(auto item:array(request,L"details")){auto row=item.GetArray();text(row.GetStringAt(0)+L"\n"+row.GetStringAt(1));}dialog.CloseButtonText(L"Done");
            }else if(kind==L"histogram"){
                auto histogram=object(details,L"histogram");auto channels=array(histogram,L"channels");auto axis=object(details,L"axis");auto binRange=array(axis,L"bins");uint32_t first=binRange.Size()?uint32_t(binRange.GetNumberAt(0)):0,last=binRange.Size()?uint32_t(binRange.GetNumberAt(1)):256;
                auto stops=array(axis,L"stops");if(stops.Size())text(to_hstring(stops.GetNumberAt(0))+L" to "+to_hstring(stops.GetNumberAt(1))+L" EV · 0 EV = 203 cd/m² reference white");
                std::array<hstring,4> names{L"Red",L"Green",L"Blue",L"Luminance Y"};
                std::array<winrt::Windows::UI::Color,4> colors{{{255,220,75,75},{255,75,185,100},{255,90,130,240},{255,160,160,160}}};
                text(to_hstring(uint64_t(num(histogram,L"pixels")))+L" sampled pixels · "+to_hstring(uint64_t(num(histogram,L"transparent")))+L" transparent pixels excluded");
                for(uint32_t i=0;i<channels.Size()&&i<4;++i){auto channel=channels.GetObjectAt(i);text(names[i]);auto bins=array(channel,L"bins");double maximum=1;for(auto bin:bins)maximum=std::max(maximum,bin.GetNumber());
                    Canvas graph;graph.Width(256);graph.Height(80);graph.HorizontalAlignment(HorizontalAlignment::Left);AutomationProperties::SetName(graph,names[i]+L" histogram");
                    for(uint32_t x=first;x<std::min(last,bins.Size());++x){Shapes::Rectangle bar;bar.Width(256./std::max(1u,last-first));auto height=80*bins.GetNumberAt(x)/maximum;bar.Height(height);bar.Fill(SolidColorBrush(colors[i]));Canvas::SetLeft(bar,256.*(x-first)/std::max(1u,last-first));Canvas::SetTop(bar,80-height);graph.Children().Append(bar);}body.Children().Append(graph);
                    text(L"Below SDR: "+to_hstring(uint64_t(num(channel,L"below")))+L" · Above SDR: "+to_hstring(uint64_t(num(channel,L"above")))+
                        L" · Black: "+to_hstring(uint64_t(num(channel,L"black")))+L" · White: "+to_hstring(uint64_t(num(channel,L"white"))));
                }
                if(details.GetNamedValue(L"sampled_time",JsonValue::CreateNullValue()).ValueType()==JsonValueType::Number)text(L"Animated effects sampled at "+to_hstring(num(details,L"sampled_time"))+L" seconds.");
                dialog.CloseButtonText(L"Done");
            }else if(stage==L"preview"){
                text(flag(details,L"copy")?L"Review the flattened converted copy. The open drawing keeps its current color space.":L"Review the complete drawing before applying this change.");
                for(uint32_t i=0;i<2;++i){text(array(details,L"preview_labels").Size()==2?array(details,L"preview_labels").GetStringAt(i):i?L"After":L"Before");Image image;image.MaxHeight(210);image.Stretch(Stretch::Uniform);body.Children().Append(image);preview(image,id,i);}
                text(L"Clipped channels: "+to_hstring(uint64_t(num(details,L"clipped_channels"))));
                if(flag(details,L"adds_layer"))text(L"Existing raster edits are preserved; the corrected source will be added as a separate layer.");
                dialog.PrimaryButtonText(kind==L"export"?L"Export…":flag(details,L"copy")?L"Save copy…":L"Apply");
            }else{
                if(kind==L"depth"){
                    depth.Header(box_value(L"Precision"));for(auto label:{L"8-bit SDR",L"16-bit SDR",L"16-bit float HDR",L"32-bit float HDR"})depth.Items().Append(box_value(label));depth.SelectedIndex(depthIndex(str(object(details,L"color"),L"depth")));body.Children().Append(depth);
                    dither.Content(box_value(L"Dither when reducing to 8-bit"));body.Children().Append(dither);
                }else if(kind!=L"rasterize"){
                    space.Header(box_value(kind==L"repair"||kind==L"place"||kind==L"paste"?L"Source interpretation":L"Working RGB"));
                    for(auto entry:spaces){space.Items().Append(box_value(entry.GetArray().GetStringAt(1)));profileChoices.Append(O({{L"Builtin",entry.GetArray().GetAt(0)}}));}
                    if(kind==L"repair"||kind==L"place"||kind==L"paste")for(auto item:array(details,L"profiles")){auto entry=item.GetObject();if(entry.HasKey(L"issue"))continue;space.Items().Append(box_value(str(entry,L"name")));profileChoices.Append(O({{L"library",S(str(entry,L"id"))}}));}
                    space.SelectedIndex(0);
                    for(uint32_t i=0;i<spaces.Size();++i)if(spaces.GetArrayAt(i).GetStringAt(0)==str(object(details,L"color"),L"space"))space.SelectedIndex(i);body.Children().Append(space);
                }
                if(kind==L"convert"){
                    intent.Header(box_value(L"Rendering intent"));for(auto name:{L"Relative colorimetric",L"Perceptual",L"Saturation",L"Absolute colorimetric"})intent.Items().Append(box_value(name));intent.SelectedIndex(0);body.Children().Append(intent);
                    blackPoint.Content(box_value(L"Black point compensation"));body.Children().Append(blackPoint);
                    copy.Content(box_value(L"Save as a flattened converted copy"));body.Children().Append(copy);
                }
                if(kind==L"assign")text(L"Change the interpretation of existing values. Use Convert to preserve their color appearance.");
                if(kind==L"repair")text(L"Source profile: "+str(details,L"source_profile"));
                if(kind==L"rasterize")text(L"Bake the retained source into the document's working color space and precision.");
                dialog.PrimaryButtonText(stage==L"interpret_image"?L"Import image":L"Preview");
            }
            ScrollViewer scroll;scroll.Content(body);scroll.MaxHeight(std::max(180.,double(window.Content().XamlRoot().Size().Height)-220));dialog.Content(scroll);
            auto result=co_await dialog.ShowAsync();
            if(library&&result==ContentDialogResult::Secondary&&profileList.SelectedIndex()>=0)
                action=O({{L"op",S(L"profile_remove")},{L"id",S(str(array(details,L"profiles").GetObjectAt(profileList.SelectedIndex()),L"id"))}});
            if(result==ContentDialogResult::Primary){
                if(library){
                    action=O({{L"op",S(L"describe")}});
                    Pickers::FileOpenPicker open(window.AppWindow().Id());open.FileTypeFilter().Append(L".icc");open.FileTypeFilter().Append(L".icm");picker=open.PickSingleFileAsync();
                    auto selected=co_await picker;if(selected)action=O({{L"op",S(L"profile_import")},{L"path",S(selected.Path())}});
                }else if(kind==L"proof"){
                    proofDraft=proofForm->current();proofProfileId=proofForm->profileId;
                    action=O({{L"op",S(L"proof_options")},{L"settings",proofDraft},{L"profile_id",proofProfileId.empty()?JsonValue::CreateNullValue():S(proofProfileId)}});
                }else if(kind==L"export"&&stage==L"options"){
                    action=O({{L"op",S(L"export_options")},{L"recipe",exportForm->current()},{L"profile_id",exportForm->profileId.empty()?JsonValue::CreateNullValue():S(exportForm->profileId)}});
                }else if(kind==L"export"&&stage==L"preview"){
                    Pickers::FileSavePicker save(window.AppWindow().Id());auto extension=L"."+str(details,L"extension");save.DefaultFileExtension(extension);save.SuggestedFileName(L"Export");
                    save.FileTypeChoices().Insert(str(details,L"format_name"),single_threaded_vector<hstring>({extension}));picker=save.PickSaveFileAsync();
                    auto selected=co_await picker;if(selected)action=O({{L"op",S(L"export_write")},{L"path",S(selected.Path())}});
                }else if(stage==L"preview"){
                    if(flag(details,L"copy")){
                        Pickers::FileSavePicker save(window.AppWindow().Id());save.DefaultFileExtension(L".capy");save.SuggestedFileName(L"Converted copy");save.FileTypeChoices().Insert(L"Capy Canvas drawing",single_threaded_vector<hstring>({L".capy"}));picker=save.PickSaveFileAsync();
                        auto selected=co_await picker;if(selected)action=O({{L"op",S(L"save_copy")},{L"path",S(selected.Path())}});
                    }else action=O({{L"op",S(L"commit")}});
                }else{
                    V choice=JsonValue::CreateNullValue();
                    if(kind==L"assign")choice=O({{L"Assign",spaces.GetArrayAt(space.SelectedIndex()).GetAt(0)}});
                    if(kind==L"convert")choice=O({{L"Convert",O({{L"space",spaces.GetArrayAt(space.SelectedIndex()).GetAt(0)},
                        {L"options",O({{L"intent",S(std::array<hstring,4>{L"RelativeColorimetric",L"Perceptual",L"Saturation",L"AbsoluteColorimetric"}[intent.SelectedIndex()])},{L"black_point_compensation",B(blackPoint.IsChecked().Value())}})}})}});
                    if(kind==L"depth")choice=O({{L"Depth",O({{L"depth",S(depthValue(depth.SelectedIndex()))},{L"dither",S(depth.SelectedIndex()==0&&dither.IsChecked().Value()?L"Stochastic8":L"None")}})}});
                    if(kind==L"repair")choice=profileChoices.GetAt(space.SelectedIndex());
                    if(stage==L"interpret_image")action=O({{L"op",S(L"interpret_image")},{L"profile",profileChoices.GetAt(space.SelectedIndex())}});
                    else action=O({{L"op",S(L"prepare")},{L"choice",choice},{L"copy",B(kind==L"convert"&&copy.IsChecked().Value())}});
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
        dialog=nullptr;picker=nullptr;
        if(scripted->Size())action=*scripted;
        if(!stopping)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",action}}).Stringify()));
        showing=false;changed();
    }
    fire_and_forget recovering(J state){
        auto lifetime=shared_from_this();showing=true;changed();auto closing=flag(state,L"closing");auto storageError=!str(state,L"error").empty()&&str(state,L"offer").empty();hstring action=closing?L"keep_open":L"later";
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.Title(box_value(closing||storageError?L"Recovery storage needs attention":L"Recover drawing"));
            dialog.PrimaryButtonText(closing||storageError?L"Retry":L"Restore drawing");if(closing||!storageError)dialog.SecondaryButtonText(closing?L"Keep window open":L"Discard recovery copy");dialog.CloseButtonText(closing?L"Keep window open":L"Later");
            TextBlock text;text.MaxWidth(420);text.TextWrapping(TextWrapping::Wrap);
            text.Text(str(state,L"error").empty()?L"An unfinished drawing from a previous session is available. Restoring it keeps the recovery copy until the restored drawing has a new durable checkpoint.":str(state,L"error"));dialog.Content(text);
            auto result=co_await dialog.ShowAsync();if(result==ContentDialogResult::Primary)action=closing||storageError?L"retry":L"restore";
            else if(result==ContentDialogResult::Secondary)action=closing?L"keep_open":L"discard";
        }catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        dialog=nullptr;if(!stopping)send(to_string(O({{L"operation",S(L"recovery")},{L"action",O({{L"op",S(action)}})}}).Stringify()));showing=false;changed();
    }
    fire_and_forget restoring(){
        auto lifetime=shared_from_this();showing=true;recoveryProgress=true;changed();
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.Title(box_value(L"Restoring drawing"));
            ProgressRing progress;progress.IsActive(true);progress.Width(48);progress.Height(48);dialog.Content(progress);co_await dialog.ShowAsync();
        }catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        dialog=nullptr;recoveryProgress=false;showing=false;changed();
    }
    fire_and_forget working(J state){
        auto lifetime=shared_from_this();showing=true;busyDialog=true;busyCompleted=false;changed();
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.Title(box_value(L"Preparing document"));dialog.CloseButtonText(L"Cancel");
            ProgressRing progress;progress.IsActive(true);progress.Width(48);progress.Height(48);dialog.Content(progress);co_await dialog.ShowAsync();
        }catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        dialog=nullptr;
        if(!stopping&&!busyCompleted&&str(state,L"type")==L"opening_busy")send(to_string(O({{L"operation",S(L"cancel")},{L"id",N(num(state,L"id"))}}).Stringify()));
        else if(!stopping&&!busyCompleted)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(num(state,L"id"))},{L"action",O({{L"op",S(L"cancel")}})}}).Stringify()));
        busyDialog=false;showing=false;changed();
    }
    void apply(J const& snapshot,bool blocked) {
        model=snapshot;
        if(busyDialog&&str(object(model,L"windows_document"),L"type")!=L"workflow_busy"&&str(object(model,L"windows_document"),L"type")!=L"opening_busy"){busyCompleted=true;if(dialog)dialog.Hide();}
        if(recoveryProgress&&!flag(object(model,L"windows_recovery"),L"restoring")){if(dialog)dialog.Hide();}
        if(stopping||blocked||showing)return;
        auto recovery=object(model,L"windows_recovery");auto offer=str(recovery,L"offer"),failure=str(recovery,L"error");
        if(flag(recovery,L"restoring")){restoring();return;}
        if(offer.empty()&&failure.empty())recoveryStamp=L"";
        if(!flag(recovery,L"busy")&&(!offer.empty()||!failure.empty())){
            auto stamp=offer+L"/"+failure+L"/"+(flag(recovery,L"closing")?L"close":L"open");if(stamp!=recoveryStamp){recoveryStamp=stamp;recovering(recovery);return;}
        }
        auto document=object(model,L"windows_document");
        if(str(document,L"type")==L"workflow_busy"||str(document,L"type")==L"opening_busy"){handled=uint32_t(num(document,L"id"));working(document);return;}
        if(str(document,L"type")==L"workflow"){
            auto stamp=to_hstring(uint32_t(num(document,L"id")))+L"/"+to_hstring(uint32_t(num(document,L"serial")));
            if(stamp!=workflowStamp){workflowStamp=stamp;workflow(document);}return;
        }
        if(str(document,L"type")==L"interpret"&&uint32_t(num(document,L"id"))!=interpreted){interpreted=uint32_t(num(document,L"id"));interpret(document);return;}
        auto import=object(model,L"windows_image_import");
        if(flag(import,L"picking")){
            auto id=str(import,L"id");if(id!=handledImport){handledImport=id;showImport(import);}
            return;
        }
        // Superseding file operations cancel decoding on the owner; wait for that
        // worker slot to drain before presenting their dialog.
        if(flag(model,L"windows_importing"))return;
        for(auto value:array(object(model,L"state"),L"requests")) {
            auto envelope=value.GetObject();
            if(str(object(envelope,L"kind"),L"type")==L"histogram"||str(object(envelope,L"kind"),L"type")==L"soft_proof_setup"){
                auto id=uint32_t(num(envelope,L"id"));if(id!=handled){handled=id;send(to_string(O({{L"operation",S(L"workflow_begin")},{L"id",N(id)}}).Stringify()));}break;
            }
            if(str(object(envelope,L"kind"),L"type")!=L"document")continue;
            auto id=uint32_t(num(envelope,L"id"));
            if(id!=handled){handled=id;show(envelope);}
            break;
        }
    }
};
DocumentView::DocumentView(Dispatch send,Json catalog,Window window,std::function<void()> changed,PreviewTransport query,Dispatch report)
    :impl(std::make_shared<Impl>()) {
    impl->send=std::move(send);impl->catalog=catalog;impl->window=window;
    impl->query=std::move(query);impl->changed=std::move(changed);impl->report=std::move(report);
}
DocumentView::~DocumentView()=default;
void DocumentView::Apply(Json const& snapshot,bool blocked){impl->apply(snapshot,blocked);}
bool DocumentView::IsOpen()const{return impl->showing;}
void DocumentView::Hide(){
    impl->stopping=true;
    if(impl->dialog)impl->dialog.Hide();
    if(impl->picker)impl->picker.Cancel();
    if(impl->multiplePicker)impl->multiplePicker.Cancel();
}
