// Separate production Color control review: build.ps1 -ControlFixture Color.
#include "pch.h"
#include "ColorView.h"
#include <fstream>
#include <iterator>
using namespace CapyUi;
namespace {
int resultCode=0;
struct Fixture:std::enable_shared_from_this<Fixture>{
    struct Item {hstring key;FrameworkElement root;std::shared_ptr<WorkspaceData> data;};
    Window window;Canvas surface;ContentControl viewport;Bindings bindings;
    std::vector<Item> items;J source;std::filesystem::path output;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    hstring previous;int stable=0;bool focused=false;
    static FrameworkElement findElement(DependencyObject const& parent,hstring const& id){
        if(auto element=parent.try_as<FrameworkElement>();element&&AutomationProperties::GetAutomationId(element)==id)return element;
        for(int i=0;i<VisualTreeHelper::GetChildrenCount(parent);i++)
            if(auto found=findElement(VisualTreeHelper::GetChild(parent,i),id))return found;
        return nullptr;
    }
    J frame(FrameworkElement const& element){
        auto origin=element.TransformToVisual(surface).TransformPoint({0,0});
        return O({{L"x",N(origin.X)},{L"y",N(origin.Y)},{L"width",N(element.ActualWidth())},{L"height",N(element.ActualHeight())}});
    }
    void publish(){
        J frames;
        for(auto const& item:items){
            if(!item.root.IsLoaded())return;
            for(auto id:{L"color-panel",L"color-wheel",L"color-background",L"color-foreground",L"color-transparent",
                L"color-swap",L"color-shape-0",L"color-shape-1",L"color-readout"}){
                auto element=findElement(item.root,id);if(!element||element.ActualWidth()==0)return;
                if(hstring(id)==L"color-wheel"&&AutomationProperties::GetItemStatus(element)!=L"Ready")return;
                frames.Insert(item.key+L"/"+id,frame(element));
            }
        }
        auto state=str(source,L"capture_state");
        if(!focused&&std::wstring_view(state).ends_with(L"-focus")){
            auto id=str(source,L"capture_target");auto target=findElement(items.front().root,id).try_as<Control>();
            if(!target||!target.Focus(FocusState::Keyboard))throw hresult_error(E_FAIL,L"Cannot focus requested review control");
            focused=true;stable=0;return;
        }
        auto current=frames.Stringify();stable=current==previous?stable+1:0;previous=current;if(stable<3)return;
        timer.Stop();auto scale=surface.XamlRoot().RasterizationScale();
        if(std::abs(scale-num(source,L"scale"))>.001)throw hresult_error(E_FAIL,L"Fixture scale does not match the actual display");
        source.Insert(L"frames",frames);source.Insert(L"process_id",N(GetCurrentProcessId()));
        std::ofstream file(output);file<<to_string(source.Stringify());if(!file)throw hresult_error(E_FAIL,L"Cannot write fixture bounds");
    }
    void init(std::filesystem::path const& path){
        std::ifstream input(path);std::string json((std::istreambuf_iterator<char>(input)),std::istreambuf_iterator<char>());
        source=J::Parse(to_hstring(json));if(num(source,L"schema")!=2||array(source,L"items").Size()!=6)throw hresult_invalid_argument(L"Expected compact color fixture");
        output=path.parent_path()/(L"native-"+std::wstring(str(source,L"name"))+L".json");
        surface.Width(num(source,L"width"));surface.Height(num(source,L"height"));
        surface.HorizontalAlignment(HorizontalAlignment::Left);surface.VerticalAlignment(VerticalAlignment::Top);
        surface.Background(fill(color(str(object(source,L"palette"),L"panel"))));
        surface.RequestedTheme(str(source,L"theme")==L"light"?ElementTheme::Light:ElementTheme::Dark);
        AutomationProperties::SetName(surface,L"Compact color control review");AutomationProperties::SetAutomationId(surface,L"color-review-surface");
        for(auto value:array(source,L"items")){
            auto item=value.GetObject();auto data=std::make_shared<WorkspaceData>();data->catalog=object(source,L"catalog");
            data->state=O({{L"theme",source.GetNamedValue(L"theme")},{L"palette",object(source,L"palette")},{L"colors",object(item,L"colors")}});
            data->model=O({{L"color_panel",object(item,L"model")}});
            data->send=[](auto&&){throw hresult_error(E_FAIL,L"Unexpected edit during static color capture");};
            auto root=ColorPanel(data,bindings,true);root.Width(num(item,L"size"));root.Height(num(item,L"size"));
            Canvas::SetLeft(root,num(item,L"x"));Canvas::SetTop(root,num(item,L"y"));surface.Children().Append(root);
            items.push_back({str(item,L"key"),root,data});
        }
        viewport.Content(surface);viewport.IsTabStop(true);viewport.HorizontalContentAlignment(HorizontalAlignment::Left);
        viewport.VerticalContentAlignment(VerticalAlignment::Top);window.Title(L"Capy Canvas compact color review");window.Content(viewport);
        surface.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){
            auto scale=self->surface.XamlRoot().RasterizationScale();
            self->window.AppWindow().ResizeClient({int32_t(std::lround(self->surface.Width()*scale)),int32_t(std::lround(self->surface.Height()*scale))});
            self->viewport.Focus(FocusState::Programmatic);self->timer=self->surface.DispatcherQueue().CreateTimer();
            self->timer.Interval(std::chrono::milliseconds(150));self->timer.IsRepeating(true);
            self->timer.Tick([weak](auto&&,auto&&){if(auto owner=weak.lock())owner->publish();});self->timer.Start();
        }});
        window.AppWindow().Move({40,40});window.Activate();
    }
};
struct App:ApplicationT<App,Markup::IXamlMetadataProvider>{
    Microsoft::UI::Xaml::XamlTypeInfo::XamlControlsXamlMetaDataProvider metadata;
    std::shared_ptr<Fixture> fixture;
    App(){UnhandledException([this](auto&&,UnhandledExceptionEventArgs const& args){
        resultCode=1;args.Handled(true);
        if(fixture){std::ofstream error(fixture->output.parent_path()/L"color-fixture-error.log");error<<to_string(args.Message());}Exit();
    });}
    Markup::IXamlType GetXamlType(Windows::UI::Xaml::Interop::TypeName const& type){return metadata.GetXamlType(type);}
    Markup::IXamlType GetXamlType(hstring const& name){return metadata.GetXamlType(name);}
    com_array<Markup::XmlnsDefinition> GetXmlnsDefinitions(){return metadata.GetXmlnsDefinitions();}
    void OnLaunched(LaunchActivatedEventArgs const&){
        Resources().MergedDictionaries().Append(XamlControlsResources());
        wchar_t path[32768];auto length=GetEnvironmentVariableW(L"CAPY_COLOR_FIXTURE",path,32768);
        if(!length||length>=32768||!std::filesystem::path(path).is_absolute())throw hresult_invalid_argument(L"CAPY_COLOR_FIXTURE must be an absolute fixture path");
        fixture=std::make_shared<Fixture>();fixture->init(path);
    }
};
}
int WINAPI wWinMain(HINSTANCE,HINSTANCE,PWSTR,int){
    init_apartment(apartment_type::single_threaded);Application::Start([](auto&&){make<App>();});return resultCode;
}
