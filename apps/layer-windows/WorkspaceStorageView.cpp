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
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    std::function<void()> changed;
    Window window{nullptr};
    J model,copy;
    Border root;
    TextBlock message;
    StackPanel buttons;
    Button retry,saveNew,backup;
    ContentDialog dialog{nullptr};
    Windows::Foundation::IAsyncOperation<Pickers::PickFileResult> picker{nullptr};
    bool showing=false,stopping=false;
    uint32_t handledClose=0;
    uint64_t handledPreferencesClose=0;
    void dispatch(hstring operation) { send(to_string(O({{L"operation",S(operation)}}).Stringify())); }
    static J input(J const& value) { return O({{L"operation",S(L"input")},{L"input",value}}); }
    static J input(hstring const& type) { return input(O({{L"type",S(type)}})); }
    hstring text(wchar_t const* key)const{return str(object(copy,L"recovery"),key);}
    void failure() { send(to_string(O({{L"operation",S(L"failure")},{L"error",S(text(L"workspace_dialog_failed"))}}).Stringify())); }
    void init() {
        root.HorizontalAlignment(HorizontalAlignment::Center);root.VerticalAlignment(VerticalAlignment::Bottom);
        root.Margin({16,16,16,64});root.Padding({12,6,12,6});root.CornerRadius({18*CornerFit,18*CornerFit,18*CornerFit,18*CornerFit});
        root.MaxWidth(640);root.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(root,L"workspace-storage-status");
        Grid content;content.ColumnSpacing(8);ColumnDefinition text;text.Width({1,GridUnitType::Star});ColumnDefinition actions;actions.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(text);content.ColumnDefinitions().Append(actions);
        message=label(data,L"");message.TextWrapping(TextWrapping::Wrap);message.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(message);
        buttons.Orientation(Orientation::Horizontal);buttons.Spacing(8);
        retry=raisedButton(data,L"",[]{});saveNew=raisedButton(data,L"",[]{});backup=raisedButton(data,L"",[]{});
        for(auto control:{retry,saveNew,backup}){control.MinHeight(34);}
        AutomationProperties::SetAutomationId(retry,L"workspace-storage-retry");
        AutomationProperties::SetAutomationId(saveNew,L"workspace-storage-save-new");
        AutomationProperties::SetAutomationId(backup,L"workspace-storage-backup");
        retry.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->send(to_string(input(L"retry").Stringify()));});
        saveNew.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->send(to_string(input(
            O({{L"type",S(L"form")},{L"action",O({{L"type",S(L"save_as_new")}})}})).Stringify()));});
        backup.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->exportBackup();});
        buttons.Children().Append(retry);buttons.Children().Append(saveNew);buttons.Children().Append(backup);
        Grid::SetColumn(buttons,1);buttons.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(buttons);root.Child(content);
    }
    void restore() {
        HWND handle=nullptr;check_hresult(window.as<IWindowNative>()->get_WindowHandle(&handle));
        if(IsIconic(handle))ShowWindow(handle,SW_RESTORE);
    }
    fire_and_forget show(bool preferences=false) {
        auto lifetime=shared_from_this();
        if(showing||stopping)co_return;
        showing=true;changed();
        auto reply=[preferences](hstring const& operation,hstring const& type){
            return preferences?O({{L"operation",S(L"preferences_"+operation)}}):input(type);
        };
        J response=reply(L"keep_open",L"resume");
        try {
            restore();
            dialog=ContentDialog();dialog.XamlRoot(window.Content().XamlRoot());
            dialogSurface(data,dialog);
            dialog.DefaultButton(ContentDialogButton::Close);
            StackPanel body;body.Spacing(12);body.MaxWidth(460);
            TextBlock explanation;explanation.TextWrapping(TextWrapping::Wrap);
            bool ready=flag(object(model,L"windows_workspace"),L"ready");
            auto title=label(data,text(preferences?L"preferences_not_saved":ready?L"workspace_not_saved":L"workspace_not_opened"),true);title.TextAlignment(TextAlignment::Center);title.HorizontalAlignment(HorizontalAlignment::Stretch);dialog.Title(title);
            dialog.PrimaryButtonText(text(L"retry"));
            dialog.SecondaryButtonText(text(L"close_without_saving"));
            dialog.CloseButtonText(str(object(copy,L"common"),L"keep_open"));
            hstring recovery=text(preferences?L"preferences_not_saved_help":ready?L"workspace_not_saved_help":L"workspace_not_opened_help");
            explanation.Text(str(object(model,preferences?L"windows_settings_close":L"windows_workspace"),L"error")+L"\n\n"+recovery);
            body.Children().Append(explanation);
            AutomationProperties::SetAutomationId(dialog,preferences?L"preferences-close-error":L"workspace-close-error");
            dialog.Content(body);
            auto result=co_await dialog.ShowAsync();
            if(result==ContentDialogResult::Primary)response=reply(L"retry",L"retry");
            else if(result==ContentDialogResult::Secondary)response=reply(L"discard_close",L"discard_close");
        } catch(hresult_canceled const&) {
        } catch(...) { if(!stopping)failure(); }
        dialog=nullptr;
        if(!stopping)send(to_string(response.Stringify()));
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
            save.CommitButtonText(str(object(copy,L"common"),L"save"));save.DefaultFileExtension(extension);
            save.SuggestedFileName(text(workspace?L"workspace_backup":L"workspace_database"));
            save.FileTypeChoices().Insert(text(workspace?L"workspace_backup":L"database_type"),single_threaded_vector<hstring>({extension}));
            picker=save.PickSaveFileAsync();
            auto selected=co_await picker;
            if(selected&&!stopping)send(to_string(O({{L"operation",S(workspace?L"export_backup":L"backup_database")},{L"path",S(selected.Path())}}).Stringify()));
        } catch(hresult_canceled const&) {
        } catch(...) {if(!stopping)failure();}
        picker=nullptr;showing=false;changed();
    }
    void apply(J const& snapshot,J const& bootstrap,bool blocked) {
        data->adoptLocalization(snapshot);model=snapshot;copy=bootstrap;data->state=object(snapshot,L"state");data->refreshPalette();
        if(dialog)dialogSurface(data,dialog);
        message.FontSize(data->textSize());for(auto control:{retry,saveNew,backup}){control.FontSize(data->textSize());control.CornerRadius(data->controlCorner());}
        buttonCaption(retry,text(L"retry"));buttonCaption(saveNew,text(L"save_as_new_workspace"));buttonCaption(backup,text(L"export_backup"));
        auto storage=object(model,L"windows_workspace");
        if(!storage.Size())return;
        auto error=str(storage,L"error"),notice=str(storage,L"notice");
        hstring status=error;
        if(!notice.empty())status=status.empty()?notice:status+L"\n"+notice;
        if(status.empty()&&!flag(storage,L"ready"))status=text(L"opening_workspace");
        if(status.empty()&&flag(storage,L"closing"))status=text(L"saving_workspace");
        message.Text(status);root.Visibility(status.empty()?Visibility::Collapsed:Visibility::Visible);
        auto preferences=object(model,L"windows_settings_close");
        if(flag(preferences,L"requested")&&!flag(preferences,L"ready")){
            root.Visibility(Visibility::Collapsed);
            auto attempt=uint64_t(num(preferences,L"attempt"));
            if(!blocked&&!showing&&!stopping&&!flag(preferences,L"busy")&&!str(preferences,L"error").empty()&&attempt!=handledPreferencesClose){
                handledPreferencesClose=attempt;show(true);
            }
            return;
        }
        auto palette=object(object(model,L"state"),L"palette");
        root.Background(data->brush(L"bg"));
        message.Foreground(fill(color(str(palette,L"text",L"#fafafb"))));
        buttons.Visibility(error.empty()?Visibility::Collapsed:Visibility::Visible);
        for(auto button:{retry,saveNew,backup})button.IsEnabled(!blocked&&!showing&&!flag(storage,L"busy"));
        saveNew.Visibility(flag(storage,L"ready")?Visibility::Visible:Visibility::Collapsed);
        auto attempt=uint32_t(num(storage,L"close_attempt"));
        if(!blocked&&!showing&&!stopping&&flag(storage,L"closing")&&!flag(storage,L"busy")&&!error.empty()&&attempt!=handledClose) {
            handledClose=attempt;show();
        }
    }
};
WorkspaceStorageView::WorkspaceStorageView(Dispatch send,Window window,std::function<void()> changed)
    :impl(std::make_shared<Impl>()) {
    impl->send=std::move(send);impl->window=window;impl->changed=std::move(changed);impl->init();
}
WorkspaceStorageView::~WorkspaceStorageView()=default;
FrameworkElement WorkspaceStorageView::Root()const{return impl->root;}
void WorkspaceStorageView::Apply(Json const& snapshot,Json const& bootstrap,bool blocked){impl->apply(snapshot,bootstrap,blocked);}
bool WorkspaceStorageView::IsOpen()const{return impl->showing;}
void WorkspaceStorageView::Hide(){
    impl->stopping=true;
    if(impl->dialog)impl->dialog.Hide();
    if(impl->picker)impl->picker.Cancel();
}
