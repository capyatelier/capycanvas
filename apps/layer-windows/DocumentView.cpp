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
    static constexpr wchar_t ClipNonce[]=L"art.capycanvas.clip.nonce";
    static int depthIndex(hstring const& value){return value==L"F32"?3:value==L"F16"?2:value==L"U16"?1:0;}
    static hstring depthValue(int index){return std::array<hstring,4>{L"U8",L"U16",L"F16",L"F32"}.at(index);}
    Dispatch send,report;
    PreviewTransport query;
    hstring workflowStamp,recoveryStamp,packageStamp;
    bool busyDialog=false,busyCompleted=false,recoveryProgress=false;
    std::function<void()> changed;
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    std::shared_ptr<CapyLocalization> localization;
    std::function<void()> presentationChanged;
    J catalog,model,proofDraft;
    std::function<void()> creationPresetsChanged;
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

    hstring recovery(wchar_t const* key) const {return str(object(object(catalog,L"bootstrap"),L"recovery"),key);}
    hstring common(wchar_t const* key) const {return str(object(object(catalog,L"bootstrap"),L"common"),key);}
    hstring native(wchar_t const* group,wchar_t const* key) const {return str(object(object(catalog,L"native_copy"),group),key);}
    hstring caption(J const& request) const {
        auto input=to_string(request.Stringify());std::unique_ptr<char,decltype(&capy_string_free)> raw(capy_native_caption(localization.get(),input.c_str()),capy_string_free);
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
    template<class Control> void copyHeader(Control const& control,LocalizedCopy const& copy){
        control.Header(box_value(hstring(copy)));AutomationProperties::SetName(control,copy);
        data->copyView([weak=make_weak(control),resolve=copy.current]{auto control=weak.get();if(!control)return false;auto value=resolve();control.Header(box_value(value));AutomationProperties::SetName(control,value);return true;});
    }
    template<class Control> void copyContent(Control const& control,LocalizedCopy const& copy){
        control.Content(box_value(hstring(copy)));AutomationProperties::SetName(control,copy);
        data->copyView([weak=make_weak(control),resolve=copy.current]{auto control=weak.get();if(!control)return false;auto value=resolve();control.Content(box_value(value));AutomationProperties::SetName(control,value);return true;});
    }
    fire_and_forget show(J envelope) {
        auto lifetime=shared_from_this();
        auto request=object(object(envelope,L"kind"),L"request");
        auto type=str(request,L"type");
        if(type==L"export"||type==L"place"||type==L"paste"||type==L"copy"||type==L"change_color"||type==L"color_history"||type==L"properties"||type==L"repair_source_profile"||type==L"rasterize_source"){
            send(to_string(O({{L"operation",S(L"workflow_begin")},{L"id",N(num(envelope,L"id"))}}).Stringify()));co_return;
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
                    auto creation=object(options,L"creation"),text=object(creation,L"text");
                    dialog.Title(box_value(str(text,L"new_title")));dialog.PrimaryButtonText(str(text,L"create"));
                    creationDraft=J::Parse(object(creation,L"options").Stringify());
                    auto labels=array(spec,L"labels"),defaults=array(creationDraft,L"extent");
                    ComboBox preset,space,depth,background,blending;
                    AutomationProperties::SetAutomationId(depth,L"document-depth");
                    preset.Header(box_value(str(text,L"preset")));space.Header(box_value(str(text,L"space")));
                    depth.Header(box_value(str(text,L"depth")));background.Header(box_value(str(text,L"background")));
                    auto presets=array(creation,L"presets"),spaces=array(creation,L"spaces"),depths=array(creation,L"depths"),backgrounds=array(creation,L"backgrounds");
                    for(auto item:presets)comboOption(preset,str(item.GetObject(),L"name"));
                    for(auto item:spaces)comboOption(space,item.GetArray().GetStringAt(1));
                    for(auto item:depths)comboOption(depth,item.GetArray().GetStringAt(1));
                    for(auto item:backgrounds)comboOption(background,item.GetArray().GetStringAt(1));
                    auto blendSpec=object(creation,L"blending");auto blendChoices=array(blendSpec,L"choices");
                    blending.Header(box_value(str(blendSpec,L"label")));AutomationProperties::SetAutomationId(blending,L"document-blending");
                    for(auto item:blendChoices)comboOption(blending,str(item.GetObject(),L"label"));
                    for(auto control:{preset,space,depth,background,blending}){control.HorizontalAlignment(HorizontalAlignment::Stretch);body.Children().Append(control);}
                    std::array<TextBox,2> entries;
                    for(uint32_t i=0;i<2;++i){entries[i].Header(box_value(labels.GetStringAt(i)));entries[i].MaxLength(512);
                        AutomationProperties::SetName(entries[i],labels.GetStringAt(i));AutomationProperties::SetAutomationId(entries[i],i==0?L"document-width":L"document-height");body.Children().Append(entries[i]);}
                    TextBlock summary,blendHelp,note,error;for(auto item:{summary,blendHelp,note,error}){item.TextWrapping(TextWrapping::Wrap);body.Children().Append(item);}
                    error.Visibility(Visibility::Collapsed);AutomationProperties::SetAutomationId(error,L"document-error");
                    auto loading=std::make_shared<bool>(false);auto appearanceSource=std::make_shared<hstring>();auto chosen=std::make_shared<hstring>(str(creationDraft,L"blend_space"));
                    auto appearance=[creationDraft,summary,blendHelp,note,appearanceSource,blending,blendChoices,loading,this]{
                        try{
                            auto source=creationDraft.Stringify()+L"/"+to_hstring(data->localizationGeneration);if(source==*appearanceSource)return;*appearanceSource=source;
                            auto raw=to_string(creationDraft.Stringify());std::unique_ptr<char,decltype(&capy_string_free)> reply(capy_document_appearance(this->localization.get(),raw.c_str()),capy_string_free);if(!reply)throw hresult_error(E_OUTOFMEMORY);
                            auto view=J::Parse(to_hstring(reply.get()));summary.Text(str(view,L"summary"));blendHelp.Text(str(view,L"blending_help"));note.Text(str(view,L"note"));
                            *loading=true;for(uint32_t i=0;i<blendChoices.Size();++i)if(str(blendChoices.GetObjectAt(i),L"id")==str(view,L"blending"))blending.SelectedIndex(i);
                            blending.IsEnabled(flag(view,L"blending_editable"));*loading=false;
                        }catch(hresult_error const&){*loading=false;}
                    };
                    auto project=[creationDraft,entries,space,spaces,depth,depths,background,backgrounds,chosen,loading,spec,defaults,appearance,this]{
                        if(*loading||space.SelectedIndex()<0||depth.SelectedIndex()<0||background.SelectedIndex()<0)return;
                        try{
                            A dimensions;for(uint32_t i=0;i<2;++i){auto result=numeric(this->localization.get(),object(spec,L"numeric"),defaults.GetNumberAt(i),O({{L"type",S(L"expression")},{L"text",S(entries[i].Text())}}));dimensions.Append(N(num(result,L"value")));}
                            creationDraft.Insert(L"extent",dimensions);
                            creationDraft.Insert(L"color",O({{L"space",spaces.GetArrayAt(space.SelectedIndex()).GetAt(0)},{L"depth",depths.GetArrayAt(depth.SelectedIndex()).GetAt(0)}}));
                            creationDraft.Insert(L"background",backgrounds.GetArrayAt(background.SelectedIndex()).GetAt(0));
                            if(!chosen->empty())creationDraft.Insert(L"blend_space",S(*chosen));
                            appearance();
                        }catch(hresult_error const&){*loading=false;}
                    };
                    auto load=[creationDraft,entries,space,depth,background,blending,spaces,depths,backgrounds,blendChoices,chosen,loading,project](J value){
                        *loading=true;*chosen=str(value,L"blend_space");creationDraft.Insert(L"blend_space",value.GetNamedValue(L"blend_space"));auto color=object(value,L"color");
                        auto dimensions=array(value,L"extent");for(uint32_t i=0;i<2;++i)entries[i].Text(to_hstring(uint32_t(dimensions.GetNumberAt(i))));
                        for(uint32_t i=0;i<spaces.Size();++i)if(spaces.GetArrayAt(i).GetStringAt(0)==str(color,L"space"))space.SelectedIndex(i);
                        for(uint32_t i=0;i<depths.Size();++i)if(depths.GetArrayAt(i).GetStringAt(0)==str(color,L"depth"))depth.SelectedIndex(i);
                        for(uint32_t i=0;i<backgrounds.Size();++i)if(backgrounds.GetArrayAt(i).GetStringAt(0)==str(value,L"background"))background.SelectedIndex(i);
                        for(uint32_t i=0;i<blendChoices.Size();++i)if(str(blendChoices.GetObjectAt(i),L"id")==str(value,L"blend_space"))blending.SelectedIndex(i);
                        *loading=false;project();
                    };
                    load(creationDraft);
                    for(auto control:{space,depth,background})control.SelectionChanged([project](auto&&,auto&&){project();});
                    blending.SelectionChanged([blending,blendChoices,chosen,loading,project](auto&&,auto&&){
                        if(!*loading&&blending.IsEnabled()&&blending.SelectedIndex()>=0)*chosen=str(blendChoices.GetObjectAt(blending.SelectedIndex()),L"id");project();
                    });
                    for(auto entry:entries)entry.TextChanged([project](auto&&,auto&&){project();});
                    preset.SelectionChanged([preset,presets,load,loading](auto&&,auto&&){if(!*loading&&preset.SelectedIndex()>=0)load(object(presets.GetObjectAt(preset.SelectedIndex()),L"options"));});
                    auto selected=creation.GetNamedValue(L"selected",JsonValue::CreateNullValue());for(uint32_t i=0;i<presets.Size();++i)if(presets.GetObjectAt(i).GetNamedValue(L"id").Stringify()==selected.Stringify())preset.SelectedIndex(i);
                    Button remove;remove.Content(box_value(str(text,L"remove_preset")));body.Children().Append(remove);
                    auto canRemove=[preset,presets,remove]{auto i=preset.SelectedIndex();remove.IsEnabled(i>=0&&presets.GetObjectAt(i).GetNamedValue(L"remove",JsonValue::CreateNullValue()).ValueType()==JsonValueType::Object);};canRemove();
                    preset.SelectionChanged([canRemove](auto&&,auto&&){canRemove();});
                    auto presetSource=std::make_shared<hstring>(presets.Stringify());
                    creationPresetsChanged=[this,presets,preset,remove,presetSource,loading]{
                        auto next=array(object(object(model,L"document_options"),L"creation"),L"presets");auto source=next.Stringify();if(source==*presetSource)return;*presetSource=source;
                        bool same=next.Size()==presets.Size();for(uint32_t i=0;same&&i<next.Size();++i)same=object(next.GetObjectAt(i),L"options").Stringify()==object(presets.GetObjectAt(i),L"options").Stringify();
                        if(same){*loading=true;auto selected=preset.SelectedIndex();for(uint32_t i=0;i<next.Size();++i){presets.SetAt(i,next.GetAt(i));comboOptionText(preset,i,str(next.GetObjectAt(i),L"name"));}preset.SelectedIndex(selected);*loading=false;return;}
                        preset.SelectedIndex(-1);presets.Clear();preset.Items().Clear();for(auto item:next){presets.Append(item);comboOption(preset,str(item.GetObject(),L"name"));}remove.IsEnabled(false);
                    };
                    remove.Click([this,preset,presets,remove,id](auto&&,auto&&){auto index=preset.SelectedIndex();if(index<0)return;auto action=presets.GetObjectAt(index).GetNamedValue(L"remove",JsonValue::CreateNullValue());if(action.ValueType()!=JsonValueType::Object)return;
                        send(to_string(O({{L"operation",S(L"new_preferences")},{L"id",N(id)},{L"action",action}}).Stringify()));preset.SelectedIndex(-1);remove.IsEnabled(false);
                    });
                    presentationChanged=[this,preset,space,depth,background,blending,presetName,remember,entries,remove,appearance,loading]{
                        auto creation=object(object(model,L"document_options"),L"creation"),text=object(creation,L"text");
                        dialog.Title(box_value(str(text,L"new_title")));dialog.PrimaryButtonText(str(text,L"create"));dialog.CloseButtonText(common(L"cancel"));
                        preset.Header(box_value(str(text,L"preset")));space.Header(box_value(str(text,L"space")));depth.Header(box_value(str(text,L"depth")));
                        background.Header(box_value(str(text,L"background")));presetName.Header(box_value(str(text,L"preset_name")));remember.Content(box_value(str(text,L"remember")));
                        remove.Content(box_value(str(text,L"remove_preset")));
                        *loading=true;
                        for(auto const& [control,key]:std::initializer_list<std::pair<ComboBox,wchar_t const*>>{{space,L"spaces"},{depth,L"depths"},{background,L"backgrounds"}}){
                            auto selected=control.SelectedIndex();auto choices=array(creation,key);for(uint32_t i=0;i<std::min(choices.Size(),control.Items().Size());++i)comboOptionText(control,i,choices.GetArrayAt(i).GetStringAt(1));control.SelectedIndex(selected);
                        }
                        auto blend=object(creation,L"blending");blending.Header(box_value(str(blend,L"label")));auto selected=blending.SelectedIndex();auto choices=array(blend,L"choices");for(uint32_t i=0;i<std::min(choices.Size(),blending.Items().Size());++i)comboOptionText(blending,i,str(choices.GetObjectAt(i),L"label"));blending.SelectedIndex(selected);*loading=false;
                        auto labels=array(object(catalog,L"new_document"),L"labels");
                        for(uint32_t i=0;i<entries.size();++i){entries[i].Language(data->language());if(i<labels.Size())entries[i].Header(box_value(labels.GetStringAt(i)));}
                        appearance();
                    };
                    presetName.Header(box_value(str(text,L"preset_name")));presetName.MaxLength(64);body.Children().Append(presetName);
                    remember.Content(box_value(str(text,L"remember")));body.Children().Append(remember);
                    dialog.PrimaryButtonClick([entries,defaults,spec,extent,error,creationDraft,project,this](auto&&,ContentDialogButtonClickEventArgs const& e){
                        try{for(uint32_t i=0;i<2;++i){auto result=numeric(this->localization.get(),object(spec,L"numeric"),defaults.GetNumberAt(i),O({{L"type",S(L"expression")},{L"text",S(entries[i].Text())}}));(*extent)[i]=uint32_t(num(result,L"value"));}project();}
                        catch(hresult_error const& failure){e.Cancel(true);error.Text(failure.message());error.Visibility(Visibility::Visible);}
                    });
                } else {
                    presentationChanged=[this,id]{auto options=object(model,L"document_options");auto current=object(object(findId(array(object(model,L"state"),L"requests"),id),L"kind"),L"request");dialog.Title(box_value(str(current,L"title")));dialog.PrimaryButtonText(str(options,L"save_label"));dialog.SecondaryButtonText(str(options,L"discard_label"));dialog.CloseButtonText(str(options,L"cancel_label"));};
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
        creationPresetsChanged={};picker=nullptr;dialog=nullptr;presentationChanged={};
        if(!stopping)send(to_string(response.Stringify()));
        if(!stopping&&queued.Size())send(to_string(O({{L"operation",S(L"open_paths")},{L"paths",queued}}).Stringify()));
        showing=false;
        changed();
    }
    fire_and_forget interpret(J request){
        auto lifetime=shared_from_this();showing=true;changed();auto id=num(request,L"id");
        J profile;bool accepted=false;
        try {
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.Title(box_value(str(object(request,L"copy"),L"interpret_title")));
            dialog.PrimaryButtonText(recovery(L"open"));dialog.CloseButtonText(common(L"cancel"));
            StackPanel body;body.Spacing(12);auto info=label(data,interpretationText(L"interpret_help"));info.TextWrapping(TextWrapping::Wrap);body.Children().Append(info);
            ComboBox space;AutomationProperties::SetAutomationId(space,L"document-interpret-profile");copyHeader(space,interpretationText(L"interpret_as"));auto spaces=array(request,L"spaces");
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
        dialog=nullptr;presentationChanged={};if(!stopping)send(to_string(O({{L"operation",S(L"interpret")},{L"id",N(id)},
            {L"profile",accepted?V(profile):JsonValue::CreateNullValue()}}).Stringify()));
        creationPresetsChanged={};showing=false;changed();
    }
    fire_and_forget copied(J request){
        auto lifetime=shared_from_this();auto id=num(request,L"id");auto details=object(request,L"details");
        J action=O({{L"op",S(L"cancel")}});
        try{
            using namespace winrt::Windows::ApplicationModel::DataTransfer;
            auto file=co_await winrt::Windows::Storage::StorageFile::GetFileFromPathAsync(str(details,L"file"));
            auto bytes=co_await winrt::Windows::Storage::FileIO::ReadBufferAsync(file);
            winrt::Windows::Storage::Streams::InMemoryRandomAccessStream png;co_await png.WriteAsync(bytes);png.Seek(0);
            DataPackage package;package.RequestedOperation(DataPackageOperation::Copy);
            package.SetData(L"PNG",png);package.SetData(ClipNonce,box_value(str(details,L"nonce")));
            Clipboard::SetContent(package);action=O({{L"op",S(L"commit")}});
        }catch(hresult_error const&){if(!stopping)report(to_string(str(object(details,L"delivery"),L"clipboard_unavailable")));}
        if(!stopping)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",action}}).Stringify()));
    }
    fire_and_forget pickImages(J request){
        auto lifetime=shared_from_this();showing=true;changed();A paths;J own;auto id=num(request,L"id");
        try{
            auto details=object(request,L"details");
            if(str(request,L"kind")==L"paste"){
                using namespace winrt::Windows::ApplicationModel::DataTransfer;
                auto content=Clipboard::GetContent();
                winrt::Windows::Storage::Streams::IRandomAccessStream input{nullptr};
                auto nonce=str(details,L"clip_nonce");
                if(!nonce.empty()&&content.Contains(ClipNonce)&&unbox_value_or<hstring>(co_await content.GetDataAsync(ClipNonce),L"")==nonce){
                    own=O({{L"op",S(L"paste_clip")},{L"nonce",S(nonce)}});
                }else if(content.Contains(StandardDataFormats::StorageItems())){
                    auto items=co_await content.GetStorageItemsAsync();for(auto item:items)if(auto file=item.try_as<winrt::Windows::Storage::StorageFile>())paths.Append(S(file.Path()));
                }else{
                    if(content.Contains(L"PNG"))input=(co_await content.GetDataAsync(L"PNG")).try_as<winrt::Windows::Storage::Streams::IRandomAccessStream>();
                    if(!input&&content.Contains(StandardDataFormats::Bitmap()))input=co_await (co_await content.GetBitmapAsync()).OpenReadAsync();
                    if(!input)report(to_string(str(object(details,L"delivery"),L"clipboard_empty")));
                }
                if(input){
                    auto target=object(details,L"clipboard");auto folder=co_await winrt::Windows::Storage::StorageFolder::GetFolderFromPathAsync(str(target,L"folder"));
                    auto file=co_await folder.CreateFileAsync(str(target,L"name"),winrt::Windows::Storage::CreationCollisionOption::ReplaceExisting);
                    auto output=co_await file.OpenAsync(winrt::Windows::Storage::FileAccessMode::ReadWrite);
                    co_await winrt::Windows::Storage::Streams::RandomAccessStream::CopyAsync(input,output);co_await output.FlushAsync();output.Close();input.Close();paths.Append(S(file.Path()));
                }
            }else{
                Pickers::FileOpenPicker open(window.AppWindow().Id());open.CommitButtonText(recovery(L"import"));
                for(auto extension:array(details,L"extensions"))open.FileTypeFilter().Append(L"."+extension.GetString());
                multiplePicker=open.PickMultipleFilesAsync();auto selected=co_await multiplePicker;for(auto file:selected)paths.Append(S(file.Path()));
            }
        }catch(hresult_canceled const&){}catch(hresult_error const& e){if(!stopping)report(to_string(e.message()));}
        multiplePicker=nullptr;
        if(!stopping)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},
            {L"action",own.Size()?own:paths.Size()?O({{L"op",S(L"read_images")},{L"paths",paths}}):O({{L"op",S(L"cancel")}})}}).Stringify()));
        showing=false;changed();
    }
    void preview(Image const& image,uint32_t id,uint32_t index){
        auto queue=window.DispatcherQueue();
        query(CanvasQueryKind::Document,to_string(O({{L"id",N(id)},{L"index",N(index)}}).Stringify()),[queue,image,id](PreviewPacket packet){
            queue.TryEnqueue([packet,image,id]{if(!packet)return;
                try{auto meta=J::Parse(to_hstring(capy_preview_metadata(packet.get())));if(uint32_t(num(meta,L"id"))!=id)return;
                    uint32_t width=uint32_t(num(meta,L"width")),height=uint32_t(num(meta,L"height"));size_t length=0;auto bytes=capy_preview_bytes(packet.get(),&length);
                    if(!width||!height||width>1024||height>1024||length!=size_t(width)*height*4)return;
                    Imaging::WriteableBitmap bitmap(width,height);uint8_t* output=nullptr;check_hresult(bitmap.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&output));
                    for(size_t i=0;i<length;i+=4){auto a=bytes[i+3];output[i]=uint8_t((uint32_t(bytes[i+2])*a+127)/255);output[i+1]=uint8_t((uint32_t(bytes[i+1])*a+127)/255);output[i+2]=uint8_t((uint32_t(bytes[i])*a+127)/255);output[i+3]=a;}
                    bitmap.Invalidate();image.Source(bitmap);
                }catch(hresult_error const&){}
            });
        });
    }
    fire_and_forget package(J request){
        auto lifetime=shared_from_this();showing=true;changed();auto id=uint32_t(num(request,L"id"));auto summary=object(request,L"summary");hstring destination,action=L"close";
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.RequestedTheme(str(object(model,L"state"),L"theme")==L"dark"?ElementTheme::Dark:ElementTheme::Light);
            AutomationProperties::SetAutomationId(dialog,L"package-view");dialog.Title(box_value(str(summary,L"status")));dialog.PrimaryButtonText(str(summary,L"copy_original"));dialog.SecondaryButtonText(flag(object(summary,L"capabilities"),L"export")?str(summary,L"export_preview"):L"");dialog.CloseButtonText(str(summary,L"close"));
            StackPanel body;body.Spacing(12);body.MaxWidth(640);
            auto text=[body](hstring value){TextBlock label;label.Text(value);label.TextWrapping(TextWrapping::Wrap);body.Children().Append(label);};text(str(summary,L"reason"));
            for(auto value:array(summary,L"outputs"))text(str(value.GetObject(),L"name"));
            if(flag(object(summary,L"capabilities"),L"view")){Image image;image.MaxHeight(384);image.Stretch(Stretch::Uniform);body.Children().Append(image);preview(image,id,0);}
            dialog.Content(body);presentationChanged=[this]{auto summary=object(object(model,L"windows_document"),L"summary");dialog.Title(box_value(str(summary,L"status")));dialog.PrimaryButtonText(str(summary,L"copy_original"));dialog.SecondaryButtonText(flag(object(summary,L"capabilities"),L"export")?str(summary,L"export_preview"):L"");dialog.CloseButtonText(str(summary,L"close"));};
            auto result=co_await dialog.ShowAsync();dialog=nullptr;presentationChanged={};
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
        dialog=nullptr;presentationChanged={};
        if(!stopping)send(to_string(O({{L"operation",S(L"package")},{L"id",N(id)},{L"action",S(action)},{L"path",action!=L"close"?S(destination):JsonValue::CreateNullValue()}}).Stringify()));
        showing=false;changed();
    }
    fire_and_forget workflow(J request){
        auto lifetime=shared_from_this();auto id=uint32_t(num(request,L"id"));
        if(str(request,L"kind")==L"copy"&&str(request,L"stage")==L"commit"){copied(request);co_return;}
        if(str(request,L"stage")==L"commit"){
            send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",O({{L"op",S(L"commit")}})}}).Stringify()));co_return;
        }
        showing=true;changed();
        auto kind=str(request,L"kind"),stage=str(request,L"stage");auto details=object(request,L"details");
        if((kind==L"place"||kind==L"paste")&&stage==L"options"){showing=false;pickImages(request);co_return;}
        J action=O({{L"op",S(L"cancel")}});
        auto scripted=std::make_shared<J>();std::shared_ptr<ExportFormView> exportForm;std::shared_ptr<ProofFormView> proofForm;ComboBox profileList;A profileChoices;std::function<void()> updateProfileVisibility;
        if(kind==L"proof"&&proofRequest!=id){proofRequest=id;proofDraft=J();proofProfileId=L"";proofManaging=false;}
        bool library=kind==L"profiles"||(kind==L"proof"&&proofManaging);
        auto feature=object(details,L"feature_copy"),colorCopy=object(feature,L"color"),exportCopy=object(feature,L"export"),profileCopy=object(feature,L"profile"),proofCopy=object(feature,L"proof");
        auto colorText=[&](wchar_t const* key){return str(colorCopy,key);};
        auto exportText=[&](wchar_t const* key){return str(exportCopy,key);};
        auto profileText=[&](wchar_t const* key){return str(profileCopy,key);};
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.CloseButtonText(common(L"cancel"));
            dialog.RequestedTheme(str(object(model,L"state"),L"theme")==L"dark"?ElementTheme::Dark:ElementTheme::Light);
            AutomationProperties::SetAutomationId(dialog,L"document-workflow");
            auto title=library?profileText(L"library_title"):kind==L"proof"?str(proofCopy,L"title"):kind==L"export"?exportText(L"title"):kind==L"assign"?colorText(L"assign_title"):kind==L"convert"?colorText(L"convert_title"):kind==L"depth"?colorText(L"depth_title"):
                kind==L"place"||kind==L"paste"?profileText(L"interpret_title"):kind==L"repair"?colorText(L"repair_title"):kind==L"rasterize"?colorText(L"rasterize_title"):kind==L"histogram"?native(L"color",L"histogram"):str(details,L"title");
            dialog.Title(box_value(title));
            StackPanel body;body.Spacing(10);body.Width(std::max(200.,std::min(540.,double(window.Content().XamlRoot().Size().Width)-120)));
            auto text=[&](auto const& value){auto item=label(data,value);item.TextWrapping(TextWrapping::Wrap);body.Children().Append(item);return item;};
            ComboBox space,depth,intent;CheckBox copy,dither;
            auto spaces=array(request,L"spaces");
            if(!str(request,L"error").empty()){auto error=text(documentText(L"error",false));AutomationProperties::SetAutomationId(error,L"document-workflow-error");}
            if(stage==L"error"&&kind!=L"proof"){
                dialog.CloseButtonText(common(L"close"));
            }else if(library){
                AutomationProperties::SetAutomationId(text(featureText(L"profile",L"library_help")),L"profile-library-help");
                AutomationProperties::SetAutomationId(profileList,L"document-profile-library");auto entries=array(details,L"profiles");copyHeader(profileList,featureText(L"profile",L"library_title"));profileList.HorizontalAlignment(HorizontalAlignment::Stretch);
                for(auto item:entries){auto entry=item.GetObject();comboOption(profileList,profileLabel(entry),L"document-profile-"+str(entry,L"id"));}
                Button visibility;visibility.HorizontalAlignment(HorizontalAlignment::Left);AutomationProperties::SetAutomationId(visibility,L"profile-visibility");
                updateProfileVisibility=[this,button=make_weak(visibility),list=make_weak(profileList)]{auto target=button.get();auto box=list.get();if(!target||!box)return;
                    auto entries=array(object(object(model,L"windows_document"),L"details"),L"profiles");auto index=box.SelectedIndex();
                    bool shown=index<0||flag(entries.GetObjectAt(uint32_t(index)),L"visible",true);auto copy=object(object(object(object(model,L"windows_document"),L"details"),L"feature_copy"),L"profile");
                    auto text=str(copy,shown?L"hide":L"show");target.Content(box_value(text));AutomationProperties::SetName(target,text);target.IsEnabled(index>=0);
                };
                profileList.SelectionChanged([updateProfileVisibility](auto&&,auto&&){updateProfileVisibility();});
                visibility.Click([this,scripted,entries,list=make_weak(profileList)](auto&&,auto&&){
                    auto box=list.get();auto index=box?box.SelectedIndex():-1;if(index<0)return;auto entry=entries.GetObjectAt(uint32_t(index));
                    *scripted=O({{L"op",S(L"profile_visibility")},{L"id",S(str(entry,L"id"))},{L"visible",B(!flag(entry,L"visible",true))}});dialog.Hide();
                });
                if(entries.Size())profileList.SelectedIndex(0);updateProfileVisibility();
                body.Children().Append(profileList);body.Children().Append(visibility);
                dialog.PrimaryButtonText(profileText(L"add_profile_dialog"));dialog.SecondaryButtonText(profileText(L"remove"));dialog.IsSecondaryButtonEnabled(entries.Size()!=0);dialog.CloseButtonText(common(L"done"));
            }else if(kind==L"proof"){
                proofForm=std::make_shared<ProofFormView>(localization);proofForm->root.Language(data->language());proofForm->init(details,proofDraft,proofProfileId);body.Children().Append(proofForm->root);
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
                exportForm=std::make_shared<ExportFormView>(localization);exportForm->root.Language(data->language());exportForm->init(details);
                presentationChanged=[this,exportForm]{
                    auto details=object(object(model,L"windows_document"),L"details"),copy=object(object(details,L"feature_copy"),L"export");
                    exportForm->relocalize(localization,details,data->language());
                    dialog.Title(box_value(str(copy,L"title")));dialog.PrimaryButtonText(str(copy,L"preview"));dialog.CloseButtonText(common(L"cancel"));
                };
                auto presetUpdating=std::make_shared<bool>(false);
                auto presets=object(details,L"presets");ComboBox preset;copyHeader(preset,featureText(L"export",L"preset"));preset.HorizontalAlignment(HorizontalAlignment::Stretch);
                for(auto name:array(presets,L"names"))comboOption(preset,name.GetString());auto selectedPreset=presets.GetNamedValue(L"index",JsonValue::CreateNullValue());preset.SelectedIndex(selectedPreset.ValueType()==JsonValueType::Number?int(selectedPreset.GetNumber()):0);body.Children().Append(preset);
                body.Children().Append(exportForm->root);TextBox name;AutomationProperties::SetAutomationId(name,L"export-preset-name");copyHeader(name,featureText(L"export",L"preset_name"));name.MaxLength(64);body.Children().Append(name);
                auto operate=[this,scripted,exportForm](J operation){
                    *scripted=O({{L"op",S(L"export_preset")},{L"action",operation},{L"profile_id",exportForm->profileId.empty()?JsonValue::CreateNullValue():S(exportForm->profileId)}});dialog.Hide();
                };
                preset.SelectionChanged([preset,operate,presetUpdating](auto&&,auto&&){if(!*presetUpdating&&preset.SelectedIndex()>=0)operate(O({{L"type",S(L"get")},{L"index",N(preset.SelectedIndex())}}));});
                StackPanel buttons;buttons.Orientation(Orientation::Horizontal);buttons.Spacing(6);
                auto add=[&](LocalizedCopy title,std::function<void()> invoke){auto button=CapyUi::button(data,title,[]{});button.Click([invoke,exportForm](auto&&,auto&&){try{invoke();}catch(hresult_error const& error){exportForm->validation.Text(error.message());}});buttons.Children().Append(button);};
                add(featureText(L"export",L"save_preset"),[name,operate,exportForm]{operate(O({{L"type",S(L"save")},{L"name",S(name.Text())},{L"recipe",exportForm->current()}}));});
                add(data->copyCommon(L"update"),[preset,operate,exportForm]{operate(O({{L"type",S(L"update")},{L"index",N(preset.SelectedIndex())},{L"recipe",exportForm->current()}}));});
                add(data->copyCommon(preset.SelectedIndex()<4?L"reset":L"remove"),[preset,operate]{operate(O({{L"type",S(preset.SelectedIndex()<4?L"reset":L"remove")},{L"index",N(preset.SelectedIndex())}}));});body.Children().Append(buttons);
                auto formChanged=presentationChanged;
                presentationChanged=[this,formChanged,preset,presetUpdating]{
                    formChanged();*presetUpdating=true;
                    auto selected=preset.SelectedIndex();auto names=array(object(object(object(model,L"windows_document"),L"details"),L"presets"),L"names");
                    for(uint32_t i=0;i<std::min(names.Size(),preset.Items().Size());++i)comboOptionText(preset,i,names.GetStringAt(i));
                    preset.SelectedIndex(selected);*presetUpdating=false;
                };
                dialog.PrimaryButtonText(exportText(L"preview"));
                dialog.PrimaryButtonClick([exportForm](auto&&,ContentDialogButtonClickEventArgs const& e){try{exportForm->current();}catch(hresult_error const& error){exportForm->validation.Text(error.message());e.Cancel(true);}});
            }else if(kind==L"properties"){
                presentationChanged=[this]{auto details=object(object(model,L"windows_document"),L"details");dialog.Title(box_value(str(details,L"title")));dialog.CloseButtonText(str(details,L"done"));};
                for(uint32_t i=0;i<array(details,L"rows").Size();++i)text(documentRow(L"rows",i));
                if(array(details,L"sources").Size())text(documentText(L"source_images"));
                for(uint32_t i=0;i<array(details,L"sources").Size();++i)text(documentRow(L"sources",i));dialog.CloseButtonText(str(details,L"done"));
            }else if(kind==L"histogram"){
                auto histogram=object(details,L"histogram");auto channels=array(histogram,L"channels");auto axis=object(details,L"axis");auto binRange=array(axis,L"bins");uint32_t first=binRange.Size()?uint32_t(binRange.GetNumberAt(0)):0,last=binRange.Size()?uint32_t(binRange.GetNumberAt(1)):256;
                auto stops=array(axis,L"stops");if(stops.Size())text(data->copyCaption(O({{L"type",S(L"inspection_range")},{L"start",N(stops.GetNumberAt(0))},{L"end",N(stops.GetNumberAt(1))}})));
                std::array<hstring,4> names{native(L"color",L"red"),native(L"color",L"green"),native(L"color",L"blue"),native(L"color",L"luminance")};
                std::array<winrt::Windows::UI::Color,4> colors{{{255,220,75,75},{255,75,185,100},{255,90,130,240},{255,160,160,160}}};
                text(data->copyCaption(O({{L"type",S(L"inspection_pixels")},{L"sampled",N(num(histogram,L"pixels"))},{L"transparent",N(num(histogram,L"transparent"))}})));
                for(uint32_t i=0;i<channels.Size()&&i<4;++i){auto channel=channels.GetObjectAt(i);text(data->copyCaption(L"color",std::array<wchar_t const*,4>{L"red",L"green",L"blue",L"luminance"}[i]));auto bins=array(channel,L"bins");double maximum=1;for(auto bin:bins)maximum=std::max(maximum,bin.GetNumber());
                    Canvas graph;graph.Width(256);graph.Height(80);graph.HorizontalAlignment(HorizontalAlignment::Left);AutomationProperties::SetName(graph,caption(O({{L"type",S(L"inspection_graph")},{L"channel",S(names[i])}})));
                    data->copyView([source=std::weak_ptr<WorkspaceData>(data),weak=make_weak(graph),i]{auto data=source.lock();auto graph=weak.get();if(!data||!graph)return false;
                        auto channel=data->caption(L"color",std::array<wchar_t const*,4>{L"red",L"green",L"blue",L"luminance"}[i]);AutomationProperties::SetName(graph,data->caption(O({{L"type",S(L"inspection_graph")},{L"channel",S(channel)}})));return true;});
                    for(uint32_t x=first;x<std::min(last,bins.Size());++x){Shapes::Rectangle bar;bar.Width(256./std::max(1u,last-first));auto height=80*bins.GetNumberAt(x)/maximum;bar.Height(height);bar.Fill(SolidColorBrush(colors[i]));Canvas::SetLeft(bar,256.*(x-first)/std::max(1u,last-first));Canvas::SetTop(bar,80-height);graph.Children().Append(bar);}body.Children().Append(graph);
                    text(data->copyCaption(O({{L"type",S(L"inspection_channel")},{L"below",N(num(channel,L"below"))},{L"above",N(num(channel,L"above"))},{L"black",N(num(channel,L"black"))},{L"white",N(num(channel,L"white"))}})));
                }
                if(details.GetNamedValue(L"sampled_time",JsonValue::CreateNullValue()).ValueType()==JsonValueType::Number)text(data->copyCaption(O({{L"type",S(L"inspection_sample")},{L"seconds",N(num(details,L"sampled_time"))}})));
                dialog.CloseButtonText(common(L"done"));
            }else if(stage==L"preview"){
                text(featureText(L"color",flag(details,L"copy")?L"clipped_comparison":L"compare_before_apply"));
                auto sdr=flag(details,L"has_sdr_preview");
                for(uint32_t i=0;i<(sdr?3u:2u);++i){text(i==2?featureText(L"export",L"sdr_base"):i?(sdr?featureText(L"export",L"preview_sdr"):featureText(L"color",L"after")):featureText(L"color",L"before"));Image image;image.MaxHeight(210);image.Stretch(Stretch::Uniform);body.Children().Append(image);preview(image,id,i);}
                text(data->copyCaption(O({{L"type",S(L"inspection_clipped")},{L"count",N(num(details,L"clipped_channels"))}})));
                if(flag(details,L"range_blocked"))text(featureText(L"export",L"outside_range"));
                if(flag(details,L"adds_layer"))text(featureText(L"color",L"adds_layer"));
                dialog.PrimaryButtonText(kind==L"export"?exportText(L"export"):flag(details,L"copy")?colorText(L"save_copy"):common(L"apply"));dialog.IsPrimaryButtonEnabled(!flag(details,L"range_blocked"));
            }else{
                if(kind==L"depth"){
                    copyHeader(depth,featureText(L"color",L"depth"));for(auto label:{colorText(L"depth_8"),colorText(L"depth_16"),colorText(L"depth_float16"),colorText(L"depth_float32")})comboOption(depth,label);depth.SelectedIndex(depthIndex(str(object(details,L"color"),L"depth")));body.Children().Append(depth);
                    copyContent(dither,featureText(L"color",L"dither_stochastic"));body.Children().Append(dither);
                }else if(kind!=L"rasterize"){
                    AutomationProperties::SetAutomationId(space,L"document-profile-choice");copyHeader(space,featureText(L"color",kind==L"repair"||kind==L"place"||kind==L"paste"?L"current_source":L"space"));
                    for(auto entry:spaces){comboOption(space,entry.GetArray().GetStringAt(1));profileChoices.Append(O({{L"Builtin",entry.GetArray().GetAt(0)}}));}
                    if(kind==L"repair"||kind==L"place"||kind==L"paste")for(auto item:array(details,L"profiles")){auto entry=item.GetObject();if(entry.HasKey(L"issue"))continue;comboOption(space,str(entry,L"name"),L"document-profile-choice-"+str(entry,L"id"));profileChoices.Append(O({{L"library",S(str(entry,L"id"))}}));}
                    space.SelectedIndex(0);
                    for(uint32_t i=0;i<spaces.Size();++i)if(spaces.GetArrayAt(i).GetStringAt(0)==str(object(details,L"color"),L"space"))space.SelectedIndex(i);body.Children().Append(space);
                }
                if(kind==L"convert"){
                    copyHeader(intent,featureText(L"export",L"intent"));for(auto name:{exportText(L"relative"),exportText(L"perceptual"),exportText(L"saturation"),exportText(L"absolute")})comboOption(intent,name);intent.SelectedIndex(0);body.Children().Append(intent);
                    copyContent(copy,featureText(L"color",L"flattened_copy"));body.Children().Append(copy);
                }
                if(kind==L"assign")text(featureText(L"color",L"assign_native_help"));
                if(kind==L"repair"){auto current=text(featureText(L"color",L"current_source"));AutomationProperties::SetAutomationId(current,L"document-source-current-label");auto source=text(documentText(L"source_profile"));AutomationProperties::SetAutomationId(source,L"document-source-profile");}
                if(kind==L"rasterize")text(featureText(L"color",L"source_baked_help"));
                dialog.PrimaryButtonText(stage==L"interpret_image"?colorText(L"add_source"):colorText(L"preview"));
            }
            ScrollViewer scroll;scroll.Content(body);scroll.MaxHeight(std::max(180.,double(window.Content().XamlRoot().Size().Height)-220));dialog.Content(scroll);
            auto projected=presentationChanged;
            presentationChanged=[this,projected,kind,stage,library,space,depth,intent,copy,dither,profileList,profileChoices,updateProfileVisibility]{
                if(projected){projected();return;}
                auto current=object(model,L"windows_document"),details=object(current,L"details"),feature=object(details,L"feature_copy");
                auto color=[&](wchar_t const* key){return str(object(feature,L"color"),key);};
                auto output=[&](wchar_t const* key){return str(object(feature,L"export"),key);};
                auto profile=[&](wchar_t const* key){return str(object(feature,L"profile"),key);};
                auto title=library?profile(L"library_title"):kind==L"assign"?color(L"assign_title"):kind==L"convert"?color(L"convert_title"):kind==L"depth"?color(L"depth_title"):kind==L"place"||kind==L"paste"?profile(L"interpret_title"):kind==L"repair"?color(L"repair_title"):kind==L"rasterize"?color(L"rasterize_title"):kind==L"histogram"?native(L"color",L"histogram"):str(details,L"title");
                dialog.Title(box_value(title));dialog.CloseButtonText(common(stage==L"error"?L"close":library||kind==L"histogram"?L"done":L"cancel"));
                if(library){dialog.PrimaryButtonText(profile(L"add_profile_dialog"));dialog.SecondaryButtonText(profile(L"remove"));
                    auto selected=profileList.SelectedIndex();auto profiles=array(details,L"profiles");for(uint32_t i=0;i<std::min(profiles.Size(),profileList.Items().Size());++i){auto entry=profiles.GetObjectAt(i);comboOptionText(profileList,i,profileLabel(entry));}profileList.SelectedIndex(selected);if(updateProfileVisibility)updateProfileVisibility();
                }
                else if(stage==L"preview")dialog.PrimaryButtonText(kind==L"export"?output(L"export"):flag(details,L"copy")?color(L"save_copy"):common(L"apply"));
                else if(kind!=L"histogram"&&stage!=L"error")dialog.PrimaryButtonText(stage==L"interpret_image"?color(L"add_source"):color(L"preview"));
                for(auto control:{space,depth,intent,profileList})control.Language(data->language());
                for(uint32_t i=0;i<profileChoices.Size();++i){auto choice=profileChoices.GetObjectAt(i);auto id=str(choice,L"library");
                    if(!id.empty()){auto entry=find(array(details,L"profiles"),L"id",id);if(entry.Size())comboOptionText(space,i,str(entry,L"name"));}
                    else for(auto item:array(current,L"spaces")){auto entry=item.GetArray();if(entry.GetAt(0).Stringify()==choice.GetNamedValue(L"Builtin").Stringify())comboOptionText(space,i,entry.GetStringAt(1));}
                }
                if(kind==L"depth"){std::array<hstring,4> labels{color(L"depth_8"),color(L"depth_16"),color(L"depth_float16"),color(L"depth_float32")};for(uint32_t i=0;i<depth.Items().Size()&&i<labels.size();++i)comboOptionText(depth,i,labels[i]);}
                if(kind==L"convert"){std::array<hstring,4> labels{output(L"relative"),output(L"perceptual"),output(L"saturation"),output(L"absolute")};for(uint32_t i=0;i<intent.Items().Size()&&i<labels.size();++i)comboOptionText(intent,i,labels[i]);}
            };
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
                    Pickers::FileSavePicker save(window.AppWindow().Id());auto extension=L"."+str(details,L"extension");save.DefaultFileExtension(extension);save.SuggestedFileName(str(details,L"suggested_name"));
                    save.FileTypeChoices().Insert(str(details,L"format_name"),single_threaded_vector<hstring>({extension}));picker=save.PickSaveFileAsync();
                    auto selected=co_await picker;if(selected)action=O({{L"op",S(L"export_write")},{L"path",S(selected.Path())}});
                }else if(stage==L"preview"){
                    if(flag(details,L"copy")){
                        Pickers::FileSavePicker save(window.AppWindow().Id());save.DefaultFileExtension(L".capy");save.SuggestedFileName(str(details,L"suggested_name"));save.FileTypeChoices().Insert(str(object(catalog,L"delivery"),L"drawing_type"),single_threaded_vector<hstring>({L".capy"}));picker=save.PickSaveFileAsync();
                        auto selected=co_await picker;if(selected)action=O({{L"op",S(L"save_copy")},{L"path",S(selected.Path())}});
                    }else action=O({{L"op",S(L"commit")}});
                }else{
                    V choice=JsonValue::CreateNullValue();
                    if(kind==L"assign")choice=O({{L"Assign",spaces.GetArrayAt(space.SelectedIndex()).GetAt(0)}});
                    if(kind==L"convert")choice=O({{L"Convert",O({{L"space",spaces.GetArrayAt(space.SelectedIndex()).GetAt(0)},
                        {L"options",O({{L"intent",S(std::array<hstring,4>{L"RelativeColorimetric",L"Perceptual",L"Saturation",L"AbsoluteColorimetric"}[intent.SelectedIndex()])},{L"black_point_compensation",B(false)}})}})}});
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
        dialog=nullptr;picker=nullptr;presentationChanged={};
        if(scripted->Size())action=*scripted;
        if(!stopping)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(id)},{L"action",action}}).Stringify()));
        showing=false;changed();
    }
    fire_and_forget recovering(J state){
        auto lifetime=shared_from_this();showing=true;changed();auto closing=flag(state,L"closing");auto storageError=!str(state,L"error").empty()&&str(state,L"offer").empty();hstring action=closing?L"keep_open":L"later";
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.Title(box_value(closing||storageError?recovery(L"attention"):recovery(L"title")));
            dialog.PrimaryButtonText(closing||storageError?recovery(L"retry"):recovery(L"restore"));if(closing||!storageError)dialog.SecondaryButtonText(closing?common(L"keep_open"):recovery(L"discard"));dialog.CloseButtonText(closing?common(L"keep_open"):recovery(L"later"));
            TextBlock text;text.MaxWidth(420);text.TextWrapping(TextWrapping::Wrap);
            text.Text(str(state,L"error").empty()?recovery(L"explanation"):str(state,L"error"));dialog.Content(text);
            presentationChanged=[this,closing,storageError,text]{
                auto current=object(model,L"windows_recovery");dialog.Title(box_value(closing||storageError?recovery(L"attention"):recovery(L"title")));
                dialog.PrimaryButtonText(closing||storageError?recovery(L"retry"):recovery(L"restore"));if(closing||!storageError)dialog.SecondaryButtonText(closing?common(L"keep_open"):recovery(L"discard"));dialog.CloseButtonText(closing?common(L"keep_open"):recovery(L"later"));text.Text(str(current,L"error").empty()?recovery(L"explanation"):str(current,L"error"));
            };
            auto result=co_await dialog.ShowAsync();if(result==ContentDialogResult::Primary)action=closing||storageError?L"retry":L"restore";
            else if(result==ContentDialogResult::Secondary)action=closing?L"keep_open":L"discard";
        }catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        dialog=nullptr;presentationChanged={};if(!stopping)send(to_string(O({{L"operation",S(L"recovery")},{L"action",O({{L"op",S(action)}})}}).Stringify()));showing=false;changed();
    }
    fire_and_forget restoring(){
        auto lifetime=shared_from_this();showing=true;recoveryProgress=true;changed();
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.Title(box_value(recovery(L"restoring")));
            ProgressRing progress;progress.IsActive(true);progress.Width(48);progress.Height(48);dialog.Content(progress);co_await dialog.ShowAsync();
        }catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        dialog=nullptr;recoveryProgress=false;showing=false;changed();
    }
    hstring busyTitle(J const& state){auto title=str(state,L"title");return title.empty()?str(object(catalog,L"bootstrap"),L"preparing_document"):title;}
    fire_and_forget working(J state){
        auto lifetime=shared_from_this();showing=true;busyDialog=true;busyCompleted=false;changed();
        try{
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());dialog.Title(box_value(busyTitle(state)));dialog.CloseButtonText(common(L"cancel"));
            ProgressRing progress;progress.IsActive(true);progress.Width(48);progress.Height(48);dialog.Content(progress);co_await dialog.ShowAsync();
        }catch(hresult_error const& error){if(!stopping)report(to_string(error.message()));}
        dialog=nullptr;presentationChanged={};
        if(!stopping&&!busyCompleted&&str(state,L"type")==L"package_busy")send(to_string(O({{L"operation",S(L"package")},{L"id",N(num(state,L"id"))},{L"action",S(L"close")},{L"path",JsonValue::CreateNullValue()}}).Stringify()));
        else if(!stopping&&!busyCompleted&&str(state,L"type")==L"opening_busy")send(to_string(O({{L"operation",S(L"cancel")},{L"id",N(num(state,L"id"))}}).Stringify()));
        else if(!stopping&&!busyCompleted)send(to_string(O({{L"operation",S(L"workflow")},{L"id",N(num(state,L"id"))},{L"action",O({{L"op",S(L"cancel")}})}}).Stringify()));
        busyDialog=false;showing=false;changed();
    }
    void apply(J const& snapshot,bool blocked) {
        data->model=snapshot;
        bool relocalize=data->adoptLocalization(snapshot);
        if(relocalize){catalog=data->catalog;localization=data->localization;}
        model=snapshot;
        if(relocalize&&presentationChanged)presentationChanged();
        if(relocalize&&dialog){
            dialog.Language(data->language());
            if(busyDialog){dialog.Title(box_value(busyTitle(object(model,L"windows_document"))));dialog.CloseButtonText(common(L"cancel"));}
            if(recoveryProgress)dialog.Title(box_value(recovery(L"restoring")));
        }
        if(creationPresetsChanged)creationPresetsChanged();
        if(busyDialog&&str(object(model,L"windows_document"),L"type")!=L"workflow_busy"&&str(object(model,L"windows_document"),L"type")!=L"opening_busy"&&str(object(model,L"windows_document"),L"type")!=L"package_busy"){busyCompleted=true;if(dialog)dialog.Hide();}
        if(recoveryProgress&&!flag(object(model,L"windows_recovery"),L"restoring")){if(dialog)dialog.Hide();}
        if(stopping||blocked||showing)return;
        auto recovery=object(model,L"windows_recovery");auto offer=str(recovery,L"offer"),failure=str(recovery,L"error");
        if(flag(recovery,L"restoring")){restoring();return;}
        if(offer.empty()&&failure.empty())recoveryStamp=L"";
        if(!flag(recovery,L"busy")&&(!offer.empty()||!failure.empty())){
            auto stamp=offer+L"/"+failure+L"/"+(flag(recovery,L"closing")?L"close":L"open");if(stamp!=recoveryStamp){recoveryStamp=stamp;recovering(recovery);return;}
        }
        auto document=object(model,L"windows_document");
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
DocumentView::DocumentView(Dispatch send,Json catalog,std::shared_ptr<CapyLocalization> localization,Window window,std::function<void()> changed,PreviewTransport query,Dispatch report)
    :impl(std::make_shared<Impl>()) {
    impl->data->catalog=catalog;impl->data->localization=localization;impl->localization=localization;impl->send=std::move(send);impl->catalog=catalog;impl->window=window;
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
