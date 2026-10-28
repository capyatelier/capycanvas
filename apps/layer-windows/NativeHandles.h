#pragma once
#include "native/include/capy_windows.h"
#include <algorithm>
#include <memory>
#include <mutex>
#include <span>
#include <stdexcept>
#include <string>
#include <vector>

template<auto Release> class NativeHandle {
    struct Owner {
        uint64_t value;
        explicit Owner(uint64_t value):value(value){}
        ~Owner(){Release(value);}
    };
    std::shared_ptr<Owner> owner;
public:
    NativeHandle()=default;
    NativeHandle(std::nullptr_t){}
    explicit NativeHandle(uint64_t value,decltype(Release)=Release){
        if(value)try{owner=std::make_shared<Owner>(value);}catch(...){Release(value);throw;}
    }
    uint64_t get()const{return owner?owner->value:0;}
    explicit operator bool()const{return bool(owner);}
    void reset(){owner.reset();}
};
using NativeLaunch=NativeHandle<capy_launch_free>;
using NativeLocalization=NativeHandle<capy_localization_free>;
using NativePanelLayout=NativeHandle<capy_panel_layout_free>;

struct NativeString {
    std::string bytes;
    bool valid;
    explicit NativeString(CapyString value,decltype(&capy_string_free) release=capy_string_free):valid(value!=0){
        struct Owner {CapyString value;decltype(release) free;~Owner(){if(value)free(value);}} owner{value,release};
        if(!value)return;
        bytes.resize(capy_string_length(value));
        for(size_t offset=0;offset<bytes.size();){
            auto count=std::min<size_t>(8*1024*1024,bytes.size()-offset);
            if(!capy_string_copy(value,offset,reinterpret_cast<uint8_t*>(bytes.data()+offset),count))
                throw std::invalid_argument("Expired native text");
            offset+=count;
        }
        if(!capy_string_copy(value,bytes.size(),nullptr,0))throw std::invalid_argument("Expired native text");
    }
    char const* get()const{return bytes.c_str();}
    explicit operator bool()const{return valid;}
};

class NativePreview:public NativeHandle<capy_preview_free> {
    struct Copy {std::once_flag once;std::vector<uint8_t> bytes;};
    std::shared_ptr<Copy> copied;
public:
    NativePreview()=default;
    NativePreview(std::nullptr_t){}
    explicit NativePreview(CapyPreview value,decltype(&capy_preview_free) release=capy_preview_free):
        NativeHandle(value,release),copied(value?std::make_shared<Copy>():nullptr){}
    void reset(){NativeHandle::reset();copied.reset();}
    std::span<uint8_t const> Bytes()const{
        if(!copied)return {};
        std::call_once(copied->once,[this]{
            auto& bytes=copied->bytes;bytes.resize(capy_preview_length(get()));
            for(size_t offset=0;offset<bytes.size();){
                auto count=std::min<size_t>(8*1024*1024,bytes.size()-offset);
                if(!capy_preview_copy(get(),offset,bytes.data()+offset,count))throw std::invalid_argument("Expired native preview");
                offset+=count;
            }
            if(!capy_preview_copy(get(),bytes.size(),nullptr,0))throw std::invalid_argument("Expired native preview");
        });
        return copied->bytes;
    }
};
