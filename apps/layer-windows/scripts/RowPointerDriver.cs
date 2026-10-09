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
 public static CursorInfo Cursor() {
  Guard(last);var info=new CursorInfo{size=(uint)Marshal.SizeOf(typeof(CursorInfo))};
  if(!GetCursorInfo(ref info))throw new Win32Exception(Marshal.GetLastWin32Error());
  return info;
 }
 public static bool CursorVisible() {return (Cursor().flags&1)!=0;}
 [DllImport("user32.dll",EntryPoint="LoadCursorW",SetLastError=true)] static extern IntPtr LoadCursor(IntPtr module,IntPtr resource);
 public static IntPtr StockCursor(int resource) {
  var cursor=LoadCursor(IntPtr.Zero,new IntPtr(resource));
  if(cursor==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
  return cursor;
 }
 [StructLayout(LayoutKind.Sequential)] struct IconInfo {public bool icon;public uint x,y;public IntPtr mask,color;}
 [DllImport("user32.dll",SetLastError=true)] static extern bool GetIconInfo(IntPtr icon,out IconInfo info);
 [DllImport("user32.dll",SetLastError=true)] static extern bool DrawIconEx(IntPtr dc,int x,int y,IntPtr icon,int width,int height,uint step,IntPtr brush,uint flags);
 [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr value);
 public static void DrawCursor(IntPtr dc,CursorInfo cursor,int left,int top) {
  Guard(cursor.position);
  if((cursor.flags&1)==0||cursor.cursor==IntPtr.Zero)throw new InvalidOperationException("The owned cursor is not visible.");
  IconInfo icon;if(!GetIconInfo(cursor.cursor,out icon))throw new Win32Exception(Marshal.GetLastWin32Error());
  try{if(!DrawIconEx(dc,cursor.position.x-left-(int)icon.x,cursor.position.y-top-(int)icon.y,cursor.cursor,0,0,0,IntPtr.Zero,3))throw new Win32Exception(Marshal.GetLastWin32Error());}
  finally{if(icon.mask!=IntPtr.Zero)DeleteObject(icon.mask);if(icon.color!=IntPtr.Zero)DeleteObject(icon.color);}
 }
 [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
 [DllImport("user32.dll")] static extern IntPtr GetThreadDesktop(uint thread);
 [DllImport("user32.dll")] static extern IntPtr OpenInputDesktop(uint flags,bool inherit,uint access);
 [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr desktop);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern bool GetUserObjectInformation(IntPtr handle,int index,System.Text.StringBuilder name,uint length,out uint needed);
 static string DesktopName(IntPtr desktop) {
  var name=new System.Text.StringBuilder(256);uint needed;
  return desktop!=IntPtr.Zero&&GetUserObjectInformation(desktop,2,name,512,out needed)?name.ToString():null;
 }
 public static void VerifyPrivateDesktop(string expected) {
  if(string.IsNullOrWhiteSpace(expected)||string.Equals(expected,"Default",StringComparison.OrdinalIgnoreCase))throw new ArgumentException("A private test desktop name is required.");
  IntPtr input=OpenInputDesktop(0,false,1);
  try{if(input==IntPtr.Zero||DesktopName(GetThreadDesktop(GetCurrentThreadId()))!=expected||DesktopName(input)!=expected)throw new InvalidOperationException("The current thread and input desktop must match the owned private test desktop.");}
  finally{if(input!=IntPtr.Zero)CloseDesktop(input);}
 }
 static string InjectionFailure(Point point,uint flags,int retry) {
  long failedAt=System.Diagnostics.Stopwatch.GetTimestamp();uint thread=GetCurrentThreadId();IntPtr input=OpenInputDesktop(0,false,1);
  try{return $"kind={kind}, id=0, flags=0x{flags:X}, point=({point.x},{point.y}), last=({last.x},{last.y}), device=0x{pen.ToInt64():X}, active={active}, hover={hovering}, retry={retry}, ms_since_accepted_at_error={(failedAt-lastInjection)*1000.0/System.Diagnostics.Stopwatch.Frequency:R}, thread={thread}, thread_desktop={DesktopName(GetThreadDesktop(thread))??"unavailable"}, input_desktop={DesktopName(input)??"unavailable"}";}
  finally{if(input!=IntPtr.Zero)CloseDesktop(input);}
 }
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
 static long lastInjection;public static double MaxGapMilliseconds {get;private set;}
 static uint owner,kind,penButtons,mouseRelease;static bool active,hovering,eraserEnd;static Point last;static IntPtr pen;
 static readonly object gate=new object();static Timer pulse;static Exception failure;
 public static bool Active {get{lock(gate)return active;}}
 public static void Initialize(uint process) {
  if(active||pulse!=null||pen!=IntPtr.Zero)throw new InvalidOperationException("Dispose the previous pointer review first.");
  failure=null;kind=0;penButtons=0;mouseRelease=0;hovering=false;eraserEnd=false;owner=process;
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
 static void EndHover() {
  if(!hovering)return;
  Send(last,0x20000);hovering=false;pulse.Change(Timeout.Infinite,Timeout.Infinite);
 }
 static void MouseMove(Point point) {
  EndHover();
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
   if(accepted){
    long now=System.Diagnostics.Stopwatch.GetTimestamp();
    if(active)MaxGapMilliseconds=Math.Max(MaxGapMilliseconds,(now-lastInjection)*1000.0/System.Diagnostics.Stopwatch.Frequency);
    lastInjection=now;return;
   }
   int error=Marshal.GetLastWin32Error();if(error!=21)throw new Win32Exception(error,new Win32Exception(error).Message+" "+InjectionFailure(point,flags,retry));Thread.Sleep(1);
  }
  throw new Exception("Windows did not accept the pointer frame.");
 }
 static void Pulse(object unused) {
  lock(gate){
   if((!active&&!hovering)||kind==4)return;
   try{Guard(last);Send(last,active?0x20006u:0x20002u);}
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
   var point=new Point{x=x,y=y};Guard(point);kind=3;Send(point,0x20002);last=point;hovering=true;pulse.Change(25,25);
  }
 }
 public static void EraserEnd(bool on) {lock(gate){eraserEnd=on;}}
 public static void Barrel(bool held) {
  lock(gate){
   Check();if(kind!=3)throw new Exception("The barrel button needs a pen in range.");
   penButtons=held?1u:0u;Guard(last);Send(last,active?0x20006u:0x20002u);
  }
 }
 public static void PenLeave() {
  lock(gate){
   Check();if(active||kind!=3)return;
   penButtons=0;Guard(last);if(hovering)EndHover();else Send(last,0x20000);
  }
 }
 public static void Down(string device,int x,int y) {Down(device,x,y,"left");}
 public static void Down(string device,int x,int y,string button) {
  lock(gate){
   Check();if(active)throw new Exception("A review contact is already active.");
   EndHover();kind=device=="touch"?2u:device=="pen"?3u:device=="mouse"?4u:0;
   if(kind==0)throw new ArgumentException("Unknown pointer device.");
   uint press=button=="left"?2u:button=="right"?8u:button=="middle"?32u:0;
   if(press==0||(kind!=4&&press!=2))throw new ArgumentException("Choose left, right or middle for mouse contacts only.");
   var point=new Point{x=x,y=y};Guard(point);MaxGapMilliseconds=0;
   if(kind==4){MouseMove(point);MouseButton(press);mouseRelease=press<<1;}else Send(point,0x10006);
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
   if(kind==4)MouseButton(mouseRelease);else Send(last,kind==3&&stayInRange?0x40002u:0x40000u);
   active=false;hovering=kind==3&&stayInRange;
   pulse.Change(hovering?25:Timeout.Infinite,hovering?25:Timeout.Infinite);
  }
 }
 public static double DoubleClick(int x,int y) {
  lock(gate){
   Check();if(active)throw new Exception("A review contact is already active.");
   var point=new Point{x=x,y=y};Guard(point);MouseMove(point);last=point;kind=4;
   var clicks=new[]{new Input{mouse=new Mouse{flags=2}},new Input{mouse=new Mouse{flags=4}},
    new Input{mouse=new Mouse{flags=2}},new Input{mouse=new Mouse{flags=4}}};
   var clock=System.Diagnostics.Stopwatch.StartNew();
   if(SendInput(4,clicks,40)!=4)throw new Win32Exception(Marshal.GetLastWin32Error());
   return clock.Elapsed.TotalMilliseconds;
  }
 }
 public static double DoubleClick(string device,int x,int y) {
  if(device=="mouse")return DoubleClick(x,y);
  lock(gate){
   var clock=System.Diagnostics.Stopwatch.StartNew();
   try{for(int i=0;i<2;i++){Down(device,x,y);Up(device=="pen");}return clock.Elapsed.TotalMilliseconds;}
   finally{if(active)Cancel();}
  }
 }
 public static void Cancel() {
  lock(gate){
   if(!active){EndHover();return;}
   // End only our existing contact, including after a foreground change.
   Thread.Sleep(2);
   if(kind==4)MouseButton(mouseRelease);
   else if(kind==3){
    // Device removal exercises native capture loss. A canceled synthetic pen UP
    // can be projected as a regular release by WinUI.
    DestroySyntheticPointerDevice(pen);pen=CreateSyntheticPointerDevice(3,1,3);
    if(pen==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
   }else Send(last,0x48000);
   active=false;pulse.Change(Timeout.Infinite,Timeout.Infinite);
  }
 }
 public static void RightDrag(int x0,int y0,int x1,int y1){ButtonDrag("right",x0,y0,x1,y1);}
 public static void MiddleDrag(int x0,int y0,int x1,int y1){ButtonDrag("middle",x0,y0,x1,y1);}
 static void ButtonDrag(string button,int x0,int y0,int x1,int y1) {
  lock(gate){
   Down("mouse",x0,y0,button);
   try{
    for(int i=1;i<=12;i++){Move(x0+(x1-x0)*i/12,y0+(y1-y0)*i/12);Thread.Sleep(10);}
    Up();
   }finally{Cancel();}
  }
 }
 public static void Wheel(int x,int y,int delta) {Wheel(x,y,delta,false);}
 public static void Wheel(int x,int y,int delta,bool horizontal) {
  lock(gate){
   Check();if(active&&kind!=4)throw new Exception("Wheel input cannot interrupt a pen or touch contact.");
   var point=new Point{x=x,y=y};Guard(point);MouseMove(point);last=point;
   if(SendInput(1,new[]{new Input{mouse=new Mouse{data=unchecked((uint)delta),flags=horizontal?0x1000u:0x0800u}}},40)!=1)throw new Win32Exception(Marshal.GetLastWin32Error());
  }
 }
 public static void RightClick(int x,int y) {
  lock(gate){
   Down("mouse",x,y,"right");
   try{Thread.Sleep(35);Up();}finally{Cancel();}
  }
 }
 // Native text input can move the app when the touch keyboard opens. Permit
 // a fresh UIA-measured point for idle keyboard input, with both guards intact.
 public static void KeyAt(ushort key,int x,int y) {
  lock(gate){
   Check();if(active)throw new Exception("Use the original contact for keys during a gesture.");
   var point=new Point{x=x,y=y};Guard(point);EndHover();last=point;Key(key);
  }
 }
 public static void Key(ushort key) {
  Guard(last);Key(owner,key);
 }
 static Input Press(ushort key,bool release) {
  bool extended=key>=0x21&&key<=0x28||key==0x2D||key==0x2E||key==0x5B||key==0x5C;
  return new Input{type=1,keyboard=new Keyboard{key=key,flags=(release?2u:0u)|(extended?1u:0u)}};
 }
 static readonly System.Collections.Generic.HashSet<ushort> held=new System.Collections.Generic.HashSet<ushort>();
 public static void Hold(ushort key,bool down) {
  lock(gate){
   if(down)Guard(last);else if(!held.Contains(key))return;
   if(SendInput(1,new[]{Press(key,!down)},40)!=1)throw new Win32Exception(Marshal.GetLastWin32Error());
   if(down)held.Add(key);else held.Remove(key);
  }
 }
 static void ReleaseHeld() {
  foreach(var key in new System.Collections.Generic.List<ushort>(held)){
   SendInput(1,new[]{Press(key,true)},40);
  }
  held.Clear();
 }
 // Standalone keyboard reviews do not need a synthetic pointer device.
 public static void Key(uint process,ushort key) {
  uint foreground;GetWindowThreadProcessId(GetForegroundWindow(),out foreground);
  if(foreground!=process)throw new Exception("Review does not own foreground input.");
  if(SendInput(2,new[]{Press(key,false),Press(key,true)},40)!=2)throw new Win32Exception(Marshal.GetLastWin32Error());
 }
 public static void Chord(uint process,ushort[] modifiers,ushort key) {
  uint foreground;GetWindowThreadProcessId(GetForegroundWindow(),out foreground);
  if(foreground!=process)throw new Exception("Review does not own foreground input.");
  var inputs=new System.Collections.Generic.List<Input>();
  foreach(var modifier in modifiers)inputs.Add(Press(modifier,false));
  inputs.Add(Press(key,false));inputs.Add(Press(key,true));
  for(int i=modifiers.Length-1;i>=0;i--)inputs.Add(Press(modifiers[i],true));
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
