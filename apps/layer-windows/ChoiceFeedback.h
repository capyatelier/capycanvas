#pragma once
#include "pch.h"
#include <memory>

namespace CapyUi {
struct WorkspaceData;
void choiceColors(std::shared_ptr<WorkspaceData> const&,winrt::Microsoft::UI::Xaml::Controls::ComboBox const&,bool drawing=false);
void dialogButtonColors(std::shared_ptr<WorkspaceData> const&,winrt::Microsoft::UI::Xaml::Controls::ContentDialog const&);
}
