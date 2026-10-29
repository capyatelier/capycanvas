#pragma once
#include "pch.h"
#include <memory>

namespace CapyUi {
struct WorkspaceData;
enum class ControlSurface { Panel, Card, Settings, Dialog, Drawing };
void refreshControlPalette(WorkspaceData&);
winrt::Microsoft::UI::Xaml::ResourceDictionary paletteResources();
void choiceColors(std::shared_ptr<WorkspaceData> const&,winrt::Microsoft::UI::Xaml::Controls::ComboBox const&,ControlSurface surface=ControlSurface::Panel);
void fieldColors(std::shared_ptr<WorkspaceData> const&,winrt::Microsoft::UI::Xaml::Controls::TextBox const&,ControlSurface surface=ControlSurface::Panel);
bool selectIndex(winrt::Microsoft::UI::Xaml::Controls::ComboBox const&,int32_t);
void clipRounded(winrt::Microsoft::UI::Xaml::FrameworkElement const&,winrt::Microsoft::UI::Xaml::CornerRadius const&);
void dialogButtonColors(std::shared_ptr<WorkspaceData> const&,winrt::Microsoft::UI::Xaml::Controls::ContentDialog const&);
}
