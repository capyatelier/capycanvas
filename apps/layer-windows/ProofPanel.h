#pragma once
#include "UiControls.h"

namespace CapyUi {
// Each retained panel/drawer owns native controls; Rust owns mode and edit policy.
inline StackPanel ProofPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    StackPanel root;root.Spacing(8);
    auto form=[data]{return object(data->model,L"windows_proof_form");};
    auto send=[data](J const& action){
        auto epoch=to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")));
        data->dispatch(O({{L"windows_proof_action",action},{L"windows_epoch",S(epoch)}}));
    };
    ComboBox mode;mode.HorizontalAlignment(HorizontalAlignment::Stretch);
    mode.Header(box_value(L"Proof"));AutomationProperties::SetAutomationId(mode,L"proof-panel-mode");
    for(auto name:{L"Off",L"SDR",L"Print"}){ComboBoxItem item;item.Content(box_value(name));mode.Items().Append(item);}
    mode.SelectionChanged([data,mode,send](auto&&,auto&&){if(!data->updating&&mode.SelectedIndex()>=0)
        send(O({{L"type",S(L"mode")},{L"mode",S(mode.SelectedIndex()==1?L"sdr":mode.SelectedIndex()==2?L"print":L"off")}}));});
    root.Children().Append(mode);
    StackPanel sdr;sdr.Spacing(6);ContentControl sdrGate;sdrGate.IsTabStop(false);sdrGate.Content(sdr);root.Children().Append(sdrGate);
    auto initial=form();
    NumberPresentation presentation;presentation.identity=[form]{return array(form(),L"identity").Stringify();};
    for(auto item:array(initial,L"numbers")){
        auto control=item.GetObject();auto key=str(control,L"key");auto spec=J::Parse(object(control,L"numeric").Stringify());
        spec.Insert(L"kind",S(L"number"));
        sdr.Children().Append(number(data,str(control,L"label"),spec,
            [form,key]{return num(object(form(),L"rendition"),key.c_str());},
            [data,form,send,key](double value){if(data->updating)return;auto recipe=J::Parse(object(form(),L"rendition").Stringify());recipe.Insert(key,N(value));
                for(auto phase:{L"down",L"up"})send(O({{L"type",S(L"rendition")},{L"phase",S(phase)},{L"recipe",recipe}}));
            },bindings,nullptr,false,L"proof-panel-"+key,false,presentation));
    }
    auto axes=array(object(initial,L"pad"),L"axes");
    for(uint32_t index=0;index<axes.Size();++index){
        auto axis=axes.GetObjectAt(index);auto spec=J::Parse(object(axis,L"numeric").Stringify());spec.Insert(L"kind",S(L"number"));
        sdr.Children().Append(number(data,str(axis,L"label"),spec,
            [form,index]{return array(form(),L"pad_values").GetNumberAt(index);},
            [data,form,send,index](double value){if(data->updating)return;auto values=A::Parse(array(form(),L"pad_values").Stringify());values.SetAt(index,N(value));
                for(auto phase:{L"down",L"up"})send(O({{L"type",S(L"pad")},{L"phase",S(phase)},{L"values",values}}));
            },bindings,nullptr,false,L"proof-panel-"+str(axis,L"key"),false,presentation));
    }
    auto appearance=button(data,L"SDR Appearance…",[data]{data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"sdr_rendition")}}));});
    appearance.Padding({6,6,6,6});sdr.Children().Append(appearance);
    auto profile=label(data,L"");profile.TextWrapping(TextWrapping::Wrap);root.Children().Append(profile);
    auto setup=button(data,L"Print setup…",[data]{data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"soft_proof_setup")}}));});
    AutomationProperties::SetAutomationId(setup,L"proof-panel-setup");setup.Padding({6,6,6,6});root.Children().Append(setup);
    CheckBox gamut;gamut.Content(box_value(L"Gamut warning"));AutomationProperties::SetAutomationId(gamut,L"proof-panel-gamut");
    gamut.Click([data](auto&&,auto&&){if(!data->updating)data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"gamut_warning")}}));});root.Children().Append(gamut);
    auto status=label(data,L"");status.TextWrapping(TextWrapping::Wrap);root.Children().Append(status);
    bindings.emplace_back([data,form,mode,sdrGate,profile,setup,gamut,status]{
        auto value=form();mode.Items().GetAt(1).as<ComboBoxItem>().IsEnabled(flag(value,L"hdr"));auto selected=str(value,L"mode");mode.SelectedIndex(selected==L"sdr"?1:selected==L"print"?2:0);
        bool available=!flag(object(data->state,L"document_file"),L"busy")&&!flag(data->model,L"windows_rendering_suspended");
        mode.IsEnabled(available);sdrGate.IsEnabled(available);sdrGate.Visibility(flag(value,L"hdr")?Visibility::Visible:Visibility::Collapsed);
        auto recipe=object(value,L"document_profile");profile.Text(recipe.Size()?L"Profile: "+str(recipe,L"name"):L"Choose a print profile");
        setup.IsEnabled(flag(find(array(data->state,L"commands"),L"id",L"soft_proof_setup"),L"enabled"));
        gamut.IsChecked(flag(data->state,L"gamut_warning"));gamut.IsEnabled(flag(find(array(data->state,L"commands"),L"id",L"gamut_warning"),L"enabled"));
        auto text=str(object(data->model,L"windows_proof"),L"text");auto analysis=object(object(data->model,L"windows_display"),L"analysis");
        if(text.empty()&&flag(value,L"hdr"))text=!str(analysis,L"error").empty()?L"Local SDR unavailable":flag(analysis,L"ready")?L"":L"Preparing SDR…";
        status.Text(text);
    });
    return root;
}
}
