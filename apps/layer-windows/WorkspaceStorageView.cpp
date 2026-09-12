#include "pch.h"
#include "WorkspaceStorageView.h"
#include "UiControls.h"
#include <winrt/Microsoft.Windows.Storage.Pickers.h>
#include <microsoft.ui.xaml.window.h>

using namespace winrt;
using namespace CapyUi;
namespace Pickers=winrt::Microsoft::Windows::Storage::Pickers;

struct WorkspaceStorageView::Impl : std::enable_shared_from_this<Impl> {
    Dispatch send;
    std::function<void()> changed;
    Window window{nullptr};
    J model;
    Border root;
    TextBlock message;
    StackPanel buttons;
    Button retry,saveNew,backup;
    ContentDialog dialog{nullptr};
    Windows::Foundation::IAsyncOperation<Pickers::PickFileResult> picker{nullptr};
    bool showing=false,stopping=false;
    uint32_t handledClose=0;
    void dispatch(hstring operation) { send(to_string(O({{L"operation",S(operation)}}).Stringify())); }
    void failure() { send(to_string(O({{L"operation",S(L"failure")},{L"error",S(L"Windows could not show the workspace dialog. Try again.")}}).Stringify())); }
    void init() {
        root.HorizontalAlignment(HorizontalAlignment::Center);root.VerticalAlignment(VerticalAlignment::Bottom);
        root.Margin({16,16,16,64});root.Padding({12,12,12,12});root.CornerRadius({8,8,8,8});
        root.MaxWidth(640);root.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(root,L"workspace-storage-status");
        StackPanel content;content.Spacing(8);
        message.TextWrapping(TextWrapping::Wrap);content.Children().Append(message);
        buttons.Orientation(Orientation::Horizontal);buttons.Spacing(8);
        retry.Content(box_value(L"Retry"));saveNew.Content(box_value(L"Save as new workspace…"));backup.Content(box_value(L"Export backup…"));
        AutomationProperties::SetAutomationId(retry,L"workspace-storage-retry");
        AutomationProperties::SetAutomationId(saveNew,L"workspace-storage-save-new");
        AutomationProperties::SetAutomationId(backup,L"workspace-storage-backup");
        retry.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->dispatch(L"retry");});
        saveNew.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->show(false);});
        backup.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->exportBackup();});
        buttons.Children().Append(retry);buttons.Children().Append(saveNew);buttons.Children().Append(backup);
        content.Children().Append(buttons);root.Child(content);
    }
    void restore() {
        HWND handle=nullptr;check_hresult(window.as<IWindowNative>()->get_WindowHandle(&handle));
        if(IsIconic(handle))ShowWindow(handle,SW_RESTORE);
    }
    fire_and_forget show(bool closing) {
        auto lifetime=shared_from_this();
        if(showing||stopping)co_return;
        showing=true;changed();
        J response=O({{L"operation",S(L"keep_open")}});
        try {
            restore();
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());
            dialog.RequestedTheme(str(object(model,L"state"),L"theme")==L"dark"?ElementTheme::Dark:ElementTheme::Light);
            dialog.DefaultButton(ContentDialogButton::Close);
            StackPanel body;body.Spacing(12);body.MaxWidth(460);
            TextBlock explanation;explanation.TextWrapping(TextWrapping::Wrap);
            TextBox name;
            if(closing) {
                bool ready=flag(object(model,L"windows_workspace"),L"ready");
                dialog.Title(box_value(ready?L"Workspace changes could not be saved":L"Workspace could not be opened"));
                dialog.PrimaryButtonText(L"Retry");
                dialog.SecondaryButtonText(L"Close without saving");
                dialog.CloseButtonText(L"Keep open");
                hstring recovery=ready?
                    L"Keep this window open to save a new workspace or export a backup. Closing without saving discards unsaved layout and tool changes.":
                    L"Keep this window open to retry opening the workspace or export a database backup. Closing leaves the original workspace database unchanged.";
                explanation.Text(str(object(model,L"windows_workspace"),L"error")+L"\n\n"+recovery);
                body.Children().Append(explanation);
                AutomationProperties::SetAutomationId(dialog,L"workspace-close-error");
            } else {
                dialog.Title(box_value(L"Save as new workspace"));
                dialog.PrimaryButtonText(L"Save");dialog.CloseButtonText(L"Cancel");
                explanation.Text(L"Preserve the current layout and tool settings in a new workspace.");
                body.Children().Append(explanation);
                name.Header(box_value(L"Name"));name.Text(L"Recovered Workspace");name.MaxLength(100);
                AutomationProperties::SetAutomationId(name,L"workspace-recovery-name");body.Children().Append(name);
                AutomationProperties::SetAutomationId(dialog,L"workspace-save-new-dialog");
            }
            dialog.Content(body);
            auto result=co_await dialog.ShowAsync();
            if(closing) {
                if(result==ContentDialogResult::Primary)response=O({{L"operation",S(L"retry")}});
                else if(result==ContentDialogResult::Secondary)response=O({{L"operation",S(L"discard_close")}});
            } else {
                response=J{};
                if(result==ContentDialogResult::Primary)response=O({{L"operation",S(L"save_as_new")},{L"name",S(name.Text())}});
            }
        } catch(hresult_canceled const&) {
        } catch(...) { if(!stopping)failure(); }
        dialog=nullptr;
        if(!stopping&&response.Size())send(to_string(response.Stringify()));
        showing=false;changed();
    }
    fire_and_forget exportBackup() {
        auto lifetime=shared_from_this();
        if(showing||stopping)co_return;
        showing=true;changed();
        try {
            restore();
            bool workspace=flag(object(model,L"windows_workspace"),L"ready");
            hstring extension=workspace?L".capyworkspace":L".sqlite3";
            Pickers::FileSavePicker save(window.AppWindow().Id());
            save.CommitButtonText(L"Export backup");save.DefaultFileExtension(extension);
            save.SuggestedFileName(workspace?L"Workspace backup":L"Original workspace database");
            save.FileTypeChoices().Insert(workspace?L"Workspace Backup":L"SQLite database",single_threaded_vector<hstring>({extension}));
            picker=save.PickSaveFileAsync();
            auto selected=co_await picker;
            if(selected&&!stopping)send(to_string(O({{L"operation",S(workspace?L"export_backup":L"backup_database")},{L"path",S(selected.Path())}}).Stringify()));
        } catch(hresult_canceled const&) {
        } catch(...) {if(!stopping)failure();}
        picker=nullptr;showing=false;changed();
    }
    void apply(J const& snapshot,bool blocked) {
        model=snapshot;
        auto storage=object(model,L"windows_workspace");
        if(!storage.Size())return;
        auto error=str(storage,L"error"),notice=str(storage,L"notice");
        hstring text=error;
        if(!notice.empty())text=text.empty()?notice:text+L"\n"+notice;
        if(text.empty()&&!flag(storage,L"ready"))text=L"Opening workspace…";
        if(text.empty()&&flag(storage,L"close_requested"))text=L"Saving workspace…";
        message.Text(text);root.Visibility(text.empty()?Visibility::Collapsed:Visibility::Visible);
        auto palette=object(object(model,L"state"),L"palette");
        root.Background(fill(color(str(palette,L"panel",L"#242428"))));
        message.Foreground(fill(color(str(palette,L"text",L"#fafafb"))));
        buttons.Visibility(error.empty()?Visibility::Collapsed:Visibility::Visible);
        for(auto button:{retry,saveNew,backup})button.IsEnabled(!blocked&&!showing&&!flag(storage,L"busy"));
        saveNew.Visibility(flag(storage,L"ready")?Visibility::Visible:Visibility::Collapsed);
        auto attempt=uint32_t(num(storage,L"close_attempt"));
        if(!blocked&&!showing&&!stopping&&flag(storage,L"close_requested")&&!flag(storage,L"busy")&&!error.empty()&&attempt!=handledClose) {
            handledClose=attempt;show(true);
        }
    }
};
WorkspaceStorageView::WorkspaceStorageView(Dispatch send,Window window,std::function<void()> changed)
    :impl(std::make_shared<Impl>()) {
    impl->send=std::move(send);impl->window=window;impl->changed=std::move(changed);impl->init();
}
WorkspaceStorageView::~WorkspaceStorageView()=default;
FrameworkElement WorkspaceStorageView::Root()const{return impl->root;}
void WorkspaceStorageView::Apply(Json const& snapshot,bool blocked){impl->apply(snapshot,blocked);}
bool WorkspaceStorageView::IsOpen()const{return impl->showing;}
void WorkspaceStorageView::Hide(){
    impl->stopping=true;
    if(impl->dialog)impl->dialog.Hide();
    if(impl->picker)impl->picker.Cancel();
}
