#include "pch.h"
#include "UiControls.h"
#include "TextCompositionKeys.h"
#include <msctf.h>

namespace CapyUi {
namespace {
auto resourceKey(){return box_value(L"CapyTextComposition");}
struct TextComposition : implements<TextComposition,Windows::Foundation::IInspectable,ITfTextEditSink> {
    TextCompositionKeys keys;
    std::function<void(bool)> localizationInput;
    void Notify(){if(localizationInput)localizationInput(keys.Busy());}
    com_ptr<ITfSource> source;
    DWORD cookie=TF_INVALID_COOKIE;
    ~TextComposition(){Detach();}
    void Detach(){
        if(source&&cookie!=TF_INVALID_COOKIE)source->UnadviseSink(cookie);
        cookie=TF_INVALID_COOKIE;source=nullptr;
    }
    void Clear(){Detach();keys.Clear();Notify();}
    void Start(){
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
        com_ptr<ITfContextComposition> composing;com_ptr<IEnumITfCompositionView> items;com_ptr<ITfCompositionView> item;ULONG count=0;
        if(SUCCEEDED(context->QueryInterface(__uuidof(ITfContextComposition),composing.put_void()))&&SUCCEEDED(composing->EnumCompositions(items.put()))&&SUCCEEDED(items->Next(1,item.put(),&count)))
            keys.Update(count!=0,uint32_t(GetMessageTime()));
        Notify();return S_OK;
    }
};
TextComposition* composition(DependencyObject element){
    for(;element;element=VisualTreeHelper::GetParent(element))if(auto entry=element.try_as<TextBox>()){
        auto key=resourceKey();
        return entry.Resources().HasKey(key)?get_self<TextComposition>(entry.Resources().Lookup(key)):nullptr;
    }
    return nullptr;
}
}
void captureTextComposition(TextBox const& entry,std::function<void(bool)> localizationInput){
    auto key=resourceKey();
    if(entry.Resources().HasKey(key)){
        if(localizationInput)get_self<TextComposition>(entry.Resources().Lookup(key))->localizationInput=std::move(localizationInput);
        return;
    }
    auto state=make_self<TextComposition>();state->localizationInput=std::move(localizationInput);
    entry.Resources().Insert(key,state.as<Windows::Foundation::IInspectable>());
    auto weak=state->get_weak();
    entry.TextCompositionStarted([weak](auto&&,auto&&){if(auto state=weak.get())state->Start();});
    entry.TextCompositionEnded([weak](auto&&,auto&&){if(auto state=weak.get();state&&!state->source){state->keys.Update(false,uint32_t(GetMessageTime()));state->Notify();}});
    entry.LostFocus([weak](auto&&,auto&&){if(auto state=weak.get())state->Clear();});
    entry.Unloaded([weak](auto&&,auto&&){if(auto state=weak.get())state->Clear();});
    entry.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([weak](auto&&,auto&&){if(auto state=weak.get()){state->keys.Pointer();state->Notify();}})),true);
    entry.AddHandler(UIElement::KeyUpEvent(),box_value(KeyEventHandler([weak](auto&&,KeyRoutedEventArgs const& event){
        if(auto state=weak.get()){
            state->keys.Owns(uint32_t(event.Key()),uint32_t(GetMessageTime()),true,event.KeyStatus().WasKeyDown);
            auto entry=event.OriginalSource().try_as<FrameworkElement>();
            if(entry)entry.DispatcherQueue().TryEnqueue([weak]{if(auto state=weak.get())state->Notify();});
        }
    })),true);
}
bool textComposing(DependencyObject element){
    auto state=composition(element);return state&&state->keys.Active();
}
bool focusedTextComposing(XamlRoot const& root){
    return root&&textComposing(FocusManager::GetFocusedElement(root).try_as<DependencyObject>());
}
bool composingKey(KeyRoutedEventArgs const& event){
    if(event.Key()==static_cast<Windows::System::VirtualKey>(VK_PROCESSKEY))return true;
    auto state=composition(event.OriginalSource().try_as<DependencyObject>());
    return state&&state->keys.Owns(uint32_t(event.Key()),uint32_t(GetMessageTime()),event.KeyStatus().IsKeyReleased,event.KeyStatus().WasKeyDown);
}
}
