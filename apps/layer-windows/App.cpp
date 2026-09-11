#include "pch.h"
#include "CanvasWindow.h"
#include <fstream>
#include <winrt/Microsoft.UI.Xaml.XamlTypeInfo.h>
#include <winrt/Windows.UI.Xaml.Interop.h>

namespace {
using namespace winrt;
using namespace Microsoft::UI::Xaml;
// Use WinUI's metadata provider for its native controls and built-in templates.
// Workspace controls are composed directly from the shared Rust models.
struct App : ApplicationT<App, Markup::IXamlMetadataProvider> {
    Microsoft::UI::Xaml::XamlTypeInfo::XamlControlsXamlMetaDataProvider metadata{nullptr};
    std::shared_ptr<CanvasWindow> window;
    App() {
        UnhandledException([](auto&&, UnhandledExceptionEventArgs const& event) {
            if(!GetEnvironmentVariableW(L"CAPY_TEST_DISPLAY",nullptr,0))return;
            std::ofstream log("startup-error.log");
            log << std::hex << uint32_t(event.Exception().value) << " " << to_string(event.Message()) << std::endl;
        });
        metadata=Microsoft::UI::Xaml::XamlTypeInfo::XamlControlsXamlMetaDataProvider();

    }
    Markup::IXamlType GetXamlType(Windows::UI::Xaml::Interop::TypeName const& type) {
        return metadata.GetXamlType(type);
    }
    Markup::IXamlType GetXamlType(hstring const& name) { return metadata.GetXamlType(name); }
    com_array<Markup::XmlnsDefinition> GetXmlnsDefinitions() { return metadata.GetXmlnsDefinitions(); }
    void OnLaunched(LaunchActivatedEventArgs const&) {
        Resources().MergedDictionaries().Append(Controls::XamlControlsResources());
        window=std::make_shared<CanvasWindow>();
        window->Open();
    }
};
}
int WINAPI wWinMain(HINSTANCE,HINSTANCE,PWSTR,int) {
    winrt::init_apartment(winrt::apartment_type::single_threaded);
    try {
        winrt::Microsoft::UI::Xaml::Application::Start([](auto&&){winrt::make<App>();});
        CapyLifecycle("application_returned");
        return 0;
    } catch(winrt::hresult_error const& error) {
        MessageBoxW(nullptr,error.message().c_str(),L"Capy Canvas could not start",MB_OK|MB_ICONERROR);
        return 1;
    }
}
