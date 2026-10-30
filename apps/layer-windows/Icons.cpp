#include "pch.h"
#include "UiControls.h"
#include <winrt/Windows.Storage.h>
#include <winrt/Windows.Storage.Streams.h>

using namespace CapyUi;
namespace {
struct IconSources {
    struct Entry {Imaging::SvgImageSource source{nullptr};bool ready=false,streaming=false;std::vector<std::pair<void*,weak_ref<Image>>> waiting;};
    struct Binding {weak_ref<Image> image;hstring key;};
    std::map<hstring,Entry> sources;
    std::map<void*,hstring> keys;
    std::map<void*,Binding> bindings;
    static IconSources& Current(){static thread_local IconSources cache;return cache;}
    static Windows::Foundation::Uri UriOf(Imaging::SvgImageSource const& source){
        if(auto uri=source.UriSource())return uri;
        auto& keys=Current().keys;auto found=keys.find(get_abi(source));
        return found==keys.end()?Windows::Foundation::Uri(nullptr):Windows::Foundation::Uri(found->second);
    }
    static void Ready(hstring const& key){
        auto& cache=Current();auto& entry=cache.sources.at(key);if(entry.ready)return;entry.ready=true;
        for(auto const& [id,weak]:std::exchange(entry.waiting,{})){
            auto image=weak.get();auto binding=cache.bindings.find(id);
            if(binding!=cache.bindings.end()&&binding->second.key==key&&binding->second.image.get()==image){cache.bindings.erase(binding);if(image)image.Source(entry.source);}
        }
    }
    static fire_and_forget Load(hstring key,std::filesystem::path path){
        auto& entry=Current().sources.at(key);if(entry.streaming)co_return;entry.streaming=true;auto source=entry.source;
        apartment_context apartment;co_await resume_background();
        try{
            auto file=co_await Windows::Storage::StorageFile::GetFileFromPathAsync(path.make_preferred().wstring());
            auto stream=co_await file.OpenReadAsync();co_await apartment;
            auto status=co_await source.SetSourceAsync(stream);stream.Close();
            if(status==Imaging::SvgImageSourceLoadStatus::Success)Ready(key);
        }catch(...){}
    }
    static Entry& Get(Windows::Foundation::Uri const& uri){
        auto key=uri.AbsoluteUri();auto& cache=Current();auto [found,added]=cache.sources.try_emplace(key);
        auto& entry=found->second;
        if(added){entry.source=Imaging::SvgImageSource(uri);cache.keys.emplace(get_abi(entry.source),key);entry.source.Opened([key](auto&&,auto&&){Ready(key);});}
        return entry;
    }
};
}
void CapyUi::setIcon(Image const& result,hstring name,hstring theme,double size){
    std::wstring file=name.c_str();
    if(!file.starts_with(L"layer-"))file=L"layer-"+file;
    if(!file.ends_with(L"-symbolic"))file+=L"-symbolic";
    result.Width(size);result.Height(size);result.IsHitTestVisible(false);
    auto relative=L"icons/"+std::wstring(theme.c_str())+L"/"+file+L".svg";auto uri=asset(relative);
    auto source=result.Source().try_as<Imaging::SvgImageSource>();
    auto key=uri.AbsoluteUri();auto& cache=IconSources::Current();auto pending=cache.bindings.find(get_abi(result));
    if(pending!=cache.bindings.end()&&pending->second.image.get()==result&&pending->second.key==key)return;
    if(source)if(auto previous=IconSources::UriOf(source);previous&&previous.AbsoluteUri()==key){cache.bindings.erase(get_abi(result));return;}
    auto& entry=IconSources::Get(uri);
    if(entry.ready||!result.Source()){cache.bindings.erase(get_abi(result));result.Source(entry.source);}
    else{cache.bindings.insert_or_assign(get_abi(result),IconSources::Binding{make_weak(result),key});entry.waiting.emplace_back(get_abi(result),make_weak(result));IconSources::Load(key,assetPath(relative));}
}
Windows::Foundation::Uri CapyUi::iconUri(Imaging::SvgImageSource const& source){return IconSources::UriOf(source);}
