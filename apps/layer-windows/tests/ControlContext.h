#pragma once
#include "UiControls.h"
#include <thread>
struct ControlContext {
    NativeLocalization localization;
    std::string bootstrap;
    void apply(std::shared_ptr<CapyUi::WorkspaceData> const& data,CapyUi::J const& catalog={})const{
        using namespace CapyUi;
        if(!data->adoptLocalization(O({{L"localization",O({{L"generation",N(0)},{L"catalog",J::Parse(catalog.Stringify())},{L"bootstrap",J::Parse(winrt::to_hstring(bootstrap))}})}})))
            throw winrt::hresult_error(E_FAIL,L"Fixture localization was not adopted");
    }
};
inline ControlContext fixtureContext(wchar_t const* variable){
    wchar_t path[32768];auto length=GetEnvironmentVariableW(variable,path,32768);
    if(!length||length>=32768||!std::filesystem::path(path).is_absolute())throw winrt::hresult_invalid_argument();
    auto directory=std::filesystem::path(path).parent_path()/(L"native-context-"+std::to_wstring(GetCurrentProcessId()));
    winrt::check_bool(SetEnvironmentVariableW(L"CAPY_STORAGE_DIR",directory.c_str()));
    ControlContext context;
    std::jthread startup([&]{
        NativeLaunch launch(capy_launch("[\"en\"]",nullptr),capy_launch_free);
        if(launch){
            context.localization=NativeLocalization(capy_launch_localization(launch.get()),capy_localization_free);
            NativeString bootstrap(capy_launch_view(launch.get()));if(bootstrap)context.bootstrap=bootstrap.bytes;
        }
    });
    startup.join();if(!context.localization||context.bootstrap.empty())throw winrt::hresult_error(E_FAIL);
    return context;
}
