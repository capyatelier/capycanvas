#pragma once
#include "UiControls.h"

namespace CapyUi {
// Native controls project the shared print settings and option availability.
struct ProofFormView {
    std::shared_ptr<WorkspaceData> data;NativeLocalization localization;std::array<TextBlock,3> captions{nullptr,nullptr,nullptr};
    explicit ProofFormView(std::shared_ptr<WorkspaceData> source):data(std::move(source)),localization(data->localization){}
    StackPanel root;
    ComboBox profile,intent,simulation;
    CheckBox bpc;
    A profiles,intents,simulations,profileCaptions;
    hstring profileId,captionKey;
    J draft,copy;

    J current() {
        auto result=J::Parse(draft.Stringify());
        auto selected=objectAt(profiles,profile.SelectedIndex());
        profileId=str(selected,L"library");
        result.Insert(L"profile",selected);
        result.Insert(L"intent",objectAt(intents,intent.SelectedIndex()).GetNamedValue(L"value",JsonValue::CreateNullValue()));
        result.Insert(L"simulation",objectAt(simulations,simulation.SelectedIndex()).GetNamedValue(L"value",JsonValue::CreateNullValue()));
        result.Insert(L"bpc",B(bpc.IsChecked().Value()));
        return result;
    }
    void relocalize(NativeLocalization context,J const& details,hstring const& tag){
        localization=std::move(context);
        copy=object(object(details,L"form"),L"copy");root.Language(tag);
        auto next=tag+to_hstring(data->textSize())+str(copy,L"profile")+str(copy,L"simulation")+str(copy,L"intent");
        if(next!=captionKey){captionKey=next;uint32_t index=0;double width=0;
            for(auto const& [control,key]:std::initializer_list<std::pair<ComboBox,wchar_t const*>>{{profile,L"profile"},{simulation,L"simulation"},{intent,L"intent"}}){
                auto text=str(copy,key);captions[index].Text(text);captions[index].Width(std::numeric_limits<double>::quiet_NaN());captions[index].Measure({INFINITY,INFINITY});width=std::max(width,double(captions[index++].DesiredSize().Width));AutomationProperties::SetName(control,text);
            }
            for(auto const& caption:captions)caption.Width(width);
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
            if(!savedId.empty()?str(entry,L"library")==savedId:entry.GetNamedValue(L"profile",JsonValue::CreateNullValue()).Stringify()==original.GetNamedValue(L"profile",JsonValue::CreateNullValue()).Stringify()){selected=int(i);break;}
        }
        if(selected<0){append(original,str(original,L"name"));selected=int(profiles.Size())-1;}
        selectIndex(profile,selected);profileId=savedId;
        intents=array(details,L"intents");simulations=array(details,L"simulations");
        auto options=[&](ComboBox const& box,A const& choices,hstring const& value){
            for(uint32_t i=0;i<choices.Size();++i){auto choice=choices.GetObjectAt(i);comboOption(box,str(choice,L"label"));if(str(choice,L"value")==value)selectIndex(box,i);}
        };
        options(intent,intents,str(draft,L"intent"));options(simulation,simulations,str(draft,L"simulation"));
        root.Spacing(data->panelMetric(L"control_spacing"));uint32_t index=0;
        auto add=[&](ComboBox const& box,hstring const& title,hstring const& id){
            choiceColors(data,box);box.MinHeight(data->controlHeight());box.MinWidth(0);box.FontSize(data->textSize());box.Padding({data->panelMetric(L"field_padding"),0,0,0});box.HorizontalAlignment(HorizontalAlignment::Stretch);
            Grid row;row.ColumnSpacing(data->panelMetric(L"control_spacing"));for(auto unit:{GridUnitType::Auto,GridUnitType::Star}){ColumnDefinition column;column.Width({1,unit});row.ColumnDefinitions().Append(column);}
            auto caption=label(data,title);twoLines(caption);caption.VerticalAlignment(VerticalAlignment::Center);captions[index++]=caption;AutomationProperties::SetLabeledBy(box,caption);row.Children().Append(caption);Grid::SetColumn(box,1);row.Children().Append(box);
            AutomationProperties::SetAutomationId(box,id);AutomationProperties::SetName(box,title);root.Children().Append(row);
        };
        add(profile,str(copy,L"profile"),L"proof-profile");add(simulation,str(copy,L"simulation"),L"proof-simulation");add(intent,str(copy,L"intent"),L"proof-intent");
        bpc=compactCheck(data,box_value(str(copy,L"black_point_compensation")));bpc.IsChecked(flag(draft,L"bpc"));AutomationProperties::SetAutomationId(bpc,L"proof-bpc");
        bpc.IsEnabled(flag(objectAt(intents,intent.SelectedIndex()),L"bpc_available"));root.Children().Append(bpc);
        relocalize(localization,details,root.Language());
        intent.SelectionChanged([box=intent,choices=intents,check=bpc](auto&&,auto&&){
            if(box.SelectedIndex()>=0)check.IsEnabled(flag(objectAt(choices,box.SelectedIndex()),L"bpc_available"));
        });
    }
};
}
