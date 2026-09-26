#pragma once
#include "UiControls.h"
#include <array>
namespace CapyUi {
// Controls retain a draft; allowed combinations are returned by ExportRecipe::draft.
struct ExportFormView : std::enable_shared_from_this<ExportFormView> {
    StackPanel root;
    ComboBox format,profile,depth,background,dither,intent,resolution;
    NumberBox quality,width,height,ppi;
    CheckBox resize,enlarge;
    TextBlock validation;
    A extent;
    J recipe,draft,color;
    A profiles;
    hstring profileId;
    bool updating=false;
    static hstring name(hstring const& value){
        if(value==L"Png")return L"PNG";if(value==L"Tiff")return L"TIFF";if(value==L"Jpeg")return L"JPEG";
        if(value==L"JpegHdr")return L"HDR JPEG · gain map";if(value==L"JpegHdrMapped")return L"HDR JPEG · clip to gain-map range";
        if(value==L"AvifHdr")return L"HDR AVIF · gain map";if(value==L"AvifHdrMapped")return L"HDR AVIF · clip to gain-map range";
        if(value==L"PngHdr")return L"HDR PNG · BT.2020 PQ";if(value==L"PngHdrMapped")return L"HDR PNG · clip to PQ range";if(value==L"Exr")return L"OpenEXR · 32-bit float";if(value==L"F32")return L"32-bit float";
        if(value==L"U8")return L"8-bit";if(value==L"U16")return L"16-bit";if(value==L"Preserve")return L"Preserve transparency";
        if(value==L"Stochastic8")return L"Dither to 8-bit";if(value==L"None")return L"No dithering";return value;
    }
    void choices(ComboBox const& box,A const& values,hstring const& selected){box.Items().Clear();for(uint32_t i=0;i<values.Size();++i){auto value=values.GetStringAt(i);box.Items().Append(box_value(name(value)));if(value==selected)box.SelectedIndex(i);}}
    void normalize(J action){
        auto input=to_string(O({{L"recipe",recipe},{L"action",action},{L"color",color}}).Stringify());
        std::unique_ptr<char,decltype(&capy_string_free)> raw(capy_export_draft(input.c_str()),capy_string_free);
        if(!raw)throw hresult_error(E_FAIL,L"Export form unavailable");draft=J::Parse(to_hstring(raw.get()));
        if(draft.HasKey(L"error"))throw hresult_invalid_argument(str(draft,L"error"));recipe=object(draft,L"recipe");
        updating=true;choices(format,array(draft,L"formats"),str(recipe,L"format"));choices(depth,array(draft,L"depths"),str(recipe,L"depth"));
        choices(background,array(draft,L"backgrounds"),str(recipe,L"background"));choices(dither,array(draft,L"dithers"),str(object(recipe,L"encoding"),L"dither"));auto hdr=str(recipe,L"format")!=L"Png"&&str(recipe,L"format")!=L"Tiff"&&str(recipe,L"format")!=L"Jpeg";if(hdr){profileId=L"";auto wanted=object(object(recipe,L"profile"),L"profile").Stringify();for(uint32_t i=0;i<profiles.Size();++i)if(object(profiles.GetObjectAt(i),L"profile").Stringify()==wanted)profile.SelectedIndex(i);}
        profile.IsEnabled(!hdr);intent.IsEnabled(!hdr);quality.IsEnabled(str(recipe,L"format")==L"Jpeg"||str(recipe,L"format")==L"JpegHdr"||str(recipe,L"format")==L"JpegHdrMapped"||str(recipe,L"format")==L"AvifHdr"||str(recipe,L"format")==L"AvifHdrMapped");updating=false;
    }
    J current(){
        auto value=J::Parse(recipe.Stringify());value.Insert(L"jpeg_quality",N(quality.Value()));
        if(resize.IsChecked().Value()){A bounds;bounds.Append(N(width.Value()));bounds.Append(N(height.Value()));value.Insert(L"size",O({{L"Fit",O({{L"bounds",bounds},{L"enlarge",B(enlarge.IsChecked().Value())}})}}));}
        else value.Insert(L"size",S(L"Original"));
        value.Insert(L"resolution",resolution.SelectedIndex()==2?V(O({{L"Ppi",N(ppi.Value())}})):S(resolution.SelectedIndex()==1?L"Omit":L"Master"));
        auto encoding=object(value,L"encoding");auto conversion=object(encoding,L"conversion");
        conversion.Insert(L"intent",S(std::array<hstring,4>{L"RelativeColorimetric",L"Perceptual",L"Saturation",L"AbsoluteColorimetric"}[intent.SelectedIndex()]));
        conversion.Insert(L"black_point_compensation",B(false));
        if(str(value,L"format")==L"Png"||str(value,L"format")==L"Tiff"||str(value,L"format")==L"Jpeg"){encoding.Insert(L"conversion",conversion);value.Insert(L"encoding",encoding);}
        auto input=to_string(O({{L"recipe",value},{L"color",color},{L"action",O({{L"type",S(L"refresh")}})},{L"validate",B(true)},{L"extent",extent}}).Stringify());
        std::unique_ptr<char,decltype(&capy_string_free)> raw(capy_export_draft(input.c_str()),capy_string_free);
        if(!raw)throw hresult_error(E_FAIL,L"Export form unavailable");auto result=J::Parse(to_hstring(raw.get()));
        if(result.HasKey(L"error"))throw hresult_invalid_argument(str(result,L"error"));return object(result,L"recipe");
    }
    void init(J const& details){
        color=object(details,L"color");extent=array(details,L"extent");root.Spacing(8);recipe=J::Parse(object(details,L"recipe").Stringify());auto form=object(details,L"form");profiles=A::Parse(array(form,L"profiles").Stringify());
        auto original=object(recipe,L"profile");bool contains=false;for(auto item:profiles)contains|=item.Stringify()==original.Stringify();if(!contains)profiles.Append(original);
        for(auto item:array(details,L"profiles")){auto entry=item.GetObject();if(entry.HasKey(L"issue"))continue;A empty;profiles.Append(O({{L"name",S(str(entry,L"name"))},{L"channels",S(str(entry,L"channels"))},{L"profile",O({{L"Icc",empty}})},{L"library",S(str(entry,L"id"))}}));}
        auto add=[&](ComboBox const& box,hstring const& label){box.Header(box_value(label));box.HorizontalAlignment(HorizontalAlignment::Stretch);root.Children().Append(box);};
        AutomationProperties::SetAutomationId(format,L"export-format");AutomationProperties::SetAutomationId(background,L"export-background");add(format,L"File format");add(profile,L"Output profile");add(depth,L"Precision");add(background,L"Background");add(dither,L"Dithering");add(intent,L"Rendering intent");
        for(auto item:profiles)profile.Items().Append(box_value(str(item.GetObject(),L"name")));
        for(uint32_t i=0;i<profiles.Size();++i)if(profiles.GetObjectAt(i).Stringify()==original.Stringify())profile.SelectedIndex(i);
        for(auto value:{L"Relative colorimetric",L"Perceptual",L"Saturation",L"Absolute colorimetric"})intent.Items().Append(box_value(value));
        auto intentName=str(object(object(recipe,L"encoding"),L"conversion"),L"intent");intent.SelectedIndex(intentName==L"Perceptual"?1:intentName==L"Saturation"?2:intentName==L"AbsoluteColorimetric"?3:0);
        quality.Header(box_value(L"Compression quality (1–100)"));quality.Value(num(recipe,L"jpeg_quality",90));root.Children().Append(quality);
        resize.Content(box_value(L"Fit within a pixel box"));root.Children().Append(resize);auto fit=object(object(recipe,L"size"),L"Fit");resize.IsChecked(fit.Size()!=0);
        auto bounds=fit.Size()?array(fit,L"bounds"):array(details,L"extent");width.Header(box_value(L"Maximum width"));height.Header(box_value(L"Maximum height"));width.Value(bounds.GetNumberAt(0));height.Value(bounds.GetNumberAt(1));root.Children().Append(width);root.Children().Append(height);
        enlarge.Content(box_value(L"Allow enlargement"));enlarge.IsChecked(flag(fit,L"enlarge"));root.Children().Append(enlarge);
        add(resolution,L"Resolution metadata");for(auto value:{L"Keep master",L"Omit",L"Set pixels per inch"})resolution.Items().Append(box_value(value));
        auto density=recipe.GetNamedValue(L"resolution");resolution.SelectedIndex(density.ValueType()==JsonValueType::Object?2:density.GetString()==L"Omit"?1:0);
        ppi.Header(box_value(L"Pixels per inch"));ppi.Value(density.ValueType()==JsonValueType::Object?num(density.GetObject(),L"Ppi",300):300);root.Children().Append(ppi);
        validation.TextWrapping(TextWrapping::Wrap);root.Children().Append(validation);
        normalize(O({{L"type",S(L"refresh")}}));auto weak=weak_from_this();
        auto bind=[&](ComboBox const& box,wchar_t const* field,wchar_t const* op){box.SelectionChanged([weak,box,field,op](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&box.SelectedIndex()>=0){
            auto value=array(self->draft,field).GetAt(box.SelectedIndex());
            if(std::wstring_view(op)==L"encoding"){auto encoding=J::Parse(object(self->recipe,L"encoding").Stringify());encoding.Insert(L"dither",value);value=encoding;}
            self->normalize(O({{L"type",S(op)},{L"value",value}}));
        }});};
        bind(format,L"formats",L"format");bind(depth,L"depths",L"depth");bind(background,L"backgrounds",L"background");bind(dither,L"dithers",L"encoding");
        profile.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&self->profile.SelectedIndex()>=0){auto value=self->profiles.GetObjectAt(self->profile.SelectedIndex());self->profileId=str(value,L"library");self->normalize(O({{L"type",S(L"profile")},{L"value",value}}));}});
    }
};
}
