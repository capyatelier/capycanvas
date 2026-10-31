#include "pch.h"
#include "ColorEditor.h"
#include "PointerGesture.h"
#include "PointerScrub.h"
#include "ColorStrip.h"
#include "ColorWheel.h"
#include "ColorIntensity.h"
#include "Checker.h"
#include "WorkspaceQuery.h"
#include "PickerDialog.h"
#include <winrt/Microsoft.UI.Xaml.Media.Animation.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Windows.ApplicationModel.DataTransfer.h>

using namespace CapyUi;
using namespace CapyWheel;
namespace {
namespace Animation=winrt::Microsoft::UI::Xaml::Media::Animation;
using winrt::Windows::Foundation::Point;
using winrt::Windows::System::VirtualKey;
constexpr double RecentTile=34,SheetTile=40,NarrowWidth=700,ScrubSlop=4;

bool held(int key){return (GetKeyState(key)&0x8000)!=0;}
hstring speed(bool shift,bool fine){return shift?L"fast":fine?L"fine":L"normal";}
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
    std::shared_ptr<PointerClick> clicks;
    bool editing()const{return entry.Visibility()==Visibility::Visible;}
    bool numeric()const{return str(target,L"kind")!=L"hex";}
};

struct Editor;
std::vector<std::shared_ptr<Editor>>& openEditors(){thread_local std::vector<std::shared_ptr<Editor>> editors;return editors;}

struct Editor:std::enable_shared_from_this<Editor>{
    std::shared_ptr<WorkspaceData> data;
    ColorAccepted accepted;
    std::function<void(J const&)> viewed;
    std::function<void(bool)> blockedChanged;
    std::optional<hstring> clearLabel;
    hstring viewedKey;
    double hexWidth=0;
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
    std::shared_ptr<ColorIntensity> intensityArc;
    WheelImage drawing;
    double side=0,stageSize=0,inset=0;
    hstring layoutKey,paintKey;
    bool drawQueued=false;
    std::optional<uint32_t> wheelPointer,arcPointer;
    uint32_t wheelPart=0;
    struct Scrub{std::shared_ptr<Field> field;uint32_t pointer=0;PointerScrub motion{0};bool active=false;};
    std::optional<Scrub> scrub;
    uint64_t colorListener=0,sheetGeneration=0,recentGeneration=0;
    Animation::Storyboard slide{nullptr};
    TranslateTransform sheetShift;
    Imaging::WriteableBitmap checker{nullptr};
    hstring checkerKey;
    XamlRoot::Changed_revoker resized;

    hstring copy(wchar_t const* key)const{return data->caption(L"color",key);}
    J panelView()const{return object(view,L"panel");}
    double scale()const{return body.XamlRoot()?body.XamlRoot().RasterizationScale():1.;}

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
        dialog=pickerDialog(data,xaml,root,page,footer,L"edit-color-dialog");
        buildBody(canPick);buildFooter();buildSheet();
        page.Children().Append(bodyScroll);page.Children().Append(sheetHost);
        page.SizeChanged([weak](auto&&,SizeChangedEventArgs const& e){if(auto self=weak.lock()){
            auto size=e.NewSize();self->sheet.Width(size.Width);self->sheet.Height(size.Height);
            RectangleGeometry clip;clip.Rect({0,0,size.Width,size.Height});self->page.Clip(clip);
            if(!self->sheetOpen)self->sheetShift.Y(size.Height);self->queueDraw();
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

    void embed(){
        auto weak=weak_from_this();
        buildBody(false);
        title.Visibility(Visibility::Collapsed);current.Visibility(Visibility::Collapsed);captions.Visibility(Visibility::Collapsed);
        pair.ColumnDefinitions().GetAt(0).Width({0,GridUnitType::Pixel});
        hexCopy.Visibility(Visibility::Collapsed);for(auto const& row:formRows)row.copy.Visibility(Visibility::Collapsed);
        Grid::SetColumn(hexBlock,1);hexBlock.HorizontalAlignment(HorizontalAlignment::Left);
        body.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock())self->key(e);});
        drawing.worker=std::make_shared<FieldWorker>();drawing.ringWorker=std::make_shared<FieldWorker>();
        drawing.worker->deliver=[weak](FieldResult result){if(auto self=weak.lock()){self->drawing.ready=std::move(result);self->paintKey=L"";self->queueDraw();}};
        drawing.ringWorker->deliver=[weak](FieldResult result){if(auto self=weak.lock()){self->drawing.ringReady=std::move(result);self->paintKey=L"";self->queueDraw();}};
        data->copyView([weak]{if(auto self=weak.lock();self&&!self->finished){self->relocalize();return true;}return false;});
        viewedKey=object(view,L"value").Stringify();
        relabel();render();
    }

    void buildBody(bool canPick){
        auto weak=weak_from_this();
        pickerTitle(data,title,L"edit-color-title");
        buildWheel();
        shapes.Orientation(Orientation::Horizontal);shapes.Spacing(2);shapes.Padding({4,4,4,4});shapes.CornerRadius({18*CornerFit,18*CornerFit,18*CornerFit,18*CornerFit});
        shapes.Background(data->tint(L"text",15));shapes.HorizontalAlignment(HorizontalAlignment::Center);
        auto choices=array(view,L"shapes");
        for(uint32_t i=0;i<3&&i<choices.Size();++i){
            auto choice=choices.GetObjectAt(i);auto shape=str(choice,L"shape");shapeIds[i]=shape;
            auto node=button(data,str(choice,L"label"),[weak,shape]{if(auto self=weak.lock())self->act(O({{L"op",S(L"wheel")},{L"action",O({{L"op",S(L"shape")},{L"shape",S(shape)}})}}));});
            StackPanel content;content.Orientation(Orientation::Horizontal);content.Spacing(4);
            content.Children().Append(icon(L"color-"+shape,data->theme()));
            TextBlock text;text.Text(str(choice,L"label"));text.FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());text.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(text);
            node.Content(content);node.Height(26);node.Padding({8,0,8,0});node.CornerRadius(data->controlCorner());
            AutomationProperties::SetAutomationId(node,L"edit-color-shape-"+shape);shapes.Children().Append(node);shapeButtons[i]=node;
        }
        left.RowSpacing(6);left.VerticalAlignment(VerticalAlignment::Top);for(int i=0;i<2;++i){RowDefinition row;row.Height(GridLength{1,GridUnitType::Auto});left.RowDefinitions().Append(row);}
        left.Children().Append(wheelBox);Grid::SetRow(shapes,1);left.Children().Append(shapes);
        framePair(pair,54);
        current=button(data,L"",[weak]{if(auto self=weak.lock())self->act(O({{L"op",S(L"revert")}}));});
        current.Height(48);current.HorizontalAlignment(HorizontalAlignment::Stretch);current.CornerRadius({0,0,0,0});
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
        hexWidth=textWidth(L"#DDDDDD",hexSize,winrt::Windows::UI::Text::FontWeights::Medium())+12;
        hex=field(O({{L"kind",S(L"hex")}}),L"edit-color-hex",hexWidth);
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
        status.TextWrapping(TextWrapping::Wrap);status.Foreground(data->brush(L"error"));status.FontSize(data->textSize());status.Margin({0,6,0,0});
        status.Visibility(Visibility::Collapsed);AutomationProperties::SetAutomationId(status,L"edit-color-error");
        AutomationProperties::SetLiveSetting(status,Automation::Peers::AutomationLiveSetting::Polite);
        for(int i=0;i<2;++i)values.RowDefinitions().Append(RowDefinition());
        Grid::SetRow(status,1);values.Children().Append(rows);values.Children().Append(status);
        body.Padding({PickerPadding,PickerPadding,PickerPadding,PickerPadding});body.ColumnSpacing(24);body.RowSpacing(14);
        for(FrameworkElement part:{FrameworkElement(title),FrameworkElement(left),FrameworkElement(head),FrameworkElement(values)})body.Children().Append(part);
        values.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->queueDraw();});
        bodyGate.Content(body);bodyGate.IsTabStop(false);bodyGate.HorizontalContentAlignment(HorizontalAlignment::Stretch);bodyGate.VerticalContentAlignment(VerticalAlignment::Stretch);
        bodyScroll.Content(bodyGate);bodyScroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);bodyScroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);mouseScrollbars(bodyScroll);bodyScroll.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->queueDraw();});
        arrange(false);
    }

    void buildWheel(){
        auto weak=weak_from_this();
        wheelBox.Background(clear());image.Stretch(Stretch::Fill);image.IsHitTestVisible(false);
        AutomationProperties::SetAutomationId(image,L"edit-color-wheel");AutomationProperties::SetName(image,copy(L"wheel"));
        wheelBox.Children().Append(image);
        intensityArc=std::make_shared<ColorIntensity>();intensityArc->data=data;
        intensityArc->changed=[weak](double stops){if(auto self=weak.lock())self->setIntensity(stops);};
        intensityArc->init(L"edit-color-intensity-range");arc=intensityArc->root;arcTrack=intensityArc->track;arc.Visibility(Visibility::Collapsed);wheelBox.Children().Append(arc);
        AutomationProperties::SetAutomationId(arcTrack,L"edit-color-intensity-arc");
        wheelBox.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->wheelDown(e);});
        wheelBox.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->wheelPointer==e.Pointer().PointerId()){self->wheelPick(e.GetCurrentPoint(self->wheelBox).Position());e.Handled(true);}});
        auto end=[weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->wheelPointer==e.Pointer().PointerId()){self->wheelPointer.reset();self->wheelBox.ReleasePointerCaptures();}};
        wheelBox.PointerReleased(end);wheelBox.PointerCanceled(end);wheelBox.PointerCaptureLost(end);
        arcTrack.PointerPressed([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto p=e.GetCurrentPoint(self->arc);
            if(self->arcPointer||!PointerGesture::PrimaryContact(p))return;
            if(!self->arcTrack.CapturePointer(e.Pointer()))return;
            self->arcPointer=p.PointerId();self->arcPick(p.Position());e.Handled(true);
        }});
        arcTrack.PointerMoved([weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->arcPointer==e.Pointer().PointerId()){self->arcPick(e.GetCurrentPoint(self->arc).Position());e.Handled(true);}});
        auto release=[weak](auto&&,PointerRoutedEventArgs const& e){if(auto self=weak.lock();self&&self->arcPointer==e.Pointer().PointerId()){self->arcPointer.reset();self->arcTrack.ReleasePointerCaptures();}};
        arcTrack.PointerReleased(release);arcTrack.PointerCanceled(release);arcTrack.PointerCaptureLost(release);
    }
    void wheelDown(PointerRoutedEventArgs const& e){
        auto p=e.GetCurrentPoint(wheelBox);
        if(wheelPointer||side<1||!PointerGesture::PrimaryContact(p))return;
        auto shape=str(panelView(),L"shape");auto at=p.Position();
        auto part=capy_color_hit(at.X,at.Y,float(side),shape==L"circle"?2:shape==L"triangle"?1:0);
        if(!part||!wheelBox.CapturePointer(e.Pointer()))return;
        wheelPointer=p.PointerId();wheelPart=part;wheelPick(at);e.Handled(true);
    }
    void wheelPick(Point at){
        A point;point.Append(N(at.X));point.Append(N(at.Y));
        act(O({{L"op",S(L"wheel")},{L"action",O({{L"op",S(L"pick_wheel")},{L"part",S(wheelPart==1?L"hue":L"field")},{L"point",point},{L"size",N(side)}})}}));
    }
    void setIntensity(double stops){
        act(O({{L"op",S(L"wheel")},{L"action",O({{L"op",S(L"hdr_intensity")},{L"stops",N(stops)}})}}),O({{L"kind",S(L"intensity")}}));
    }
    void arcPick(Point at){setIntensity(intensityArc->pick(at));}

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
        fieldColors(data,f->entry,ControlSurface::Dialog);f->entry.Visibility(Visibility::Collapsed);
        f->entry.IsSpellCheckEnabled(false);f->entry.IsTextPredictionEnabled(false);AutomationProperties::SetAutomationId(f->entry,id+L"-entry");
        f->cell.Children().Append(f->show);f->cell.Children().Append(f->entry);
        std::weak_ptr<Field> owner=f;
        f->show.Click([weak,owner](auto&&,auto&&){auto self=weak.lock();auto f=owner.lock();if(!self||!f)return;if(f->clicks&&f->clicks->SuppressClick())return;self->begin(f);});
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
            if(self->act(O({{L"op",S(L"scrub")},{L"target",f->target},{L"pixels",N(e.Key()==VirtualKey::Up?2:-2)},{L"speed",S(speed(held(VK_SHIFT),held(VK_CONTROL)||held(VK_MENU)))}}),f->target))
                self->act(O({{L"op",S(L"end_scrub")},{L"cancel",B(false)}}),f->target);
        });
        f->clicks=PointerGesture::Clicks(f->show,[weak](uint32_t id,bool cancel){
            auto self=weak.lock();if(!self||!self->scrub||self->scrub->pointer!=id)return;
            auto ended=*self->scrub;self->scrub.reset();
            if(ended.active)self->act(O({{L"op",S(L"end_scrub")},{L"cancel",B(cancel)}}),ended.field->target);
        });
        f->show.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak,owner](auto&&,PointerRoutedEventArgs const& e){
            auto self=weak.lock();auto f=owner.lock();if(!self||!f||self->scrub)return;
            auto p=e.GetCurrentPoint(self->body);
            if(!f->clicks->Owns(p.PointerId()))return;
            self->scrub=Scrub{f,p.PointerId(),PointerScrub{p.Position().Y}};
        })),true);
        f->show.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](auto&&,PointerRoutedEventArgs const& e){
            auto self=weak.lock();if(!self||!self->scrub||self->scrub->pointer!=e.Pointer().PointerId())return;
            auto pixels=self->scrub->motion.Move(e.GetCurrentPoint(self->body).Position().Y,e.IsGenerated());
            if(!pixels||(!self->scrub->active&&std::abs(*pixels)<ScrubSlop))return;
            self->scrub->active=true;self->scrub->field->clicks->Suppress(e.Pointer().PointerId());e.Handled(true);
            using Mod=winrt::Windows::System::VirtualKeyModifiers;auto modifiers=e.KeyModifiers();
            self->act(O({{L"op",S(L"scrub")},{L"target",self->scrub->field->target},{L"pixels",N(*pixels)},{L"speed",S(speed((modifiers&Mod::Shift)!=Mod::None,(modifiers&(Mod::Control|Mod::Menu))!=Mod::None))}}),self->scrub->field->target);
        })),true);
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

    void framePair(Grid const& frame,double width){
        for(int column=0;column<2;++column){ColumnDefinition definition;definition.Width({width,GridUnitType::Pixel});frame.ColumnDefinitions().Append(definition);}
        frame.CornerRadius(data->controlCorner());clipRounded(frame,data->controlCorner());frame.BorderThickness({1,1,1,1});frame.BorderBrush(data->tint(L"text",31));
    }
    void buildFooter(){
        auto weak=weak_from_this();
        pickerFooter(data,footer,{GridUnitType::Star,GridUnitType::Auto,GridUnitType::Auto,GridUnitType::Auto});
        recent.Orientation(Orientation::Horizontal);recent.Spacing(4);
        recentFrame.Children().Append(recent);recentFrame.VerticalAlignment(VerticalAlignment::Center);
        RectangleGeometry clip;recentFrame.Clip(clip);
        recentFrame.SizeChanged([](auto&& sender,SizeChangedEventArgs const& e){
            RectangleGeometry clip;clip.Rect({0,0,e.NewSize().Width,e.NewSize().Height});sender.template as<Grid>().Clip(clip);
        });
        framePair(miniPair,38);miniPair.VerticalAlignment(VerticalAlignment::Center);AutomationProperties::SetAutomationId(miniPair,L"edit-color-mini-pair");
        AutomationProperties::SetAutomationId(miniCurrent,L"edit-color-mini-current");AutomationProperties::SetAutomationId(miniNew,L"edit-color-mini-new");
        Grid::SetColumn(miniNew,1);
        miniPair.Children().Append(miniCurrent);miniPair.Children().Append(miniNew);
        miniHex.Foreground(data->brush(L"text"));miniHex.VerticalAlignment(VerticalAlignment::Center);miniHex.FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());
        mini.Orientation(Orientation::Horizontal);mini.Spacing(8);mini.VerticalAlignment(VerticalAlignment::Center);
        mini.Children().Append(miniPair);mini.Children().Append(miniHex);mini.Visibility(Visibility::Collapsed);
        footer.Children().Append(recentFrame);footer.Children().Append(mini);
        more=raisedButton(data,copy(L"all_swatches"),[weak]{if(auto self=weak.lock())self->showSheet(!self->sheetOpen);});
        more.Width(34);more.Height(34);moreGlyph=icon(L"chevron-down",data->theme());moreGlyph.RenderTransformOrigin({.5f,.5f});
        RotateTransform turn;turn.Angle(180);moreGlyph.RenderTransform(turn);more.Content(moreGlyph);
        AutomationProperties::SetAutomationId(more,L"edit-color-swatches");Grid::SetColumn(more,1);footer.Children().Append(more);
        cancel=pickerButton(data,L"edit-color-cancel",2,false,[weak]{if(auto self=weak.lock())self->close(false);});footer.Children().Append(cancel);
        miniPair.Height(cancel.MinHeight());
        cancel.SizeChanged([pair=make_weak(miniPair)](auto&&,SizeChangedEventArgs const& e){if(auto frame=pair.get())frame.Height(e.NewSize().Height);});
        apply=pickerButton(data,L"edit-color-apply",3,true,[weak]{if(auto self=weak.lock();self&&self->apply.IsEnabled())self->close(true);});footer.Children().Append(apply);
    }

    void buildSheet(){
        auto weak=weak_from_this();
        sheet.Background(data->brush(L"settings"));sheet.Padding({20,20,20,0});sheet.RowSpacing(10);sheet.RenderTransform(sheetShift);
        sheet.Visibility(Visibility::Collapsed);AutomationProperties::SetAutomationId(sheet,L"edit-color-sheet");
        for(auto height:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star}}){RowDefinition row;row.Height(height);sheet.RowDefinitions().Append(row);}
        Grid head;head.ColumnSpacing(6);
        for(auto width:{GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){ColumnDefinition definition;definition.Width(width);head.ColumnDefinitions().Append(definition);}
        fieldColors(data,search,ControlSurface::Dialog);search.MinHeight(34);search.MaxLength(256);search.IsSpellCheckEnabled(false);AutomationProperties::SetAutomationId(search,L"edit-color-search");
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
        checker=checkerBitmap(std::max(size,64.),scale(),color(str(palette,L"checker_light")),color(str(palette,L"checker_dark")));
    }
    Button tile(J const& entry,hstring const& id,double size){
        auto weak=weak_from_this();auto detail=str(entry,L"detail");auto value=object(entry,L"color");
        auto node=button(data,detail,[weak,value]{if(auto self=weak.lock())self->act(O({{L"op",S(L"color")},{L"color",value}}));});
        node.Width(size);node.Height(size);node.Padding({3,3,3,3});node.CornerRadius(data->controlCorner());
        node.HorizontalContentAlignment(HorizontalAlignment::Stretch);node.VerticalContentAlignment(VerticalAlignment::Stretch);
        ensureChecker(SheetTile);Border patch,paint;patch.CornerRadius(data->controlCorner());paint.CornerRadius(data->controlCorner());
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
            updating=true;updateText(search,str(object(object(editor,L"picker"),L"editor"),L"search"));updating=false;
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
            for(auto width:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star}}){ColumnDefinition column;column.Width(width);body.ColumnDefinitions().Append(column);}
            for(auto height:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star}}){RowDefinition row;row.Height(height);body.RowDefinitions().Append(row);}
            Grid::SetColumnSpan(title,2);Grid::SetRow(left,1);Grid::SetColumn(left,0);Grid::SetRowSpan(left,2);
            Grid::SetRow(head,1);Grid::SetColumn(head,1);Grid::SetRow(values,2);Grid::SetColumn(values,1);left.HorizontalAlignment(HorizontalAlignment::Stretch);
        }
        values.VerticalAlignment(VerticalAlignment::Top);
    }
    void fit(){
        auto width=fitPicker(root);if(!width)return;
        bool next=width<NarrowWidth;if(next!=narrow)arrange(next);
        queueDraw();
    }

    void relabel(){
        layoutKey=L"";
        auto edit=copy(L"edit");
        if(dialog){title.Text(edit);AutomationProperties::SetName(dialog,edit);AutomationProperties::SetName(root,edit);AutomationProperties::SetName(miniPair,edit);}
        currentCaption.Text(copy(L"current"));newCaption.Text(copy(L"new"));
        AutomationProperties::SetName(current,copy(L"current"));tooltip(current,copy(L"current"));AutomationProperties::SetName(fresh,copy(L"new"));
        AutomationProperties::SetName(miniCurrent,copy(L"current"));AutomationProperties::SetName(miniNew,copy(L"new"));
        AutomationProperties::SetName(pick,copy(L"pick_canvas"));tooltip(pick,copy(L"pick_canvas"));
        for(auto const& node:{hexCopy,formRows[0].copy,formRows[1].copy,formRows[2].copy}){AutomationProperties::SetName(node,copy(L"copy"));tooltip(node,copy(L"copy"));}
        for(auto const& row:formRows)tooltip(row.format,copy(L"format"));
        intensityName.Text(copy(L"intensity_ev"));
        AutomationProperties::SetName(image,copy(L"wheel"));AutomationProperties::SetName(arcTrack,copy(L"intensity"));
        if(!dialog)return;
        pickerLabel(cancel,data->common(L"cancel"));pickerLabel(apply,copy(L"use_color"));
        search.PlaceholderText(copy(L"swatch_search"));AutomationProperties::SetName(search,copy(L"swatch_search"));
        AutomationProperties::SetName(sheetClose,copy(L"close_swatches"));tooltip(sheetClose,copy(L"close_swatches"));
        auto swatches=sheetOpen?copy(L"close_swatches"):copy(L"all_swatches");AutomationProperties::SetName(more,swatches);tooltip(more,swatches);
    }
    void relocalize(){
        relabel();
        try{request(std::nullopt);if(refused){if(auto failure=request(*refused))error=failure;}}catch(hresult_error const& e){error=e.message();}
        render();if(dialog)fillRecent();if(sheetOpen)fillSheet();
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
        if(dialog)dialogSurface(data,dialog);
        updating=true;
        auto choices=array(view,L"shapes");
        for(uint32_t i=0;i<3&&i<choices.Size();++i){
            auto choice=choices.GetObjectAt(i);bool on=flag(choice,L"selected");
            shapeButtons[i].Background(on?data->brush(L"header_selection"):clear());
            buttonFeedback(shapeButtons[i],data->brush(L"text"),20,41,on?Brush(data->brush(L"header_selection_hover")):Brush(nullptr));
            AutomationProperties::SetItemStatus(shapeButtons[i],on?data->caption(L"search",L"selected"):L"");
            AutomationProperties::SetName(shapeButtons[i],str(choice,L"label"));tooltip(shapeButtons[i],str(choice,L"name"));
            AutomationProperties::SetHelpText(shapeButtons[i],str(choice,L"name"));
        }
        auto currentColor=displayColor(object(view,L"current")),newColor=displayColor(object(view,L"new"));
        current.Background(fill(currentColor));fresh.Background(fill(newColor));miniCurrent.Background(fill(currentColor));miniNew.Background(fill(newColor));
        setField(*hex,clearLabel.value_or(str(view,L"hex")),str(view,L"hex"),copy(L"hex"));miniHex.Text(str(view,L"hex"));
        hex->text.Opacity(clearLabel?.6:1);left.Opacity(clearLabel?.4:1);values.Opacity(clearLabel?.4:1);
        hex->show.Width(clearLabel?std::max(hexWidth,textWidth(*clearLabel,hex->text.FontSize(),winrt::Windows::UI::Text::FontWeights::Medium())+12):hexWidth);
        if(clearLabel){ensureChecker(SheetTile);ImageBrush pixels;pixels.ImageSource(checker);pixels.Stretch(Stretch::None);pixels.AlignmentX(AlignmentX::Left);pixels.AlignmentY(AlignmentY::Top);fresh.Background(pixels);}
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
            f->entry.BorderBrush().as<SolidColorBrush>().Color(invalid?InvalidFieldColor:Windows::UI::Color{0,0,0,0});
            blocked|=invalid&&f->editing();blocked|=f->editing()&&textComposing(f->entry);
        }
        if(apply){apply.IsEnabled(!blocked);apply.Opacity(blocked?.5:1);}
        if(blockedChanged)blockedChanged(blocked);
        updating=false;
        if(auto value=object(view,L"value");viewed&&value.Stringify()!=viewedKey){viewedKey=value.Stringify();viewed(value);}
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
    bool escape(){if(auto f=editingField()){end(f,true);return true;}return false;}

    void key(KeyRoutedEventArgs const& e){
        auto control=held(VK_CONTROL);
        if(!control||(e.Key()!=VirtualKey::C&&e.Key()!=VirtualKey::V))return;
        if(auto focused=FocusManager::GetFocusedElement(body.XamlRoot());focused&&focused.try_as<TextBox>())return;
        e.Handled(true);
        if(e.Key()==VirtualKey::C){copyText(str(view,L"hex"));return;}
        auto content=winrt::Windows::ApplicationModel::DataTransfer::Clipboard::GetContent();
        if(!content.Contains(winrt::Windows::ApplicationModel::DataTransfer::StandardDataFormats::Text()))return;
        auto weak=weak_from_this();
        content.GetTextAsync().Completed([weak,dispatcher=body.DispatcherQueue()](auto const& operation,auto status){
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
        body.DispatcherQueue().TryEnqueue([weak=weak_from_this()]{if(auto self=weak.lock()){self->drawQueued=false;self->draw();}});
    }
    void draw(){
        if(finished||!view.Size())return;
        auto panel=panelView();bool hdr=flag(panel,L"hdr");
        double width=bodyScroll.ViewportWidth(),height=dialog?root.MaxHeight()-footer.ActualHeight():bodyScroll.ViewportHeight();
        if(width<=0)width=body.ActualWidth();if(width<=0)width=10000;
        width=std::max(0.,width-body.Padding().Left-body.Padding().Right);
        if(!std::isfinite(height)||height<=0)height=10000;
        else {height-=body.Padding().Top+body.Padding().Bottom+title.ActualHeight()+shapes.ActualHeight()+left.RowSpacing()+body.RowSpacing();if(narrow)height-=head.ActualHeight()+values.ActualHeight()+body.RowSpacing()*2;height=std::max(0.,height);}
        auto nextLayout=to_hstring(width)+L"/"+to_hstring(height)+L"/"+to_hstring(scale())+data->language()+(narrow?L"/narrow":L"/wide")+(hdr?L"/hdr":L"");
        if(nextLayout!=layoutKey){
            layoutKey=nextLayout;
            if(!narrow){
                head.Measure({INFINITY,INFINITY});rows.Measure({INFINITY,INFINITY});
                width=std::max(0.,width-std::max(head.DesiredSize().Width,rows.DesiredSize().Width)-body.ColumnSpacing());
            }
            auto bare=colorUi(data->localization.get(),O({{L"type",S(L"bare_layout")},{L"width",N(width)},{L"height",N(height)},{L"hdr",B(hdr)}})).GetObject();
            side=num(bare,L"wheel");stageSize=num(bare,L"stage");inset=num(bare,L"inset");double bottom=num(bare,L"bottom");
            wheelBox.Width(side);wheelBox.Height(bottom-inset);image.Width(side);image.Height(side);
            Canvas::SetLeft(arc,-inset);Canvas::SetTop(arc,-inset);arc.Width(stageSize);arc.Height(bottom);
            paintKey=L"";
        }
        intensityArc->present(panel,stageSize,hdr);
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
std::optional<CapyUi::ColorForm> CapyUi::EmbedColor(std::shared_ptr<WorkspaceData> const& data,J const& color,std::function<void(J const&)> viewed,std::function<void(bool)> blocked){
    auto rendition=object(data->model,L"color_panel").GetNamedValue(L"rendition",JsonValue::CreateNullValue());
    J opened;
    try{opened=colorUi(data->localization.get(),O({{L"type",S(L"editor_open")},{L"colors",displayColors(data->state)},{L"color",color},{L"opaque",B(true)},{L"display_space",S(L"Srgb")},{L"rendition",rendition}})).GetObject();}
    catch(hresult_error const&){return std::nullopt;}
    if(!opened.HasKey(L"editor"))return std::nullopt;
    auto editor=std::make_shared<Editor>();editor->data=data;editor->rendition=rendition;
    editor->editor=object(opened,L"editor");editor->view=object(opened,L"view");
    editor->memory=object(object(editor->editor,L"picker"),L"editor").Stringify();
    editor->viewed=std::move(viewed);editor->blockedChanged=std::move(blocked);
    editor->embed();
    std::weak_ptr<Editor> weak=editor;
    ColorForm form{editor,editor->body,editor->head,{}};
    RowDefinition row;row.Height(GridLength{1,GridUnitType::Auto});editor->values.RowDefinitions().Append(row);
    form.below.Margin({0,8,0,0});Grid::SetRow(form.below,2);editor->values.Children().Append(form.below);
    form.choose=[weak](J const& value){if(auto self=weak.lock())self->act(O({{L"op",S(L"color")},{L"color",value}}));};
    form.transparent=[weak](std::optional<hstring> const& label){if(auto self=weak.lock();self&&self->clearLabel!=label){self->clearLabel=label;self->render();}};
    form.arrange=[weak](bool narrow){if(auto self=weak.lock();self&&self->narrow!=narrow)self->arrange(narrow);};
    form.escape=[weak]{auto self=weak.lock();return self&&self->escape();};
    form.commit=[weak]{auto self=weak.lock();if(!self)return false;if(auto f=self->editingField())self->commit(f);return !self->editingField();};
    form.close=[weak]{if(auto self=weak.lock())self->finish();};
    return form;
}
