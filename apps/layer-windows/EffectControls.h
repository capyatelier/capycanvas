#pragma once
#include "UiControls.h"
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

// Layer ids can be reused after New/Open. A retained draft belongs to an epoch,
// layer and schema, not merely to the control's current position in the UI.
struct Property {
    std::shared_ptr<WorkspaceData> data;
    double epoch,layer;
    hstring key,schema;
    Property(std::shared_ptr<WorkspaceData> source,J const& control):data(std::move(source)),
        epoch(num(object(data->state,L"document_file"),L"epoch")),
        layer(num(object(data->state,L"layer_properties"),L"layer")),key(str(control,L"key")),
        schema(object(control,L"kind").Stringify()){}
    J view()const{return object(data->state,L"layer_properties");}
    J model()const{return find(array(view(),L"controls"),L"key",key);}
    V value()const{return object(model(),L"value").GetNamedValue(L"value",JsonValue::CreateNullValue());}
    bool current()const{
        return epoch==num(object(data->state,L"document_file"),L"epoch")
            &&layer==num(view(),L"layer",-1)&&schema==object(model(),L"kind").Stringify();
    }
    void action(J operation)const{
        if(data->updating||!current()||!flag(view(),L"enabled"))return;
        operation.Insert(L"layer",N(layer));operation.Insert(L"key",S(key));
        data->dispatchDocument(O({{L"type",S(L"effect")},{L"action",operation}}),to_hstring(uint64_t(epoch)));
    }
    void set(V const& value)const{
        action(O({{L"op",S(L"set")},{L"value",O({{L"kind",S(str(object(model(),L"kind"),L"kind"))},{L"value",value}})}}));
    }
    void reset()const{action(O({{L"op",S(L"reset")}}));}
    hstring id()const{return L"property-"+key;}
};
FrameworkElement ColorField(std::shared_ptr<Property> const& property,hstring const& title,
    std::function<A()> get,std::function<void(A)> set,Bindings& bindings,
    std::function<hstring()> context={});
FrameworkElement CurveField(std::shared_ptr<Property> const& property,Bindings& bindings);
FrameworkElement GradientField(std::shared_ptr<Property> const& property,Bindings& bindings);
}
winrt::Microsoft::UI::Xaml::FrameworkElement PropertiesPanel(std::shared_ptr<CapyUi::WorkspaceData> const& data,CapyUi::Bindings& bindings);

winrt::Microsoft::UI::Xaml::FrameworkElement FiltersPanel(std::shared_ptr<CapyUi::WorkspaceData> const& data,CapyUi::Bindings& bindings);
