#pragma once
#include "UiControls.h"
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>

namespace CapyUi {
inline constexpr double PickerPadding=20,PickerDialogHeight=486.67;
inline ContentDialog pickerDialog(std::shared_ptr<WorkspaceData> const& data,XamlRoot const& xaml,Grid const& root,FrameworkElement const& page,Grid const& footer,hstring const& id){
    ContentDialog dialog;dialog.XamlRoot(xaml);
    dialogSurface(data,dialog);
    dialog.Resources().Insert(box_value(L"ContentDialogMaxWidth"),box_value(760.));
    dialog.Resources().Insert(box_value(L"ContentDialogMinWidth"),box_value(300.));
    dialog.Resources().Insert(box_value(L"ContentDialogMaxHeight"),box_value(100000.));
    dialog.Resources().Insert(box_value(L"ContentDialogPadding"),box_value(Thickness{0,0,0,0}));
    dialog.Content(root);
    AutomationProperties::SetAutomationId(dialog,id);
    AutomationProperties::SetAutomationId(root,id+L"-frame");
    inheritLanguage(root,data);
    for(auto height:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){RowDefinition row;row.Height(height);root.RowDefinitions().Append(row);}
    root.Children().Append(page);Grid::SetRow(footer,1);root.Children().Append(footer);
    return dialog;
}
inline void pickerTitle(std::shared_ptr<WorkspaceData> const& data,TextBlock const& title,hstring const& id){
    title.FontFamily(FontFamily(L"Segoe UI"));title.FontWeight(winrt::Windows::UI::Text::FontWeights::Bold());title.FontSize(data->textSize()*1.05);
    title.HorizontalAlignment(HorizontalAlignment::Center);title.Foreground(data->brush(L"text"));AutomationProperties::SetAutomationId(title,id);
    AutomationProperties::SetHeadingLevel(title,Automation::Peers::AutomationHeadingLevel::Level1);
}
inline void pickerFooter(std::shared_ptr<WorkspaceData> const& data,Grid const& footer,std::initializer_list<GridUnitType> columns){
    for(auto unit:columns){ColumnDefinition definition;definition.Width({1,unit});footer.ColumnDefinitions().Append(definition);}
    footer.ColumnSpacing(8);footer.Padding({PickerPadding,16,PickerPadding,PickerPadding});footer.BorderThickness({0,1,0,0});footer.BorderBrush(data->line(L"settings",L"border"));
}
inline Button pickerButton(std::shared_ptr<WorkspaceData> const& data,hstring const& id,int column,bool primary,std::function<void()> action){
    auto result=button(data,L"",std::move(action));
    result.MinHeight(34);result.Padding({16,0,16,0});result.FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());
    AutomationProperties::SetAutomationId(result,id);Grid::SetColumn(result,column);
    if(!primary){result.Background(buttonBackground(data));return result;}
    result.Background(accent(data));result.Foreground(data->brush(L"accent_foreground"));
    return result;
}
inline void pickerLabel(Button const& target,hstring const& text){buttonCaption(target,text);}
inline double fitPicker(Grid const& root){
    auto xaml=root.XamlRoot();if(!xaml)return 0;auto size=xaml.Size();
    double width=std::max(280.,std::min(720.,double(size.Width)-24));
    root.Width(width);root.MaxHeight(std::max(240.,double(size.Height)-24));
    return width;
}
}
