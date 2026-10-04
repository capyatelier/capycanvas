#include "pch.h"
#include "ShortcutPage.h"
#include "KeyNames.h"
#include "WorkspaceGeometry.h"
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>
#include <winrt/Microsoft.Windows.Storage.Pickers.h>
#include <filesystem>
#include <fstream>
#include <set>
#include <thread>

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Input;
using namespace CapyUi;
namespace Pickers=winrt::Microsoft::Windows::Storage::Pickers;

namespace {
constexpr double SheetWidth=460,PickerHeight=600,GroupWidth=576;
void send(std::shared_ptr<WorkspaceData> const& data,J const& action){data->dispatch(O({{L"type",S(L"preferences")},{L"action",action}}));}
J act(wchar_t const* type,std::initializer_list<std::pair<wchar_t const*,V>> fields={}){
    auto action=O(fields);action.Insert(L"type",S(type));return action;
}
TextBlock note(std::shared_ptr<WorkspaceData> const& data,hstring const& text){
    auto line=label(data,text);line.TextWrapping(TextWrapping::Wrap);line.Opacity(.55);line.FontSize(data->textSize()/1.2);line.LineHeight(15);line.LineStackingStrategy(LineStackingStrategy::MaxHeight);return line;
}
constexpr wchar_t ModifierCategory[]=L"Modifier keys";

}

struct ShortcutPage::Impl:std::enable_shared_from_this<Impl>{
    struct Group{StackPanel section,list,actions;Grid heading;TextBlock title{nullptr},summary{nullptr};};
    struct Row{Button button{nullptr};TextBlock title{nullptr},detail{nullptr},value{nullptr};};
    struct KeymapResult{hstring diagnostic,text;bool tooLarge=false;};
    struct Sheet{Grid frame;TextBlock title{nullptr};Button close{nullptr},start{nullptr};StackPanel body;hstring signature;};
    std::shared_ptr<WorkspaceData> data;
    Grid overlay;
    J model;
    StackPanel root,category,modifier,inputRoot{nullptr},pen{nullptr};
    ComboBox keymapChoice,contextChoice,showChoice;
    TextBlock keymapOutdated{nullptr};
    Button keymapMore{nullptr};
    hstring iconTheme;
    TextBox search,pickerSearch;
    Group categories,modifierMain,modifierCategory;
    StackPanel rootResults,categoryResults,emptyStatus;
    TextBlock emptyTitle{nullptr},emptyText{nullptr};
    std::map<std::wstring,Row> categoryRows,triggerRows;
    std::map<std::wstring,J> shortcutSpecs;
    std::map<std::wstring,Group> triggerGroups;
    Sheet editor,modifierSheet,picker,details,import;
    TextBlock pickerDescription{nullptr};
    StackPanel pickerList;
    std::set<uint32_t> handledRequests;
    hstring categorySignature,resultsSignature,modifierSignature,triggerSignature,modifierPane,penPane;
    std::wstring buildingScope;
    std::map<std::wstring,std::vector<std::function<void()>>> retained;
    void beginRefresh(std::wstring scope){buildingScope=std::move(scope);retained[buildingScope].clear();}
    void remember(std::function<void()> present){present();retained[buildingScope].push_back(std::move(present));}
    void refresh(std::wstring const& scope){for(auto const& present:retained[scope])present();}
    Image themed(wchar_t const* glyph){auto image=icon(glyph,data->theme());remember([this,image,glyph=std::wstring(glyph),theme=data->theme()]()mutable{if(theme!=data->theme()){theme=data->theme();image.Source(icon(hstring(glyph.c_str()),theme).Source());}});return image;}
    static void title(Group const& group,hstring const& text){group.title.Text(text);group.title.Visibility(text.empty()?Visibility::Collapsed:Visibility::Visible);group.heading.Visibility(text.empty()&&group.summary.Text().empty()?Visibility::Collapsed:Visibility::Visible);}

    hstring copy(wchar_t const* key)const{return data->caption(L"shortcuts",key);}
    LocalizedCopy localized(wchar_t const* key)const{return data->copyCaption(L"shortcuts",key);}
    void follow(LocalizedCopy const& text,std::function<void(hstring const&)> present){
        present(text);
        data->copyView([alive=weak_from_this(),present=std::move(present),resolve=text.current]{if(!alive.lock())return false;present(resolve());return true;});
    }
    static std::function<void(hstring const&)> naming(UIElement const& target){return [target](hstring const& name){AutomationProperties::SetName(target,name);tooltip(target,name);};}
    static std::function<void(hstring const&)> showing(TextBlock const& target){return [target](hstring const& text){target.Text(text);};}
    hstring categoryLabel(hstring const& id)const{
        for(auto value:array(page(),L"categories"))if(auto entry=value.GetObject();str(entry,L"id")==id)return str(entry,L"label");
        return id;
    }
    void init(){
        overlay.Visibility(Visibility::Collapsed);
        for(auto sheet:{&editor,&modifierSheet,&picker,&details,&import})build(*sheet);
        picker.start=flat(L"",[weak=weak_from_this()]{if(auto self=weak.lock()){auto spec=self->pickerModel();if(spec.Size())send(self->data,act(L"reset_trigger",{{L"trigger",S(str(spec,L"trigger"))}}));}});
        follow(data->copyCommon(L"reset"),[button=picker.start](hstring const& text){button.Content(box_value(text));AutomationProperties::SetName(button,text);});
        AutomationProperties::SetAutomationId(picker.start,L"action-picker-reset");picker.start.HorizontalAlignment(HorizontalAlignment::Left);
        picker.start.VerticalAlignment(VerticalAlignment::Top);picker.start.Margin({11,7,0,0});picker.frame.Children().Append(picker.start);
        pickerDescription=note(data,L"");AutomationProperties::SetAutomationId(pickerDescription,L"action-picker-description");
        follow(localized(L"search_actions"),[entry=pickerSearch](hstring const& text){entry.PlaceholderText(text);AutomationProperties::SetName(entry,text);});AutomationProperties::SetAutomationId(pickerSearch,L"action-picker-search");
        pickerSearch.TextChanged([weak=weak_from_this()](auto&& sender,auto&&){
            if(auto self=weak.lock();self&&!self->data->updating)send(self->data,act(L"search_action_picker",{{L"query",S(sender.template as<TextBox>().Text())}}));
        });
        pickerList.Spacing(18);
        picker.body.Children().Append(pickerDescription);picker.body.Children().Append(pickerSearch);picker.body.Children().Append(pickerList);
        AutomationProperties::SetAutomationId(editor.frame,L"shortcut-editor");AutomationProperties::SetAutomationId(modifierSheet.frame,L"modifier-key");
        AutomationProperties::SetAutomationId(picker.frame,L"action-picker");AutomationProperties::SetAutomationId(details.frame,L"keymap-details");
        AutomationProperties::SetAutomationId(import.frame,L"keymap-import");
        for(auto [sheet,id]:{std::pair{&editor,L"shortcut-editor-close"},std::pair{&modifierSheet,L"modifier-key-close"},std::pair{&picker,L"action-picker-close"},std::pair{&details,L"keymap-details-close"},std::pair{&import,L"keymap-import-close"}})
            AutomationProperties::SetAutomationId(sheet->close,id);
        editor.close.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())send(self->data,act(object(self->preferences(),L"capture").Size()?L"cancel_shortcut":L"close_shortcut_editor"));});
        modifierSheet.close.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())send(self->data,act(L"cancel_shortcut"));});
        picker.close.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())send(self->data,act(L"close_action_picker"));});
        details.close.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())send(self->data,act(L"keymap_details",{{L"open",B(false)}}));});
        import.close.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())send(self->data,act(L"cancel_keymap_import"));});
    }
    void build(Sheet& sheet){
        sheet.frame.Width(SheetWidth);sheet.frame.HorizontalAlignment(HorizontalAlignment::Center);sheet.frame.VerticalAlignment(VerticalAlignment::Center);
        sheet.frame.CornerRadius({16*CornerFit,16*CornerFit,16*CornerFit,16*CornerFit});sheet.frame.Visibility(Visibility::Collapsed);
        RowDefinition head;head.Height({46,GridUnitType::Pixel});RowDefinition rest;rest.Height({1,GridUnitType::Star});
        sheet.frame.RowDefinitions().Append(head);sheet.frame.RowDefinitions().Append(rest);
        sheet.title=label(data,L"",true);sheet.title.HorizontalAlignment(HorizontalAlignment::Center);sheet.title.VerticalAlignment(VerticalAlignment::Center);
        sheet.frame.Children().Append(sheet.title);
        sheet.close=button(data,L"",[]{});sheet.close.Width(24);sheet.close.Height(24);sheet.close.CornerRadius({12,12,12,12});
        sheet.close.HorizontalAlignment(HorizontalAlignment::Right);sheet.close.VerticalAlignment(VerticalAlignment::Top);sheet.close.Margin({0,11,11,0});
        follow(data->copyCommon(L"close"),naming(sheet.close));sheet.frame.Children().Append(sheet.close);
        ScrollViewer scroll;scroll.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);Grid::SetRow(scroll,1);
        sheet.body.Spacing(18);sheet.body.Padding({12,0,12,24});scroll.Content(sheet.body);sheet.frame.Children().Append(scroll);
        overlay.Children().Append(sheet.frame);
    }
    Button flat(hstring const& text,std::function<void()> action){
        auto result=button(data,text,std::move(action));result.Padding({10,5,10,5});result.MinHeight(34);result.Background(data->brush(L"button"));
        return result;
    }
    Button glyphButton(wchar_t const* glyph,wchar_t const* key,J action,hstring const& id){
        auto name=copy(key);
        auto result=button(data,L"",[data=data,action]{send(data,action);});result.Width(34);result.Height(34);result.Content(themed(glyph));
        AutomationProperties::SetName(result,name);AutomationProperties::SetAutomationId(result,id);tooltip(result,name);
        remember([this,result,key=std::wstring(key)]{auto name=copy(key.c_str());AutomationProperties::SetName(result,name);tooltip(result,name);});
        return result;
    }
    Group group(hstring const& title,hstring const& summary=L""){
        Group g;g.section.Spacing(6);g.section.MaxWidth(GroupWidth);g.section.HorizontalAlignment(HorizontalAlignment::Stretch);
        auto heading=g.heading;heading.ColumnSpacing(12);
        ColumnDefinition text;text.Width({1,GridUnitType::Star});ColumnDefinition tail;tail.Width({1,GridUnitType::Auto});
        heading.ColumnDefinitions().Append(text);heading.ColumnDefinitions().Append(tail);
        StackPanel words;words.Spacing(6);
        g.title=label(data,title,true);g.title.Visibility(title.empty()?Visibility::Collapsed:Visibility::Visible);words.Children().Append(g.title);
        g.summary=label(data,summary);g.summary.TextWrapping(TextWrapping::Wrap);g.summary.Opacity(.55);
        g.summary.Visibility(summary.empty()?Visibility::Collapsed:Visibility::Visible);words.Children().Append(g.summary);
        heading.Children().Append(words);Grid::SetColumn(g.actions,1);g.actions.VerticalAlignment(VerticalAlignment::Center);heading.Children().Append(g.actions);
        heading.Visibility(title.empty()&&summary.empty()?Visibility::Collapsed:Visibility::Visible);heading.Margin({0,8,0,6});
        g.section.Children().Append(heading);
        Border card;card.Background(data->brush(L"card"));card.CornerRadius({12,12,12,12});card.Child(g.list);
        g.section.Children().Append(card);
        return g;
    }
    void add(StackPanel const& list,UIElement const& row){
        if(list.Children().Size()){Border line;line.Height(1);line.Background(data->tint(L"text",20));list.Children().Append(line);}
        list.Children().Append(row);
    }
    Grid line(){
        Grid result;result.MinHeight(54);result.Padding({14,8,14,8});result.ColumnSpacing(6);
        ColumnDefinition text;text.Width({1,GridUnitType::Star});result.ColumnDefinitions().Append(text);
        for(int i=0;i<3;++i){ColumnDefinition tail;tail.Width({1,GridUnitType::Auto});result.ColumnDefinitions().Append(tail);}
        return result;
    }
    StackPanel words(hstring const& title,hstring const& detail,TextBlock* titleOut=nullptr,TextBlock* detailOut=nullptr){
        StackPanel text;text.VerticalAlignment(VerticalAlignment::Center);text.Spacing(3);
        auto heading=label(data,title);heading.TextWrapping(TextWrapping::Wrap);text.Children().Append(heading);
        auto sub=note(data,detail);sub.Visibility(detail.empty()?Visibility::Collapsed:Visibility::Visible);text.Children().Append(sub);
        if(titleOut)*titleOut=heading;
        if(detailOut)*detailOut=sub;
        return text;
    }
    template<typename T> void cell(Grid const& grid,T const& element,int column){
        element.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(element,column);grid.Children().Append(element);
    }
    Row navRow(hstring const& title,hstring const& detail,hstring const& value,J action,hstring const& id){
        Row row;row.button=button(data,L"",[data=data,action]{send(data,action);});
        row.button.HorizontalAlignment(HorizontalAlignment::Stretch);row.button.HorizontalContentAlignment(HorizontalAlignment::Stretch);
        row.button.CornerRadius({0,0,0,0});row.button.MinHeight(54);row.button.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());row.button.Padding({14,8,14,8});
        Grid content;content.ColumnSpacing(6);
        ColumnDefinition text;text.Width({1,GridUnitType::Star});content.ColumnDefinitions().Append(text);
        for(int i=0;i<2;++i){ColumnDefinition tail;tail.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(tail);}
        cell(content,words(title,detail,&row.title,&row.detail),0);
        row.value=label(data,value);row.value.Opacity(.55);cell(content,row.value,1);
        cell(content,themed(L"go-next"),2);
        row.button.Content(content);AutomationProperties::SetName(row.button,title);AutomationProperties::SetAutomationId(row.button,id);
        return row;
    }
    static void update(Row const& row,hstring const& title,hstring const& detail,hstring const& value){
        if(row.title.Text()!=title)row.title.Text(title);
        if(row.detail.Text()!=detail){row.detail.Text(detail);row.detail.Visibility(detail.empty()?Visibility::Collapsed:Visibility::Visible);}
        if(row.value.Text()!=value)row.value.Text(value);
        AutomationProperties::SetName(row.button,title);AutomationProperties::SetItemStatus(row.button,value);
    }
    Button buttonRow(wchar_t const* glyph,wchar_t const* key,J action,hstring const& id,bool destructive=false){
        auto text=copy(key);
        auto result=button(data,L"",[data=data,action]{send(data,action);});
        result.HorizontalAlignment(HorizontalAlignment::Stretch);result.HorizontalContentAlignment(HorizontalAlignment::Left);
        result.CornerRadius({0,0,0,0});result.MinHeight(54);result.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());result.Padding({14,8,14,8});
        StackPanel content;content.Orientation(Orientation::Horizontal);content.Spacing(12);content.Children().Append(themed(glyph));
        auto words=label(data,text);words.VerticalAlignment(VerticalAlignment::Center);if(destructive)words.Foreground(fill(color(data->theme()==L"dark"?L"#ff7b63":L"#c01c28")));
        content.Children().Append(words);result.Content(content);AutomationProperties::SetName(result,text);AutomationProperties::SetAutomationId(result,id);
        remember([this,result,words,destructive,key=std::wstring(key)]{auto text=copy(key.c_str());words.Text(text);AutomationProperties::SetName(result,text);if(destructive)words.Foreground(fill(color(data->theme()==L"dark"?L"#ff7b63":L"#c01c28")));});
        return result;
    }
    ComboBox dropdown(hstring const& name,hstring const& id,std::function<void(int32_t)> choose){
        ComboBox result;result.MinWidth(120);result.MaxWidth(220);AutomationProperties::SetName(result,name);AutomationProperties::SetAutomationId(result,id);tooltip(result,name);
        result.SelectionChanged([data=data,choose=std::move(choose)](auto&& sender,auto&&){
            auto index=sender.template as<ComboBox>().SelectedIndex();if(!data->updating&&index>=0)choose(index);
        });
        return result;
    }
    static void choices(ComboBox const& box,std::vector<hstring> const& labels,int32_t selected){
        if(box.Items().Size()!=labels.size()){
            box.Items().Clear();
            for(auto const& value:labels){ComboBoxItem item;item.Content(box_value(value));box.Items().Append(item);}
        }
        for(uint32_t i=0;i<labels.size();++i)comboOptionText(box,i,labels[i]);
        if(box.SelectedIndex()!=selected)box.SelectedIndex(selected);
    }
    StackPanel Container(hstring const& page,StackPanel const& node){
        if(page==L"shortcuts"){buildShortcuts(node);return root;}
        if(page==L"input"){
            inputRoot=StackPanel();inputRoot.Spacing(24);pen=StackPanel();pen.Spacing(24);pen.Visibility(Visibility::Collapsed);
            AutomationProperties::SetAutomationId(pen,L"pen-button-page");
            node.Children().Append(inputRoot);node.Children().Append(pen);return inputRoot;
        }
        return node;
    }
    void buildShortcuts(StackPanel const& node){
        for(auto pane:{root,category,modifier}){pane.Spacing(24);node.Children().Append(pane);}
        category.Visibility(Visibility::Collapsed);modifier.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(category,L"shortcut-category-page");AutomationProperties::SetAutomationId(modifier,L"modifier-page");
        auto keymap=group(copy(L"keymap"));follow(localized(L"keymap"),showing(keymap.title));AutomationProperties::SetAutomationId(keymap.section,L"keymap");
        TextBlock presetTitle{nullptr};auto preset=line();cell(preset,words(copy(L"preset"),copy(L"updated"),&presetTitle,&keymapOutdated),0);
        follow(localized(L"preset"),showing(presetTitle));follow(localized(L"updated"),showing(keymapOutdated));keymapOutdated.Visibility(Visibility::Collapsed);
        keymapChoice=dropdown(copy(L"keymap_preset"),L"keymap-preset",[weak=weak_from_this()](int32_t index){
            if(auto self=weak.lock()){auto presets=array(object(self->preferences(),L"keymap"),L"presets");
                if(uint32_t(index)<presets.Size())send(self->data,act(L"select_keymap",{{L"id",S(str(presets.GetObjectAt(index),L"id"))}}));}
        });cell(preset,keymapChoice,1);follow(localized(L"keymap_preset"),naming(keymapChoice));
        auto more=keymapMore=button(data,L"",[]{});more.Width(34);more.Height(34);more.CornerRadius({17,17,17,17});
        follow(localized(L"keymap_options"),naming(more));AutomationProperties::SetAutomationId(more,L"keymap-menu");
        MenuFlyout options;
        for(auto [text,action,id]:{std::tuple{L"import_menu",act(L"choose_keymap_file"),L"keymap-import-button"},std::tuple{L"export_menu",act(L"export_keymap"),L"keymap-export-button"},
            std::tuple{L"differences",act(L"keymap_details",{{L"open",B(true)}}),L"keymap-details-button"},std::tuple{L"reset_all",act(L"reset_all_shortcuts"),L"reset-all-shortcuts"}}){
            if(std::wstring_view(id)==L"reset-all-shortcuts")options.Items().Append(MenuFlyoutSeparator());
            MenuFlyoutItem item;follow(localized(text),[item](hstring const& label){item.Text(label);});AutomationProperties::SetAutomationId(item,id);
            item.Click([data=data,action](auto&&,auto&&){send(data,action);});options.Items().Append(item);
        }
        more.Flyout(options);cell(preset,more,2);
        keymap.list.Children().Append(preset);root.Children().Append(keymap.section);

        StackPanel shortcuts;shortcuts.Spacing(12);shortcuts.MaxWidth(GroupWidth);AutomationProperties::SetAutomationId(shortcuts,L"shortcuts");
        auto heading=label(data,localized(L"title"),true);heading.Margin({0,8,0,0});shortcuts.Children().Append(heading);
        Grid filters;filters.ColumnSpacing(6);AutomationProperties::SetAutomationId(filters,L"shortcut-filters");
        ColumnDefinition field;field.Width({1,GridUnitType::Star});filters.ColumnDefinitions().Append(field);
        for(int i=0;i<2;++i){ColumnDefinition tail;tail.Width({1,GridUnitType::Auto});filters.ColumnDefinitions().Append(tail);}
        follow(localized(L"search_or_press"),[entry=search](hstring const& text){entry.PlaceholderText(text);});
        follow(localized(L"search_shortcuts"),[entry=search](hstring const& text){AutomationProperties::SetName(entry,text);});AutomationProperties::SetAutomationId(search,L"shortcuts-search");
        search.TextChanged([weak=weak_from_this()](auto&& sender,auto&&){
            if(auto self=weak.lock();self&&!self->data->updating)send(self->data,act(L"search_shortcuts",{{L"query",S(sender.template as<TextBox>().Text())}}));
        });
        search.PreviewKeyDown([weak=weak_from_this()](auto&&,KeyRoutedEventArgs const& e){if(auto self=weak.lock())self->chord(e);});
        cell(filters,search,0);
        contextChoice=dropdown(copy(L"tool_shortcuts"),L"shortcut-context",[weak=weak_from_this()](int32_t index){
            if(auto self=weak.lock()){auto contexts=array(object(object(self->model,L"preferences"),L"shortcut_page"),L"contexts");
                if(uint32_t(index)<contexts.Size())send(self->data,act(L"shortcut_context",{{L"category",contexts.GetObjectAt(index).GetNamedValue(L"category")}}));}
        });cell(filters,contextChoice,1);follow(localized(L"tool_shortcuts"),naming(contextChoice));
        showChoice=dropdown(copy(L"choose_actions"),L"shortcut-show",[weak=weak_from_this()](int32_t index){
            if(auto self=weak.lock()){auto shows=array(object(object(self->model,L"preferences"),L"shortcut_page"),L"shows");
                if(uint32_t(index)<shows.Size())send(self->data,act(L"shortcut_show",{{L"show",S(str(shows.GetObjectAt(index),L"show"))}}));}
        });cell(filters,showChoice,2);follow(localized(L"choose_actions"),naming(showChoice));
        shortcuts.Children().Append(filters);
        categories=group(L"");AutomationProperties::SetAutomationId(categories.section,L"shortcut-categories");shortcuts.Children().Append(categories.section);
        emptyStatus.Spacing(6);emptyStatus.HorizontalAlignment(HorizontalAlignment::Center);emptyStatus.Margin({0,24,0,24});emptyStatus.Visibility(Visibility::Collapsed);
        AutomationProperties::SetAutomationId(emptyStatus,L"shortcut-empty");
        emptyTitle=label(data,L"",true);emptyTitle.HorizontalAlignment(HorizontalAlignment::Center);emptyText=note(data,L"");emptyText.TextAlignment(TextAlignment::Center);
        emptyStatus.Children().Append(emptyTitle);emptyStatus.Children().Append(emptyText);shortcuts.Children().Append(emptyStatus);
        root.Children().Append(shortcuts);
        modifierMain=group(L"");AutomationProperties::SetAutomationId(modifierMain.section,L"modifier-results");modifierMain.section.Visibility(Visibility::Collapsed);
        root.Children().Append(modifierMain.section);rootResults.Spacing(24);root.Children().Append(rootResults);
        modifierCategory=group(L"",copy(L"hold_key_help"));follow(localized(L"hold_key_help"),showing(modifierCategory.summary));AutomationProperties::SetAutomationId(modifierCategory.section,L"modifier-keys");
        category.Children().Append(modifierCategory.section);categoryResults.Spacing(24);category.Children().Append(categoryResults);
    }
    void chord(KeyRoutedEventArgs const& e){
        using winrt::Windows::System::VirtualKey;
        auto key=e.Key();
        bool control=(GetKeyState(VK_CONTROL)&0x8000)!=0,alt=(GetKeyState(VK_MENU)&0x8000)!=0,shift=(GetKeyState(VK_SHIFT)&0x8000)!=0;
        if(key==VirtualKey::Control||key==VirtualKey::Shift||key==VirtualKey::Menu||key==VirtualKey::LeftWindows||key==VirtualKey::RightWindows)return;
        bool editing=control&&!alt&&(key==VirtualKey::A||key==VirtualKey::C||key==VirtualKey::V||key==VirtualKey::X||key==VirtualKey::Back||
            key==VirtualKey::Delete||key==VirtualKey::Left||key==VirtualKey::Right||key==VirtualKey::Home||key==VirtualKey::End);
        bool named=(key>=VirtualKey::F1&&key<=VirtualKey::F24)||DeviceKey(key);
        if(editing||!(control||alt||named))return;
        auto name=KeyName(key,e.KeyStatus().ScanCode);if(name.empty())return;
        e.Handled(true);
        send(data,act(L"search_shortcut_key",{{L"chord",O({{L"key",S(hstring(name))},{L"command",B(control)},{L"shift",B(shift)},{L"alt",B(alt)}})}}));
        search.SelectAll();
    }
    J preferences()const{return object(model,L"preferences");}
    J page()const{return object(preferences(),L"shortcut_page");}
    J pickerModel()const{return object(page(),L"picker");}

    Grid recordingRow(J const& capture){
        auto row=line();AutomationProperties::SetAutomationId(row,L"shortcut-recording");bool existing=flag(capture,L"existing");auto notice=str(capture,L"notice");
        StackPanel lead;lead.Orientation(Orientation::Horizontal);lead.Spacing(12);
        auto glyph=icon(existing||!notice.empty()?L"info":L"keyboard",data->theme());
        remember([this,glyph,shown=(existing||!notice.empty()?hstring(L"info"):hstring(L"keyboard"))+data->theme()]()mutable{auto current=object(preferences(),L"capture");auto name=flag(current,L"existing")||!str(current,L"notice").empty()?hstring(L"info"):hstring(L"keyboard");auto wanted=name+data->theme();if(shown!=wanted){shown=wanted;glyph.Source(icon(name,data->theme()).Source());}});glyph.VerticalAlignment(VerticalAlignment::Center);lead.Children().Append(glyph);
        TextBlock heading{nullptr},detail{nullptr};lead.Children().Append(words(str(capture,L"shortcut"),notice,&heading,&detail));cell(row,lead,0);
        auto cancel=flat(L"",[data=data]{send(data,act(L"cancel_shortcut"));});AutomationProperties::SetAutomationId(cancel,L"cancel-shortcut");cell(row,cancel,1);
        auto confirm=flat(L"",[weak=weak_from_this()]{if(auto self=weak.lock())send(self->data,act(L"confirm_shortcut",{{L"replace",B(object(self->preferences(),L"capture").GetNamedValue(L"conflict",JsonValue::CreateNullValue()).ValueType()!=JsonValueType::Null)}}));});
        confirm.Background(accent(data));confirm.Foreground(data->brush(L"accent_foreground"));
        confirm.Resources().Insert(box_value(L"ButtonBackgroundPointerOver"),accent(data));confirm.Resources().Insert(box_value(L"ButtonBackgroundPressed"),accent(data));
        AutomationProperties::SetAutomationId(confirm,L"confirm-shortcut");cell(row,confirm,2);
        remember([this,row,heading,detail,cancel,confirm]{auto current=object(preferences(),L"capture");auto notice=str(current,L"notice");heading.Text(str(current,L"shortcut"));detail.Text(notice);detail.Visibility(notice.empty()?Visibility::Collapsed:Visibility::Visible);AutomationProperties::SetName(row,str(current,L"shortcut"));auto cancelText=data->common(L"cancel");cancel.Content(box_value(cancelText));AutomationProperties::SetName(cancel,cancelText);auto confirmText=copy(flag(current,L"existing")?L"open":current.GetNamedValue(L"conflict",JsonValue::CreateNullValue()).ValueType()!=JsonValueType::Null?L"reassign":L"add");confirm.Content(box_value(confirmText));AutomationProperties::SetName(confirm,confirmText);confirm.IsEnabled(object(current,L"chord").Size()!=0&&str(current,L"error").empty());});
        return row;
    }
    void refreshKeymap(){
        auto keymap=object(preferences(),L"keymap");auto presets=array(keymap,L"presets");std::vector<hstring> titles;int32_t selected=-1;
        for(uint32_t i=0;i<presets.Size();++i){auto preset=presets.GetObjectAt(i);titles.push_back(str(preset,L"title"));if(str(preset,L"id")==str(keymap,L"selected"))selected=int32_t(i);}
        choices(keymapChoice,titles,selected);
        keymapOutdated.Visibility(flag(keymap,L"outdated")?Visibility::Visible:Visibility::Collapsed);
        auto signature=str(keymap,L"selected")+array(keymap,L"links").Stringify()+L"|"+to_hstring(array(keymap,L"differences").Size());
        if(details.signature!=signature){
            details.signature=signature;beginRefresh(L"details");details.body.Children().Clear();
            auto source=label(data,L"");source.TextWrapping(TextWrapping::Wrap);source.Opacity(.55);details.body.Children().Append(source);
            remember([this,source]{auto current=object(preferences(),L"keymap");details.title.Text(str(current,L"title"));source.Text(str(current,L"source"));});
            VariableSizedWrapGrid links;links.Orientation(Orientation::Horizontal);
            for(auto link:array(keymap,L"links")){
                std::wstring url(link.GetString().c_str());auto trimmed=url;while(!trimmed.empty()&&trimmed.back()==L'/')trimmed.pop_back();
                auto name=trimmed.substr(trimmed.find_last_of(L'/')+1);name=name.substr(0,name.find(L'.'));for(auto& c:name)if(c==L'_'||c==L'-')c=L' ';
                HyperlinkButton anchor;anchor.Content(box_value(hstring(name.empty()?url:name)));anchor.NavigateUri(winrt::Windows::Foundation::Uri(link.GetString()));links.Children().Append(anchor);
            }
            details.body.Children().Append(links);auto list=group(L"");auto differences=array(keymap,L"differences");
            if(!differences.Size()){
                auto row=line();TextBlock heading{nullptr},help{nullptr};cell(row,words(L"",L"",&heading,&help),0);add(list.list,row);
                remember([this,heading,help]{heading.Text(copy(L"no_differences"));help.Text(copy(L"defaults_help"));help.Visibility(Visibility::Visible);});
            }
            for(uint32_t i=0;i<differences.Size();++i){
                auto row=line();TextBlock heading{nullptr},help{nullptr};cell(row,words(L"",L"",&heading,&help),0);add(list.list,row);
                remember([this,i,heading,help]{auto current=array(object(preferences(),L"keymap"),L"differences").GetObjectAt(i);heading.Text(str(current,L"trigger"));auto detail=str(current,L"note");help.Text(detail);help.Visibility(detail.empty()?Visibility::Collapsed:Visibility::Visible);});
            }
            details.body.Children().Append(list.section);
        }
        refresh(L"details");show(details,flag(keymap,L"details"));
        auto preview=object(keymap,L"import");
        if(preview.Size()&&import.signature.empty()){
            import.signature=L"open";beginRefresh(L"import");import.body.Children().Clear();
            auto body=label(data,L"");body.TextWrapping(TextWrapping::Wrap);import.body.Children().Append(body);
            StackPanel footer;footer.Orientation(Orientation::Horizontal);footer.Spacing(6);footer.HorizontalAlignment(HorizontalAlignment::Right);
            auto cancel=flat(L"",[data=data]{send(data,act(L"cancel_keymap_import"));});footer.Children().Append(cancel);
            auto confirm=flat(L"",[data=data]{send(data,act(L"confirm_keymap_import"));});confirm.Background(accent(data));confirm.Foreground(data->brush(L"accent_foreground"));
            AutomationProperties::SetAutomationId(confirm,L"confirm-keymap-import");footer.Children().Append(confirm);import.body.Children().Append(footer);
            remember([this,body,cancel,confirm]{auto current=object(object(preferences(),L"keymap"),L"import");import.title.Text(str(current,L"heading"));body.Text(str(current,L"body"));auto cancelText=str(current,L"cancel_label"),importText=str(current,L"import_label");cancel.Content(box_value(cancelText));AutomationProperties::SetName(cancel,cancelText);confirm.Content(box_value(importText));AutomationProperties::SetName(confirm,importText);});
        }
        if(preview.Size())refresh(L"import");else {import.signature=L"";retained[L"import"].clear();}
        show(import,preview.Size()!=0);
    }
    void refreshFilters(){
        auto spec=page();auto contexts=array(spec,L"contexts");std::vector<hstring> labels;int32_t selected=-1;
        auto context=spec.GetNamedValue(L"context",JsonValue::CreateNullValue()).Stringify();
        for(uint32_t i=0;i<contexts.Size();++i){auto choice=contexts.GetObjectAt(i);labels.push_back(str(choice,L"label"));if(choice.GetNamedValue(L"category",JsonValue::CreateNullValue()).Stringify()==context)selected=int32_t(i);}
        choices(contextChoice,labels,selected);
        auto shows=array(spec,L"shows");labels.clear();selected=-1;
        for(uint32_t i=0;i<shows.Size();++i){auto choice=shows.GetObjectAt(i);labels.push_back(str(choice,L"label"));if(str(choice,L"show")==str(spec,L"show"))selected=int32_t(i);}
        choices(showChoice,labels,selected);
        if(search.Text()!=str(preferences(),L"shortcut_query"))search.Text(str(preferences(),L"shortcut_query"));
    }
    void refreshCategories(){
        auto spec=page();auto list=array(spec,L"categories");std::wstring ids;
        for(auto value:list)ids+=L"\n"+std::wstring(str(value.GetObject(),L"id"));
        if(categorySignature!=hstring(ids)){
            categorySignature=hstring(ids);beginRefresh(L"categories");categories.list.Children().Clear();categoryRows.clear();
            for(auto value:list){
                auto entry=value.GetObject();auto id=str(entry,L"id");
                auto row=navRow(str(entry,L"label"),L"",to_hstring(uint32_t(num(entry,L"count"))),act(L"shortcut_category",{{L"id",S(id)}}),L"shortcut-category-"+id);
                add(categories.list,row.button);categoryRows.emplace(id.c_str(),row);
            }
        }
        for(auto value:list){auto entry=value.GetObject();if(auto it=categoryRows.find(str(entry,L"id").c_str());it!=categoryRows.end())update(it->second,str(entry,L"label"),L"",to_hstring(uint32_t(num(entry,L"count"))));}
        refresh(L"categories");bool filtering=flag(spec,L"filtering");categories.section.Visibility(filtering?Visibility::Collapsed:Visibility::Visible);
        auto empty=object(spec,L"empty");emptyStatus.Visibility(empty.Size()?Visibility::Visible:Visibility::Collapsed);
        if(empty.Size()){emptyTitle.Text(str(empty,L"title"));emptyText.Text(str(empty,L"description"));}
    }
    UIElement shortcutRow(J const& spec){
        auto id=str(spec,L"id");auto row=line();AutomationProperties::SetAutomationId(row,L"shortcut-row-"+id);
        auto choose=button(data,L"",[data=data,id]{send(data,act(L"edit_shortcut",{{L"id",S(id)}}));});
        choose.HorizontalAlignment(HorizontalAlignment::Stretch);choose.HorizontalContentAlignment(HorizontalAlignment::Stretch);choose.Padding({0,0,0,0});choose.Background(clear());choose.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());
        Grid content;content.ColumnSpacing(12);
        ColumnDefinition text;text.Width({1,GridUnitType::Star});ColumnDefinition binding;binding.Width({1,GridUnitType::Auto});
        content.ColumnDefinitions().Append(text);content.ColumnDefinitions().Append(binding);
        auto scope=str(spec,L"scope_caption");auto detail=str(spec,L"detail");
        auto subtitle=detail.empty()?scope:scope.empty()?detail:detail+L" · "+scope;
        TextBlock title{nullptr},detailText{nullptr};cell(content,words(str(spec,L"label"),subtitle,&title,&detailText),0);
        auto shortcut=label(data,str(spec,L"shortcut"));shortcut.Opacity(.55);if(flag(spec,L"modified"))shortcut.FontWeight(winrt::Windows::UI::Text::FontWeights::Bold());
        cell(content,shortcut,1);choose.Content(content);
        AutomationProperties::SetName(choose,str(spec,L"label"));AutomationProperties::SetAutomationId(choose,L"shortcut-"+id);
        AutomationProperties::SetItemStatus(choose,str(spec,L"shortcut"));AutomationProperties::SetHelpText(choose,flag(spec,L"modified")?copy(L"modified"):hstring());
        Grid::SetColumnSpan(choose,3);cell(row,choose,0);
        if(flag(spec,L"modified"))cell(row,glyphButton(L"reset",L"reset_default",act(L"reset_shortcut",{{L"id",S(id)}}),L"shortcut-reset-"+id),3);
        remember([this,id,choose,title,detailText,shortcut]{
            auto current=shortcutSpecs.at(id.c_str());auto scope=str(current,L"scope_caption"),detail=str(current,L"detail");
            auto subtitle=detail.empty()?scope:scope.empty()?detail:detail+L" · "+scope;
            title.Text(str(current,L"label"));detailText.Text(subtitle);detailText.Visibility(subtitle.empty()?Visibility::Collapsed:Visibility::Visible);shortcut.Text(str(current,L"shortcut"));
            AutomationProperties::SetName(choose,str(current,L"label"));AutomationProperties::SetItemStatus(choose,str(current,L"shortcut"));AutomationProperties::SetHelpText(choose,flag(current,L"modified")?copy(L"modified"):hstring());
        });
        return row;
    }
    void refreshResults(){
        auto spec=page();bool filtering=flag(spec,L"filtering");auto categoryId=str(spec,L"category");bool inCategory=!categoryId.empty();
        struct Layout{hstring title;std::vector<J> specs;};std::vector<Layout> layout;shortcutSpecs.clear();
        for(auto value:array(preferences(),L"shortcuts")){
            auto row=value.GetObject();shortcutSpecs.emplace(str(row,L"id").c_str(),row);if(!flag(row,L"visible",true))continue;
            auto title=inCategory&&!filtering?str(row,L"subgroup"):str(row,L"group");
            if(layout.empty()||layout.back().title!=title)layout.push_back({title,{}});
            layout.back().specs.push_back(row);
        }
        std::wstring key=std::wstring(categoryId)+(filtering?L"|filtered":L"|all");
        for(auto const& section:layout){key+=L"#";for(auto const& row:section.specs)key+=L","+std::wstring(str(row,L"id"))+(flag(row,L"modified")?L"*":L"");}
        if(resultsSignature!=hstring(key)){
            resultsSignature=hstring(key);beginRefresh(L"results");
            auto target=inCategory?categoryResults:rootResults,other=inCategory?rootResults:categoryResults;target.Children().Clear();other.Children().Clear();
            for(auto const& section:layout){
                auto g=group(inCategory&&!filtering?section.title:categoryLabel(section.title));auto id=str(section.specs.front(),L"id");
                remember([this,g,id,inCategory,filtering]{auto current=shortcutSpecs.at(id.c_str());title(g,inCategory&&!filtering?str(current,L"subgroup"):categoryLabel(str(current,L"group")));});
                for(auto const& row:section.specs)add(g.list,shortcutRow(row));target.Children().Append(g.section);
            }
        }
        refresh(L"results");
    }
    void refreshModifiers(){
        auto spec=page();std::vector<J> visible;
        for(auto value:array(spec,L"modifiers"))if(flag(value.GetObject(),L"visible"))visible.push_back(value.GetObject());
        bool onCategory=str(spec,L"category")==ModifierCategory;
        title(modifierMain,categoryLabel(ModifierCategory));
        modifierMain.section.Visibility(flag(spec,L"filtering")&&!visible.empty()?Visibility::Visible:Visibility::Collapsed);
        modifierCategory.section.Visibility(onCategory?Visibility::Visible:Visibility::Collapsed);
        std::wstring key=onCategory?L"category":L"main";for(auto const& row:visible)key+=L"\n"+std::wstring(row.GetNamedValue(L"key").Stringify());
        if(modifierSignature!=hstring(key)){
            modifierSignature=hstring(key);beginRefresh(L"modifiers");
            auto list=onCategory?modifierCategory.list:modifierMain.list;(onCategory?modifierMain.list:modifierCategory.list).Children().Clear();list.Children().Clear();
            for(auto const& row:visible){
                auto held=row.GetNamedValue(L"key");auto entry=navRow(str(row,L"label"),str(row,L"detail"),str(row,L"action"),act(L"edit_modifier_key",{{L"key",held}}),L"modifier-"+str(held.GetObject(),L"key")+(flag(held.GetObject(),L"command")?L"-command":L"")+(flag(held.GetObject(),L"shift")?L"-shift":L"")+(flag(held.GetObject(),L"alt")?L"-alt":L""));add(list,entry.button);
                remember([this,entry,held]{for(auto value:array(page(),L"modifiers")){auto current=value.GetObject();if(current.GetNamedValue(L"key").Stringify()==held.Stringify()){update(entry,str(current,L"label"),str(current,L"detail"),str(current,L"action"));break;}}});
            }
            if(onCategory)add(list,buttonRow(L"plus",L"add_modifier",act(L"add_modifier_key"),L"add-modifier-key"));
        }
        refresh(L"modifiers");
    }
    void refreshTriggers(){
        if(!inputRoot)return;
        auto triggers=array(page(),L"triggers");std::wstring key;hstring section;uint32_t sectionIndex=0;
        for(auto value:triggers){auto t=value.GetObject();if(str(t,L"section")!=section){section=str(t,L"section");key+=L"#";}key+=L"\n"+std::wstring(str(t,L"id"));}
        if(triggerSignature!=hstring(key)){
            triggerSignature=hstring(key);beginRefresh(L"triggers");
            for(auto const& [name,group]:triggerGroups){uint32_t index;if(inputRoot.Children().IndexOf(group.section,index))inputRoot.Children().RemoveAt(index);}
            triggerGroups.clear();triggerRows.clear();section=L"";
            for(auto value:triggers){
                auto trigger=value.GetObject();auto id=str(trigger,L"id");
                if(str(trigger,L"section")!=section){
                    section=str(trigger,L"section");auto g=group(section);auto groupId=to_hstring(sectionIndex++);AutomationProperties::SetAutomationId(g.section,L"triggers-"+groupId);
                    triggerGroups.emplace(groupId.c_str(),g);inputRoot.Children().Append(g.section);
                    remember([this,g,id]{title(g,str(find(array(page(),L"triggers"),L"id",id),L"section"));});
                }
                bool penButton=std::wstring_view(id).starts_with(L"pen.");
                auto row=navRow(str(trigger,L"label"),str(trigger,L"detail"),str(trigger,L"action"),penButton?act(L"edit_pen_button",{{L"trigger",S(id)}}):act(L"open_action_picker",{{L"trigger",S(id)}}),L"trigger-"+id);
                add(triggerGroups.at(to_hstring(sectionIndex-1).c_str()).list,row.button);triggerRows.emplace(id.c_str(),row);
            }
        }
        refresh(L"triggers");
        for(auto value:triggers){auto t=value.GetObject();if(auto it=triggerRows.find(str(t,L"id").c_str());it!=triggerRows.end())update(it->second,str(t,L"label"),str(t,L"detail"),str(t,L"action"));}
    }
    void perTool(StackPanel const& pane,hstring& signature,J const& binding,hstring const& prefix,hstring const& summary,J reset,
        std::function<J(bool)> same,std::function<J(V)> pick,J remove=J{},wchar_t const* removeKey=L""){
        auto key=(prefix==L"modifier"?binding.GetNamedValue(L"key").Stringify():str(binding,L"trigger"))+(flag(binding,L"modified")?L"*":L"")+(remove.Size()?L"|r":L"");
        for(auto value:array(binding,L"actions"))key=key+value.GetObject().GetNamedValue(L"category",JsonValue::CreateNullValue()).Stringify();
        auto current=[this,prefix]{return object(preferences(),prefix==L"modifier"?L"modifier_editor":L"pen_button_editor");};
        if(signature!=key){
            signature=key;beginRefresh(std::wstring(prefix));pane.Children().Clear();auto section=group(L"",summary);AutomationProperties::SetAutomationId(section.summary,prefix==L"modifier"?L"modifier-hold-summary":L"pen-action-summary");
            remember([this,section,current,prefix]{auto value=current();section.summary.Text(prefix==L"modifier"?data->caption(O({{L"type",S(L"modifier_hold")},{L"label",S(str(value,L"label"))}})):copy(L"pen_action_help"));});
            if(flag(binding,L"modified"))section.actions.Children().Append(glyphButton(L"reset",L"reset_default",reset,prefix+L"-reset"));
            auto row=line();TextBlock heading{nullptr};cell(row,words(L"",L"",&heading),0);
            ToggleSwitch toggle;toggle.MinWidth(0);toggle.OnContent(box_value(L""));toggle.OffContent(box_value(L""));
            AutomationProperties::SetAutomationId(toggle,prefix+L"-same");
            toggle.Toggled([data=data,same](auto&& sender,auto&&){if(!data->updating)send(data,same(!sender.template as<ToggleSwitch>().IsOn()));});cell(row,toggle,1);add(section.list,row);
            remember([this,heading,toggle,current]{auto text=copy(L"same_all_tools");heading.Text(text);AutomationProperties::SetName(toggle,text);auto selected=!flag(current(),L"per_tool");if(toggle.IsOn()!=selected)toggle.IsOn(selected);});
            for(auto value:array(binding,L"actions")){
                auto action=value.GetObject();auto area=action.GetNamedValue(L"category",JsonValue::CreateNullValue());auto name=area.ValueType()==JsonValueType::String?area.GetString():hstring(L"all");
                auto entry=navRow(str(action,L"label"),L"",str(action,L"action"),pick(area),prefix+L"-action-"+name);add(section.list,entry.button);
                remember([entry,current,area]{for(auto value:array(current(),L"actions")){auto action=value.GetObject();if(action.GetNamedValue(L"category",JsonValue::CreateNullValue()).Stringify()==area.Stringify()){update(entry,str(action,L"label"),L"",str(action,L"action"));break;}}});
            }
            pane.Children().Append(section.section);
            if(remove.Size()){auto removal=group(L"");add(removal.list,buttonRow(L"delete",removeKey,remove,prefix+L"-remove",true));pane.Children().Append(removal.section);}
        }
        refresh(std::wstring(prefix));
    }
    void refreshEditor(){
        auto spec=object(preferences(),L"shortcut_editor");if(!spec.Size()){show(editor,false);return;}
        auto capture=object(preferences(),L"capture");if(str(capture,L"id")!=str(spec,L"id"))capture=J{};
        auto signature=str(spec,L"id")+L"|"+to_hstring(array(spec,L"bindings").Size())+L"|"+to_hstring(array(spec,L"gestures").Size())+(flag(spec,L"modified")?L"*":L"")+(capture.Size()?L"|capture":flag(spec,L"can_add")?L"|add":L"");
        if(editor.signature!=signature){
            editor.signature=signature;beginRefresh(L"editor");editor.body.Children().Clear();auto id=str(spec,L"id");
            auto description=label(data,L"");description.TextWrapping(TextWrapping::Wrap);description.Opacity(.55);description.TextAlignment(TextAlignment::Center);editor.body.Children().Append(description);
            auto problem=label(data,L"");problem.TextWrapping(TextWrapping::Wrap);editor.body.Children().Append(problem);
            auto section=group(L"",L" ");AutomationProperties::SetAutomationId(section.summary,L"shortcut-default-summary");
            remember([this,description,problem,section]{auto current=object(preferences(),L"shortcut_editor");editor.title.Text(str(current,L"label"));description.Text(str(current,L"description"));auto capture=object(preferences(),L"capture");auto error=str(capture,L"id")==str(current,L"id")?hstring():str(preferences(),L"error");problem.Text(error);problem.Visibility(error.empty()?Visibility::Collapsed:Visibility::Visible);problem.Foreground(fill(color(data->theme()==L"dark"?L"#ff7b63":L"#c01c28")));auto summary=data->caption(O({{L"type",S(L"shortcut_defaults")},{L"keys",array(current,L"defaults")}}));for(auto overlap:array(current,L"overlaps"))summary=summary+L"\n"+overlap.GetString();section.summary.Text(summary);});
            if(flag(spec,L"modified"))section.actions.Children().Append(glyphButton(L"reset",L"reset_default",act(L"reset_shortcut",{{L"id",S(id)}}),L"shortcut-editor-reset"));
            auto bindings=array(spec,L"bindings");
            for(uint32_t i=0;i<bindings.Size();++i){
                auto row=line();TextBlock heading{nullptr};cell(row,words(bindings.GetStringAt(i),L"",&heading),0);
                remember([this,i,heading]{heading.Text(array(object(preferences(),L"shortcut_editor"),L"bindings").GetStringAt(i));});
                cell(row,glyphButton(L"delete",L"remove_shortcut",act(L"remove_shortcut",{{L"id",S(id)},{L"index",N(i)}}),L"remove-shortcut-"+to_hstring(i)),3);add(section.list,row);
            }
            if(capture.Size())add(section.list,recordingRow(capture));else if(flag(spec,L"can_add"))add(section.list,buttonRow(L"plus",L"add_shortcut",act(L"begin_shortcut",{{L"id",S(id)}}),L"add-shortcut"));
            editor.body.Children().Append(section.section);
            if(auto gestures=array(spec,L"gestures");gestures.Size()){
                auto touch=group(copy(L"pen_touch"),copy(L"pen_page_help"));remember([this,touch]{title(touch,copy(L"pen_touch"));touch.summary.Text(copy(L"pen_page_help"));});
                for(uint32_t i=0;i<gestures.Size();++i){auto row=line();TextBlock heading{nullptr};cell(row,words(gestures.GetStringAt(i),L"",&heading),0);add(touch.list,row);remember([this,i,heading]{heading.Text(array(object(preferences(),L"shortcut_editor"),L"gestures").GetStringAt(i));});}
                editor.body.Children().Append(touch.section);
            }
        }
        refresh(L"editor");show(editor,true);
    }
    void refreshModifierSheet(){
        auto capture=object(preferences(),L"capture");if(str(capture,L"id")!=L"modifier"){show(modifierSheet,false);return;}
        if(modifierSheet.signature.empty()){
            modifierSheet.signature=L"capture";beginRefresh(L"modifier-capture");modifierSheet.body.Children().Clear();
            auto prompt=label(data,L"");prompt.TextAlignment(TextAlignment::Center);prompt.Opacity(.55);modifierSheet.body.Children().Append(prompt);
            remember([this,prompt]{modifierSheet.title.Text(copy(L"new_modifier"));prompt.Text(copy(L"press_hold_key"));});
            auto section=group(L"");add(section.list,recordingRow(capture));modifierSheet.body.Children().Append(section.section);
        }
        refresh(L"modifier-capture");show(modifierSheet,true);
    }
    void refreshPicker(){
        auto spec=pickerModel();if(!spec.Size()){show(picker,false);return;}
        picker.title.Text(str(spec,L"title"));pickerDescription.Text(str(spec,L"description"));picker.start.Visibility(flag(spec,L"modified")?Visibility::Visible:Visibility::Collapsed);
        auto query=str(spec,L"query");if(pickerSearch.Text()!=query)pickerSearch.Text(query);
        auto signature=str(spec,L"trigger")+(flag(spec,L"nothing_visible")?L"|nothing":L"");
        for(auto value:array(spec,L"sections")){signature=signature+L"#";for(auto action:array(value.GetObject(),L"actions"))signature=signature+L","+str(action.GetObject(),L"id");}
        if(picker.signature!=signature){
            picker.signature=signature;beginRefresh(L"picker");pickerList.Children().Clear();
            auto choice=[this](hstring const& id,std::function<J()> current){
                auto result=button(data,L"",[data=data,id]{send(data,act(L"choose_action",{{L"id",S(id)}}));});
                result.HorizontalAlignment(HorizontalAlignment::Stretch);result.HorizontalContentAlignment(HorizontalAlignment::Stretch);result.CornerRadius({0,0,0,0});result.MinHeight(54);result.FontWeight(winrt::Windows::UI::Text::FontWeights::Normal());result.Padding({14,8,14,8});
                Grid content;content.ColumnSpacing(6);ColumnDefinition text;text.Width({1,GridUnitType::Star});ColumnDefinition tail;tail.Width({1,GridUnitType::Auto});content.ColumnDefinitions().Append(text);content.ColumnDefinitions().Append(tail);
                TextBlock heading{nullptr},detail{nullptr};cell(content,words(L"",L"",&heading,&detail),0);auto check=themed(L"check");cell(content,check,1);result.Content(content);AutomationProperties::SetAutomationId(result,L"action-"+(id.empty()?hstring(L"nothing"):id));
                remember([this,result,heading,detail,check,id,current]{auto action=current();auto title=id.empty()?str(action,L"nothing_label"):str(action,L"label"),help=id.empty()?hstring():str(action,L"detail");bool selected=flag(action,id.empty()?L"nothing":L"selected");heading.Text(title);detail.Text(help);detail.Visibility(help.empty()?Visibility::Collapsed:Visibility::Visible);check.Opacity(selected?1:0);AutomationProperties::SetName(result,title);AutomationProperties::SetItemStatus(result,selected?data->caption(L"search",L"selected"):hstring());});return result;
            };
            bool any=false;
            if(flag(spec,L"nothing_visible")){auto none=group(L"");add(none.list,choice(L"",[this]{return pickerModel();}));pickerList.Children().Append(none.section);any=true;}
            auto sections=array(spec,L"sections");
            for(uint32_t i=0;i<sections.Size();++i){
                auto section=sections.GetObjectAt(i);auto list=group(str(section,L"title"));remember([this,i,list]{title(list,str(array(pickerModel(),L"sections").GetObjectAt(i),L"title"));});
                for(auto value:array(section,L"actions")){auto id=str(value.GetObject(),L"id");add(list.list,choice(id,[this,i,id]{return find(array(array(pickerModel(),L"sections").GetObjectAt(i),L"actions"),L"id",id);}));}
                pickerList.Children().Append(list.section);any=true;
            }
            if(!any){
                StackPanel none;none.Spacing(6);none.HorizontalAlignment(HorizontalAlignment::Center);none.Margin({0,24,0,24});auto heading=label(data,L"",true);heading.HorizontalAlignment(HorizontalAlignment::Center);none.Children().Append(heading);auto hint=note(data,L"");hint.HorizontalAlignment(HorizontalAlignment::Center);none.Children().Append(hint);pickerList.Children().Append(none);remember([this,heading,hint]{heading.Text(copy(L"no_results"));hint.Text(copy(L"search_help"));});
            }
        }
        refresh(L"picker");bool opening=picker.frame.Visibility()==Visibility::Collapsed;show(picker,true);if(opening)pickerSearch.Focus(FocusState::Programmatic);
    }
    void show(Sheet& sheet,bool open){
        auto state=open?Visibility::Visible:Visibility::Collapsed;
        if(sheet.frame.Visibility()!=state)sheet.frame.Visibility(state);
    }
    void layoutSheets(){
        Sheet* order[]{&import,&picker,&modifierSheet,&editor,&details};
        Sheet* top=nullptr;
        for(auto sheet:order){if(!top&&sheet->frame.Visibility()==Visibility::Visible)top=sheet;else if(top)sheet->frame.Opacity(0);}
        for(auto sheet:order){
            bool front=sheet==top;sheet->frame.Opacity(front?1:0);sheet->frame.IsHitTestVisible(front);
            AutomationProperties::SetName(sheet->frame,sheet->title.Text());
            sheet->frame.Background(data->brush(L"settings"));
        }
        auto host=overlay.Parent().try_as<FrameworkElement>();
        double width=host?host.Width():0,height=host?host.Height():0;
        if(!std::isfinite(width)||width<=0)width=SheetWidth+32;
        if(!std::isfinite(height)||height<=0)height=PickerHeight+32;
        for(auto sheet:order){
            sheet->frame.Width(std::min(SheetWidth,std::max(280.,width-32)));
            double limit=std::max(200.,height-32);
            if(sheet==&picker||sheet==&details)sheet->frame.Height(std::min(PickerHeight,limit));else sheet->frame.MaxHeight(limit);
        }
        overlay.Background(top?fill(winrt::Windows::UI::Color{uint8_t(data->theme()==L"dark"?0x88:0x22),0,0,0}):nullptr);
        overlay.Visibility(top?Visibility::Visible:Visibility::Collapsed);
    }
    void finish(uint32_t id,hstring const& diagnostic,hstring const& text,bool tooLarge=false){
        if(!diagnostic.empty())OutputDebugStringW((diagnostic+L"\n").c_str());
        if(!text.empty())send(data,act(L"import_keymap",{{L"text",S(text)}}));
        if(tooLarge||!diagnostic.empty())data->dispatch(O({{L"type",S(L"complete_request_failure")},{L"id",N(id)},{L"reason",S(tooLarge?L"keymap_too_large":L"action_failed")}}));
        else data->dispatch(O({{L"type",S(L"complete_request")},{L"id",N(id)},{L"error",JsonValue::CreateNullValue()}}));
    }
    void background(uint32_t id,std::function<KeymapResult()> job){
        std::thread([weak=weak_from_this(),queue=overlay.DispatcherQueue(),id,job=std::move(job)]{
            auto result=job();
            queue.TryEnqueue([weak,id,result]{if(auto self=weak.lock())self->finish(id,result.diagnostic,result.text,result.tooLarge);});
        }).detach();
    }
    fire_and_forget exportKeymap(uint32_t id,hstring name,hstring text){
        auto lifetime=shared_from_this();hstring path;
        try{
            Pickers::FileSavePicker save(Microsoft::UI::WindowId{data->windowId});save.CommitButtonText(copy(L"export_menu"));
            std::wstring stem(name.c_str());if(auto dot=stem.rfind(L'.');dot!=std::wstring::npos)stem=stem.substr(0,dot);
            save.SuggestedFileName(hstring(stem));save.DefaultFileExtension(L".capykeys");
            save.FileTypeChoices().Insert(copy(L"keymap"),single_threaded_vector<hstring>({L".capykeys"}));
            if(auto picked=co_await save.PickSaveFileAsync())path=picked.Path();
        }catch(hresult_error const& failure){finish(id,failure.message(),L"");co_return;}
        if(path.empty()){finish(id,L"",L"");co_return;}
        background(id,[path=to_string(path),bytes=to_string(text)]{
            if(capy_write_file(path.c_str(),reinterpret_cast<uint8_t const*>(bytes.data()),bytes.size())!=0)
                return KeymapResult{to_hstring(capy_error()),L""};
            return KeymapResult{};
        });
    }
    fire_and_forget importKeymap(uint32_t id){
        auto lifetime=shared_from_this();hstring path;
        try{
            Pickers::FileOpenPicker open(Microsoft::UI::WindowId{data->windowId});open.CommitButtonText(copy(L"import_menu"));
            open.FileTypeFilter().Append(L".capykeys");open.FileTypeFilter().Append(L".json");
            if(auto picked=co_await open.PickSingleFileAsync())path=picked.Path();
        }catch(hresult_error const& failure){finish(id,failure.message(),L"");co_return;}
        if(path.empty()){finish(id,L"",L"");co_return;}
        background(id,[path]{
            constexpr size_t limit=size_t(1)<<20;
            std::ifstream in(std::filesystem::path(path.c_str()),std::ios::binary);
            if(!in)return KeymapResult{L"Could not read the keymap.",L""};
            std::string bytes(limit+1,'\0');in.read(bytes.data(),std::streamsize(bytes.size()));bytes.resize(size_t(in.gcount()));
            if(bytes.size()>limit)return KeymapResult{L"",L"",true};
            if(!bytes.empty()&&!MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,bytes.data(),int(bytes.size()),nullptr,0))
                return KeymapResult{L"The keymap is not UTF-8 text.",L""};
            return KeymapResult{L"",to_hstring(bytes)};
        });
    }
    void requests(){
        auto list=array(object(model,L"state"),L"requests");std::set<uint32_t> present;
        for(auto value:list)present.insert(uint32_t(num(value.GetObject(),L"id")));
        std::erase_if(handledRequests,[&](uint32_t id){return !present.contains(id);});
        for(auto value:list){
            auto request=value.GetObject();auto id=uint32_t(num(request,L"id"));if(handledRequests.contains(id))continue;
            auto kind=object(request,L"kind");auto type=str(kind,L"type");
            if(type==L"export_keymap"){handledRequests.insert(id);exportKeymap(id,str(kind,L"name"),str(kind,L"text"));}
            else if(type==L"import_keymap"){handledRequests.insert(id);importKeymap(id);}
        }
    }
    void Apply(J const& snapshot){
        model=snapshot;
        requests();
        if(!preferences().Size()){for(auto sheet:{&editor,&modifierSheet,&picker,&details,&import})show(*sheet,false);layoutSheets();return;}
        if(iconTheme!=data->theme()){
            iconTheme=data->theme();if(keymapMore)keymapMore.Content(icon(L"more",iconTheme));
            for(auto sheet:{&editor,&modifierSheet,&picker,&details,&import})sheet->close.Content(icon(L"window-close",iconTheme));
        }
        if(root.Children().Size()){
            refreshKeymap();refreshFilters();refreshCategories();refreshResults();refreshModifiers();
            auto modifierEditor=object(preferences(),L"modifier_editor");
            if(modifierEditor.Size())perTool(modifier,modifierPane,modifierEditor,L"modifier",data->caption(O({{L"type",S(L"modifier_hold")},{L"label",S(str(modifierEditor,L"label"))}})),
                act(L"reset_modifier_key",{{L"key",modifierEditor.GetNamedValue(L"key")}}),
                [key=modifierEditor.GetNamedValue(L"key")](bool perTool){return act(L"modifier_key_per_tool",{{L"key",key},{L"per_tool",B(perTool)}});},
                [key=modifierEditor.GetNamedValue(L"key")](V category){return act(L"open_modifier_picker",{{L"key",key},{L"category",category}});},
                act(L"remove_modifier_key",{{L"key",modifierEditor.GetNamedValue(L"key")}}),L"remove_modifier");
            auto pane=modifierEditor.Size()?modifier:str(page(),L"category").empty()?root:category;
            for(auto candidate:{root,category,modifier})candidate.Visibility(candidate==pane?Visibility::Visible:Visibility::Collapsed);
        }
        refreshTriggers();
        if(pen){
            auto editorModel=object(preferences(),L"pen_button_editor");
            if(editorModel.Size())perTool(pen,penPane,editorModel,L"pen-button",copy(L"pen_action_help"),
                act(L"reset_trigger",{{L"trigger",S(str(editorModel,L"trigger"))}}),
                [trigger=str(editorModel,L"trigger")](bool perTool){return act(L"pen_button_per_tool",{{L"trigger",S(trigger)},{L"per_tool",B(perTool)}});},
                [trigger=str(editorModel,L"trigger")](V category){return act(L"open_pen_button_picker",{{L"trigger",S(trigger)},{L"category",category}});});
            pen.Visibility(editorModel.Size()?Visibility::Visible:Visibility::Collapsed);
            inputRoot.Visibility(editorModel.Size()?Visibility::Collapsed:Visibility::Visible);
        }
        refreshEditor();refreshModifierSheet();refreshPicker();
        layoutSheets();
    }
    hstring Title()const{
        auto spec=preferences();auto current=str(spec,L"page");
        if(current==L"shortcuts"){
            if(auto held=object(spec,L"modifier_editor");held.Size())return str(held,L"label");
            if(auto id=str(page(),L"category");!id.empty())return categoryLabel(id);
            return L"";
        }
        if(current==L"input")return str(object(spec,L"pen_button_editor"),L"label");
        return L"";
    }
    void Back(){
        auto spec=preferences();
        if(str(spec,L"page")==L"input"&&object(spec,L"pen_button_editor").Size())send(data,act(L"close_pen_button"));
        else if(object(spec,L"modifier_editor").Size())send(data,act(L"close_modifier_key"));
        else send(data,act(L"shortcut_category",{{L"id",JsonValue::CreateNullValue()}}));
    }
    bool Dismiss(){
        auto spec=preferences();auto keymap=object(spec,L"keymap");
        if(object(spec,L"capture").Size()){send(data,act(L"cancel_shortcut"));return true;}
        if(pickerModel().Size()){send(data,act(L"close_action_picker"));return true;}
        if(object(keymap,L"import").Size()){send(data,act(L"cancel_keymap_import"));return true;}
        if(flag(keymap,L"details")){send(data,act(L"keymap_details",{{L"open",B(false)}}));return true;}
        if(object(spec,L"shortcut_editor").Size()){send(data,act(L"close_shortcut_editor"));return true;}
        if(!Title().empty()){Back();return true;}
        return false;
    }
};

ShortcutPage::ShortcutPage(std::shared_ptr<WorkspaceData> data,Grid overlay):impl(std::make_shared<Impl>()){
    impl->data=std::move(data);impl->overlay=std::move(overlay);impl->init();
}
ShortcutPage::~ShortcutPage()=default;
StackPanel ShortcutPage::Container(hstring const& page,StackPanel const& node){return impl->Container(page,node);}
void ShortcutPage::Apply(J const& snapshot){impl->Apply(snapshot);}
hstring ShortcutPage::Title()const{return impl->Title();}
void ShortcutPage::Back(){impl->Back();}
bool ShortcutPage::Dismiss(){return impl->Dismiss();}
