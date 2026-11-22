// OS-injected pen and touch workload for an explicitly owned foreground review window.
// Measures software delivery; it cannot measure a physical digitizer or photons.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
public static class WindowsPenMotion {
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [StructLayout(LayoutKind.Sequential)] public struct PointerInfo {
  public uint type,id,frame,flags;public IntPtr device,target;public Point pixel,himetric,pixelRaw,himetricRaw;
  public uint time,history;public int data;public uint keys;public ulong performance;public uint change;
 }
 [StructLayout(LayoutKind.Sequential)] public struct PenInfo {public PointerInfo pointer;public uint flags,mask,pressure,rotation;public int tiltX,tiltY;}
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct TouchInfo {public PointerInfo pointer;public uint flags,mask;public Rect contact,contactRaw;public uint orientation,pressure;}
 [StructLayout(LayoutKind.Explicit,Size=152)] public struct TypeInfo {[FieldOffset(0)]public uint type;[FieldOffset(8)]public PenInfo pen;}
 [DllImport("user32.dll",SetLastError=true)]static extern IntPtr CreateSyntheticPointerDevice(uint type,uint count,uint feedback);
 [DllImport("user32.dll",SetLastError=true)]static extern bool InjectSyntheticPointerInput(IntPtr device,TypeInfo[] info,uint count);
 [DllImport("user32.dll")]static extern void DestroySyntheticPointerDevice(IntPtr device);
 [DllImport("user32.dll",SetLastError=true)]static extern bool InitializeTouchInjection(uint count,uint feedback);
 [DllImport("user32.dll",SetLastError=true)]static extern bool InjectTouchInput(uint count,TouchInfo[] contacts);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern IntPtr CreateWaitableTimerEx(IntPtr attributes,string name,uint flags,uint access);
 [DllImport("kernel32.dll",SetLastError=true)]static extern bool SetWaitableTimer(IntPtr timer,ref long due,int period,IntPtr callback,IntPtr state,bool resume);
 [DllImport("kernel32.dll")]static extern uint WaitForSingleObject(IntPtr handle,uint timeout);
 [DllImport("kernel32.dll")]static extern bool CloseHandle(IntPtr handle);
 [DllImport("user32.dll")]static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")]static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")]static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")]static extern bool IsChild(IntPtr parent,IntPtr child);
 public static void VerifyPoint(uint owner,IntPtr window,int x,int y) {
  var target=WindowFromPoint(new Point{x=x,y=y});uint process;
  GetWindowThreadProcessId(target,out process);
  if(process!=owner||(target!=window&&!IsChild(window,target)))throw new Exception("Motion point is outside the owned drawing window");
 }
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")]public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")]static extern IntPtr GetDC(IntPtr window);
 [DllImport("user32.dll")]static extern int ReleaseDC(IntPtr window,IntPtr dc);
 [DllImport("gdi32.dll")]static extern uint GetPixel(IntPtr dc,int x,int y);
 public static int[] Sample(uint owner,int[] points) {
  if(points.Length<8||points.Length%2!=0)throw new ArgumentException("Four or more sample points are required");
  var dc=GetDC(IntPtr.Zero);if(dc==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
  try {
   var result=new int[points.Length/2];
   for(int i=0;i<result.Length;i++) {
    uint process;GetWindowThreadProcessId(WindowFromPoint(new Point{x=points[i*2],y=points[i*2+1]}),out process);
    if(process!=owner)throw new Exception("Pixel sample is outside the owned window");
    uint color=GetPixel(dc,points[i*2],points[i*2+1]);if(color==0xffffffff)throw new Exception("Pixel sample is unavailable");result[i]=(int)color;
   }
   return result;
  } finally {ReleaseDC(IntPtr.Zero,dc);}
 }
 public static Task RunAsync(uint owner,int cx,int cy,int rx,int ry,int seconds,int hz,string output) {
  return Task.Run(delegate {
   var previous=SetThreadDpiAwarenessContext(new IntPtr(-4));
   try {Run(owner,cx,cy,rx,ry,seconds,hz,output);} finally {SetThreadDpiAwarenessContext(previous);}
  });
 }
 public static void Run(uint owner,int cx,int cy,int rx,int ry,int seconds,int hz,string output) {
  Run(owner,cx,cy,rx,ry,seconds,hz,output,double.NaN,0);
 }
 public static void Run(uint owner,int cx,int cy,int rx,int ry,int seconds,int hz,string output,double sweep,double startAngle) {
  if(!double.IsNaN(sweep)&&(Math.Abs(sweep)>2*Math.PI||sweep==0||double.IsNaN(startAngle)||double.IsInfinity(startAngle)))throw new ArgumentOutOfRangeException();
  if(seconds<1||seconds>30||hz<30||hz>1000)throw new ArgumentOutOfRangeException();
  var path=new int[seconds*hz*2];var pressure=new int[seconds*hz];
  for(int i=0;i<seconds*hz;i++) {
   double angle=double.IsNaN(sweep)?2*Math.PI*i/hz:startAngle+sweep*i/(seconds*hz-1);
   path[i*2]=cx+(int)(rx*Math.Cos(angle));path[i*2+1]=cy+(int)(ry*Math.Sin(angle));pressure[i]=1024;
  }
  Play(owner,3,1,path,pressure,hz,output);
 }
 public static void Play(uint owner,uint kind,int contacts,int[] path,int[] pressure,int hz,string output) {
  int frames=contacts>0?path.Length/(2*contacts):0;
  if((kind!=2&&kind!=3)||contacts<1||contacts>(kind==3?1:2)||frames<1||path.Length!=frames*contacts*2||(kind==3&&(pressure==null||pressure.Length!=frames))||hz<30||hz>1000||frames>hz*60)throw new ArgumentOutOfRangeException();
  if(Marshal.SizeOf(typeof(PenInfo))!=120||Marshal.SizeOf(typeof(TouchInfo))!=144)throw new Exception("Requires 64-bit Windows ABI");
  var device=IntPtr.Zero;
  if(kind==3){device=CreateSyntheticPointerDevice(3,1,3);if(device==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());}
  else if(!InitializeTouchInjection((uint)contacts,3))throw new Win32Exception(Marshal.GetLastWin32Error());
  var timer=CreateWaitableTimerEx(IntPtr.Zero,null,2,0x1F0003);
  if(timer==IntPtr.Zero){if(device!=IntPtr.Zero)DestroySyntheticPointerDevice(device);throw new Win32Exception(Marshal.GetLastWin32Error());}
  var rows=new List<string>(frames+2);rows.Add("index,qpc_before,qpc_after,x,y");
  var next=Stopwatch.GetTimestamp();int at=0;bool contact=false;
  try {
   for(int i=0;i<=frames;i++) {
    long remaining=next-Stopwatch.GetTimestamp();
    if(remaining>0){long delay=-Math.Max(1,remaining*10000000/Stopwatch.Frequency);
     if(!SetWaitableTimer(timer,ref delay,0,IntPtr.Zero,IntPtr.Zero,false)||WaitForSingleObject(timer,1000)!=0)throw new Win32Exception(Marshal.GetLastWin32Error());}
    if(i!=frames)at=i;
    uint process;GetWindowThreadProcessId(GetForegroundWindow(),out process);if(process!=owner)throw new Exception("Benchmark lost foreground ownership");
    for(int c=0;c<contacts;c++){GetWindowThreadProcessId(WindowFromPoint(At(path,at*contacts+c)),out process);if(process!=owner)throw new Exception("Pointer point is outside the owned window");}
    uint flags=i==0?0x10006u:i==frames?0x40000u:0x20006u;
    var before=Stopwatch.GetTimestamp();
    Inject(kind,device,contacts,path,at,kind==3?(uint)pressure[at]:0,flags);
    next=Stopwatch.GetTimestamp()+Stopwatch.Frequency/hz;
    contact=i!=frames;
    rows.Add(i+","+before+","+Stopwatch.GetTimestamp()+","+path[at*contacts*2]+","+path[at*contacts*2+1]);
   }
  } finally {
   if(contact)try{Inject(kind,device,contacts,path,at,0,0x48000);}catch(Win32Exception){}
   CloseHandle(timer);if(device!=IntPtr.Zero)DestroySyntheticPointerDevice(device);File.WriteAllLines(output,rows);
  }
 }
 static Point At(int[] path,int index) {return new Point{x=path[index*2],y=path[index*2+1]};}
 static void Inject(uint kind,IntPtr device,int contacts,int[] path,int at,uint pressure,uint flags) {
  if(kind==3) {
   var info=new TypeInfo{type=3,pen=new PenInfo{pointer=new PointerInfo{type=3,id=0,pixel=At(path,at),flags=flags},mask=1,pressure=pressure}};
   if(!InjectSyntheticPointerInput(device,new[]{info},1))throw new Win32Exception(Marshal.GetLastWin32Error());
   return;
  }
  var touches=new TouchInfo[contacts];
  for(int c=0;c<contacts;c++){var p=At(path,at*contacts+c);touches[c]=new TouchInfo{pointer=new PointerInfo{type=2,id=(uint)c,pixel=p,flags=flags},mask=7,contact=new Rect{left=p.x-2,top=p.y-2,right=p.x+2,bottom=p.y+2},orientation=90,pressure=512};}
  for(int retry=0;!InjectTouchInput((uint)contacts,touches);retry++){int error=Marshal.GetLastWin32Error();if(error!=21||retry==20)throw new Win32Exception(error);Thread.Sleep(1);}
 }
}
