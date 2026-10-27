#pragma once
#include <cstdint>
#include <initializer_list>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>

namespace CapyUi {
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
