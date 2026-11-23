#include "pch.h"
#include "StampPreview.h"
#include <robuffer.h>
#include <winrt/Windows.Storage.Streams.h>

namespace CapyUi {
static fire_and_forget ConvertStamp(Microsoft::UI::Dispatching::DispatcherQueue queue,PreviewPacket packet,J result,
    winrt::Windows::UI::Color ink,StampReply reply,agile_ref<Imaging::WriteableBitmap> bitmap,
    agile_ref<winrt::Windows::Storage::Streams::IBuffer> buffer,uint8_t* pixels,size_t expected){
    co_await resume_background();
    bool valid=true;
    try{
        auto mask=packet.Bytes();auto count=mask.size();
        if(count!=expected)throw hresult_invalid_argument();
        for(size_t i=0;i<count;++i){auto alpha=uint32_t(mask[i])*ink.A/255;auto p=pixels+i*4;
            p[0]=uint8_t(ink.B*alpha/255);p[1]=uint8_t(ink.G*alpha/255);p[2]=uint8_t(ink.R*alpha/255);p[3]=uint8_t(alpha);}
    }catch(...){valid=false;}
    packet.reset();
    queue.TryEnqueue([reply=std::move(reply),result,bitmap,buffer,valid]{
        auto image=valid?bitmap.get():nullptr;if(image)image.Invalidate();reply(result,image);
    });
}
bool RequestStamp(PreviewTransport const& transport,J const& query,winrt::Windows::UI::Color ink,StampReply reply){
    auto queue=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread();
    return transport&&queue&&transport(CanvasQueryKind::Workspace,to_string(query.Stringify()),
        [queue,ink,reply=std::move(reply)](PreviewPacket packet){queue.TryEnqueue([queue,ink,reply,packet=std::move(packet)]{
            try{
                auto result=packet?object(J::Parse(to_hstring(NativeString(capy_preview_metadata(packet.get())).get())),L"result"):J{};
                auto count=capy_preview_length(packet.get());
                if(!count){reply(result,nullptr);return;}
                auto width=int(num(result,L"width")),height=int(num(result,L"height"));
                if(width<=0||height<=0||width>512||height>512||count!=size_t(width)*size_t(height))throw hresult_invalid_argument();
                Imaging::WriteableBitmap bitmap(width,height);auto buffer=bitmap.PixelBuffer();uint8_t* pixels=nullptr;
                check_hresult(buffer.as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&pixels));
                if(buffer.Capacity()!=count*4)throw hresult_invalid_argument();
                ConvertStamp(queue,std::move(packet),result,ink,reply,agile_ref(bitmap),agile_ref(buffer),pixels,count);
            }catch(hresult_error const& error){OutputDebugStringW(error.message().c_str());reply(J{},nullptr);}
        });});
}
}
