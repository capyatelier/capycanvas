#pragma once
#include "UiControls.h"
#include <array>
#include <tuple>

namespace CapyUi {
inline V colorUi(CapyLocalization const* localization,J const& request) {
    auto text=to_string(request.Stringify());
    std::unique_ptr<char,decltype(&capy_string_free)> raw(capy_color_ui(localization,text.c_str()),capy_string_free);
    if(!raw)throw hresult_error(E_OUTOFMEMORY);
    return JsonValue::Parse(to_hstring(raw.get()));
}
inline Windows::UI::Color displayColor(J const& value){
    auto a=array(value,L"rgba");if(a.Size()!=4)return {};
    auto byte=[&](int i){return uint8_t(std::round(std::clamp(a.GetNumberAt(i),0.,1.)*255));};
    return {byte(3),byte(0),byte(1),byte(2)};
}
// Native draft controls; Rust owns coordinates, parsing, conversion and precision.
struct ColorForm : std::enable_shared_from_this<ColorForm> {
    std::shared_ptr<WorkspaceData> data;
    explicit ColorForm(std::shared_ptr<WorkspaceData> context):data(std::move(context)){}
    StackPanel root;
    ComboBox model;
    TextBox intensity;
    std::array<TextBox,4> entries;
    TextBlock description,error;
    Grid comparison;
    StackPanel baseFigure,adjustedFigure;
    Border basePreview,preview;
    TextBlock adjustedLabel;
    hstring modelsKey;
    Button apply;
    J view;
    bool updating=false;
    hstring source;
    std::function<void(J)> commit;
    bool refresh(J request){
        auto next=colorUi(data->localization.get(),O({{L"type",S(L"form")},{L"request",request}})).GetObject();
        if(!next.HasKey(L"draft")){error.Text(str(next,L"error"));apply.IsEnabled(false);return false;}
        view=next;
        updating=true;
        auto draft=object(view,L"draft");auto choices=array(view,L"models");
        if(auto key=choices.Stringify();key!=modelsKey){
            modelsKey=key;model.Items().Clear();
            for(uint32_t i=0;i<choices.Size();++i)model.Items().Append(box_value(choices.GetArrayAt(i).GetStringAt(1)));
        }
        for(uint32_t i=0;i<choices.Size();++i)if(choices.GetArrayAt(i).GetStringAt(0)==str(draft,L"model")&&model.SelectedIndex()!=int32_t(i))model.SelectedIndex(i);
        auto fields=array(draft,L"fields"),labels=array(view,L"labels");
        for(uint32_t i=0;i<4;++i){
            entries[i].Header(box_value(labels.GetStringAt(i)));
            if(entries[i].FocusState()==FocusState::Unfocused&&entries[i].Text()!=fields.GetStringAt(i))entries[i].Text(fields.GetStringAt(i));
        }
        auto shown=object(view,L"preview"),base=object(view,L"base_preview");
        preview.Background(shown.Size()?fill(displayColor(shown)):clear());
        baseFigure.Visibility(base.Size()?Visibility::Visible:Visibility::Collapsed);adjustedLabel.Visibility(base.Size()?Visibility::Visible:Visibility::Collapsed);
        Grid::SetColumnSpan(adjustedFigure,base.Size()?1:2);Grid::SetColumn(adjustedFigure,base.Size()?1:0);
        if(base.Size())basePreview.Background(fill(displayColor(base)));
        description.Text(str(view,L"description"));error.Text(str(view,L"error",str(view,L"validation")));
        apply.IsEnabled(view.GetNamedValue(L"value").ValueType()==JsonValueType::Object&&str(view,L"error").empty());
        auto stops=draft.GetNamedValue(L"intensity",JsonValue::CreateNullValue());intensity.Visibility(stops.ValueType()==JsonValueType::Number?Visibility::Visible:Visibility::Collapsed);
        if(stops.ValueType()==JsonValueType::Number)intensity.Text(str(draft,L"change_intensity_text",to_hstring(stops.GetNumber())));
        updating=false;
        return true;
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
        for(int i=0;i<2;++i){ColumnDefinition column;column.Width({1,GridUnitType::Star});comparison.ColumnDefinitions().Append(column);}
        comparison.ColumnSpacing(8);
        for(auto [figure,swatch,caption]:{std::tuple{baseFigure,basePreview,data->caption(L"color",L"base")},std::tuple{adjustedFigure,preview,data->caption(L"color",L"adjusted")}}){
            swatch.MinHeight(48);swatch.CornerRadius({6,6,6,6});figure.Spacing(4);
            TextBlock name;name.Text(caption);name.Opacity(.72);if(figure==adjustedFigure)adjustedLabel=name;
            figure.Children().Append(name);figure.Children().Append(swatch);comparison.Children().Append(figure);
        }
        Grid::SetColumn(adjustedFigure,1);
        root.Children().Append(comparison);
        model.Header(box_value(data->caption(L"color",L"model")));model.HorizontalAlignment(HorizontalAlignment::Stretch);
        AutomationProperties::SetAutomationId(model,id+L"-model");root.Children().Append(model);
        model.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&self->model.SelectedIndex()>=0){
            auto request=self->draft();request.Insert(L"change_model",array(self->view,L"models").GetArrayAt(self->model.SelectedIndex()).GetAt(0));self->refresh(request);
        }});
        for(uint32_t i=0;i<4;++i){auto entry=entries[i];entry.MaxLength(128);AutomationProperties::SetAutomationId(entry,id+L"-"+to_hstring(i));root.Children().Append(entry);}
        intensity.Header(box_value(data->caption(L"color",L"intensity_ev")));AutomationProperties::SetAutomationId(intensity,id+L"-intensity");root.Children().Append(intensity);
        description.TextWrapping(TextWrapping::Wrap);error.TextWrapping(TextWrapping::Wrap);root.Children().Append(description);root.Children().Append(error);
        apply.Content(box_value(data->common(L"apply")));AutomationProperties::SetAutomationId(apply,id+L"-apply");root.Children().Append(apply);
        apply.Click([weak](auto&&,auto&&){if(auto self=weak.lock()){
            try{if(!self->refresh(self->draft()))return;}catch(hresult_error const& e){self->error.Text(e.message());return;}auto value=self->view.GetNamedValue(L"value",JsonValue::CreateNullValue());
            if(value.ValueType()==JsonValueType::Object&&str(self->view,L"error").empty())self->commit(value.GetObject());
        }});
        // Invalid drafts must remain editable and retryable.
        intensity.TextChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating){auto draft=object(self->view,L"draft");if(self->intensity.Text()!=str(draft,L"change_intensity_text",to_hstring(num(draft,L"intensity"))))self->apply.IsEnabled(true);}});
        for(uint32_t i=0;i<entries.size();++i)entries[i].TextChanged([weak,i](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating){
            auto fields=array(object(self->view,L"draft"),L"fields");if(fields.Size()<=i||self->entries[i].Text()==fields.GetStringAt(i))return;
            self->apply.IsEnabled(true);
            try{self->refresh(self->draft());}catch(hresult_error const& e){self->error.Text(e.message());}
        }});
    }
};
}
