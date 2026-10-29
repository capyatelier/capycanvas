#pragma once
#include "UiControls.h"
#include "ProofDial.h"
#include "WorkflowMotion.h"
#include "ProofForm.h"

namespace CapyUi {
// Each retained panel/drawer owns native controls; Rust owns mode and edit policy.
inline StackPanel ProofPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    StackPanel root;root.Spacing(data->panelMetric(L"control_spacing"));
    auto form=[data]{return object(data->model,L"windows_proof_form");};
    auto copy=[form](wchar_t const* key){return str(object(form(),L"copy"),key);};
    auto send=[data](J const& action){
        auto epoch=to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")));
        data->dispatch(O({{L"windows_proof_action",action},{L"windows_epoch",S(epoch)}}));
    };
    Grid mode;mode.ColumnSpacing(data->panelMetric(L"control_spacing"));
    AutomationProperties::SetAutomationId(mode,L"proof-panel-mode");AutomationProperties::SetName(mode,copy(L"mode"));
    std::vector<std::pair<Button,hstring>> modes;
    for(auto [value,name]:std::initializer_list<std::pair<hstring,hstring>>{{L"off",copy(L"mode_off")},{L"sdr",L"SDR"},{L"print",copy(L"mode_print")}}){
        auto choice=button(data,name,[data,send,value=value]{if(!data->updating)send(O({{L"type",S(L"mode")},{L"mode",S(value)}}));});
        choice.HorizontalAlignment(HorizontalAlignment::Stretch);choice.MinHeight(data->controlHeight());choice.Padding({6,0,6,0});
        AutomationProperties::SetAutomationId(choice,L"proof-panel-mode-"+value);
        mode.Children().Append(choice);modes.emplace_back(choice,value);
    }
    auto arrangeModes=[mode,modes](bool hdr){
        mode.ColumnDefinitions().Clear();
        for(auto const& [choice,id]:modes){
            bool visible=id!=L"sdr"||hdr;choice.Visibility(visible?Visibility::Visible:Visibility::Collapsed);
            if(!visible)continue;
            Grid::SetColumn(choice,int32_t(mode.ColumnDefinitions().Size()));
            ColumnDefinition column;column.Width({1,GridUnitType::Star});mode.ColumnDefinitions().Append(column);
        }
    };
    auto modeHdr=std::make_shared<bool>(flag(form(),L"hdr"));arrangeModes(*modeHdr);
    root.Children().Append(mode);
    auto pages=std::make_shared<WorkflowPages>();Border off;pages->append(off);root.Children().Append(pages->root);
    StackPanel sdr;sdr.Spacing(data->panelMetric(L"control_spacing"));ContentControl sdrGate;sdrGate.IsTabStop(false);sdrGate.Content(sdr);sdrGate.Visibility(Visibility::Collapsed);pages->append(sdrGate);
    sdr.Children().Append(ProofDialControl(data,bindings));
    auto initial=form();
    NumberPresentation presentation;presentation.identity=[form]{return array(form(),L"identity").Stringify();};
    for(auto item:array(initial,L"numbers")){
        auto control=item.GetObject();auto key=str(control,L"key");auto spec=J::Parse(object(control,L"numeric").Stringify());
        spec.Insert(L"kind",S(L"number"));
        presentation.title=[form,key]{return str(find(array(form(),L"numbers"),L"key",key),L"label");};
        sdr.Children().Append(number(data,str(control,L"label"),spec,
            [form,key]{return num(object(form(),L"rendition"),key.c_str());},
            [data,send,key](double value){if(!data->updating)send(O({{L"type",S(L"number")},{L"key",S(key)},{L"value",N(value)}}));},bindings,nullptr,false,L"proof-panel-"+key,false,presentation));
    }
    auto axes=array(object(initial,L"pad"),L"axes");
    for(uint32_t index=0;index<axes.Size();++index){
        auto axis=axes.GetObjectAt(index);auto spec=J::Parse(object(axis,L"numeric").Stringify());spec.Insert(L"kind",S(L"number"));
        presentation.title=[form,index]{return str(array(object(form(),L"pad"),L"axes").GetObjectAt(index),L"label");};
        sdr.Children().Append(number(data,str(axis,L"label"),spec,
            [form,index]{return array(form(),L"pad_values").GetNumberAt(index);},
            [data,send,key=str(axis,L"key")](double value){if(!data->updating)send(O({{L"type",S(L"number")},{L"key",S(key)},{L"value",N(value)}}));},bindings,nullptr,false,L"proof-panel-"+str(axis,L"key"),false,presentation));
    }
    StackPanel print;print.Spacing(data->panelMetric(L"control_spacing"));print.Visibility(Visibility::Collapsed);pages->append(print);
    StackPanel printControls;ContentControl printGate;printGate.IsTabStop(false);printGate.Content(printControls);print.Children().Append(printGate);
    auto printKey=std::make_shared<hstring>();
    auto printForm=std::make_shared<std::shared_ptr<ProofFormView>>();
    auto gamut=compactCheck(data,box_value(copy(L"gamut_warning")));AutomationProperties::SetName(gamut,copy(L"gamut_warning"));AutomationProperties::SetAutomationId(gamut,L"proof-panel-gamut");
    gamut.Click([data](auto&&,auto&&){if(!data->updating)data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"gamut_warning")}}));});print.Children().Append(gamut);
    auto status=label(data,L"");status.TextWrapping(TextWrapping::Wrap);status.Visibility(Visibility::Collapsed);root.Children().Append(status);
    bindings.emplace_back([data,form,copy,mode,modes,arrangeModes,modeHdr,pages,sdrGate,print,printControls,printGate,printKey,printForm,gamut,status]{
        auto value=form();auto selected=str(value,L"mode");bool hdr=flag(value,L"hdr");
        bool available=!flag(object(data->state,L"document_file"),L"busy")&&!flag(data->model,L"windows_rendering_suspended");
        if(hdr!=*modeHdr){*modeHdr=hdr;arrangeModes(hdr);}
        AutomationProperties::SetName(mode,copy(L"mode"));
        for(auto const& [choice,id]:modes){
            auto title=id==L"off"?copy(L"mode_off"):id==L"print"?copy(L"mode_print"):hstring(L"SDR");buttonCaption(choice,title);AutomationProperties::SetName(choice,title);
            bool pressed=id==selected;
            choice.IsEnabled(available);
            choice.Background(pressed?Brush(data->tint(L"text",31)):Brush(clear()));
            AutomationProperties::SetItemStatus(choice,pressed?data->caption(L"search",L"selected"):hstring());
        }
        sdrGate.IsEnabled(available);pages->show(selected==L"print"?2:hdr&&selected==L"sdr"?1:0,true);
        auto settings=object(value,L"print_settings"),document=object(value,L"document_profile");
        auto key=array(value,L"identity").Stringify()+settings.Stringify()+document.Stringify()+array(value,L"library").Stringify()+data->language();
        if(array(value,L"profiles").Size()&&key!=*printKey&&!flag(value,L"preparing")){
            *printKey=key;printControls.Children().Clear();auto details=J::Parse(value.Stringify());
            auto options=J::Parse(value.Stringify());options.Insert(L"document_profile",document);
            if(!object(settings,L"profile").Size())settings.Insert(L"profile",document.Size()?document:array(value,L"profiles").GetObjectAt(0));
            details.Insert(L"form",options);details.Insert(L"settings",settings);details.Insert(L"profiles",array(value,L"library"));
            auto form=std::make_shared<ProofFormView>(data);form->init(details,J(),document.Size()?hstring(L"document"):hstring());
            for(auto box:{form->profile,form->intent,form->simulation})box.FontWeight(winrt::Windows::UI::Text::FontWeights::Bold());
            for(auto const& [control,id]:std::initializer_list<std::pair<FrameworkElement,wchar_t const*>>{{form->profile,L"proof-panel-profile"},{form->intent,L"proof-panel-intent"},{form->simulation,L"proof-panel-simulation"},{form->bpc,L"proof-panel-bpc"}})AutomationProperties::SetAutomationId(control,id);
            *printForm=form;printControls.Children().Append(form->root);
            auto epoch=num(object(data->state,L"document_file"),L"epoch");
            auto edit=[data,source=std::weak_ptr<ProofFormView>(form),epoch]{auto form=source.lock();if(data->updating||!form)return;try{
                auto settings=form->current();data->document(to_string(O({{L"operation",S(L"print_proof")},{L"epoch",N(epoch)},{L"settings",settings},{L"profile_id",form->profileId.empty()?JsonValue::CreateNullValue():S(form->profileId)}}).Stringify()));
            }catch(hresult_error const&){}};
            for(auto box:{form->profile,form->intent,form->simulation})box.SelectionChanged([edit](auto&&,auto&&){edit();});form->bpc.Click([edit](auto&&,auto&&){edit();});
        }
        printGate.IsEnabled(available);
        gamut.Content(box_value(copy(L"gamut_warning")));AutomationProperties::SetName(gamut,copy(L"gamut_warning"));
        gamut.IsChecked(flag(data->state,L"gamut_warning"));gamut.IsEnabled(flag(find(array(data->state,L"commands"),L"id",L"gamut_warning"),L"enabled"));
        auto text=str(object(data->model,L"windows_proof"),L"text");auto analysis=object(object(data->model,L"windows_display"),L"analysis");
        if(text.empty()&&flag(value,L"hdr"))text=!str(analysis,L"error").empty()?copy(L"sdr_unavailable"):flag(analysis,L"ready")?hstring():copy(L"preparing_sdr");
        status.Text(text);status.Visibility(text.empty()?Visibility::Collapsed:Visibility::Visible);
    });
    return root;
}
}
