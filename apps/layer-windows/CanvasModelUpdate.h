#pragma once
#include "CanvasModelSections.h"
#include <map>
#include <optional>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Data.Json.h>

class CanvasModelUpdate {
    using J=winrt::Windows::Data::Json::JsonObject;
    using A=winrt::Windows::Data::Json::JsonArray;
    using V=winrt::Windows::Data::Json::IJsonValue;
    using Type=winrt::Windows::Data::Json::JsonValueType;
    struct Change {std::optional<V> value;std::map<winrt::hstring,Change> children;};
    J baseline{nullptr};
    static V replace(V const& value,Change const& change){
        if(change.value)return *change.value;
        if(value.ValueType()==Type::Object){
            J result;
            for(auto const& item:value.GetObject()){
                auto found=change.children.find(item.Key());
                result.Insert(item.Key(),found==change.children.end()?item.Value():replace(item.Value(),found->second));
            }
            return result;
        }
        if(value.ValueType()==Type::Array){
            A result;auto array=value.GetArray();
            for(uint32_t i=0;i<array.Size();++i){
                auto found=change.children.find(winrt::to_hstring(i));auto item=array.GetAt(i);
                result.Append(found==change.children.end()?item:replace(item,found->second));
            }
            return result;
        }
        throw winrt::hresult_invalid_argument(L"Invalid model update path");
    }
    static J copy(J const& object){J result;for(auto const& item:object)result.Insert(item.Key(),item.Value());return result;}
public:
    uint32_t Read(J const& packet){
        if(packet.HasKey(L"state")){
            baseline=copy(packet);
            if(baseline.HasKey(L"localization"))baseline.Remove(L"localization");
            return CanvasAllSections;
        }
        if(!packet.HasKey(L"model_update"))return 0;
        if(!baseline)throw winrt::hresult_invalid_argument(L"Model update without a baseline");
        Change changes;uint32_t sections=0;
        for(auto const& entry:packet.GetNamedArray(L"model_update")){
            auto pair=entry.GetArray();auto path=pair.GetArrayAt(0);auto* change=&changes;std::vector<std::wstring> names;
            for(auto const& segment:path){auto name=segment.GetString();names.emplace_back(name);change=&change->children[name];}
            change->value=pair.GetAt(1);sections|=CanvasSections(names);
        }
        baseline=replace(baseline,changes).GetObject();return sections;
    }
    J Model()const{
        auto model=copy(baseline);
        auto state=copy(model.GetNamedObject(L"state"));model.Insert(L"state",state);
        if(state.HasKey(L"workspace")){
            auto workspace=copy(state.GetNamedObject(L"workspace"));state.Insert(L"workspace",workspace);
            if(workspace.HasKey(L"layout"))workspace.Insert(L"layout",copy(workspace.GetNamedObject(L"layout")));
        }
        return model;
    }
};
