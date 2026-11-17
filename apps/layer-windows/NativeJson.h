#pragma once
#include <algorithm>
#include <cstdint>
#include "CanvasModelInputs.h"
#include <initializer_list>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Data.Json.h>

namespace CapyUi {
using winrt::hstring;
using winrt::Windows::Data::Json::JsonValue;
using winrt::Windows::Data::Json::JsonValueType;
using J=winrt::Windows::Data::Json::JsonObject;
using A=winrt::Windows::Data::Json::JsonArray;
using V=winrt::Windows::Data::Json::IJsonValue;
inline J copyObject(J const& value){J result;for(auto const& item:value)result.Insert(item.Key(),item.Value());return result;}

struct CameraTrace {
    std::optional<J> navigation;
    double cameraRevision=0,automaticRevisions=0;
    std::string Payload(J const& packet,bool full,std::string const& identity){
        auto source=full?packet.GetNamedObject(L"state",J{}):packet;
        auto camera=source.GetNamedObject(L"camera",J{});
        if(camera.Size())cameraRevision=camera.GetNamedNumber(L"revision");
        if(packet.HasKey(L"windows_tabs")){
            navigation.reset();
            auto tabValue=packet.GetNamedValue(L"windows_tabs");
            auto tabs=tabValue.ValueType()==JsonValueType::Object?tabValue.GetObject():J{};
            auto selected=tabs.GetNamedNumber(L"selected",0);
            for(auto value:tabs.GetNamedArray(L"session_stamps",A{})){
                auto entry=value.GetObject();
                if(entry.GetNamedNumber(L"id",-1)!=selected)continue;
                auto stamp=entry.GetNamedObject(L"stamp");
                navigation=J{};
                navigation->Insert(L"drawing_id",entry.GetNamedValue(L"id"));
                navigation->Insert(L"artwork",stamp.GetNamedValue(L"artwork"));
                navigation->Insert(L"epoch",stamp.GetNamedValue(L"epoch"));
                automaticRevisions=cameraRevision-stamp.GetNamedNumber(L"camera_navigation");
                break;
            }
        }
        if(!camera.Size())return {};
        auto result=identity+",\"camera\":"+winrt::to_string(camera.Stringify());
        if(navigation){
            navigation->Insert(L"revision",JsonValue::CreateNumberValue(cameraRevision-automaticRevisions));
            result+=",\"navigation\":"+winrt::to_string(navigation->Stringify());
        }
        return result+"}";
    }
};
using ::JsonEqual;
inline void CanonicalJson(std::string_view text,size_t& at,std::string& out){
    auto space=[&]{while(at<text.size()&&std::string_view(" \t\r\n").find(text[at])!=std::string_view::npos)++at;};
    auto token=[&]{
        auto start=at;
        if(text[at]=='"'){for(++at;at<text.size()&&text[at]!='"';++at)if(text[at]=='\\')++at;++at;}
        else while(at<text.size()&&std::string_view(",:]} \t\r\n").find(text[at])==std::string_view::npos)++at;
        if(at>text.size()||at==start)throw std::invalid_argument("Invalid JSON text");
        return text.substr(start,at-start);
    };
    space();
    if(at>=text.size())throw std::invalid_argument("Invalid JSON text");
    auto open=text[at],close=open=='{'?'}':']';
    if(open!='{'&&open!='['){out+=token();return;}
    std::vector<std::pair<std::string_view,std::string>> members;
    for(++at,space();at>=text.size()||text[at]!=close;space()){
        auto& member=members.emplace_back();
        if(open=='{'){
            if(at>=text.size()||text[at]!='"')throw std::invalid_argument("Invalid JSON member");
            member.first=token();space();
            if(at>=text.size()||text[at++]!=':')throw std::invalid_argument("Invalid JSON member");
        }
        CanonicalJson(text,at,member.second);space();
        if(at<text.size()&&text[at]==',')++at;
        else if(at>=text.size()||text[at]!=close)throw std::invalid_argument("Invalid JSON text");
    }
    ++at;
    if(open=='{')std::sort(members.begin(),members.end(),[](auto const& a,auto const& b){return a.first<b.first;});
    out+=open;
    for(size_t i=0;i<members.size();++i){
        if(i)out+=',';
        if(open=='{'){out+=members[i].first;out+=':';}
        out+=members[i].second;
    }
    out+=close;
}
inline std::string CanonicalJson(std::string_view text){
    size_t at=0;std::string out;out.reserve(text.size());CanonicalJson(text,at,out);
    if(text.find_first_not_of(" \t\r\n",at)!=std::string_view::npos)throw std::invalid_argument("Invalid JSON text");
    return out;
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
class JsonTraceHistory {
    std::string samples;
    size_t count=0;
public:
    void Append(std::string_view sample){if(count++)samples+=',';samples+=sample;}
    size_t Size()const{return count;}
    size_t Bytes()const{return samples.size();}
    void AppendArray(std::string& result,std::string_view tail={})const{
        result.reserve(result.size()+samples.size()+tail.size()+3);result+='[';result+=samples;
        if(!tail.empty()){if(count)result+=',';result+=tail;}result+=']';
    }
    std::string Array(std::string_view tail={})const{std::string result;AppendArray(result,tail);return result;}
};
inline V named(J const& value,wchar_t const* key){CanvasModelInputs::Capture::Read(value,key);return value.GetNamedValue(key,JsonValue::CreateNullValue());}
inline J object(J const& value,wchar_t const* key){
    auto item=named(value,key);
    auto result=item.ValueType()==JsonValueType::Object?item.GetObject():J{};if(CanvasModelInputs::Capture::Active())CanvasModelInputs::Capture::Alias(result,value,{key});return result;
}
inline J branch(J const& value,wchar_t const* key){
    auto item=value.GetNamedValue(key,JsonValue::CreateNullValue());auto result=item.ValueType()==JsonValueType::Object?item.GetObject():J{};
    if(CanvasModelInputs::Capture::Active()){if(!result.Size())CanvasModelInputs::Capture::Read(value,key);CanvasModelInputs::Capture::Alias(result,value,{key});}
    return result;
}
inline A array(J const& value,wchar_t const* key){
    auto item=named(value,key);
    return item.ValueType()==JsonValueType::Array?item.GetArray():A{};
}
inline uint32_t count(J const& value,wchar_t const* field){
    if(CanvasModelInputs::Capture::Active())CanvasModelInputs::Capture::ReadPath(value,{field,L"length"});
    return value.GetNamedArray(field,A{}).Size();
}
inline J objectAt(A const& values,int32_t index){
    if(index<0||uint32_t(index)>=values.Size())return {};auto item=values.GetAt(index);return item.ValueType()==JsonValueType::Object?item.GetObject():J{};
}
inline hstring str(J const& value,wchar_t const* key,hstring fallback=L""){
    auto item=named(value,key);
    return item.ValueType()==JsonValueType::String?item.GetString():fallback;
}
inline double num(J const& value,wchar_t const* key,double fallback=0){
    auto item=named(value,key);
    return item.ValueType()==JsonValueType::Number?item.GetNumber():fallback;
}
inline bool flag(J const& value,wchar_t const* key,bool fallback=false){CanvasModelInputs::Capture::Read(value,key);return value.GetNamedBoolean(key,fallback);}
inline J find(A const& list,wchar_t const* key,hstring const& id){
    for(auto value:list){auto row=value.GetObject();if(str(row,key)==id)return row;}return J{};
}
inline J find(J const& model,wchar_t const* field,wchar_t const* key,hstring const& id){
    auto list=model.GetNamedArray(field,A{});
    bool recording=CanvasModelInputs::Capture::Active();
    for(uint32_t i=0;i<list.Size();++i){auto row=list.GetObjectAt(i);if(recording)CanvasModelInputs::Capture::ReadPath(model,{field,std::to_wstring(i),key});if(str(row,key)==id){if(recording)CanvasModelInputs::Capture::Alias(row,model,{field,std::to_wstring(i)});return row;}}
    CanvasModelInputs::Capture::Read(model,field);return {};
}
inline J findId(J const& model,wchar_t const* field,double id){
    auto list=model.GetNamedArray(field,A{});
    bool recording=CanvasModelInputs::Capture::Active();
    for(uint32_t i=0;i<list.Size();++i){auto row=list.GetObjectAt(i);if(recording)CanvasModelInputs::Capture::ReadPath(model,{field,std::to_wstring(i),L"id"});if(num(row,L"id")==id){if(recording)CanvasModelInputs::Capture::Alias(row,model,{field,std::to_wstring(i)});return row;}}
    CanvasModelInputs::Capture::Read(model,field);return {};
}
inline double num(J const& model,std::initializer_list<wchar_t const*> fields,double fallback=0){
    if(CanvasModelInputs::Capture::Active()){CanvasModelChanges::Path path;for(auto field:fields)path.emplace_back(field);CanvasModelInputs::Capture::ReadPath(model,path);}
    auto current=model;V value=JsonValue::CreateNullValue();
    for(auto it=fields.begin();it!=fields.end();++it){value=current.GetNamedValue(*it,JsonValue::CreateNullValue());if(it+1!=fields.end()){if(value.ValueType()!=JsonValueType::Object)return fallback;current=value.GetObject();}}
    return value.ValueType()==JsonValueType::Number?value.GetNumber():fallback;
}
inline J findId(A const& list,double id){
    for(auto value:list){auto row=value.GetObject();if(num(row,L"id")==id)return row;}return J{};
}
}
