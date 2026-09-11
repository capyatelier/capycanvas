#pragma once
#include "pch.h"
#include "native/include/capy_windows.h"
#include <functional>
#include <memory>
#include <string>
#include <vector>

using PreviewPacket=std::shared_ptr<CapyPreview>;
using PreviewReply=std::function<void(PreviewPacket)>;
using PreviewTransport=std::function<bool(std::string,PreviewReply)>;
// One request slot per window, separate from the reserved input command queue.
struct PreviewWork {std::string json; PreviewReply reply;};

namespace CapyUi {
struct FilterPreviewCache;
std::shared_ptr<FilterPreviewCache> CreateFilterPreviewCache(PreviewTransport);
void RefreshFilterPreviews(std::shared_ptr<FilterPreviewCache> const&,
    winrt::hstring const& context,int width,int height,std::vector<winrt::hstring> const& visible);
winrt::Microsoft::UI::Xaml::Media::ImageSource FilterPreviewSource(
    std::shared_ptr<FilterPreviewCache> const&,winrt::hstring const& context,int width,int height,winrt::hstring const& id);
}
