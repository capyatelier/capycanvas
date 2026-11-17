#pragma once
#include "UiControls.h"
#include "PanelScrollReceipt.h"

namespace CapyUi {
struct PanelScrollTrace {
    PanelScrollReceipt receipt;
    weak_ref<ScrollView> owner;
    uint64_t tick=0;
    std::optional<uint64_t> published;
    std::optional<double> nativeOffset,nativeHeight;
    bool idle=false;
    explicit PanelScrollTrace(ScrollView const& scroll):owner(make_weak(scroll)){}
    static J event(PanelScrollReceipt::Event const& value){
        static constexpr wchar_t const* names[]={L"contact",L"release",L"target",L"applied",L"complete",L"started",L"native"};
        return O({{L"kind",S(names[size_t(value.kind)])},{L"serial",N(double(value.serial))},{L"tick",N(double(value.tick))},
            {L"target",N(value.target)},{L"offset",N(value.applied)},{L"inertial",B(value.inertial)}});
    }
    bool Publish(uint64_t frame){
        auto scroll=owner.get();if(!scroll)return false;tick=frame;
        auto offset=scroll.VerticalOffset(),height=scroll.ScrollableHeight();bool nativeIdle=scroll.State()==ScrollingInteractionState::Idle;
        if(!nativeOffset||*nativeOffset!=offset||*nativeHeight!=height||idle!=nativeIdle){
            nativeOffset=offset;nativeHeight=height;idle=nativeIdle;
            receipt.Record(PanelScrollReceipt::Kind::Native,tick,receipt.finalTarget?receipt.finalTarget->target:offset,offset);
        }
        if(published&&*published==receipt.serial)return true;
        A events;for(auto const& row:receipt.events)events.Append(event(row));
        auto optional=[](std::optional<PanelScrollReceipt::Event> const& row)->V{if(row)return event(*row);return JsonValue::CreateNullValue();};
        auto panel=O({{L"contact",N(double(receipt.contact))},{L"pointer",N(double(receipt.pointer))},{L"serial",N(double(receipt.serial))},
            {L"terminal",B(receipt.terminal)},{L"release",optional(receipt.release)},{L"inertia",optional(receipt.inertia)},
            {L"applied",optional(receipt.applied)},{L"events",events},{L"final_target",optional(receipt.finalTarget)},{L"last_applied",optional(receipt.lastApplied)},
            {L"holding",B(receipt.holding)},{L"running",B(receipt.running)},{L"idle",B(idle)},{L"offset",N(offset)},{L"scrollable_height",N(height)}});
        edgeTag(scroll,L"panel_scroll",panel);
        auto existing=AutomationProperties::GetItemStatus(scroll);auto value=existing.empty()?J{}:J::Parse(existing);
        value.Insert(L"panel_scroll",panel);AutomationProperties::SetItemStatus(scroll,value.Stringify());published=receipt.serial;return true;
    }
};
inline void panelScrolling(ScrollView const& scroll,UIElement const& content,std::shared_ptr<WorkspaceData> const& data){
    struct Contact {std::optional<uint32_t> pointer;uint64_t epoch=0;double offset=0;bool running=false,canceled=false;std::shared_ptr<PanelScrollTrace> trace;std::function<void()> complete;};
    auto state=std::make_shared<Contact>();auto context=std::weak_ptr(data);auto weak=make_weak(scroll);auto surface=make_weak(content);
    edgeTag(content.as<FrameworkElement>(),L"edge_contact",S(L"free"));
    if(GetEnvironmentVariableW(L"CAPY_TRACE_UI",nullptr,0)){
        state->trace=std::make_shared<PanelScrollTrace>(scroll);
        data->nativeFrameTraces.emplace_back([weak=std::weak_ptr(state->trace)](uint64_t frame){auto trace=weak.lock();return trace&&trace->Publish(frame);});
        scroll.ViewChanged([state,weak](auto&&,auto&&){if(auto scroll=weak.get();scroll)state->trace->receipt.Applied(state->trace->tick,state->offset,scroll.VerticalOffset());});
    }

    auto forget=[context,state]{if(auto data=context.lock();data&&state->pointer)data->pointerCancels.Forget(*state->pointer,state.get());state->pointer.reset();};
    auto cancel=[state,forget,weak]{if(state->trace&&state->trace->receipt.contact)if(auto scroll=weak.get())state->trace->receipt.Complete(state->trace->tick,state->offset,scroll.VerticalOffset());state->canceled=true;state->running=false;forget();if(auto complete=std::exchange(state->complete,{}))complete();};
    scroll.IgnoredInputKinds(ScrollingInputKinds::Touch|ScrollingInputKinds::Pen);
    scroll.Background(nullptr);scroll.Loaded([weak](auto&&,auto&&){if(auto scroll=weak.get())if(auto presenter=scroll.ScrollPresenter())presenter.Background(nullptr);});
    content.ManipulationMode(ManipulationModes::TranslateRailsY|ManipulationModes::TranslateInertia);
    content.AddHandler(UIElement::PointerPressedEvent(),box_value(PointerEventHandler([context,state,cancel,surface,weak](auto&&,PointerRoutedEventArgs const& e){
        if(auto data=context.lock();data&&data->claimBubbleContact(e))return;
        auto point=e.GetCurrentPoint(nullptr);
        if(state->pointer||point.PointerDeviceType()==Microsoft::UI::Input::PointerDeviceType::Mouse||!PointerGesture::PrimaryContact(point))return;
        if(state->complete)cancel();
        if(auto content=surface.get())content.ManipulationMode(ManipulationModes::TranslateRailsY|ManipulationModes::TranslateInertia);
        state->pointer=point.PointerId();state->canceled=false;
        if(state->trace)if(auto scroll=weak.get())state->trace->receipt.Begin(point.PointerId(),state->trace->tick,scroll.VerticalOffset(),scroll.VerticalOffset());
        if(auto data=context.lock()){state->epoch=uint64_t(num(branch(data->state,L"document_file"),L"epoch"));data->pointerCancels.Observe(*state->pointer,state.get(),cancel);}
    })),true);
    content.ManipulationStarting([weak,state](auto&&,ManipulationStartingRoutedEventArgs const& e){if(auto scroll=weak.get())e.Container(scroll);if(state->canceled||!state->pointer)e.Mode(ManipulationModes::None);});
    content.ManipulationStarted([weak,state,cancel](auto&&,ManipulationStartedRoutedEventArgs const& e){state->complete=[e]{e.Complete();};if(auto scroll=weak.get();scroll&&!state->canceled){state->offset=scroll.VerticalOffset();state->running=true;if(state->trace)state->trace->receipt.Started(state->trace->tick,state->offset,scroll.VerticalOffset());}else cancel();});
    content.ManipulationDelta([weak,context,state,cancel](auto&&,ManipulationDeltaRoutedEventArgs const& e){
        state->complete=[e]{e.Complete();};auto data=context.lock();auto scroll=weak.get();
        if(!scroll||!data||!state->running||state->canceled||scroll.VerticalScrollMode()==ScrollingScrollMode::Disabled||state->epoch!=uint64_t(num(branch(data->state,L"document_file"),L"epoch"))){cancel();return;}
        auto delta=e.Delta().Translation.Y;auto next=std::clamp(state->offset-delta,0.,scroll.ScrollableHeight());
        if(e.IsInertial()&&next==state->offset&&delta!=0){cancel();return;}
        if(next!=state->offset){state->offset=next;if(state->trace)state->trace->receipt.Target(state->trace->tick,next,scroll.VerticalOffset(),e.IsInertial());scrollVertical(scroll,next);}e.Handled(true);
    });
    content.ManipulationCompleted([state,weak](auto&&,auto&&){if(state->trace)if(auto scroll=weak.get())state->trace->receipt.Complete(state->trace->tick,state->offset,scroll.VerticalOffset());state->complete=nullptr;state->running=false;});
    auto released=PointerEventHandler([state,forget,cancel,weak](auto&&,PointerRoutedEventArgs const& e){
        if(state->pointer==e.Pointer().PointerId()){if(e.GetCurrentPoint(nullptr).Properties().IsCanceled())cancel();else{if(state->trace)if(auto scroll=weak.get())state->trace->receipt.Release(state->trace->tick,state->offset,scroll.VerticalOffset());forget();}}
    });
    auto canceled=PointerEventHandler([state,cancel](auto&&,PointerRoutedEventArgs const& e){
        if(state->pointer==e.Pointer().PointerId())cancel();
    });
    auto input=std::make_shared<weak_ref<UIElement>>();
    auto detach=[input,released,canceled]{if(auto root=input->get()){
        root.RemoveHandler(UIElement::PointerReleasedEvent(),box_value(released));
        root.RemoveHandler(UIElement::PointerCanceledEvent(),box_value(canceled));
    }*input={};};
    auto attach=[surface,input,detach,released,canceled]{detach();if(auto content=surface.get();content&&content.XamlRoot())if(auto root=content.XamlRoot().Content()){
        *input=make_weak(root);root.AddHandler(UIElement::PointerReleasedEvent(),box_value(released),true);
        root.AddHandler(UIElement::PointerCanceledEvent(),box_value(canceled),true);
    }};
    content.as<FrameworkElement>().Loaded([attach](auto&&,auto&&){attach();});
    content.as<FrameworkElement>().Unloaded([detach,cancel](auto&&,auto&&){detach();cancel();});
    if(content.as<FrameworkElement>().IsLoaded())attach();
}
}
