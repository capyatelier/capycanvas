#pragma once
#include "UiControls.h"
#include "ColorEditor.h"
#include <array>
#include <utility>

namespace CapyEffects {
using namespace CapyUi;
struct Updating {
    bool& flag;bool previous;
    explicit Updating(std::shared_ptr<WorkspaceData> const& data):flag(data->updating),previous(std::exchange(flag,true)){}
    ~Updating(){flag=previous;}
};
inline A values(std::initializer_list<double> list){A a;for(double v:list)a.Append(N(v));return a;}
inline J effectGesture(J const& action,hstring const& phase){
    return phase.empty()?action:O({{L"op",S(L"gesture")},{L"phase",S(phase)},{L"action",action}});
}

// Layer ids can be reused after New/Open. A retained draft belongs to an epoch,
// layer and schema, not merely to the control's current position in the UI.
inline hstring propertyKindSignature(J const& control){
    auto kind=object(control,L"kind");
    return str(kind,L"kind")==L"choice"?O({{L"kind",S(L"choice")},{L"options_count",N(array(kind,L"options").Size())}}).Stringify():kind.Stringify();
}
struct Property:std::enable_shared_from_this<Property> {
    std::shared_ptr<WorkspaceData> data;
    double epoch,layer;
    hstring key,schema;
    Property(std::shared_ptr<WorkspaceData> source,J const& control):data(std::move(source)),
        epoch(num(object(data->state,L"document_file"),L"epoch")),
        layer(num(object(data->state,L"layer_properties"),L"layer")),key(str(control,L"key")),
        schema(propertyKindSignature(control)){}
    J view()const{return object(data->state,L"layer_properties");}
    J model()const{return find(array(view(),L"controls"),L"key",key);}
    LocalizedCopy label()const{auto resolve=[weak=weak_from_this()]{if(auto property=weak.lock())return str(property->model(),L"label");return hstring();};return {resolve(),resolve};}
    hstring identity()const{return O({{L"epoch",object(data->state,L"document_file").GetNamedValue(L"epoch",JsonValue::CreateNullValue())},{L"layer",view().GetNamedValue(L"layer",JsonValue::CreateNullValue())},{L"key",S(key)},{L"kind",S(propertyKindSignature(model()))}}).Stringify();}
    V value()const{return object(model(),L"value").GetNamedValue(L"value",JsonValue::CreateNullValue());}
    J curve()const{return object(model(),L"curve");}
    bool current()const{
        return epoch==num(object(data->state,L"document_file"),L"epoch")
            &&layer==num(view(),L"layer",-1)&&schema==propertyKindSignature(model());
    }
    void action(J operation,hstring const& phase={})const{
        // A captured preview must finish even if its view was hidden or disabled.
        // Rust validates the gesture owner; dispatchDocument guards its epoch.
        bool continuing=!phase.empty()&&phase!=L"down";
        if(!continuing&&(data->updating||!current()||!flag(view(),L"enabled")))return;
        operation.Insert(L"layer",N(layer));operation.Insert(L"key",S(key));
        data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",effectGesture(operation,phase)}}),to_hstring(uint64_t(epoch)));
    }
    J setting(V const& value)const{
        return O({{L"op",S(L"set")},{L"value",O({{L"kind",S(str(object(model(),L"kind"),L"kind"))},{L"value",value}})}});
    }
    void set(V const& value)const{action(setting(value));}
    void reset()const{action(O({{L"op",S(L"reset")}}));}
    hstring id()const{return L"property-"+key;}
};
FrameworkElement ColorField(std::shared_ptr<Property> const& property,hstring const& title,
    std::function<J()> get,std::function<void(J)> set,Bindings& bindings,
    std::function<hstring()> context={},std::function<hstring()> currentTitle={});
Button CompactColorField(std::shared_ptr<WorkspaceData> const& data,hstring const& id,std::function<hstring()> title,
    std::function<J()> get,std::function<void(J)> set,Bindings& bindings,std::function<hstring()> context);
FrameworkElement CurveField(std::shared_ptr<Property> const& property,Bindings& bindings);
struct GradientSource {
    std::function<J()> control;
    std::function<void(J,hstring)> send;
    std::function<bool()> enabled;
    hstring id;
};
FrameworkElement GradientEditor(std::shared_ptr<WorkspaceData> const& data,GradientSource source,Bindings& bindings);
FrameworkElement GradientRamp(std::shared_ptr<WorkspaceData> const& data,std::function<J()> gradient,double height,Bindings& bindings);
FrameworkElement GradientField(std::shared_ptr<Property> const& property,Bindings& bindings);
}
winrt::Microsoft::UI::Xaml::FrameworkElement PropertiesPanel(std::shared_ptr<CapyUi::WorkspaceData> const& data,CapyUi::Bindings& bindings);

winrt::Microsoft::UI::Xaml::FrameworkElement FiltersPanel(std::shared_ptr<CapyUi::WorkspaceData> const& data,CapyUi::Bindings& bindings,std::function<double()>* contentHeight=nullptr,std::function<CapyUi::J()>* scrollMetrics=nullptr,bool split=false);
winrt::Microsoft::UI::Xaml::FrameworkElement FilterTypesPanel(std::shared_ptr<CapyUi::WorkspaceData> const& data,CapyUi::Bindings& bindings);
