#include "pch.h"
#include "CanvasSizeDialog.h"
#include "UiControls.h"

using namespace CapyUi;
struct CanvasSizeDialog::Impl:std::enable_shared_from_this<Impl>{
    static constexpr double Width=332,Cell=36;
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    ContentDialog dialog;
    XamlRoot xamlRoot{nullptr};
    StackPanel body,values;
    Grid anchorGrid;
    ComboBox unit;
    CheckBox relative;
    TextBlock anchorLabel,message;
    std::vector<Button> anchors;
    Bindings fields,commits;
    hstring built,units;
    uint64_t generation=0;
    bool showing=false,closing=false,programmatic=false,stopping=false,cancelPending=false,applyPending=false;
    std::function<void()> changed;
    J view()const{return object(object(data->state,L"layer_tools"),L"canvas_size");}
    void send(J const& action)const{data->dispatch(O({{L"type",S(L"canvas_size")},{L"action",action}}));}
    bool settled()const{return !closing&&!cancelPending&&!applyPending;}
    void commit(){for(auto const& field:commits)field();}
    void choose(J const& action){
        if(data->updating||!settled())return;
        commit();if(view().Size())send(action);
    }
    void init(){
        dialog.XamlRoot(xamlRoot);dialog.Content(body);dialog.DefaultButton(ContentDialogButton::Primary);
        
        AutomationProperties::SetAutomationId(dialog,L"canvas-size-dialog");
        body.Width(Width);body.Spacing(10);
        values.Spacing(10);body.Children().Append(values);
        auto weak=weak_from_this();
        Grid options;options.ColumnSpacing(12);
        ColumnDefinition grow;grow.Width({1,GridUnitType::Star});options.ColumnDefinitions().Append(grow);
        ColumnDefinition fit;fit.Width({1,GridUnitType::Auto});options.ColumnDefinitions().Append(fit);
        unit.HorizontalAlignment(HorizontalAlignment::Stretch);unit.MinWidth(0);
        AutomationProperties::SetAutomationId(unit,L"canvas-size-unit");
        unit.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock()){
            auto choices=array(self->view(),L"units");auto index=self->unit.SelectedIndex();
            if(index<0||uint32_t(index)>=choices.Size())return;
            auto next=str(choices.GetObjectAt(index),L"unit");
            if(next!=str(self->view(),L"unit"))self->choose(O({{L"op",S(L"unit")},{L"unit",S(next)}}));
        }});
        options.Children().Append(unit);
        relative.MinWidth(0);relative.MinHeight(34);Grid::SetColumn(relative,1);
        AutomationProperties::SetAutomationId(relative,L"canvas-size-relative");
        relative.Click([weak](auto&&,auto&&){if(auto self=weak.lock()){
            bool next=self->relative.IsChecked().Value();
            self->relative.IsChecked(flag(self->view(),L"relative"));
            self->choose(O({{L"op",S(L"relative")},{L"relative",B(next)}}));
        }});
        options.Children().Append(relative);body.Children().Append(options);
        Grid anchorRow;anchorRow.ColumnSpacing(12);
        ColumnDefinition caption;caption.Width({1,GridUnitType::Star});anchorRow.ColumnDefinitions().Append(caption);
        ColumnDefinition cells;cells.Width({1,GridUnitType::Auto});anchorRow.ColumnDefinitions().Append(cells);
        anchorLabel.VerticalAlignment(VerticalAlignment::Center);anchorRow.Children().Append(anchorLabel);
        anchorGrid.RowSpacing(2);anchorGrid.ColumnSpacing(2);Grid::SetColumn(anchorGrid,1);
        for(int i=0;i<3;++i){
            ColumnDefinition column;column.Width({Cell,GridUnitType::Pixel});anchorGrid.ColumnDefinitions().Append(column);
            RowDefinition row;row.Height({Cell,GridUnitType::Pixel});anchorGrid.RowDefinitions().Append(row);
        }
        anchorRow.Children().Append(anchorGrid);body.Children().Append(anchorRow);
        message.TextWrapping(TextWrapping::Wrap);message.Opacity(.72);message.MinHeight(20);
        AutomationProperties::SetAutomationId(message,L"canvas-size-message");
        AutomationProperties::SetLiveSetting(message,Automation::Peers::AutomationLiveSetting::Polite);
        body.Children().Append(message);
        dialog.PrimaryButtonClick([weak](auto&&,ContentDialogButtonClickEventArgs const& e){
            e.Cancel(true);
            auto self=weak.lock();if(!self||!self->settled()||!self->view().Size())return;
            self->commit();
            if(!flag(self->view(),L"can_apply"))return;
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
        showing=false;closing=false;programmatic=false;cancelPending=false;applyPending=false;built=L"";units=L"";changed();
    }
    void hide(){if(showing&&!closing){programmatic=true;closing=true;dialog.Hide();}}
    void field(uint32_t axis,J const& size){
        auto weak=weak_from_this();auto op=axis?L"height":L"width";auto current=generation;
        auto spec=array(size,L"numeric").GetObjectAt(axis);
        auto control=number(data,array(size,L"labels").GetStringAt(axis),spec,
            [weak,axis]{if(auto self=weak.lock())return array(self->view(),L"values").GetNumberAt(axis);return 0.;},
            [weak,op,current](double value){if(auto self=weak.lock();self&&self->generation==current&&self->settled()&&!self->data->updating)
                self->send(O({{L"op",S(op)},{L"value",N(value)}}));},
            fields,&commits,false,hstring(L"canvas-size-")+op);
        if(auto entry=numberEntry(control)){
            entry.Width(96);
            entry.TextChanged([weak,axis,op,spec,current](Windows::Foundation::IInspectable const& sender,auto&&){
                auto self=weak.lock();if(!self||self->generation!=current||self->data->updating||!self->settled())return;
                auto current=array(self->view(),L"values");if(axis>=current.Size())return;
                double value;
                try{value=num(numeric(self->data->localization.get(),spec,current.GetNumberAt(axis),O({{L"type",S(L"expression")},{L"text",S(sender.as<TextBox>().Text())}})),L"value");}
                catch(hresult_error const&){return;}
                if(value!=current.GetNumberAt(axis))self->send(O({{L"op",S(op)},{L"value",N(value)}}));
            });
        }
        values.Children().Append(control);
    }
    void build(J const& size){
        ++generation;fields.clear();commits.clear();values.Children().Clear();
        field(0,size);field(1,size);
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
            AutomationProperties::SetAutomationId(cell,L"canvas-size-anchor-"+anchor);AutomationProperties::SetName(cell,str(choice,L"label"));
            tooltip(cell,str(choice,L"label"));
            Grid::SetColumn(cell,int32_t(i%3));Grid::SetRow(cell,int32_t(i/3));
            anchorGrid.Children().Append(cell);anchors.push_back(cell);
        }
    }
    void apply(J const& snapshot,bool blocked){
        data->model=snapshot;data->state=object(snapshot,L"state");data->refreshPalette();
        auto size=view();
        if(!size.Size()){hide();return;}
        if(!showing&&blocked)return;
        dialog.RequestedTheme(data->theme()==L"light"?ElementTheme::Light:ElementTheme::Dark);
        dialog.Title(box_value(str(size,L"title")));dialog.PrimaryButtonText(str(size,L"apply_label"));dialog.CloseButtonText(str(size,L"cancel_label"));
        data->updating=true;
        struct Reset{bool& flag;~Reset(){flag=false;}} reset{data->updating};
        auto key=array(size,L"numeric").Stringify()+array(size,L"labels").Stringify()+data->theme();
        if(key!=built){
            bool rebuildAnchors=built.empty()||!built.ends_with(data->theme());
            built=key;build(size);
            if(rebuildAnchors)buildAnchors(size);
        }
        auto choices=array(size,L"units");auto unitKey=choices.Stringify();
        if(unitKey!=units){
            units=unitKey;unit.Items().Clear();
            for(auto const& choice:choices)unit.Items().Append(box_value(str(choice.GetObject(),L"label")));
        }
        for(uint32_t i=0;i<choices.Size();++i)if(str(choices.GetObjectAt(i),L"unit")==str(size,L"unit")&&unit.SelectedIndex()!=int32_t(i))unit.SelectedIndex(i);
        relative.Content(box_value(str(size,L"relative_label")));relative.IsChecked(flag(size,L"relative"));
        anchorLabel.Text(str(size,L"anchor_label"));
        for(auto const& cell:anchors){
            bool chosen=unbox_value<hstring>(cell.Tag())==str(size,L"anchor");
            cell.Content().as<UIElement>().Opacity(chosen?1.:0.);
        }
        message.Text(str(size,L"message"));
        for(auto const& bind:fields)bind();
        dialog.IsPrimaryButtonEnabled(flag(size,L"can_apply")&&!applyPending&&!cancelPending);
        if(!showing)show();
    }
};
CanvasSizeDialog::CanvasSizeDialog(Dispatch send,Json catalog,std::shared_ptr<CapyLocalization> localization,XamlRoot root,std::function<void()> changed):impl(std::make_shared<Impl>()){
    impl->data->send=std::move(send);impl->data->localization=localization;impl->data->catalog=catalog;impl->xamlRoot=root;impl->changed=std::move(changed);impl->init();
}
CanvasSizeDialog::~CanvasSizeDialog(){impl->stopping=true;impl->hide();}
void CanvasSizeDialog::Apply(Json const& snapshot,bool blocked){impl->apply(snapshot,blocked);}
bool CanvasSizeDialog::IsOpen()const{return impl->showing;}
void CanvasSizeDialog::CancelAll(){if(impl->view().Size())impl->send(O({{L"op",S(L"cancel")}}));}
void CanvasSizeDialog::Hide(){impl->stopping=true;impl->hide();}
