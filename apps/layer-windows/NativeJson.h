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
