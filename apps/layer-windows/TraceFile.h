#pragma once
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <filesystem>
#include <fstream>
#include <string>

inline void WriteTraceFile(std::filesystem::path const& name,std::string const& value){
    auto pending=name.wstring()+L".pending";
    {std::ofstream stream(pending);stream<<value;if(!stream)return;}
    auto published=ReplaceFileW(name.c_str(),pending.c_str(),nullptr,0,nullptr,nullptr);
    auto error=GetLastError();
    if(!published&&error==ERROR_FILE_NOT_FOUND){
        published=MoveFileExW(pending.c_str(),name.c_str(),0);
        error=GetLastError();
    }
    if(!published){
        DeleteFileW(pending.c_str());
        OutputDebugStringW((std::wstring(L"Capy trace replacement failed: ")+std::to_wstring(error)+L"\n").c_str());
    }
}
