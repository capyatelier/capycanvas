#pragma once
#include "UiControls.h"
#include <robuffer.h>

namespace CapyUi {
inline winrt::Microsoft::UI::Xaml::Media::Imaging::WriteableBitmap checkerBitmap(double logical,double scale,winrt::Windows::UI::Color light,winrt::Windows::UI::Color dark){
    int size=int(std::ceil(logical*scale));
    winrt::Microsoft::UI::Xaml::Media::Imaging::WriteableBitmap result(size,size);uint8_t* bytes=nullptr;
    winrt::check_hresult(result.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&bytes));
    for(int y=0;y<size;y++)for(int x=0;x<size;x++){
        // Match the shared repeating conic gradient, including its quadrant boundaries.
        double dx=std::fmod((x+.5)/scale,10.)-5,dy=std::fmod((y+.5)/scale,10.)-5;
        auto value=dx==0||dx*dy<0?dark:light;auto p=bytes+(y*size+x)*4;
        p[0]=value.B;p[1]=value.G;p[2]=value.R;p[3]=255;
    }
    result.Invalidate();return result;
}
}
