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
 [StructLayout(LayoutKind.Sequential)] public struct CursorInfo {public uint size,flags;public IntPtr cursor;public Point position;}
 [DllImport("user32.dll",SetLastError=true)] static extern bool GetCursorInfo(ref CursorInfo info);
 public static bool CursorVisible() {
  Guard(last);var info=new CursorInfo{size=(uint)Marshal.SizeOf(typeof(CursorInfo))};
  if(!GetCursorInfo(ref info))throw new Win32Exception(Marshal.GetLastWin32Error());
  return (info.flags&1)!=0;
 }
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
 static uint owner,kind,penButtons;static bool active;static Point last;static IntPtr pen;
 static readonly object gate=new object();static Timer pulse;static Exception failure;
 public static bool Active {get{lock(gate)return active;}}
 public static void Initialize(uint process) {
  if(active||pulse!=null||pen!=IntPtr.Zero)throw new InvalidOperationException("Dispose the previous pointer review first.");
  failure=null;kind=0;penButtons=0;owner=process;
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
 public static void Verify(){lock(gate)Check();}
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
   else accepted=InjectSyntheticPointerInput(pen,new[]{new TypeInfo{type=3,pen=new PenInfo{pointer=info,flags=penButtons,mask=1,pressure=512}}},1);
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
 public static void Hover(int x,int y) {
  lock(gate){
   Check();if(active)throw new Exception("A review contact is already active.");
   var point=new Point{x=x,y=y};Guard(point);MouseMove(point);last=point;
  }
 }
 // Keep a pen in hover between taps, as a physical tablet normally does.
 public static void PenHover(int x,int y) {
  lock(gate){
   Check();if(active)throw new Exception("A review contact is already active.");
   var point=new Point{x=x,y=y};Guard(point);kind=3;Send(point,0x20002);last=point;
  }
 }
 public static void Barrel(bool held) {
  lock(gate){
   Check();if(kind!=3)throw new Exception("The barrel button needs a pen in range.");
   penButtons=held?1u:0u;Guard(last);Send(last,active?0x20006u:0x20002u);
  }
 }
 public static void PenLeave() {
  lock(gate){
   Check();if(active||kind!=3)return;
   penButtons=0;Guard(last);Send(last,0x20000);
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
 public static void Up(bool stayInRange=false) {
  lock(gate){
   Check();if(!active)return;Guard(last);Thread.Sleep(2);
   if(kind==4)MouseButton(4);else Send(last,kind==3&&stayInRange?0x40002u:0x40000u);
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
 public static void RightDrag(int x0,int y0,int x1,int y1){ButtonDrag(8,16,x0,y0,x1,y1);}
 public static void MiddleDrag(int x0,int y0,int x1,int y1){ButtonDrag(32,64,x0,y0,x1,y1);}
 static void ButtonDrag(uint down,uint up,int x0,int y0,int x1,int y1) {
  lock(gate){
   Check();if(active)throw new Exception("A review contact is already active.");
   var start=new Point{x=x0,y=y0};Guard(start);MouseMove(start);last=start;
   MouseButton(down);
   try{
    for(int i=1;i<=12;i++){var point=new Point{x=x0+(x1-x0)*i/12,y=y0+(y1-y0)*i/12};Guard(point);MouseMove(point);last=point;Thread.Sleep(10);}
   }finally{MouseButton(up);}
  }
 }
 public static void RightClick(int x,int y) {
  lock(gate){
   Check();if(active)throw new Exception("A review contact is already active.");
   var point=new Point{x=x,y=y};Guard(point);MouseMove(point);last=point;
   MouseButton(8);try{Thread.Sleep(35);}finally{MouseButton(16);}
  }
 }
 // Native text input can move the app when the touch keyboard opens. Permit
 // a fresh UIA-measured point for idle keyboard input, with both guards intact.
 public static void KeyAt(ushort key,int x,int y) {
  lock(gate){
   Check();if(active)throw new Exception("Use the original contact for keys during a gesture.");
   var point=new Point{x=x,y=y};Guard(point);last=point;Key(key);
  }
 }
 public static void Key(ushort key) {
  Guard(last);Key(owner,key);
 }
 static readonly System.Collections.Generic.HashSet<ushort> held=new System.Collections.Generic.HashSet<ushort>();
 public static void Hold(ushort key,bool down) {
  lock(gate){
   if(down)Guard(last);else if(!held.Contains(key))return;
   var input=new Input{type=1,keyboard=new Keyboard{key=key,flags=down?0u:2u}};
   if(SendInput(1,new[]{input},40)!=1)throw new Win32Exception(Marshal.GetLastWin32Error());
   if(down)held.Add(key);else held.Remove(key);
  }
 }
 static void ReleaseHeld() {
  foreach(var key in new System.Collections.Generic.List<ushort>(held)){
   var input=new Input{type=1,keyboard=new Keyboard{key=key,flags=2}};SendInput(1,new[]{input},40);
  }
  held.Clear();
 }
 // Standalone keyboard reviews do not need a synthetic pointer device.
 public static void Key(uint process,ushort key) {
  uint foreground;GetWindowThreadProcessId(GetForegroundWindow(),out foreground);
  if(foreground!=process)throw new Exception("Review does not own foreground input.");
  var down=new Input{type=1,keyboard=new Keyboard{key=key}};
  var up=new Input{type=1,keyboard=new Keyboard{key=key,flags=2}};
  if(SendInput(2,new[]{down,up},40)!=2)throw new Win32Exception(Marshal.GetLastWin32Error());
 }
 public static void Chord(uint process,ushort[] modifiers,ushort key) {
  uint foreground;GetWindowThreadProcessId(GetForegroundWindow(),out foreground);
  if(foreground!=process)throw new Exception("Review does not own foreground input.");
  var inputs=new System.Collections.Generic.List<Input>();
  foreach(var modifier in modifiers)inputs.Add(new Input{type=1,keyboard=new Keyboard{key=modifier}});
  inputs.Add(new Input{type=1,keyboard=new Keyboard{key=key}});inputs.Add(new Input{type=1,keyboard=new Keyboard{key=key,flags=2}});
  for(int i=modifiers.Length-1;i>=0;i--)inputs.Add(new Input{type=1,keyboard=new Keyboard{key=modifiers[i],flags=2}});
  if(SendInput((uint)inputs.Count,inputs.ToArray(),40)!=inputs.Count){
   int error=Marshal.GetLastWin32Error();var releases=inputs.GetRange(modifiers.Length+1,modifiers.Length+1);
   SendInput((uint)releases.Count,releases.ToArray(),40);throw new Win32Exception(error);
  }
 }
 public static void Dispose() {
  try{lock(gate)ReleaseHeld();Cancel();}finally{
   if(pulse!=null){
    using(var completed=new ManualResetEvent(false)){if(pulse.Dispose(completed))completed.WaitOne();}
    pulse=null;
   }
   if(pen!=IntPtr.Zero){DestroySyntheticPointerDevice(pen);pen=IntPtr.Zero;}
   penButtons=0;
  }
 }
}
