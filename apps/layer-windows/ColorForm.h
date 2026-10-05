#pragma once
#include "UiControls.h"
#include <array>
#include <winrt/Windows.ApplicationModel.DataTransfer.h>

namespace CapyUi {
inline V colorUi(CapyLocalization const* localization,J const& request) {
    auto text=to_string(request.Stringify());
    std::unique_ptr<char,decltype(&capy_string_free)> raw(capy_color_ui(localization,text.c_str()),capy_string_free);
    if(!raw)throw hresult_error(E_OUTOFMEMORY);
    return JsonValue::Parse(to_hstring(raw.get()));
}
inline Windows::UI::Color previewColor(A const& a){
    if(a.Size()!=4)return {};
    auto byte=[&](int i){return uint8_t(std::round(std::clamp(a.GetNumberAt(i),0.,1.)*255));};
    return {byte(3),byte(0),byte(1),byte(2)};
}
inline Windows::UI::Color displayColor(J const& value){return previewColor(array(value,L"rgba"));}
inline void copyText(hstring const& text){
    Windows::ApplicationModel::DataTransfer::DataPackage package;package.SetText(text);
    Windows::ApplicationModel::DataTransfer::Clipboard::SetContent(package);
}
// Native value rows; Rust owns the draft, parsing, conversion and formats.
struct ColorForm : std::enable_shared_from_this<ColorForm> {
    std::shared_ptr<WorkspaceData> data;
    explicit ColorForm(std::shared_ptr<WorkspaceData> context):data(std::move(context)){}
    StackPanel root;
    Grid pair,rows;
    Border current,preview;
    TextBox hex,intensity;
    Button hexCopy,apply;
    std::array<ComboBox,3> forms;
    std::array<TextBlock,3> spaces;
    std::array<std::array<TextBox,3>,3> values;
    std::array<Button,3> copies;
    TextBlock hexLabel,intensityLabel,error;
    J editor,view,rendition;
    std::optional<J> refused;
    hstring source,language;
    bool updating=false;
    std::function<void(J,std::optional<double>)> commit;
    std::optional<hstring> send(std::optional<J> action){
        auto request=O({{L"type",S(L"editor")},{L"editor",editor},{L"display_space",S(L"Srgb")},{L"rendition",rendition}});
        if(action)request.Insert(L"action",*action);
        auto next=colorUi(data->localization.get(),request).GetObject();
        if(!next.HasKey(L"editor"))return str(next,L"error");
        editor=object(next,L"editor");view=object(next,L"view");
        auto failure=next.GetNamedValue(L"error",JsonValue::CreateNullValue());
        if(failure.ValueType()==JsonValueType::String)return failure.GetString();
        return std::nullopt;
    }
    void act(J const& action){
        std::optional<hstring> failure;
        try{failure=send(action);}catch(hresult_error const& e){failure=e.message();}
        if(failure)refused=action;else refused.reset();
        error.Text(failure.value_or(L""));apply.IsEnabled(!failure);present();
    }
    void present(){
        updating=true;
        auto shown=[](TextBox const& box,hstring const& text){if(box.FocusState()==FocusState::Unfocused&&box.Text()!=text)box.Text(text);};
        current.Background(fill(displayColor(object(view,L"current"))));preview.Background(fill(displayColor(object(view,L"new"))));
        shown(hex,str(view,L"hex"));
        auto shownRows=array(view,L"rows");
        for(uint32_t row=0;row<3&&row<shownRows.Size();++row){
            auto item=shownRows.GetObjectAt(row);auto choices=array(item,L"forms");
            if(forms[row].Items().Size()!=choices.Size()){forms[row].Items().Clear();for(auto choice:choices)comboOption(forms[row],str(choice.GetObject(),L"label"),str(choice.GetObject(),L"form"));}
            for(uint32_t i=0;i<choices.Size();++i){
                comboOptionText(forms[row],i,str(choices.GetObjectAt(i),L"label"));
                if(str(choices.GetObjectAt(i),L"form")==str(item,L"form")&&forms[row].SelectedIndex()!=int32_t(i))forms[row].SelectedIndex(i);
            }
            AutomationProperties::SetName(forms[row],str(item,L"label"));
            auto space=item.GetNamedValue(L"space",JsonValue::CreateNullValue());
            spaces[row].Text(space.ValueType()==JsonValueType::String?space.GetString():L"");
            auto entries=array(item,L"values");
            for(uint32_t i=0;i<3&&i<entries.Size();++i){
                auto value=entries.GetObjectAt(i);
                AutomationProperties::SetName(values[row][i],str(value,L"name"));
                shown(values[row][i],str(value,L"text"));
            }
        }
        auto stops=view.GetNamedValue(L"intensity",JsonValue::CreateNullValue());
        auto hdr=stops.ValueType()==JsonValueType::Object;
        intensity.Visibility(hdr?Visibility::Visible:Visibility::Collapsed);intensityLabel.Visibility(intensity.Visibility());
        if(hdr)shown(intensity,str(stops.GetObject(),L"text"));
        auto copy=[&](wchar_t const* key){return data->caption(L"color",key);};
        hexLabel.Text(copy(L"hex"));intensityLabel.Text(copy(L"intensity_ev"));
        AutomationProperties::SetName(hex,copy(L"hex"));AutomationProperties::SetName(intensity,copy(L"intensity_ev"));
        for(auto const& button:copies)AutomationProperties::SetName(button,copy(L"copy"));
        AutomationProperties::SetName(hexCopy,copy(L"copy"));
        AutomationProperties::SetName(current,copy(L"current"));AutomationProperties::SetName(preview,copy(L"new"));
        apply.Content(box_value(copy(L"use_color")));AutomationProperties::SetName(apply,copy(L"use_color"));
        root.Language(data->language());language=data->language();
        updating=false;
    }
    void relocalize(){
        if(!view.Size()||language==data->language())return;
        try{send(std::nullopt);if(refused){if(auto failure=send(*refused))error.Text(*failure);}}catch(hresult_error const& e){error.Text(e.message());}
        present();
    }
    void load(J const& colors,J const& target,bool opaque,J const& panel){
        rendition=object(panel,L"rendition");
        auto request=O({{L"type",S(L"editor_open")},{L"colors",colors},{L"opaque",B(opaque)},{L"display_space",S(L"Srgb")},{L"rendition",rendition}});
        for(auto const& [key,value]:target)request.Insert(key,value);
        auto next=request.Stringify();if(next==source){relocalize();return;}source=next;
        try{
            auto opened=colorUi(data->localization.get(),request).GetObject();
            if(!opened.HasKey(L"editor")){error.Text(str(opened,L"error"));apply.IsEnabled(false);return;}
            editor=object(opened,L"editor");view=object(opened,L"view");refused.reset();error.Text(L"");apply.IsEnabled(true);
        }catch(hresult_error const& e){error.Text(e.message());apply.IsEnabled(false);return;}
        present();
    }
    void entry(TextBox const& box,std::function<J(hstring const&)> action){
        auto weak=weak_from_this();box.MaxLength(256);
        auto submit=[weak,box,action]{if(auto self=weak.lock();self&&!self->updating)self->act(action(box.Text()));};
        box.KeyDown([submit](auto&&,KeyRoutedEventArgs const& e){if(e.Key()==Windows::System::VirtualKey::Enter){e.Handled(true);submit();}});
        box.LostFocus([submit](auto&&,auto&&){submit();});
    }
    Button copyButton(hstring const& id,std::function<hstring()> text){
        auto copy=button(data,data->caption(L"color",L"copy"),[text]{copyText(text());});
        AutomationProperties::SetAutomationId(copy,id);return copy;
    }
    void init(std::function<void(J,std::optional<double>)> action,hstring const& id){
        commit=std::move(action);root.Spacing(8);auto weak=weak_from_this();
        for(int i=0;i<2;++i){ColumnDefinition column;column.Width({1,GridUnitType::Star});pair.ColumnDefinitions().Append(column);}
        for(auto [swatch,column,key]:{std::tuple{current,0,L"current"},std::tuple{preview,1,L"new"}}){
            swatch.MinHeight(44);Grid::SetColumn(swatch,column);pair.Children().Append(swatch);AutomationProperties::SetAutomationId(swatch,id+L"-"+key);
        }
        pair.CornerRadius({6,6,6,6});
        current.Tapped([weak](auto&&,auto&&){if(auto self=weak.lock())self->act(O({{L"op",S(L"revert")}}));});
        root.Children().Append(pair);
        Grid hexRow;for(int column=0;column<3;++column){ColumnDefinition definition;definition.Width(column==0?GridLength{1,GridUnitType::Star}:GridLength{0,GridUnitType::Auto});hexRow.ColumnDefinitions().Append(definition);}hexRow.ColumnSpacing(6);
        hexLabel.VerticalAlignment(VerticalAlignment::Center);hexRow.Children().Append(hexLabel);
        hex.FontSize(20);AutomationProperties::SetAutomationId(hex,id+L"-hex");Grid::SetColumn(hex,1);hexRow.Children().Append(hex);
        entry(hex,[](hstring const& text){return O({{L"op",S(L"text")},{L"text",S(text)}});});
        hexCopy=copyButton(id+L"-hex-copy",[weak]{auto self=weak.lock();return self?str(self->view,L"hex"):hstring{};});Grid::SetColumn(hexCopy,2);hexRow.Children().Append(hexCopy);
        root.Children().Append(hexRow);
        for(int column=0;column<6;++column){ColumnDefinition definition;definition.Width(column==0?GridLength{1,GridUnitType::Star}:GridLength{0,GridUnitType::Auto});rows.ColumnDefinitions().Append(definition);}
        rows.ColumnSpacing(6);rows.RowSpacing(4);
        for(int32_t row=0;row<4;++row)rows.RowDefinitions().Append(RowDefinition());
        for(int32_t row=0;row<3;++row){
            StackPanel name;name.Orientation(Orientation::Horizontal);name.Spacing(4);
            AutomationProperties::SetAutomationId(forms[row],id+L"-form-"+to_hstring(row));name.Children().Append(forms[row]);
            spaces[row].VerticalAlignment(VerticalAlignment::Center);spaces[row].Opacity(.72);name.Children().Append(spaces[row]);
            Grid::SetRow(name,row);rows.Children().Append(name);
            forms[row].SelectionChanged([weak,row](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&self->forms[row].SelectedIndex()>=0){
                auto choices=array(array(self->view,L"rows").GetObjectAt(row),L"forms");
                self->act(O({{L"op",S(L"form")},{L"row",N(row)},{L"form",S(str(choices.GetObjectAt(self->forms[row].SelectedIndex()),L"form"))}}));
            }});
            for(int32_t index=0;index<3;++index){
                auto box=values[row][index];box.Width(76);box.TextAlignment(TextAlignment::Right);
                AutomationProperties::SetAutomationId(box,id+L"-"+to_hstring(row)+L"-"+to_hstring(index));
                Grid::SetRow(box,row);Grid::SetColumn(box,1+index);rows.Children().Append(box);
                entry(box,[row,index](hstring const& text){return O({{L"op",S(L"value")},{L"row",N(row)},{L"index",N(index)},{L"text",S(text)}});});
            }
            copies[row]=copyButton(id+L"-copy-"+to_hstring(row),[weak,row]{auto self=weak.lock();return self?str(array(self->view,L"rows").GetObjectAt(row),L"copy"):hstring{};});
            Grid::SetRow(copies[row],row);Grid::SetColumn(copies[row],4);rows.Children().Append(copies[row]);
        }
        Grid::SetRow(intensityLabel,3);rows.Children().Append(intensityLabel);
        AutomationProperties::SetAutomationId(intensity,id+L"-intensity");intensity.TextAlignment(TextAlignment::Right);
        Grid::SetRow(intensity,3);Grid::SetColumn(intensity,1);Grid::SetColumnSpan(intensity,3);rows.Children().Append(intensity);
        entry(intensity,[](hstring const& text){return O({{L"op",S(L"intensity")},{L"text",S(text)}});});
        root.Children().Append(rows);
        error.TextWrapping(TextWrapping::Wrap);AutomationProperties::SetAutomationId(error,id+L"-error");root.Children().Append(error);
        AutomationProperties::SetAutomationId(apply,id+L"-apply");root.Children().Append(apply);
        apply.Click([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->refused){
            auto stops=self->view.GetNamedValue(L"stops",JsonValue::CreateNullValue());
            self->commit(object(self->view,L"value"),stops.ValueType()==JsonValueType::Number?std::optional<double>(stops.GetNumber()):std::nullopt);
        }});
        data->copyView([weak]{if(auto self=weak.lock()){self->relocalize();return true;}return false;});
    }
};
}
