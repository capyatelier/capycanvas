#include "pch.h"
#include "SizeDialog.h"
#include "UiControls.h"

using namespace CapyUi;
struct SizeDialog::Impl:std::enable_shared_from_this<Impl>{
    static constexpr double Width=332,Cell=36;
    struct Field{Border slot;hstring spec;uint64_t generation=0;Bindings binds;NumericAdmissions admissions;};
    Kind kind=Kind::Canvas;
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    ContentDialog dialog;
    XamlRoot xamlRoot{nullptr};
    StackPanel body,values;
    std::vector<Field> fields;
    ComboBox unit,resample;
    CheckBox option;
    TextBlock caption,message;
    Grid anchorGrid;
    std::vector<Button> anchors;
    hstring units,resamples,anchorKey;
    bool showing=false,closing=false,programmatic=false,stopping=false,cancelPending=false,applyPending=false;
    std::function<void()> changed;
    wchar_t const* type()const{return kind==Kind::Canvas?L"canvas_size":L"image_size";}
    wchar_t const* optionKey()const{return kind==Kind::Canvas?L"relative":L"constrain";}
    hstring id(wchar_t const* part)const{return hstring(kind==Kind::Canvas?L"canvas-size-":L"image-size-")+part;}
    J view()const{return object(object(data->state,L"layer_tools"),type());}
    void send(J const& action)const{data->dispatch(O({{L"type",S(type())},{L"action",action}}));}
    bool settled()const{return !closing&&!cancelPending&&!applyPending;}
    bool commit(){
        bool admitted=true;
        for(auto& field:fields)for(auto const& admit:field.admissions)admitted=admit(false)&&admitted;
        return admitted;
    }
    void choose(J const& action){
        if(data->updating||!settled())return;
        if(commit()&&view().Size())send(action);
    }
    static wchar_t const* op(size_t index){return index==0?L"width":index==1?L"height":L"resolution";}
    static J spec(J const& size,size_t index){return index<2?array(size,L"numeric").GetObjectAt(uint32_t(index)):object(size,L"resolution_numeric");}
    static hstring label(J const& size,size_t index){return index<2?array(size,L"labels").GetStringAt(uint32_t(index)):str(size,L"resolution_label");}
    static double value(J const& size,size_t index){
        if(!size.Size())return 0;
        return index<2?array(size,L"values").GetNumberAt(uint32_t(index)):num(size,L"resolution");
    }
    void choiceBox(ComboBox Impl::* member,hstring const& automation,wchar_t const* key,wchar_t const* list){
        auto box=this->*member;box.HorizontalAlignment(HorizontalAlignment::Stretch);box.MinWidth(0);
        AutomationProperties::SetAutomationId(box,automation);
        box.SelectionChanged([weak=weak_from_this(),member,key,list](auto&&,auto&&){if(auto self=weak.lock()){
            auto choices=array(self->view(),list);auto index=(self.get()->*member).SelectedIndex();
            if(index<0||uint32_t(index)>=choices.Size())return;
            auto next=str(choices.GetObjectAt(index),key);
            if(next!=str(self->view(),key))self->choose(O({{L"op",S(key)},{key,S(next)}}));
        }});
    }
    Grid row(FrameworkElement const& lead,FrameworkElement const& tail){
        Grid line;line.ColumnSpacing(12);
        ColumnDefinition grow;grow.Width({1,GridUnitType::Star});line.ColumnDefinitions().Append(grow);
        ColumnDefinition fit;fit.Width({1,GridUnitType::Auto});line.ColumnDefinitions().Append(fit);
        line.Children().Append(lead);Grid::SetColumn(tail,1);line.Children().Append(tail);
        return line;
    }
    void init(){
        dialog.XamlRoot(xamlRoot);inheritLanguage(dialog,data);dialog.Content(body);dialog.DefaultButton(ContentDialogButton::Primary);
        AutomationProperties::SetAutomationId(dialog,id(L"dialog"));
        body.Width(Width);body.Spacing(10);
        values.Spacing(10);body.Children().Append(values);
        fields.resize(kind==Kind::Canvas?2:3);
        for(size_t i=0;i<2;++i)values.Children().Append(fields[i].slot);
        auto weak=weak_from_this();
        choiceBox(&Impl::unit,id(L"unit"),L"unit",L"units");
        option.MinWidth(0);option.MinHeight(34);AutomationProperties::SetAutomationId(option,id(optionKey()));
        option.Click([weak](auto&&,auto&&){if(auto self=weak.lock()){
            bool next=self->option.IsChecked().Value();
            self->option.IsChecked(flag(self->view(),self->optionKey()));
            self->choose(O({{L"op",S(self->optionKey())},{self->optionKey(),B(next)}}));
        }});
        body.Children().Append(row(unit,option));
        caption.VerticalAlignment(VerticalAlignment::Center);
        if(kind==Kind::Canvas){
            anchorGrid.RowSpacing(2);anchorGrid.ColumnSpacing(2);
            for(int i=0;i<3;++i){
                ColumnDefinition column;column.Width({Cell,GridUnitType::Pixel});anchorGrid.ColumnDefinitions().Append(column);
                RowDefinition line;line.Height({Cell,GridUnitType::Pixel});anchorGrid.RowDefinitions().Append(line);
            }
            body.Children().Append(row(caption,anchorGrid));
        }else{
            body.Children().Append(fields[2].slot);
            choiceBox(&Impl::resample,id(L"resample"),L"resample",L"resamples");resample.MinWidth(160);
            body.Children().Append(row(caption,resample));
        }
        message.TextWrapping(TextWrapping::Wrap);message.Opacity(.72);message.MinHeight(20);
        AutomationProperties::SetAutomationId(message,id(L"message"));
        AutomationProperties::SetLiveSetting(message,Automation::Peers::AutomationLiveSetting::Polite);
        body.Children().Append(message);
        dialog.PrimaryButtonClick([weak](auto&&,ContentDialogButtonClickEventArgs const& e){
            e.Cancel(true);
            auto self=weak.lock();if(!self||!self->settled()||!self->view().Size())return;
            if(!self->commit()||!flag(self->view(),L"can_apply"))return;
            self->applyPending=true;self->dialog.IsPrimaryButtonEnabled(false);self->send(O({{L"op",S(L"apply")}}));
        });
        dialog.Closing([weak](auto&&,ContentDialogClosingEventArgs const& e){if(auto self=weak.lock()){
            if(!self->stopping&&!self->programmatic&&self->view().Size()){
                e.Cancel(true);
                if(!self->cancelPending&&!self->applyPending){
                    self->cancelPending=true;self->dialog.IsPrimaryButtonEnabled(false);self->send(O({{L"op",S(L"cancel")}}));
                }
                return;
            }
            self->closing=true;
        }});
    }
    fire_and_forget show(){
        auto lifetime=shared_from_this();showing=true;closing=false;programmatic=false;cancelPending=false;applyPending=false;
        try{co_await dialog.ShowAsync();}catch(hresult_error const&){}
        showing=false;closing=false;programmatic=false;cancelPending=false;applyPending=false;
        for(auto& field:fields)field.spec=L"";
        units=L"";resamples=L"";anchorKey=L"";changed();
    }
    void hide(){if(showing&&!closing){programmatic=true;closing=true;dialog.Hide();}}
    void build(size_t index,J const& size){
        auto& field=fields[index];auto generation=++field.generation;
        field.binds.clear();field.admissions.clear();
        auto weak=weak_from_this();auto operation=op(index);auto control=spec(size,index);
        NumberPresentation presentation;presentation.title=[weak,index]{if(auto self=weak.lock())return label(self->view(),index);return hstring();};
        auto number=CapyUi::number(data,label(size,index),control,
            [weak,index]{if(auto self=weak.lock())return value(self->view(),index);return 0.;},
            [weak,operation,index,generation](double next){
                if(auto self=weak.lock();self&&self->fields[index].generation==generation&&self->settled()&&!self->data->updating)
                    self->send(O({{L"op",S(operation)},{L"value",N(next)}}));
            },
            field.binds,nullptr,false,id(operation),false,presentation,&field.admissions);
        if(auto entry=numberEntry(number)){
            if(str(control,L"kind")!=L"slider")entry.Width(96);
            entry.TextChanged([weak,index,operation,control,generation](Windows::Foundation::IInspectable const& sender,auto&&){
                auto self=weak.lock();
                if(!self||self->fields[index].generation!=generation||self->data->updating||!self->settled())return;
                auto size=self->view();if(!size.Size())return;
                double typed;
                try{typed=num(numeric(self->data->localization.get(),control,value(size,index),O({{L"type",S(L"expression")},{L"text",S(sender.as<TextBox>().Text())}})),L"value");}
                catch(hresult_error const&){return;}
                if(typed!=value(size,index))self->send(O({{L"op",S(operation)},{L"value",N(typed)}}));
            });
        }
        field.slot.Child(number);
    }
    void buildAnchors(J const& size){
        anchors.clear();anchorGrid.Children().Clear();
        auto choices=array(size,L"anchors");
        for(uint32_t i=0;i<choices.Size();++i){
            auto choice=choices.GetObjectAt(i);auto anchor=str(choice,L"anchor");auto weak=weak_from_this();
            auto cell=button(data,L"",[weak,anchor]{if(auto self=weak.lock())self->choose(O({{L"op",S(L"anchor")},{L"anchor",S(anchor)}}));});
            cell.Width(Cell);cell.Height(Cell);cell.MinWidth(0);cell.MinHeight(0);cell.Padding({0,0,0,0});
            cell.IsTabStop(false);cell.AllowFocusOnInteraction(false);cell.Tag(box_value(anchor));
            cell.Content(icon(L"rectangle-fill",data->theme()));cell.Background(data->tint(L"text",20));
            AutomationProperties::SetAutomationId(cell,id(L"anchor-")+anchor);AutomationProperties::SetName(cell,str(choice,L"label"));
            tooltip(cell,str(choice,L"label"));
            Grid::SetColumn(cell,int32_t(i%3));Grid::SetRow(cell,int32_t(i/3));
            anchorGrid.Children().Append(cell);anchors.push_back(cell);
        }
    }
    void populate(ComboBox const& box,hstring& signature,A const& choices,wchar_t const* key,hstring const& chosen){
        if(auto next=choices.Stringify();next!=signature){
            signature=next;box.Items().Clear();
            for(auto const& choice:choices)box.Items().Append(box_value(str(choice.GetObject(),L"label")));
        }
        for(uint32_t i=0;i<choices.Size();++i)if(str(choices.GetObjectAt(i),key)==chosen){
            if(box.SelectedIndex()!=int32_t(i))box.SelectedIndex(i);
            AutomationProperties::SetName(box,str(choices.GetObjectAt(i),L"label"));
        }
    }
    void apply(J const& snapshot,bool blocked){
        data->adoptLocalization(snapshot);data->model=snapshot;data->state=object(snapshot,L"state");data->refreshPalette();
        auto size=view();
        if(!size.Size()){hide();return;}
        if(!showing&&blocked)return;
        dialog.RequestedTheme(data->theme()==L"light"?ElementTheme::Light:ElementTheme::Dark);
        dialog.Title(box_value(str(size,L"title")));
        dialog.PrimaryButtonText(str(size,L"apply_label"));dialog.CloseButtonText(str(size,L"cancel_label"));
        data->updating=true;
        struct Reset{bool& flag;~Reset(){flag=false;}} reset{data->updating};
        for(size_t i=0;i<fields.size();++i){
            auto key=spec(size,i).Stringify()+L"|"+data->theme();
            if(key!=fields[i].spec){fields[i].spec=key;build(i,size);}
        }
        populate(unit,units,array(size,L"units"),L"unit",str(size,L"unit"));
        option.Content(box_value(str(size,kind==Kind::Canvas?L"relative_label":L"constrain_label")));
        option.IsChecked(flag(size,optionKey()));
        if(kind==Kind::Canvas){
            caption.Text(str(size,L"anchor_label"));
            if(auto key=array(size,L"anchors").Stringify()+data->theme();key!=anchorKey){anchorKey=key;buildAnchors(size);}
            for(auto const& cell:anchors)cell.Content().as<UIElement>().Opacity(unbox_value<hstring>(cell.Tag())==str(size,L"anchor")?1.:0.);
        }else{
            caption.Text(str(size,L"resample_label"));
            populate(resample,resamples,array(size,L"resamples"),L"resample",str(size,L"resample"));
        }
        message.Text(str(size,L"message"));
        for(auto& field:fields)for(auto const& bind:field.binds)bind();
        dialog.IsPrimaryButtonEnabled(flag(size,L"can_apply")&&!applyPending&&!cancelPending);
        if(!showing)show();
    }
};
SizeDialog::SizeDialog(Kind kind,Dispatch send,Json catalog,std::shared_ptr<CapyLocalization> localization,XamlRoot root,std::function<void()> changed):impl(std::make_shared<Impl>()){
    impl->kind=kind;impl->data->send=std::move(send);impl->data->localization=localization;impl->data->catalog=catalog;
    impl->xamlRoot=root;impl->changed=std::move(changed);impl->init();
}
SizeDialog::~SizeDialog(){impl->stopping=true;impl->hide();}
void SizeDialog::Apply(Json const& snapshot,bool blocked){impl->apply(snapshot,blocked);}
bool SizeDialog::IsOpen()const{return impl->showing;}
void SizeDialog::CancelAll(){if(impl->view().Size())impl->send(O({{L"op",S(L"cancel")}}));}
void SizeDialog::Hide(){impl->stopping=true;impl->hide();}
