#pragma once
#include "WorkspaceSurface.h"

namespace CapyUi {
inline ContourCommands bridgeCommands(A const& values){
    ContourCommands result;result.reserve(values.Size());
    for(auto value:values){auto row=value.GetArray();std::array<float,7> command{};
        for(uint32_t i=0;i<command.size();++i)command[i]=float(row.GetNumberAt(i));result.push_back(command);}
    return result;
}
inline void appendBridgeCommands(ContourCommands& result,ContourCommands const& source,float x,float y){
    for(auto command:source){int points=command[0]==2?3:command[0]==0||command[0]==1?1:0;
        for(int i=0;i<points;++i){command[1+i*2]+=x;command[2+i*2]+=y;}result.push_back(command);}
}
inline ContourCommands squircleCommands(float width,float height,std::array<float,4> radii){
    ContourCommands result;auto points=squircleOutline(width,height,radii);
    for(size_t i=0;i<points.size();++i)result.push_back({i?1.f:0.f,points[i].X,points[i].Y,0,0,0,0});
    if(!result.empty())result.push_back({3,0,0,0,0,0,0});return result;
}
}
