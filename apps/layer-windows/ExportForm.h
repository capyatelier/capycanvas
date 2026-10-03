#pragma once
#include "UiControls.h"
#include <array>
namespace CapyUi {
struct ExportFormView : std::enable_shared_from_this<ExportFormView> {
    std::shared_ptr<CapyLocalization> localization;
    explicit ExportFormView(std::shared_ptr<CapyLocalization> context):localization(std::move(context)){}
    StackPanel root;
    ComboBox format,profile,depth,background,dither,intent,resolution,metadata;
    NumberBox quality,width,height,ppi;
    CheckBox resize,enlarge,removeLocation;
    TextBlock validation,metadataNote;
    A extent;
    J recipe,draft,color;
    A profiles;
    uint32_t formProfiles=0;int recipeProfile=-1;
    hstring profileId,metadataChoices;
    bool updating=false,photoMetadata=false;
    J copy;V validationReason=JsonValue::CreateNullValue();
    hstring text(wchar_t const* key) const {return str(copy,key);}
    void choices(ComboBox const& box,A const& values,hstring const& selected){box.Items().Clear();for(uint32_t i=0;i<values.Size();++i){auto choice=values.GetObjectAt(i);comboOption(box,str(choice,L"label"));if(str(choice,L"value")==selected)box.SelectedIndex(i);}}
    void normalize(J action){
        draft=exportDraft(localization.get(),O({{L"recipe",recipe},{L"action",action},{L"color",color}}));
        if(draft.HasKey(L"error"))throw hresult_invalid_argument(str(draft,L"error"));recipe=object(draft,L"recipe");
        updating=true;choices(format,array(object(draft,L"choices"),L"formats"),str(recipe,L"format"));choices(depth,array(object(draft,L"choices"),L"depths"),str(recipe,L"depth"));
        choices(background,array(object(draft,L"choices"),L"backgrounds"),str(recipe,L"background"));choices(dither,array(object(draft,L"choices"),L"dithers"),str(object(recipe,L"encoding"),L"dither"));auto hdr=flag(draft,L"hdr");if(hdr){profileId=L"";auto wanted=object(object(recipe,L"profile"),L"profile").Stringify();for(uint32_t i=0;i<profiles.Size();++i)if(object(profiles.GetObjectAt(i),L"profile").Stringify()==wanted)profile.SelectedIndex(i);}
        profile.IsEnabled(!hdr);intent.IsEnabled(!hdr);quality.IsEnabled(str(recipe,L"format")==L"Jpeg"||str(recipe,L"format")==L"JpegHdr"||str(recipe,L"format")==L"JpegHdrMapped"||str(recipe,L"format")==L"AvifHdr"||str(recipe,L"format")==L"AvifHdrMapped");
        presentMetadata();updating=false;
    }
    void presentMetadata(){
        auto view=object(draft,L"metadata");auto choices=array(view,L"choices");auto kept=object(recipe,L"metadata");
        metadata.Header(box_value(str(view,L"label")));AutomationProperties::SetName(metadata,str(view,L"label"));
        A identities;for(auto choice:choices)identities.Append(choice.GetObject().GetNamedValue(L"value"));
        if(auto key=identities.Stringify();key!=metadataChoices){
            metadataChoices=key;metadata.Items().Clear();
            for(auto choice:choices)comboOption(metadata,str(choice.GetObject(),L"label"));
        }
        for(uint32_t i=0;i<choices.Size();++i)comboOptionText(metadata,i,str(choices.GetObjectAt(i),L"label"));
        for(uint32_t i=0;i<choices.Size();++i)if(str(choices.GetObjectAt(i),L"value")==str(kept,L"keep")&&metadata.SelectedIndex()!=int32_t(i))metadata.SelectedIndex(i);
        removeLocation.Content(box_value(str(view,L"remove_location")));removeLocation.IsChecked(flag(kept,L"remove_location"));
        auto note=str(view,L"note");metadataNote.Text(note);
        metadata.Visibility(photoMetadata&&flag(view,L"available")?Visibility::Visible:Visibility::Collapsed);
        removeLocation.Visibility(photoMetadata&&flag(view,L"location")?Visibility::Visible:Visibility::Collapsed);
        metadataNote.Visibility(photoMetadata&&!note.empty()?Visibility::Visible:Visibility::Collapsed);
    }
    void chooseMetadata(){
        if(updating||metadata.SelectedIndex()<0)return;
        auto choice=array(object(draft,L"metadata"),L"choices").GetObjectAt(metadata.SelectedIndex());
        normalize(O({{L"type",S(L"metadata")},{L"value",O({{L"keep",choice.GetNamedValue(L"value")},{L"remove_location",B(removeLocation.IsChecked().Value())}})}}));
    }
    J current(){
        auto value=J::Parse(recipe.Stringify());value.Insert(L"jpeg_quality",N(quality.Value()));
        if(resize.IsChecked().Value()){A bounds;bounds.Append(N(width.Value()));bounds.Append(N(height.Value()));value.Insert(L"size",O({{L"Fit",O({{L"bounds",bounds},{L"enlarge",B(enlarge.IsChecked().Value())}})}}));}
        else value.Insert(L"size",S(L"Original"));
        value.Insert(L"resolution",resolution.SelectedIndex()==2?V(O({{L"Ppi",N(ppi.Value())}})):S(resolution.SelectedIndex()==1?L"Omit":L"Master"));
        auto encoding=object(value,L"encoding");auto conversion=object(encoding,L"conversion");
        conversion.Insert(L"intent",S(std::array<hstring,4>{L"RelativeColorimetric",L"Perceptual",L"Saturation",L"AbsoluteColorimetric"}[intent.SelectedIndex()]));
        conversion.Insert(L"black_point_compensation",B(false));
        if(str(value,L"format")==L"Png"||str(value,L"format")==L"Tiff"||str(value,L"format")==L"Jpeg"||str(value,L"format")==L"Webp"){encoding.Insert(L"conversion",conversion);value.Insert(L"encoding",encoding);}
        auto result=exportDraft(localization.get(),O({{L"recipe",value},{L"color",color},{L"action",O({{L"type",S(L"refresh")}})},{L"validate",B(true)},{L"extent",extent}}));
        if(result.HasKey(L"error")){validationReason=result.GetNamedValue(L"error_reason",JsonValue::CreateNullValue());throw hresult_invalid_argument(str(result,L"error"));}return object(result,L"recipe");
    }
    void relocalize(std::shared_ptr<CapyLocalization> context,J const& details,hstring const& tag){
        localization=std::move(context);copy=object(object(details,L"form"),L"copy");root.Language(tag);
        for(auto const& [control,key]:std::initializer_list<std::pair<ComboBox,wchar_t const*>>{{format,L"format"},{profile,L"profile"},{depth,L"depth"},{background,L"background"},{dither,L"dither"},{intent,L"intent"},{resolution,L"resolution"}}){
            control.Header(box_value(text(key)));AutomationProperties::SetName(control,text(key));
        }
        for(auto const& [control,key]:std::initializer_list<std::pair<NumberBox,wchar_t const*>>{{quality,L"quality"},{width,L"maximum_width"},{height,L"maximum_height"},{ppi,L"ppi"}}){
            control.Header(box_value(text(key)));AutomationProperties::SetName(control,text(key));control.Language(tag);
        }
        resize.Content(box_value(text(L"fit_bounds")));enlarge.Content(box_value(text(L"enlarge")));
        updating=true;struct Reset{bool& updating;~Reset(){updating=false;}}reset{updating};
        auto selected=profile.SelectedIndex();
        auto names=array(object(details,L"form"),L"profile_names");
        for(uint32_t i=0;i<profiles.Size();++i){
            if(i<formProfiles){comboOptionText(profile,i,names.GetStringAt(i));continue;}
            if(int(i)==recipeProfile){comboOptionText(profile,i,str(object(details,L"form"),L"recipe_profile_name"));continue;}
            auto current=find(array(details,L"profiles"),L"id",str(profiles.GetObjectAt(i),L"library"));if(current.Size())comboOptionText(profile,i,str(current,L"name"));
        }profile.SelectedIndex(selected);
        uint32_t i=0;for(auto key:{L"relative",L"perceptual",L"saturation",L"absolute"})comboOptionText(intent,i++,text(key));
        i=0;for(auto key:{L"keep_resolution",L"omit",L"ppi"})comboOptionText(resolution,i++,text(key));
        auto projected=exportDraft(localization.get(),O({{L"copy",O({{L"choices",object(draft,L"choices")},{L"error_reason",validationReason},{L"metadata",O({{L"format",recipe.GetNamedValue(L"format")},{L"keep",object(recipe,L"metadata").GetNamedValue(L"keep")}})}})}}));
        draft.Insert(L"choices",object(projected,L"choices"));draft.Insert(L"metadata",object(projected,L"metadata"));
        for(auto const& [control,key]:std::initializer_list<std::pair<ComboBox,wchar_t const*>>{{format,L"formats"},{depth,L"depths"},{background,L"backgrounds"},{dither,L"dithers"}}){
            auto values=array(object(draft,L"choices"),key);
            for(uint32_t optionIndex=0;optionIndex<std::min(values.Size(),control.Items().Size());++optionIndex)comboOptionText(control,optionIndex,str(values.GetObjectAt(optionIndex),L"label"));
        }
        presentMetadata();
        if(!validation.Text().empty()&&validationReason.ValueType()!=JsonValueType::Null)validation.Text(str(projected,L"error"));
    }
    void init(J const& details){
        color=object(details,L"color");extent=array(details,L"extent");root.Spacing(8);recipe=J::Parse(object(details,L"recipe").Stringify());auto form=object(details,L"form");copy=object(form,L"copy");profiles=A::Parse(array(form,L"profiles").Stringify());
        auto original=object(recipe,L"profile");formProfiles=profiles.Size();bool contains=false;for(auto item:profiles)contains|=item.Stringify()==original.Stringify();if(!contains){recipeProfile=int(profiles.Size());profiles.Append(original);}
        for(auto item:array(details,L"profiles")){auto entry=item.GetObject();if(entry.HasKey(L"issue"))continue;A empty;profiles.Append(O({{L"name",S(str(entry,L"name"))},{L"channels",S(str(entry,L"channels"))},{L"profile",O({{L"Icc",empty}})},{L"library",S(str(entry,L"id"))}}));}
        auto add=[&](ComboBox const& box,hstring const& label){box.Header(box_value(label));box.HorizontalAlignment(HorizontalAlignment::Stretch);root.Children().Append(box);};
        AutomationProperties::SetAutomationId(profile,L"export-profile");AutomationProperties::SetAutomationId(format,L"export-format");AutomationProperties::SetAutomationId(background,L"export-background");add(format,text(L"format"));add(profile,text(L"profile"));add(depth,text(L"depth"));add(background,text(L"background"));add(dither,text(L"dither"));add(intent,text(L"intent"));
        auto names=array(form,L"profile_names");for(uint32_t i=0;i<profiles.Size();++i)comboOption(profile,i<formProfiles?names.GetStringAt(i):int(i)==recipeProfile?str(form,L"recipe_profile_name"):str(profiles.GetObjectAt(i),L"name"));
        for(uint32_t i=0;i<profiles.Size();++i)if(profiles.GetObjectAt(i).Stringify()==original.Stringify())profile.SelectedIndex(i);
        for(auto value:{text(L"relative"),text(L"perceptual"),text(L"saturation"),text(L"absolute")})comboOption(intent,value);
        auto intentName=str(object(object(recipe,L"encoding"),L"conversion"),L"intent");intent.SelectedIndex(intentName==L"Perceptual"?1:intentName==L"Saturation"?2:intentName==L"AbsoluteColorimetric"?3:0);
        for(auto const& [control,id]:std::initializer_list<std::pair<NumberBox,wchar_t const*>>{{quality,L"export-quality"},{width,L"export-width"},{height,L"export-height"},{ppi,L"export-ppi"}})AutomationProperties::SetAutomationId(control,id);
        quality.Header(box_value(text(L"quality")));quality.Value(num(recipe,L"jpeg_quality",90));root.Children().Append(quality);
        resize.Content(box_value(text(L"fit_bounds")));root.Children().Append(resize);auto fit=object(object(recipe,L"size"),L"Fit");resize.IsChecked(fit.Size()!=0);
        auto bounds=fit.Size()?array(fit,L"bounds"):array(details,L"extent");width.Header(box_value(text(L"maximum_width")));height.Header(box_value(text(L"maximum_height")));width.Value(bounds.GetNumberAt(0));height.Value(bounds.GetNumberAt(1));root.Children().Append(width);root.Children().Append(height);
        enlarge.Content(box_value(text(L"enlarge")));enlarge.IsChecked(flag(fit,L"enlarge"));root.Children().Append(enlarge);
        add(resolution,text(L"resolution"));for(auto value:{text(L"keep_resolution"),text(L"omit"),text(L"ppi")})comboOption(resolution,value);
        auto density=recipe.GetNamedValue(L"resolution");resolution.SelectedIndex(density.ValueType()==JsonValueType::Object?2:density.GetString()==L"Omit"?1:0);
        ppi.Header(box_value(text(L"ppi")));ppi.Value(density.ValueType()==JsonValueType::Object?num(density.GetObject(),L"Ppi",300):300);root.Children().Append(ppi);
        photoMetadata=flag(form,L"metadata");metadata.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetAutomationId(metadata,L"export-metadata");AutomationProperties::SetAutomationId(removeLocation,L"export-remove-location");
        AutomationProperties::SetAutomationId(metadataNote,L"export-metadata-note");metadataNote.TextWrapping(TextWrapping::Wrap);metadataNote.Opacity(.72);
        for(UIElement control:{UIElement(metadata),UIElement(removeLocation),UIElement(metadataNote)})root.Children().Append(control);
        AutomationProperties::SetAutomationId(validation,L"export-validation");validation.TextWrapping(TextWrapping::Wrap);root.Children().Append(validation);
        normalize(O({{L"type",S(L"refresh")}}));relocalize(localization,details,root.Language());auto weak=weak_from_this();
        auto bind=[&](ComboBox const& box,wchar_t const* field,wchar_t const* op){box.SelectionChanged([weak,box,field,op](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&box.SelectedIndex()>=0){
            V value=array(object(self->draft,L"choices"),field).GetObjectAt(box.SelectedIndex()).GetNamedValue(L"value");
            if(std::wstring_view(op)==L"encoding"){auto encoding=J::Parse(object(self->recipe,L"encoding").Stringify());encoding.Insert(L"dither",value);value=encoding;}
            self->normalize(O({{L"type",S(op)},{L"value",value}}));
        }});};
        bind(format,L"formats",L"format");bind(depth,L"depths",L"depth");bind(background,L"backgrounds",L"background");bind(dither,L"dithers",L"encoding");
        profile.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&self->profile.SelectedIndex()>=0){auto value=self->profiles.GetObjectAt(self->profile.SelectedIndex());self->profileId=str(value,L"library");self->normalize(O({{L"type",S(L"profile")},{L"value",value}}));}});
        metadata.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->chooseMetadata();});
        removeLocation.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->chooseMetadata();});
    }
};
}
