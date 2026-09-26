#pragma once
#include "UiControls.h"

namespace CapyUi {
struct CommandSearchPopup : std::enable_shared_from_this<CommandSearchPopup> {
    std::shared_ptr<WorkspaceData> data;
    Canvas host{nullptr};
    Primitives::Popup popup;
    Border frame;
    Grid field;
    TextBox entry;
    TextBlock unit,detail,empty;
    StackPanel results;
    Button close{nullptr};
    J view;
    hstring signature,parameterId,theme,focusScope=L"canvas";
    bool updating=false,open=false;
    weak_ref<Control> previousFocus;
    std::vector<Button> rows;
    UIElement::GettingFocus_revoker focusChanged;

    J style()const{return object(data->catalog,L"command_search_style");}
    void send(J const& action){data->dispatch(O({{L"type",S(L"command_search")},{L"action",action}}));}
    J parameter()const{return object(view,L"parameter");}
    J selected()const{
        auto list=array(view,L"results");auto index=uint32_t(num(view,L"selected"));
        return index<list.Size()?list.GetObjectAt(index):J{};
    }
    void reportFocus(DependencyObject node){
        hstring scope=L"canvas";
        if(node.try_as<TextBox>()||node.try_as<PasswordBox>()||node.try_as<RichEditBox>()||node.try_as<AutoSuggestBox>())scope=L"text";
        else for(;node;node=VisualTreeHelper::GetParent(node))if(AutomationProperties::GetAutomationId(node)==L"palettes-panel"){scope=L"palette";break;}
        if(scope!=focusScope){focusScope=scope;send(O({{L"type",S(L"focus")},{L"focus",S(scope)}}));}
    }
    void execute(hstring const& id){
        send(O({{L"type",S(L"execute")},{L"id",S(id)},
            {L"value",parameter().Size()?JsonValue::CreateStringValue(entry.Text()):JsonValue::CreateNullValue()}}));
    }
    void init(Canvas const& root){
        host=root;auto weak=weak_from_this();auto metrics=style();
        host.Loaded([weak](auto&&,auto&&){
            auto self=weak.lock();if(!self||self->focusChanged)return;
            self->focusChanged=self->host.XamlRoot().Content().GettingFocus(auto_revoke,[weak](auto&&,Input::GettingFocusEventArgs const& e){
                if(auto self=weak.lock();self&&!self->open)self->reportFocus(e.NewFocusedElement());
            });
        });
        double inset=num(metrics,L"inset",12),gap=num(metrics,L"gap",8);
        StackPanel body;body.Spacing(gap);
        Grid header;header.ColumnSpacing(gap);
        for(auto width:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Auto}}){
            ColumnDefinition column;column.Width(width);header.ColumnDefinitions().Append(column);
        }
        entry.MinHeight(32);entry.Padding({32,5,6,6});entry.PlaceholderText(L"Search commands");entry.IsSpellCheckEnabled(false);
        AutomationProperties::SetAutomationId(entry,L"command-search");AutomationProperties::SetName(entry,L"Search commands");
        field.Children().Append(entry);header.Children().Append(field);
        unit.VerticalAlignment(VerticalAlignment::Center);unit.Opacity(.7);unit.Visibility(Visibility::Collapsed);
        Grid::SetColumn(unit,1);header.Children().Append(unit);
        close=button(data,L"Close command search",[weak]{if(auto self=weak.lock())self->send(O({{L"type",S(L"close")}}));});
        close.Width(32);close.Height(32);close.Padding({0,0,0,0});
        AutomationProperties::SetAutomationId(close,L"command-search-close");
        Grid::SetColumn(close,2);header.Children().Append(close);
        body.Children().Append(header);
        results.Spacing(2);AutomationProperties::SetAutomationId(results,L"command-results");AutomationProperties::SetName(results,L"Commands");
        body.Children().Append(results);
        empty.Text(L"No matching commands");empty.Opacity(.6);empty.Margin({0,12,0,12});empty.HorizontalAlignment(HorizontalAlignment::Center);
        empty.Visibility(Visibility::Collapsed);body.Children().Append(empty);
        detail.Opacity(.7);detail.FontSize(12);detail.Height(20);detail.Margin({inset,0,inset,0});
        detail.TextTrimming(TextTrimming::CharacterEllipsis);AutomationProperties::SetAutomationId(detail,L"command-search-detail");
        body.Children().Append(detail);
        frame.Child(body);frame.Padding({inset,inset,inset,inset});frame.CornerRadius({12,12,12,12});frame.BorderThickness({1,1,1,1});
        frame.Shadow(ThemeShadow());frame.Translation({0,0,32});
        AutomationProperties::SetAutomationId(frame,L"command-bar");AutomationProperties::SetName(frame,L"Command search");
        popup.Child(frame);popup.IsLightDismissEnabled(true);
        popup.Closed([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->open&&!self->updating){
            self->open=false;self->data->popup(false);self->send(O({{L"type",S(L"close")}}));self->restoreFocus();
        }});
        entry.TextChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&self->view.Size()&&!self->parameter().Size())
            self->send(O({{L"type",S(L"query")},{L"text",S(self->entry.Text())}}));});
        frame.PreviewKeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->view.Size()){
            using Windows::System::VirtualKey;
            switch(e.Key()){
                case VirtualKey::Up:case VirtualKey::Down:
                    if(self->parameter().Size())return;
                    self->send(O({{L"type",S(L"move")},{L"delta",N(e.Key()==VirtualKey::Up?-1:1)}}));break;
                case VirtualKey::Enter:self->send(O({{L"type",S(L"commit")},{L"text",S(self->entry.Text())}}));break;
                case VirtualKey::Escape:self->send(O({{L"type",S(L"back")}}));break;
                default:return;
            }
            e.Handled(true);
        }});
    }
    void restoreFocus(){
        if(auto focus=previousFocus.get();focus&&focus.IsLoaded())focus.Focus(FocusState::Programmatic);
        previousFocus=nullptr;
    }
    void paint(){
        frame.Background(data->brush(L"panel"));frame.BorderBrush(data->tint(L"text",26));
        close.Content(icon(L"close",data->theme()));
        auto glyph=icon(L"search",data->theme());glyph.Opacity(.65);glyph.Margin({10,0,0,0});
        glyph.HorizontalAlignment(HorizontalAlignment::Left);glyph.VerticalAlignment(VerticalAlignment::Center);
        if(field.Children().Size()>1)field.Children().SetAt(1,glyph);else field.Children().Append(glyph);
        for(auto item:{unit,detail,empty})item.Foreground(data->brush(L"text"));
    }
    void buildRows(){
        results.Children().Clear();rows.clear();auto weak=weak_from_this();
        double height=num(style(),L"row_height",44),inset=num(style(),L"inset",12);
        auto list=array(view,L"results");
        for(uint32_t i=0;i<list.Size();++i){
            auto command=list.GetObjectAt(i);auto id=str(command,L"id");bool enabled=flag(command,L"enabled");
            Grid content;content.ColumnSpacing(inset);
            for(auto width:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Auto}}){
                ColumnDefinition column;column.Width(width);content.ColumnDefinitions().Append(column);
            }
            auto name=label(data,str(command,L"label"));name.TextTrimming(TextTrimming::CharacterEllipsis);name.VerticalAlignment(VerticalAlignment::Center);
            if(!enabled)name.Opacity(.45);
            content.Children().Append(name);
            if(flag(command,L"selected")){auto check=icon(L"check",data->theme(),16);check.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(check,1);content.Children().Append(check);}
            auto shortcut=label(data,str(command,L"shortcut"));shortcut.FontSize(12);shortcut.Opacity(.6);shortcut.VerticalAlignment(VerticalAlignment::Center);
            Grid::SetColumn(shortcut,2);content.Children().Append(shortcut);
            Button row;row.Content(content);row.Height(height);row.HorizontalAlignment(HorizontalAlignment::Stretch);
            row.HorizontalContentAlignment(HorizontalAlignment::Stretch);row.Padding({inset,0,inset,0});row.BorderThickness({0,0,0,0});
            row.CornerRadius({8,8,8,8});row.IsTabStop(false);
            AutomationProperties::SetAutomationId(row,L"command-result-"+to_hstring(i));AutomationProperties::SetName(row,str(command,L"label"));
            AutomationProperties::SetHelpText(row,str(command,L"disabled_reason").empty()?str(command,L"description"):str(command,L"disabled_reason"));
            row.Click([weak,id](auto&&,auto&&){if(auto self=weak.lock())self->execute(id);});
            row.PointerMoved([weak,id](auto&&,auto&&){if(auto self=weak.lock();self&&str(self->selected(),L"id")!=id)
                self->send(O({{L"type",S(L"select")},{L"id",S(id)}}));});
            results.Children().Append(row);rows.push_back(row);
        }
    }
    void Apply(J const& state){
        auto value=state.GetNamedValue(L"command_search",JsonValue::CreateNullValue());
        updating=true;
        if(value.ValueType()!=JsonValueType::Object){
            view=J{};
            if(open){open=false;popup.IsOpen(false);data->popup(false);restoreFocus();}
            updating=false;return;
        }
        view=value.GetObject();auto input=parameter();bool entering=input.Size()!=0;
        auto numeric=object(object(input,L"parameter"),L"numeric");auto suffix=str(numeric,L"unit");
        unit.Text(suffix);unit.Visibility(suffix.empty()?Visibility::Collapsed:Visibility::Visible);
        AutomationProperties::SetName(entry,entering?str(input,L"label"):hstring(L"Search commands"));
        auto nextParameter=str(input,L"id");
        if(!open||nextParameter!=parameterId){
            entry.Text(entering?str(object(input,L"parameter"),L"text"):str(view,L"query"));
            if(entering)entry.SelectAll();
        }
        parameterId=nextParameter;
        entry.PlaceholderText(entering?L"Enter a value":L"Search commands");
        if(theme!=data->theme()){theme=data->theme();paint();signature=L"";}
        auto nextSignature=array(view,L"results").Stringify()+theme;
        if(nextSignature!=signature){signature=nextSignature;buildRows();}
        auto chosen=uint32_t(num(view,L"selected"));
        for(uint32_t i=0;i<rows.size();++i){
            rows[i].Background(i==chosen?Brush(data->tint(L"text",26)):Brush(clear()));
            AutomationProperties::SetItemStatus(rows[i],i==chosen?L"Selected":L"");
        }
        results.Visibility(entering?Visibility::Collapsed:Visibility::Visible);
        empty.Visibility(!entering&&!array(view,L"results").Size()?Visibility::Visible:Visibility::Collapsed);
        auto current=entering?input:selected();
        hstring text=str(view,L"error");
        if(text.empty())text=str(current,L"disabled_reason");
        if(text.empty())text=str(current,L"description");
        detail.Text(text);ToolTipService::SetToolTip(detail,text.empty()?nullptr:box_value(text));
        if(!open){
            previousFocus=FocusManager::GetFocusedElement(host.XamlRoot()).try_as<Control>();
            double width=std::clamp(host.ActualWidth()-num(style(),L"inset",12)*4,240.,num(style(),L"width",560));
            frame.Width(width);popup.XamlRoot(host.XamlRoot());
            double inset=num(style(),L"inset",12);
            popup.HorizontalOffset(std::max(0.,(host.ActualWidth()-width)/2));popup.VerticalOffset(std::clamp(host.ActualHeight()/5,inset*4,inset*16));
            open=true;data->popup(true);popup.IsOpen(true);entry.Focus(FocusState::Programmatic);
        }
        updating=false;
    }
};
}
