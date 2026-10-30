#pragma once
#include "WorkspaceSurface.h"

namespace CapyUi {
inline ContourCommands bridgeCommands(A const& values){
    ContourCommands result;result.reserve(values.Size());
    for(auto value:values){auto row=value.GetArray();std::array<float,7> command{};
        for(uint32_t i=0;i<command.size();++i)command[i]=float(row.GetNumberAt(i));result.push_back(command);}
    return result;
}
}
