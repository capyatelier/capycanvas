// OS-injected pen workload for an explicitly owned foreground review window.
// Measures software delivery; it cannot measure a physical digitizer or photons.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
public static class WindowsPenMotion {
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [StructLayout(LayoutKind.Sequential)] public struct PointerInfo {
  public uint type,id,frame,flags;public IntPtr device,target;public Point pixel,himetric,pixelRaw,himetricRaw;
  public uint time,history;public int data;public uint keys;public ulong performance;public uint change;
 }
 [StructLayout(LayoutKind.Sequential)] public struct PenInfo {public PointerInfo pointer;public uint flags,mask,pressure,rotation;public int tiltX,tiltY;}
 [StructLayout(LayoutKind.Explicit,Size=152)] public struct TypeInfo {[FieldOffset(0)]public uint type;[FieldOffset(8)]public PenInfo pen;}
 [DllImport("user32.dll",SetLastError=true)]static extern IntPtr CreateSyntheticPointerDevice(uint type,uint count,uint feedback);
 [DllImport("user32.dll",SetLastError=true)]static extern bool InjectSyntheticPointerInput(IntPtr device,TypeInfo[] info,uint count);
 [DllImport("user32.dll")]static extern void DestroySyntheticPointerDevice(IntPtr device);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern IntPtr CreateWaitableTimerEx(IntPtr attributes,string name,uint flags,uint access);
 [DllImport("kernel32.dll",SetLastError=true)]static extern bool SetWaitableTimer(IntPtr timer,ref long due,int period,IntPtr callback,IntPtr state,bool resume);
 [DllImport("kernel32.dll")]static extern uint WaitForSingleObject(IntPtr handle,uint timeout);
 [DllImport("kernel32.dll")]static extern bool CloseHandle(IntPtr handle);
 [DllImport("user32.dll")]static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")]static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")]static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")]public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 public static void Run(uint owner,int cx,int cy,int rx,int ry,int seconds,int hz,string output) {
  if(seconds<1||seconds>30||hz<30||hz>1000)throw new ArgumentOutOfRangeException();
  if(Marshal.SizeOf(typeof(PenInfo))!=120)throw new Exception("Requires x64 Windows ABI");
  var device=CreateSyntheticPointerDevice(3,1,3);if(device==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
  var timer=CreateWaitableTimerEx(IntPtr.Zero,null,2,0x1F0003);
  if(timer==IntPtr.Zero){DestroySyntheticPointerDevice(device);throw new Win32Exception(Marshal.GetLastWin32Error());}
  var rows=new List<string>(seconds*hz+2);rows.Add("index,qpc_before,qpc_after,x,y");
  var start=Stopwatch.GetTimestamp();Point last=new Point();bool contact=false;
  try {
   for(int i=0;i<=seconds*hz;i++) {
    long due=start+(long)i*Stopwatch.Frequency/hz;
    long remaining=due-Stopwatch.GetTimestamp();
    if(remaining>0){long delay=-Math.Max(1,remaining*10000000/Stopwatch.Frequency);
     if(!SetWaitableTimer(timer,ref delay,0,IntPtr.Zero,IntPtr.Zero,false)||WaitForSingleObject(timer,1000)!=0)throw new Win32Exception(Marshal.GetLastWin32Error());}
    double angle=2*Math.PI*i/hz;
    last=new Point{x=cx+(int)(rx*Math.Cos(angle)),y=cy+(int)(ry*Math.Sin(angle))};
    uint process;GetWindowThreadProcessId(GetForegroundWindow(),out process);if(process!=owner)throw new Exception("Benchmark lost foreground ownership");
    GetWindowThreadProcessId(WindowFromPoint(last),out process);if(process!=owner)throw new Exception("Pen point is outside the owned window");
    uint flags=i==0?0x10006u:i==seconds*hz?0x40000u:0x20006u;
    var before=Stopwatch.GetTimestamp();
    var info=new TypeInfo{type=3,pen=new PenInfo{pointer=new PointerInfo{type=3,id=0,pixel=last,flags=flags},mask=1,pressure=1024}};
    if(!InjectSyntheticPointerInput(device,new[]{info},1))throw new Win32Exception(Marshal.GetLastWin32Error());
    contact=i!=seconds*hz;
    rows.Add(i+","+before+","+Stopwatch.GetTimestamp()+","+last.x+","+last.y);
   }
  } finally {
   if(contact)InjectSyntheticPointerInput(device,new[]{new TypeInfo{type=3,pen=new PenInfo{pointer=new PointerInfo{type=3,id=0,pixel=last,flags=0x48000},mask=1,pressure=0}}},1);
   CloseHandle(timer);DestroySyntheticPointerDevice(device);File.WriteAllLines(output,rows);
  }
 }
}
