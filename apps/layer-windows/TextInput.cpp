#include "pch.h"
#include "UiControls.h"
#include "TextCompositionKeys.h"
#include <msctf.h>

namespace CapyUi {
namespace {
auto resourceKey(){return box_value(L"CapyTextComposition");}
auto windowKey(){return box_value(L"CapyTextCompositionWindow");}
struct TextComposition : implements<TextComposition,Windows::Foundation::IInspectable,ITfTextEditSink> {
    TextCompositionKeys keys;
    weak_ref<TextBox> entry;
    weak_ref<FrameworkElement> host;
    weak_ref<UIElement> releaseTarget;
    Windows::Foundation::IInspectable releaseHandler{nullptr};
    FocusManager::GettingFocus_revoker focus;
    void Watch(UIElement const& target){
        if(releaseTarget.get()==target)return;
        if(auto previous=releaseTarget.get();previous&&releaseHandler)previous.RemoveHandler(UIElement::KeyUpEvent(),releaseHandler);
        releaseTarget=target?make_weak(target):weak_ref<UIElement>{};releaseHandler=nullptr;
        if(target){
            releaseHandler=box_value(KeyEventHandler([weak=get_weak()](auto&&,KeyRoutedEventArgs const& event){if(auto state=weak.get())state->Key(event);}));
            target.AddHandler(UIElement::KeyUpEvent(),releaseHandler,true);
        }
    }
    void Initialize(FrameworkElement const& root){host=make_weak(root);focus=FocusManager::GettingFocus(auto_revoke,[weak=get_weak()](auto&&,GettingFocusEventArgs const& event){if(auto state=weak.get();state&&state->keys.Busy()){auto next=event.NewFocusedElement().try_as<UIElement>();state->Watch(next?next:state->host.get());}});}
    bool Key(KeyRoutedEventArgs const& event,UIElement const& target=nullptr){
        auto status=event.KeyStatus();auto physical=status.ScanCode?status.ScanCode|(status.IsExtendedKey?0x100u:0u):0x10000u|uint32_t(event.OriginalKey());
        auto owned=keys.Owns(uint32_t(event.Key()),uint32_t(GetMessageTime()),status.IsKeyReleased,status.WasKeyDown,physical);
        if(keys.Busy()){
            if(!status.IsKeyReleased)Watch(target?target:host.get());
            Notify();
        }else Watch(nullptr);
        if(event.KeyStatus().IsKeyReleased)NotifyAfterInput();
        return owned;
    }
    std::function<void(bool)> localizationInput;
    void Notify(){if(localizationInput)localizationInput(keys.Busy());}
    void NotifyAfterInput(){if(keys.Busy())Notify();else Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread().TryEnqueue([weak=get_weak()]{if(auto state=weak.get())state->Notify();});}
    com_ptr<ITfSource> source;
    DWORD cookie=TF_INVALID_COOKIE;
    ~TextComposition(){Watch(nullptr);Detach();}
    void Detach(){
        if(source&&cookie!=TF_INVALID_COOKIE)source->UnadviseSink(cookie);
        cookie=TF_INVALID_COOKIE;source=nullptr;
    }
    void Clear(TextBox const& field){
        if(entry.get()==field){Detach();entry={};keys.Clear();}
        if(keys.Busy()&&releaseTarget.get()==field)Watch(host.get());
        NotifyAfterInput();
    }
    void Start(TextBox const& field){
        entry=make_weak(field);
        keys.Update(true,uint32_t(GetMessageTime()));Notify();
        auto library=GetModuleHandleW(L"msctf.dll");
        auto getManager=library?reinterpret_cast<HRESULT(WINAPI*)(ITfThreadMgr**)>(GetProcAddress(library,"TF_GetThreadMgr")):nullptr;
        com_ptr<ITfThreadMgr> manager;com_ptr<ITfDocumentMgr> document;com_ptr<ITfContext> context;
        if(!getManager||FAILED(getManager(manager.put()))||!manager||FAILED(manager->GetFocus(document.put()))||!document||FAILED(document->GetTop(context.put()))||!context)return;
        auto next=context.try_as<ITfSource>();
        if(next==source)return;
        Detach();
        if(next&&SUCCEEDED(next->AdviseSink(__uuidof(ITfTextEditSink),static_cast<ITfTextEditSink*>(this),&cookie)))source=next;
    }
    HRESULT __stdcall OnEndEdit(ITfContext* context,TfEditCookie,ITfEditRecord*) noexcept override {
        com_ptr<ITfContext> active;active.copy_from(context);if(active.try_as<ITfSource>()!=source)return S_OK;
        com_ptr<ITfContextComposition> composing;com_ptr<IEnumITfCompositionView> items;com_ptr<ITfCompositionView> item;ULONG count=0;
        if(SUCCEEDED(context->QueryInterface(__uuidof(ITfContextComposition),composing.put_void()))&&SUCCEEDED(composing->EnumCompositions(items.put()))&&SUCCEEDED(items->Next(1,item.put(),&count)))
            keys.Update(count!=0,uint32_t(GetTickCount()));
        NotifyAfterInput();return S_OK;
    }
};
TextBox textEntry(DependencyObject element){
    for(;element;element=VisualTreeHelper::GetParent(element))if(auto entry=element.try_as<TextBox>())return entry;
    return nullptr;
}
TextComposition* composition(DependencyObject element){
    auto entry=textEntry(element);if(!entry)return nullptr;
    auto key=resourceKey();
    if(!entry.Resources().HasKey(key))entry.Resources().Insert(key,make_self<TextComposition>().as<Windows::Foundation::IInspectable>());
    auto local=get_self<TextComposition>(entry.Resources().Lookup(key));auto root=entry.XamlRoot();
    if(auto host=root?root.Content().try_as<FrameworkElement>():FrameworkElement{nullptr}){
        auto shared=windowKey();
        if(!host.Resources().HasKey(shared)){local->Initialize(host);host.Resources().Insert(shared,entry.Resources().Lookup(key));}
        auto state=get_self<TextComposition>(host.Resources().Lookup(shared));
        if(state!=local&&local->localizationInput)state->localizationInput=std::move(local->localizationInput);
        entry.Resources().Insert(key,host.Resources().Lookup(shared));return state;
    }
    return local;
}
}
void captureTextComposition(TextBox const& entry,std::function<void(bool)> localizationInput){
    auto captured=box_value(L"CapyTextCompositionCaptured");
    if(localizationInput)if(auto state=composition(entry))state->localizationInput=std::move(localizationInput);
    if(entry.Resources().HasKey(captured))return;
    entry.Resources().Insert(captured,box_value(true));
    entry.TextCompositionStarted([](TextBox const& entry,auto&&){if(auto state=composition(entry))state->Start(entry);});
    entry.TextCompositionEnded([](TextBox const& entry,auto&&){if(auto state=composition(entry);state&&!state->source&&state->entry.get()==entry){state->keys.Update(false,uint32_t(GetTickCount()));state->NotifyAfterInput();}});
    entry.LostFocus([](auto&& sender,auto&&){auto entry=sender.template as<TextBox>();if(auto state=composition(entry))state->Clear(entry);});
    entry.Unloaded([](auto&& sender,auto&&){auto entry=sender.template as<TextBox>();if(auto state=composition(entry))state->Clear(entry);});
    entry.PreviewKeyDown([](auto&&,KeyRoutedEventArgs const& event){if(composingKey(event))event.Handled(true);});
    entry.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([](auto&& sender,auto&&){if(auto state=composition(sender.template as<TextBox>())){state->keys.Pointer();state->NotifyAfterInput();}})),true);
}

bool textComposing(DependencyObject element){
    auto state=composition(element);return state&&state->keys.Active()&&state->entry.get()==textEntry(element);
}
void updateText(TextBox const& entry,hstring const& value){
    captureTextComposition(entry);
    if(!textComposing(entry)&&entry.Text()!=value)entry.Text(value);
}
bool focusedTextComposing(XamlRoot const& root){
    return root&&textComposing(FocusManager::GetFocusedElement(root).try_as<DependencyObject>());
}
bool composingKey(KeyRoutedEventArgs const& event){
    auto state=composition(event.OriginalSource().try_as<DependencyObject>());
    return state?state->Key(event,textEntry(event.OriginalSource().try_as<DependencyObject>())):event.Key()==static_cast<Windows::System::VirtualKey>(VK_PROCESSKEY);
}
}
