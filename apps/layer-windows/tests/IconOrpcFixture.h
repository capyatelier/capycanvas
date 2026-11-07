#pragma once
#include "UiControls.h"
#include "WindowRegistry.h"
#include <objbase.h>
#include <objidl.h>
#include <atomic>
#include <array>
#include <condition_variable>
#include <fstream>
#include <future>
#include <iomanip>
#include <mutex>
#include <sstream>
#include <thread>
#pragma comment(lib,"ole32.lib")

namespace CapyTests {
inline constexpr GUID IconChannelExtent{0x3b43bbec,0xa7b2,0x4b4e,{0xa1,0x30,0x51,0x53,0x5c,0x28,0xf9,0x2a}};
struct IconChannelHook:winrt::implements<IconChannelHook,IChannelHook> {
    DWORD ui=GetCurrentThreadId();
    std::atomic<unsigned> phase=0,calibration=0,icons=0;
    void STDMETHODCALLTYPE ClientGetSize(REFGUID,REFIID,ULONG* size)noexcept override{
        *size=0;if(GetCurrentThreadId()!=ui)return;
        switch(phase.load(std::memory_order_relaxed)){
        case 1:++calibration;break;
        case 2:++icons;break;
        }
    }
    void STDMETHODCALLTYPE ClientFillBuffer(REFGUID,REFIID,ULONG* size,void*)noexcept override{*size=0;}
    void STDMETHODCALLTYPE ClientNotify(REFGUID,REFIID,ULONG,void*,DWORD,HRESULT)noexcept override{}
    void STDMETHODCALLTYPE ServerNotify(REFGUID,REFIID,ULONG,void*,DWORD)noexcept override{}
    void STDMETHODCALLTYPE ServerGetSize(REFGUID,REFIID,HRESULT,ULONG* size)noexcept override{*size=0;}
    void STDMETHODCALLTYPE ServerFillBuffer(REFGUID,REFIID,ULONG* size,void*,HRESULT)noexcept override{*size=0;}
};
struct IconOrpcCalibration:winrt::implements<IconOrpcCalibration,IStream,winrt::non_agile> {
    std::atomic<DWORD>& called;std::atomic<int>& apartment;
    IconOrpcCalibration(std::atomic<DWORD>& value,std::atomic<int>& type):called(value),apartment(type){}
    HRESULT STDMETHODCALLTYPE Read(void*,ULONG,ULONG*)noexcept override{return E_NOTIMPL;}
    HRESULT STDMETHODCALLTYPE Write(void const*,ULONG,ULONG*)noexcept override{return E_NOTIMPL;}
    HRESULT STDMETHODCALLTYPE Seek(LARGE_INTEGER,DWORD,ULARGE_INTEGER*)noexcept override{return E_NOTIMPL;}
    HRESULT STDMETHODCALLTYPE SetSize(ULARGE_INTEGER)noexcept override{return E_NOTIMPL;}
    HRESULT STDMETHODCALLTYPE CopyTo(IStream*,ULARGE_INTEGER,ULARGE_INTEGER*,ULARGE_INTEGER*)noexcept override{return E_NOTIMPL;}
    HRESULT STDMETHODCALLTYPE Commit(DWORD)noexcept override{return E_NOTIMPL;}
    HRESULT STDMETHODCALLTYPE Revert()noexcept override{return E_NOTIMPL;}
    HRESULT STDMETHODCALLTYPE LockRegion(ULARGE_INTEGER,ULARGE_INTEGER,DWORD)noexcept override{return E_NOTIMPL;}
    HRESULT STDMETHODCALLTYPE UnlockRegion(ULARGE_INTEGER,ULARGE_INTEGER,DWORD)noexcept override{return E_NOTIMPL;}
    HRESULT STDMETHODCALLTYPE Stat(STATSTG* result,DWORD)noexcept override{
        if(!result)return E_POINTER;*result={};
        APTTYPE type;APTTYPEQUALIFIER qualifier;auto status=CoGetApartmentType(&type,&qualifier);
        if(FAILED(status))return status;called=GetCurrentThreadId();apartment=int(type);return S_OK;
    }
    HRESULT STDMETHODCALLTYPE Clone(IStream**)noexcept override{return E_NOTIMPL;}
};
struct IconOrpcFixture:std::enable_shared_from_this<IconOrpcFixture> {
    winrt::com_ptr<IconChannelHook> hook=winrt::make_self<IconChannelHook>();
    winrt::Microsoft::UI::Xaml::Window window{nullptr};
    winrt::Microsoft::UI::Xaml::Controls::StackPanel root{nullptr};
    std::array<CapyUi::Image,2> monitors{nullptr,nullptr},waiting{nullptr,nullptr};
    std::array<CapyUi::Imaging::SvgImageSource,2> placeholders{nullptr,nullptr};
    std::array<bool,2> ready{},pixels{};
    std::filesystem::path output;
    winrt::hstring fault;
    bool control=false,terminal=false;
    unsigned published=0,beforeClose=0;
    void close(){beforeClose=hook->icons.load();window.Close();}
    DWORD calibrationWorker=0,calibrationCalled=0;int calibrationApartment=-1;
    static void require(bool value,wchar_t const* message){if(!value)throw winrt::hresult_error(E_FAIL,message);}
    static winrt::hstring describe(winrt::hresult_error const& error){
        std::wostringstream text;text<<L"0x"<<std::hex<<std::setw(8)<<std::setfill(L'0')<<uint32_t(error.code())<<L": "<<error.message().c_str();return winrt::hstring(text.str());
    }
    IconOrpcFixture(){
        wchar_t path[32768];auto length=GetEnvironmentVariableW(L"CAPY_ICON_ORPC_FIXTURE",path,32768);
        require(length&&length<32768&&std::filesystem::path(path).is_absolute(),L"Icon ORPC output is not private and absolute");
        output=path;control=GetEnvironmentVariableW(L"CAPY_ICON_ORPC_CONTROL",nullptr,0)!=0;
    }
    void calibrate(){
        winrt::check_hresult(CoRegisterChannelHook(IconChannelExtent,hook.get()));
        struct Marshaled {winrt::com_ptr<IStream> stream;DWORD worker;};
        std::promise<Marshaled> promise;auto future=promise.get_future();std::atomic<DWORD> called=0;std::atomic<int> calledApartment=-1;std::atomic<bool> workerFault=false;
        std::jthread worker([&](std::stop_token stop){
            bool initialized=false,publishedResult=false;
            try{
                winrt::init_apartment(winrt::apartment_type::multi_threaded);initialized=true;
                {
                    auto object=winrt::make_self<IconOrpcCalibration>(called,calledApartment);
                    require(!object.try_as<IAgileObject>()&&!object.try_as<IMarshal>(),L"ORPC calibration object is agile");
                    winrt::com_ptr<IStream> stream;
                    winrt::check_hresult(CoMarshalInterThreadInterfaceInStream(__uuidof(IStream),object.get(),stream.put()));
                    promise.set_value({std::move(stream),GetCurrentThreadId()});publishedResult=true;
                    std::mutex mutex;std::condition_variable_any wake;std::unique_lock lock(mutex);
                    wake.wait(lock,stop,[]{return false;});
                }
                winrt::uninit_apartment();initialized=false;
            }catch(...){
                if(initialized)winrt::uninit_apartment();
                if(!publishedResult)promise.set_exception(std::current_exception());else workerFault=true;
            }
        });
        auto result=future.get();calibrationWorker=result.worker;winrt::com_ptr<IStream> proxy;
        winrt::check_hresult(CoGetInterfaceAndReleaseStream(result.stream.detach(),__uuidof(IStream),proxy.put_void()));
        hook->phase=1;STATSTG stat{};auto status=proxy->Stat(&stat,STATFLAG_NONAME);hook->phase=0;
        proxy=nullptr;worker.request_stop();worker.join();
        winrt::check_hresult(status);
        calibrationCalled=called;calibrationApartment=calledApartment;
        require(!workerFault&&calibrationCalled&&calibrationCalled!=hook->ui&&calibrationWorker!=hook->ui&&calibrationApartment==int(APTTYPE_MTA),L"ORPC calibration did not execute in its MTA");
        require(hook->calibration>0,L"Channel hook did not observe a real outgoing UI call");
    }
    void fail(winrt::hstring message){
        if(fault.empty())fault=message;
        if(window)close();winrt::Microsoft::UI::Xaml::Application::Current().Exit();
    }
    void changed(size_t index){
        if(ready[index]||!waiting[index].Source()||waiting[index].Source()==placeholders[index])return;
        if(waiting[index].Source()!=monitors[index].Source()){fail(L"Icon Ready published another source");return;}
        ready[index]=true;++published;
        if(published==2){terminal=true;close();winrt::Microsoft::UI::Xaml::Application::Current().Exit();}
    }
    void start(){
        APTTYPE type;APTTYPEQUALIFIER qualifier;winrt::check_hresult(CoGetApartmentType(&type,&qualifier));
        require(type==APTTYPE_STA||type==APTTYPE_MAINSTA,L"Icon loader was not entered on a UI STA");
        hook->phase=2;
        if(control){terminal=true;close();winrt::Microsoft::UI::Xaml::Application::Current().Exit();return;}
        auto weak=weak_from_this();double scale=root.XamlRoot().RasterizationScale();
        auto expected=uint32_t(std::ceil(17.5*std::max(2.,scale)));
        for(size_t i=0;i<2;++i){
            auto theme=i?L"dark":L"light";
            CapyUi::setIcon(monitors[i],L"undo",theme,17.5);
            auto source=monitors[i].Source().as<CapyUi::Imaging::SvgImageSource>();
            pixels[i]=source.RasterizePixelWidth()==expected&&source.RasterizePixelHeight()==expected;
            source.OpenFailed([weak](auto&&,auto&&){if(auto self=weak.lock())self->fail(L"Icon asset did not load");});
            CapyUi::setIcon(waiting[i],L"undo",theme,17.5);
        }
    }
    void open(){
        window=winrt::Microsoft::UI::Xaml::Window();root=winrt::Microsoft::UI::Xaml::Controls::StackPanel();root.Orientation(CapyUi::Orientation::Horizontal);
        auto weak=weak_from_this();
        for(size_t i=0;i<2;++i){
            monitors[i]=CapyUi::Image();waiting[i]=CapyUi::Image();placeholders[i]=CapyUi::Imaging::SvgImageSource();
            placeholders[i].RasterizePixelWidth(1);placeholders[i].RasterizePixelHeight(1);waiting[i].Source(placeholders[i]);
            root.Children().Append(monitors[i]);root.Children().Append(waiting[i]);
            waiting[i].RegisterPropertyChangedCallback(CapyUi::Image::SourceProperty(),[weak,i](auto&&,auto&&){if(auto self=weak.lock())self->changed(i);});
        }
        window.Content(root);window.Title(L"Capy Canvas icon ORPC regression");CapyUi::RegisterFixtureWindow(window);
        auto token=std::make_shared<winrt::event_token>();
        *token=root.Loaded([weak,token](auto&&,auto&&){if(auto self=weak.lock()){
            self->root.Loaded(*token);try{self->start();}catch(winrt::hresult_error const& error){self->fail(describe(error));}
        }});
        window.Activate();
    }
    int finish(winrt::hstring failure=L""){
        hook->phase=0;if(!failure.empty())fault=failure;
        bool valid=terminal&&hook->calibration>0&&(control||(published==2&&pixels[0]&&pixels[1]));
        if(!valid&&fault.empty())fault=L"Icon ORPC terminal precondition was not reached";
        int code=!fault.empty()||!valid?2:(beforeClose?1:0);
        if(code==1)fault=L"Icon load made an outgoing ORPC call on the UI STA";
        auto callbacks=hook->icons.load(),calibration=hook->calibration.load();bool saved=false;
        std::jthread writer([&]{
            std::ofstream result(output);result<<"{\"exit_code\":"<<code<<",\"control\":"<<(control?"true":"false")
                <<",\"terminal\":"<<(terminal?"true":"false")<<",\"ui_thread\":"<<hook->ui<<",\"calibration_worker\":"<<calibrationWorker
                <<",\"process_id\":"<<GetCurrentProcessId()<<",\"calibration_called_thread\":"<<calibrationCalled<<",\"calibration_apartment\":"<<calibrationApartment
                <<",\"calibration_callbacks\":"<<calibration<<",\"published_sources\":"<<published<<",\"pixels_match\":"<<((pixels[0]&&pixels[1])?"true":"false")
                <<",\"ui_outgoing_get_size_callbacks_before_close\":"<<beforeClose<<",\"ui_outgoing_get_size_callbacks\":"<<callbacks<<"}\n";
            saved=bool(result);
            if(!fault.empty()){std::ofstream error(std::filesystem::path(output.wstring()+L".error.txt"));error<<winrt::to_string(fault);saved=saved&&bool(error);}
        });writer.join();return saved?code:2;
    }
};
}
