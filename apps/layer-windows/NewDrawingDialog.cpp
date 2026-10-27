#include "pch.h"
#include "NewDrawingDialog.h"
#include "WorkflowMotion.h"
#include "ColorEditor.h"
#include "TextureDialog.h"
#include "PickerDialog.h"
#include "Checker.h"
#include "WorkspaceGeometry.h"
#include <winrt/Microsoft.UI.Xaml.Documents.h>
#include <winrt/Microsoft.UI.Xaml.Media.Animation.h>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.Windows.Storage.Pickers.h>
#include <microsoft.ui.xaml.window.h>
#include <ShellScalingApi.h>
#include <array>

namespace CapyUi {
V drawingScreen(Window const& window){
    HWND handle=nullptr;check_hresult(window.as<IWindowNative>()->get_WindowHandle(&handle));
    if(auto monitor=MonitorFromWindow(handle,MONITOR_DEFAULTTONEAREST)){
        MONITORINFO info{};info.cbSize=sizeof(info);UINT dpiX=0,dpiY=0;
        if(GetMonitorInfoW(monitor,&info)){
            A extent;extent.Append(N(info.rcMonitor.right-info.rcMonitor.left));extent.Append(N(info.rcMonitor.bottom-info.rcMonitor.top));
            bool physical=SUCCEEDED(GetDpiForMonitor(monitor,MDT_RAW_DPI,&dpiX,&dpiY))&&dpiX>0;
            return O({{L"extent",extent},{L"ppi",physical?N(dpiX):V(JsonValue::CreateNullValue())}});
        }
    }
    return JsonValue::CreateNullValue();
}
namespace Pickers=winrt::Microsoft::Windows::Storage::Pickers;
namespace {
namespace Animation=winrt::Microsoft::UI::Xaml::Media::Animation;
using Size2=std::array<double,2>;
constexpr double InputHeight=34,ControlGap=6,RowGap=10,SectionGap=14,FieldWidth=92,ValueWidth=64,NarrowWidth=700,ThumbnailSide=36,SwatchSide=40;
constexpr uint32_t PaperColorLimit=64;
constexpr double PresetTile=128,PresetFrameWidth=116,PresetFrameHeight=86,PresetFit=74,SummaryArt=36,TileGap=10;
constexpr Size2 Preview{232,250},PreviewFit{196,214},NarrowPreview{348,140},NarrowFit{312,104};
constexpr winrt::Windows::UI::Color Problem{255,0xe0,0x1b,0x24};

struct SheetArt {
    Grid root;
    Border sheet;
    Canvas grain;
    Size2 fit{0,0};
    double tile=40;
    J shown;
    winrt::Windows::UI::Color paint{};
    hstring grainKey;
    void init(Size2 bounds,Size2 fitted,double grainTile){
        tile=grainTile;resize(bounds,fitted);
        sheet.HorizontalAlignment(HorizontalAlignment::Center);sheet.VerticalAlignment(VerticalAlignment::Center);
        sheet.CornerRadius({2,2,2,2});grain.IsHitTestVisible(false);sheet.Child(grain);
        root.IsHitTestVisible(false);root.Children().Append(sheet);
    }
    void resize(Size2 bounds,Size2 fitted){root.Width(bounds[0]);root.Height(bounds[1]);fit=fitted;grainKey=L"";}
    void show(std::shared_ptr<WorkspaceData> const& data,ImageBrush const& checker,J const& view,winrt::Windows::UI::Color color){
        shown=view;paint=color;
        auto extent=array(view,L"extent");
        double width=std::max(1.,extent.Size()==2?extent.GetNumberAt(0):1.),height=std::max(1.,extent.Size()==2?extent.GetNumberAt(1):1.);
        double side=std::min(fit[0],fit[1]);
        double scale=std::min({side/std::max(width,height),fit[0]/width,fit[1]/height});
        double w=std::max(4.,std::round(width*scale)),h=std::max(4.,std::round(height*scale));
        sheet.Width(w);sheet.Height(h);
        bool visible=flag(view,L"shown");
        if(visible)sheet.Background(fill(color));else sheet.Background(checker);
        sheet.BorderThickness({1,1,1,1});sheet.BorderBrush(data->tint(L"text",31));
        auto texture=view.GetNamedValue(L"texture",JsonValue::CreateNullValue());
        ImageSource source{nullptr};
        if(texture.ValueType()==JsonValueType::String)source=paperPreview(data,texture.GetString()).source;
        auto key=texture.Stringify()+L"/"+to_hstring(w)+L"x"+to_hstring(h)+(source?L"/ready":L"");
        grain.Opacity(visible?.2:.08);
        if(key==grainKey)return;
        grainKey=key;grain.Children().Clear();grain.Width(w-2);grain.Height(h-2);
        RectangleGeometry clip;clip.Rect({0,0,float(std::max(0.,w-2)),float(std::max(0.,h-2))});grain.Clip(clip);
        if(!source)return;
        for(double y=0;y<h;y+=tile)for(double x=0;x<w;x+=tile){
            Image image;image.Source(source);image.Width(tile);image.Height(tile);image.Stretch(Stretch::Fill);
            Canvas::SetLeft(image,x);Canvas::SetTop(image,y);grain.Children().Append(image);
        }
    }
};

struct Drawer {
    Grid root,head,footer;
    ScrollViewer scroll;
    StackPanel body;
    TranslateTransform shift;
    Animation::Storyboard motion{nullptr};
    bool open=false;
};
}

struct NewDrawingDialog::Impl:std::enable_shared_from_this<Impl>{
    std::shared_ptr<WorkspaceData> data;
    Dispatch document;
    Window window{nullptr};
    std::function<void()> changed;
    double request=0,epoch=0,revision=0;
    bool waiting=false,seen=false,showing=false,programmatic=false,stopping=false,creating=false,built=false,narrow=false;
    J shown;
    hstring copyKey,unitsKey,spacesKey,depthsKey,blendingsKey,numericKey,reliefKey,resolutionsKey,tabsKey,tilesKey,paperKey,paletteKey,groupsKey,drawerKind;
    ContentDialog dialog{nullptr};
    Grid root,footer,body,drawerHost;
    ContentControl gate;
    ScrollViewer scroll;
    StackPanel page,left,right,captions;
    TextBlock title,captionText,capacity,nameLabel,sizeHeading,resolutionLabel,resolutionUnit,sizeLine,colorHeading,spaceLabel,depthLabel,blendingLabel,paperHeading,textureName,textureRelief,paperColorName,presetsLabel;
    Button preview{nullptr},presets{nullptr},cancel{nullptr},create{nullptr},resolutionChoices{nullptr},texture{nullptr},paperColor{nullptr};
    TextBox name;
    std::array<Border,2> fields;
    std::array<TextBlock,2> prefixes;
    Border resolutionField;
    Grid resolutionSlot;
    TextBox resolutionEntry{nullptr};
    Button link{nullptr},portrait{nullptr},landscape{nullptr};
    ComboBox unit,space,depth,blending;
    Grid blendingHost{nullptr};
    Flyout resolutionMenu;
    StackPanel resolutionItems;
    SheetArt art;
    std::shared_ptr<PaperThumbnail> textureThumbnail;
    Microsoft::UI::Xaml::Shapes::Path paperPatch;
    Bindings numberBinds;
    std::array<NumericAdmissions,3> admissions;
    ImageBrush checker{nullptr};
    hstring checkerKey;

    Drawer presetsDrawer,paperDrawer,colorDrawer;
    TextBox search,saveName;
    ScrollViewer tabScroll;
    StackPanel tabs,summaryText,summaryTop,actions,saveRow;
    std::shared_ptr<WorkflowPages> footerPages=std::make_shared<WorkflowPages>();
    Grid presetGrid,summary;
    TextBlock empty,summaryName,summarySize,summaryDetails,paperTitle,colorTitle,paletteTitle,paletteCount;
    SheetArt summaryArt;
    std::vector<std::shared_ptr<SheetArt>> presetArts;
    std::vector<Button> presetTiles;
    std::vector<Border> presetFrames;
    int presetColumns=0;
    Button browserCancel{nullptr},saveAs{nullptr},select{nullptr},saveCancel{nullptr},saveConfirm{nullptr};
    ComboBox saveGroup;
    std::shared_ptr<TextureGrid> papers;
    Grid reliefRow,reliefSlot;
    Border reliefField;
    TextBlock reliefLabel,reliefUnit;
    TextBox reliefEntry{nullptr};
    Bindings reliefBinds;
    NumericAdmissions reliefAdmissions;
    Button paperCancel{nullptr},paperUse{nullptr};
    std::optional<ColorForm> form;
    Border well;
    Button colorMode{nullptr},transparentMode{nullptr};
    TextBlock colorModeLabel,transparentModeLabel;
    VariableSizedWrapGrid swatches;
    std::vector<Button> swatchTiles;
    std::vector<SolidColorBrush> swatchPaints;
    std::vector<J> swatchValues;
    MenuFlyout swatchMenu;
    MenuFlyoutItem swatchDelete;
    uint32_t swatchTarget=0;
    Button addSwatch{nullptr},colorCancel{nullptr},colorUse{nullptr};
    winrt::Windows::Foundation::IAsyncOperation<Pickers::PickFileResult> picker{nullptr};
    XamlRoot::Changed_revoker resized;

    J copy()const{return object(shown,L"copy");}
    hstring text(wchar_t const* key)const{return str(copy(),key);}
    J drawerView()const{return object(shown,L"drawer");}
    void send(J const& action){
        if(data->updating||!dialog)return;
        creating=str(action,L"op")==L"create";
        data->dispatch(O({{L"type",S(L"new_drawing")},{L"action",action}}));
    }
    std::function<void()> sends(J action){return [weak=weak_from_this(),action]{if(auto self=weak.lock())self->send(action);};}
    static J op(wchar_t const* operation){return O({{L"op",S(operation)}});}

    void open(J const& envelope){
        request=num(envelope,L"id");
        auto file=object(object(data->model,L"state"),L"document_file");
        epoch=num(file,L"epoch");revision=num(file,L"revision");
        HWND handle=nullptr;check_hresult(window.as<IWindowNative>()->get_WindowHandle(&handle));
        if(IsIconic(handle))ShowWindow(handle,SW_RESTORE);
        auto screen=drawingScreen(window);
        waiting=true;seen=false;creating=false;shown=J{};
        data->dispatch(O({{L"type",S(L"new_drawing")},{L"action",O({{L"op",S(L"open")},{L"screen",screen},{L"untitled",object(data->model,L"windows_tabs").GetNamedValue(L"upcoming_id")}})}}));
        changed();
    }
    void apply(){
        if(!waiting)return;
        auto state=object(data->model,L"state");
        auto view=state.GetNamedValue(L"new_drawing",JsonValue::CreateNullValue());
        bool pending=false;for(auto value:array(state,L"requests"))pending|=num(value.GetObject(),L"id")==request;
        if(view.ValueType()==JsonValueType::Object){
            seen=true;
            if(!showing&&!stopping)present();
            render(view.GetObject());
            return;
        }
        if(seen||!pending)finish(pending);
    }
    void finish(bool pending){
        waiting=false;seen=false;
        if(pending&&!stopping)document(to_string((creating?O({{L"operation",S(L"create")},{L"id",N(request)},{L"epoch",N(epoch)},{L"revision",N(revision)}})
            :O({{L"operation",S(L"cancel")},{L"id",N(request)}})).Stringify()));
        if(showing){programmatic=true;dialog.Hide();}else{closeForm();changed();}
    }
    fire_and_forget present(){
        auto lifetime=shared_from_this();
        if(!built)build();
        showing=true;programmatic=false;shown=J{};copyKey=L"";
        dialogSurface(data,dialog);
        data->popup(true);
        try{co_await dialog.ShowAsync();}catch(hresult_error const&){}
        data->popup(false);
        showing=false;programmatic=false;
        for(auto panel:{&presetsDrawer,&paperDrawer,&colorDrawer}){if(panel->motion)panel->motion.Stop();panel->motion=nullptr;panel->open=false;panel->root.Visibility(Visibility::Collapsed);}
        closeForm();
        drawerKind=L"";
        if(waiting&&!stopping){waiting=false;document(to_string(O({{L"operation",S(L"cancel")},{L"id",N(request)}}).Stringify()));}
        changed();
    }
    void hide(){
        stopping=true;waiting=false;
        if(picker)picker.Cancel();
        if(showing){programmatic=true;dialog.Hide();}else closeForm();
    }

    ImageBrush checks(){
        auto palette=object(data->state,L"palette");
        auto scale=root.XamlRoot()?root.XamlRoot().RasterizationScale():1.;
        auto key=str(palette,L"checker_light")+str(palette,L"checker_dark")+to_hstring(scale);
        if(checker&&key==checkerKey)return checker;
        checkerKey=key;checker=ImageBrush();
        checker.ImageSource(checkerBitmap(260,scale,color(str(palette,L"checker_light")),color(str(palette,L"checker_dark"))));
        checker.Stretch(Stretch::None);checker.AlignmentX(AlignmentX::Left);checker.AlignmentY(AlignmentY::Top);
        return checker;
    }
    std::vector<winrt::Windows::UI::Color> displayed(std::vector<J> const& colors){
        std::vector<winrt::Windows::UI::Color> result(colors.size());if(colors.empty())return result;
        A list;for(auto const& value:colors)list.Append(value);
        try{
            auto previews=colorUi(data->localization.get(),O({{L"type",S(L"preview")},{L"colors",list},{L"rendition",JsonValue::CreateNullValue()}})).GetArray();
            for(uint32_t i=0;i<previews.Size()&&i<result.size();++i)result[i]=previewColor(array(previews.GetObjectAt(i),L"rgba"));
        }catch(hresult_error const&){}
        return result;
    }
    void showSheet(SheetArt& target,J const& view){target.show(data,checks(),view,displayed({object(view,L"color")})[0]);}

    TextBlock caption(bool bold=false,double scale=1,double opacity=1){
        auto result=label(data,L"",bold);result.FontSize(data->textSize()*scale);result.Opacity(opacity);
        result.TextTrimming(TextTrimming::CharacterEllipsis);result.VerticalAlignment(VerticalAlignment::Center);return result;
    }
    void input(Control const& control){
        control.Height(InputHeight);control.MinHeight(0);control.MinWidth(0);control.Background(data->brush(L"input"));
        control.BorderThickness({0});control.CornerRadius(data->controlCorner());control.VerticalAlignment(VerticalAlignment::Center);
        control.VerticalContentAlignment(VerticalAlignment::Center);
    }
    ComboBox choice(hstring const& id){
        ComboBox result;input(result);result.HorizontalAlignment(HorizontalAlignment::Stretch);AutomationProperties::SetAutomationId(result,id);
        return result;
    }
    Button toggle(hstring const& id,double width,double height){
        auto result=button(data,L"",[]{});if(width>0)result.Width(width);result.Height(height);
        result.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());AutomationProperties::SetAutomationId(result,id);result.VerticalAlignment(VerticalAlignment::Center);
        return result;
    }
    Button primary(hstring const& id,int column,std::function<void()> action){
        auto result=pickerButton(data,id,column,true,std::move(action));
        result.IsEnabledChanged([](winrt::Windows::Foundation::IInspectable const& sender,DependencyPropertyChangedEventArgs const& args){sender.as<Button>().Opacity(unbox_value<bool>(args.NewValue())?1.:.5);});
        return result;
    }
    void mark(Button const& target,bool on,Brush const& off){
        target.Background(on?Brush(selected(data)):off);
        AutomationProperties::SetItemStatus(target,on?data->caption(L"search",L"selected"):hstring());
    }
    Button tileButton(hstring const& id){
        auto result=button(data,L"",[]{});result.HorizontalAlignment(HorizontalAlignment::Stretch);result.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        result.MinHeight(48);result.Padding({6,6,10,6});result.Background(buttonBackground(data));result.FontWeight(winrt::Windows::UI::Text::FontWeights::Bold());
        AutomationProperties::SetAutomationId(result,id);return result;
    }
    StackPanel section(TextBlock const& heading,FrameworkElement const& content){
        StackPanel result;result.Spacing(ControlGap);result.Children().Append(heading);result.Children().Append(content);return result;
    }
    template<class Element> static void column(Grid const& grid,Element const& element,int index){Grid::SetColumn(element,index);grid.Children().Append(element);}
    static void columns(Grid const& grid,std::initializer_list<GridLength> widths){for(auto width:widths){ColumnDefinition definition;definition.Width(width);grid.ColumnDefinitions().Append(definition);}}
    static GridLength autoWidth(){return {1,GridUnitType::Auto};}
    static GridLength star(double weight=1){return {weight,GridUnitType::Star};}

    void build(){
        built=true;auto weak=weak_from_this();
        auto xaml=window.Content().XamlRoot();
        dialog=pickerDialog(data,xaml,root,gate,footer,L"new-drawing-dialog");
        root.Height(PickerDialogHeight);
        buildPage();buildFooter();
        drawerHost.IsHitTestVisible(true);Grid::SetRowSpan(drawerHost,2);root.Children().Append(drawerHost);
        buildPresets();buildPaper();buildColors();
        root.SizeChanged([weak](auto&&,SizeChangedEventArgs const& e){if(auto self=weak.lock()){
            auto size=e.NewSize();RectangleGeometry clip;clip.Rect({0,0,size.Width,size.Height});self->drawerHost.Clip(clip);
            for(auto panel:{&self->presetsDrawer,&self->paperDrawer,&self->colorDrawer})if(!panel->open)panel->shift.Y(size.Height);
            self->arrange(size.Width>0&&size.Width<NarrowWidth);
        }});
        resized=xaml.Changed(auto_revoke,[weak](auto&&,auto&&){if(auto self=weak.lock())fitPicker(self->root);});
        dialog.Opened([weak](auto&&,auto&&){if(auto self=weak.lock()){fitPicker(self->root);self->name.Focus(FocusState::Programmatic);}});
        dialog.Closing([weak](auto&&,ContentDialogClosingEventArgs const& e){if(auto self=weak.lock()){
            if(self->programmatic||self->stopping)return;
            e.Cancel(true);
            if(self->form&&self->form->escape())return;
            self->send(op(L"back"));
        }});
        watchPaperPreviews(data,weak_from_this(),[weak]{if(auto self=weak.lock())self->refreshArt();});
        fitPicker(root);
    }
    void buildPage(){
        auto weak=weak_from_this();
        pickerTitle(data,title,L"new-drawing-title");
        art.init(Preview,PreviewFit,40);
        preview=button(data,L"",sends(op(L"presets")));preview.Width(Preview[0]);preview.Height(Preview[1]);preview.Padding({0});
        preview.Background(data->tint(L"text",13));preview.CornerRadius({12,12,12,12});preview.Content(art.root);
        AutomationProperties::SetAutomationId(preview,L"new-drawing-preview");
        captionText=caption();captionText.TextAlignment(TextAlignment::Center);captionText.HorizontalAlignment(HorizontalAlignment::Center);AutomationProperties::SetAutomationId(captionText,L"new-drawing-caption");
        capacity=caption(false,.82,.6);capacity.TextAlignment(TextAlignment::Center);capacity.HorizontalAlignment(HorizontalAlignment::Center);AutomationProperties::SetAutomationId(capacity,L"new-drawing-capacity");
        captions.Spacing(2);captions.Children().Append(captionText);captions.Children().Append(capacity);
        left.Spacing(8);left.VerticalAlignment(VerticalAlignment::Top);left.Children().Append(preview);left.Children().Append(captions);

        nameLabel=caption();
        input(name);name.HorizontalAlignment(HorizontalAlignment::Stretch);name.MaxLength(256);name.Padding({10,7,10,0});
        AutomationProperties::SetAutomationId(name,L"new-drawing-name");inheritLanguage(name,data);
        name.TextChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating&&self->name.Text()!=str(self->shown,L"name"))self->send(O({{L"op",S(L"name")},{L"text",S(self->name.Text())}}));});
        name.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock();self&&!composingKey(e)&&e.Key()==winrt::Windows::System::VirtualKey::Enter){e.Handled(true);self->createDrawing();}});
        Grid nameRow;nameRow.ColumnSpacing(RowGap);columns(nameRow,{autoWidth(),star()});column(nameRow,nameLabel,0);column(nameRow,name,1);

        sizeHeading=caption(true);
        StackPanel linked;linked.Orientation(Orientation::Horizontal);linked.Spacing(2);
        for(int i=0;i<2;++i){
            prefixes[i]=caption(false,1,.6);prefixes[i].Margin({10,0,0,0});
            fields[i].Width(FieldWidth);fields[i].Height(InputHeight);fields[i].Background(data->brush(L"input"));fields[i].CornerRadius(data->controlCorner());
            AutomationProperties::SetAutomationId(fields[i],i?L"new-drawing-height":L"new-drawing-width");
            Grid slot;columns(slot,{autoWidth(),star()});column(slot,prefixes[i],0);fields[i].Child(slot);
            if(i)linked.Children().Append(link);
            linked.Children().Append(fields[i]);
            if(!i){
                link=toggle(L"new-drawing-link",24,InputHeight);
                link.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->send(O({{L"op",S(L"linked")},{L"linked",B(!flag(self->shown,L"linked"))}}));});
            }
        }
        unit=choice(L"new-drawing-unit");unit.MinWidth(76);
        unit.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->chose(self->unit,L"units",L"unit");});
        StackPanel orientation;orientation.Orientation(Orientation::Horizontal);orientation.Spacing(2);orientation.Padding({2,2,2,2});
        Border orientationWell;orientationWell.Background(data->tint(L"text",15));orientationWell.CornerRadius({data->controlCorner().TopLeft+2,data->controlCorner().TopLeft+2,data->controlCorner().TopLeft+2,data->controlCorner().TopLeft+2});
        orientationWell.Child(orientation);orientationWell.VerticalAlignment(VerticalAlignment::Center);AutomationProperties::SetAutomationId(orientationWell,L"new-drawing-orientation");
        for(auto [target,value,id]:{std::tuple{&portrait,L"portrait",L"new-drawing-portrait"},std::tuple{&landscape,L"landscape",L"new-drawing-landscape"}}){
            *target=toggle(id,30,30);(*target).Content(icon(hstring(L"layer-")+value,data->theme()));
            auto orientationValue=hstring(value);
            target->Click([weak,orientationValue](auto&&,auto&&){if(auto self=weak.lock())self->send(O({{L"op",S(L"orientation")},{L"orientation",S(orientationValue)}}));});
            orientation.Children().Append(*target);
        }
        Grid dimensions;dimensions.ColumnSpacing(ControlGap);columns(dimensions,{autoWidth(),autoWidth(),star(),autoWidth()});
        column(dimensions,linked,0);column(dimensions,unit,1);column(dimensions,orientationWell,3);

        resolutionLabel=caption();resolutionLabel.Margin({0,0,4,0});
        resolutionUnit=caption(false,1,.6);
        resolutionChoices=button(data,L"",[weak]{if(auto self=weak.lock()){
            Primitives::FlyoutShowOptions options;options.Placement(Primitives::FlyoutPlacementMode::BottomEdgeAlignedLeft);self->resolutionMenu.ShowAt(self->resolutionField,options);
        }});
        resolutionChoices.Width(28);resolutionChoices.Height(InputHeight);resolutionChoices.Content(icon(L"chevron-down",data->theme(),14));
        AutomationProperties::SetAutomationId(resolutionChoices,L"new-drawing-resolution-choices");
        columns(resolutionSlot,{{ValueWidth,GridUnitType::Pixel},autoWidth(),autoWidth()});resolutionSlot.ColumnSpacing(2);
        resolutionUnit.Margin({2,0,0,0});column(resolutionSlot,resolutionUnit,1);column(resolutionSlot,resolutionChoices,2);
        resolutionField.Height(InputHeight);resolutionField.Background(data->brush(L"input"));resolutionField.CornerRadius(data->controlCorner());resolutionField.Child(resolutionSlot);
        AutomationProperties::SetAutomationId(resolutionField,L"new-drawing-resolution-field");
        resolutionItems.Padding({4,4,4,4});resolutionMenu.Content(resolutionItems);AutomationProperties::SetAutomationId(resolutionItems,L"new-drawing-resolution-menu");
        sizeLine=caption(false,.82,.6);sizeLine.Margin({RowGap,0,0,0});AutomationProperties::SetAutomationId(sizeLine,L"new-drawing-size");
        AutomationProperties::SetLiveSetting(sizeLine,Automation::Peers::AutomationLiveSetting::Polite);
        Grid resolutionRow;resolutionRow.ColumnSpacing(ControlGap);columns(resolutionRow,{autoWidth(),autoWidth(),star()});
        column(resolutionRow,resolutionLabel,0);column(resolutionRow,resolutionField,1);column(resolutionRow,sizeLine,2);
        StackPanel sizeRows;sizeRows.Spacing(RowGap);sizeRows.Children().Append(dimensions);sizeRows.Children().Append(resolutionRow);

        colorHeading=caption(true);
        Grid colors;colors.ColumnSpacing(ControlGap);columns(colors,{star(4),star(5),star(4)});
        space=choice(L"document-space");depth=choice(L"document-depth");blending=choice(L"document-blending");
        space.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->chose(self->space,L"spaces",L"space");});
        depth.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->chose(self->depth,L"depths",L"depth");});
        blending.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->chose(self->blending,L"blendings",L"blending");});
        blendingHost=explainable(blending);blendingHost.Height(InputHeight);
        spaceLabel=caption(false,.82,.6);depthLabel=caption(false,.82,.6);blendingLabel=caption(false,.82,.6);
        int index=0;
        for(auto [heading,control]:{std::pair<TextBlock,FrameworkElement>{spaceLabel,space},{depthLabel,depth},{blendingLabel,blendingHost}}){
            StackPanel cell;cell.Spacing(2);cell.Children().Append(heading);cell.Children().Append(control);column(colors,cell,index++);
        }

        paperHeading=caption(true);
        texture=tileButton(L"new-drawing-texture");paperColor=tileButton(L"new-drawing-paper-color");
        texture.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->send(op(L"paper"));});
        paperColor.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->send(op(L"paper_color"));});
        textureThumbnail=PaperThumbnail::Create(data,ThumbnailSide,20,true);
        paperPatch.Width(ThumbnailSide);paperPatch.Height(ThumbnailSide);paperPatch.IsHitTestVisible(false);
        paperPatch.Data(squircleRectangle(float(ThumbnailSide),float(ThumbnailSide),{18,18,18,18}));
        paperPatch.Stroke(data->tint(L"text",31));paperPatch.StrokeThickness(1);
        textureName=caption(true);paperColorName=caption(true);
        textureRelief=caption(false,.82,.6);textureRelief.LineHeight(0);textureRelief.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());AutomationProperties::SetAutomationId(textureRelief,L"new-drawing-texture-relief");
        StackPanel textureText;textureText.VerticalAlignment(VerticalAlignment::Center);textureText.Children().Append(textureName);textureText.Children().Append(textureRelief);
        for(auto [tile,picture,label]:{std::tuple<Button,FrameworkElement,FrameworkElement>{texture,textureThumbnail->root,textureText},{paperColor,paperPatch,paperColorName}}){
            Grid content;content.ColumnSpacing(RowGap);columns(content,{autoWidth(),star()});column(content,picture,0);column(content,label,1);tile.Content(content);
        }
        Grid paperTiles;paperTiles.ColumnSpacing(ControlGap);columns(paperTiles,{star(),star()});column(paperTiles,texture,0);column(paperTiles,paperColor,1);

        right.Spacing(SectionGap);
        for(FrameworkElement part:{FrameworkElement(nameRow),FrameworkElement(section(sizeHeading,sizeRows)),FrameworkElement(section(colorHeading,colors)),FrameworkElement(section(paperHeading,paperTiles))})right.Children().Append(part);
        body.ColumnSpacing(24);body.RowSpacing(SectionGap);body.Children().Append(left);body.Children().Append(right);arrangeBody();
        page.Padding({PickerPadding,PickerPadding,PickerPadding,PickerPadding});page.Spacing(SectionGap);page.Children().Append(title);page.Children().Append(body);
        scroll.Content(page);scroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);mouseScrollbars(scroll);
        gate.Content(scroll);gate.IsTabStop(false);gate.HorizontalContentAlignment(HorizontalAlignment::Stretch);gate.VerticalContentAlignment(VerticalAlignment::Stretch);
    }
    void buildFooter(){
        auto weak=weak_from_this();
        pickerFooter(data,footer,{GridUnitType::Auto,GridUnitType::Star,GridUnitType::Auto,GridUnitType::Auto});
        presets=pickerButton(data,L"new-drawing-presets-button",0,false,sends(op(L"presets")));
        presetsLabel=caption();presetsLabel.FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());
        auto up=icon(L"chevron-down",data->theme(),14);up.RenderTransformOrigin({.5f,.5f});RotateTransform turn;turn.Angle(180);up.RenderTransform(turn);
        StackPanel content;content.Orientation(Orientation::Horizontal);content.Spacing(6);content.Children().Append(presetsLabel);content.Children().Append(up);presets.Content(content);
        cancel=pickerButton(data,L"new-drawing-cancel",2,false,sends(op(L"cancel")));
        create=primary(L"new-drawing-create",3,[weak]{if(auto self=weak.lock())self->createDrawing();});
        for(auto const& node:{presets,cancel,create})footer.Children().Append(node);
    }
    void drawer(Drawer& target,hstring const& id){
        auto& panel=target.root;panel.Background(data->brush(L"settings"));panel.Visibility(Visibility::Collapsed);panel.RenderTransform(target.shift);
        AutomationProperties::SetAutomationId(panel,id);
        for(auto height:{GridLength{1,GridUnitType::Auto},GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){RowDefinition row;row.Height(height);panel.RowDefinitions().Append(row);}
        target.head.Padding({PickerPadding,PickerPadding,PickerPadding,8});panel.Children().Append(target.head);
        target.body.Padding({PickerPadding,8,PickerPadding,PickerPadding});target.body.Spacing(SectionGap);
        target.scroll.Content(target.body);target.scroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);target.scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);
        mouseScrollbars(target.scroll);Grid::SetRow(target.scroll,1);panel.Children().Append(target.scroll);
        Grid::SetRow(target.footer,2);panel.Children().Append(target.footer);
        drawerHost.Children().Append(panel);
    }
    TextBlock drawerTitle(Drawer& target,hstring const& id){
        TextBlock heading;pickerTitle(data,heading,id);target.head.Children().Append(heading);return heading;
    }
    void buildPresets(){
        auto weak=weak_from_this();
        drawer(presetsDrawer,L"new-drawing-browser");
        input(search);search.HorizontalAlignment(HorizontalAlignment::Stretch);search.Padding({10,7,10,0});search.MaxLength(256);search.IsSpellCheckEnabled(false);
        AutomationProperties::SetAutomationId(search,L"new-drawing-search");inheritLanguage(search,data);
        search.TextChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating&&self->search.Text()!=str(self->drawerView(),L"query"))self->send(O({{L"op",S(L"search")},{L"query",S(self->search.Text())}}));});
        presetsDrawer.head.Children().Append(search);
        tabs.Orientation(Orientation::Horizontal);tabs.Spacing(4);AutomationProperties::SetAutomationId(tabs,L"new-drawing-tabs");
        tabScroll.Content(tabs);tabScroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Auto);tabScroll.VerticalScrollBarVisibility(ScrollBarVisibility::Disabled);
        tabScroll.HorizontalScrollMode(ScrollMode::Enabled);tabScroll.VerticalScrollMode(ScrollMode::Disabled);mouseScrollbars(tabScroll);
        presetGrid.ColumnSpacing(TileGap);presetGrid.RowSpacing(12);AutomationProperties::SetAutomationId(presetGrid,L"new-drawing-presets");
        presetGrid.XYFocusKeyboardNavigation(XYFocusKeyboardNavigationMode::Enabled);
        presetGrid.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->arrangePresets();});
        empty=caption(false,.82,.6);empty.TextWrapping(TextWrapping::Wrap);empty.Visibility(Visibility::Collapsed);AutomationProperties::SetAutomationId(empty,L"new-drawing-presets-empty");
        for(FrameworkElement part:{FrameworkElement(tabScroll),FrameworkElement(presetGrid),FrameworkElement(empty)})presetsDrawer.body.Children().Append(part);
        pickerFooter(data,presetsDrawer.footer,{GridUnitType::Star,GridUnitType::Auto});
        summaryArt.init({SummaryArt,SummaryArt},{SummaryArt-2,SummaryArt-2},18);
        summaryName=caption(true);AutomationProperties::SetAutomationId(summaryName,L"new-drawing-summary-name");
        summarySize=caption(false,.82,.6);AutomationProperties::SetAutomationId(summarySize,L"new-drawing-summary-size");
        summaryDetails=caption(false,.82,.6);AutomationProperties::SetAutomationId(summaryDetails,L"new-drawing-summary-details");
        summaryTop.Orientation(Orientation::Horizontal);summaryTop.Spacing(6);summaryTop.Children().Append(summaryName);summaryTop.Children().Append(summarySize);
        summaryText.Spacing(1);summaryText.VerticalAlignment(VerticalAlignment::Center);summaryText.Children().Append(summaryTop);summaryText.Children().Append(summaryDetails);
        summary.ColumnSpacing(RowGap);columns(summary,{autoWidth(),star()});column(summary,summaryArt.root,0);column(summary,summaryText,1);
        summary.VerticalAlignment(VerticalAlignment::Center);
        presetsDrawer.footer.Children().Append(summary);
        browserCancel=pickerButton(data,L"new-drawing-browser-cancel",0,false,sends(op(L"close_drawer")));
        saveAs=pickerButton(data,L"new-drawing-save-as",0,false,sends(op(L"save_as")));
        select=primary(L"new-drawing-select",0,sends(op(L"use_preset")));
        actions.Orientation(Orientation::Horizontal);actions.Spacing(8);for(auto const& node:{browserCancel,saveAs,select})actions.Children().Append(node);
        input(saveName);saveName.Width(150);saveName.MaxLength(64);saveName.Padding({10,7,10,0});AutomationProperties::SetAutomationId(saveName,L"new-drawing-save-name");inheritLanguage(saveName,data);
        saveName.TextChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->data->updating&&self->saveName.Text()!=str(object(self->drawerView(),L"save"),L"name"))self->send(O({{L"op",S(L"preset_name")},{L"text",S(self->saveName.Text())}}));});
        saveName.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock();self&&!composingKey(e)&&e.Key()==winrt::Windows::System::VirtualKey::Enter){e.Handled(true);self->send(op(L"save_preset"));}});
        input(saveGroup);saveGroup.Width(150);AutomationProperties::SetAutomationId(saveGroup,L"new-drawing-save-group");
        saveGroup.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock()){
            auto groups=array(object(self->drawerView(),L"save"),L"groups");auto index=self->saveGroup.SelectedIndex();
            if(self->data->updating||index<0||uint32_t(index)>=groups.Size())return;
            self->send(O({{L"op",S(L"preset_group")},{L"group",groups.GetObjectAt(index).GetNamedValue(L"value")}}));
        }});
        saveCancel=pickerButton(data,L"new-drawing-save-cancel",0,false,sends(op(L"cancel_save")));
        saveConfirm=primary(L"new-drawing-save-confirm",0,sends(op(L"save_preset")));
        saveRow.Orientation(Orientation::Horizontal);saveRow.Spacing(8);saveRow.Visibility(Visibility::Collapsed);
        for(FrameworkElement node:{FrameworkElement(saveName),FrameworkElement(saveGroup),FrameworkElement(saveCancel),FrameworkElement(saveConfirm)})saveRow.Children().Append(node);
        footerPages->append(actions);footerPages->append(saveRow);Grid::SetColumn(footerPages->root,1);presetsDrawer.footer.Children().Append(footerPages->root);
    }
    void buildPaper(){
        auto weak=weak_from_this();
        drawer(paperDrawer,L"new-drawing-paper-drawer");
        paperTitle=drawerTitle(paperDrawer,L"new-drawing-paper-title");
        papers=TextureGrid::Create(data,L"new-drawing-paper");
        papers->picked=[weak](uint32_t index){if(auto self=weak.lock())self->pickPaper(index);};
        papers->activated=[weak](uint32_t index){if(auto self=weak.lock()){self->pickPaper(index);self->usePaper();}};
        papers->trail(L"import",L"plus",[weak]{if(auto self=weak.lock())self->importPaper();});
        paperDrawer.body.Children().Append(papers->grid);
        pickerFooter(data,paperDrawer.footer,{GridUnitType::Star,GridUnitType::Auto,GridUnitType::Auto});
        reliefLabel=caption();reliefUnit=caption(false,1,.6);reliefUnit.Margin({2,0,10,0});
        columns(reliefSlot,{{ValueWidth,GridUnitType::Pixel},autoWidth()});reliefSlot.ColumnSpacing(2);column(reliefSlot,reliefUnit,1);
        reliefField.Height(InputHeight);reliefField.Background(data->tint(L"text",15));reliefField.CornerRadius(data->controlCorner());reliefField.Child(reliefSlot);
        AutomationProperties::SetAutomationId(reliefField,L"new-drawing-paper-relief-field");
        reliefRow.ColumnSpacing(RowGap);reliefRow.HorizontalAlignment(HorizontalAlignment::Left);columns(reliefRow,{star(),autoWidth()});
        column(reliefRow,reliefLabel,0);column(reliefRow,reliefField,1);AutomationProperties::SetAutomationId(reliefLabel,L"new-drawing-paper-relief-label");
        paperCancel=pickerButton(data,L"new-drawing-paper-cancel",1,false,sends(op(L"close_drawer")));
        paperUse=primary(L"new-drawing-paper-use",2,[weak]{if(auto self=weak.lock())self->usePaper();});
        for(FrameworkElement node:{FrameworkElement(reliefRow),FrameworkElement(paperCancel),FrameworkElement(paperUse)})paperDrawer.footer.Children().Append(node);
    }
    void buildColors(){
        auto weak=weak_from_this();
        drawer(colorDrawer,L"new-drawing-color-drawer");
        colorTitle=drawerTitle(colorDrawer,L"new-drawing-color-title");
        colorDrawer.body.Padding({0});
        StackPanel modes;modes.Orientation(Orientation::Horizontal);modes.Spacing(2);modes.Padding({4,4,4,4});
        well.Child(modes);well.Background(data->tint(L"text",15));well.CornerRadius({17,17,17,17});well.VerticalAlignment(VerticalAlignment::Center);
        well.HorizontalAlignment(HorizontalAlignment::Right);AutomationProperties::SetAutomationId(well,L"new-drawing-color-mode");
        for(auto [target,label,glyph,id,transparent]:{std::tuple{&colorMode,&colorModeLabel,L"layer-solid",L"new-drawing-color-mode-color",false},std::tuple{&transparentMode,&transparentModeLabel,L"layer-transparent",L"new-drawing-color-mode-transparent",true}}){
            *target=toggle(id,0,26);target->Padding({8,0,8,0});target->CornerRadius({13,13,13,13});
            *label=caption();label->FontWeight(winrt::Windows::UI::Text::FontWeights::Medium());
            StackPanel content;content.Orientation(Orientation::Horizontal);content.Spacing(5);content.Children().Append(icon(glyph,data->theme(),14));content.Children().Append(*label);
            target->Content(content);
            bool clearSheet=transparent;
            target->Click([weak,clearSheet](auto&&,auto&&){if(auto self=weak.lock())self->send(O({{L"op",S(L"transparent")},{L"transparent",B(clearSheet)}}));});
            modes.Children().Append(*target);
        }
        paletteTitle=caption();paletteCount=caption(false,1,.6);
        swatches.Orientation(Orientation::Horizontal);swatches.ItemWidth(SwatchSide+4);swatches.ItemHeight(SwatchSide+4);
        AutomationProperties::SetAutomationId(swatches,L"new-drawing-paper-color-tiles");
        for(uint32_t index=0;index<PaperColorLimit;++index){
            auto tile=button(data,L"",[weak,index]{if(auto self=weak.lock();self&&self->form&&index<self->swatchValues.size())self->form->choose(self->swatchValues[index]);});
            tile.Width(SwatchSide);tile.Height(SwatchSide);tile.Padding({3,3,3,3});tile.CornerRadius({6,6,6,6});
            tile.HorizontalContentAlignment(HorizontalAlignment::Stretch);tile.VerticalContentAlignment(VerticalAlignment::Stretch);
            SolidColorBrush paint;Border patch;patch.CornerRadius({5,5,5,5});patch.Background(paint);patch.BorderBrush(data->tint(L"text",31));patch.BorderThickness({1,1,1,1});tile.Content(patch);
            AutomationProperties::SetAutomationId(tile,L"new-drawing-paper-color-"+to_hstring(index));tile.Visibility(Visibility::Collapsed);
            tile.ContextRequested([weak,index](winrt::Windows::Foundation::IInspectable const& sender,ContextRequestedEventArgs const& e){if(auto self=weak.lock()){
                e.Handled(true);self->swatchTarget=index;self->swatchDelete.Text(self->text(L"delete"));self->swatchMenu.ShowAt(sender.as<FrameworkElement>());
            }});
            swatches.Children().Append(tile);swatchTiles.push_back(tile);swatchPaints.push_back(paint);
        }
        addSwatch=button(data,L"",sends(op(L"add_paper_color")));
        addSwatch.Width(SwatchSide-6);addSwatch.Height(SwatchSide-6);addSwatch.Margin({3,3,3,3});addSwatch.Background(data->tint(L"text",15));
        addSwatch.Content(icon(L"plus",data->theme()));AutomationProperties::SetAutomationId(addSwatch,L"new-drawing-add-paper-color");
        swatches.Children().Append(addSwatch);
        AutomationProperties::SetAutomationId(swatchDelete,L"new-drawing-paper-color-delete");swatchMenu.Items().Append(swatchDelete);
        swatchDelete.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->send(O({{L"op",S(L"remove_paper_color")},{L"index",N(self->swatchTarget)}}));});
        pickerFooter(data,colorDrawer.footer,{GridUnitType::Star,GridUnitType::Auto,GridUnitType::Auto});
        colorCancel=pickerButton(data,L"new-drawing-color-cancel",1,false,sends(op(L"close_drawer")));
        colorUse=primary(L"new-drawing-color-use",2,[weak]{if(auto self=weak.lock();self&&self->form&&self->form->commit())self->send(op(L"use_paper_color"));});
        for(auto const& node:{colorCancel,colorUse})colorDrawer.footer.Children().Append(node);
    }

    void arrangeBody(){
        body.ColumnDefinitions().Clear();body.RowDefinitions().Clear();
        if(narrow){
            columns(body,{star()});for(int i=0;i<2;++i){RowDefinition row;row.Height({1,GridUnitType::Auto});body.RowDefinitions().Append(row);}
            Grid::SetColumn(right,0);Grid::SetRow(right,1);left.HorizontalAlignment(HorizontalAlignment::Center);
        }else{
            columns(body,{{Preview[0],GridUnitType::Pixel},star()});
            Grid::SetColumn(right,1);Grid::SetRow(right,0);left.HorizontalAlignment(HorizontalAlignment::Stretch);
        }
    }
    void arrange(bool next){
        auto width=root.ActualWidth();
        Size2 bounds=next?Size2{std::min(NarrowPreview[0],std::max(120.,width-2*PickerPadding)),NarrowPreview[1]}:Preview;
        Size2 fitted=next?Size2{bounds[0]-36,NarrowFit[1]}:PreviewFit;
        if(next!=narrow){narrow=next;arrangeBody();if(form)form->arrange(narrow);}
        preview.Width(bounds[0]);preview.Height(bounds[1]);art.resize(bounds,fitted);
        if(art.shown.Size())showSheet(art,art.shown);
    }
    void refreshArt(){
        if(!showing)return;
        if(art.shown.Size())showSheet(art,art.shown);
        if(summaryArt.shown.Size())showSheet(summaryArt,summaryArt.shown);
        for(auto const& sheet:presetArts)if(sheet->shown.Size())sheet->show(data,checks(),sheet->shown,sheet->paint);
    }

    void chose(ComboBox const& box,wchar_t const* list,wchar_t const* key){
        if(data->updating)return;
        auto choices=array(shown,list);auto index=box.SelectedIndex();
        if(index<0||uint32_t(index)>=choices.Size())return;
        send(O({{L"op",S(key)},{key,choices.GetObjectAt(index).GetNamedValue(L"value")}}));
    }
    void markOrientation(){
        auto orientation=str(object(shown,L"page"),L"orientation");
        mark(portrait,orientation==L"portrait",clear());mark(landscape,orientation==L"landscape",clear());
    }
    void markTransparency(){
        bool transparent=flag(drawerView(),L"transparent");
        mark(colorMode,!transparent,clear());mark(transparentMode,transparent,clear());
    }
    void pickPaper(uint32_t index){
        auto tiles=array(drawerView(),L"tiles");if(index>=tiles.Size())return;
        send(O({{L"op",S(L"pick_paper")},{L"id",tiles.GetObjectAt(index).GetNamedValue(L"id",JsonValue::CreateNullValue())}}));
    }
    void createDrawing(){
        if(!dialog||data->updating||drawerKind.size())return;
        bool admitted=true;
        for(auto const& list:admissions)for(auto const& admit:list)admitted=admit(false)&&admitted;
        if(admitted)send(op(L"create"));
    }
    void usePaper(){
        if(!dialog||data->updating)return;
        bool admitted=true;
        for(auto const& admit:reliefAdmissions)admitted=admit(false)&&admitted;
        if(admitted)send(op(L"use_paper"));
    }
    fire_and_forget importPaper(){
        auto lifetime=shared_from_this();
        if(picker)co_return;
        try{
            auto copied=object(object(data->catalog,L"native_copy"),L"paper");
            Pickers::FileOpenPicker chooser(window.AppWindow().Id());chooser.CommitButtonText(str(copied,L"import_accept"));
            for(auto suffix:array(copied,L"extensions"))chooser.FileTypeFilter().Append(suffix.GetString());
            picker=chooser.PickSingleFileAsync();auto chosen=co_await picker;picker=nullptr;
            if(chosen&&!stopping&&waiting)document(to_string(O({{L"operation",S(L"paper_import")},{L"id",JsonValue::CreateNullValue()},{L"path",S(chosen.Path())}}).Stringify()));
        }catch(hresult_canceled const&){picker=nullptr;}
        catch(hresult_error const&){picker=nullptr;}
    }
    void closeForm(){
        if(!form)return;
        auto closing=std::move(*form);form.reset();
        closing.close();detach(well);
        colorDrawer.body.Children().Clear();
    }

    void relabel(J const& view){
        auto labels=object(view,L"copy");auto t=[&](wchar_t const* key){return str(labels,key);};
        auto named=[](FrameworkElement const& element,hstring const& value,bool tip=false){AutomationProperties::SetName(element,value);if(tip)tooltip(element,value);};
        title.Text(t(L"title"));named(dialog,t(L"title"));named(root,t(L"title"));
        nameLabel.Text(t(L"name"));named(name,t(L"name"));
        sizeHeading.Text(t(L"size"));prefixes[0].Text(t(L"width"));prefixes[1].Text(t(L"height"));
        named(link,t(L"link"),true);named(unit,t(L"unit"));named(portrait,t(L"portrait"),true);named(landscape,t(L"landscape"),true);
        resolutionLabel.Text(t(L"resolution"));resolutionUnit.Text(t(L"resolution_unit"));named(resolutionChoices,t(L"common_resolutions"),true);
        colorHeading.Text(t(L"color"));spaceLabel.Text(t(L"space"));depthLabel.Text(t(L"depth"));blendingLabel.Text(t(L"blending"));
        named(space,t(L"space"));named(depth,t(L"depth"));named(blending,t(L"blending"));
        paperHeading.Text(t(L"paper"));named(texture,t(L"paper"),true);named(paperColor,t(L"paper_color"),true);
        presetsLabel.Text(t(L"presets"));named(presets,t(L"presets"));named(preview,t(L"presets"),true);
        pickerLabel(cancel,t(L"cancel"));pickerLabel(create,t(L"create"));
        search.PlaceholderText(t(L"search"));named(search,t(L"search"));
        pickerLabel(browserCancel,t(L"cancel"));pickerLabel(select,t(L"select"));
        saveName.PlaceholderText(t(L"preset_name"));named(saveName,t(L"preset_name"));named(saveGroup,t(L"preset_category"),true);
        pickerLabel(saveCancel,t(L"cancel"));
        paperTitle.Text(t(L"paper"));papers->rename(*papers->trailing,t(L"import"));pickerLabel(paperCancel,t(L"cancel"));pickerLabel(paperUse,t(L"use_texture"));
        reliefLabel.Text(t(L"relief"));tooltip(reliefLabel,t(L"relief"));reliefUnit.Text(t(L"relief_unit"));named(reliefField,t(L"relief"));
        colorTitle.Text(t(L"paper_color"));colorModeLabel.Text(t(L"color"));transparentModeLabel.Text(t(L"transparent"));named(well,t(L"sheet_mode"));
        named(colorMode,t(L"color"));named(transparentMode,t(L"transparent"));
        paletteTitle.Text(t(L"paper_colors"));pickerLabel(colorCancel,t(L"cancel"));pickerLabel(colorUse,t(L"use_color"));
        named(addSwatch,t(L"add_paper_color"),true);
    }
    static void items(ComboBox const& box,hstring& key,A const& choices,std::function<hstring(J const&)> const& labelFor,std::function<bool(J const&)> const& chosen,hstring const& prefix){
        auto next=choices.Stringify();
        if(next!=key){
            key=next;box.Items().Clear();
            for(uint32_t i=0;i<choices.Size();++i)comboOption(box,labelFor(choices.GetObjectAt(i)),prefix.empty()?hstring():prefix+L"-"+to_hstring(i));
        }
        for(uint32_t i=0;i<choices.Size();++i)if(chosen(choices.GetObjectAt(i))&&box.SelectedIndex()!=int32_t(i))box.SelectedIndex(int32_t(i));
    }
    static std::function<bool(J const&)> valued(V const& value){auto expected=value.Stringify();return [expected](J const& choice){return choice.GetNamedValue(L"value").Stringify()==expected;};}
    static hstring labelOf(J const& choice){return str(choice,L"label");}

    TextBox numberField(J const& spec,wchar_t const* titleKey,wchar_t const* operation,hstring const& id,std::function<double(J const&)> value,Bindings& binds,NumericAdmissions& admit){
        auto weak=weak_from_this();
        NumberPresentation presentation;presentation.title=[weak,titleKey]{if(auto self=weak.lock())return self->text(titleKey);return hstring();};
        auto number=CapyUi::number(data,text(titleKey),spec,
            [weak,value]{if(auto self=weak.lock();self&&self->shown.Size())return value(self->shown);return 0.;},
            [weak,operation](double next){if(auto self=weak.lock())self->send(O({{L"op",S(operation)},{L"value",N(next)}}));},
            binds,nullptr,true,id,false,presentation,&admit);
        auto entry=numberEntry(number);detach(entry);
        entry.Height(InputHeight);entry.TextAlignment(TextAlignment::Right);entry.HorizontalAlignment(HorizontalAlignment::Stretch);
        entry.Padding({4,7,10,0});entry.VerticalAlignment(VerticalAlignment::Center);
        return entry;
    }
    void reliefNumber(J const& relief){
        auto key=object(relief,L"numeric").Stringify()+L"|"+data->theme();
        if(key==reliefKey)return;
        reliefKey=key;reliefBinds.clear();reliefAdmissions.clear();
        if(reliefEntry)detach(reliefEntry);
        reliefEntry=numberField(object(relief,L"numeric"),L"relief",L"paper_relief",L"new-drawing-paper-relief",
            [](J const& current){return num(object(object(current,L"drawer"),L"relief"),L"value");},reliefBinds,reliefAdmissions);
        reliefEntry.Padding({10,7,0,0});column(reliefSlot,reliefEntry,0);
    }
    void numbers(J const& view){
        auto key=object(view,L"numeric").Stringify()+L"|"+object(view,L"resolution_numeric").Stringify()+L"|"+data->theme();
        if(key==numericKey)return;
        numericKey=key;numberBinds.clear();for(auto& list:admissions)list.clear();
        auto make=[&](size_t index)->TextBox{
            auto value=[index](J const& current){return index<2?array(current,L"values").GetNumberAt(uint32_t(index)):num(object(current,L"setup"),L"resolution");};
            auto spec=index<2?object(view,L"numeric"):object(view,L"resolution_numeric");
            wchar_t const* titleKey=index==0?L"width_name":index==1?L"height_name":L"resolution";
            wchar_t const* operation=index==0?L"width":index==1?L"height":L"resolution";
            hstring id=index==0?L"document-width":index==1?L"document-height":L"document-resolution";
            return numberField(spec,titleKey,operation,id,value,numberBinds,admissions[index]);
        };
        for(size_t i=0;i<2;++i){
            auto slot=fields[i].Child().as<Grid>();
            while(slot.Children().Size()>1)slot.Children().RemoveAtEnd();
            column(slot,make(i),1);
        }
        if(resolutionEntry)detach(resolutionEntry);
        resolutionEntry=make(2);resolutionEntry.Padding({10,7,0,0});column(resolutionSlot,resolutionEntry,0);
    }
    static void detach(FrameworkElement const& element){
        if(auto parent=VisualTreeHelper::GetParent(element).try_as<Panel>()){uint32_t at=0;if(parent.Children().IndexOf(element,at))parent.Children().RemoveAt(at);}
        else if(auto panel=element.Parent().try_as<Panel>()){uint32_t at=0;if(panel.Children().IndexOf(element,at))panel.Children().RemoveAt(at);}
    }
    void fillResolutions(J const& view){
        auto choices=array(view,L"resolutions");auto key=choices.Stringify();
        if(key==resolutionsKey)return;
        resolutionsKey=key;resolutionItems.Children().Clear();auto weak=weak_from_this();
        for(uint32_t i=0;i<choices.Size();++i){
            auto choice=choices.GetObjectAt(i);auto value=num(choice,L"value");
            auto item=button(data,str(choice,L"label"),[weak,value]{if(auto self=weak.lock()){self->resolutionMenu.Hide();self->send(O({{L"op",S(L"resolution")},{L"value",N(value)}}));}});
            item.HorizontalAlignment(HorizontalAlignment::Stretch);item.HorizontalContentAlignment(HorizontalAlignment::Stretch);item.MinHeight(InputHeight);item.Padding({10,0,10,0});
            item.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());item.IsEnabled(flag(choice,L"enabled"));
            if(flag(choice,L"selected"))item.Background(selected(data));
            Grid content;content.ColumnSpacing(16);columns(content,{star(),autoWidth()});
            auto amount=caption();amount.Text(str(choice,L"label"));auto purpose=caption(false,1,.6);purpose.Text(str(choice,L"purpose"));
            column(content,amount,0);column(content,purpose,1);content.MinWidth(200);item.Content(content);
            AutomationProperties::SetAutomationId(item,L"new-drawing-resolution-"+to_hstring(i));
            AutomationProperties::SetName(item,str(choice,L"label")+L" "+str(choice,L"purpose"));
            AutomationProperties::SetItemStatus(item,flag(choice,L"selected")?data->caption(L"search",L"selected"):hstring());
            resolutionItems.Children().Append(item);
        }
    }
    void render(J const& view){
        dialogSurface(data,dialog);
        if(view.Stringify()==shown.Stringify())return;
        shown=view;
        data->updating=true;
        struct Reset{bool& flag;~Reset(){flag=false;}} reset{data->updating};
        if(auto key=object(view,L"copy").Stringify()+data->language();key!=copyKey){copyKey=key;relabel(view);}
        renderPage(view);
        renderDrawer(view);
    }
    void renderPage(J const& view){
        auto setup=object(view,L"setup"),pageView=object(view,L"page"),sheet=object(view,L"sheet");
        name.PlaceholderText(str(view,L"untitled"));
        if(name.Text()!=str(view,L"name")&&name.FocusState()==FocusState::Unfocused)name.Text(str(view,L"name"));
        numbers(view);for(auto const& bind:numberBinds)bind();
        bool linked=flag(view,L"linked");mark(link,linked,clear());link.Content(icon(linked?L"link":L"unlink",data->theme()));
        items(unit,unitsKey,array(view,L"units"),labelOf,valued(setup.GetNamedValue(L"unit")),L"new-drawing-unit");
        markOrientation();
        fillResolutions(view);
        sizeLine.Text(str(pageView,L"size"));
        bool problem=flag(pageView,L"problem");sizeLine.Foreground(problem?fill(Problem):data->brush(L"text"));sizeLine.Opacity(problem?1:.6);
        auto colorChoice=object(setup,L"color");
        items(space,spacesKey,array(view,L"spaces"),labelOf,valued(colorChoice.GetNamedValue(L"space")),L"document-space");
        items(depth,depthsKey,array(view,L"depths"),labelOf,valued(colorChoice.GetNamedValue(L"depth")),L"document-depth");
        items(blending,blendingsKey,array(view,L"blendings"),labelOf,valued(setup.GetNamedValue(L"blend_space")),L"document-blending");
        auto note=view.GetNamedValue(L"blending_note",JsonValue::CreateNullValue());
        explain(blendingHost,blending,note.ValueType()!=JsonValueType::String,text(L"blending"),note.ValueType()==JsonValueType::String?note.GetString():hstring());
        textureThumbnail->show(sheet.GetNamedValue(L"texture",JsonValue::CreateNullValue()),str(pageView,L"texture"));
        textureName.Text(str(pageView,L"texture"));
        auto relief=pageView.GetNamedValue(L"relief",JsonValue::CreateNullValue());bool textured=relief.ValueType()==JsonValueType::String;
        textureRelief.Text(textured?relief.GetString():hstring());textureRelief.Visibility(textured?Visibility::Visible:Visibility::Collapsed);
        auto paint=displayed({object(sheet,L"color")})[0];
        if(flag(sheet,L"shown"))paperPatch.Fill(fill(paint));else paperPatch.Fill(checks());
        paperColorName.Text(str(pageView,L"paper_color"));paperColor.IsEnabled(flag(view,L"paper_color_enabled"));
        art.show(data,checks(),sheet,paint);
        captionText.Inlines().Clear();
        Documents::Run lead;lead.Text(str(pageView,L"preset"));lead.FontWeight(winrt::Windows::UI::Text::FontWeights::SemiBold());
        Documents::Run tail;tail.Text(L" · "+str(pageView,L"orientation_label"));tail.Foreground(data->tint(L"text",153));
        captionText.Inlines().Append(lead);captionText.Inlines().Append(tail);
        AutomationProperties::SetName(captionText,str(pageView,L"preset")+L" · "+str(pageView,L"orientation_label"));
        capacity.Text(str(pageView,L"capacity"));tooltip(capacity,str(pageView,L"memory"));
        bool covered=view.GetNamedValue(L"drawer",JsonValue::CreateNullValue()).ValueType()==JsonValueType::Object;
        gate.IsEnabled(!covered);presets.IsEnabled(!covered);cancel.IsEnabled(!covered);
        bool creatable=!covered&&flag(view,L"can_create");create.IsEnabled(creatable);
    }
    void renderDrawer(J const& view){
        auto drawerValue=view.GetNamedValue(L"drawer",JsonValue::CreateNullValue());
        auto current=drawerValue.ValueType()==JsonValueType::Object?drawerValue.GetObject():J{};
        auto kind=str(current,L"kind");
        bool opening=kind!=drawerKind;auto closed=drawerKind;drawerKind=kind;
        if(kind==L"presets")renderPresets(current,opening);
        else if(kind==L"paper")renderPaper(current,opening);
        else if(kind==L"paper_color")renderColors(current,opening);
        for(auto [panel,which]:{std::pair{&presetsDrawer,L"presets"},std::pair{&paperDrawer,L"paper"},std::pair{&colorDrawer,L"paper_color"}})slide(*panel,kind==which);
        if(!opening)return;
        if(kind==L"presets")search.Focus(FocusState::Programmatic);
        else if(kind==L"paper"){papers->focus(selectedPaper(current));}
        else if(kind.empty()){
            if(closed==L"presets")presets.Focus(FocusState::Programmatic);
            else if(closed==L"paper")texture.Focus(FocusState::Programmatic);
            else if(closed==L"paper_color")paperColor.Focus(FocusState::Programmatic);
        }
    }
    void settle(Drawer& target){
        if(target.motion){target.motion.Stop();target.motion=nullptr;}
        target.shift.Y(target.open?0:root.ActualHeight());
        if(target.open)return;
        target.root.Visibility(Visibility::Collapsed);
    }
    void slide(Drawer& target,bool visible){
        if(target.open==visible)return;target.open=visible;
        auto height=root.ActualHeight();
        if(target.motion){target.motion.Stop();target.motion=nullptr;}
        if(visible){target.root.Visibility(Visibility::Visible);if(height>0)target.shift.Y(height);}
        if(!winrt::Windows::UI::ViewManagement::UISettings().AnimationsEnabled()||height<=0){settle(target);return;}
        Animation::DoubleAnimation motion;motion.From(target.shift.Y());motion.To(visible?0:height);motion.Duration(DurationHelper::FromTimeSpan(std::chrono::milliseconds(250)));
        Animation::CubicEase ease;ease.EasingMode(Animation::EasingMode::EaseOut);motion.EasingFunction(ease);
        target.motion=Animation::Storyboard();Animation::Storyboard::SetTarget(motion,target.shift);Animation::Storyboard::SetTargetProperty(motion,L"Y");target.motion.Children().Append(motion);
        target.motion.Completed([weak=weak_from_this(),panel=&target,visible](auto&&,auto&&){if(auto self=weak.lock();self&&panel->open==visible)self->settle(*panel);});
        target.motion.Begin();
    }

    void renderPresets(J const& view,bool opening){
        auto weak=weak_from_this();
        if(search.Text()!=str(view,L"query")&&(opening||search.FocusState()==FocusState::Unfocused))search.Text(str(view,L"query"));
        auto tabList=array(view,L"tabs");
        if(auto key=tabList.Stringify();key!=tabsKey){
            tabsKey=key;tabs.Children().Clear();
            for(uint32_t i=0;i<tabList.Size();++i){
                auto tab=tabList.GetObjectAt(i);
                auto pill=toggle(L"new-drawing-tab-"+to_hstring(i),0,28);pill.Padding({10,0,10,0});
                pill.Content(box_value(str(tab,L"label")));AutomationProperties::SetName(pill,str(tab,L"label"));
                pill.Click([weak,i](auto&&,auto&&){if(auto self=weak.lock())self->send(O({{L"op",S(L"tab")},{L"index",N(i)}}));});
                tabs.Children().Append(pill);
            }
        }
        tabs.Opacity(flag(view,L"tabs_enabled")?1:.4);
        for(uint32_t i=0;i<tabs.Children().Size()&&i<tabList.Size();++i){
            auto pill=tabs.Children().GetAt(i).as<Button>();pill.IsEnabled(flag(view,L"tabs_enabled"));
            mark(pill,flag(tabList.GetObjectAt(i),L"selected"),buttonBackground(data));
        }
        auto tiles=array(view,L"tiles");
        A stripped;for(auto value:tiles){auto tile=J::Parse(value.Stringify());tile.Remove(L"selected");stripped.Append(tile);}
        if(auto key=stripped.Stringify();key!=tilesKey||opening){tilesKey=key;fillPresets(tiles);}
        for(uint32_t i=0;i<tiles.Size()&&i<presetFrames.size();++i){
            bool on=flag(tiles.GetObjectAt(i),L"selected");
            presetFrames[i].BorderBrush(on?accent(data):clear());
            AutomationProperties::SetItemStatus(presetTiles[i],on?data->caption(L"search",L"selected"):hstring());
        }
        auto message=view.GetNamedValue(L"empty",JsonValue::CreateNullValue());
        empty.Visibility(message.ValueType()==JsonValueType::String?Visibility::Visible:Visibility::Collapsed);
        if(message.ValueType()==JsonValueType::String)empty.Text(message.GetString());
        showSheet(summaryArt,object(view,L"summary_sheet"));
        auto about=object(view,L"summary");
        summaryName.Text(str(about,L"name"));summarySize.Text(str(about,L"size"));summaryDetails.Text(str(about,L"details"));
        pickerLabel(saveAs,str(view,L"save_label"));select.IsEnabled(flag(view,L"can_select"));
        auto saveValue=view.GetNamedValue(L"save",JsonValue::CreateNullValue());
        bool saving=saveValue.ValueType()==JsonValueType::Object;
        bool starting=saving&&footerPages->selected!=1;
        if(!saving){footerPages->show(0);return;}
        auto save=saveValue.GetObject();
        items(saveGroup,groupsKey,array(save,L"groups"),labelOf,valued(save.GetNamedValue(L"group")),L"new-drawing-save-group");
        if(saveName.Text()!=str(save,L"name")&&(starting||saveName.FocusState()==FocusState::Unfocused))saveName.Text(str(save,L"name"));
        pickerLabel(saveConfirm,str(save,L"confirm"));
        bool valid=flag(save,L"can_save");saveConfirm.IsEnabled(valid);
        saveName.BorderThickness(valid?Thickness{0}:Thickness{1,1,1,1});saveName.BorderBrush(fill(Problem));
        footerPages->show(1);
        if(starting){saveName.Focus(FocusState::Programmatic);saveName.SelectAll();}
    }
    void fillPresets(A const& tiles){
        auto weak=weak_from_this();
        presetGrid.Children().Clear();presetTiles.clear();presetFrames.clear();presetArts.clear();
        auto colors=[&]{std::vector<J> list;for(auto value:tiles)list.push_back(object(object(value.GetObject(),L"sheet"),L"color"));return displayed(list);}();
        for(uint32_t index=0;index<tiles.Size();++index){
            auto tile=tiles.GetObjectAt(index);
            auto sheet=std::make_shared<SheetArt>();sheet->init({PresetFrameWidth,PresetFrameHeight},{PresetFit,PresetFit},37);
            sheet->show(data,checks(),object(tile,L"sheet"),colors[index]);
            Border frame;frame.Width(PresetFrameWidth);frame.Height(PresetFrameHeight);frame.CornerRadius({10,10,10,10});frame.BorderThickness({2,2,2,2});
            frame.BorderBrush(clear());frame.Background(clear());frame.HorizontalAlignment(HorizontalAlignment::Center);frame.Child(sheet->root);
            auto line=[&](hstring const& value,bool dim,int lines){
                auto result=caption(false,dim?.82:1,dim?.6:1);result.Text(value);result.TextAlignment(TextAlignment::Center);result.HorizontalAlignment(HorizontalAlignment::Stretch);
                result.TextWrapping(TextWrapping::Wrap);result.MaxLines(lines);return result;
            };
            StackPanel content;content.Width(PresetTile);content.Spacing(4);content.Children().Append(frame);
            StackPanel words;words.Spacing(1);words.Children().Append(line(str(tile,L"name"),false,2));words.Children().Append(line(str(tile,L"dimensions"),true,1));words.Children().Append(line(str(tile,L"details"),true,2));
            content.Children().Append(words);
            auto node=button(data,str(tile,L"name"),[weak,index]{if(auto self=weak.lock())self->send(O({{L"op",S(L"pick_preset")},{L"index",N(index)}}));});
            node.IsEnabled(flag(tile,L"enabled"));
            node.Padding({0,0,0,0});node.VerticalAlignment(VerticalAlignment::Top);node.HorizontalAlignment(HorizontalAlignment::Center);node.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());
            node.Content(content);AutomationProperties::SetAutomationId(node,L"new-drawing-preset-"+to_hstring(index));
            tooltip(node,str(tile,L"name")+L"\n"+str(tile,L"dimensions")+L" · "+str(tile,L"details"));
            frame.Background(buttonFeedbackBrush(node,data->brush(L"text")));
            node.DoubleTapped([weak](auto&&,DoubleTappedRoutedEventArgs const& e){if(auto self=weak.lock()){e.Handled(true);self->send(op(L"use_preset"));}});
            if(flag(tile,L"own")){
                MenuFlyout menu;AutomationProperties::SetAutomationId(menu,L"new-drawing-preset-menu");
                for(auto [key,action,id]:{std::tuple{L"rename",L"rename_preset",L"new-drawing-preset-rename-"},std::tuple{L"delete",L"delete_preset",L"new-drawing-preset-delete-"}}){
                    MenuFlyoutItem item;item.Text(text(key));AutomationProperties::SetAutomationId(item,hstring(id)+to_hstring(index));
                    auto chosen=O({{L"op",S(action)},{L"index",N(index)}});
                    item.Click([weak,chosen](auto&&,auto&&){if(auto self=weak.lock())self->send(chosen);});
                    menu.Items().Append(item);
                }
                node.ContextFlyout(menu);
            }
            presetGrid.Children().Append(node);presetTiles.push_back(node);presetFrames.push_back(frame);presetArts.push_back(sheet);
        }
        presetColumns=0;arrangePresets();
    }
    void arrangePresets(){
        double width=presetGrid.ActualWidth();if(width<=0||presetTiles.empty())return;
        int next=std::clamp(int((width+TileGap)/(PresetTile+TileGap)),2,8);
        if(next!=presetColumns){
            presetColumns=next;presetGrid.ColumnDefinitions().Clear();
            for(int i=0;i<presetColumns;++i){ColumnDefinition definition;definition.Width(star());presetGrid.ColumnDefinitions().Append(definition);}
        }
        auto rows=int((presetTiles.size()+presetColumns-1)/presetColumns);
        while(int(presetGrid.RowDefinitions().Size())<rows){RowDefinition row;row.Height({1,GridUnitType::Auto});presetGrid.RowDefinitions().Append(row);}
        while(int(presetGrid.RowDefinitions().Size())>rows)presetGrid.RowDefinitions().RemoveAtEnd();
        for(size_t i=0;i<presetTiles.size();++i){Grid::SetColumn(presetTiles[i],int(i)%presetColumns);Grid::SetRow(presetTiles[i],int(i)/presetColumns);}
    }
    static hstring selectedPaper(J const& view){
        for(auto value:array(view,L"tiles")){auto tile=value.GetObject();if(flag(tile,L"selected"))return textureTileId(tile);}
        return L"";
    }
    void renderPaper(J const& view,bool opening){
        auto tiles=array(view,L"tiles");
        A stripped;for(auto value:tiles){auto tile=value.GetObject();stripped.Append(O({{L"id",tile.GetNamedValue(L"id",JsonValue::CreateNullValue())},{L"label",tile.GetNamedValue(L"label",JsonValue::CreateNullValue())}}));}
        if(auto key=stripped.Stringify();key!=paperKey||opening){paperKey=key;papers->rebuild(stripped);}
        papers->mark(selectedPaper(view));
        auto relief=object(view,L"relief");reliefNumber(relief);
        if(opening)for(auto const& admit:reliefAdmissions)admit(true);
        for(auto const& bind:reliefBinds)bind();
        bool enabled=flag(relief,L"enabled");reliefEntry.IsEnabled(enabled);reliefRow.Opacity(enabled?1:.5);
    }
    void renderColors(J const& view,bool opening){
        auto weak=weak_from_this();
        if(!form){
            form=EmbedColor(data,object(view,L"color"),
                [weak](J const& value){if(auto self=weak.lock())self->send(O({{L"op",S(L"paper_color_value")},{L"color",value}}));},
                [weak](bool blocked){if(auto self=weak.lock())self->colorUse.IsEnabled(!blocked);});
            if(!form)return;
            form->arrange(narrow);
            Grid::SetColumn(well,2);Grid::SetRow(well,0);form->head.Children().Append(well);
            StackPanel heading;heading.Orientation(Orientation::Horizontal);heading.Spacing(8);
            for(auto const& part:{paletteTitle,paletteCount}){detach(part);heading.Children().Append(part);}
            detach(swatches);form->below.Spacing(6);form->below.Children().Append(heading);form->below.Children().Append(swatches);
            colorDrawer.body.Children().Append(form->body);
        }else if(opening){
            form->escape();
            form->choose(object(view,L"color"));
        }
        markTransparency();
        paletteCount.Text(str(view,L"count"));
        auto palette=array(view,L"palette");
        A colors;for(auto value:palette){auto choice=value.GetObject();colors.Append(O({{L"color",choice.GetNamedValue(L"color")},{L"name",choice.GetNamedValue(L"name")}}));}
        if(auto key=colors.Stringify();key!=paletteKey){paletteKey=key;fillPalette(palette);}
        for(uint32_t i=0;i<palette.Size()&&i<swatchTiles.size();++i){
            bool on=flag(palette.GetObjectAt(i),L"selected");
            swatchTiles[i].BorderBrush(on?Brush(accent(data)):Brush(clear()));swatchTiles[i].BorderThickness(on?Thickness{2,2,2,2}:Thickness{0});
            AutomationProperties::SetItemStatus(swatchTiles[i],on?data->caption(L"search",L"selected"):hstring());
        }
        addSwatch.IsEnabled(flag(view,L"can_add"));addSwatch.Opacity(flag(view,L"can_add")?1:.5);
        form->transparent(flag(view,L"transparent")?std::optional<hstring>(text(L"transparent")):std::nullopt);
    }
    void fillPalette(A const& palette){
        std::vector<J> values;for(auto choice:palette)values.push_back(object(choice.GetObject(),L"color"));
        auto paints=displayed(values);
        for(uint32_t index=0;index<swatchTiles.size();++index){
            bool shown=index<palette.Size();swatchTiles[index].Visibility(shown?Visibility::Visible:Visibility::Collapsed);
            if(!shown)continue;
            auto name=str(palette.GetObjectAt(index),L"name");swatchPaints[index].Color(paints[index]);
            tooltip(swatchTiles[index],name);AutomationProperties::SetName(swatchTiles[index],name);
        }
        swatchValues=std::move(values);
    }
};

NewDrawingDialog::NewDrawingDialog(std::shared_ptr<WorkspaceData> data,Dispatch document,Window window,std::function<void()> changed):impl(std::make_shared<Impl>()){
    impl->data=std::move(data);impl->document=std::move(document);impl->window=window;impl->changed=std::move(changed);
}
NewDrawingDialog::~NewDrawingDialog(){impl->hide();}
void NewDrawingDialog::Open(J const& envelope){impl->open(envelope);}
void NewDrawingDialog::Apply(){impl->apply();}
bool NewDrawingDialog::IsOpen()const{return impl->waiting||impl->showing;}
void NewDrawingDialog::Hide(){impl->hide();}
}
