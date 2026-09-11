#include "pch.h"
#include "DocumentView.h"
#include "UiControls.h"
#include <winrt/Microsoft.Windows.Storage.Pickers.h>
#include <microsoft.ui.xaml.window.h>
#include <array>

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace CapyUi;
namespace Pickers=winrt::Microsoft::Windows::Storage::Pickers;

struct DocumentView::Impl : std::enable_shared_from_this<Impl> {
    Dispatch send,report;
    std::function<void()> changed;
    J catalog,model;
    Window window{nullptr};
    ContentDialog dialog{nullptr};
    Windows::Foundation::IAsyncOperation<Pickers::PickFileResult> picker{nullptr};
    uint32_t handled=0;
    hstring handledImport;
    bool showing=false,stopping=false;

    fire_and_forget show(J envelope) {
        auto lifetime=shared_from_this();
        auto request=object(object(envelope,L"kind"),L"request");
        auto type=str(request,L"type");
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
                dialog.RequestedTheme(str(object(model,L"state"),L"theme")==L"dark"?ElementTheme::Dark:ElementTheme::Light);
                dialog.CloseButtonText(str(options,L"cancel_label"));
                dialog.DefaultButton(ContentDialogButton::Primary);
                auto size=window.Content().XamlRoot().Size();
                StackPanel body;body.Spacing(12);body.Width(std::max(180.,std::min(380.,double(size.Width)-96)));
                std::shared_ptr<std::array<uint32_t,2>> extent=std::make_shared<std::array<uint32_t,2>>();
                if(type==L"new") {
                    auto spec=object(catalog,L"new_document");
                    dialog.Title(box_value(str(spec,L"title")));
                    dialog.PrimaryButtonText(str(spec,L"accept"));
                    auto labels=array(spec,L"labels"),defaults=array(spec,L"extent");
                    std::array<TextBox,2> entries;
                    for(uint32_t i=0;i<2;++i) {
                        entries[i].Header(box_value(labels.GetStringAt(i)));
                        entries[i].Text(to_hstring(uint32_t(defaults.GetNumberAt(i))));entries[i].MaxLength(512);
                        AutomationProperties::SetName(entries[i],labels.GetStringAt(i));
                        AutomationProperties::SetAutomationId(entries[i],i==0?L"document-width":L"document-height");
                        body.Children().Append(entries[i]);
                    }
                    TextBlock error;error.TextWrapping(TextWrapping::Wrap);error.Visibility(Visibility::Collapsed);
                    AutomationProperties::SetAutomationId(error,L"document-error");body.Children().Append(error);
                    dialog.PrimaryButtonClick([entries,defaults,spec,extent,error](auto&&,ContentDialogButtonClickEventArgs const& e) {
                        try {
                            for(uint32_t i=0;i<2;++i) {
                                auto result=numeric(object(spec,L"numeric"),defaults.GetNumberAt(i),
                                    O({{L"type",S(L"expression")},{L"text",S(entries[i].Text())}}));
                                (*extent)[i]=uint32_t(num(result,L"value"));
                            }
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
                dialog.Content(body);
                auto choice=co_await dialog.ShowAsync();
                if(type==L"new"&&choice==ContentDialogResult::Primary)
                    response=O({{L"operation",S(L"new")},{L"id",N(id)},
                        {L"epoch",N(num(file,L"epoch"))},{L"revision",N(num(file,L"revision"))},
                        {L"width",N((*extent)[0])},{L"height",N((*extent)[1])}});
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
    void apply(J const& snapshot,bool blocked) {
        model=snapshot;
        if(stopping||blocked||showing)return;
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
            if(str(object(envelope,L"kind"),L"type")!=L"document")continue;
            auto id=uint32_t(num(envelope,L"id"));
            if(id!=handled){handled=id;show(envelope);}
            break;
        }
    }
};
DocumentView::DocumentView(Dispatch send,Json catalog,Window window,std::function<void()> changed,Dispatch report)
    :impl(std::make_shared<Impl>()) {
    impl->send=std::move(send);impl->catalog=catalog;impl->window=window;
    impl->changed=std::move(changed);impl->report=std::move(report);
}
DocumentView::~DocumentView()=default;
void DocumentView::Apply(Json const& snapshot,bool blocked){impl->apply(snapshot,blocked);}
bool DocumentView::IsOpen()const{return impl->showing;}
void DocumentView::Hide(){
    impl->stopping=true;
    if(impl->dialog)impl->dialog.Hide();
    if(impl->picker)impl->picker.Cancel();
}
