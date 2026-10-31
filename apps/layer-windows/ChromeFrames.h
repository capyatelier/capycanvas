#pragma once
#include "pch.h"
#include <map>
#include <set>
#include <functional>
#include <vector>
#include <utility>

namespace CapyUi {
struct ChromeFrames {
    using Array=winrt::Windows::Data::Json::JsonArray;
    using Object=winrt::Windows::Data::Json::JsonObject;
    struct Frame {
        std::set<uint32_t> groups,columns;
        std::set<void*> elements;
        std::map<void*,Object> views;
        Array overviews;
        Object document;
    };
    Frame current,presented;
    std::map<uint64_t,Frame> sent;
    std::map<void*,std::function<void()>> retiring,observers;
    winrt::weak_ref<winrt::Microsoft::UI::Xaml::FrameworkElement> root;
    uint64_t revision=0,sentRevision=0;
    bool closed=false;
    static void* Id(winrt::Windows::Foundation::IInspectable const& element){return element.as<::IUnknown>().get();}
    void Track(winrt::Windows::Foundation::IInspectable const& element){current.elements.insert(Id(element));}
    bool RetainsId(void* id)const{
        if(closed)return false;
        if(presented.elements.contains(id))return true;
        for(auto const& [revision,frame]:sent)if(frame.elements.contains(id))return true;
        return false;
    }
    bool Retains(winrt::Windows::Foundation::IInspectable const& element)const{return RetainsId(Id(element));}
    bool Group(uint32_t id)const{return current.groups.contains(id)&&presented.groups.contains(id);}
    bool Column(uint32_t id)const{return current.columns.contains(id)&&presented.columns.contains(id);}
    bool Sent(uint64_t next){
        if(closed)return false;
        sentRevision=next;sent.insert_or_assign(next,current);
        while(sent.size()>16&&sent.begin()->first<next)sent.erase(sent.begin());
        if(next!=revision||(current.elements==presented.elements&&current.groups==presented.groups&&current.columns==presented.columns))return false;
        return Present(next,presented.document);
    }
    bool Present(uint64_t next,Object document){
        if(closed)return false;
        auto found=sent.find(next);if(found==sent.end()||next<revision)return false;
        presented=found->second;presented.document=document;revision=next;
        sent.erase(sent.begin(),found);
        std::vector<std::function<void()>> release;
        for(auto it=retiring.begin();it!=retiring.end();)if(!RetainsId(it->first)){
            release.push_back(std::move(it->second));it=retiring.erase(it);
        }else ++it;
        for(auto& ready:release)ready();
        for(auto const& [id,refresh]:observers)refresh();
        return true;
    }
    void Reset(){presented=Frame{};revision=0;for(auto const& [id,refresh]:observers)refresh();}
    void Close(){if(closed)return;closed=true;current=Frame{};presented=Frame{};sent.clear();auto release=std::exchange(retiring,{});for(auto& [id,ready]:release)ready();observers.clear();}
    Object View(winrt::Windows::Foundation::IInspectable const& element)const{
        auto found=presented.views.find(Id(element));return found==presented.views.end()?Object{}:found->second;
    }
};
}
