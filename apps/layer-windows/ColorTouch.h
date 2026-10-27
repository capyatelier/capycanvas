#pragma once
#include "UiControls.h"
#include "NativeJson.h"
#include <map>
#include <winrt/Microsoft.UI.Input.h>

namespace CapyUi {
struct ColorTouches {
    using Point=Windows::Foundation::Point;
    std::map<uint32_t,Point> contacts;
    bool claimed()const{return contacts.size()>=2;}
    bool forward(WorkspaceData& data,UIElement const& surface,Microsoft::UI::Input::PointerPoint const& point,hstring phase,bool inside){
        if(point.PointerDeviceType()!=Microsoft::UI::Input::PointerDeviceType::Touch)return false;
        auto id=point.PointerId();
        if(point.Properties().IsCanceled())phase=L"cancel";
        if(phase==L"down"){if(!inside||contacts.contains(id))return claimed();contacts[id]=point.Position();}
        auto found=contacts.find(id);if(found==contacts.end())return false;
        found->second=point.Position();
        send(data,surface,id,phase,found->second,point.Timestamp()*1000);
        bool result=claimed();
        if(phase==L"up"||phase==L"cancel")contacts.erase(found);
        return result;
    }
    void cancel(WorkspaceData& data,UIElement const& surface){
        for(auto const& [id,at]:std::exchange(contacts,{}))send(data,surface,id,L"cancel",at,0);
    }
    static void send(WorkspaceData& data,UIElement const& surface,uint32_t id,hstring const& phase,Point at,uint64_t time){
        if(!data.input)return;
        double scale=surface.XamlRoot()?surface.XamlRoot().RasterizationScale():1.;
        A position;position.Append(N(at.X*scale));position.Append(N(at.Y*scale));
        data.input(JsonUnsignedFields(to_string(O({{L"type",S(L"color_touch")},{L"phase",S(phase)},{L"position",position}}).Stringify()),
            {{"surface",reinterpret_cast<uintptr_t>(get_abi(surface))},{"id",id},{"time_ns",time}}));
    }
};
}
