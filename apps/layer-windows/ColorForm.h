#pragma once
#include "UiControls.h"
#include <array>

namespace CapyUi {
inline V colorUi(J const& request) {
    auto text=to_string(request.Stringify());
    std::unique_ptr<char,decltype(&capy_string_free)> raw(capy_color_ui(text.c_str()),capy_string_free);
    if(!raw)throw hresult_error(E_FAIL,L"Color form is unavailable");
    return JsonValue::Parse(to_hstring(raw.get()));
}
inline Windows::UI::Color displayColor(J const& value){
    auto a=array(value,L"rgba");if(a.Size()!=4)return {};
    auto byte=[&](int i){return uint8_t(std::round(std::clamp(a.GetNumberAt(i),0.,1.)*255));};
    return {byte(3),byte(0),byte(1),byte(2)};
}
// Native draft controls; Rust owns coordinates, parsing, conversion and precision.
struct ColorForm : std::enable_shared_from_this<ColorForm> {
    StackPanel root;
    ComboBox model;
    TextBox intensity;
    std::array<TextBox,4> entries;
    TextBlock description,error;
    Button apply;
    J view;
    bool updating=false;
    hstring source;
    std::function<void(J)> commit;
    void refresh(J request){
        view=colorUi(O({{L"type",S(L"form")},{L"request",request}})).GetObject();
        if(!view.HasKey(L"draft")){error.Text(str(view,L"error"));apply.IsEnabled(false);return;}
        updating=true;
        auto draft=object(view,L"draft");auto choices=array(view,L"models");model.Items().Clear();
        for(uint32_t i=0;i<choices.Size();++i){auto choice=choices.GetArrayAt(i);model.Items().Append(box_value(choice.GetStringAt(1)));
            if(choice.GetStringAt(0)==str(draft,L"model"))model.SelectedIndex(i);}
        auto fields=array(draft,L"fields"),labels=array(view,L"labels");
        for(uint32_t i=0;i<4;++i){entries[i].Header(box_value(labels.GetStringAt(i)));entries[i].Text(fields.GetStringAt(i));}
        description.Text(str(view,L"description"));error.Text(str(view,L"error",str(view,L"validation")));
        apply.IsEnabled(view.GetNamedValue(L"value").ValueType()==JsonValueType::Object&&str(view,L"error").empty());
        auto stops=draft.GetNamedValue(L"intensity",JsonValue::CreateNullValue());intensity.Visibility(stops.ValueType()==JsonValueType::Number?Visibility::Visible:Visibility::Collapsed);
        if(stops.ValueType()==JsonValueType::Number)intensity.Text(str(draft,L"change_intensity_text",to_hstring(stops.GetNumber())));
        updating=false;
    }
    J draft(){auto request=J::Parse(object(view,L"draft").Stringify());A fields;for(auto entry:entries)fields.Append(S(entry.Text()));request.Insert(L"fields",fields);if(intensity.Visibility()==Visibility::Visible)request.Insert(L"change_intensity_text",S(intensity.Text()));return request;}
    void load(J const& color,hstring const& space,J const& panel=J{},bool paint=false){
        auto next=color.Stringify()+space+panel.Stringify();if(next==source)return;source=next;
        auto request=O({{L"color",color},{L"document_space",S(space)}});
        if(flag(panel,L"hdr")){request.Insert(L"document_depth",S(str(panel,L"document_depth")));if(paint)request.Insert(L"intensity",N(num(panel,L"intensity")));request.Insert(L"rendition",object(panel,L"rendition"));}
        if(view.HasKey(L"draft"))request.Insert(L"model",S(str(object(view,L"draft"),L"model")));
        refresh(request);
    }
    void init(std::function<void(J)> action,hstring const& id){
        commit=std::move(action);root.Spacing(6);auto weak=weak_from_this();
        model.Header(box_value(L"Color coordinates"));model.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetAutomationId(model,id+L"-model");root.Children().Append(model);
        model.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&self->model.SelectedIndex()>=0){
            auto request=self->draft();request.Insert(L"change_model",array(self->view,L"models").GetArrayAt(self->model.SelectedIndex()).GetAt(0));self->refresh(request);
        }});
        for(uint32_t i=0;i<4;++i){auto entry=entries[i];entry.MaxLength(128);AutomationProperties::SetAutomationId(entry,id+L"-"+to_hstring(i));root.Children().Append(entry);}
        intensity.Header(box_value(L"HDR intensity (EV)"));AutomationProperties::SetAutomationId(intensity,id+L"-intensity");root.Children().Append(intensity);
        description.TextWrapping(TextWrapping::Wrap);error.TextWrapping(TextWrapping::Wrap);root.Children().Append(description);root.Children().Append(error);
        apply.Content(box_value(L"Apply color"));AutomationProperties::SetAutomationId(apply,id+L"-apply");root.Children().Append(apply);
        apply.Click([weak](auto&&,auto&&){if(auto self=weak.lock()){
            try{self->refresh(self->draft());}catch(hresult_error const& e){self->error.Text(e.message());return;}auto value=self->view.GetNamedValue(L"value",JsonValue::CreateNullValue());
            if(value.ValueType()==JsonValueType::Object&&str(self->view,L"error").empty())self->commit(value.GetObject());
        }});
        // Invalid drafts must remain editable and retryable.
        intensity.TextChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating){auto draft=object(self->view,L"draft");if(self->intensity.Text()!=str(draft,L"change_intensity_text",to_hstring(num(draft,L"intensity"))))self->apply.IsEnabled(true);}});
        for(uint32_t i=0;i<entries.size();++i)entries[i].TextChanged([weak,i](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating){auto fields=array(object(self->view,L"draft"),L"fields");if(fields.Size()>i&&self->entries[i].Text()!=fields.GetStringAt(i))self->apply.IsEnabled(true);}});
    }
};
}
