#pragma once
#include "NativeInputObserver.h"
#include "SmokeCommand.h"
#include <functional>
#include <string_view>
#include <winrt/Windows.Graphics.h>
#include <microsoft.ui.xaml.window.h>

namespace CapyUi {
inline UIElement NativeInputHitRoot(XamlRoot const& xaml){
    auto scope=xaml.Content().as<UIElement>();
    while(auto parent=VisualTreeHelper::GetParent(scope).try_as<UIElement>()){
        if(parent.XamlRoot()!=xaml)throw hresult_invalid_argument(L"Input query ancestor belongs to another XamlRoot.");
        scope=parent;
    }
    return scope;
}
class NativeInputQuery:public std::enable_shared_from_this<NativeInputQuery>{
public:
    using Semantic=NativeInputObserver::Semantic;
    static constexpr wchar_t ArgumentsId[]=L"test-input-query";
    static constexpr wchar_t CommandId[]=L"test-input-query-invoke";
private:
    NativeInputQuery()=default;
    inline static thread_local std::map<HWND,std::weak_ptr<NativeInputQuery>> instances;
    struct Root {weak_ref<UIElement> element;Windows::Foundation::IInspectable handler;};
    weak_ref<FrameworkElement> root;
    weak_ref<TextBox> arguments;
    HWND window=nullptr;
    uint64_t windowId=0;
    DWORD thread=GetCurrentThreadId();
    bool closed=false;
    std::function<bool()> isolated;
    Semantic semantic;
    std::vector<Root> roots;
    std::set<uintptr_t> bindings;
    std::shared_ptr<NativeInputJournal> journal=std::make_shared<NativeInputJournal>();
    std::shared_ptr<std::atomic<bool>> authorized=std::make_shared<std::atomic<bool>>(false);
    void onThread()const{if(thread!=GetCurrentThreadId())throw hresult_wrong_thread();}
    static A ids(std::vector<std::wstring> const& path){A values;for(auto const& id:path)values.Append(S(hstring(id)));return values;}
    static V optionalRecord(std::optional<uint64_t> value){return value?V(N(double(*value))):JsonValue::CreateNullValue();}
    static J identity(DependencyObject const& node){return O({{L"path",ids(NativeInputObserver::Path(node))},{L"type",S(node?get_class_name(node):hstring{})}});}
    bool recordingAllowed(){
        if(closed||!isolated()||!GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)){authorized->store(false);return false;}
        return authorized->load();
    }
    void observe(UIElement const& element){
        if(!element||!element.XamlRoot())return;
        std::erase_if(roots,[](auto const& entry){return !entry.element.get();});
        for(auto const& entry:roots)if(entry.element.get()==element)return;
        auto weak=weak_from_this();
        bindings.insert(NativeInputObserver::Bind(element.XamlRoot(),journal,[weak]{auto self=weak.lock();return self&&self->recordingAllowed();},
            [weak](DependencyObject const& source,Windows::Foundation::Point point){auto self=weak.lock();return self&&self->semantic?self->semantic(source,point):NativeInputSemantic{};}));
        PointerEventHandler handler([](auto&& sender,PointerRoutedEventArgs const& e){NativeInputObserver::Down(sender.template as<UIElement>(),e,NativeInputAdmission::Unknown);});
        element.AddHandler(UIElement::PointerPressedEvent(),box_value(handler),true);
        roots.push_back({make_weak(element),box_value(handler)});
    }
    void observeRoots(FrameworkElement const& view){
        observe(view.XamlRoot().Content().as<UIElement>());
        for(auto const& popup:VisualTreeHelper::GetOpenPopupsForXamlRoot(view.XamlRoot()))if(popup.IsOpen())observe(popup.Child());
    }
    J downs(std::shared_ptr<NativeInputJournal::Snapshot const> const& snapshot)const{
        A rows;
        for(auto const& row:snapshot->downs){
            A point;for(auto coordinate:row.screen)point.Append(N(coordinate));
            auto route=row.route==NativeInputRoute::Independent?L"independent":row.route==NativeInputRoute::XamlCanvas?L"xaml_canvas":L"routed";
            V admitted=row.admitted==NativeInputAdmission::Unknown?JsonValue::CreateNullValue():V(B(row.admitted==NativeInputAdmission::Accepted));
            rows.Append(O({{L"serial",N(double(row.serial))},{L"device",S(hstring(row.device))},{L"pointer_id",N(double(row.pointer))},{L"time",N(double(row.time))},
                {L"screen",point},{L"path",ids(row.path)},{L"route",S(route)},{L"admitted",admitted},
                {L"record",optionalRecord(row.record)},{L"part",row.part.empty()?JsonValue::CreateNullValue():V(S(hstring(row.part)))}}));
        }
        return O({{L"down_serial",N(double(snapshot->serial))},{L"dropped",N(double(snapshot->dropped))},{L"downs",rows}});
    }
    void execute(TextBox const& target){
        J request;
        try{
            if(target.Text().size()>4096)throw hresult_invalid_argument(L"Input query is too large.");
            request=J::Parse(target.Text());
            AutomationProperties::SetItemStatus(target,Query(request).Stringify());
        }catch(hresult_error const& error){
            auto response=O({{L"request",request.GetNamedValue(L"request",JsonValue::CreateNullValue())},
                {L"process_id",N(GetCurrentProcessId())},{L"window_id",N(double(windowId))},{L"hwnd",N(double(uintptr_t(window)))},
                {L"error",S(error.message())},{L"hresult",N(uint32_t(error.code().value))}});
            AutomationProperties::SetItemStatus(target,response.Stringify());
        }
    }
    void init(Panel const& owner){
        auto weak=weak_from_this();
        auto [target,command]=smokeCommand(owner,ArgumentsId,L"Test input state",[weak](TextBox const& value){if(auto self=weak.lock())self->execute(value);});
        arguments=make_weak(target);
        AutomationProperties::SetAutomationId(command,CommandId);
        if(owner.XamlRoot())observeRoots(owner);
    }
public:
    static std::shared_ptr<NativeInputQuery> Create(Panel const& owner,HWND handle,uint64_t id,std::function<bool()> isIsolated,Semantic describe={}){
        if(!GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)||!owner||!isIsolated)return {};
        auto safe=bool(owner.try_as<Grid>())||bool(owner.try_as<Canvas>());
        if(auto stack=owner.try_as<StackPanel>())safe=stack.Spacing()==0;
        if(!safe)return {};
        std::erase_if(instances,[](auto const& entry){return entry.second.expired();});
        if(auto previous=Find(handle)){
            if(previous->root.get()==owner&&previous->windowId==id)return previous;
            previous->Close();
        }
        auto self=std::shared_ptr<NativeInputQuery>(new NativeInputQuery());self->root=make_weak(owner.as<FrameworkElement>());self->window=handle;self->windowId=id;
        self->isolated=std::move(isIsolated);self->semantic=std::move(describe);self->init(owner);instances[handle]=self;return self;
    }
    static std::shared_ptr<NativeInputQuery> Find(HWND handle){auto found=instances.find(handle);return found==instances.end()?nullptr:found->second.lock();}
    std::shared_ptr<NativeInputJournal> Journal()const{return journal;}
    std::function<void(NativeCanvasDown)> IndependentReceiver(UIElement const& target,NativeInputRoute route=NativeInputRoute::Independent){
        onThread();return NativeInputReceiver(journal,authorized,NativeInputObserver::Path(target),route);
    }
    J Query(J const& request){
        onThread();if(closed||!isolated()||!GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)){
            authorized->store(false);throw hresult_access_denied(L"Input query requires an isolated trace window.");
        }
        auto nonce=request.GetNamedValue(L"request");if(nonce.ValueType()!=JsonValueType::String||nonce.GetString().empty())throw hresult_invalid_argument(L"Input query needs a request nonce.");
        auto kind=request.GetNamedString(L"kind");if(kind!=L"pointer"&&kind!=L"keyboard"&&kind!=L"receipts")throw hresult_invalid_argument(L"Unknown input query kind.");
        auto view=root.get();if(!IsWindow(window)||!view||!view.XamlRoot())throw hresult_error(E_ABORT,L"Input query root is not loaded.");
        observeRoots(view);
        auto foreground=GetForegroundWindow(),active=GetActiveWindow(),owner=GetAncestor(window,GA_ROOTOWNER);
        auto focused=FocusManager::GetFocusedElement(view.XamlRoot()).try_as<DependencyObject>();auto focus=identity(focused);
        auto pointer=identity(nullptr);pointer.Insert(L"canvas",B(false));pointer.Insert(L"record",JsonValue::CreateNullValue());pointer.Insert(L"part",JsonValue::CreateNullValue());
        pointer.Insert(L"enabled",B(false));pointer.Insert(L"loaded",B(false));
        HWND pointWindow=nullptr;DWORD pointProcess=0;
        if(kind==L"pointer"){
            auto at=request.GetNamedArray(L"screen");if(at.Size()!=2)throw hresult_invalid_argument(L"Input query needs one screen point.");
            auto x=at.GetNumberAt(0),y=at.GetNumberAt(1);
            if(!std::isfinite(x)||!std::isfinite(y)||x!=std::trunc(x)||y!=std::trunc(y)||x<INT32_MIN||x>INT32_MAX||y<INT32_MIN||y>INT32_MAX)throw hresult_invalid_argument(L"Input query screen point is outside native coordinates.");
            Windows::Graphics::PointInt32 point{int32_t(x),int32_t(y)};
            pointWindow=WindowFromPoint({point.X,point.Y});GetWindowThreadProcessId(pointWindow,&pointProcess);
            if(pointProcess==GetCurrentProcessId()&&GetAncestor(pointWindow,GA_ROOTOWNER)==owner){
                auto coordinates=view.XamlRoot().CoordinateConverter();if(!coordinates)throw hresult_error(E_ABORT,L"Input query coordinate conversion is unavailable.");
                auto local=coordinates.ConvertScreenToLocal(point);auto hits=VisualTreeHelper::FindElementsInHostCoordinates(local,NativeInputHitRoot(view.XamlRoot()),false).First();
                if(hits.HasCurrent()){
                    auto hit=hits.Current();pointer=identity(hit);bool enabled=true;
                    for(auto node=hit.as<DependencyObject>();node;node=VisualTreeHelper::GetParent(node))if(auto control=node.try_as<Control>())enabled&=control.IsEnabled();
                    auto target=semantic?semantic(hit,local):NativeInputSemantic{};
                    pointer.Insert(L"canvas",B(target.canvas));pointer.Insert(L"record",optionalRecord(target.record));pointer.Insert(L"part",target.part.empty()?JsonValue::CreateNullValue():V(S(hstring(target.part))));
                    pointer.Insert(L"enabled",B(enabled));pointer.Insert(L"loaded",B(hit.try_as<FrameworkElement>()&&hit.as<FrameworkElement>().IsLoaded()));
                }
            }
        }
        auto snapshot=journal->Read();auto response=downs(snapshot);
        response.Insert(L"request",nonce);response.Insert(L"process_id",N(GetCurrentProcessId()));response.Insert(L"window_id",N(double(windowId)));response.Insert(L"hwnd",N(double(uintptr_t(window))));
        response.Insert(L"active",B(!IsIconic(window)&&GetAncestor(active,GA_ROOTOWNER)==owner));response.Insert(L"foreground",N(double(uintptr_t(foreground))));
        response.Insert(L"point_hwnd",N(double(uintptr_t(pointWindow))));response.Insert(L"point_process",N(pointProcess));response.Insert(L"pointer",pointer);response.Insert(L"focus",focus);
        response.Insert(L"progress",O({{L"pointer",pointer},{L"focus",focus},{L"active",response.GetNamedValue(L"active")},{L"foreground",response.GetNamedValue(L"foreground")},
            {L"downs",response.GetNamedValue(L"downs")}}));
        authorized->store(true);return response;
    }
    void Close(){
        onThread();authorized->store(false);if(closed)return;closed=true;journal->Close();
        for(auto const& entry:roots)if(auto element=entry.element.get())element.RemoveHandler(UIElement::PointerPressedEvent(),entry.handler);
        roots.clear();for(auto key:bindings)NativeInputObserver::Unbind(key,journal);bindings.clear();auto found=instances.find(window);
        if(found!=instances.end()&&found->second.lock().get()==this)instances.erase(found);
    }
};
inline bool NativeInputPrivateDesktop(){
    wchar_t expected[256]{},actual[256]{};DWORD bytes=0;
    auto length=GetEnvironmentVariableW(L"CAPY_PRIVATE_DESKTOP",expected,256);
    return length&&length<256&&GetUserObjectInformationW(GetThreadDesktop(GetCurrentThreadId()),UOI_NAME,actual,sizeof actual,&bytes)&&std::wstring_view(expected)==actual;
}
inline std::shared_ptr<NativeInputQuery> InstallNativeInputQuery(Window const& window,uint64_t id,std::function<bool()> isolated,NativeInputQuery::Semantic semantic={}){
    if(!GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0))return {};
    auto owner=window.Content().try_as<Panel>();if(!owner)return {};
    HWND hwnd=nullptr;check_hresult(window.as<IWindowNative>()->get_WindowHandle(&hwnd));
    auto query=NativeInputQuery::Create(owner,hwnd,id,std::move(isolated),std::move(semantic));
    if(query)window.Closed([query](auto&&,auto&&){query->Close();});
    return query;
}
}
