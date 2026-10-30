#pragma once
#include "FilterPreviews.h"
#include <chrono>

namespace CapyUi {
struct LayerThumbnailCache;
std::shared_ptr<LayerThumbnailCache> CreateLayerThumbnailCache(PreviewTransport);
struct LayerThumbnail {
    winrt::hstring layer,target,revision;
    bool mask=false;
    bool operator==(LayerThumbnail const&)const=default;
};
std::chrono::milliseconds RefreshLayerThumbnails(std::shared_ptr<LayerThumbnailCache> const&,
    void const* owner,winrt::hstring const& epoch,std::vector<LayerThumbnail> const& visible);
void ForgetLayerThumbnails(std::shared_ptr<LayerThumbnailCache> const&,void const* owner);
winrt::Microsoft::UI::Xaml::Media::ImageSource LayerThumbnailSource(
    std::shared_ptr<LayerThumbnailCache> const&,winrt::hstring const& epoch,LayerThumbnail const&,bool* current=nullptr);
}
