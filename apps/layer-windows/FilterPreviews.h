#pragma once
#include "pch.h"
#include "native/include/capy_windows.h"
#include <functional>
#include <memory>
#include <string>
#include <vector>

#include "CanvasQueryQueue.h"

namespace CapyUi {
struct FilterPreviewCache;
std::shared_ptr<FilterPreviewCache> CreateFilterPreviewCache(PreviewTransport);
void RefreshFilterPreviews(std::shared_ptr<FilterPreviewCache> const&,
    winrt::hstring const& context,int width,int height,std::vector<winrt::hstring> const& visible);
winrt::Microsoft::UI::Xaml::Media::ImageSource FilterPreviewSource(
    std::shared_ptr<FilterPreviewCache> const&,winrt::hstring const& context,int width,int height,winrt::hstring const& id);
}
