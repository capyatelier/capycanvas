#pragma once
#include "UiControls.h"
#include "WorkspaceQuery.h"
#include <winrt/Microsoft.UI.Xaml.Controls.Primitives.h>

namespace CapyUi {
struct ZoomReadout:std::enable_shared_from_this<ZoomReadout>{
    static constexpr double Width=220;
    std::shared_ptr<WorkspaceData> data;
    Button root{nullptr};
    TextBlock text;
    Flyout popup;
    StackPanel body,items;
    Bindings fields;
    double zoom=1;
    hstring menuKey;
    uint64_t transient=0,request=0;
    bool open=false;
    weak_ref<Control> previous;

    void init(){
        auto weak=weak_from_this();
        text.FontSize(data->textSize());text.FontWeight(Windows::UI::Text::FontWeights::Normal());text.IsHitTestVisible(false);
        AutomationProperties::SetAutomationId(text,L"canvas-camera");
        root=button(data,L"",[weak]{if(auto self=weak.lock())self->toggle();});
        root.Content(text);root.Padding({10,3,10,3});root.AllowFocusOnInteraction(false);root.IsTabStop(false);
        AutomationProperties::SetAutomationId(root,L"canvas-view-info");
        auto name=data->copyCaption(L"header",L"zoom");
        auto rename=[button=root,menu=popup](hstring const& text){AutomationProperties::SetName(button,text);tooltip(button,text);AutomationProperties::SetName(menu,text);};
        rename(name);data->copyView([weak,rename,resolve=name.current]{if(!weak.lock())return false;rename(resolve());return true;});
        NumberPresentation presentation;presentation.title=name.current;
        body.Width(Width);body.Spacing(6);
        body.Children().Append(number(data,name,object(data->catalog,L"zoom"),[weak]{auto self=weak.lock();return self?self->zoom:1.;},
            [weak](double value){if(auto self=weak.lock())self->data->dispatch(O({{L"type",S(L"set_zoom")},{L"zoom",N(value)}}));},fields,nullptr,false,L"zoom-field",false,presentation));
        if(auto entry=numberEntry(body.Children().GetAt(0))){
            entry.AddHandler(UIElement::KeyDownEvent(),box_value(KeyEventHandler([weak](auto&&,KeyRoutedEventArgs const& e){
                auto key=e.Key();
                if(key==Windows::System::VirtualKey::Enter||key==Windows::System::VirtualKey::Escape)if(auto self=weak.lock())self->restoreFocus();
            })),true);
        }
        Border line;line.Height(1);line.Background(data->tint(L"text",36));body.Children().Append(line);
        items.Spacing(2);body.Children().Append(items);
        popup.Content(body);popup.ShowMode(Primitives::FlyoutShowMode::Transient);popup.Placement(Primitives::FlyoutPlacementMode::TopEdgeAlignedRight);
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
        double value=num(camera,L"zoom",1);
        if(value==zoom)return;
        zoom=value;
        if(!open)return;
        for(auto const& bind:fields)bind();
        query();
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
        return L"zoom-item";
    }
    void render(J const& menu){
        auto key=array(menu,L"sections").Stringify()+data->theme();
        if(key==menuKey)return;
        menuKey=key;items.Children().Clear();
        auto sections=array(menu,L"sections");
        for(uint32_t s=0;s<sections.Size();++s){
            if(s){Border line;line.Height(1);line.Margin({0,2,0,2});line.Background(data->tint(L"text",36));items.Children().Append(line);}
            for(auto value:sections.GetArrayAt(s)){
                auto item=value.GetObject();auto action=object(item,L"action");auto weak=weak_from_this();
                auto row=button(data,L"",[weak,action]{if(auto self=weak.lock()){self->data->dispatch(action);self->popup.Hide();}});
                row.HorizontalAlignment(HorizontalAlignment::Stretch);row.HorizontalContentAlignment(HorizontalAlignment::Stretch);
                row.Padding({10,6,10,6});row.MinHeight(34);row.AllowFocusOnInteraction(false);row.IsTabStop(false);
                row.FontWeight(Windows::UI::Text::FontWeights::Normal());
                Grid content;ColumnDefinition name;name.Width({1,GridUnitType::Star});ColumnDefinition keys;keys.Width({1,GridUnitType::Auto});
                content.ColumnDefinitions().Append(name);content.ColumnDefinitions().Append(keys);content.ColumnSpacing(12);
                auto caption=label(data,str(item,L"label"));caption.VerticalAlignment(VerticalAlignment::Center);content.Children().Append(caption);
                auto shortcut=label(data,str(item,L"shortcut"));shortcut.Opacity(.55);shortcut.VerticalAlignment(VerticalAlignment::Center);
                Grid::SetColumn(shortcut,1);content.Children().Append(shortcut);
                row.Content(content);row.IsEnabled(flag(item,L"enabled",true));
                AutomationProperties::SetAutomationId(row,itemId(action));AutomationProperties::SetName(row,str(item,L"label"));
                items.Children().Append(row);
            }
        }
    }
};
}
