#include "pch.h"
#include "SelectionDialog.h"
#include "UiControls.h"

using namespace CapyUi;
struct SelectionDialog::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    ContentDialog dialog;
    XamlRoot xamlRoot{nullptr};
    StackPanel body;
    Bindings fields;
    NumericAdmissions admissions;
    hstring built;
    bool showing=false,closing=false,programmatic=false,stopping=false,cancelPending=false,applyPending=false;
    std::function<void()> changed;
    J view()const{return object(object(data->state,L"layer_tools"),L"selection_resize");}
    void send(J const& action)const{data->dispatch(O({{L"type",S(L"selection")},{L"action",action}}));}
    bool settled()const{return !closing&&!cancelPending&&!applyPending;}
    void init(){
        dialog.XamlRoot(xamlRoot);dialog.Content(body);dialog.DefaultButton(ContentDialogButton::Primary);
        dialog.PrimaryButtonText(data->common(L"apply"));dialog.CloseButtonText(data->common(L"cancel"));
        AutomationProperties::SetAutomationId(dialog,L"selection-resize-dialog");
        body.MinWidth(280);body.Spacing(8);
        auto weak=weak_from_this();
        dialog.PrimaryButtonClick([weak](auto&&,ContentDialogButtonClickEventArgs const& e){
            e.Cancel(true);
            if(auto self=weak.lock();self&&self->settled()&&self->view().Size()){
                for(auto const& commit:self->admissions)if(!commit(false))return;
                self->applyPending=true;self->dialog.IsPrimaryButtonEnabled(false);self->send(O({{L"op",S(L"apply_resize")}}));
            }
        });
        dialog.Closing([weak](auto&&,ContentDialogClosingEventArgs const& e){if(auto self=weak.lock()){
            if(!self->stopping&&!self->programmatic&&self->view().Size()){
                e.Cancel(true);
                if(!self->cancelPending&&!self->applyPending){
                    for(auto const& commit:self->admissions)commit(true);
                    self->cancelPending=true;self->dialog.IsPrimaryButtonEnabled(false);self->send(O({{L"op",S(L"cancel_resize")}}));
                }
                return;
            }
            self->closing=true;
        }});
    }
    fire_and_forget show(){
        auto lifetime=shared_from_this();showing=true;closing=false;programmatic=false;cancelPending=false;applyPending=false;
        try{co_await dialog.ShowAsync();}catch(hresult_error const&){}
        showing=false;closing=false;programmatic=false;cancelPending=false;applyPending=false;built=L"";changed();
    }
    void hide(){if(showing&&!closing){programmatic=true;closing=true;dialog.Hide();}}
    void build(J const& resize){
        fields.clear();admissions.clear();body.Children().Clear();
        auto weak=weak_from_this();
        NumberPresentation presentation;presentation.title=[weak]{if(auto self=weak.lock())return str(self->view(),L"label");return hstring();};
        auto control=number(data,str(resize,L"label"),object(resize,L"numeric"),
            [weak]{if(auto self=weak.lock())return num(self->view(),L"radius");return 0.;},
            [weak](double value){if(auto self=weak.lock();self&&self->settled()&&!self->data->updating)
                self->send(O({{L"op",S(L"resize_radius")},{L"radius",N(value)}}));},
            fields,nullptr,false,L"selection-resize-distance",false,presentation,&admissions);
        body.Children().Append(control);
    }
    void apply(J const& snapshot,bool blocked){
        data->adoptLocalization(snapshot);
        data->model=snapshot;data->state=object(snapshot,L"state");data->refreshPalette();
        auto resize=view();
        if(!resize.Size()){hide();return;}
        if(!showing&&blocked)return;
        dialog.RequestedTheme(data->theme()==L"light"?ElementTheme::Light:ElementTheme::Dark);
        dialog.Language(data->language());dialog.PrimaryButtonText(data->common(L"apply"));dialog.CloseButtonText(data->common(L"cancel"));
        dialog.Title(box_value(str(resize,L"title")));
        auto key=object(resize,L"numeric").Stringify();
        if(key!=built){built=key;build(resize);}
        data->updating=true;
        struct Reset{bool& flag;~Reset(){flag=false;}} reset{data->updating};
        for(auto const& bind:fields)bind();
        if(!showing){dialog.IsPrimaryButtonEnabled(true);show();}
    }
};
SelectionDialog::SelectionDialog(Dispatch send,Json catalog,std::shared_ptr<CapyLocalization> localization,XamlRoot root,std::function<void()> changed):impl(std::make_shared<Impl>()){
    impl->data->send=std::move(send);impl->data->localization=localization;impl->data->catalog=catalog;impl->xamlRoot=root;impl->changed=std::move(changed);impl->init();
}
SelectionDialog::~SelectionDialog(){impl->stopping=true;impl->hide();}
void SelectionDialog::Apply(Json const& snapshot,bool blocked){impl->apply(snapshot,blocked);}
bool SelectionDialog::IsOpen()const{return impl->showing;}
void SelectionDialog::CancelAll(){if(impl->view().Size())impl->send(O({{L"op",S(L"cancel_resize")}}));}
void SelectionDialog::Hide(){impl->stopping=true;impl->hide();}
