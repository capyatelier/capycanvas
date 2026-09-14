// Guarded OS multi-touch for an owned foreground window (x64 Windows).
// Frames include every active contact, including stationary contacts:
// https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-injecttouchinput
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Threading;
public static class CapyCanvasTouch {
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct PointerInfo {
  public uint type,id,frame,flags;public IntPtr device,target;public Point pixel,himetric,pixelRaw,himetricRaw;
  public uint time,history;public int data;public uint keys;public ulong performance;public uint change;
 }
 [StructLayout(LayoutKind.Sequential)] public struct TouchInfo {public PointerInfo pointer;public uint flags,mask;public Rect contact,contactRaw;public uint orientation,pressure;}
 [DllImport("user32.dll",SetLastError=true)] static extern bool InitializeTouchInjection(uint count,uint feedback);
 [DllImport("user32.dll",SetLastError=true)] static extern bool InjectTouchInput(uint count,TouchInfo[] contacts);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint process);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 static readonly object gate=new object();static SortedDictionary<uint,Point> contacts=new SortedDictionary<uint,Point>();
 static uint owner;static Timer pulse;static Exception failure;
 static void Guard(Point point){uint process;GetWindowThreadProcessId(GetForegroundWindow(),out process);if(process!=owner)throw new Exception("Review does not own foreground input.");GetWindowThreadProcessId(WindowFromPoint(point),out process);if(process!=owner)throw new Exception("Touch point is outside the owned review.");}
 public static void Verify(){lock(gate){if(failure!=null)throw new Exception("Touch keepalive failed.",failure);}}
 // Public contacts 1–3 map to the three native slots 0–2.
 static void Frame(SortedDictionary<uint,Point> points,uint changed=uint.MaxValue,uint flags=0x20006,bool guard=true){
  var frames=new List<TouchInfo>();foreach(var pair in points){var p=pair.Value;if(guard)Guard(p);frames.Add(new TouchInfo{pointer=new PointerInfo{type=2,id=pair.Key-1,pixel=p,flags=pair.Key==changed?flags:0x20006},mask=7,contact=new Rect{left=p.x-2,top=p.y-2,right=p.x+2,bottom=p.y+2},orientation=90,pressure=512});}
  if(frames.Count==0)return;
  for(int attempt=0;attempt<20;attempt++){if(InjectTouchInput((uint)frames.Count,frames.ToArray()))return;int error=Marshal.GetLastWin32Error();if(error!=21)throw new Win32Exception(error);Thread.Sleep(1);}
  throw new Exception("Windows did not accept the touch frame.");
 }
 public static void Initialize(uint process){lock(gate){if(pulse!=null||contacts.Count!=0)throw new Exception("Dispose the preceding touch review first.");if(Marshal.SizeOf(typeof(TouchInfo))!=144)throw new Exception("Touch structures require the x64 ABI.");if(!InitializeTouchInjection(3,3))throw new Win32Exception(Marshal.GetLastWin32Error());owner=process;failure=null;pulse=new Timer(Pulse,null,Timeout.Infinite,Timeout.Infinite);}}
 static void Pulse(object unused){lock(gate){try{Frame(contacts);}catch(Exception error){failure=error;try{CancelAll();}catch{}}}}
 public static void Down(uint id,int x,int y){lock(gate){Verify();if(id<1||id>3||contacts.ContainsKey(id)||contacts.Count>=3)throw new Exception("Invalid touch contact.");var next=new SortedDictionary<uint,Point>(contacts);next.Add(id,new Point{x=x,y=y});Frame(next,id,0x10006);contacts=next;pulse.Change(8,8);}}
 public static void Move(uint id,int x,int y){lock(gate){Verify();if(!contacts.ContainsKey(id))throw new Exception("Missing touch contact.");var next=new SortedDictionary<uint,Point>(contacts);next[id]=new Point{x=x,y=y};Frame(next);contacts=next;}}
 public static void Pair(int ax,int ay,int bx,int by){lock(gate){Verify();if(contacts.Count!=2||!contacts.ContainsKey(1)||!contacts.ContainsKey(2))throw new Exception("Expected contacts 1 and 2.");var next=new SortedDictionary<uint,Point>{{1,new Point{x=ax,y=ay}},{2,new Point{x=bx,y=by}}};Frame(next);contacts=next;}}
 public static void Up(uint id){lock(gate){Verify();if(!contacts.ContainsKey(id))throw new Exception("Missing touch contact.");Frame(contacts,id,0x40000);contacts.Remove(id);if(contacts.Count==0)pulse.Change(Timeout.Infinite,Timeout.Infinite);}}
 public static void CancelAll(){lock(gate){if(pulse!=null)pulse.Change(Timeout.Infinite,Timeout.Infinite);while(contacts.Count!=0){uint id=0;foreach(var key in contacts.Keys){id=key;break;}Frame(contacts,id,0x48000,false);contacts.Remove(id);}}}
 public static void Dispose(){try{CancelAll();}finally{if(pulse!=null){using(var done=new ManualResetEvent(false)){if(pulse.Dispose(done))done.WaitOne();}pulse=null;}}}
}
