#pragma once
#include "UiControls.h"
#include <winrt/Windows.ApplicationModel.DataTransfer.h>

namespace CapyUi {
inline V colorUi(CapyLocalization const* localization,J const& request) {
    auto text=to_string(request.Stringify());
    std::unique_ptr<char,decltype(&capy_string_free)> raw(capy_color_ui(localization,text.c_str()),capy_string_free);
    if(!raw)throw hresult_error(E_OUTOFMEMORY);
    return JsonValue::Parse(to_hstring(raw.get()));
}
inline Windows::UI::Color previewColor(A const& a){
    if(a.Size()!=4)return {};
    auto byte=[&](int i){return uint8_t(std::round(std::clamp(a.GetNumberAt(i),0.,1.)*255));};
    return {byte(3),byte(0),byte(1),byte(2)};
}
inline Windows::UI::Color displayColor(J const& value){return previewColor(array(value,L"rgba"));}
inline void copyText(hstring const& text){
    Windows::ApplicationModel::DataTransfer::DataPackage package;package.SetText(text);
    Windows::ApplicationModel::DataTransfer::Clipboard::SetContent(package);
}
using ColorAccepted=std::function<void(J,std::optional<double>)>;
void EditColor(std::shared_ptr<WorkspaceData> const& data,UIElement const& owner,J const& target,bool opaque,ColorAccepted accepted);
}
