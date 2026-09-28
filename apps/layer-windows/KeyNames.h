#pragma once
#include "pch.h"
#include <string>

namespace CapyUi {
inline std::wstring KeyName(winrt::Windows::System::VirtualKey key,uint32_t scanCode){
    using winrt::Windows::System::VirtualKey;
    switch(key){
    case VirtualKey::Shift:return L"shift";
    case VirtualKey::Control:return L"control";
    case VirtualKey::Menu:return L"alt";
    case VirtualKey::Escape:return L"escape";
    case VirtualKey::Space:return L" ";
    case VirtualKey::Enter:return L"enter";
    case VirtualKey::Tab:return L"tab";
    case VirtualKey::Back:return L"backspace";
    case VirtualKey::Delete:return L"delete";
    case VirtualKey::Insert:return L"insert";
    case VirtualKey::Home:return L"home";
    case VirtualKey::End:return L"end";
    case VirtualKey::PageUp:return L"pageup";
    case VirtualKey::PageDown:return L"pagedown";
    case VirtualKey::Left:return L"arrowleft";
    case VirtualKey::Right:return L"arrowright";
    case VirtualKey::Up:return L"arrowup";
    case VirtualKey::Down:return L"arrowdown";
    default:break;
    }
    if(key>=VirtualKey::F1&&key<=VirtualKey::F24)return L"f"+std::to_wstring(uint32_t(key)-uint32_t(VirtualKey::F1)+1);
    switch(uint32_t(key)){
    case VK_VOLUME_MUTE:return L"volumemute";
    case VK_VOLUME_DOWN:return L"volumedown";
    case VK_VOLUME_UP:return L"volumeup";
    case VK_MEDIA_NEXT_TRACK:return L"mediatracknext";
    case VK_MEDIA_PREV_TRACK:return L"mediatrackprevious";
    case VK_MEDIA_PLAY_PAUSE:return L"mediaplaypause";
    default:break;
    }
    BYTE state[256]{};
    GetKeyboardState(state);
    state[VK_CONTROL]=state[VK_LCONTROL]=state[VK_RCONTROL]=0;
    state[VK_MENU]=state[VK_LMENU]=state[VK_RMENU]=0;
    wchar_t characters[8]{};
    int count=ToUnicodeEx(uint32_t(key),scanCode,state,characters,8,4,GetKeyboardLayout(0));
    return count>0&&characters[0]>=L' '?std::wstring(characters,count):std::wstring();
}
inline bool DeviceKey(winrt::Windows::System::VirtualKey key){
    auto code=uint32_t(key);
    return code==VK_VOLUME_MUTE||code==VK_VOLUME_DOWN||code==VK_VOLUME_UP||code==VK_MEDIA_NEXT_TRACK||code==VK_MEDIA_PREV_TRACK||code==VK_MEDIA_PLAY_PAUSE;
}
}
