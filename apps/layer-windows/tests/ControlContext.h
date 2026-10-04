#pragma once
#include "UiControls.h"
#include <thread>
inline std::shared_ptr<CapyLocalization> fixtureContext(wchar_t const* variable){
    wchar_t path[32768];auto length=GetEnvironmentVariableW(variable,path,32768);
    if(!length||length>=32768||!std::filesystem::path(path).is_absolute())throw winrt::hresult_invalid_argument();
    auto directory=std::filesystem::path(path).parent_path()/(L"native-context-"+std::to_wstring(GetCurrentProcessId()));
    winrt::check_bool(SetEnvironmentVariableW(L"CAPY_STORAGE_DIR",directory.c_str()));
    std::shared_ptr<CapyLocalization> context;
    std::jthread startup([&]{
        std::unique_ptr<CapyLaunch,decltype(&capy_launch_free)> launch(capy_launch("[\"en\"]"),capy_launch_free);
        if(launch)context=std::shared_ptr<CapyLocalization>(capy_launch_localization(launch.get()),capy_localization_free);
    });
    startup.join();if(!context)throw winrt::hresult_error(E_FAIL);
    return context;
}
