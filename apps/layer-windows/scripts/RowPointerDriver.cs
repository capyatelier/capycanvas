// OS-delivered input for an owned, foreground test window (x64 Windows).
// Synthetic pen/touch checks routing, not physical digitizers or latency.
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Threading;
public static class CapyRowPointer {
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct PointerInfo {
  public uint type,id,frame,flags; public IntPtr device,target;
  public Point pixel,himetric,pixelRaw,himetricRaw;
  public uint time,history; public int data; public uint keys; public ulong performance; public uint change;
 }
 [StructLayout(LayoutKind.Sequential)] public struct TouchInfo {
  public PointerInfo pointer; public uint flags,mask; public Rect contact,contactRaw; public uint orientation,pressure;
 }
 [StructLayout(LayoutKind.Sequential)] public struct PenInfo {
  public PointerInfo pointer; public uint flags,mask,pressure,rotation; public int tiltX,tiltY;
 }
 [StructLayout(LayoutKind.Explicit,Size=152)] public struct TypeInfo {
  [FieldOffset(0)]public uint type; [FieldOffset(8)]public TouchInfo touch; [FieldOffset(8)]public PenInfo pen;
 }
 [StructLayout(LayoutKind.Sequential)] public struct Mouse {public int dx,dy;public uint data,flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Sequential)] public struct Keyboard {public ushort key,scan;public uint flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)] public struct Input {
  [FieldOffset(0)]public uint type;[FieldOffset(8)]public Mouse mouse;[FieldOffset(8)]public Keyboard keyboard;
 }
 [DllImport("user32.dll",SetLastError=true)] static extern bool InitializeTouchInjection(uint count,uint feedback);
 [DllImport("user32.dll",SetLastError=true)] static extern bool InjectTouchInput(uint count,TouchInfo[] contacts);
 [DllImport("user32.dll",SetLastError=true)] static extern IntPtr CreateSyntheticPointerDevice(uint type,uint count,uint feedback);
 [DllImport("user32.dll",SetLastError=true)] static extern bool InjectSyntheticPointerInput(IntPtr device,TypeInfo[] contacts,uint count);
 [DllImport("user32.dll")] static extern void DestroySyntheticPointerDevice(IntPtr device);
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 [DllImport("user32.dll")] static extern int GetSystemMetrics(int code);
 [DllImport("user32.dll")] static extern bool GetCursorPos(out Point point);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 static uint owner,kind;static bool active;static Point last;static IntPtr pen;
 static readonly object gate=new object();static Timer pulse;static Exception failure;
 public static bool Active {get{lock(gate)return active;}}
 public static void Initialize(uint process) {
  if(active||pulse!=null||pen!=IntPtr.Zero)throw new InvalidOperationException("Dispose the previous pointer review first.");
  failure=null;kind=0;owner=process;
  if(Marshal.SizeOf(typeof(TouchInfo))!=144||Marshal.SizeOf(typeof(PenInfo))!=120||Marshal.SizeOf(typeof(TypeInfo))!=152)
   throw new Exception("Pointer structures require the x64 ABI.");
  if(!InitializeTouchInjection(1,3))throw new Win32Exception(Marshal.GetLastWin32Error());
  pen=CreateSyntheticPointerDevice(3,1,3);
  if(pen==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
  pulse=new Timer(Pulse,null,Timeout.Infinite,Timeout.Infinite);
 }
 static void Guard(Point point) {
  uint process;GetWindowThreadProcessId(GetForegroundWindow(),out process);
  if(process!=owner)throw new Exception("Review does not own foreground input.");
  GetWindowThreadProcessId(WindowFromPoint(point),out process);
  if(process!=owner)throw new Exception("Input point is outside the owned review.");
 }
 static void Check(){if(failure!=null)throw new Exception("Pointer keepalive failed.",failure);}
 static void MouseMove(Point point) {
  var mouse=new Mouse{dx=(point.x-GetSystemMetrics(76))*65535/(GetSystemMetrics(78)-1),
   dy=(point.y-GetSystemMetrics(77))*65535/(GetSystemMetrics(79)-1),flags=0xC001};
  if(SendInput(1,new[]{new Input{mouse=mouse}},40)!=1)throw new Win32Exception(Marshal.GetLastWin32Error());
  Point actual;GetCursorPos(out actual);
  if(Math.Abs(actual.x-point.x)>2||Math.Abs(actual.y-point.y)>2)throw new Exception("Windows did not move the mouse to the requested point.");
 }
 static void MouseButton(uint flags) {
  if(SendInput(1,new[]{new Input{mouse=new Mouse{flags=flags}}},40)!=1)throw new Win32Exception(Marshal.GetLastWin32Error());
 }
 static void Send(Point point,uint flags) {
  var info=new PointerInfo{type=kind,id=0,flags=flags,pixel=point};
  for(int retry=0;retry<20;retry++){
   bool accepted;
   if(kind==2)accepted=InjectTouchInput(1,new[]{new TouchInfo{pointer=info,mask=7,
    contact=new Rect{left=point.x-2,top=point.y-2,right=point.x+2,bottom=point.y+2},orientation=90,pressure=512}});
   else accepted=InjectSyntheticPointerInput(pen,new[]{new TypeInfo{type=3,pen=new PenInfo{pointer=info,mask=1,pressure=512}}},1);
   if(accepted)return;
   int error=Marshal.GetLastWin32Error();if(error!=21)throw new Win32Exception(error);Thread.Sleep(1);
  }
  throw new Exception("Windows did not accept the pointer frame.");
 }
 static void Pulse(object unused) {
  lock(gate){
   if(!active||kind==4)return;
   try{Guard(last);Send(last,0x20006);}
   catch(Exception error){
    failure=error;try{Cancel();}catch{}
    active=false;pulse.Change(Timeout.Infinite,Timeout.Infinite);
   }
  }
 }
 public static void Down(string device,int x,int y) {
  lock(gate){
   Check();if(active)throw new Exception("A review contact is already active.");
   kind=device=="touch"?2u:device=="pen"?3u:device=="mouse"?4u:0;
   if(kind==0)throw new ArgumentException("Unknown pointer device.");
   var point=new Point{x=x,y=y};Guard(point);
   if(kind==4){MouseMove(point);MouseButton(2);}else Send(point,0x10006);
   last=point;active=true;if(kind!=4)pulse.Change(25,25);
  }
 }
 public static void Move(int x,int y) {
  lock(gate){
   Check();if(!active)throw new Exception("No review contact is active.");
   var point=new Point{x=x,y=y};Guard(point);
   if(kind==4)MouseMove(point);else Send(point,0x20006);last=point;
  }
 }
 public static void Up() {
  lock(gate){
   Check();if(!active)return;Guard(last);Thread.Sleep(2);
   if(kind==4)MouseButton(4);else Send(last,0x40000);
   active=false;pulse.Change(Timeout.Infinite,Timeout.Infinite);
  }
 }
 public static void Cancel() {
  lock(gate){
   if(!active)return;
   // End only our existing contact, including after a foreground change.
   Thread.Sleep(2);
   if(kind==4)MouseButton(4);
   else if(kind==3){
    // Device removal exercises native capture loss. A canceled synthetic pen UP
    // can be projected as a regular release by WinUI.
    DestroySyntheticPointerDevice(pen);pen=CreateSyntheticPointerDevice(3,1,3);
    if(pen==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
   }else Send(last,0x48000);
   active=false;pulse.Change(Timeout.Infinite,Timeout.Infinite);
  }
 }
 public static void RightClick(int x,int y) {
  lock(gate){
   Check();if(active)throw new Exception("A review contact is already active.");
   var point=new Point{x=x,y=y};Guard(point);MouseMove(point);last=point;
   MouseButton(8);try{Thread.Sleep(35);}finally{MouseButton(16);}
  }
 }
 public static void Key(ushort key) {
  Guard(last);
  var down=new Input{type=1,keyboard=new Keyboard{key=key}};
  var up=new Input{type=1,keyboard=new Keyboard{key=key,flags=2}};
  if(SendInput(2,new[]{down,up},40)!=2)throw new Win32Exception(Marshal.GetLastWin32Error());
 }
 public static void Dispose() {
  try{Cancel();}finally{
   if(pulse!=null){
    using(var completed=new ManualResetEvent(false)){if(pulse.Dispose(completed))completed.WaitOne();}
    pulse=null;
   }
   if(pen!=IntPtr.Zero){DestroySyntheticPointerDevice(pen);pen=IntPtr.Zero;}
  }
 }
}
