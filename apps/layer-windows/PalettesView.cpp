#include "pch.h"
#include "PalettesView.h"
#include "WorkspaceQuery.h"
#include "NativeMenus.h"
#include "Checker.h"
#include <winrt/Microsoft.UI.Composition.h>
#include <winrt/Microsoft.UI.Input.h>
#include <winrt/Microsoft.UI.Xaml.Controls.Primitives.h>
#include <winrt/Microsoft.Windows.Storage.Pickers.h>
#include <winrt/Windows.UI.ViewManagement.h>
#include <cwctype>
#include <limits>
#include <set>

using namespace CapyUi;
namespace Pickers=winrt::Microsoft::Windows::Storage::Pickers;
namespace {
constexpr double Cell=40,Gap=4,Pitch=Cell+Gap;
struct Tile{uint64_t id=0;Button node{nullptr};Border patch{nullptr};J swatch;double x=0,y=0;Microsoft::UI::Composition::Vector3KeyFrameAnimation animation{nullptr};};
struct Drag{
    uint32_t pointer=0;uint64_t id=0,palette=0;winrt::Windows::Foundation::Point origin{},point{},grab{};
    bool started=false,held=false;std::optional<int> slot;J action;uint64_t generation=0;
    Primitives::Popup ghost{nullptr};Microsoft::UI::Input::GestureRecognizer hold{nullptr};
};
struct PalettesView:std::enable_shared_from_this<PalettesView>{
    std::shared_ptr<WorkspaceData> data;
    Grid root,body,chooser,footer;StackPanel normal,list,info;Canvas history,grid,expanded,overlays;ScrollViewer scroll,results;
    TextBox search,editor;Button more{nullptr},selector{nullptr},name{nullptr},add{nullptr};TextBlock selectorLabel,nameLabel,detail,note,empty;
    std::map<uint64_t,Tile> tiles;std::vector<uint64_t> order;std::vector<FrameworkElement> historyCells,expandedCells;
    hstring viewKey,historyKey,choicesKey,currentKey,checkerKey;Imaging::WriteableBitmap checker{nullptr};
    std::optional<uint64_t> selected;bool editing=false,animate=true;int columns=6;uint64_t reportGeneration=0;
    std::optional<Drag> drag;Microsoft::UI::Dispatching::DispatcherQueueTimer scrollTimer{nullptr};double scrollCarry=0;
    J view()const{return object(data->model,L"palette_panel");}
    J files()const{return object(data->model,L"windows_palettes");}
    J current()const{return object(object(data->model,L"color_panel"),L"definition");}
    std::optional<J> swatch(uint64_t id)const{for(auto v:array(view(),L"swatches"))if(uint64_t(num(v.GetObject(),L"id"))==id)return v.GetObject();return std::nullopt;}

    void report(hstring const& text,bool notice=false){
        note.Text(text);note.Visibility(text.empty()?Visibility::Collapsed:Visibility::Visible);
        note.Foreground(notice?data->tint(L"text",179):SolidColorBrush(winrt::Windows::UI::Color{255,0xee,0x55,0x55}));
        editor.BorderThickness({0,0,0,0});if(!notice&&!text.empty()&&editing){editor.BorderBrush(SolidColorBrush(winrt::Windows::UI::Color{255,0xee,0x55,0x55}));editor.BorderThickness({1,1,1,1});}
    }
    void library(J action,std::function<void(hstring)> done=nullptr,bool dryRun=false){
        auto weak=weak_from_this();
        QueryWorkspace(data->query,O({{L"type",S(L"palette_action")},{L"action",action},{L"dry_run",B(dryRun)}}),[weak,done,dryRun](J reply){
            auto self=weak.lock();if(!self)return;
            auto error=str(object(reply,L"result"),L"error",str(reply,L"error"));
            if(!dryRun)self->report(error);
            if(done)done(error);
        });
    }
    void reveal(){QueryWorkspace(data->query,O({{L"type",S(L"reveal_panel")},{L"panel",S(L"palettes")}}),[](J){});}
    Border paint(){
        Border patch;patch.CornerRadius({6,6,6,6});patch.IsHitTestVisible(false);
        ImageBrush pixels;pixels.ImageSource(checker);pixels.Stretch(Stretch::None);pixels.AlignmentX(AlignmentX::Left);pixels.AlignmentY(AlignmentY::Top);patch.Background(pixels);
        Border fill;fill.CornerRadius({6,6,6,6});patch.Child(fill);return patch;
    }
    static winrt::Windows::UI::Color rgbaColor(A const& rgba){
        auto c=[&](uint32_t i){return uint8_t(std::lround(std::clamp(i<rgba.Size()?rgba.GetNumberAt(i):1.,0.,1.)*255));};
        return winrt::Windows::UI::Color{c(3),c(0),c(1),c(2)};
    }
    void fillPatch(Border const& patch,A const& rgba){patch.Child().as<Border>().Background(SolidColorBrush(rgbaColor(rgba)));}
    void ensureChecker(){
        auto palette=object(data->state,L"palette");auto scale=root.XamlRoot()?root.XamlRoot().RasterizationScale():1.;
        auto key=str(palette,L"checker_light")+str(palette,L"checker_dark")+to_hstring(scale);
        if(checker&&key==checkerKey)return;checkerKey=key;
        checker=checkerBitmap(80,scale,color(str(palette,L"checker_light")),color(str(palette,L"checker_dark")));
    }
    Button tileButton(hstring const& label){
        auto node=button(data,label,[]{});node.Height(Cell);node.Padding({3,3,3,3});node.CornerRadius({6,6,6,6});
        node.HorizontalContentAlignment(HorizontalAlignment::Stretch);node.VerticalContentAlignment(VerticalAlignment::Stretch);return node;
    }
    void place(FrameworkElement const& node,int index,double cell){
        Canvas::SetLeft(node,std::round((index%columns)*(cell+Gap)));Canvas::SetTop(node,(index/columns)*Pitch);node.Width(cell);node.Height(Cell);
    }
    double cellWidth(double width)const{return std::max(Cell,(width-Gap*(columns-1))/columns);}
    void arrange(){
        double width=scroll.ActualWidth();if(width<=0)return;
        columns=std::max(1,int((width+Gap)/Pitch));auto cell=cellWidth(width);
        int index=0;for(auto id:order){auto& t=tiles.at(id);place(t.node,index,cell);t.x=std::round((index%columns)*(cell+Gap));t.y=(index/columns)*Pitch;++index;}
        place(add,index,cell);++index;
        int rows=(index+columns-1)/columns;grid.Width(width);grid.Height(std::max(0.,rows*Pitch-Gap));
        arrangeHistory(historyCells,1,cell);
        if(expanded.Visibility()==Visibility::Visible){
            int limit=std::min(4,std::max(1,int((body.ActualHeight()+Gap)/Pitch)));arrangeHistory(expandedCells,limit,cell);
        }
        history.Height(Cell);
    }
    void arrangeHistory(std::vector<FrameworkElement>& cells,int rows,double cell){
        if(cells.empty())return;int capacity=rows*columns;
        for(size_t i=0;i+1<cells.size();++i){bool shown=int(i)<capacity-1;cells[i].Visibility(shown?Visibility::Visible:Visibility::Collapsed);if(shown)place(cells[i],int(i),cell);}
        place(cells.back(),capacity-1,cell);
    }
    void historyTiles(Canvas const& target,std::vector<FrameworkElement>& cells,bool open){
        target.Children().Clear();cells.clear();auto weak=weak_from_this();
        auto entries=array(view(),L"history");
        for(auto value:entries){
            auto entry=value.GetObject();auto node=tileButton(str(entry,L"detail"));auto patch=paint();fillPatch(patch,array(entry,L"rgba"));node.Content(patch);
            tooltip(node,str(entry,L"detail"));auto color=object(entry,L"color");
            node.Click([weak,color](auto&&,auto&&){if(auto self=weak.lock()){
                if(self->editing){self->commitName();if(self->editing)return;}
                self->selected.reset();self->data->dispatch(O({{L"type",S(L"color")},{L"action",O({{L"op",S(L"definition")},{L"color",color}})}}));self->report(L"");
            }});
            AutomationProperties::SetAutomationId(node,L"palette-recent-"+to_hstring(uint32_t(cells.size())));
            target.Children().Append(node);cells.push_back(node);
        }
        if(!entries.Size())for(int i=0;i<5;i++){Border blank,fill;blank.Padding({3,3,3,3});fill.CornerRadius({6,6,6,6});fill.Background(data->tint(L"text",13));blank.Child(fill);
            tooltip(blank,L"Colors appear here after painting");target.Children().Append(blank);cells.push_back(blank);}
        auto toggle=button(data,open?L"Collapse color history":L"Expand color history",[weak,open]{if(auto self=weak.lock())self->expand(!open);});
        toggle.Height(Cell);toggle.Padding({0,0,0,0});auto chevron=icon(L"chevron-down",data->theme());chevron.RenderTransformOrigin({.5f,.5f});
        if(open){RotateTransform turn;turn.Angle(180);chevron.RenderTransform(turn);}
        toggle.Content(chevron);tooltip(toggle,open?L"Collapse color history":L"Expand color history");
        AutomationProperties::SetAutomationId(toggle,open?L"palette-history-collapse":L"palette-history-expand");
        target.Children().Append(toggle);cells.push_back(toggle);
    }
    void expand(bool open){
        chooser.Visibility(Visibility::Collapsed);expanded.Visibility(open?Visibility::Visible:Visibility::Collapsed);normal.IsHitTestVisible(!open);normal.Opacity(open?0:1);
        arrange();if(open&&!expandedCells.empty())expandedCells.back().as<Control>().Focus(FocusState::Programmatic);
        else if(!open&&!historyCells.empty())if(auto c=historyCells.back().try_as<Control>())c.Focus(FocusState::Programmatic);
    }
    void browse(bool show,bool typing=true){
        if(show&&editing){commitName();if(editing)return;}
        expanded.Visibility(Visibility::Collapsed);chooser.Visibility(show?Visibility::Visible:Visibility::Collapsed);normal.IsHitTestVisible(!show);normal.Opacity(show?0:1);
        if(!show){selector.Focus(FocusState::Programmatic);return;}
        filter();
        Control target=search;
        if(!typing)for(auto child:list.Children())if(auto row=child.try_as<Button>();row&&AutomationProperties::GetItemStatus(row)==L"Selected")target=row;
        target.Focus(FocusState::Programmatic);
    }
    void filter(){
        std::wstring query=search.Text().c_str();for(auto& ch:query)ch=wchar_t(std::towlower(ch));
        auto trim=[](std::wstring s){auto a=s.find_first_not_of(L" \t");auto b=s.find_last_not_of(L" \t");return a==std::wstring::npos?std::wstring{}:s.substr(a,b-a+1);};
        query=trim(query);int visible=0;
        for(auto child:list.Children()){
            auto row=child.as<Button>();std::wstring label=AutomationProperties::GetName(row).c_str();for(auto& ch:label)ch=wchar_t(std::towlower(ch));
            bool shown=label.find(query)!=std::wstring::npos;row.Visibility(shown?Visibility::Visible:Visibility::Collapsed);visible+=shown;
        }
        empty.Visibility(visible?Visibility::Collapsed:Visibility::Visible);
    }
    void editName(){
        editing=true;hstring value;
        if(selected)if(auto s=swatch(*selected))value=str(*s,L"name");
        if(!selected){auto pending=array(object(object(data->state,L"colors"),L"library"),L"pending_name");
            if(pending.Size()==2&&pending.GetAt(1).ValueType()==JsonValueType::String&&pending.GetStringAt(1)==str(view(),L"color_name"))value=pending.GetStringAt(1);}
        editor.Text(value);name.Visibility(Visibility::Collapsed);editor.Visibility(Visibility::Visible);editor.Focus(FocusState::Programmatic);editor.SelectAll();
    }
    void showLabel(){editor.Visibility(Visibility::Collapsed);name.Visibility(Visibility::Visible);}
    void commitName(){
        if(!editing)return;editing=false;auto weak=weak_from_this();
        auto action=selected?O({{L"op",S(L"rename")},{L"id",N(double(*selected))},{L"name",S(editor.Text())}}):O({{L"op",S(L"name_current")},{L"color",current()},{L"name",S(editor.Text())}});
        showLabel();
        library(action,[weak](hstring error){if(auto self=weak.lock();self&&!error.empty()){self->editing=true;self->name.Visibility(Visibility::Collapsed);self->editor.Visibility(Visibility::Visible);self->report(error);self->editor.Focus(FocusState::Programmatic);}});
    }
    void cancelName(){editing=false;showLabel();report(L"");name.Focus(FocusState::Programmatic);}
    void addColor(){
        if(editing){commitName();if(editing)return;}
        auto weak=weak_from_this();
        library(O({{L"op",S(L"store")},{L"palette",N(num(view(),L"palette"))},{L"name",S(L"")},{L"color",current()}}),[weak](hstring error){
            if(auto self=weak.lock();self&&error.empty())self->selectNewest=true;
        });
    }
    bool selectNewest=false;
    MenuFlyout openMenu{nullptr};
    void menu(J target,FrameworkElement const& anchor,hstring const& title){
        auto weak=weak_from_this();auto held=make_weak(anchor);
        QueryWorkspace(data->query,O({{L"type",S(L"palette_menu")},{L"target",target}}),[weak,held,title](J reply){
            auto self=weak.lock();auto anchor=held.get();if(!self||!anchor||(self->drag&&self->drag->started))return;
            auto sections=array(reply,L"result");if(!sections.Size())return;
            MenuFlyout flyout;TrackPopup(flyout,self->data);self->fillMenu(flyout.Items(),sections);
            AutomationProperties::SetName(flyout,title);self->openMenu=flyout;flyout.ShowAt(anchor);
        });
    }
    void fillMenu(winrt::Windows::Foundation::Collections::IVector<MenuFlyoutItemBase> const& target,A const& sections){
        auto weak=weak_from_this();bool first=true;
        for(auto sectionValue:sections){
            auto section=sectionValue.GetArray();if(!section.Size())continue;if(!first)target.Append(MenuFlyoutSeparator());first=false;
            for(auto value:section){
                auto item=value.GetObject();auto label=str(item,L"label");auto children=array(item,L"sections");
                if(children.Size()){MenuFlyoutSubItem sub;sub.Text(label);sub.IsEnabled(flag(item,L"enabled",true));sub.FontSize(data->textSize());fillMenu(sub.Items(),children);target.Append(sub);continue;}
                MenuFlyoutItem entry;entry.Text(label);entry.IsEnabled(flag(item,L"enabled",true));entry.FontSize(data->textSize());entry.MinHeight(34);
                auto command=object(item,L"command");AutomationProperties::SetAutomationId(entry,L"palette-command-"+str(command,L"command")+(command.HasKey(L"format")?L"-"+str(command,L"format"):L""));
                entry.Click([weak,command,label](auto&&,auto&&){if(auto self=weak.lock())self->run(command,label);});target.Append(entry);
            }
        }
    }
    hstring paletteName(uint64_t id)const{for(auto v:array(view(),L"palettes"))if(uint64_t(num(v.GetObject(),L"id"))==id)return str(v.GetObject(),L"name");return L"";}
    void run(J command,hstring const& label){
        auto kind=str(command,L"command");
        if(kind==L"new_palette"||kind==L"rename_palette"){
            bool renaming=kind==L"rename_palette";uint64_t id=renaming?uint64_t(num(command,L"id")):0;
            if(renaming&&paletteName(id).empty())return;
            nameDialog(renaming?L"Rename Palette":L"New Palette",renaming?paletteName(id):L"",[id,renaming](hstring value){
                return renaming?O({{L"op",S(L"rename_palette")},{L"id",N(double(id))},{L"name",S(value)}}):O({{L"op",S(L"create_palette")},{L"name",S(value)}});
            });
            return;
        }
        if(kind==L"import_palette"){importFile();return;}
        if(kind==L"export_palette"){exportFile(uint64_t(num(command,L"id")),str(command,L"format"),label);return;}
        if(kind==L"remove_palette"){removeDialog(uint64_t(num(command,L"id")));return;}
        if(kind==L"rename_color"){
            auto id=uint64_t(num(command,L"id"));if(!swatch(id))return;
            selected=id;library(O({{L"op",S(L"use")},{L"id",N(double(id))}}));editName();return;
        }
        if(kind==L"library")library(object(command,L"action"));
    }
    fire_and_forget nameDialog(hstring title,hstring value,std::function<J(hstring)> action){
        auto lifetime=shared_from_this();
        ContentDialog dialog;dialog.XamlRoot(root.XamlRoot());dialog.Title(box_value(title));dialog.PrimaryButtonText(L"Save");dialog.CloseButtonText(L"Cancel");
        dialog.DefaultButton(ContentDialogButton::Primary);dialog.RequestedTheme(data->theme()==L"light"?ElementTheme::Light:ElementTheme::Dark);
        StackPanel content;content.Spacing(8);TextBox input;input.Header(box_value(L"Name"));input.MaxLength(64);input.Text(value);AutomationProperties::SetAutomationId(input,L"palette-name-input");
        TextBlock error;error.Foreground(SolidColorBrush(winrt::Windows::UI::Color{255,0xee,0x55,0x55}));error.TextWrapping(TextWrapping::Wrap);
        content.Children().Append(input);content.Children().Append(error);dialog.Content(content);
        auto weak=weak_from_this();auto generation=std::make_shared<uint64_t>(0);
        auto shown=make_weak(dialog);auto field=make_weak(input);auto message=make_weak(error);
        auto validate=[weak,shown,field,message,action,generation]{
            auto self=weak.lock();auto input=field.get();if(!self||!input)return;auto id=++*generation;
            self->library(action(input.Text()),[shown,message,id,generation](hstring text){
                if(id!=*generation)return;if(auto error=message.get())error.Text(text);if(auto dialog=shown.get())dialog.IsPrimaryButtonEnabled(text.empty());
            },true);
        };
        input.TextChanged([validate](auto&&,auto&&){validate();});validate();
        dialog.PrimaryButtonClick([weak,shown,field,message,action](auto&&,ContentDialogButtonClickEventArgs const& e){
            e.Cancel(true);auto self=weak.lock();auto input=field.get();if(!self||!input)return;
            self->library(action(input.Text()),[weak,shown,message](hstring text){
                if(!text.empty()){if(auto error=message.get())error.Text(text);return;}
                if(auto dialog=shown.get())dialog.Hide();if(auto self=weak.lock())self->browse(false);
            });
        });
        input.Loaded([field](auto&&,auto&&){if(auto input=field.get()){input.Focus(FocusState::Programmatic);input.SelectAll();}});
        data->popup(true);
        try{co_await dialog.ShowAsync();}catch(hresult_error const&){}
        data->popup(false);
    }
    fire_and_forget removeDialog(uint64_t id){
        auto lifetime=shared_from_this();auto title=paletteName(id);if(title.empty())co_return;
        ContentDialog dialog;dialog.XamlRoot(root.XamlRoot());dialog.Title(box_value(L"Remove Palette?"));
        dialog.Content(box_value(L"Remove “"+title+L"” and its saved colors?"));dialog.PrimaryButtonText(L"Remove");dialog.CloseButtonText(L"Cancel");
        dialog.DefaultButton(ContentDialogButton::Close);dialog.RequestedTheme(data->theme()==L"light"?ElementTheme::Light:ElementTheme::Dark);
        data->popup(true);ContentDialogResult result=ContentDialogResult::None;
        try{result=co_await dialog.ShowAsync();}catch(hresult_error const&){}
        data->popup(false);
        if(result==ContentDialogResult::Primary)library(O({{L"op",S(L"remove_palette")},{L"id",N(double(id))}}));
    }
    fire_and_forget importFile(){
        auto lifetime=shared_from_this();
        if(!data->windowId)co_return;
        Pickers::FileOpenPicker open(Microsoft::UI::WindowId{data->windowId});open.CommitButtonText(L"Import palette");
        for(auto ext:array(files(),L"extensions"))open.FileTypeFilter().Append(L"."+ext.GetString());
        data->popup(true);hstring path;
        try{auto picked=co_await open.PickSingleFileAsync();if(picked)path=picked.Path();}catch(hresult_error const&){}
        data->popup(false);
        if(path.empty())co_return;
        browse(false);
        data->document(to_string(O({{L"operation",S(L"palette")},{L"action",O({{L"op",S(L"import")},{L"path",S(path)}})}}).Stringify()));
    }
    fire_and_forget exportFile(uint64_t id,hstring format,hstring label){
        auto lifetime=shared_from_this();
        auto title=paletteName(id);if(title.empty()||!data->windowId)co_return;
        hstring extension;for(auto v:array(files(),L"formats"))if(str(v.GetObject(),L"format")==format)extension=L"."+str(v.GetObject(),L"extension");
        if(extension.empty())co_return;
        Pickers::FileSavePicker save(Microsoft::UI::WindowId{data->windowId});save.CommitButtonText(L"Export palette");
        save.DefaultFileExtension(extension);save.SuggestedFileName(title);
        save.FileTypeChoices().Insert(label.empty()?extension:label,single_threaded_vector<hstring>({extension}));
        data->popup(true);hstring path;
        try{auto picked=co_await save.PickSaveFileAsync();if(picked)path=picked.Path();}catch(hresult_error const&){}
        data->popup(false);
        if(path.empty())co_return;
        data->document(to_string(O({{L"operation",S(L"palette")},{L"action",O({{L"op",S(L"export")},{L"id",N(double(id))},{L"format",S(format)},{L"path",S(path)}})}}).Stringify()));
    }
    Tile makeTile(uint64_t id){
        Tile t;t.id=id;t.node=tileButton(L"");t.patch=paint();t.node.Content(t.patch);auto weak=weak_from_this();
        AutomationProperties::SetAutomationId(t.node,L"palette-swatch-"+to_hstring(id));
        t.node.Click([weak,id](auto&&,auto&&){if(auto self=weak.lock()){
            if(self->drag&&(self->drag->started||self->drag->held))return;
            if(self->editing){self->commitName();if(self->editing)return;}
            self->selected=id;self->library(O({{L"op",S(L"use")},{L"id",N(double(id))}}));self->sync();
        }});
        t.node.ContextRequested([weak,id](auto&&,ContextRequestedEventArgs const& e){e.Handled(true);if(auto self=weak.lock();self&&!(self->drag&&self->drag->started)){
            auto s=self->swatch(id);self->menu(O({{L"kind",S(L"color")},{L"id",N(double(id))}}),self->tiles.at(id).node,s?str(*s,L"name"):L"");
        }});
        t.node.IsHoldingEnabled(false);
        t.node.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak,id](winrt::Windows::Foundation::IInspectable const&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock())self->press(e,id);})),true);
        return t;
    }
    void press(PointerRoutedEventArgs const& e,uint64_t id){
        if(drag||!tiles.contains(id))return;auto point=e.GetCurrentPoint(root);
        auto type=point.PointerDeviceType();bool mouse=type==Microsoft::UI::Input::PointerDeviceType::Mouse;
        if(mouse&&!point.Properties().IsLeftButtonPressed())return;
        auto node=tiles.at(id).node;auto local=e.GetCurrentPoint(node).Position();
        Drag d;d.pointer=point.PointerId();d.id=id;d.palette=uint64_t(num(view(),L"palette"));d.origin=d.point=point.Position();d.grab=local;
        if(!node.CapturePointer(e.Pointer()))return;
        if(!mouse){
            d.hold=Microsoft::UI::Input::GestureRecognizer();d.hold.GestureSettings(Microsoft::UI::Input::GestureSettings::Hold);
            auto weak=weak_from_this();
            d.hold.Holding([weak,id](auto&&,Microsoft::UI::Input::HoldingEventArgs const& h){if(auto self=weak.lock();self&&self->drag&&!self->drag->started&&h.HoldingState()==Microsoft::UI::Input::HoldingState::Started){
                self->drag->held=true;auto s=self->swatch(id);self->menu(O({{L"kind",S(L"color")},{L"id",N(double(id))}}),self->tiles.at(id).node,s?str(*s,L"name"):L"");
            }});
            d.hold.ProcessDownEvent(e.GetCurrentPoint(node));
        }
        drag=std::move(d);
    }
    void moved(PointerRoutedEventArgs const& e){
        if(!drag||e.Pointer().PointerId()!=drag->pointer)return;
        drag->point=e.GetCurrentPoint(root).Position();
        if(drag->hold&&!drag->started&&tiles.contains(drag->id))drag->hold.ProcessMoveEvents(e.GetIntermediatePoints(tiles.at(drag->id).node));
        if(!drag->started){
            auto dpi=GetDpiForWindow(GetForegroundWindow());double slop=std::max(2.,double(GetSystemMetricsForDpi(SM_CXDRAG,dpi))*96./std::max(96u,dpi));
            if(std::abs(drag->point.X-drag->origin.X)<=slop&&std::abs(drag->point.Y-drag->origin.Y)<=slop)return;
            start();
        }
        update();e.Handled(true);
    }
    void start(){
        auto& d=*drag;d.started=true;auto& source=tiles.at(d.id);if(openMenu){openMenu.Hide();openMenu=nullptr;}
        animate=winrt::Windows::UI::ViewManagement::UISettings().AnimationsEnabled();
        Border ghost;ghost.Width(source.node.ActualWidth());ghost.Height(source.node.ActualHeight());ghost.Padding({3,3,3,3});ghost.CornerRadius({6,6,6,6});
        ghost.Background(data->brush(L"panel"));if(selected==d.id){ghost.BorderBrush(accent(data));ghost.BorderThickness({2,2,2,2});}
        auto patch=paint();fillPatch(patch,array(source.swatch,L"rgba"));ghost.Child(patch);ghost.IsHitTestVisible(false);
        ThemeShadow shadow;ghost.Shadow(shadow);ghost.Translation({0,0,16});
        d.ghost=Primitives::Popup();d.ghost.XamlRoot(root.XamlRoot());d.ghost.Child(ghost);d.ghost.IsHitTestVisible(false);d.ghost.IsOpen(true);
        source.node.Opacity(0);AutomationProperties::SetItemStatus(root,L"Dragging");
        auto weak=weak_from_this();scrollCarry=0;
        scrollTimer=root.DispatcherQueue().CreateTimer();scrollTimer.Interval(std::chrono::milliseconds(16));
        scrollTimer.Tick([weak](auto&&,auto&&){if(auto self=weak.lock())self->autoscroll();});scrollTimer.Start();
    }
    std::optional<int> hit(winrt::Windows::Foundation::Point p){
        if(chooser.Visibility()==Visibility::Visible||expanded.Visibility()==Visibility::Visible)return std::nullopt;
        auto clip=scroll.TransformToVisual(root).TransformBounds({0,0,float(scroll.ActualWidth()),float(scroll.ActualHeight())});
        if(p.X<clip.X||p.X>=clip.X+clip.Width||p.Y<clip.Y||p.Y>=clip.Y+clip.Height)return std::nullopt;
        auto g=grid.TransformToVisual(root).TransformPoint({0,0});
        double x=p.X-g.X,y=p.Y-g.Y;if(x<0||y<0||x>=grid.ActualWidth())return std::nullopt;
        int index=int(y/Pitch)*columns+int(x*columns/(grid.ActualWidth()+Gap));
        return index<int(order.size())+1?std::optional<int>(index):std::nullopt;
    }
    void update(){
        auto& d=*drag;
        if(d.ghost){auto origin=root.TransformToVisual(nullptr).TransformPoint({0,0});d.ghost.HorizontalOffset(origin.X+d.point.X-d.grab.X);d.ghost.VerticalOffset(origin.Y+d.point.Y-d.grab.Y);}
        auto slot=hit(d.point);if(d.generation&&slot==d.slot)return;d.slot=slot;
        int original=int(std::find(order.begin(),order.end(),d.id)-order.begin());
        auto generation=++d.generation;auto weak=weak_from_this();
        QueryWorkspace(data->query,O({{L"type",S(L"palette_reorder_preview")},{L"palette",N(double(d.palette))},{L"id",N(double(d.id))},{L"slot",N(double(slot.value_or(original)))}}),
            [weak,generation,outside=!slot.has_value()](J reply){
                auto self=weak.lock();if(!self||!self->drag||self->drag->generation!=generation)return;
                auto preview=object(reply,L"result");if(!preview.Size())return;
                self->drag->action=outside?J{}:object(preview,L"action");self->shift(array(preview,L"order"));
            });
    }
    void shift(A const& target){
        double cell=cellWidth(scroll.ActualWidth());auto compositor=CompositionTarget::GetCompositorForCurrentThread();
        for(uint32_t i=0;i<target.Size();++i){
            auto id=uint64_t(target.GetNumberAt(i));if(!tiles.contains(id))continue;auto& t=tiles.at(id);
            float dx=float(std::round((int(i)%columns)*(cell+Gap))-t.x),dy=float((int(i)/columns)*Pitch-t.y);
            if(animate){
                auto move=compositor.CreateVector3KeyFrameAnimation();move.Target(L"Translation");move.InsertExpressionKeyFrame(0,L"this.StartingValue");
                move.InsertKeyFrame(1,{dx,dy,0},compositor.CreateCubicBezierEasingFunction({.33f,1.f},{.68f,1.f}));move.Duration(std::chrono::milliseconds(140));
                t.animation=move;t.node.StartAnimation(move);
            }else t.node.Translation({dx,dy,0});
        }
        AutomationProperties::SetItemStatus(grid,target.Stringify());
    }
    void autoscroll(){
        if(!drag||!drag->started)return;
        auto clip=scroll.TransformToVisual(root).TransformBounds({0,0,float(scroll.ActualWidth()),float(scroll.ActualHeight())});
        auto p=drag->point;double delta=0;
        if(p.X>=clip.X&&p.X<=clip.X+clip.Width){
            if(p.Y<clip.Y+20&&p.Y>=clip.Y-12)delta=-240*.016;else if(p.Y>clip.Y+clip.Height-20&&p.Y<=clip.Y+clip.Height+12)delta=240*.016;
        }
        scrollCarry+=delta;auto step=std::trunc(scrollCarry);if(!step)return;scrollCarry-=step;
        auto before=scroll.VerticalOffset();scroll.ChangeView(nullptr,before+step,nullptr,true);
        drag->generation=0;update();
    }
    void finish(bool cancel){
        if(!drag)return;auto d=std::move(*drag);drag.reset();
        if(scrollTimer){scrollTimer.Stop();scrollTimer=nullptr;}
        if(d.hold)d.hold.CompleteGesture();
        if(tiles.contains(d.id))tiles.at(d.id).node.ReleasePointerCaptures();
        if(d.started){
            if(d.ghost)d.ghost.IsOpen(false);
            for(auto& [id,t]:tiles){if(t.animation){t.node.StopAnimation(t.animation);t.animation=nullptr;}t.node.Translation({0,0,0});t.node.Opacity(1);}
            AutomationProperties::SetItemStatus(root,L"Ready");AutomationProperties::SetItemStatus(grid,L"");
        }
        if(!cancel&&d.started&&d.action.Size()){library(d.action);if(tiles.contains(d.id))tiles.at(d.id).node.Focus(FocusState::Programmatic);}
    }
    void released(PointerRoutedEventArgs const& e){
        if(!drag||e.Pointer().PointerId()!=drag->pointer)return;
        if(drag->hold)drag->hold.ProcessUpEvent(e.GetCurrentPoint(tiles.contains(drag->id)?UIElement(tiles.at(drag->id).node):UIElement(root)));
        drag->point=e.GetCurrentPoint(root).Position();bool started=drag->started;
        if(started){drag->generation=0;if(!hit(drag->point))drag->action=J{};}
        finish(false);if(started)e.Handled(true);
    }
    void sync(){
        std::optional<J> s=selected?swatch(*selected):std::nullopt;
        for(auto& [id,t]:tiles){bool on=selected==id;t.node.BorderBrush(on?accent(data):clear());t.node.BorderThickness(on?Thickness{2,2,2,2}:Thickness{0,0,0,0});
            AutomationProperties::SetItemStatus(t.node,on?L"Selected":L"");}
        auto label=s?str(*s,L"name"):str(view(),L"color_name");
        if(nameLabel.Text()!=label)nameLabel.Text(label);
        tooltip(name,label+L" · Click to rename");AutomationProperties::SetName(name,label);
    }
    void choices(){
        list.Children().Clear();auto weak=weak_from_this();
        for(auto value:array(view(),L"palettes")){
            auto palette=value.GetObject();auto id=uint64_t(num(palette,L"id"));auto title=str(palette,L"name");
            auto row=button(data,title,[weak,id]{if(auto self=weak.lock()){
                if(self->editing){self->commitName();if(self->editing)return;}
                self->library(O({{L"op",S(L"select_palette")},{L"id",N(double(id))}}),[weak](hstring error){if(auto self=weak.lock();self&&error.empty())self->browse(false);});
            }});
            row.HorizontalAlignment(HorizontalAlignment::Stretch);row.HorizontalContentAlignment(HorizontalAlignment::Stretch);row.Padding({4,4,4,4});
            row.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());
            Grid content;content.ColumnSpacing(6);ColumnDefinition text;text.Width({1,GridUnitType::Star});content.ColumnDefinitions().Append(text);
            ColumnDefinition strip;strip.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(strip);
            auto caption=label(data,title);caption.TextTrimming(TextTrimming::CharacterEllipsis);caption.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(caption);
            StackPanel cells;cells.Orientation(Orientation::Horizontal);cells.CornerRadius({3,3,3,3});cells.VerticalAlignment(VerticalAlignment::Center);
            for(auto rgba:array(palette,L"preview")){Border cell;cell.Width(12);cell.Height(18);cell.Background(SolidColorBrush(rgbaColor(rgba.GetArray())));cells.Children().Append(cell);}
            Grid::SetColumn(cells,1);content.Children().Append(cells);row.Content(content);
            bool active=flag(palette,L"active");row.Background(active?selectionBrush():clear());
            AutomationProperties::SetName(row,title);AutomationProperties::SetItemStatus(row,active?L"Selected":L"");AutomationProperties::SetAutomationId(row,L"palette-choice-"+to_hstring(id));
            tooltip(row,title);
            row.ContextRequested([weak,id,title](winrt::Windows::Foundation::IInspectable const& sender,ContextRequestedEventArgs const& e){e.Handled(true);
                if(auto self=weak.lock())self->menu(O({{L"kind",S(L"palette")},{L"id",N(double(id))}}),sender.as<FrameworkElement>(),title);});
            list.Children().Append(row);
        }
        filter();
    }
    SolidColorBrush selectionBrush()const{return data->brush(L"selection");}
    void render(){
        ensureChecker();
        auto v=view();auto palette=uint64_t(num(v,L"palette"));
        if(drag&&(palette!=drag->palette||!swatch(drag->id)))finish(true);
        std::wstring currentNext;{A marks;for(auto s:array(v,L"swatches"))marks.Append(B(flag(s.GetObject(),L"current")));currentNext=(str(v,L"color_detail")+marks.Stringify()).c_str();}
        auto nextChoices=array(v,L"palettes").Stringify();
        if(editing&&(hstring(currentNext)!=currentKey||nextChoices!=choicesKey)){editing=false;showLabel();report(L"");}
        currentKey=currentNext;
        bool keep=false;if(selected)if(auto s=swatch(*selected))keep=flag(*s,L"current");
        if(!keep){selected.reset();for(auto s:array(v,L"swatches"))if(flag(s.GetObject(),L"current")){selected=uint64_t(num(s.GetObject(),L"id"));break;}}
        if(selectNewest){selectNewest=false;auto all=array(v,L"swatches");if(all.Size())selected=uint64_t(num(all.GetObjectAt(all.Size()-1),L"id"));}
        std::set<uint64_t> live;order.clear();
        for(auto value:array(v,L"swatches")){
            auto s=value.GetObject();auto id=uint64_t(num(s,L"id"));live.insert(id);order.push_back(id);
            if(!tiles.contains(id)){tiles.emplace(id,makeTile(id));grid.Children().Append(tiles.at(id).node);}
            auto& t=tiles.at(id);
            if(t.swatch.Stringify()!=s.Stringify()){fillPatch(t.patch,array(s,L"rgba"));tooltip(t.node,str(s,L"detail"));AutomationProperties::SetName(t.node,str(s,L"detail"));}
            t.swatch=s;
        }
        for(auto it=tiles.begin();it!=tiles.end();){if(live.contains(it->first)){++it;continue;}uint32_t at;if(grid.Children().IndexOf(it->second.node,at))grid.Children().RemoveAt(at);it=tiles.erase(it);}
        add.IsEnabled(flag(v,L"can_name"));name.IsEnabled(flag(v,L"can_name"));
        auto nextHistory=array(v,L"history").Stringify();
        if(nextHistory!=historyKey){historyKey=nextHistory;historyTiles(history,historyCells,false);historyTiles(expanded,expandedCells,true);}
        if(nextChoices!=choicesKey){choicesKey=nextChoices;choices();}
        if(selectorLabel.Text()!=str(v,L"name"))selectorLabel.Text(str(v,L"name"));
        tooltip(selector,L"Choose a palette · "+str(v,L"name"));AutomationProperties::SetName(selector,L"Choose a palette");
        if(detail.Text()!=str(v,L"color_detail"))detail.Text(str(v,L"color_detail"));
        sync();arrange();
        auto status=files();auto generation=uint64_t(num(status,L"generation"));
        if(generation!=reportGeneration){reportGeneration=generation;auto error=str(status,L"error");auto notice=str(status,L"notice");
            if(!error.empty())report(error);else if(!notice.empty())report(notice,true);}
    }
    void refresh(){
        auto v=view();if(!v.Size())return;
        auto key=v.Stringify()+files().Stringify()+data->theme();
        if(key==viewKey)return;viewKey=key;render();
    }
    void keys(KeyRoutedEventArgs const& e){
        using K=winrt::Windows::System::VirtualKey;auto key=e.Key();
        if(key==K::Escape){
            if(drag){finish(true);}else if(editing)cancelName();else if(chooser.Visibility()==Visibility::Visible)browse(false);else if(expanded.Visibility()==Visibility::Visible)expand(false);else return;
            e.Handled(true);return;
        }
        bool control=(GetKeyState(VK_CONTROL)&0x8000)!=0;bool shift=(GetKeyState(VK_SHIFT)&0x8000)!=0;
        if(control&&(key==K::Z||key==K::Y)&&!FocusManager::GetFocusedElement(root.XamlRoot()).try_as<TextBox>()){
            bool redo=key==K::Y||shift;auto v=view();
            if(redo?flag(v,L"can_redo"):flag(v,L"can_undo"))library(O({{L"op",S(redo?L"redo_reorder":L"undo_reorder")},{L"palette",N(num(v,L"palette"))}}));
            e.Handled(true);return;
        }
        if(key==K::Space||key==K::Enter)e.Handled(true);
    }
    void init(){
        auto weak=weak_from_this();
        root.Padding({8,6,8,6});root.RowSpacing(6);root.MinWidth(0);root.Tag(O({{L"native_keys",B(true)}}));AutomationProperties::SetAutomationId(root,L"palettes-panel");AutomationProperties::SetName(root,L"Palettes");
        for(auto star:{true,false,false,false}){RowDefinition row;row.Height(star?GridLength{1,GridUnitType::Star}:GridLength{1,GridUnitType::Auto});root.RowDefinitions().Append(row);}
        auto divider=[&]{Border line;line.Height(1);line.Background(data->tint(L"text",38));return line;};
        normal.Spacing(6);history.Height(Cell);AutomationProperties::SetAutomationId(history,L"palette-history");AutomationProperties::SetName(history,L"Recent colors");
        tooltip(history,L"Recent colors — added only when used in artwork");
        normal.Children().Append(history);normal.Children().Append(divider());
        scroll.MinHeight(84);scroll.MaxHeight(172);scroll.HorizontalScrollMode(ScrollMode::Disabled);scroll.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);
        scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Hidden);scroll.Content(grid);AutomationProperties::SetAutomationId(grid,L"palette-swatches");AutomationProperties::SetName(grid,L"Saved colors");
        normal.Children().Append(scroll);body.Children().Append(normal);body.Children().Append(overlays);
        expanded.Visibility(Visibility::Collapsed);AutomationProperties::SetAutomationId(expanded,L"palette-history-expanded");overlays.Children().Append(expanded);
        chooser.RowSpacing(6);chooser.Visibility(Visibility::Collapsed);AutomationProperties::SetAutomationId(chooser,L"palette-chooser");
        for(auto star:{false,true}){RowDefinition row;row.Height(star?GridLength{1,GridUnitType::Star}:GridLength{1,GridUnitType::Auto});chooser.RowDefinitions().Append(row);}
        Grid searchRow;searchRow.ColumnSpacing(4);ColumnDefinition field;field.Width({1,GridUnitType::Star});searchRow.ColumnDefinitions().Append(field);
        ColumnDefinition square;square.Width({24,GridUnitType::Pixel});searchRow.ColumnDefinitions().Append(square);
        search.PlaceholderText(L"Find a palette");search.Height(24);search.MinHeight(24);search.Padding({6,2,6,2});search.Background(data->brush(L"input"));search.BorderThickness({0,0,0,0});
        search.FontSize(data->textSize());AutomationProperties::SetName(search,L"Find a palette");AutomationProperties::SetAutomationId(search,L"palette-search");
        search.TextChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->filter();});searchRow.Children().Append(search);
        more=button(data,L"New or import palette",[]{});more.Width(24);more.Height(24);more.Padding({0,0,0,0});more.Content(icon(L"plus",data->theme()));
        tooltip(more,L"New or import palette");AutomationProperties::SetAutomationId(more,L"palette-library-add");
        more.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->menu(O({{L"kind",S(L"library")}}),self->more,L"Palettes");});
        Grid::SetColumn(more,1);searchRow.Children().Append(more);chooser.Children().Append(searchRow);
        Grid resultsHost;list.Spacing(0);results.Content(list);results.HorizontalScrollMode(ScrollMode::Disabled);results.VerticalScrollBarVisibility(ScrollBarVisibility::Hidden);
        AutomationProperties::SetAutomationId(list,L"palette-list");AutomationProperties::SetName(list,L"Palettes");resultsHost.Children().Append(results);
        empty=label(data,L"No matching palettes");empty.Opacity(.55);empty.HorizontalAlignment(HorizontalAlignment::Center);empty.VerticalAlignment(VerticalAlignment::Center);empty.IsHitTestVisible(false);
        empty.Visibility(Visibility::Collapsed);resultsHost.Children().Append(empty);Grid::SetRow(resultsHost,1);chooser.Children().Append(resultsHost);
        overlays.Children().Append(chooser);root.Children().Append(body);
        auto footerLine=divider();Grid::SetRow(footerLine,1);root.Children().Append(footerLine);
        footer.ColumnSpacing(4);ColumnDefinition left;left.Width({1,GridUnitType::Auto});footer.ColumnDefinitions().Append(left);ColumnDefinition right;right.Width({1,GridUnitType::Star});footer.ColumnDefinitions().Append(right);
        selector=button(data,L"Choose a palette",[]{});selector.Height(24);selector.MinHeight(24);selector.Padding({4,0,4,0});selector.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());
        StackPanel selectorContent;selectorContent.Orientation(Orientation::Horizontal);selectorContent.Spacing(4);selectorLabel=label(data,L"");selectorLabel.TextTrimming(TextTrimming::CharacterEllipsis);
        selectorLabel.VerticalAlignment(VerticalAlignment::Center);selectorContent.Children().Append(selectorLabel);auto up=icon(L"chevron-down",data->theme(),12);up.RenderTransformOrigin({.5f,.5f});
        RotateTransform flip;flip.Angle(180);up.RenderTransform(flip);up.VerticalAlignment(VerticalAlignment::Center);selectorContent.Children().Append(up);selector.Content(selectorContent);
        AutomationProperties::SetAutomationId(selector,L"palette-selector");
        selector.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak](winrt::Windows::Foundation::IInspectable const&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock())self->openerTouch=e.Pointer().PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Mouse;})),true);
        selector.Click([weak](auto&&,auto&&){if(auto self=weak.lock()){bool touch=self->openerTouch;self->openerTouch=false;self->browse(self->chooser.Visibility()!=Visibility::Visible,!touch);}});
        footer.Children().Append(selector);
        info.HorizontalAlignment(HorizontalAlignment::Right);info.MinWidth(0);
        name=button(data,L"Color name",[weak]{if(auto self=weak.lock())self->editName();});name.Height(24);name.MinHeight(24);name.Padding({2,0,2,0});
        name.HorizontalAlignment(HorizontalAlignment::Right);name.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());nameLabel=label(data,L"");nameLabel.TextTrimming(TextTrimming::CharacterEllipsis);name.Content(nameLabel);
        AutomationProperties::SetAutomationId(name,L"palette-name");info.Children().Append(name);
        editor.MaxLength(64);editor.Height(24);editor.MinHeight(24);editor.Padding({6,2,6,2});editor.TextAlignment(TextAlignment::Right);editor.Background(data->brush(L"input"));editor.BorderThickness({0,0,0,0});
        editor.FontSize(data->textSize());editor.Visibility(Visibility::Collapsed);AutomationProperties::SetName(editor,L"Color name");AutomationProperties::SetAutomationId(editor,L"palette-name-editor");
        editor.KeyDown([weak](auto&&,KeyRoutedEventArgs const& e){if(e.Key()==winrt::Windows::System::VirtualKey::Enter)if(auto self=weak.lock()){e.Handled(true);self->commitName();if(!self->editing)self->name.Focus(FocusState::Programmatic);}});
        editor.LostFocus([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->editing&&self->editor.Visibility()==Visibility::Visible)self->commitName();});
        info.Children().Append(editor);
        detail=label(data,L"");detail.FontSize(data->textSize()*.9);detail.Opacity(.55);detail.HorizontalAlignment(HorizontalAlignment::Right);detail.Margin({0,0,2,0});
        tooltip(detail,L"sRGB hex preview; saved colors retain their original color space, alpha and HDR intensity");
        AutomationProperties::SetAutomationId(detail,L"palette-detail");info.Children().Append(detail);
        Grid::SetColumn(info,1);footer.Children().Append(info);Grid::SetRow(footer,2);root.Children().Append(footer);
        note.TextWrapping(TextWrapping::Wrap);note.FontSize(data->textSize());note.Visibility(Visibility::Collapsed);AutomationProperties::SetAutomationId(note,L"palette-message");
        AutomationProperties::SetLiveSetting(note,Microsoft::UI::Xaml::Automation::Peers::AutomationLiveSetting::Polite);Grid::SetRow(note,3);root.Children().Append(note);
        add=tileButton(L"Add current color to this palette");add.Background(data->brush(L"input"));add.Padding({0,0,0,0});add.Content(icon(L"plus",data->theme()));
        tooltip(add,L"Add current color to this palette");AutomationProperties::SetAutomationId(add,L"palette-add");
        add.Click([weak](auto&&,auto&&){if(auto self=weak.lock())self->addColor();});grid.Children().Append(add);
        scroll.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->arrange();});
        body.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock()){
            for(FrameworkElement cover:{FrameworkElement(self->chooser),FrameworkElement(self->expanded)}){cover.Width(self->body.ActualWidth());cover.Height(self->body.ActualHeight());}
            self->arrange();
        }});
        root.AddHandler(UIElement::PointerMovedEvent(),box_value(PointerEventHandler([weak](winrt::Windows::Foundation::IInspectable const&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->moved(e);})),true);
        root.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler([weak](winrt::Windows::Foundation::IInspectable const&,PointerRoutedEventArgs const& e){if(auto self=weak.lock())self->released(e);})),true);
        root.AddHandler(UIElement::PointerCanceledEvent(),box_value(PointerEventHandler([weak](winrt::Windows::Foundation::IInspectable const&,PointerRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&self->drag&&self->drag->pointer==e.Pointer().PointerId())self->finish(true);})),true);
        root.AddHandler(UIElement::PointerCaptureLostEvent(),box_value(PointerEventHandler([weak](winrt::Windows::Foundation::IInspectable const&,PointerRoutedEventArgs const& e){
            auto self=weak.lock();if(!self||!self->drag||self->drag->pointer!=e.Pointer().PointerId())return;
            if(e.GetCurrentPoint(self->root).IsInContact())self->finish(true);else self->released(e);})),true);
        root.AddHandler(UIElement::KeyDownEvent(),box_value(KeyEventHandler([weak](winrt::Windows::Foundation::IInspectable const&,KeyRoutedEventArgs const& e){if(auto self=weak.lock())self->keys(e);})),true);
        root.Unloaded([weak](auto&&,auto&&){if(auto self=weak.lock())self->finish(true);});
        refresh();
    }
    bool openerTouch=false;
};
}
FrameworkElement PalettesPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,std::function<double()>* contentHeight,std::function<J()>* scrollMetrics){
    auto view=std::make_shared<PalettesView>();view->data=data;view->init();
    bindings.emplace_back([view]{view->refresh();});
    if(contentHeight)*contentHeight=[view]{
        view->root.Measure({float(view->root.ActualWidth()>0?view->root.ActualWidth():280),std::numeric_limits<float>::infinity()});
        return double(view->root.DesiredSize().Height);
    };
    if(scrollMetrics)*scrollMetrics=[view]{
        view->root.Measure({float(view->root.ActualWidth()>0?view->root.ActualWidth():280),std::numeric_limits<float>::infinity()});
        return O({{L"fixed_height",N(std::max(0.,double(view->root.DesiredSize().Height)-double(view->scroll.DesiredSize().Height)))},{L"unit_height",N(Pitch)}});
    };
    return view->root;
}
