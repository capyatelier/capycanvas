#pragma once
#include <cstdint>
#include <string>
#include <vector>

enum CanvasModelSection : uint32_t {
    CanvasWorkspace=1,CanvasHeader=2,CanvasWindowState=4,CanvasDialogs=8,CanvasScopes=16,
    CanvasGlass=32,CanvasAllSections=63
};
inline uint32_t CanvasSections(std::vector<std::wstring> const& path){
    if(path.empty())return CanvasAllSections;
    auto const& field=path[0];
    if(field==L"workspace_update")return 0;
    if(field==L"windows_scopes")return CanvasScopes;
    if(field==L"windows_glass")return CanvasGlass;
    if(field==L"windows_workspace"||field==L"windows_tabs"||field==L"windows_recovery")
        return CanvasHeader|CanvasWindowState|CanvasDialogs;
    if(field==L"windows_settings_close"||field==L"windows_document"||field==L"windows_filter_load"||field==L"windows_request_copy")
        return CanvasWindowState|CanvasDialogs;
    if(field==L"header"||field==L"application_menus"||field==L"header_presentation"||field==L"header_presentation_height")return CanvasHeader;
    if(field==L"state"&&path.size()>1){
        auto const& state=path[1];
        if(state==L"revision")return 0;
        if(state==L"histogram"||state==L"waveform"||state==L"tonal_histogram")return CanvasScopes;
    }
    return CanvasAllSections;
}
