#pragma once
#include <cstdint>
#include <initializer_list>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Data.Json.h>

namespace CapyUi {
inline std::string CameraTracePayload(winrt::Windows::Data::Json::JsonObject const& packet,bool full,std::string const& identity){
    using namespace winrt::Windows::Data::Json;
    auto source=full?packet.GetNamedObject(L"state",JsonObject{}):packet;
    auto camera=source.GetNamedObject(L"camera",JsonObject{});
    if(!camera.Size())return {};
    auto result=identity+",\"camera\":"+winrt::to_string(camera.Stringify());
    auto tabValue=packet.GetNamedValue(L"windows_tabs",JsonValue::CreateNullValue());
    if(tabValue.ValueType()!=JsonValueType::Object)return result+"}";
    auto tabs=tabValue.GetObject();
    auto selected=tabs.GetNamedNumber(L"selected",0);
    for(auto value:tabs.GetNamedArray(L"session_stamps",JsonArray{})){
        auto entry=value.GetObject();
        if(entry.GetNamedNumber(L"id",-1)!=selected)continue;
        auto stamp=entry.GetNamedObject(L"stamp");
        JsonObject navigation;
        navigation.Insert(L"drawing_id",entry.GetNamedValue(L"id"));
        navigation.Insert(L"artwork",stamp.GetNamedValue(L"artwork"));
        navigation.Insert(L"epoch",stamp.GetNamedValue(L"epoch"));
        navigation.Insert(L"revision",stamp.GetNamedValue(L"camera_navigation"));
        result+=",\"navigation\":"+winrt::to_string(navigation.Stringify());
        break;
    }
    return result+"}";
}
inline bool JsonEqual(winrt::Windows::Data::Json::IJsonValue const& left,winrt::Windows::Data::Json::IJsonValue const& right){
    using Type=winrt::Windows::Data::Json::JsonValueType;
    if(!left||!right)return !left&&!right;
    if(left.ValueType()!=right.ValueType())return false;
    switch(left.ValueType()){
    case Type::Object:{
        auto a=left.GetObject(),b=right.GetObject();if(a.Size()!=b.Size())return false;
        for(auto const& entry:a)if(!b.HasKey(entry.Key())||!JsonEqual(entry.Value(),b.Lookup(entry.Key())))return false;
        return true;
    }
    case Type::Array:{
        auto a=left.GetArray(),b=right.GetArray();if(a.Size()!=b.Size())return false;
        for(uint32_t i=0;i<a.Size();++i)if(!JsonEqual(a.GetAt(i),b.GetAt(i)))return false;
        return true;
    }
    case Type::String:return left.GetString()==right.GetString();
    case Type::Number:return left.GetNumber()==right.GetNumber();
    case Type::Boolean:return left.GetBoolean()==right.GetBoolean();
    case Type::Null:return true;
    default:return false;
    }
}
inline bool ApplyColorPreview(winrt::Windows::Data::Json::JsonObject const& packet,winrt::Windows::Data::Json::JsonObject& preview){
    using namespace winrt::Windows::Data::Json;
    if(packet.HasKey(L"color_preview")){auto value=packet.GetNamedValue(L"color_preview");preview=value.ValueType()==JsonValueType::Object?value.GetObject():JsonObject{};return true;}
    return false;
}
inline std::string JsonUnsignedFields(std::string object,std::initializer_list<std::pair<std::string_view,uint64_t>> fields){
    if(object.empty()||object.front()!='{'||object.back()!='}')throw std::invalid_argument("Expected a JSON object");
    object.pop_back();
    for(auto const& [key,value]:fields){
        if(object.size()>1)object+=',';
        object+='"';object+=key;object+="\":";object+=std::to_string(value);
    }
    object+='}';return object;
}
}
