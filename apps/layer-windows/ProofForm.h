#pragma once
#include "UiControls.h"

namespace CapyUi {
// Native controls project the shared print settings and option availability.
struct ProofFormView {
    std::shared_ptr<CapyLocalization> localization;
    explicit ProofFormView(std::shared_ptr<CapyLocalization> context):localization(std::move(context)){}
    StackPanel root;
    ComboBox profile,intent,simulation;
    CheckBox bpc;
    A profiles,intents,simulations,profileCaptions;
    hstring profileId;
    J draft,copy;

    J current() {
        auto result=J::Parse(draft.Stringify());
        auto selected=profiles.GetObjectAt(profile.SelectedIndex());
        profileId=str(selected,L"library");
        result.Insert(L"profile",selected);
        result.Insert(L"intent",intents.GetObjectAt(intent.SelectedIndex()).GetNamedValue(L"value"));
        result.Insert(L"simulation",simulations.GetObjectAt(simulation.SelectedIndex()).GetNamedValue(L"value"));
        result.Insert(L"bpc",B(bpc.IsChecked().Value()));
        return result;
    }
    void relocalize(std::shared_ptr<CapyLocalization> context,J const& details,hstring const& tag){
        localization=std::move(context);
        copy=object(object(details,L"form"),L"copy");root.Language(tag);
        for(auto const& [control,key]:std::initializer_list<std::pair<ComboBox,wchar_t const*>>{{profile,L"profile"},{simulation,L"simulation"},{intent,L"intent"}}){
            control.Header(box_value(str(copy,key)));AutomationProperties::SetName(control,str(copy,key));
        }
        bpc.Content(box_value(str(copy,L"black_point_compensation")));
        auto replace=[&](ComboBox const& control,A& values,wchar_t const* key){
            values=array(details,key);for(uint32_t i=0;i<std::min(values.Size(),control.Items().Size());++i)comboOptionText(control,i,str(values.GetObjectAt(i),L"label"));
        };
        replace(intent,intents,L"intents");replace(simulation,simulations,L"simulations");
        auto names=array(exportDraft(localization.get(),O({{L"copy",O({{L"profile_captions",profileCaptions}})}})),L"profile_names");
        for(uint32_t i=0;i<profiles.Size();++i){auto id=str(profiles.GetObjectAt(i),L"library");auto current=find(array(details,L"profiles"),L"id",id);comboOptionText(profile,i,id.empty()?names.GetStringAt(i):str(current,L"name"));}
    }
    void init(J const& details,J const& saved,hstring const& savedId) {
        draft=J::Parse((saved.Size()?saved:object(details,L"settings")).Stringify());
        auto form=object(details,L"form");copy=object(form,L"copy");auto original=object(draft,L"profile");
        auto document=object(form,L"document_profile");
        auto append=[&](J const& entry,hstring const& title){profiles.Append(entry);profileCaptions.Append(O({{L"type",S(L"profile")},{L"name",S(str(entry,L"name"))}}));comboOption(profile,title);};
        if(document.Size())append(document,str(document,L"name"));
        for(auto item:array(form,L"profiles"))append(item.GetObject(),str(item.GetObject(),L"name"));
        for(auto item:array(details,L"profiles")){
            auto entry=item.GetObject();if(entry.HasKey(L"issue"))continue;
            A bytes;auto value=O({{L"name",S(str(entry,L"name"))},{L"channels",S(str(entry,L"channels"))},
                {L"profile",O({{L"Icc",bytes}})},{L"library",S(str(entry,L"id"))}});
            append(value,str(entry,L"name"));
        }
        int selected=-1;
        for(uint32_t i=0;i<profiles.Size();++i){
            auto entry=profiles.GetObjectAt(i);
            if(!savedId.empty()?str(entry,L"library")==savedId:entry.GetNamedValue(L"profile").Stringify()==original.GetNamedValue(L"profile").Stringify()){selected=int(i);break;}
        }
        if(selected<0){append(original,str(original,L"name"));selected=int(profiles.Size())-1;}
        profile.SelectedIndex(selected);profileId=savedId;
        intents=array(details,L"intents");simulations=array(details,L"simulations");
        auto options=[&](ComboBox const& box,A const& choices,hstring const& value){
            for(uint32_t i=0;i<choices.Size();++i){auto choice=choices.GetObjectAt(i);comboOption(box,str(choice,L"label"));if(str(choice,L"value")==value)box.SelectedIndex(i);}
        };
        options(intent,intents,str(draft,L"intent"));options(simulation,simulations,str(draft,L"simulation"));
        root.Spacing(10);
        auto add=[&](ComboBox const& box,hstring const& label,hstring const& id){
            box.Header(box_value(label));box.HorizontalAlignment(HorizontalAlignment::Stretch);
            AutomationProperties::SetAutomationId(box,id);AutomationProperties::SetName(box,label);root.Children().Append(box);
        };
        add(profile,str(copy,L"profile"),L"proof-profile");add(simulation,str(copy,L"simulation"),L"proof-simulation");add(intent,str(copy,L"intent"),L"proof-intent");
        bpc.Content(box_value(str(copy,L"black_point_compensation")));bpc.IsChecked(flag(draft,L"bpc"));AutomationProperties::SetAutomationId(bpc,L"proof-bpc");
        bpc.IsEnabled(flag(intents.GetObjectAt(intent.SelectedIndex()),L"bpc_available"));root.Children().Append(bpc);
        relocalize(localization,details,root.Language());
        intent.SelectionChanged([box=intent,choices=intents,check=bpc](auto&&,auto&&){
            if(box.SelectedIndex()>=0)check.IsEnabled(flag(choices.GetObjectAt(box.SelectedIndex()),L"bpc_available"));
        });
    }
};
}
