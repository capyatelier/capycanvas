#pragma once
#include "PanelBody.h"
#include "LayersView.h"
#include "WorkspaceQuery.h"
#include <chrono>

namespace CapyUi {
struct LayerInspector {
    Grid root;
    Canvas pages;
    Button footer{nullptr};
    Grid footerLayout,face;
    ColumnDefinition addColumn;
    Button add{nullptr};
    TextBlock label{nullptr},summary{nullptr};
    Image mark,chevron,addMark;
    RotateTransform chevronRotation;
    std::unique_ptr<PanelBody> properties;
    J sizes,anchor;
    hstring sizingKey,target;
    double position=0,startPosition=0,targetPosition=0;
    std::chrono::steady_clock::time_point started;
    bool animating=false;
    std::shared_ptr<WorkspaceData> data;
    LayerInspector(std::shared_ptr<WorkspaceData> source,J const& panel,std::shared_ptr<WorkspaceGestures> const& gestures,std::function<void()> changed,std::function<void()> toggle):data(std::move(source)){
        properties=std::make_unique<PanelBody>(data,panel,J{},changed,gestures,true);
        RowDefinition page;page.Height({1,GridUnitType::Star});root.RowDefinitions().Append(page);
        RowDefinition footerRow;footerRow.Height({49,GridUnitType::Pixel});root.RowDefinitions().Append(footerRow);
        root.Children().Append(pages);
        footer=button(data,L"",std::move(toggle));
        footer.MinWidth(0);footer.Padding({6,0,6,0});footer.BorderThickness({0});footer.CornerRadius({0,0,6,6});footer.HorizontalAlignment(HorizontalAlignment::Stretch);footer.HorizontalContentAlignment(HorizontalAlignment::Stretch);footer.Background(clear());footer.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());
        face.ColumnSpacing(data->panelMetric(L"group_spacing"));for(int i=0;i<3;++i){ColumnDefinition col;col.Width({i==1?1.:data->panelMetric(L"control_height"),i==1?GridUnitType::Star:GridUnitType::Pixel});face.ColumnDefinitions().Append(col);}
        mark=icon(L"properties",data->theme());mark.HorizontalAlignment(HorizontalAlignment::Center);mark.VerticalAlignment(VerticalAlignment::Center);face.Children().Append(mark);
        StackPanel text;text.Spacing(1);text.VerticalAlignment(VerticalAlignment::Center);
        label=CapyUi::label(data,L"");summary=CapyUi::label(data,L"");for(auto const& line:{label,summary}){line.MaxLines(1);line.TextTrimming(TextTrimming::CharacterEllipsis);}summary.Opacity(.55);text.Children().Append(label);text.Children().Append(summary);Grid::SetColumn(text,1);face.Children().Append(text);
        chevron=icon(L"chevron-down",data->theme());chevron.HorizontalAlignment(HorizontalAlignment::Center);chevron.VerticalAlignment(VerticalAlignment::Center);chevron.RenderTransformOrigin({.5f,.5f});chevron.RenderTransform(chevronRotation);Grid::SetColumn(chevron,2);face.Children().Append(chevron);footer.Content(face);
        addColumn.Width({0,GridUnitType::Pixel});footerLayout.ColumnDefinitions().Append(addColumn);ColumnDefinition disclosure;disclosure.Width({1,GridUnitType::Star});footerLayout.ColumnDefinitions().Append(disclosure);
        auto action=O({{L"type",S(L"layer")},{L"action",O({{L"op",S(L"new")},{L"group",B(false)},{L"clipped",B(false)}})}});
        add=button(data,data->copyCaption(L"layers",L"new_layer"),[data=data,action]{data->dispatch(action);});add.Padding({0});add.BorderThickness({0});add.Background(clear());add.Visibility(Visibility::Collapsed);
        add.HorizontalAlignment(HorizontalAlignment::Stretch);add.VerticalAlignment(VerticalAlignment::Stretch);add.HorizontalContentAlignment(HorizontalAlignment::Center);add.VerticalContentAlignment(VerticalAlignment::Center);
        addMark=icon(L"add-layer",data->theme());add.Content(addMark);
        AutomationProperties::SetAutomationId(add,L"layer-new");actionTooltip(data,add,[action]{return action;});footerLayout.Children().Append(add);Grid::SetColumn(footer,1);footerLayout.Children().Append(footer);
        Border strip;strip.BorderThickness({0,1,0,0});strip.BorderBrush(data->tint(L"text",41));strip.Child(footerLayout);Grid::SetRow(strip,1);root.Children().Append(strip);
        AutomationProperties::SetAutomationId(root,L"layer-inspector");AutomationProperties::SetAutomationId(footer,L"layer-properties-disclosure");AutomationProperties::SetAutomationId(label,L"layer-properties-disclosure-label");
        AutomationProperties::SetAutomationId(mark,L"layer-properties-icon");AutomationProperties::SetAutomationId(chevron,L"layer-properties-chevron");
    }
    void refresh(bool open,hstring const& caption={},bool simple=false){
        auto view=object(data->state,L"layer_properties"),disclosure=object(view,L"disclosure");
        label.Text(simple&&!caption.empty()?caption:str(disclosure,open?L"return_label":L"label"));summary.Text(open||simple?L"":str(disclosure,L"summary"));summary.Visibility(summary.Text().empty()?Visibility::Collapsed:Visibility::Visible);
        auto slot=data->panelMetric(L"control_height"),padding=footer.Padding().Left;
        add.Visibility(simple?Visibility::Visible:Visibility::Collapsed);addColumn.Width({simple?slot+2*padding:0.,GridUnitType::Pixel});setIcon(addMark,L"add-layer",data->theme());
        footer.Margin({simple?data->panelMetric(L"group_spacing"):0.,0,0,0});
        add.Opacity(open?0:1);add.IsHitTestVisible(!open);add.IsTabStop(!open);
        mark.Visibility(simple?Visibility::Collapsed:Visibility::Visible);face.ColumnDefinitions().GetAt(0).Width({simple?0.:slot,GridUnitType::Pixel});label.TextAlignment(simple?TextAlignment::Right:TextAlignment::Left);
        footer.IsEnabled(open||view.GetNamedValue(L"layer",JsonValue::CreateNullValue()).ValueType()!=JsonValueType::Null);
        AutomationProperties::SetName(footer,label.Text());ToolTipService::SetToolTip(footer,box_value(label.Text()+(summary.Text().empty()?L"":L"\n"+summary.Text())));
        setIcon(mark,L"properties",data->theme());setIcon(chevron,L"chevron-down",data->theme());chevronRotation.Angle(open?0.:180.);label.FontSize(data->textSize());summary.FontSize(data->textSize()*.9);
        auto nextTarget=CapyLayers::epochOf(data)+L":"+to_hstring(num(view,L"layer",-1));if(nextTarget!=target){target=nextTarget;anchor=J{};}
    }
    void place(FrameworkElement const& layers,double width,double height,double footerHeight,bool open){
        auto page=std::max(0.,height-footerHeight);
        root.Width(width);root.Height(height);footer.Height(std::max(0.,footerHeight-1.));
        root.RowDefinitions().GetAt(1).Height({footerHeight,GridUnitType::Pixel});
        pages.Width(width);pages.Height(page);
        auto clip=pages.Clip();auto before=clip?clip.Rect():Rect{};
        if(!clip||before.Width!=width||before.Height!=page){if(!clip)clip=RectangleGeometry();clip.Rect({0,0,float(width),float(page)});pages.Clip(clip);}
        auto propertyPage=properties->Root();
        auto inset=data->panelMetric(add.Visibility()==Visibility::Visible?L"inset":L"compound_spacing");
        layers.Width(std::max(1.,width));layers.Height(std::max(1.,page-inset*2));Canvas::SetLeft(layers,0);Canvas::SetTop(layers,inset-position*page);
        propertyPage.Width(width);propertyPage.Height(std::max(1.,page));Canvas::SetTop(propertyPage,(1.-position)*page);
        layers.IsHitTestVisible(!open);propertyPage.IsHitTestVisible(open);
    }
    J sizingRequest(PanelBody const& layers)const{
        auto measurement=layers.ScrollMetrics();
        if(num(measurement,L"unit_height")<=0)measurement.Insert(L"unit_height",N(40));
        measurement.Insert(L"fixed_height",N(num(measurement,L"fixed_height")+data->panelMetric(add.Visibility()==Visibility::Visible?L"inset":L"compound_spacing")*2));
        return O({{L"type",S(L"layer_drawer_sizing")},{L"measurement",measurement},{L"properties_height",N(properties->ContentHeight())}});
    }
};
}
