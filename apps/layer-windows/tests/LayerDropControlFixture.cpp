#include "pch.h"
#include "LayersView.h"
#include "NativeHandles.h"
#include <chrono>
#include <condition_variable>
#include <deque>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <thread>

using namespace CapyUi;
using namespace CapyLayers;
namespace {
int resultCode=0;

struct Fixture : std::enable_shared_from_this<Fixture> {
    struct Work {
        std::string json;
        PreviewReply reply;
        bool query=false;
        std::function<void()> ready;
        bool publication=false;
    };
    struct Held {
        PreviewReply reply;
        PreviewPacket packet;
    };
    std::mutex mutex;
    std::condition_variable wake;
    std::deque<Work> work;
    std::optional<Held> held;
    bool stopping=false,armed=false,released=false;
    bool closeReady=false;
    std::filesystem::path directory;
    Microsoft::UI::Dispatching::DispatcherQueue dispatcher{Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread()};
    Window window{nullptr};
    Grid root;
    Button arm,release,undo,redo;
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    std::shared_ptr<LayersView> layers;
    Bindings bindings;

    void status(hstring value){
        dispatcher.TryEnqueue([weak=weak_from_this(),value]{
            if(auto self=weak.lock();self&&!self->isStopping())AutomationProperties::SetItemStatus(self->arm,value);
        });
    }
    void fail(std::string const& message){
        std::ofstream output(directory/L"failure.txt");output<<message;
    }
    bool isStopping(){std::lock_guard lock(mutex);return stopping;}
    void writeSnapshot(std::string const& bytes){
        auto path=directory/(L"ui-state-"+std::to_wstring(GetCurrentProcessId())+L"-1.json");
        auto temporary=path;temporary+=L".tmp";
        {
            std::ofstream output(temporary,std::ios::binary);
            output<<"{\"process_id\":"<<GetCurrentProcessId()<<",\"window_id\":1,\"model\":"<<bytes<<"}";
            if(!output)throw std::runtime_error("Could not write layer fixture snapshot");
        }
        auto deadline=std::chrono::steady_clock::now()+std::chrono::seconds(2);
        for(;;){
            if(MoveFileExW(temporary.c_str(),path.c_str(),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH))break;
            auto error=GetLastError();
            if((error!=ERROR_SHARING_VIOLATION&&error!=ERROR_LOCK_VIOLATION&&error!=ERROR_ACCESS_DENIED)||std::chrono::steady_clock::now()>=deadline)
                throw std::runtime_error("Could not publish layer fixture snapshot: Win32 "+std::to_string(error));
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }
    void publish(std::string bytes){
        std::lock_guard lock(mutex);
        if(!stopping){work.push_back(Work{std::move(bytes),{},false,{},true});wake.notify_one();}
    }
    void snapshot(CapyHost host,std::function<void()> ready={}){
        CapySnapshotInfo info{};
        NativeString value(capy_snapshot(host,&info),capy_string_free);
        if(!value||!info.full){if(ready)dispatcher.TryEnqueue(std::move(ready));return;}
        dispatcher.TryEnqueue([weak=weak_from_this(),bytes=std::move(value.bytes),ready=std::move(ready)]()mutable{
            if(auto self=weak.lock();self&&!self->isStopping()){self->present(bytes);self->publish(std::move(bytes));if(ready)ready();}
        });
    }
    void present(std::string const& bytes){
        auto model=J::Parse(to_hstring(bytes));
        data->updating=true;
        if(!data->adoptLocalization(model)){data->model=model;data->state=object(model,L"state");}
        data->refreshPalette();
        root.RequestedTheme(data->theme()==L"light"?ElementTheme::Light:ElementTheme::Dark);
        if(!layers){
            auto panel=LayersPanel(data,bindings,nullptr,nullptr,&layers);
            Grid::SetRow(panel,1);root.Children().Append(panel);
            layers->root.AddHandler(UIElement::PointerReleasedEvent(),box_value(PointerEventHandler(
                [weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->markReleased();})),true);
        }
        for(auto const& refresh:bindings)refresh();
        data->updating=false;
    }
    void markReleased(){
        std::lock_guard lock(mutex);
        if(armed){released=true;status(L"release-seen");}
    }
    void armDrop(){
        std::lock_guard lock(mutex);
        if(held)throw hresult_error(E_FAIL,L"Previous layer drop is still held");
        armed=true;released=false;status(L"armed");
    }
    void releaseDrop(){
        std::optional<Held> pending;
        {
            std::lock_guard lock(mutex);
            if(!held)throw hresult_error(E_FAIL,L"No final layer drop is held");
            pending=std::move(held);held.reset();armed=false;released=false;
        }
        status(L"released");
        pending->reply(std::move(pending->packet));
    }
    bool query(CanvasQueryKind kind,std::string json,PreviewReply reply){
        if(kind!=CanvasQueryKind::Workspace)return false;
        std::lock_guard lock(mutex);
        if(stopping)return false;
        work.push_back(Work{std::move(json),std::move(reply),true});wake.notify_one();return true;
    }
    void action(std::string json,std::function<void()> ready={}){
        std::lock_guard lock(mutex);
        if(!stopping){work.push_back(Work{std::move(json),{},false,std::move(ready)});wake.notify_one();}
    }
    void stop(){std::lock_guard lock(mutex);stopping=true;wake.notify_one();}
    void run(){
        CapyHost host=0;
        bool failed=false;
        try{
            init_apartment(apartment_type::multi_threaded);
            NativeLaunch launch(capy_launch("[\"en\"]",nullptr),capy_launch_free);
            if(!launch)throw std::runtime_error(NativeString(capy_error()).get());
            NativeString bootstrap(capy_launch_view(launch.get()));
            NativeLocalization localization(capy_launch_localization(launch.get()),capy_localization_free);
            if(!bootstrap||!localization)throw std::runtime_error("Layer fixture localization is unavailable");
            host=capy_create(512,768,1,launch.get());
            if(!host)throw std::runtime_error(NativeString(capy_error()).get());
            launch.reset();
            NativeString catalog(capy_query(host,R"({"type":"catalog"})"),capy_string_free);
            if(!catalog)throw std::runtime_error(NativeString(capy_error()).get());
            dispatcher.TryEnqueue([weak=weak_from_this(),catalog=std::move(catalog.bytes),bootstrap=std::move(bootstrap.bytes),localization]{
                if(auto self=weak.lock();self&&!self->isStopping()){
                    self->data->localization=localization;
                    self->data->catalog=J::Parse(to_hstring(catalog));
                    self->data->catalog.Insert(L"bootstrap",J::Parse(to_hstring(bootstrap)));
                }
            });
            if(capy_start_services(host,nullptr,nullptr)<0)throw std::runtime_error(NativeString(capy_error()).get());
            bool ready=false;
            for(int attempt=0;attempt<300&&!ready;attempt++){
                if(capy_poll_services(host)<0)throw std::runtime_error(NativeString(capy_error()).get());
                CapySnapshotInfo info{};
                NativeString value(capy_snapshot(host,&info),capy_string_free);
                if(value){
                    auto model=J::Parse(to_hstring(value.bytes));
                    auto workspace=object(model,L"windows_workspace");
                    ready=flag(workspace,L"ready")&&!flag(workspace,L"busy");
                }
                if(!ready)std::this_thread::sleep_for(std::chrono::milliseconds(50));
            }
            if(!ready)throw std::runtime_error("Layer fixture workspace did not become ready");
            if(capy_action(host,R"({"type":"layer","action":{"op":"new","group":false,"clipped":false}})")!=0)
                throw std::runtime_error(NativeString(capy_error()).get());
            snapshot(host);
            for(;;){
                Work next;
                {
                    std::unique_lock lock(mutex);
                    wake.wait_for(lock,std::chrono::milliseconds(100),[&]{return stopping||!work.empty();});
                    if(stopping)break;
                    if(work.empty()){
                        lock.unlock();
                        if(capy_poll_services(host)<0)throw std::runtime_error(NativeString(capy_error()).get());
                        snapshot(host);
                        continue;
                    }
                    next=std::move(work.front());work.pop_front();
                }
                if(next.publication){writeSnapshot(next.json);continue;}
                if(!next.query){
                    if(capy_action(host,next.json.c_str())!=0)throw std::runtime_error(NativeString(capy_error()).get());
                    snapshot(host,std::move(next.ready));
                    continue;
                }
                PreviewPacket packet(capy_workspace_query(host,next.json.c_str()),capy_preview_free);
                if(!packet)throw std::runtime_error(NativeString(capy_error()).get());
                auto request=J::Parse(to_hstring(next.json));
                bool hold=false;
                {
                    std::lock_guard lock(mutex);
                    hold=armed&&released&&str(request,L"type")==L"layer_drop";
                    if(hold){held=Held{std::move(next.reply),std::move(packet)};released=false;}
                }
                if(hold)status(L"held");else next.reply(std::move(packet));
            }
        }catch(std::exception const& error){failed=true;fail(error.what());}
        stop();
        {
            std::lock_guard lock(mutex);
            held.reset();work.clear();
        }
        if(host){capy_finish_services(host);capy_destroy(host);}
        dispatcher.TryEnqueue([weak=weak_from_this(),failed]{
            if(auto self=weak.lock()){
                if(failed)resultCode=1;
                self->bindings.clear();
                self->layers.reset();
                self->closeReady=true;
                self->window.Close();
            }
        });
    }
    void open(std::filesystem::path path){
        directory=std::move(path);
        data->query=[weak=weak_from_this()](CanvasQueryKind kind,std::string json,PreviewReply reply){
            if(auto self=weak.lock())return self->query(kind,std::move(json),std::move(reply));return false;
        };
        data->send=[weak=weak_from_this()](std::string json){if(auto self=weak.lock())self->action(std::move(json));};
        data->commit=[weak=weak_from_this()](std::string json,bool,std::function<void()> ready){if(auto self=weak.lock())self->action(std::move(json),std::move(ready));};
        RowDefinition controls;controls.Height({1,GridUnitType::Auto});root.RowDefinitions().Append(controls);
        RowDefinition rows;rows.Height({1,GridUnitType::Star});root.RowDefinitions().Append(rows);
        StackPanel buttons;buttons.Orientation(Orientation::Horizontal);
        arm.Content(box_value(L"Arm final drop"));release.Content(box_value(L"Release final drop"));
        undo.Content(box_value(L"Undo"));redo.Content(box_value(L"Redo"));
        AutomationProperties::SetAutomationId(arm,L"layer-drop-arm");
        AutomationProperties::SetAutomationId(release,L"layer-drop-release");
        AutomationProperties::SetName(undo,L"Undo");AutomationProperties::SetName(redo,L"Redo");
        arm.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->armDrop();});
        release.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->releaseDrop();});
        undo.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"undo")}}));});
        redo.Click([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->data->dispatch(O({{L"type",S(L"invoke")},{L"command",S(L"redo")}}));});
        buttons.Children().Append(arm);buttons.Children().Append(release);buttons.Children().Append(undo);buttons.Children().Append(redo);root.Children().Append(buttons);
        AutomationProperties::SetItemStatus(arm,L"ready");
        window=Window();window.Title(L"Capy Canvas layer drop fixture");window.Content(root);
        window.AppWindow().Closing([weak=weak_from_this()](auto&&,auto const& args){
            if(auto self=weak.lock();self&&!self->closeReady){args.Cancel(true);self->stop();}
        });
        window.Closed([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->stop();});
        window.Activate();window.AppWindow().ResizeClient({460,700});
        std::thread([this]{run();}).detach();
    }
};

struct App : ApplicationT<App,Markup::IXamlMetadataProvider> {
    XamlTypeInfo::XamlControlsXamlMetaDataProvider metadata;
    std::shared_ptr<Fixture> fixture;
    Markup::IXamlType GetXamlType(Windows::UI::Xaml::Interop::TypeName const& type){return metadata.GetXamlType(type);}
    Markup::IXamlType GetXamlType(hstring const& name){return metadata.GetXamlType(name);}
    com_array<Markup::XmlnsDefinition> GetXmlnsDefinitions(){return metadata.GetXmlnsDefinitions();}
    void OnLaunched(LaunchActivatedEventArgs const&){
        Resources().MergedDictionaries().Append(XamlControlsResources());
        Resources().MergedDictionaries().Append(CapyUi::paletteResources());
        wchar_t path[32768]{};auto length=GetEnvironmentVariableW(L"CAPY_LAYER_DROP_FIXTURE",path,32768);
        if(!length||length>=32768||!std::filesystem::path(path).is_absolute())throw hresult_invalid_argument(L"CAPY_LAYER_DROP_FIXTURE must name an absolute directory");
        fixture=std::make_shared<Fixture>();fixture->open(path);
    }
};
}
int WINAPI wWinMain(HINSTANCE,HINSTANCE,PWSTR,int){
    init_apartment(apartment_type::single_threaded);
    Application::Start([](auto&&){make<App>();});
    return resultCode;
}
