#pragma once
#include "pch.h"
#include <memory>
#include <optional>

namespace CapyUi {
struct WorkspaceData;
enum class ControlSurface { Panel, Card, Settings, Dialog, Drawing, Transparent };
enum class ChoiceFeedbackMode { Field, RowSuffix };
void refreshControlPalette(WorkspaceData&);
winrt::Microsoft::UI::Xaml::ResourceDictionary paletteResources();
winrt::Microsoft::UI::Xaml::Media::SolidColorBrush fieldBackground(std::shared_ptr<WorkspaceData> const&,ControlSurface);
void choiceColors(std::shared_ptr<WorkspaceData> const&,winrt::Microsoft::UI::Xaml::Controls::ComboBox const&,ControlSurface surface=ControlSurface::Panel,ChoiceFeedbackMode mode=ChoiceFeedbackMode::Field,std::optional<winrt::Windows::UI::Text::FontWeight> captionWeight={});
void fieldColors(std::shared_ptr<WorkspaceData> const&,winrt::Microsoft::UI::Xaml::Controls::TextBox const&,ControlSurface surface=ControlSurface::Panel);
void entryBaseline(winrt::Microsoft::UI::Xaml::Controls::TextBox const&,winrt::Microsoft::UI::Xaml::Controls::TextBlock const& reference=nullptr);
winrt::Microsoft::UI::Xaml::Controls::ToggleSwitch compactSwitch();
winrt::Microsoft::UI::Xaml::Controls::Button choiceRow(std::shared_ptr<WorkspaceData> const&,winrt::Microsoft::UI::Xaml::UIElement const&,winrt::Microsoft::UI::Xaml::Controls::ComboBox const&);
bool selectIndex(winrt::Microsoft::UI::Xaml::Controls::ComboBox const&,int32_t);
void clipRounded(winrt::Microsoft::UI::Xaml::FrameworkElement const&,winrt::Microsoft::UI::Xaml::CornerRadius const&);
void dialogButtonColors(std::shared_ptr<WorkspaceData> const&,winrt::Microsoft::UI::Xaml::Controls::ContentDialog const&);
void dialogLayout(std::shared_ptr<WorkspaceData> const&,winrt::Microsoft::UI::Xaml::Controls::ContentDialog const&);
}
