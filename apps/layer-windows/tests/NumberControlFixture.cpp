// Separate review executable: /p:CapyControlFixture=true.
// Uses the production numeric control and Rust resolver without a document/GPU.
#include "pch.h"
#include "UiControls.h"
#include <fstream>
#include <iterator>

using namespace CapyUi;
namespace {
int resultCode=0;
struct Fixture : std::enable_shared_from_this<Fixture> {
    struct Item {hstring key;StackPanel root;J row;};
    Window window;Canvas surface;ContentControl viewport;
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    Bindings bindings;std::vector<Item> items;J source;
    std::filesystem::path output;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::wstring previous;int stable=0;

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
            if(!item.root.IsLoaded()||item.root.ActualWidth()==0)return;
            auto header=item.root.Children().GetAt(0).as<Grid>();
            auto entry=findElement(item.root,item.key).as<TextBox>();
            auto minus=findElement(item.root,item.key+L"-decrease").as<Button>();
            auto plus=findElement(item.root,item.key+L"-increase").as<Button>();
            auto spec=object(item.row,L"control");bool enabled=flag(item.row,L"enabled");
            if(entry.Text()!=str(object(item.row,L"formatted"),L"text")||entry.IsEnabled()!=enabled||
                minus.IsEnabled()!=(enabled&&num(item.row,L"value")>num(spec,L"min"))||
                plus.IsEnabled()!=(enabled&&num(item.row,L"value")<num(spec,L"max")))
                throw hresult_error(E_FAIL,L"Numeric display or step availability differs from the shared model");
            if(std::abs(minus.Opacity()-(minus.IsEnabled()?1.:.36))>.001||std::abs(plus.Opacity()-(plus.IsEnabled()?1.:.36))>.001)
                throw hresult_error(E_FAIL,L"Numeric disabled icon opacity differs at "+item.key);
            frames.Insert(item.key+L":root",frame(item.root));
            frames.Insert(item.key+L":header",frame(header));
            frames.Insert(item.key+L":label",frame(header.Children().GetAt(0).as<FrameworkElement>()));
            frames.Insert(item.key+(str(spec,L"kind")==L"slider"?L":value":L":entry"),frame(entry));
            frames.Insert(item.key+L":minus",frame(minus));frames.Insert(item.key+L":plus",frame(plus));
            if(str(spec,L"kind")==L"slider")frames.Insert(item.key+L":track",frame(findElement(item.root,item.key+L"-slider")));
        }
        std::wstring current(frames.Stringify());
        stable=current==previous?stable+1:0;previous=std::move(current);
        if(stable<3)return;
        timer.Stop();
        auto report=J::Parse(source.Stringify());
        report.Insert(L"frames",frames);report.Insert(L"scale",N(surface.XamlRoot().RasterizationScale()));
        report.Insert(L"process_id",N(GetCurrentProcessId()));
        report.Insert(L"native_value_control",S(L"Production field with native TextBox and formatted readout"));
        std::ofstream file(output);file<<to_string(report.Stringify());
        if(!file)throw hresult_error(E_FAIL,L"Cannot write numeric fixture geometry");
    }
    void init(std::filesystem::path const& path){
        std::ifstream input(path);
        std::string json((std::istreambuf_iterator<char>(input)),std::istreambuf_iterator<char>());
        source=J::Parse(to_hstring(json));
        auto name=str(source,L"name");
        if(num(source,L"schema")!=1||array(source,L"rows").Size()!=10||array(source,L"column_widths").Size()!=3||
            (name!=L"windows-light"&&name!=L"windows-dark"))
            throw hresult_invalid_argument(L"Expected the synthetic Windows numeric fixture");
        output=path.parent_path()/(L"native-"+std::wstring(name)+L".json");
        data->catalog=object(source,L"catalog");data->state=O({{L"theme",S(str(source,L"theme"))},{L"palette",object(source,L"palette")}});
        surface.Width(num(source,L"width"));surface.Height(num(source,L"height"));
        surface.HorizontalAlignment(HorizontalAlignment::Left);surface.VerticalAlignment(VerticalAlignment::Top);
        surface.Background(data->brush(L"panel"));
        surface.RequestedTheme(str(source,L"theme")==L"light"?ElementTheme::Light:ElementTheme::Dark);
        AutomationProperties::SetAutomationId(surface,L"number-review-surface");
        AutomationProperties::SetName(surface,L"Numeric control review");
        double x=6;int column=0;
        for(auto width:array(source,L"column_widths")){
            int rowIndex=0;
            for(auto rowValue:array(source,L"rows")){
                auto row=rowValue.GetObject();auto value=std::make_shared<double>(num(row,L"value"));
                auto key=to_hstring(column)+L"-"+to_hstring(rowIndex);
                auto root=number(data,str(row,L"label"),object(row,L"control"),[value]{return *value;},
                    [value,weak=weak_from_this()](double next){
                        *value=next;if(auto self=weak.lock()){
                            self->data->updating=true;for(auto const& update:self->bindings)update();self->data->updating=false;
                        }
                    },bindings,nullptr,false,key);
                ContentControl gate;gate.Content(root);gate.Width(width.GetNumber());
                gate.HorizontalContentAlignment(HorizontalAlignment::Stretch);gate.VerticalContentAlignment(VerticalAlignment::Top);
                gate.IsEnabled(flag(row,L"enabled"));Canvas::SetLeft(gate,x);Canvas::SetTop(gate,6+rowIndex*64);
                surface.Children().Append(gate);items.push_back({key,root,row});rowIndex++;
            }
            x+=width.GetNumber()+8;column++;
        }
        data->updating=true;for(auto const& update:bindings)update();data->updating=false;
        viewport.Content(surface);viewport.IsTabStop(true);
        viewport.HorizontalContentAlignment(HorizontalAlignment::Left);viewport.VerticalContentAlignment(VerticalAlignment::Top);
        window.Title(L"Capy Canvas numeric control review");window.Content(viewport);
        surface.Loaded([weak=weak_from_this()](auto&&,auto&&){
            if(auto self=weak.lock()){
                auto scale=self->surface.XamlRoot().RasterizationScale();
                self->window.AppWindow().ResizeClient({int32_t(std::lround(self->surface.Width()*scale)),int32_t(std::lround(self->surface.Height()*scale))});
                self->viewport.Focus(FocusState::Programmatic);
                self->timer=self->surface.DispatcherQueue().CreateTimer();
                self->timer.Interval(std::chrono::milliseconds(200));self->timer.IsRepeating(true);
                self->timer.Tick([weak](auto&&,auto&&){if(auto owner=weak.lock())owner->publish();});
                self->timer.Start();
            }
        });
        window.AppWindow().Move({40,40});window.Activate();
    }
};
struct App : ApplicationT<App,Markup::IXamlMetadataProvider> {
    Microsoft::UI::Xaml::XamlTypeInfo::XamlControlsXamlMetaDataProvider metadata;
    std::shared_ptr<Fixture> fixture;
    App(){
        UnhandledException([this](auto&&,UnhandledExceptionEventArgs const& args){
            resultCode=1;args.Handled(true);
            if(fixture){std::ofstream error(fixture->output.parent_path()/L"numeric-fixture-error.log");error<<to_string(args.Message());}
            Exit();
        });
    }
    Markup::IXamlType GetXamlType(Windows::UI::Xaml::Interop::TypeName const& type){return metadata.GetXamlType(type);}
    Markup::IXamlType GetXamlType(hstring const& name){return metadata.GetXamlType(name);}
    com_array<Markup::XmlnsDefinition> GetXmlnsDefinitions(){return metadata.GetXmlnsDefinitions();}
    void OnLaunched(LaunchActivatedEventArgs const&){
        Resources().MergedDictionaries().Append(XamlControlsResources());
        wchar_t path[32768];auto length=GetEnvironmentVariableW(L"CAPY_NUMBER_FIXTURE",path,32768);
        if(!length||length>=32768||!std::filesystem::path(path).is_absolute())throw hresult_invalid_argument(L"CAPY_NUMBER_FIXTURE must be an absolute fixture path");
        fixture=std::make_shared<Fixture>();fixture->init(path);
    }
};
}
int WINAPI wWinMain(HINSTANCE,HINSTANCE,PWSTR,int){
    init_apartment(apartment_type::single_threaded);
    Application::Start([](auto&&){make<App>();});
    return resultCode;
}
