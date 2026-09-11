#pragma once
#include "FilterPreviews.h"

namespace CapyUi {
struct LayerThumbnailCache;
std::shared_ptr<LayerThumbnailCache> CreateLayerThumbnailCache(PreviewTransport);
struct LayerThumbnail {
    winrt::hstring layer,target,revision;
    bool mask=false;
};
void RefreshLayerThumbnails(std::shared_ptr<LayerThumbnailCache> const&,
    winrt::hstring const& epoch,std::vector<LayerThumbnail> const& visible);
winrt::Microsoft::UI::Xaml::Media::ImageSource LayerThumbnailSource(
    std::shared_ptr<LayerThumbnailCache> const&,winrt::hstring const& epoch,LayerThumbnail const&);
}
