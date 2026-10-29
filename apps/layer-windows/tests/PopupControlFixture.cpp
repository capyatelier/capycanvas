#include "pch.h"
#include "ControlContext.h"
#include "NativeMenus.h"
#include "ChromeVisibility.h"
#include "WorkspaceManagerView.h"
#include <msctf.h>
#pragma comment(lib,"uuid.lib")
#include <winrt/Microsoft.UI.Xaml.Automation.Peers.h>
#include <winrt/Microsoft.UI.Xaml.Automation.Provider.h>
#include <microsoft.ui.xaml.window.h>
#include <fstream>
using namespace CapyUi;
namespace {
struct TextServicesLibrary{
    using Current=HRESULT(WINAPI*)(ITfThreadMgr**);
    HMODULE module=nullptr;Current current=nullptr;DWORD loadError=0,lookupError=0,worker=0;std::wstring path;
    TextServicesLibrary(){
        std::jthread startup([this]{
            worker=GetCurrentThreadId();SetLastError(ERROR_SUCCESS);
            module=LoadLibraryExW(L"msctf.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32);
            if(!module){loadError=GetLastError();return;}
            wchar_t name[32768]{};auto length=GetModuleFileNameW(module,name,32768);if(length&&length<32768)path.assign(name,length);
            SetLastError(ERROR_SUCCESS);current=reinterpret_cast<Current>(GetProcAddress(module,"TF_GetThreadMgr"));
            if(!current)lookupError=GetLastError();
        });
        startup.join();
    }
    ~TextServicesLibrary(){if(module)FreeLibrary(module);}
};
struct CompositionEdit:implements<CompositionEdit,ITfEditSession>{
    std::function<void(TfEditCookie)> run;
    explicit CompositionEdit(std::function<void(TfEditCookie)> value):run(std::move(value)){}
    HRESULT __stdcall DoEditSession(TfEditCookie cookie) noexcept override{
        try{run(cookie);return S_OK;}catch(...){return to_hresult();}
    }
};
struct CompositionSink:implements<CompositionSink,ITfCompositionSink>{
    using Trace=std::function<void(hstring,HRESULT,uintptr_t)>;
    Trace trace;bool terminated=false;
    explicit CompositionSink(Trace observer):trace(std::move(observer)){}
    HRESULT __stdcall OnCompositionTerminated(TfEditCookie cookie,ITfComposition* value) noexcept override{
        terminated=true;
        try{trace(L"ITfCompositionSink::OnCompositionTerminated",S_OK,reinterpret_cast<uintptr_t>(value));trace(L"CompositionTerminationCookie",S_OK,uintptr_t(cookie));return S_OK;}catch(...){return to_hresult();}
    }
};
struct ScopedComposition{
    using Trace=CompositionSink::Trace;
    TextServicesLibrary const& library;Trace trace;
    com_ptr<ITfThreadMgr> manager;com_ptr<ITfContext> context;com_ptr<ITfComposition> composition;com_ptr<CompositionSink> sink;
    TfClientId client=TF_CLIENTID_NULL;bool activated=false;std::map<std::wstring,std::pair<HRESULT,uintptr_t>> observations;unsigned probes=0;
    ScopedComposition(TextServicesLibrary const& services,Trace observer):library(services),trace(std::move(observer)),sink(make_self<CompositionSink>(trace)){
        auto loaded=library.module?S_OK:HRESULT_FROM_WIN32(library.loadError?library.loadError:ERROR_MOD_NOT_FOUND);
        checked(L"LoadLibraryExW(msctf,system32)",loaded,reinterpret_cast<uintptr_t>(library.module));
        auto found=library.current?S_OK:HRESULT_FROM_WIN32(library.lookupError?library.lookupError:ERROR_PROC_NOT_FOUND);
        checked(L"GetProcAddress(TF_GetThreadMgr)",found,reinterpret_cast<uintptr_t>(library.current));
    }
    ScopedComposition(ScopedComposition const&)=delete;
    ScopedComposition& operator=(ScopedComposition const&)=delete;
    void record(wchar_t const* api,HRESULT result,uintptr_t pointer=0){
        auto value=std::pair{result,pointer};auto found=observations.find(api);
        if(found==observations.end()||found->second!=value){observations[api]=value;trace(api,result,pointer);}
    }
    void checked(wchar_t const* api,HRESULT result,uintptr_t pointer=0){record(api,result,pointer);if(FAILED(result))throw hresult_error(result,api);}
    bool focused(TextBox const& entry,XamlRoot const& root){
        ++probes;auto focus=FocusManager::GetFocusedElement(root);
        record(L"XamlFocusedTextBox",focus==entry?S_OK:S_FALSE,reinterpret_cast<uintptr_t>(get_abi(focus)));
        if(focus!=entry)return false;
        if(!manager){
            auto result=library.current(manager.put());checked(L"TF_GetThreadMgr",result,reinterpret_cast<uintptr_t>(manager.get()));
            if(!manager){result=CoCreateInstance(CLSID_TF_ThreadMgr,nullptr,CLSCTX_INPROC_SERVER,__uuidof(ITfThreadMgr),manager.put_void());checked(L"CoCreateInstance(CLSID_TF_ThreadMgr)",result,reinterpret_cast<uintptr_t>(manager.get()));}
            if(!manager)return false;
            result=manager->Activate(&client);activated=SUCCEEDED(result);checked(L"ITfThreadMgr::Activate",result,uintptr_t(client));
        }
        BOOL threadFocus=FALSE;auto threadResult=manager->IsThreadFocus(&threadFocus);checked(L"ITfThreadMgr::IsThreadFocus",threadResult,uintptr_t(threadFocus));
        com_ptr<ITfDocumentMgr> document;auto result=manager->GetFocus(document.put());
        checked(L"ITfThreadMgr::GetFocus",result,reinterpret_cast<uintptr_t>(document.get()));
        if(result==S_FALSE||!document)return false;
        com_ptr<ITfContext> candidate;result=document->GetTop(candidate.put());checked(L"ITfDocumentMgr::GetTop",result,reinterpret_cast<uintptr_t>(candidate.get()));
        if(result==S_FALSE||!candidate||!threadFocus)return false;
        context=std::move(candidate);return true;
    }
    void edit(wchar_t const* phase,std::function<void(TfEditCookie)> run){
        auto session=make_self<CompositionEdit>(std::move(run));HRESULT result=E_FAIL;
        auto request=context->RequestEditSession(client,session.get(),TF_ES_SYNC|TF_ES_READWRITE,&result);
        checked((hstring(L"ITfContext::RequestEditSession/")+phase).c_str(),request,reinterpret_cast<uintptr_t>(context.get()));
        checked((hstring(L"ITfEditSession::DoEditSession/")+phase).c_str(),result,reinterpret_cast<uintptr_t>(context.get()));
    }
    void begin(){
        edit(L"begin",[this](TfEditCookie cookie){
            TF_SELECTION selection{};ULONG count=0;auto result=context->GetSelection(cookie,TF_DEFAULT_SELECTION,1,&selection,&count);
            com_ptr<ITfRange> range;range.attach(selection.range);checked(L"ITfContext::GetSelection",result,reinterpret_cast<uintptr_t>(range.get()));
            checked(L"SelectionRange",count==1&&range?S_OK:E_UNEXPECTED,uintptr_t(count));
            com_ptr<ITfContextComposition> compositions;result=context->QueryInterface(__uuidof(ITfContextComposition),compositions.put_void());checked(L"QueryInterface(ITfContextComposition)",result,reinterpret_cast<uintptr_t>(compositions.get()));
            record(L"StartCompositionSink",S_OK,reinterpret_cast<uintptr_t>(static_cast<ITfCompositionSink*>(sink.get())));
            result=compositions->StartComposition(cookie,range.get(),sink.get(),composition.put());
            checked(L"ITfContextComposition::StartComposition",result,reinterpret_cast<uintptr_t>(composition.get()));
            checked(L"StartedComposition",composition?S_OK:E_UNEXPECTED,reinterpret_cast<uintptr_t>(composition.get()));
            checked(L"ITfRange::SetText",range->SetText(cookie,0,L"\u5019\u88dc",2),reinterpret_cast<uintptr_t>(range.get()));
        });
    }
    void end(){if(composition){if(!sink->terminated)edit(L"end",[this](TfEditCookie cookie){if(!sink->terminated)checked(L"ITfComposition::EndComposition",composition->EndComposition(cookie));});composition=nullptr;}}
    void deactivate(){if(activated){activated=false;client=TF_CLIENTID_NULL;checked(L"ITfThreadMgr::Deactivate",manager->Deactivate());}}
    void finish(){end();deactivate();}
    ~ScopedComposition(){try{end();}catch(...){}try{deactivate();}catch(...){}}
};

int resultCode=0;
struct Fixture:std::enable_shared_from_this<Fixture> {
    Window window,other;StackPanel root;Button anchor,untagged;
    std::shared_ptr<WorkspaceData> data=std::make_shared<WorkspaceData>();
    ControlContext context;std::shared_ptr<TextServicesLibrary> textServices;unsigned imeChecks=0;std::shared_ptr<std::vector<J>> imeTrace=std::make_shared<std::vector<J>>();
    bool windowClosed=false;OwnedPopup owner;std::optional<OwnedPopup::Ticket> origin;std::vector<MenuFlyout> popups;HWND hwnd=nullptr;unsigned checks=0,keys=0,presses=0,blurs=0,opened=0,closedCount=0,originPresses=0;
    std::filesystem::path output;hstring phase;
    winrt::apartment_context apartment;
    static void require(bool value,wchar_t const* text){if(!value)throw hresult_error(E_FAIL,text);}
    static HWND handle(Window const& value){HWND result=nullptr;check_hresult(value.as<IWindowNative>()->get_WindowHandle(&result));return result;}
    void privacy(){
        wchar_t expected[256],actual[256];DWORD bytes=0;
        auto length=GetEnvironmentVariableW(L"CAPY_PRIVATE_DESKTOP",expected,256);
        require(length&&length<256,L"Private desktop name required");
        check_bool(GetUserObjectInformationW(GetThreadDesktop(GetCurrentThreadId()),UOI_NAME,actual,sizeof actual,&bytes));
        require(std::wstring(expected)==actual,L"Fixture is not on its named private desktop");
        DWORD pid=0;GetWindowThreadProcessId(GetForegroundWindow(),&pid);
        require(pid==GetCurrentProcessId()&&GetForegroundWindow()==hwnd,L"Fixture does not own foreground input");
    }
    void nativePress(FrameworkElement const& target,bool right=false){
        privacy();auto position=target.TransformToVisual(nullptr).TransformPoint({float(target.ActualWidth()/2),float(target.ActualHeight()/2)});
        bool hit=false;for(auto const& element:VisualTreeHelper::FindElementsInHostCoordinates(position,root))if(element==target)hit=true;
        require(hit,L"Native point does not hit the intended XAML source");
        auto scale=root.XamlRoot().RasterizationScale();POINT point{LONG(position.X*scale),LONG(position.Y*scale)};check_bool(ClientToScreen(hwnd,&point));
        require(GetAncestor(WindowFromPoint(point),GA_ROOT)==hwnd,L"Native press target is outside owned window");
        check_bool(SetCursorPos(point.x,point.y));INPUT input[2]{};input[0].type=input[1].type=INPUT_MOUSE;
        input[0].mi.dwFlags=right?MOUSEEVENTF_RIGHTDOWN:MOUSEEVENTF_LEFTDOWN;input[1].mi.dwFlags=right?MOUSEEVENTF_RIGHTUP:MOUSEEVENTF_LEFTUP;
        require(SendInput(2,input,sizeof(INPUT))==2,L"Native pointer injection failed");
    }
    winrt::Windows::Foundation::IAsyncAction settle(){co_await winrt::resume_after(std::chrono::milliseconds(120));co_await apartment;}
    winrt::Windows::Foundation::IAsyncAction wait(std::function<bool()> ready,wchar_t const* message){
        auto deadline=std::chrono::steady_clock::now()+std::chrono::seconds(5);
        while(!ready()){require(std::chrono::steady_clock::now()<deadline,message);co_await winrt::resume_after(std::chrono::milliseconds(10));co_await apartment;}
    }
    MenuFlyout menu(){MenuFlyout popup;MenuFlyoutItem item;item.Text(L"Fixture action");popup.Items().Append(item);popup.Opened([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())++self->opened;});popup.Closed([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())++self->closedCount;});popups.push_back(popup);return popup;}
    bool visiblePopup(){for(auto const& popup:VisualTreeHelper::GetOpenPopupsForXamlRoot(root.XamlRoot()))if(popup.IsOpen())return true;return false;}
    void pending(){require(owner.Pending()&&!owner.Open()&&data->transients.size()==1,L"Pending popup lacks immediate unique cancellation registration");}
    void empty(){require(!owner.Active()&&!owner.Pending()&&!owner.Open()&&data->transients.empty()&&data->popupCount==0,L"Popup cancellation left retained state or registrations");}
    winrt::Windows::Foundation::IAsyncAction ordinaryOpening(){
        struct Observation{unsigned opening=0,opened=0,closed=0;bool cancelOpening=true;hstring error;};
        auto observed=std::make_shared<Observation>();MenuFlyout ordinary;MenuFlyoutItem item;item.Text(L"Ordinary popup action");ordinary.Items().Append(item);
        TrackPopup(ordinary,data);auto weak=weak_from_this();
        ordinary.Opening([weak,observed](auto&&,auto&&){if(auto self=weak.lock())try{
            ++observed->opening;
            require(self->data->transients.size()==1&&self->data->popupCount==0,L"Ordinary popup is not registered before native Opened");
            if(observed->cancelOpening){
                require(self->data->dismissTransients(),L"Ordinary popup ignored cancellation during native Opening");
                require(!self->data->dismissTransients(),L"Repeated opening cancellation retained a registration");
            }
        }catch(hresult_error const& e){observed->error=e.message();}catch(...){observed->error=L"Ordinary popup Opening callback failed";}});
        ordinary.Opened([observed](auto&&,auto&&){++observed->opened;});
        ordinary.Closed([observed](auto&&,auto&&){++observed->closed;});
        require(data->transients.empty()&&data->popupCount==0,L"Ordinary popup opening inherited another popup lifetime");
        ordinary.ShowAt(anchor);co_await settle();require(observed->error.empty(),observed->error.c_str());
        require(observed->opening==1&&!ordinary.IsOpen()&&!visiblePopup(),L"Canceled ordinary Opening left a visible or late native popup");
        require(!data->dismissTransients(),L"Late ordinary Opened resurrected cancellation registration");empty();
        observed->cancelOpening=false;auto beforeOpened=observed->opened;auto beforeClosed=observed->closed;
        ordinary.ShowAt(anchor);
        co_await wait([this,observed,beforeOpened]{return observed->opened==beforeOpened+1&&data->popupCount==1&&visiblePopup();},L"Same ordinary flyout did not reopen with fresh lifetime");
        require(observed->error.empty(),observed->error.c_str());
        require(observed->opening==2&&data->transients.size()==1,L"Reopened ordinary flyout lacks exactly one fresh registration");
        require(data->dismissTransients(),L"Reopened ordinary flyout ignored cancellation");
        require(!data->dismissTransients(),L"Repeated visible ordinary cancellation retained a registration");
        co_await wait([this,observed,beforeClosed]{return observed->closed==beforeClosed+1&&!visiblePopup();},L"Reopened ordinary flyout did not acknowledge native Closed");
        co_await settle();require(!ordinary.IsOpen(),L"Ordinary flyout reopened after cancellation");empty();
    }
    winrt::Windows::Foundation::IAsyncAction ordinaryClosing(){
        struct Observation{bool cancel=true,seen=false;int count=-1;};
        auto observed=std::make_shared<Observation>();auto blocker=std::make_shared<WorkspaceData>();context.apply(blocker);
        blocker->popupChanged=[data=data](bool open){data->externalPopup=open;};
        MenuFlyout ordinary;MenuFlyoutItem item;item.Text(L"Ordinary popup action");ordinary.Items().Append(item);TrackPopup(ordinary,blocker);
        ordinary.Closing([weak=weak_from_this(),observed,blocker](auto const& sender,auto const& args){
            args.Cancel(observed->cancel);
            sender.DispatcherQueue().TryEnqueue([weak,observed,blocker]{if(weak.lock()){observed->count=blocker->popupCount;observed->seen=true;}});
        });
        ordinary.ShowAt(anchor);co_await wait([this,blocker]{return blocker->popupCount==1&&data->externalPopup;},L"Ordinary popup did not block foreign admission");
        ordinary.Hide();co_await wait([observed]{return observed->seen;},L"Canceled Closing did not finish event dispatch");
        require(observed->count==1&&data->externalPopup&&ordinary.IsOpen(),L"Canceled Closing retired popup admission");
        observed->cancel=false;observed->seen=false;ordinary.Hide();
        co_await wait([observed]{return observed->seen;},L"Accepted Closing did not finish event dispatch");
        require(observed->count==0&&!data->externalPopup,L"Accepted Closing blocked the next native menu until Closed");
        auto next=owner.Begin(data,anchor,menu());require(next.Current(),L"Closing popup rejected the next source request");next.Cancel();empty();
        co_await wait([this,&ordinary]{return !ordinary.IsOpen()&&!visiblePopup();},L"Ordinary popup did not finish closing");
        co_await wait([blocker]{return blocker->transients.empty();},L"Closed ordinary popup retained cancellation registration");
    }
    static FrameworkElement findElement(DependencyObject const& parent,hstring const& id){
        if(!parent)return nullptr;
        if(auto node=parent.try_as<FrameworkElement>();node&&(AutomationProperties::GetAutomationId(node)==id||node.Name()==id))return node;
        for(int i=0;i<VisualTreeHelper::GetChildrenCount(parent);++i)if(auto node=findElement(VisualTreeHelper::GetChild(parent,i),id))return node;
        return nullptr;
    }
    FrameworkElement workspaceElement(hstring const& id){
        for(auto const& popup:VisualTreeHelper::GetOpenPopupsForXamlRoot(root.XamlRoot()))if(auto node=findElement(popup.Child(),id))return node;
        return findElement(root,id);
    }
    void nativeKey(WORD key,bool released){
        privacy();INPUT event{};event.type=INPUT_KEYBOARD;event.ki.wVk=key;event.ki.dwFlags=released?KEYEVENTF_KEYUP:0;
        require(SendInput(1,&event,sizeof(INPUT))==1,L"Workspace IME key injection failed");
    }
    winrt::Windows::Foundation::IAsyncAction workspaceIme(){
        auto dark=root.RequestedTheme()!=ElementTheme::Light;
        auto header=O({{L"this_workspace",S(L"This workspace")},{L"saved_toolbars",S(L"Saved toolbars")},{L"toolbar_actions",S(L"Actions")},
            {L"new_workspace",S(L"New Workspace")},{L"new_toolbar",S(L"New Toolbar")},{L"search",S(L"Search")},{L"description",S(L"Description")},
            {L"retry",S(L"Retry")},{L"move_up",S(L"Move Up")},{L"move_down",S(L"Move Down")},{L"current_workspace",S(L"Current workspace")},{L"drag_to_reorder",S(L"Drag to reorder")}});
        auto catalog=J::Parse(data->catalog.Stringify());catalog.Insert(L"native_copy",O({{L"header",header}}));
        auto palette=O({{L"text",S(dark?L"#eeeeee":L"#111111")},{L"card",S(dark?L"#343434":L"#eeeeee")},{L"accent",S(L"#4488cc")},{L"accent_foreground",S(L"#ffffff")}});
        auto state=O({{L"theme",S(dark?L"dark":L"light")},{L"palette",palette}});
        A options;for(auto type:{L"rename",L"delete"})options.Append(O({{L"label",S(type)},{L"enabled",B(true)},{L"action",O({{L"type",S(type)},{L"value",S(L"fixture-custom")}})}}));
        A rows;rows.Append(O({{L"id",S(L"fixture-custom")},{L"title",S(L"Fixture workspace")},{L"subtitle",S(L"")},{L"current",B(true)},
            {L"pin",O({{L"selected",B(true)},{L"enabled",B(true)},{L"label",S(L"Hide from top bar")},{L"icon",S(L"pin")},{L"edit",O({{L"type",S(L"toggle")},{L"id",S(L"fixture-custom")}})}})},
            {L"options",options}}));
        A order;order.Append(S(L"fixture-custom"));
        auto model=O({{L"id",S(L"fixture-custom")},{L"name",S(L"Fixture workspace")},{L"ready",B(true)},{L"busy",B(false)},{L"loading",B(false)},
            {L"page",S(L"workspaces")},{L"selected",S(L"fixture-custom")},{L"rows",rows},{L"order",order},{L"title",S(L"Workspaces")},{L"primary",S(L"Switch to Workspace")},{L"enabled",B(false)}});
        auto trace=imeTrace;
        auto close=[this](WorkspaceManagerView& manager)->winrt::Windows::Foundation::IAsyncAction{
            manager.Hide();co_await wait([&manager]{return !manager.IsOpen();},L"Workspace IME dialog did not close");
        };
        for(auto [key,label]:{std::pair<WORD,wchar_t const*>{WORD(VK_RETURN),L"inline-enter"},{WORD(VK_ESCAPE),L"inline-escape"},{WORD(VK_F2),L"inline-f2"},{WORD(VK_ESCAPE),L"form-escape"},{WORD(0),L"form-primary"}}){
            bool heldKey=false,inlineName=label[0]==L'i';auto inputs=std::make_shared<std::vector<J>>();auto events=std::make_shared<unsigned>(0);
            auto activeModel=J::Parse(model.Stringify());
            activeModel.Insert(L"prompt_action",inlineName?O({{L"type",S(L"rename")},{L"value",S(L"fixture-custom")}}):O({{L"type",S(L"new")}}));
            activeModel.Insert(L"prompt",O({{L"title",S(L"Workspace name")},{L"name",S(L"Fixture draft")},{L"confirm",S(L"Create")},{L"message",S(L"")},{L"destructive",B(false)}}));
            WorkspaceManagerView manager([inputs,trace,label](std::string json){auto request=J::Parse(to_hstring(json));inputs->push_back(request);trace->push_back(O({{L"case",S(label)},{L"dispatch",request}}));},catalog,context.localization,root.XamlRoot(),[]{},
                [](CanvasQueryKind,std::string,PreviewReply reply){reply(PreviewPacket{});return true;});
            try{
                manager.Apply(O({{L"state",state},{L"windows_workspace",activeModel}}),false);
                auto id=inlineName?L"workspace-manager-rename-fixture-custom":L"workspace-manager-name";
                co_await wait([this,id]{auto entry=workspaceElement(id).try_as<TextBox>();return entry&&entry.IsLoaded()&&entry.ActualWidth()>0;},L"Workspace IME TextBox did not become arranged");
                auto entry=workspaceElement(id).as<TextBox>();captureTextComposition(entry);entry.Focus(FocusState::Keyboard);
                co_await wait([this,entry]{return FocusManager::GetFocusedElement(root.XamlRoot())==entry;},L"Workspace IME TextBox did not own focus");privacy();
                entry.AddHandler(UIElement::KeyDownEvent(),box_value(KeyEventHandler([trace,events,label,editor=make_weak(entry)](auto&&,KeyRoutedEventArgs const& e){auto entry=editor.get();
                    ++*events;trace->push_back(O({{L"case",S(label)},{L"key",N(uint32_t(e.Key()))},{L"composing",B(textComposing(entry))},{L"handled",B(e.Handled())},{L"time",N(uint32_t(GetMessageTime()))}}));
                })),true);
                trace->push_back(O({{L"case",S(label)},{L"composition_requested",B(true)}}));
                auto dialog=workspaceElement(L"workspace-manager").as<ContentDialog>();
                std::vector<std::pair<UIElement,hstring>> routes{{dialog,L"dialog"}};
                if(inlineName)routes.emplace_back(workspaceElement(L"workspace-manager-items").as<UIElement>(),L"list");
                for(auto [control,route]:routes){
                    control.AddHandler(UIElement::KeyDownEvent(),box_value(KeyEventHandler([trace,label,route,editor=make_weak(entry)](auto&&,KeyRoutedEventArgs const& e){
                        trace->push_back(O({{L"case",S(label)},{L"route",S(route)},{L"key",N(uint32_t(e.Key()))},{L"handled",B(e.Handled())},{L"composing",B(textComposing(editor.get()))}}));
                    })),true);
                }
                dialog.Closing([trace,label,editor=make_weak(entry)](auto&&,ContentDialogClosingEventArgs const& e){trace->push_back(O({{L"case",S(label)},{L"native_event",S(L"Closing")},{L"canceled",B(e.Cancel())},{L"composing",B(textComposing(editor.get()))}}));});
                dialog.CloseButtonClick([trace,label,editor=make_weak(entry)](auto&&,ContentDialogButtonClickEventArgs const& e){trace->push_back(O({{L"case",S(label)},{L"native_event",S(L"CloseButtonClick")},{L"canceled",B(e.Cancel())},{L"composing",B(textComposing(editor.get()))}}));});
                dialog.PrimaryButtonClick([trace,label,editor=make_weak(entry)](auto&&,ContentDialogButtonClickEventArgs const& e){trace->push_back(O({{L"case",S(label)},{L"native_event",S(L"PrimaryButtonClick")},{L"canceled",B(e.Cancel())},{L"composing",B(textComposing(editor.get()))}}));});
                trace->push_back(O({{L"case",S(label)},{L"module_path",S(hstring(textServices->path))},{L"module_worker",N(textServices->worker)},{L"ui_thread",N(GetCurrentThreadId())}}));
                ScopedComposition composition(*textServices,[trace,label](hstring api,HRESULT result,uintptr_t pointer){
                    wchar_t hex[11]{};swprintf_s(hex,L"0x%08X",static_cast<unsigned>(result));
                    trace->push_back(O({{L"case",S(label)},{L"api",S(api)},{L"hresult",S(hex)},{L"code",N(int32_t(result))},{L"pointer",N(double(pointer))},{L"thread",N(GetCurrentThreadId())},{L"time",N(double(GetTickCount64()))}}));
                });
                co_await wait([this,entry,&composition]{return composition.focused(entry,root.XamlRoot());},L"Owned TextBox real TSF document/context did not become focused within the existing five-second prerequisite");
                trace->push_back(O({{L"case",S(label)},{L"focus_probes",N(composition.probes)},{L"context_ready",B(true)}}));composition.begin();
                co_await wait([entry]{return textComposing(entry);},L"Actual TSF composition did not reach the production TextBox observer");
                require(focusedTextComposing(root.XamlRoot()),L"Workspace TSF composition is not on the focused owned TextBox");
                trace->push_back(O({{L"case",S(label)},{L"composition_context",N(double(reinterpret_cast<uintptr_t>(composition.context.get())))},{L"preedit",S(entry.Text())},{L"started",B(true)}}));
                if(key){
                    nativeKey(key,false);heldKey=true;co_await wait([events]{return *events>0;},L"Workspace composition key did not reach the actual TextBox route");co_await settle();
                }else{
                    co_await wait([this]{auto button=workspaceElement(L"PrimaryButton").try_as<Button>();return button&&button.IsEnabled();},L"Workspace native primary button was not enabled");
                    auto peer=winrt::Microsoft::UI::Xaml::Automation::Peers::ButtonAutomationPeer(workspaceElement(L"PrimaryButton").as<Button>());
                    peer.GetPattern(winrt::Microsoft::UI::Xaml::Automation::Peers::PatternInterface::Invoke).as<winrt::Microsoft::UI::Xaml::Automation::Provider::IInvokeProvider>().Invoke();co_await settle();
                    require(focusedTextComposing(root.XamlRoot()),L"Native primary invocation lost its composing TextBox prerequisite");
                }
                require(inputs->empty(),L"Composition-owned input dispatched a workspace submit, cancel, select, form or dismissal");
                require(manager.IsOpen()&&workspaceElement(id)==entry&&entry.IsLoaded(),L"Composition-owned input closed or replaced its native manager control");
                composition.end();co_await wait([entry]{return !textComposing(entry);},L"Workspace TSF composition did not finish");
                if(key){nativeKey(key,true);heldKey=false;co_await settle();require(inputs->empty(),L"Composition key release dispatched a workspace action");nativeKey(key,false);heldKey=true;nativeKey(key,true);heldKey=false;}
                else{
                    auto peer=winrt::Microsoft::UI::Xaml::Automation::Peers::ButtonAutomationPeer(workspaceElement(L"PrimaryButton").as<Button>());
                    peer.GetPattern(winrt::Microsoft::UI::Xaml::Automation::Peers::PatternInterface::Invoke).as<winrt::Microsoft::UI::Xaml::Automation::Provider::IInvokeProvider>().Invoke();
                }
                co_await wait([inputs]{return !inputs->empty();},L"Fresh ordinary workspace input did not recover after composition");co_await settle();
                std::vector<hstring> actual;for(auto const& request:*inputs){require(str(request,L"operation")==L"input",L"Workspace IME fixture received a host failure");actual.push_back(str(object(request,L"input"),L"type"));}
                std::vector<hstring> expected=key==VK_F2?std::vector<hstring>{L"submit",L"form"}:std::vector<hstring>{key==VK_ESCAPE?L"cancel":L"submit"};
                require(actual==expected,L"Ordinary workspace input produced duplicate or incorrect follow-up actions");
                if(key!=VK_ESCAPE)require(str(object(inputs->front(),L"input"),L"name")==entry.Text(),L"Workspace submitted an older TextBox draft");
                composition.finish();trace->push_back(O({{L"case",S(label)},{L"passed",B(true)}}));++imeChecks;co_await close(manager);
            }catch(...){if(heldKey)try{nativeKey(key,true);}catch(...){}manager.Hide();throw;}
        }
    }

    winrt::fire_and_forget run(){
        auto self=shared_from_this();hstring error;
        try {
            co_await wait([this]{return anchor.IsLoaded()&&untagged.ActualWidth()>0&&GetForegroundWindow()==hwnd;},L"Fixture did not become arranged and foreground");privacy();
            phase=L"chrome-replacement";auto fade=std::make_shared<ChromeVisibility>();Border previous,replacement,decoration;
            fade->apply(previous,true,true);fade->apply(replacement,true,true);
            require(replacement.Visibility()==Visibility::Collapsed&&replacement.Opacity()==0&&!replacement.IsHitTestVisible(),L"Replacement chrome became visible while Zen remained hidden");
            fade->apply(replacement,true,false);
            require(replacement.Visibility()==Visibility::Visible&&replacement.Opacity()==1&&replacement.IsHitTestVisible(),L"Replacement chrome did not return from Zen");
            decoration.IsHitTestVisible(false);fade->apply(decoration,true,false);
            require(!decoration.IsHitTestVisible(),L"Replacement decorative chrome accepted input");++checks;
            phase=L"registry";auto popup=menu();auto ticket=owner.Begin(data,anchor,popup);pending();
            require(data->dismissTransients(),L"Pending popup ignored window cancellation registry");
            require(!ticket.Current()&&!ticket.Show(anchor,{}),L"Registry-canceled reply reopened popup");empty();++checks;
            phase=L"replacement";auto reused=menu();auto old=owner.Begin(data,anchor,reused);auto current=owner.Begin(data,anchor,reused);pending();
            require(!old.Cancel()&&!old.Current()&&!old.Show(anchor,{})&&current.Current(),L"Superseded query canceled or replaced current popup lifetime");
            require(current.Cancel(),L"Current query cancellation was refused");empty();++checks;
            phase=L"unload";auto unloaded=owner.Begin(data,anchor,menu());root.Children().RemoveAt(0);co_await settle();
            require(!unloaded.Current()&&!unloaded.Show(untagged,{}),L"Unloaded source query opened on replacement anchor");empty();
            root.Children().InsertAt(0,anchor);co_await settle();++checks;
            phase=L"escape";auto escaped=owner.Begin(data,anchor,menu());pending();anchor.Focus(FocusState::Programmatic);privacy();
            INPUT keyboard[2]{};keyboard[0].type=keyboard[1].type=INPUT_KEYBOARD;keyboard[0].ki.wVk=keyboard[1].ki.wVk=VK_ESCAPE;keyboard[1].ki.dwFlags=KEYEVENTF_KEYUP;
            require(SendInput(2,keyboard,sizeof(INPUT))==2,L"Native Escape injection failed");co_await wait([this]{return keys==1&&!owner.Active();},L"Native Escape cancellation timed out");
            require(keys==1&&!escaped.Current()&&!escaped.Show(anchor,{}),L"Native Escape did not cancel delayed popup");empty();++checks;
            phase=L"originating-press";nativePress(anchor,true);
            co_await wait([this]{return originPresses==1;},L"Originating native press did not reach source");co_await settle();
            require(origin&&origin->Current(),L"Popup canceled itself on its originating routed press");pending();
            nativePress(untagged);co_await wait([this]{return presses==1&&!owner.Active();},L"Next native press did not cancel originating popup request");
            require(!origin->Current()&&!origin->Show(anchor,{}),L"Canceled originating request reopened popup");empty();++checks;
            phase=L"untagged-press";auto beforePress=presses;auto pressed=owner.Begin(data,anchor,menu());pending();co_await settle();co_await wait([this]{return anchor.IsLoaded()&&untagged.ActualWidth()>0&&GetForegroundWindow()==hwnd;},L"Fixture did not become arranged and foreground");privacy();
            nativePress(untagged);co_await wait([this,beforePress]{return presses==beforePress+1&&!owner.Active();},L"Native untagged press cancellation timed out");
            require(presses==beforePress+1&&!pressed.Current()&&!pressed.Show(anchor,{}),L"Handled native untagged press did not cancel delayed popup");empty();++checks;
            phase=L"blur";auto beforeBlur=blurs;auto blurred=owner.Begin(data,anchor,menu());pending();other.Activate();SetForegroundWindow(handle(other));co_await wait([this,beforeBlur]{return blurs>beforeBlur&&!owner.Active();},L"Native deactivation cancellation timed out");
            require(blurs>beforeBlur&&!blurred.Current()&&!blurred.Show(anchor,{}),L"Native window deactivation did not cancel delayed popup");empty();auto wrongOwner=owner.Begin(data,anchor,menu());
            require(!wrongOwner.Current()&&!wrongOwner.Show(anchor,{}),L"Popup accepted source owned by background window");empty();++checks;
            window.Activate();SetForegroundWindow(hwnd);co_await wait([this]{return anchor.IsLoaded()&&untagged.ActualWidth()>0&&GetForegroundWindow()==hwnd;},L"Fixture did not become arranged and foreground");privacy();
            phase=L"opening-gap";auto beforeGap=opened;auto opening=owner.Begin(data,anchor,menu());pending();require(opening.Show(anchor,{}),L"Current opening-gap popup did not show");
            require(owner.Pending()&&opened==beforeGap,L"Opening-gap prerequisite reached Opened synchronously");
            require(data->dismissTransients(),L"Opening popup ignored window cancellation registry");co_await settle();
            require(opened==beforeGap&&!visiblePopup(),L"Opening-gap cancellation left a late or visible native popup");
            require(!opening.Current()&&!opening.Show(anchor,{}),L"Opening-gap canceled reply reopened popup");empty();
            phase=L"ordinary-opening";co_await ordinaryOpening();phase=L"ordinary-closing";co_await ordinaryClosing();++checks;
            phase=L"show-close";auto beforeOpen=opened;auto beforeClosed=closedCount;auto shown=owner.Begin(data,anchor,menu());pending();require(shown.Show(anchor,{}),L"Current popup did not show");co_await wait([this,beforeOpen]{return opened==beforeOpen+1&&owner.Open();},L"Popup Opened timed out");
            require(opened==beforeOpen+1&&owner.Open()&&data->transients.size()==1,L"Popup registration did not survive ShowAt through Opened");
            require(data->dismissTransients(),L"Visible popup ignored cancellation registry");co_await wait([this,beforeClosed]{return closedCount==beforeClosed+1&&!visiblePopup();},L"Native popup Closed acknowledgement or visible-popup retirement timed out");
            require(!shown.Current()&&!shown.Show(anchor,{}),L"Reply reopened closed popup");empty();++checks;
            phase=L"workspace-ime";co_await workspaceIme();
            phase=L"window-close";auto closed=owner.Begin(data,anchor,menu());window.Close();co_await settle();
            require(!closed.Current()&&!closed.Show(anchor,{}),L"Reply reopened canceled owner");empty();++checks;
        } catch(hresult_error const& e){resultCode=1;error=e.message();}catch(...){resultCode=1;error=L"Unhandled popup fixture failure";}
        owner.Cancel();auto count=checks;auto imeCount=imeChecks;auto step=to_string(phase);auto message=to_string(error);auto path=output;
        A imeEvents;for(auto const& event:*imeTrace)imeEvents.Append(event);auto imeJson=to_string(imeEvents.Stringify());
        co_await resume_background();
        {std::ofstream file(path.parent_path()/L"workspace-ime-events.json");file<<imeJson;if(!file)resultCode=1;}
        {std::ofstream file(path);file<<"{\"passed\":"<<count<<",\"ime_passed\":"<<imeCount<<",\"exit_code\":"<<resultCode<<",\"phase\":\""<<step<<"\"}\n";if(!file)resultCode=1;}
        if(!message.empty()){std::ofstream file(path.parent_path()/L"popup-fixture-error.log");file<<message;}
        co_await apartment;other.Close();if(!windowClosed)window.Close();Application::Current().Exit();
    }
    void init(std::filesystem::path path){
        output=path.parent_path()/L"popup-fixture-result.json";context.apply(data);
        wchar_t theme[16]{};GetEnvironmentVariableW(L"CAPY_POPUP_THEME",theme,16);root.RequestedTheme(std::wstring(theme)==L"light"?ElementTheme::Light:ElementTheme::Dark);
        anchor.Content(box_value(L"Delayed popup source"));untagged.Content(box_value(L"Untagged input target"));
        root.Children().Append(anchor);root.Children().Append(untagged);root.Width(400);root.Height(240);
        root.HorizontalAlignment(HorizontalAlignment::Left);root.VerticalAlignment(VerticalAlignment::Top);
        root.KeyDown([weak=weak_from_this()](auto&&,Input::KeyRoutedEventArgs const& e){if(e.Key()==Windows::System::VirtualKey::Escape)if(auto self=weak.lock()){++self->keys;e.Handled(self->data->dismissTransients());}});
        anchor.AddHandler(UIElement::PointerPressedEvent(),box_value(Input::PointerEventHandler([weak=weak_from_this()](auto&&,Input::PointerRoutedEventArgs const& e){
            if(auto self=weak.lock();self&&self->phase==L"originating-press"){
                ++self->originPresses;self->origin=self->owner.Begin(self->data,self->anchor,self->menu());e.Handled(true);
            }
        })),true);
        untagged.AddHandler(UIElement::PointerPressedEvent(),box_value(Input::PointerEventHandler([weak=weak_from_this()](auto&&,Input::PointerRoutedEventArgs const& e){if(auto self=weak.lock())++self->presses;e.Handled(true);})),true);
        window.Closed([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){self->windowClosed=true;self->owner.Cancel();}});
        window.Activated([weak=weak_from_this()](auto&&,WindowActivatedEventArgs const& e){if(e.WindowActivationState()==WindowActivationState::Deactivated)if(auto self=weak.lock()){++self->blurs;self->owner.Cancel();}});
        TextBlock text;text.Text(L"Owned blur target");other.Content(text);other.Title(L"Popup fixture blur target");other.AppWindow().Move({600,80});
        root.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->run();});
        window.Content(root);window.Title(L"Capy Canvas popup review");hwnd=handle(window);window.AppWindow().Move({40,40});window.Activate();
    }
};
struct App:ApplicationT<App,Markup::IXamlMetadataProvider>{
    Microsoft::UI::Xaml::XamlTypeInfo::XamlControlsXamlMetaDataProvider metadata;
    std::shared_ptr<Fixture> fixture;ControlContext context;std::shared_ptr<TextServicesLibrary> textServices;
    App(ControlContext source,std::shared_ptr<TextServicesLibrary> services):context(std::move(source)),textServices(std::move(services)){UnhandledException([this](auto&&,UnhandledExceptionEventArgs const& e){resultCode=1;e.Handled(true);Exit();});}
    Markup::IXamlType GetXamlType(Windows::UI::Xaml::Interop::TypeName const& type){return metadata.GetXamlType(type);}
    Markup::IXamlType GetXamlType(hstring const& name){return metadata.GetXamlType(name);}
    com_array<Markup::XmlnsDefinition> GetXmlnsDefinitions(){return metadata.GetXmlnsDefinitions();}
    void OnLaunched(LaunchActivatedEventArgs const&){Resources().MergedDictionaries().Append(XamlControlsResources());wchar_t path[32768];auto size=GetEnvironmentVariableW(L"CAPY_POPUP_FIXTURE",path,32768);if(!size||size>=32768||!std::filesystem::path(path).is_absolute())throw hresult_invalid_argument();fixture=std::make_shared<Fixture>();fixture->context=context;fixture->textServices=textServices;fixture->init(path);}
};
}
int WINAPI wWinMain(HINSTANCE,HINSTANCE,PWSTR,int){auto context=fixtureContext(L"CAPY_POPUP_FIXTURE");auto textServices=std::make_shared<TextServicesLibrary>();init_apartment(apartment_type::single_threaded);Application::Start([context,textServices](auto&&){make<App>(context,textServices);});return resultCode;}
