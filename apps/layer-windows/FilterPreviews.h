#pragma once
#include "pch.h"
#include "NativeHandles.h"
#include <functional>
#include <memory>
#include <string>
#include <vector>

#include "CanvasQueryQueue.h"

namespace CapyUi {
struct FilterPreviewCache;
std::shared_ptr<FilterPreviewCache> CreateFilterPreviewCache(PreviewTransport);
void RefreshFilterPreviewContext(std::shared_ptr<FilterPreviewCache> const&,winrt::hstring const& epoch);
void RefreshFilterPreviews(std::shared_ptr<FilterPreviewCache> const&,
    uint64_t view,winrt::hstring const& epoch,std::vector<winrt::hstring> const& visible);
void RemoveFilterPreviewView(std::shared_ptr<FilterPreviewCache> const&,uint64_t view);
winrt::Microsoft::UI::Xaml::Media::ImageSource FilterPreviewSource(
    std::shared_ptr<FilterPreviewCache> const&,winrt::hstring const& epoch,winrt::hstring const& id,bool* current=nullptr);
}
