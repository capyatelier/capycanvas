#pragma once
#include "UiControls.h"
#include "ProofDial.h"

namespace CapyUi {
// Each retained panel/drawer owns native controls; Rust owns mode and edit policy.
inline StackPanel ProofPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    StackPanel root;root.Spacing(8);
    auto form=[data]{return object(data->model,L"windows_proof_form");};
    auto send=[data](J const& action){
        auto epoch=to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")));
        data->dispatch(O({{L"windows_proof_action",action},{L"windows_epoch",S(epoch)}}));
    };
    Grid mode;mode.ColumnSpacing(2);mode.Margin({0,0,0,6});
    AutomationProperties::SetAutomationId(mode,L"proof-panel-mode");AutomationProperties::SetName(mode,L"Proof mode");
    std::vector<std::pair<Button,hstring>> modes;
    for(auto [value,name]:{std::pair{L"off",L"Off"},std::pair{L"sdr",L"SDR"},std::pair{L"print",L"Print"}}){
        ColumnDefinition column;column.Width({1,GridUnitType::Star});mode.ColumnDefinitions().Append(column);
        auto choice=button(data,name,[data,send,value=hstring(value)]{if(!data->updating)send(O({{L"type",S(L"mode")},{L"mode",S(value)}}));});
        choice.HorizontalAlignment(HorizontalAlignment::Stretch);choice.MinHeight(34);choice.Padding({6,0,6,0});
        AutomationProperties::SetAutomationId(choice,L"proof-panel-mode-"+hstring(value));
        Grid::SetColumn(choice,int32_t(modes.size()));mode.Children().Append(choice);modes.emplace_back(choice,value);
    }
    root.Children().Append(mode);
    StackPanel sdr;sdr.Spacing(6);ContentControl sdrGate;sdrGate.IsTabStop(false);sdrGate.Content(sdr);sdrGate.Visibility(Visibility::Collapsed);root.Children().Append(sdrGate);
    sdr.Children().Append(ProofDialControl(data,bindings));
    auto initial=form();
    NumberPresentation presentation;presentation.identity=[form]{return array(form(),L"identity").Stringify();};
    for(auto item:array(initial,L"numbers")){
        auto control=item.GetObject();auto key=str(control,L"key");auto spec=J::Parse(object(control,L"numeric").Stringify());
        spec.Insert(L"kind",S(L"number"));
        sdr.Children().Append(number(data,str(control,L"label"),spec,
            [form,key]{return num(object(form(),L"rendition"),key.c_str());},
            [data,send,key](double value){if(!data->updating)send(O({{L"type",S(L"number")},{L"key",S(key)},{L"value",N(value)}}));},bindings,nullptr,false,L"proof-panel-"+key,false,presentation));
    }
    auto axes=array(object(initial,L"pad"),L"axes");
    for(uint32_t index=0;index<axes.Size();++index){
        auto axis=axes.GetObjectAt(index);auto spec=J::Parse(object(axis,L"numeric").Stringify());spec.Insert(L"kind",S(L"number"));
        sdr.Children().Append(number(data,str(axis,L"label"),spec,
            [form,index]{return array(form(),L"pad_values").GetNumberAt(index);},
            [data,send,key=str(axis,L"key")](double value){if(!data->updating)send(O({{L"type",S(L"number")},{L"key",S(key)},{L"value",N(value)}}));},bindings,nullptr,false,L"proof-panel-"+str(axis,L"key"),false,presentation));
    }
    StackPanel print;print.Spacing(8);print.Visibility(Visibility::Collapsed);root.Children().Append(print);
    auto profile=label(data,L"");profile.TextWrapping(TextWrapping::Wrap);print.Children().Append(profile);
    auto setup=button(data,L"Print setup…",[data]{data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"soft_proof_setup")}}));});
    AutomationProperties::SetAutomationId(setup,L"proof-panel-setup");setup.Padding({6,6,6,6});print.Children().Append(setup);
    CheckBox gamut;gamut.Content(box_value(L"Gamut warning"));AutomationProperties::SetAutomationId(gamut,L"proof-panel-gamut");
    gamut.Click([data](auto&&,auto&&){if(!data->updating)data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"gamut_warning")}}));});print.Children().Append(gamut);
    auto status=label(data,L"");status.TextWrapping(TextWrapping::Wrap);status.Visibility(Visibility::Collapsed);root.Children().Append(status);
    bindings.emplace_back([data,form,mode,modes,sdrGate,print,profile,setup,gamut,status]{
        auto value=form();auto selected=str(value,L"mode");bool hdr=flag(value,L"hdr");
        bool available=!flag(object(data->state,L"document_file"),L"busy")&&!flag(data->model,L"windows_rendering_suspended");
        mode.ColumnDefinitions().GetAt(1).Width(hdr?GridLength{1,GridUnitType::Star}:GridLength{0,GridUnitType::Pixel});
        for(auto const& [choice,id]:modes){
            bool pressed=id==selected;
            choice.Visibility(id!=L"sdr"||hdr?Visibility::Visible:Visibility::Collapsed);choice.IsEnabled(available);
            choice.Background(pressed?Brush(data->tint(L"text",31)):Brush(clear()));
            AutomationProperties::SetItemStatus(choice,pressed?L"Selected":L"");
        }
        sdrGate.IsEnabled(available);sdrGate.Visibility(hdr&&selected==L"sdr"?Visibility::Visible:Visibility::Collapsed);
        print.Visibility(selected==L"print"?Visibility::Visible:Visibility::Collapsed);
        auto recipe=object(value,L"document_profile");profile.Text(recipe.Size()?L"Profile: "+str(recipe,L"name"):L"Choose a print profile");
        setup.IsEnabled(flag(find(array(data->state,L"commands"),L"id",L"soft_proof_setup"),L"enabled"));
        gamut.IsChecked(flag(data->state,L"gamut_warning"));gamut.IsEnabled(flag(find(array(data->state,L"commands"),L"id",L"gamut_warning"),L"enabled"));
        auto text=str(object(data->model,L"windows_proof"),L"text");auto analysis=object(object(data->model,L"windows_display"),L"analysis");
        if(text.empty()&&flag(value,L"hdr"))text=!str(analysis,L"error").empty()?L"Local SDR unavailable":flag(analysis,L"ready")?L"":L"Preparing SDR…";
        status.Text(text);status.Visibility(text.empty()?Visibility::Collapsed:Visibility::Visible);
    });
    return root;
}
}
