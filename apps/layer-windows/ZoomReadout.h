#pragma once
#include "UiControls.h"
#include "WorkspaceQuery.h"
#include "NavigationControls.h"
#include <winrt/Microsoft.UI.Xaml.Controls.Primitives.h>

namespace CapyUi {
struct ZoomReadout:std::enable_shared_from_this<ZoomReadout>{
    static constexpr double Width=220;
    std::shared_ptr<WorkspaceData> data;
    Button root{nullptr};
    TextBlock text;
    Flyout popup;
    ScrollViewer scroller;
    StackPanel body,items,rotationItems;
    Bindings fields,controls;
    A buttons;
    double zoom=1,rotation=0;
    hstring flipped;bool zoomLocked=false,rotationLocked=false;
    hstring menuKey;
    uint64_t transient=0,request=0;
    bool open=false;
    weak_ref<Control> previous;

    void separator(){Border line;line.Height(1);line.Background(data->tint(L"text",36));body.Children().Append(line);}
    void field(LocalizedCopy const& name,wchar_t const* spec,hstring const& id,double ZoomReadout::* value,std::function<J(double)> action){
        auto weak=weak_from_this();NumberPresentation presentation;presentation.title=name.current;
        auto control=number(data,name,object(data->catalog,spec),[weak,value]{auto self=weak.lock();return self?(*self).*value:0.;},
            [weak,action](double next){if(auto self=weak.lock())self->data->dispatch(action(next));},fields,nullptr,false,id,true,presentation);
        if(auto entry=numberEntry(control))entry.AddHandler(UIElement::KeyDownEvent(),box_value(KeyEventHandler([weak](auto&&,KeyRoutedEventArgs const& e){
            auto key=e.Key();
            if(key==Windows::System::VirtualKey::Enter||key==Windows::System::VirtualKey::Escape)if(auto self=weak.lock())self->restoreFocus();
        })),true);
        body.Children().Append(control);
    }
    void init(){
        auto weak=weak_from_this();
        text.FontSize(data->textSize());text.FontWeight(Windows::UI::Text::FontWeights::Normal());text.IsHitTestVisible(false);
        AutomationProperties::SetAutomationId(text,L"canvas-camera");
        root=button(data,L"",[weak]{if(auto self=weak.lock())self->toggle();});
        root.Content(text);root.Padding({10,3,10,3});root.AllowFocusOnInteraction(false);root.IsTabStop(false);
        root.ContextRequested([weak](auto&&,ContextRequestedEventArgs const& e){e.Handled(true);if(auto self=weak.lock();self&&!self->open)self->toggle();});
        AutomationProperties::SetAutomationId(root,L"canvas-view-info");
        auto name=data->copyCaption(L"header",L"zoom");
        auto rename=[button=root,menu=popup](hstring const& text){AutomationProperties::SetName(button,text);tooltip(button,text);AutomationProperties::SetName(menu,text);};
        rename(name);data->copyView([weak,rename,resolve=name.current]{if(!weak.lock())return false;rename(resolve());return true;});
        body.Width(Width);body.Spacing(6);
        field(name,L"zoom",L"zoom-field",&ZoomReadout::zoom,[](double value){return O({{L"type",S(L"set_zoom")},{L"zoom",N(value)}});});
        separator();items.Spacing(2);body.Children().Append(items);separator();
        field(data->copyCaption(L"header",L"rotation"),L"rotation",L"rotation-field",&ZoomReadout::rotation,[](double value){return O({{L"type",S(L"set_rotation")},{L"rotation",N(value)}});});
        separator();rotationItems.Spacing(2);body.Children().Append(rotationItems);separator();
        auto row=NavigationButtons(data,L"zoom-button",[weak]{auto self=weak.lock();return self?self->buttons:A{};},controls);
        for(auto child:row.Children())if(auto pick=child.try_as<Button>()){pick.AllowFocusOnInteraction(false);pick.IsTabStop(false);}
        body.Children().Append(row);
        scroller.Content(body);scroller.HorizontalScrollMode(ScrollMode::Disabled);scroller.HorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);
        scroller.VerticalScrollMode(ScrollMode::Auto);scroller.VerticalScrollBarVisibility(ScrollBarVisibility::Auto);
        popup.Content(scroller);popup.ShowMode(Primitives::FlyoutShowMode::Transient);popup.Placement(Primitives::FlyoutPlacementMode::TopEdgeAlignedRight);
        popup.Opened([weak](auto&&,auto&&){if(auto self=weak.lock()){self->open=true;self->data->popup(true);}});
        popup.Closed([weak](auto&&,auto&&){if(auto self=weak.lock()){
            self->open=false;self->data->popup(false);self->menuKey=L"";
            auto focus=FocusManager::GetFocusedElement(self->root.XamlRoot()).try_as<UIElement>();
            if(!focus||self->within(focus))self->restoreFocus();
            self->previous=nullptr;
        }});
        transient=data->transient([weak]{auto self=weak.lock();if(!self||!self->open)return false;self->popup.Hide();return true;});
    }
    ~ZoomReadout(){if(data)data->transients.erase(transient);}
    void Update(J const& camera){
        if(!camera.Size())return;
        auto next=to_hstring(int(std::round(num(camera,L"zoom",1)*100)))+L"% · "+to_hstring(int(std::round(num(camera,L"rotation")*180/3.141592653589793)))+L"°";
        if(text.Text()!=next)text.Text(next);
        double nextZoom=num(camera,L"zoom",1),nextRotation=num(camera,L"rotation");
        auto nextFlipped=array(camera,L"flipped").Stringify();bool nextZoomLocked=flag(camera,L"zoom_locked"),nextRotationLocked=flag(camera,L"rotation_locked");
        bool moved=nextZoom!=zoom||nextRotation!=rotation;
        bool menu=nextZoom!=zoom||nextFlipped!=flipped||nextZoomLocked!=zoomLocked||nextRotationLocked!=rotationLocked;
        zoom=nextZoom;rotation=nextRotation;flipped=nextFlipped;zoomLocked=nextZoomLocked;rotationLocked=nextRotationLocked;
        if(!open)return;
        if(moved)for(auto const& bind:fields)bind();
        if(menu)query();
    }
    void toggle(){
        if(open){popup.Hide();return;}
        previous=FocusManager::GetFocusedElement(root.XamlRoot()).try_as<Control>();
        for(auto const& bind:fields)bind();
        query();
    }
    bool within(UIElement const& element)const{
        for(DependencyObject node=element;node;node=VisualTreeHelper::GetParent(node))if(node==body)return true;
        return false;
    }
    void restoreFocus(){
        if(auto focus=previous.get();focus&&focus.IsLoaded())focus.Focus(FocusState::Programmatic);
    }
    void query(){
        auto serial=++request;auto weak=weak_from_this();
        QueryWorkspace(data->query,O({{L"type",S(L"zoom_menu")}}),[weak,serial](J reply){
            auto self=weak.lock();if(!self||serial!=self->request)return;
            self->render(object(reply,L"result"));
            if(!self->open){
                if(auto xaml=self->root.XamlRoot())self->scroller.MaxHeight(std::max(0.,double(xaml.Size().Height)-12));
                Primitives::FlyoutShowOptions options;options.Placement(Primitives::FlyoutPlacementMode::TopEdgeAlignedRight);
                options.ShowMode(Primitives::FlyoutShowMode::Transient);
                self->popup.ShowAt(self->root,options);
            }
        });
    }
    static hstring itemId(J const& action){
        auto type=str(action,L"type");
        if(type==L"invoke")return L"zoom-"+str(action,L"command");
        if(type==L"set_zoom")return L"zoom-"+to_hstring(int(std::round(num(action,L"zoom")*100)));
        if(type==L"set_zoom_locked")return L"zoom-lock-zoom";
        if(type==L"set_rotation_locked")return L"zoom-lock-rotation";
        if(type==L"set_rotation")return L"zoom-reset-rotation";
        return L"zoom-item";
    }
    void rows(StackPanel const& target,A const& sections,uint32_t from,uint32_t to){
        target.Children().Clear();
        for(uint32_t s=from;s<to&&s<sections.Size();++s){
            if(s>from){Border line;line.Height(1);line.Margin({0,2,0,2});line.Background(data->tint(L"text",36));target.Children().Append(line);}
            for(auto value:sections.GetArrayAt(s)){
                auto item=value.GetObject();auto action=object(item,L"action");auto weak=weak_from_this();
                auto row=button(data,L"",[weak,action]{if(auto self=weak.lock()){self->data->dispatch(action);self->popup.Hide();}});
                row.HorizontalAlignment(HorizontalAlignment::Stretch);row.HorizontalContentAlignment(HorizontalAlignment::Stretch);
                row.Padding({10,6,10,6});row.MinHeight(34);row.AllowFocusOnInteraction(false);row.IsTabStop(false);
                row.FontWeight(Windows::UI::Text::FontWeights::Normal());
                auto checked=item.GetNamedValue(L"selected",JsonValue::CreateNullValue());
                bool chosen=checked.ValueType()==JsonValueType::Boolean&&checked.GetBoolean();
                Grid content;
                for(auto width:{GridLength{16,GridUnitType::Pixel},GridLength{1,GridUnitType::Star},GridLength{1,GridUnitType::Auto}}){
                    ColumnDefinition column;column.Width(width);content.ColumnDefinitions().Append(column);
                }
                if(chosen){auto mark=icon(L"check",data->theme(),16);mark.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(mark);}
                auto caption=label(data,str(item,L"label"));caption.VerticalAlignment(VerticalAlignment::Center);caption.Margin({8,0,12,0});
                Grid::SetColumn(caption,1);content.Children().Append(caption);
                auto shortcut=label(data,str(item,L"hint"));shortcut.Opacity(.55);shortcut.VerticalAlignment(VerticalAlignment::Center);
                Grid::SetColumn(shortcut,2);content.Children().Append(shortcut);
                row.Content(content);row.IsEnabled(flag(item,L"enabled",true));
                AutomationProperties::SetAutomationId(row,itemId(action));AutomationProperties::SetName(row,str(item,L"label"));
                AutomationProperties::SetItemStatus(row,chosen?data->caption(L"search",L"selected"):hstring());
                target.Children().Append(row);
            }
        }
    }
    void render(J const& menu){
        buttons=array(menu,L"buttons");for(auto const& bind:controls)bind();
        auto key=array(menu,L"sections").Stringify()+data->theme();
        if(key==menuKey)return;
        menuKey=key;
        auto sections=array(menu,L"sections");auto boundary=uint32_t(num(menu,L"rotation_section",sections.Size()));
        rows(items,sections,0,boundary);rows(rotationItems,sections,boundary,sections.Size());
    }
};
}
