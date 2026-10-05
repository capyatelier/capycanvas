#include "pch.h"
#include "ColorEditor.h"
#include "ColorStrip.h"
#include "ColorWheel.h"
#include "Checker.h"
#include "WorkspaceQuery.h"
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>
#include <winrt/Microsoft.UI.Xaml.Media.Animation.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Windows.ApplicationModel.DataTransfer.h>

using namespace CapyUi;
using namespace CapyWheel;
namespace {
namespace Animation=winrt::Microsoft::UI::Xaml::Media::Animation;
using winrt::Windows::Foundation::Point;
using winrt::Windows::System::VirtualKey;
constexpr double WheelSide=232,RecentTile=34,SheetTile=40,NarrowWidth=700,ScrubSlop=4;
constexpr winrt::Windows::UI::Color Invalid{255,0xe0,0x1b,0x24};

bool held(int key){return (GetKeyState(key)&0x8000)!=0;}
hstring speed(){
    if(held(VK_SHIFT))return L"fast";
    return held(VK_CONTROL)||held(VK_MENU)?L"fine":L"normal";
}
double textWidth(hstring const& sample,double size,winrt::Windows::UI::Text::FontWeight weight){
    TextBlock probe;probe.FontFamily(FontFamily(L"Segoe UI"));probe.FontSize(size);probe.FontWeight(weight);probe.Text(sample);
    probe.Measure({10000,10000});return std::ceil(probe.DesiredSize().Width);
}

struct Field {
    J target;
    Grid cell;
    Button show{nullptr};
    TextBlock text;
    TextBox entry;
    hstring edit,name;
    bool suppress=false;
    bool editing()const{return entry.Visibility()==Visibility::Visible;}
    bool numeric()const{return str(target,L"kind")!=L"hex";}
};

struct Editor;
std::vector<std::shared_ptr<Editor>>& openEditors(){thread_local std::vector<std::shared_ptr<Editor>> editors;return editors;}

struct Editor:std::enable_shared_from_this<Editor>{
    std::shared_ptr<WorkspaceData> data;
    ColorAccepted accepted;
    J editor,view,errorTarget;
    V rendition{nullptr};
    hstring memory;
    double epoch=0,pickRevision=0;
    std::optional<hstring> error;
    std::optional<J> refused;
    bool picking=false,seenPicker=false,closing=false,finished=false,accepting=false,sheetOpen=false,narrow=false,updating=false;
    ContentDialog dialog{nullptr};
    Grid root,page,body,left,head,values,rows,footer,sheet,pair,captions,miniPair,recentFrame;
    Canvas sheetHost;
    ScrollViewer bodyScroll,sheetScroll;
    StackPanel shapes,hexBlock,recent,mini,sheetBody;
    ContentControl bodyGate;
    TextBlock title,currentCaption,newCaption,status,hexNote,miniHex,intensityName;
    Border fresh,miniCurrent,miniNew,hexNoteFrame;
    Button current{nullptr},pick{nullptr},hexCopy{nullptr},more{nullptr},cancel{nullptr},apply{nullptr},sheetClose{nullptr};
    Image moreGlyph{nullptr};
    TextBox search;
    std::array<Button,3> shapeButtons{nullptr,nullptr,nullptr};
    std::array<hstring,3> shapeIds;
    struct Row{Button format{nullptr};Grid names;std::vector<TextBlock> labels;MenuFlyout menu;std::vector<RadioMenuFlyoutItem> items;Border space;std::array<std::shared_ptr<Field>,3> fields;Button copy{nullptr};};
    std::array<Row,3> formRows;
    std::shared_ptr<Field> hex,intensity;
    Canvas wheelBox,arc;
    Image image;
    Shapes::Path arcTrack;
    std::vector<Shapes::Line> ramp;
    Shapes::Ellipse markerShadow,marker;
    WheelImage drawing;
    J layout;
    double side=0,stageSize=0,inset=0;
    hstring layoutKey,paintKey,arcKey;
    bool drawQueued=false;
    std::optional<uint32_t> wheelPointer,arcPointer;
    uint32_t wheelPart=0;
    struct Scrub{std::shared_ptr<Field> field;uint32_t pointer=0;double y=0;bool active=false;};
    std::optional<Scrub> scrub;
    uint64_t colorListener=0,sheetGeneration=0,recentGeneration=0;
    Animation::Storyboard slide{nullptr};
    TranslateTransform sheetShift;
    Imaging::WriteableBitmap checker{nullptr};
    hstring checkerKey;
    XamlRoot::Changed_revoker resized;

    hstring copy(wchar_t const* key)const{return data->caption(L"color",key);}
    J panelView()const{return object(view,L"panel");}
    double scale()const{return root.XamlRoot()?root.XamlRoot().RasterizationScale():1.;}

    std::optional<hstring> request(std::optional<J> action){
        auto value=O({{L"type",S(L"editor")},{L"editor",editor},{L"display_space",S(L"Srgb")},{L"rendition",rendition}});
        if(action)value.Insert(L"action",*action);
        auto next=colorUi(data->localization.get(),value).GetObject();
        if(!next.HasKey(L"editor"))return str(next,L"error");
        editor=object(next,L"editor");view=object(next,L"view");
        auto failure=next.GetNamedValue(L"error",JsonValue::CreateNullValue());
        if(failure.ValueType()==JsonValueType::String)return failure.GetString();
        return std::nullopt;
    }
    bool act(J const& action,J const& target=J{}){
        std::optional<hstring> failure;
        try{failure=request(action);}catch(hresult_error const& e){failure=e.message();}
        if(failure||!errorTarget.Size()||errorTarget.Stringify()==target.Stringify()){
            error=failure;errorTarget=failure?target:J{};refused=failure?std::optional<J>(action):std::nullopt;
        }
        render();return !failure;
    }

    void init(XamlRoot const& xaml,bool canPick){
        auto weak=weak_from_this();
        dialog=ContentDialog();dialog.XamlRoot(xaml);
        dialog.RequestedTheme(data->theme()==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        dialog.Resources().Insert(box_value(L"ContentDialogMaxWidth"),box_value(760.));
        dialog.Resources().Insert(box_value(L"ContentDialogMinWidth"),box_value(300.));
        dialog.Resources().Insert(box_value(L"ContentDialogMaxHeight"),box_value(100000.));
        dialog.Resources().Insert(box_value(L"ContentDialogPadding"),box_value(Thickness{0,0,0,0}));
        dialog.Background(data->brush(L"dialog"));dialog.Content(root);
        AutomationProperties::SetAutomationId(dialog,L"edit-color-dialog");
        inheritLanguage(root,data);
        for(auto height:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){RowDefinition row;row.Height(height);root.RowDefinitions().Append(row);}
        buildBody(canPick);buildFooter();buildSheet();
        page.Children().Append(bodyScroll);page.Children().Append(sheetHost);root.Children().Append(page);
        Grid::SetRow(footer,1);root.Children().Append(footer);
        page.SizeChanged([weak](auto&&,SizeChangedEventArgs const& e){if(auto self=weak.lock()){
            auto size=e.NewSize();self->sheet.Width(size.Width);self->sheet.Height(size.Height);
            RectangleGeometry clip;clip.Rect({0,0,size.Width,size.Height});self->page.Clip(clip);
            if(!self->sheetOpen)self->sheetShift.Y(size.Height);
        }});
        root.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock())self->key(e);});
        resized=xaml.Changed(auto_revoke,[weak](auto&&,auto&&){if(auto self=weak.lock())self->fit();});
        dialog.Closing([weak](auto&&,ContentDialogClosingEventArgs const& e){if(auto self=weak.lock())self->closingRequested(e);});
        dialog.Closed([weak](auto&&,auto&&){if(auto self=weak.lock()){self->data->popup(false);if(!self->picking)self->finish();}});
        dialog.Opened([weak](auto&&,auto&&){if(auto self=weak.lock()){self->fit();self->queueDraw();if(self->pick.Visibility()==Visibility::Visible&&self->seenPicker)self->pick.Focus(FocusState::Programmatic);}});
        drawing.worker=std::make_shared<FieldWorker>();drawing.ringWorker=std::make_shared<FieldWorker>();
        drawing.worker->deliver=[weak](FieldResult result){if(auto self=weak.lock()){self->drawing.ready=std::move(result);self->paintKey=L"";self->queueDraw();}};
        drawing.ringWorker->deliver=[weak](FieldResult result){if(auto self=weak.lock()){self->drawing.ringReady=std::move(result);self->paintKey=L"";self->queueDraw();}};
        colorListener=data->colorView([weak]{if(auto self=weak.lock())self->refreshPicking();});
        data->copyView([weak]{if(auto self=weak.lock();self&&!self->finished){self->relocalize();return true;}return false;});
        relabel();fit();render();fillRecent();
    }

    void buildBody(bool canPick){
        auto weak=weak_from_this();
        title.FontFamily(FontFamily(L"Segoe UI"));title.FontWeight(winrt::Windows::UI::Text::FontWeights::Bold());title.FontSize(data->textSize()*1.05);
        title.HorizontalAlignment(HorizontalAlignment::Center);title.Foreground(data->brush(L"text"));AutomationProperties::SetAutomationId(title,L"edit-color-title");
        AutomationProperties::SetHeadingLevel(title,Automation::Peers::AutomationHeadingLevel::Level1);
        buildWheel();
        shapes.Orientation(Orientation::Horizontal);shapes.Spacing(2);shapes.Padding({4,4,4,4});shapes.CornerRadius({17,17,17,17});
        shapes.Background(data->tint(L"text",15));shapes.HorizontalAlignment(HorizontalAlignment::Center);
        auto choices=array(view,L"shapes");
        for(uint32_t i=0;i<3&&i<choices.Size();++i){
            auto choice=choices.GetObjectAt(i);auto shape=str(choice,L"shape");shapeIds[i]=shape;
            auto node=button(data,str(choice,L"label"),[weak,shape]{if(auto self=weak.lock())self->act(O({{L"op",S(L"wheel")},{L"action",O({{L"op",S(L"shape")},{L"shape",S(shape)}})}}));});
            StackPanel content;content.Orientation(Orientation::Horizontal);content.Spacing(4);
            content.Children().Append(icon(L"color-"+shape,data->theme()));
            TextBlock text;text.Text(str(choice,L"label"));text.FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());text.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(text);
            node.Content(content);node.Height(26);node.Padding({8,0,8,0});node.CornerRadius({13,13,13,13});
            AutomationProperties::SetAutomationId(node,L"edit-color-shape-"+shape);shapes.Children().Append(node);shapeButtons[i]=node;
        }
        left.RowSpacing(6);left.VerticalAlignment(VerticalAlignment::Top);for(int i=0;i<2;++i){RowDefinition row;row.Height(GridLength{1,GridUnitType::Auto});left.RowDefinitions().Append(row);}
        left.Children().Append(wheelBox);Grid::SetRow(shapes,1);left.Children().Append(shapes);
        for(int column=0;column<2;++column){ColumnDefinition definition;definition.Width({54,GridUnitType::Pixel});pair.ColumnDefinitions().Append(definition);}
        pair.CornerRadius({6,6,6,6});pair.BorderThickness({1,1,1,1});pair.BorderBrush(data->tint(L"text",31));
        current=button(data,L"",[weak]{if(auto self=weak.lock())self->act(O({{L"op",S(L"revert")}}));});
        current.Height(48);current.HorizontalAlignment(HorizontalAlignment::Stretch);current.CornerRadius({0,0,0,0});
        current.Resources().Insert(box_value(L"ButtonBackgroundPointerOver"),nullptr);current.Resources().Insert(box_value(L"ButtonBackgroundPressed"),nullptr);
        AutomationProperties::SetAutomationId(current,L"edit-color-current");
        fresh.Height(48);Grid::SetColumn(fresh,1);AutomationProperties::SetAutomationId(fresh,L"edit-color-new");
        pair.Children().Append(current);pair.Children().Append(fresh);
        for(int column=0;column<2;++column){ColumnDefinition definition;definition.Width({54,GridUnitType::Pixel});captions.ColumnDefinitions().Append(definition);}
        for(auto const& [text,column]:{std::pair{currentCaption,0},std::pair{newCaption,1}}){
            text.FontSize(data->textSize()*.82);text.Opacity(.6);text.TextAlignment(TextAlignment::Center);text.Foreground(data->brush(L"text"));
            Grid::SetColumn(text,column);captions.Children().Append(text);
        }
        pick=button(data,L"",[weak]{if(auto self=weak.lock())self->startPick();});
        pick.Width(48);pick.Height(48);pick.Background(buttonBackground(data));pick.Content(icon(L"eyedropper",data->theme(),24));
        pick.Visibility(canPick?Visibility::Visible:Visibility::Collapsed);AutomationProperties::SetAutomationId(pick,L"edit-color-pick");
        auto hexSize=data->textSize()*1.6;
        hex=field(O({{L"kind",S(L"hex")}}),L"edit-color-hex",textWidth(L"#DDDDDD",hexSize,winrt::Windows::UI::Text::FontWeights::Medium())+12);
        hex->text.FontSize(hexSize);hex->text.FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());hex->text.TextAlignment(TextAlignment::Left);hex->text.HorizontalAlignment(HorizontalAlignment::Left);
        hex->entry.FontSize(hexSize);hex->entry.FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());hex->entry.TextAlignment(TextAlignment::Left);hex->show.MinHeight(40);hex->entry.MinHeight(40);
        hexNote.FontSize(data->textSize()*.8);hexNote.Foreground(data->brush(L"text"));hexNoteFrame.Child(hexNote);hexNoteFrame.Padding({5,1,5,1});hexNoteFrame.CornerRadius({4,4,4,4});
        hexNoteFrame.Background(data->tint(L"text",20));hexNoteFrame.VerticalAlignment(VerticalAlignment::Center);AutomationProperties::SetAutomationId(hexNoteFrame,L"edit-color-hex-note");
        hexCopy=copyButton(L"edit-color-hex-copy",[weak]{auto self=weak.lock();return self?str(self->view,L"hex"):hstring{};});
        hexBlock.Orientation(Orientation::Horizontal);hexBlock.Spacing(4);hexBlock.HorizontalAlignment(HorizontalAlignment::Right);hexBlock.VerticalAlignment(VerticalAlignment::Center);
        for(FrameworkElement part:{FrameworkElement(hexNoteFrame),FrameworkElement(hex->cell),FrameworkElement(hexCopy)})hexBlock.Children().Append(part);
        for(auto width:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star}}){ColumnDefinition definition;definition.Width(width);head.ColumnDefinitions().Append(definition);}
        for(int i=0;i<2;++i)head.RowDefinitions().Append(RowDefinition());
        head.ColumnSpacing(12);head.RowSpacing(2);
        Grid::SetColumn(pick,1);Grid::SetColumn(hexBlock,2);Grid::SetRow(captions,1);
        for(FrameworkElement part:{FrameworkElement(pair),FrameworkElement(pick),FrameworkElement(hexBlock),FrameworkElement(captions)})head.Children().Append(part);
        buildRows();
        status.TextWrapping(TextWrapping::Wrap);status.Foreground(fill(Invalid));status.FontSize(data->textSize()*.9);status.Margin({0,6,0,0});
        status.Visibility(Visibility::Collapsed);AutomationProperties::SetAutomationId(status,L"edit-color-error");
        AutomationProperties::SetLiveSetting(status,Automation::Peers::AutomationLiveSetting::Polite);
        for(int i=0;i<2;++i)values.RowDefinitions().Append(RowDefinition());
        Grid::SetRow(status,1);values.Children().Append(rows);values.Children().Append(status);
        body.Padding({20,20,20,20});body.ColumnSpacing(24);body.RowSpacing(14);
        for(FrameworkElement part:{FrameworkElement(title),FrameworkElement(left),FrameworkElement(head),FrameworkElement(values)})body.Children().Append(part);
        bodyGate.Content(body);bodyGate.IsTabStop(false);bodyGate.HorizontalContentAlignment(HorizontalAlignment::Stretch);bodyGate.VerticalContentAlignment(VerticalAlignment::Stretch);
        bodyScroll.Content(bodyGate);bodyScroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);bodyScroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);
        arrange(false);
    }

    void buildWheel(){
        auto weak=weak_from_this();
        wheelBox.Background(clear());image.Stretch(Stretch::Fill);image.IsHitTestVisible(false);
        AutomationProperties::SetAutomationId(image,L"edit-color-wheel");AutomationProperties::SetName(image,copy(L"wheel"));
        wheelBox.Children().Append(image);
        arc.Visibility(Visibility::Collapsed);arcTrack.Stroke(clear());arcTrack.StrokeStartLineCap(PenLineCap::Round);arcTrack.StrokeEndLineCap(PenLineCap::Round);
        arc.Children().Append(arcTrack);markerShadow.IsHitTestVisible(false);marker.IsHitTestVisible(false);
        markerShadow.Stroke(fill({128,0,0,0}));markerShadow.StrokeThickness(4);marker.Stroke(fill({255,255,255,255}));marker.StrokeThickness(2);
        arc.Children().Append(markerShadow);arc.Children().Append(marker);wheelBox.Children().Append(arc);
        AutomationProperties::SetAutomationId(arcTrack,L"edit-color-intensity-arc");
        wheelBox.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->wheelDown(e);});
        wheelBox.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->wheelPointer==e.Pointer().PointerId()){self->wheelPick(e.GetCurrentPoint(self->wheelBox).Position());e.Handled(true);}});
        auto end=[weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->wheelPointer==e.Pointer().PointerId()){self->wheelPointer.reset();self->wheelBox.ReleasePointerCaptures();}};
        wheelBox.PointerReleased(end);wheelBox.PointerCanceled(end);wheelBox.PointerCaptureLost(end);
        arcTrack.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->arc);
            if(self->arcPointer||(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed()))return;
            if(!self->arcTrack.CapturePointer(e.Pointer()))return;
            self->arcPointer=p.PointerId();self->arcPick(p.Position());e.Handled(true);
        }});
        arcTrack.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->arcPointer==e.Pointer().PointerId()){self->arcPick(e.GetCurrentPoint(self->arc).Position());e.Handled(true);}});
        auto release=[weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->arcPointer==e.Pointer().PointerId()){self->arcPointer.reset();self->arcTrack.ReleasePointerCaptures();}};
        arcTrack.PointerReleased(release);arcTrack.PointerCanceled(release);arcTrack.PointerCaptureLost(release);
    }
    void wheelDown(PointerRoutedEventArgs const& e){
        auto p=e.GetCurrentPoint(wheelBox);
        if(wheelPointer||side<1||(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed()))return;
        auto shape=str(panelView(),L"shape");auto at=p.Position();
        auto part=capy_color_hit(at.X,at.Y,float(side),shape==L"circle"?2:shape==L"triangle"?1:0);
        if(!part||!wheelBox.CapturePointer(e.Pointer()))return;
        wheelPointer=p.PointerId();wheelPart=part;wheelPick(at);e.Handled(true);
    }
    void wheelPick(Point at){
        A point;point.Append(N(at.X));point.Append(N(at.Y));
        act(O({{L"op",S(L"wheel")},{L"action",O({{L"op",S(L"pick_wheel")},{L"part",S(wheelPart==1?L"hue":L"field")},{L"point",point},{L"size",N(side)}})}}));
    }
    void arcPick(Point at){
        A point;point.Append(N(at.X));point.Append(N(at.Y));
        auto hit=colorUi(data->localization.get(),O({{L"type",S(L"arc")},{L"size",N(stageSize)},{L"point",point}})).GetObject();
        auto fraction=hit.GetNamedValue(L"fraction",JsonValue::CreateNullValue());
        if(fraction.ValueType()!=JsonValueType::Number)return;
        act(O({{L"op",S(L"wheel")},{L"action",O({{L"op",S(L"hdr_intensity")},{L"stops",N(std::round((-2+8*fraction.GetNumber())*100)/100)}})}}),O({{L"kind",S(L"intensity")}}));
    }

    void buildRows(){
        auto weak=weak_from_this();
        auto cellWidth=textWidth(L"000000",data->textSize(),winrt::Windows::UI::Text::FontWeights::Normal())+8;
        for(auto width:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Auto}}){
            ColumnDefinition definition;definition.Width(width);rows.ColumnDefinitions().Append(definition);
        }
        for(int i=0;i<4;++i){RowDefinition row;row.Height(GridLength{1,GridUnitType::Auto});rows.RowDefinitions().Append(row);}
        rows.ColumnSpacing(4);rows.RowSpacing(2);
        for(int32_t row=0;row<3;++row){
            auto& r=formRows[row];
            Grid content;content.ColumnSpacing(4);
            for(auto width:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){ColumnDefinition definition;definition.Width(width);content.ColumnDefinitions().Append(definition);}
            auto chevron=icon(L"chevron-down",data->theme(),12);chevron.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(chevron,1);
            content.Children().Append(r.names);content.Children().Append(chevron);
            r.format=button(data,L"",[]{});r.format.Content(content);r.format.MinHeight(28);r.format.Padding({6,0,6,0});
            r.format.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());r.format.HorizontalContentAlignment(HorizontalAlignment::Stretch);
            AutomationProperties::SetAutomationId(r.format,L"edit-color-form-"+to_hstring(row));
            r.format.Flyout(r.menu);
            Grid::SetRow(r.format,row);rows.Children().Append(r.format);
            TextBlock space;space.FontSize(data->textSize()*.8);space.Foreground(data->brush(L"text"));r.space.Child(space);r.space.Padding({5,1,5,1});r.space.CornerRadius({4,4,4,4});
            r.space.Background(data->tint(L"text",20));r.space.HorizontalAlignment(HorizontalAlignment::Left);r.space.VerticalAlignment(VerticalAlignment::Center);
            AutomationProperties::SetAutomationId(r.space,L"edit-color-space-"+to_hstring(row));
            Grid::SetRow(r.space,row);Grid::SetColumn(r.space,1);rows.Children().Append(r.space);
            for(int32_t index=0;index<3;++index){
                auto f=field(O({{L"kind",S(L"value")},{L"row",N(row)},{L"index",N(index)}}),L"edit-color-"+to_hstring(row)+L"-"+to_hstring(index),cellWidth);
                Grid::SetRow(f->cell,row);Grid::SetColumn(f->cell,2+index);rows.Children().Append(f->cell);r.fields[index]=f;
            }
            r.copy=copyButton(L"edit-color-copy-"+to_hstring(row),[weak,row]{auto self=weak.lock();return self?str(array(self->view,L"rows").GetObjectAt(row),L"copy"):hstring{};});
            Grid::SetRow(r.copy,row);Grid::SetColumn(r.copy,5);rows.Children().Append(r.copy);
        }
        intensityName.Foreground(data->brush(L"text"));intensityName.VerticalAlignment(VerticalAlignment::Center);intensityName.Margin({6,0,0,0});
        Grid::SetRow(intensityName,3);Grid::SetColumnSpan(intensityName,2);rows.Children().Append(intensityName);
        intensity=field(O({{L"kind",S(L"intensity")}}),L"edit-color-intensity",textWidth(L"000000000",data->textSize(),winrt::Windows::UI::Text::FontWeights::Normal())+8);
        Grid::SetRow(intensity->cell,3);Grid::SetColumn(intensity->cell,2);Grid::SetColumnSpan(intensity->cell,3);intensity->cell.HorizontalAlignment(HorizontalAlignment::Right);
        rows.Children().Append(intensity->cell);
    }

    std::shared_ptr<Field> field(J target,hstring const& id,double width){
        auto weak=weak_from_this();auto f=std::make_shared<Field>();f->target=std::move(target);
        f->show=button(data,L"",[]{});f->show.Width(width);f->show.MinHeight(28);f->show.Padding({4,0,4,0});
        f->show.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());f->show.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        f->text.TextAlignment(TextAlignment::Right);f->text.FontFamily(FontFamily(L"Segoe UI"));f->text.FontSize(data->textSize());f->text.Foreground(data->brush(L"text"));
        f->text.VerticalAlignment(VerticalAlignment::Center);f->show.Content(f->text);AutomationProperties::SetAutomationId(f->show,id);
        f->entry.Width(width);f->entry.MinWidth(0);f->entry.MinHeight(28);f->entry.Padding({4,2,4,2});f->entry.TextAlignment(TextAlignment::Right);f->entry.MaxLength(256);
        f->entry.FontSize(data->textSize());f->entry.BorderThickness({2,2,2,2});f->entry.BorderBrush(accent(data));f->entry.Visibility(Visibility::Collapsed);
        f->entry.IsSpellCheckEnabled(false);f->entry.IsTextPredictionEnabled(false);AutomationProperties::SetAutomationId(f->entry,id+L"-entry");
        f->cell.Children().Append(f->show);f->cell.Children().Append(f->entry);
        std::weak_ptr<Field> owner=f;
        f->show.Click([weak,owner](auto&&,auto&&){auto self=weak.lock();auto f=owner.lock();if(!self||!f)return;if(std::exchange(f->suppress,false))return;self->begin(f);});
        f->entry.KeyDown([weak,owner](auto&&,KeyRoutedEventArgs const& e){
            auto self=weak.lock();auto f=owner.lock();if(!self||!f||composingKey(e))return;
            if(e.Key()==VirtualKey::Enter){e.Handled(true);self->commit(f);}
            else if(e.Key()==VirtualKey::Escape){e.Handled(true);self->end(f,true);}
        });
        f->entry.LostFocus([weak,owner](auto&&,auto&&){auto self=weak.lock();auto f=owner.lock();if(self&&f&&!self->picking&&!self->closing&&!self->finished)self->commit(f);});
        if(!f->numeric())return f;
        f->show.KeyDown([weak,owner](auto&&,KeyRoutedEventArgs const& e){
            auto self=weak.lock();auto f=owner.lock();if(!self||!f)return;
            if(e.Key()!=VirtualKey::Up&&e.Key()!=VirtualKey::Down)return;
            e.Handled(true);
            if(self->act(O({{L"op",S(L"scrub")},{L"target",f->target},{L"pixels",N(e.Key()==VirtualKey::Up?2:-2)},{L"speed",S(speed())}}),f->target))
                self->act(O({{L"op",S(L"end_scrub")},{L"cancel",B(false)}}),f->target);
        });
        f->show.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak,owner](auto&&,PointerRoutedEventArgs const& e){
            auto self=weak.lock();auto f=owner.lock();if(!self||!f||self->scrub)return;
            auto p=e.GetCurrentPoint(self->root);
            if(p.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse&&!p.Properties().IsLeftButtonPressed())return;
            f->suppress=false;self->scrub=Scrub{f,p.PointerId(),p.Position().Y,false};f->show.CapturePointer(e.Pointer());
        })),true);
        f->show.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){
            auto self=weak.lock();if(!self||!self->scrub||self->scrub->pointer!=e.Pointer().PointerId())return;
            double pixels=self->scrub->y-e.GetCurrentPoint(self->root).Position().Y;
            if(!self->scrub->active&&std::abs(pixels)<ScrubSlop)return;
            self->scrub->active=true;self->scrub->field->suppress=true;e.Handled(true);
            self->act(O({{L"op",S(L"scrub")},{L"target",self->scrub->field->target},{L"pixels",N(pixels)},{L"speed",S(speed())}}),self->scrub->field->target);
        })),true);
        auto finishScrub=[weak](PointerRoutedEventArgs const& e,bool cancel){
            auto self=weak.lock();if(!self||!self->scrub||self->scrub->pointer!=e.Pointer().PointerId())return;
            auto ended=*self->scrub;self->scrub.reset();
            if(!ended.active)return;
            if(cancel)ended.field->suppress=false;
            self->act(O({{L"op",S(L"end_scrub")},{L"cancel",B(cancel)}}),ended.field->target);
        };
        f->show.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([finishScrub](auto&&,PointerRoutedEventArgs const& e){finishScrub(e,false);})),true);
        f->show.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler([finishScrub](auto&&,PointerRoutedEventArgs const& e){finishScrub(e,true);})),true);
        f->show.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([finishScrub](auto&&,PointerRoutedEventArgs const& e){finishScrub(e,e.GetCurrentPoint(nullptr).IsInContact());})),true);
        return f;
    }
    Button copyButton(hstring const& id,std::function<hstring()> text){
        auto weak=weak_from_this();
        auto node=button(data,copy(L"copy"),[]{});node.Width(28);node.Height(28);node.Content(icon(L"copy",data->theme()));
        AutomationProperties::SetAutomationId(node,id);tooltip(node,copy(L"copy"));
        node.Click([weak,text,owner=make_weak(node)](auto&&,auto&&){
            auto self=weak.lock();auto node=owner.get();if(!self||!node)return;
            copyText(text());node.Content(icon(L"check",self->data->theme()));
            auto done=self->copy(L"copied");AutomationProperties::SetName(node,done);tooltip(node,done);
            auto timer=node.DispatcherQueue().CreateTimer();timer.IsRepeating(false);timer.Interval(std::chrono::milliseconds(1200));
            timer.Tick([weak,owner](auto&& sender,auto&&){
                sender.Stop();auto self=weak.lock();auto node=owner.get();if(!self||!node)return;
                node.Content(icon(L"copy",self->data->theme()));auto label=self->copy(L"copy");AutomationProperties::SetName(node,label);tooltip(node,label);
            });
            timer.Start();
        });
        return node;
    }

    void buildFooter(){
        auto weak=weak_from_this();
        for(auto width:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Auto}}){
            ColumnDefinition definition;definition.Width(width);footer.ColumnDefinitions().Append(definition);
        }
        footer.ColumnSpacing(8);footer.Padding({20,16,20,20});footer.BorderThickness({0,1,0,0});footer.BorderBrush(data->tint(L"text",31));
        recent.Orientation(Orientation::Horizontal);recent.Spacing(4);
        recentFrame.Children().Append(recent);recentFrame.VerticalAlignment(VerticalAlignment::Center);
        RectangleGeometry clip;recentFrame.Clip(clip);
        recentFrame.SizeChanged([](auto&& sender,SizeChangedEventArgs const& e){
            RectangleGeometry clip;clip.Rect({0,0,e.NewSize().Width,e.NewSize().Height});sender.template as<Grid>().Clip(clip);
        });
        for(int column=0;column<2;++column){ColumnDefinition definition;definition.Width({18,GridUnitType::Pixel});miniPair.ColumnDefinitions().Append(definition);}
        miniCurrent.Height(22);miniNew.Height(22);Grid::SetColumn(miniNew,1);miniPair.CornerRadius({4,4,4,4});
        miniPair.Children().Append(miniCurrent);miniPair.Children().Append(miniNew);
        miniHex.Foreground(data->brush(L"text"));miniHex.VerticalAlignment(VerticalAlignment::Center);miniHex.FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());
        mini.Orientation(Orientation::Horizontal);mini.Spacing(8);mini.VerticalAlignment(VerticalAlignment::Center);
        mini.Children().Append(miniPair);mini.Children().Append(miniHex);mini.Visibility(Visibility::Collapsed);
        footer.Children().Append(recentFrame);footer.Children().Append(mini);
        more=button(data,copy(L"all_swatches"),[weak]{if(auto self=weak.lock())self->showSheet(!self->sheetOpen);});
        more.Width(34);more.Height(34);moreGlyph=icon(L"chevron-down",data->theme());moreGlyph.RenderTransformOrigin({.5f,.5f});
        RotateTransform turn;turn.Angle(180);moreGlyph.RenderTransform(turn);more.Content(moreGlyph);
        AutomationProperties::SetAutomationId(more,L"edit-color-swatches");Grid::SetColumn(more,1);footer.Children().Append(more);
        cancel=button(data,data->common(L"cancel"),[weak]{if(auto self=weak.lock())self->close(false);});
        cancel.MinHeight(34);cancel.Padding({16,0,16,0});cancel.Background(buttonBackground(data));cancel.FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());
        AutomationProperties::SetAutomationId(cancel,L"edit-color-cancel");Grid::SetColumn(cancel,2);footer.Children().Append(cancel);
        apply=button(data,copy(L"use_color"),[weak]{if(auto self=weak.lock();self&&self->apply.IsEnabled())self->close(true);});
        apply.MinHeight(34);apply.Padding({16,0,16,0});apply.Background(accent(data));apply.Foreground(data->brush(L"accent_foreground"));
        apply.FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());
        apply.Resources().Insert(box_value(L"ButtonBackgroundPointerOver"),accent(data));apply.Resources().Insert(box_value(L"ButtonForegroundPointerOver"),data->brush(L"accent_foreground"));
        apply.Resources().Insert(box_value(L"ButtonBackgroundPressed"),accent(data));apply.Resources().Insert(box_value(L"ButtonForegroundPressed"),data->brush(L"accent_foreground"));
        AutomationProperties::SetAutomationId(apply,L"edit-color-apply");Grid::SetColumn(apply,3);footer.Children().Append(apply);
    }

    void buildSheet(){
        auto weak=weak_from_this();
        sheet.Background(data->brush(L"settings"));sheet.Padding({20,20,20,0});sheet.RowSpacing(10);sheet.RenderTransform(sheetShift);
        sheet.Visibility(Visibility::Collapsed);AutomationProperties::SetAutomationId(sheet,L"edit-color-sheet");
        for(auto height:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star}}){RowDefinition row;row.Height(height);sheet.RowDefinitions().Append(row);}
        Grid head;head.ColumnSpacing(6);
        for(auto width:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){ColumnDefinition definition;definition.Width(width);head.ColumnDefinitions().Append(definition);}
        search.MinHeight(34);search.MaxLength(256);search.IsSpellCheckEnabled(false);AutomationProperties::SetAutomationId(search,L"edit-color-search");
        search.TextChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating){
            try{if(!self->request(O({{L"op",S(L"search")},{L"text",S(self->search.Text())}})))self->fillSheet();}catch(hresult_error const&){}
        }});
        sheetClose=button(data,copy(L"close_swatches"),[weak]{if(auto self=weak.lock())self->showSheet(false);});
        sheetClose.Width(34);sheetClose.Height(34);sheetClose.Content(icon(L"chevron-down",data->theme()));
        AutomationProperties::SetAutomationId(sheetClose,L"edit-color-sheet-close");Grid::SetColumn(sheetClose,1);
        head.Children().Append(search);head.Children().Append(sheetClose);sheet.Children().Append(head);
        sheetBody.Spacing(14);sheetBody.Padding({0,0,0,16});sheetScroll.Content(sheetBody);sheetScroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);
        Grid::SetRow(sheetScroll,1);sheet.Children().Append(sheetScroll);
        sheetHost.Children().Append(sheet);
    }

    void ensureChecker(double size){
        auto palette=object(data->state,L"palette");
        auto next=str(palette,L"checker_light")+str(palette,L"checker_dark")+to_hstring(scale());
        if(checker&&next==checkerKey)return;checkerKey=next;
        checker=checkerBitmap(size,scale(),color(str(palette,L"checker_light")),color(str(palette,L"checker_dark")));
    }
    Button tile(J const& entry,hstring const& id,double size){
        auto weak=weak_from_this();auto detail=str(entry,L"detail");auto value=object(entry,L"color");
        auto node=button(data,detail,[weak,value]{if(auto self=weak.lock())self->act(O({{L"op",S(L"color")},{L"color",value}}));});
        node.Width(size);node.Height(size);node.Padding({3,3,3,3});node.CornerRadius({6,6,6,6});
        node.HorizontalContentAlignment(HorizontalAlignment::Stretch);node.VerticalContentAlignment(VerticalAlignment::Stretch);
        ensureChecker(SheetTile);Border patch,paint;patch.CornerRadius({5,5,5,5});paint.CornerRadius({5,5,5,5});
        ImageBrush pixels;pixels.ImageSource(checker);pixels.Stretch(Stretch::None);pixels.AlignmentX(AlignmentX::Left);pixels.AlignmentY(AlignmentY::Top);patch.Background(pixels);
        paint.Background(fill(previewColor(array(entry,L"rgba"))));patch.Child(paint);node.Content(patch);
        if(flag(entry,L"current")){node.BorderBrush(accent(data));node.BorderThickness({2,2,2,2});}
        tooltip(node,detail);AutomationProperties::SetAutomationId(node,id);
        return node;
    }
    void fillRecent(){
        auto weak=weak_from_this();auto generation=++recentGeneration;
        QueryWorkspace(data->query,O({{L"type",S(L"swatch_sheet")},{L"query",S(L"")},{L"current",object(view,L"value")}}),[weak,generation](J reply){
            auto self=weak.lock();if(!self||self->finished||generation!=self->recentGeneration)return;
            self->recent.Children().Clear();
            for(auto section:array(object(reply,L"result"),L"sections")){
                auto value=section.GetObject();if(value.GetNamedValue(L"palette",JsonValue::CreateNullValue()).ValueType()!=JsonValueType::Null)continue;
                uint32_t index=0;for(auto entry:array(value,L"tiles"))self->recent.Children().Append(self->tile(entry.GetObject(),L"edit-color-recent-"+to_hstring(index++),RecentTile));
            }
        });
    }
    void fillSheet(){
        auto weak=weak_from_this();auto generation=++sheetGeneration;
        auto query=str(object(object(editor,L"picker"),L"editor"),L"search");
        QueryWorkspace(data->query,O({{L"type",S(L"swatch_sheet")},{L"query",S(query)},{L"current",object(view,L"value")}}),[weak,generation](J reply){
            auto self=weak.lock();if(!self||self->finished||generation!=self->sheetGeneration)return;
            self->sheetBody.Children().Clear();auto shown=object(reply,L"result");
            auto empty=shown.GetNamedValue(L"empty",JsonValue::CreateNullValue());
            if(empty.ValueType()==JsonValueType::String){
                auto note=label(self->data,empty.GetString(),false,false);note.TextWrapping(TextWrapping::Wrap);note.Opacity(.6);
                AutomationProperties::SetAutomationId(note,L"edit-color-sheet-empty");self->sheetBody.Children().Append(note);
            }
            uint32_t index=0;
            for(auto value:array(shown,L"sections")){
                auto section=value.GetObject();StackPanel block;block.Spacing(6);
                StackPanel heading;heading.Orientation(Orientation::Horizontal);heading.Spacing(8);
                heading.Children().Append(label(self->data,str(section,L"title"),false,false));
                auto count=label(self->data,str(section,L"count"),false,false);count.Opacity(.6);heading.Children().Append(count);
                block.Children().Append(heading);
                VariableSizedWrapGrid tiles;tiles.Orientation(Orientation::Horizontal);tiles.ItemWidth(SheetTile+4);tiles.ItemHeight(SheetTile+4);
                for(auto entry:array(section,L"tiles"))tiles.Children().Append(self->tile(entry.GetObject(),L"edit-color-sheet-tile-"+to_hstring(index++),SheetTile));
                auto palette=section.GetNamedValue(L"palette",JsonValue::CreateNullValue());
                if(flag(section,L"can_add")&&palette.ValueType()==JsonValueType::Number){
                    auto id=palette.GetNumber();auto text=self->data->caption(L"palettes",L"add_current");
                    auto add=button(self->data,text,[weak,id]{if(auto self=weak.lock())self->store(id);});
                    add.Width(SheetTile-6);add.Height(SheetTile-6);add.Margin({3,3,3,3});add.Background(self->data->tint(L"text",15));add.Content(icon(L"plus",self->data->theme()));
                    tooltip(add,text);AutomationProperties::SetAutomationId(add,L"edit-color-add-"+to_hstring(uint64_t(id)));tiles.Children().Append(add);
                }
                block.Children().Append(tiles);self->sheetBody.Children().Append(block);
            }
        });
    }
    void store(double palette){
        data->dispatch(O({{L"type",S(L"color")},{L"action",O({{L"op",S(L"library")},{L"action",O({{L"op",S(L"store")},{L"palette",N(palette)},{L"name",S(L"")},{L"color",object(view,L"value")}})}})}}));
        libraryChanged=true;
    }
    bool libraryChanged=false;
    void showSheet(bool open){
        if(sheetOpen==open)return;sheetOpen=open;
        bodyGate.IsEnabled(!open);bodyScroll.IsHitTestVisible(!open);
        recentFrame.Visibility(open?Visibility::Collapsed:Visibility::Visible);mini.Visibility(open?Visibility::Visible:Visibility::Collapsed);
        moreGlyph.RenderTransform().as<RotateTransform>().Angle(open?0:180);relabel();
        if(open){
            updating=true;search.Text(str(object(object(editor,L"picker"),L"editor"),L"search"));updating=false;
            fillSheet();sheetScroll.ChangeView(nullptr,0.,nullptr,true);sheet.Visibility(Visibility::Visible);
        }
        animateSheet(open);
        if(open)search.Focus(FocusState::Programmatic);else more.Focus(FocusState::Programmatic);
    }
    void animateSheet(bool open){
        if(slide)slide.Stop();
        double height=page.ActualHeight(),target=open?0:height;
        if(!winrt::Windows::UI::ViewManagement::UISettings().AnimationsEnabled()||height<=0){
            sheetShift.Y(target);if(!open)sheet.Visibility(Visibility::Collapsed);return;
        }
        Animation::DoubleAnimation motion;motion.From(sheetShift.Y());motion.To(target);motion.Duration(DurationHelper::FromTimeSpan(std::chrono::milliseconds(250)));
        Animation::CubicEase ease;ease.EasingMode(Animation::EasingMode::EaseOut);motion.EasingFunction(ease);
        slide=Animation::Storyboard();Animation::Storyboard::SetTarget(motion,sheetShift);Animation::Storyboard::SetTargetProperty(motion,L"Y");
        slide.Children().Append(motion);
        slide.Completed([weak=weak_from_this(),open](auto&&,auto&&){if(auto self=weak.lock()){self->sheetShift.Y(open?0:self->page.ActualHeight());if(!open&&!self->sheetOpen)self->sheet.Visibility(Visibility::Collapsed);}});
        slide.Begin();
    }

    void arrange(bool narrowLayout){
        narrow=narrowLayout;body.ColumnDefinitions().Clear();body.RowDefinitions().Clear();
        if(narrow){
            ColumnDefinition column;column.Width({1,GridUnitType::Star});body.ColumnDefinitions().Append(column);
            for(int i=0;i<4;++i)body.RowDefinitions().Append(RowDefinition());
            Grid::SetColumnSpan(title,1);Grid::SetRow(head,1);Grid::SetColumn(head,0);Grid::SetRow(left,2);Grid::SetColumn(left,0);Grid::SetRowSpan(left,1);
            Grid::SetRow(values,3);Grid::SetColumn(values,0);left.HorizontalAlignment(HorizontalAlignment::Center);
        }else{
            for(auto width:{GridLength{WheelSide,GridUnitType::Pixel},GridLength{1,GridUnitType::Star}}){ColumnDefinition column;column.Width(width);body.ColumnDefinitions().Append(column);}
            for(auto height:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star}}){RowDefinition row;row.Height(height);body.RowDefinitions().Append(row);}
            Grid::SetColumnSpan(title,2);Grid::SetRow(left,1);Grid::SetColumn(left,0);Grid::SetRowSpan(left,2);
            Grid::SetRow(head,1);Grid::SetColumn(head,1);Grid::SetRow(values,2);Grid::SetColumn(values,1);left.HorizontalAlignment(HorizontalAlignment::Stretch);
        }
        values.VerticalAlignment(VerticalAlignment::Top);
    }
    void fit(){
        auto xaml=root.XamlRoot();if(!xaml)return;auto size=xaml.Size();
        double width=std::max(280.,std::min(720.,double(size.Width)-24));
        root.Width(width);root.MaxHeight(std::max(240.,double(size.Height)-24));
        bool next=width<NarrowWidth;if(next!=narrow)arrange(next);
        queueDraw();
    }

    void relabel(){
        auto edit=copy(L"edit");title.Text(edit);AutomationProperties::SetName(dialog,edit);AutomationProperties::SetName(root,edit);
        currentCaption.Text(copy(L"current"));newCaption.Text(copy(L"new"));
        AutomationProperties::SetName(current,copy(L"current"));tooltip(current,copy(L"current"));AutomationProperties::SetName(fresh,copy(L"new"));
        AutomationProperties::SetName(pick,copy(L"pick_canvas"));tooltip(pick,copy(L"pick_canvas"));
        for(auto const& node:{hexCopy,formRows[0].copy,formRows[1].copy,formRows[2].copy}){AutomationProperties::SetName(node,copy(L"copy"));tooltip(node,copy(L"copy"));}
        for(auto const& row:formRows)tooltip(row.format,copy(L"format"));
        intensityName.Text(copy(L"intensity_ev"));
        cancel.Content(box_value(data->common(L"cancel")));AutomationProperties::SetName(cancel,data->common(L"cancel"));
        apply.Content(box_value(copy(L"use_color")));AutomationProperties::SetName(apply,copy(L"use_color"));
        search.PlaceholderText(copy(L"swatch_search"));AutomationProperties::SetName(search,copy(L"swatch_search"));
        AutomationProperties::SetName(sheetClose,copy(L"close_swatches"));tooltip(sheetClose,copy(L"close_swatches"));
        auto swatches=sheetOpen?copy(L"close_swatches"):copy(L"all_swatches");AutomationProperties::SetName(more,swatches);tooltip(more,swatches);
        AutomationProperties::SetName(image,copy(L"wheel"));AutomationProperties::SetName(arcTrack,copy(L"intensity"));
    }
    void relocalize(){
        relabel();
        try{request(std::nullopt);if(refused){if(auto failure=request(*refused))error=failure;}}catch(hresult_error const& e){error=e.message();}
        render();fillRecent();if(sheetOpen)fillSheet();
    }

    static void setField(Field& f,hstring const& text,hstring const& edit,hstring const& name){
        f.edit=edit;f.name=name;
        if(f.text.Text()!=text)f.text.Text(text);
        AutomationProperties::SetName(f.show,name+L" "+text);AutomationProperties::SetName(f.entry,name);
    }
    std::vector<std::shared_ptr<Field>> fields()const{
        std::vector<std::shared_ptr<Field>> all{hex};
        for(auto const& row:formRows)for(auto const& f:row.fields)all.push_back(f);
        all.push_back(intensity);return all;
    }
    void render(){
        updating=true;
        auto choices=array(view,L"shapes");
        for(uint32_t i=0;i<3&&i<choices.Size();++i){
            auto choice=choices.GetObjectAt(i);bool on=flag(choice,L"selected");
            shapeButtons[i].Background(on?data->brush(L"header_selection"):clear());
            AutomationProperties::SetItemStatus(shapeButtons[i],on?data->caption(L"search",L"selected"):L"");
            AutomationProperties::SetName(shapeButtons[i],str(choice,L"label"));tooltip(shapeButtons[i],str(choice,L"name"));
            AutomationProperties::SetHelpText(shapeButtons[i],str(choice,L"name"));
        }
        auto currentColor=displayColor(object(view,L"current")),newColor=displayColor(object(view,L"new"));
        current.Background(fill(currentColor));fresh.Background(fill(newColor));miniCurrent.Background(fill(currentColor));miniNew.Background(fill(newColor));
        setField(*hex,str(view,L"hex"),str(view,L"hex"),copy(L"hex"));miniHex.Text(str(view,L"hex"));
        auto note=object(view,L"hex_note");hexNoteFrame.Visibility(note.Size()?Visibility::Visible:Visibility::Collapsed);
        if(note.Size()){hexNote.Text(str(note,L"text"));auto tip=str(note,L"tip");if(!tip.empty())tooltip(hexNoteFrame,tip);AutomationProperties::SetName(hexNoteFrame,tip.empty()?str(note,L"text"):tip);}
        auto shownRows=array(view,L"rows");
        for(uint32_t row=0;row<3&&row<shownRows.Size();++row){
            auto item=shownRows.GetObjectAt(row);auto& r=formRows[row];auto forms=array(item,L"forms");
            AutomationProperties::SetName(r.format,str(item,L"label"));
            while(r.labels.size()<forms.Size()){
                TextBlock name;name.Foreground(data->brush(L"text"));name.VerticalAlignment(VerticalAlignment::Center);r.names.Children().Append(name);r.labels.push_back(name);
                RadioMenuFlyoutItem choice;choice.GroupName(L"edit-color-form-"+to_hstring(row));auto index=uint32_t(r.items.size());
                choice.RegisterPropertyChangedCallback(RadioMenuFlyoutItem::IsCheckedProperty(),[weak=weak_from_this(),row,index,owner=make_weak(choice)](auto&&,auto&&){
                    auto self=weak.lock();auto item=owner.get();if(!self||!item||self->updating||!item.IsChecked())return;
                    auto options=array(array(self->view,L"rows").GetObjectAt(row),L"forms");
                    if(index<options.Size())self->act(O({{L"op",S(L"form")},{L"row",N(row)},{L"form",S(str(options.GetObjectAt(index),L"form"))}}));
                });
                r.menu.Items().Append(choice);r.items.push_back(choice);
            }
            for(uint32_t i=0;i<forms.Size();++i){
                auto choice=forms.GetObjectAt(i);bool on=str(choice,L"form")==str(item,L"form");
                r.labels[i].Text(str(choice,L"label"));r.labels[i].Opacity(on?1:0);
                r.items[i].Text(str(choice,L"label"));r.items[i].IsChecked(on);
                AutomationProperties::SetAutomationId(r.items[i],L"edit-color-form-"+to_hstring(row)+L"-"+str(choice,L"form"));
            }
            auto space=item.GetNamedValue(L"space",JsonValue::CreateNullValue());
            r.space.Visibility(space.ValueType()==JsonValueType::String?Visibility::Visible:Visibility::Collapsed);
            if(space.ValueType()==JsonValueType::String)r.space.Child().as<TextBlock>().Text(space.GetString());
            auto entries=array(item,L"values");
            for(uint32_t i=0;i<3&&i<entries.Size();++i){auto value=entries.GetObjectAt(i);setField(*r.fields[i],str(value,L"text"),str(value,L"edit"),str(value,L"name"));}
        }
        auto stops=object(view,L"intensity");bool hdr=stops.Size()!=0;
        intensityName.Visibility(hdr?Visibility::Visible:Visibility::Collapsed);intensity->cell.Visibility(intensityName.Visibility());
        if(hdr)setField(*intensity,str(stops,L"text"),str(stops,L"edit"),str(stops,L"name"));
        status.Text(error.value_or(L""));status.Visibility(error?Visibility::Visible:Visibility::Collapsed);
        bool blocked=false;
        for(auto const& f:fields()){
            bool invalid=error&&errorTarget.Size()&&errorTarget.Stringify()==f->target.Stringify();
            f->entry.BorderBrush(invalid?fill(Invalid):accent(data));
            blocked|=invalid&&f->editing();blocked|=f->editing()&&textComposing(f->entry);
        }
        apply.IsEnabled(!blocked);apply.Opacity(blocked?.5:1);
        updating=false;
        queueDraw();
    }

    void begin(std::shared_ptr<Field> const& f){
        f->entry.Text(f->edit);f->entry.Visibility(Visibility::Visible);f->show.Visibility(Visibility::Collapsed);
        f->entry.Focus(FocusState::Programmatic);f->entry.SelectAll();
    }
    void end(std::shared_ptr<Field> const& f,bool cancel){
        if(!f->editing())return;
        f->entry.Visibility(Visibility::Collapsed);f->show.Visibility(Visibility::Visible);
        if(errorTarget.Size()&&errorTarget.Stringify()==f->target.Stringify()){error.reset();errorTarget=J{};refused.reset();}
        render();if(cancel||!closing)f->show.Focus(FocusState::Programmatic);
    }
    void commit(std::shared_ptr<Field> const& f){
        if(!f->editing()||textComposing(f->entry))return;
        auto text=f->entry.Text();
        if(text==f->edit){end(f,false);return;}
        auto kind=str(f->target,L"kind");
        auto action=kind==L"hex"?O({{L"op",S(L"text")},{L"text",S(text)}})
            :kind==L"intensity"?O({{L"op",S(L"intensity")},{L"text",S(text)}})
            :O({{L"op",S(L"value")},{L"row",N(num(f->target,L"row"))},{L"index",N(num(f->target,L"index"))},{L"text",S(text)}});
        if(act(action,f->target))end(f,false);
        else f->entry.Focus(FocusState::Programmatic);
    }
    std::shared_ptr<Field> editingField()const{for(auto const& f:fields())if(f->editing())return f;return nullptr;}

    void key(KeyRoutedEventArgs const& e){
        auto control=held(VK_CONTROL);
        if(!control||(e.Key()!=VirtualKey::C&&e.Key()!=VirtualKey::V))return;
        if(auto focused=FocusManager::GetFocusedElement(root.XamlRoot());focused&&focused.try_as<TextBox>())return;
        e.Handled(true);
        if(e.Key()==VirtualKey::C){copyText(str(view,L"hex"));return;}
        auto content=winrt::Windows::ApplicationModel::DataTransfer::Clipboard::GetContent();
        if(!content.Contains(winrt::Windows::ApplicationModel::DataTransfer::StandardDataFormats::Text()))return;
        auto weak=weak_from_this();
        content.GetTextAsync().Completed([weak,dispatcher=root.DispatcherQueue()](auto const& operation,auto status){
            if(status!=winrt::Windows::Foundation::AsyncStatus::Completed)return;
            auto text=operation.GetResults();
            dispatcher.TryEnqueue([weak,text]{if(auto self=weak.lock();self&&!self->finished)self->act(O({{L"op",S(L"text")},{L"text",S(text)}}));});
        });
    }
    void closingRequested(ContentDialogClosingEventArgs const& e){
        if(closing)return;
        if(auto f=editingField()){e.Cancel(true);end(f,true);return;}
        if(sheetOpen){
            e.Cancel(true);
            auto focused=FocusManager::GetFocusedElement(root.XamlRoot());
            if(focused&&focused.try_as<TextBox>()==search&&!search.Text().empty())search.Text(L"");else showSheet(false);
        }
    }

    void queueDraw(){
        if(std::exchange(drawQueued,true))return;
        root.DispatcherQueue().TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock()){self->drawQueued=false;self->draw();}});
    }
    void draw(){
        if(finished||!view.Size())return;
        auto panel=panelView();bool hdr=flag(panel,L"hdr");
        double next=WheelSide;
        auto nextLayout=to_hstring(next)+(hdr?L"/hdr":L"");
        if(nextLayout!=layoutKey){
            layoutKey=nextLayout;side=next;stageSize=side+28;arcKey=L"";
            layout=colorUi(data->localization.get(),O({{L"type",S(L"layout")},{L"size",N(stageSize)},{L"hdr",B(hdr)}})).GetObject();
            inset=array(layout,L"wheel").GetNumberAt(1);double bottom=inset+side;
            if(hdr){
                auto start=colorUi(data->localization.get(),O({{L"type",S(L"arc")},{L"size",N(stageSize)},{L"fraction",N(0)}})).GetObject();
                auto end=colorUi(data->localization.get(),O({{L"type",S(L"arc")},{L"size",N(stageSize)},{L"fraction",N(1)}})).GetObject();
                auto geometry=object(start,L"geometry");double radius=num(geometry,L"radius"),width=num(geometry,L"width"),markerRadius=num(geometry,L"marker_radius");
                bottom=array(geometry,L"center").GetNumberAt(1)+radius+std::max(width/2,markerRadius)+2;
                PathFigure figure;auto from=array(start,L"point"),to=array(end,L"point");
                figure.StartPoint({float(from.GetNumberAt(0)),float(from.GetNumberAt(1))});figure.IsClosed(false);figure.IsFilled(false);
                ArcSegment segment;segment.Point({float(to.GetNumberAt(0)),float(to.GetNumberAt(1))});segment.Size({float(radius),float(radius)});
                segment.SweepDirection(SweepDirection::Counterclockwise);figure.Segments().Append(segment);
                PathGeometry track;track.Figures().Append(figure);arcTrack.Data(track);arcTrack.StrokeThickness(width);
                auto path=array(start,L"path");size_t segments=path.Size()?path.Size()-1:0;
                while(ramp.size()>segments){uint32_t at;if(arc.Children().IndexOf(ramp.back(),at))arc.Children().RemoveAt(at);ramp.pop_back();}
                while(ramp.size()<segments){Shapes::Line line;line.StrokeStartLineCap(PenLineCap::Round);line.StrokeEndLineCap(PenLineCap::Round);line.IsHitTestVisible(false);arc.Children().InsertAt(uint32_t(ramp.size()),line);ramp.push_back(line);}
                for(uint32_t i=1;i<path.Size();i++){auto const& line=ramp[i-1];auto a=path.GetArrayAt(i-1),b=path.GetArrayAt(i);line.X1(a.GetNumberAt(0));line.Y1(a.GetNumberAt(1));line.X2(b.GetNumberAt(0));line.Y2(b.GetNumberAt(1));line.StrokeThickness(width);}
                markerShadow.Width(markerRadius*2+4);markerShadow.Height(markerRadius*2+4);marker.Width(markerRadius*2+2);marker.Height(markerRadius*2+2);
            }
            wheelBox.Width(side);wheelBox.Height(bottom-inset);image.Width(side);image.Height(side);
            Canvas::SetLeft(arc,-inset);Canvas::SetTop(arc,-inset);arc.Width(stageSize);arc.Height(bottom);
        }
        arc.Visibility(hdr?Visibility::Visible:Visibility::Collapsed);
        if(hdr){
            double stops=num(panel,L"intensity");auto ramps=array(panel,L"intensity_ramp");auto markerColor=array(panel,L"marker_color");
            auto nextArc=to_hstring(stops)+ramps.Stringify()+markerColor.Stringify();
            if(nextArc!=arcKey){
                arcKey=nextArc;
                for(size_t i=0;i<ramp.size()&&i<ramps.Size();i++)ramp[i].Stroke(fill(previewColor(ramps.GetArrayAt(uint32_t(i)))));
                auto point=array(colorUi(data->localization.get(),O({{L"type",S(L"arc")},{L"size",N(stageSize)},{L"fraction",N((stops+2)/8)}})).GetObject(),L"point");
                for(auto const& dot:{markerShadow,marker}){Canvas::SetLeft(dot,point.GetNumberAt(0)-dot.Width()/2);Canvas::SetTop(dot,point.GetNumberAt(1)-dot.Height()/2);}
                marker.Fill(fill(previewColor(markerColor)));
                wchar_t text[32];swprintf(text,32,L"%.2f EV",stops);AutomationProperties::SetItemStatus(arcTrack,text);
            }
        }
        auto nextPaint=panel.Stringify()+L"/"+to_hstring(scale());
        if(nextPaint==paintKey)return;
        try{
            try{drawing.draw(image,panel,object(editor,L"picker"),side,scale());}
            catch(hresult_error const& exception){
                auto code=exception.code();
                if(code!=DXGI_ERROR_DEVICE_REMOVED&&code!=DXGI_ERROR_DEVICE_RESET&&code!=D2DERR_RECREATE_TARGET&&code!=E_SURFACE_CONTENTS_LOST)throw;
                drawing.draw(image,panel,object(editor,L"picker"),side,scale(),true);
            }
            paintKey=nextPaint;AutomationProperties::SetItemStatus(image,data->caption(L"header",L"ready"));
        }catch(hresult_error const&){AutomationProperties::SetItemStatus(image,copy(L"wheel_failed"));}
    }

    void startPick(){
        if(!data->colorStrip)return;
        picking=true;seenPicker=false;pickRevision=num(data->state,L"revision");data->colorStrip->corner=L"top_right";
        closing=true;dialog.Hide();
        data->dispatch(O({{L"type",S(L"color_picker")},{L"action",O({{L"kind",S(L"editor")},{L"original",object(view,L"value")},{L"touch_offset",N(44*scale())}})}}));
    }
    void refreshPicking(){
        if(libraryChanged){libraryChanged=false;fillRecent();if(sheetOpen)fillSheet();}
        if(!picking||finished)return;
        auto picker=object(data->colorPreview,L"picker");
        if(flag(picker,L"editor")){
            seenPicker=true;
            auto preview=picker.GetNamedValue(L"preview",JsonValue::CreateNullValue());
            auto sample=preview.ValueType()==JsonValueType::Object?preview.GetObject():object(view,L"value");
            try{
                auto shown=colorUi(data->localization.get(),O({{L"type",S(L"editor_strip")},{L"editor",editor},{L"sample",sample}})).GetObject();
                A colors;colors.Append(object(view,L"value"));colors.Append(sample);
                auto previews=colorUi(data->localization.get(),O({{L"type",S(L"preview")},{L"colors",colors},{L"document_space",S(str(panelView(),L"rgb_space",L"Srgb"))},{L"display_space",S(L"Srgb")},{L"rendition",rendition}})).GetArray();
                std::wstring joined;for(auto value:array(shown,L"values")){if(!joined.empty())joined+=L"  ";joined+=value.GetString().c_str();}
                auto stops=shown.GetNamedValue(L"intensity",JsonValue::CreateNullValue());
                data->colorStrip->Show(O({{L"original",array(previews.GetObjectAt(0),L"rgba")},{L"sample",array(previews.GetObjectAt(1),L"rgba")},
                    {L"hex",S(str(shown,L"hex"))},{L"intensity",S(stops.ValueType()==JsonValueType::String?stops.GetString():hstring{})},
                    {L"label",S(str(shown,L"label"))},{L"values",S(hstring(joined))}}));
            }catch(hresult_error const&){}
            return;
        }
        if(!seenPicker&&num(data->state,L"revision")<=pickRevision)return;
        picking=false;data->colorStrip->Hide();
        auto picked=picker.GetNamedValue(L"picked",JsonValue::CreateNullValue());
        if(seenPicker&&picked.ValueType()==JsonValueType::Object)act(O({{L"op",S(L"color")},{L"color",picked.GetObject()}}));
        present();
    }
    void present(){
        closing=false;data->popup(true);
        try{dialog.ShowAsync();}catch(hresult_error const&){data->popup(false);finish();}
    }
    void close(bool accept){
        if(accept){if(auto f=editingField())commit(f);if(!apply.IsEnabled())return;}
        accepting=accept;closing=true;dialog.Hide();
    }
    void finish(){
        if(std::exchange(finished,true))return;
        if(picking&&data->colorStrip)data->colorStrip->Hide();
        if(colorListener)data->colorViews.erase(colorListener);
        for(auto const& worker:{drawing.ringWorker,drawing.worker})if(worker){worker->deliver=nullptr;worker->cancel();}
        resized.revoke();
        auto memoryNow=object(object(editor,L"picker"),L"editor");
        if(memoryNow.Stringify()!=memory)data->dispatch(O({{L"type",S(L"color")},{L"action",O({{L"op",S(L"editor_memory")},{L"memory",memoryNow}})}}));
        if(accepting&&num(object(data->state,L"document_file"),L"epoch")==epoch&&accepted){
            auto stops=view.GetNamedValue(L"stops",JsonValue::CreateNullValue());
            accepted(object(view,L"value"),stops.ValueType()==JsonValueType::Number?std::optional<double>(stops.GetNumber()):std::nullopt);
        }
        auto& open=openEditors();auto self=shared_from_this();
        open.erase(std::remove(open.begin(),open.end(),self),open.end());
    }
};
}

void CapyUi::EditColor(std::shared_ptr<WorkspaceData> const& data,UIElement const& owner,J const& target,bool opaque,ColorAccepted accepted){
    auto xaml=owner.XamlRoot();if(!xaml)return;
    auto rendition=object(data->model,L"color_panel").GetNamedValue(L"rendition",JsonValue::CreateNullValue());
    auto request=O({{L"type",S(L"editor_open")},{L"colors",displayColors(data->state)},{L"opaque",B(opaque)},{L"display_space",S(L"Srgb")},{L"rendition",rendition}});
    for(auto const& [key,value]:target)request.Insert(key,value);
    J opened;
    try{opened=colorUi(data->localization.get(),request).GetObject();}catch(hresult_error const&){return;}
    if(!opened.HasKey(L"editor"))return;
    auto editor=std::make_shared<Editor>();editor->data=data;editor->accepted=std::move(accepted);editor->rendition=rendition;
    editor->editor=object(opened,L"editor");editor->view=object(opened,L"view");
    editor->memory=object(object(editor->editor,L"picker"),L"editor").Stringify();
    editor->epoch=num(object(data->state,L"document_file"),L"epoch");
    editor->init(xaml,data->colorStrip!=nullptr);
    openEditors().push_back(editor);
    editor->present();
}
