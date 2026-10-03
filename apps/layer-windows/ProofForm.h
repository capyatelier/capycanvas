#pragma once
#include "UiControls.h"

namespace CapyUi {
// Native controls project the shared print settings and option availability.
struct ProofFormView {
    StackPanel root;
    ComboBox profile,intent,simulation;
    CheckBox bpc;
    A profiles,intents,simulations;
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
    void relocalize(J const& details,hstring const& tag){
        copy=object(object(details,L"form"),L"copy");root.Language(tag);
        for(auto const& [control,key]:std::initializer_list<std::pair<ComboBox,wchar_t const*>>{{profile,L"profile"},{simulation,L"simulation"},{intent,L"intent"}}){
            control.Header(box_value(str(copy,key)));AutomationProperties::SetName(control,str(copy,key));
        }
        bpc.Content(box_value(str(copy,L"black_point_compensation")));
        auto replace=[&](ComboBox const& control,A& values,wchar_t const* key){
            values=array(details,key);for(uint32_t i=0;i<std::min(values.Size(),control.Items().Size());++i)control.Items().SetAt(i,box_value(str(values.GetObjectAt(i),L"label")));
        };
        replace(intent,intents,L"intents");replace(simulation,simulations,L"simulations");
        auto selected=profile.SelectedIndex();for(uint32_t i=0;i<profiles.Size();++i){auto entry=profiles.GetObjectAt(i);auto id=str(entry,L"library");if(id.empty())continue;auto current=find(array(details,L"profiles"),L"id",id);if(current.Size()){entry.Insert(L"name",S(str(current,L"name")));entry.Insert(L"channels",S(str(current,L"channels")));profile.Items().SetAt(i,box_value(str(current,L"name")));}}profile.SelectedIndex(selected);
    }
    void init(J const& details,J const& saved,hstring const& savedId) {
        draft=J::Parse((saved.Size()?saved:object(details,L"settings")).Stringify());
        auto form=object(details,L"form");copy=object(form,L"copy");auto original=object(draft,L"profile");
        auto document=object(form,L"document_profile");
        auto append=[&](J const& entry,hstring const& label){profiles.Append(entry);profile.Items().Append(box_value(label));};
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
            for(uint32_t i=0;i<choices.Size();++i){auto choice=choices.GetObjectAt(i);box.Items().Append(box_value(str(choice,L"label")));if(str(choice,L"value")==value)box.SelectedIndex(i);}
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
        intent.SelectionChanged([box=intent,choices=intents,check=bpc](auto&&,auto&&){
            if(box.SelectedIndex()>=0)check.IsEnabled(flag(choices.GetObjectAt(box.SelectedIndex()),L"bpc_available"));
        });
    }
};
}
