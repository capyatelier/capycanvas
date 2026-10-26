using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

public static class CapyBubbleFixtureNative {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr LoadLibrary(string path);
 [DllImport("layer_windows.dll",CallingConvention=CallingConvention.Cdecl)] static extern ulong capy_localization_for_tag([MarshalAs(UnmanagedType.LPUTF8Str)]string tag);
 [DllImport("layer_windows.dll",CallingConvention=CallingConvention.Cdecl)] static extern ulong capy_bubble_ui(ulong context,[MarshalAs(UnmanagedType.LPUTF8Str)]string json);
 [DllImport("layer_windows.dll",CallingConvention=CallingConvention.Cdecl)] static extern void capy_localization_free(ulong context);
 [DllImport("layer_windows.dll",CallingConvention=CallingConvention.Cdecl)] static extern UIntPtr capy_string_length(ulong value);
 [DllImport("layer_windows.dll",CallingConvention=CallingConvention.Cdecl)] [return:MarshalAs(UnmanagedType.U1)] static extern bool capy_string_copy(ulong value,UIntPtr offset,byte[] output,UIntPtr length);
 [DllImport("layer_windows.dll",CallingConvention=CallingConvention.Cdecl)] static extern void capy_string_free(ulong value);
 [DllImport("layer_windows.dll",CallingConvention=CallingConvention.Cdecl)] static extern uint capy_color_hit(float x,float y,float size,uint shape);
 [StructLayout(LayoutKind.Sequential)] struct ScreenPoint {public int x,y;}
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(ScreenPoint point);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window,StringBuilder name,int count);
 public static string WindowAt(int x,int y){var window=WindowFromPoint(new ScreenPoint{x=x,y=y});uint process;GetWindowThreadProcessId(window,out process);var name=new StringBuilder(256);GetClassName(window,name,name.Capacity);return "hwnd="+window.ToInt64().ToString("X")+";class="+name+";process="+process;}
 static IntPtr library;
 static void EnsureLibrary(string path) {
  if(library==IntPtr.Zero){library=LoadLibrary(path);if(library==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());}
 }
 public static uint ColorHit(string path,float x,float y,float size,uint shape) {
  EnsureLibrary(path);return capy_color_hit(x,y,size,shape);
 }
 public static string Query(string path,string json) {
  EnsureLibrary(path);
  ulong context=capy_localization_for_tag("en"),result=0;
  try{
   if(context==0)throw new InvalidOperationException("Fixture localization unavailable.");
   result=capy_bubble_ui(context,json);
   if(result==0)throw new InvalidOperationException("Shared bubble geometry query failed.");
   byte[] bytes=new byte[checked((int)capy_string_length(result).ToUInt64())];
   if(!capy_string_copy(result,UIntPtr.Zero,bytes,new UIntPtr((uint)bytes.Length)))throw new InvalidOperationException("Shared bubble geometry text expired.");
   return Encoding.UTF8.GetString(bytes);
  }finally{if(result!=0)capy_string_free(result);if(context!=0)capy_localization_free(context);}
 }
}
