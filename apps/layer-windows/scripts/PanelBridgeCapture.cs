using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Automation;

public sealed class PanelBridgeCapture : IDisposable {
 [StructLayout(LayoutKind.Sequential)] struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] struct Point {public int x,y;}
 [DllImport("user32.dll",SetLastError=true)] static extern bool GetWindowRect(IntPtr window,out Rect rectangle);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr window,uint message,IntPtr wparam,IntPtr lparam);
 public sealed class Sample {public long begin,end;public int width_begin,width_end,height;public string path;}
 readonly List<Sample> states=new List<Sample>();
 readonly ManualResetEventSlim stateReady=new ManualResetEventSlim(),cameraReady=new ManualResetEventSlim();
 readonly IntPtr window;
 readonly uint owner;
 readonly AutomationElement workspace;
 readonly string folder;
 readonly Rectangle area;
 Thread observer,camera;
 volatile bool stopping;
 Exception failure;
 public long DownQpc {get;private set;}
 public long UpQpc {get;private set;}
 public int InitialWidth {get;private set;}
 public Sample[] States {get{return states.ToArray();}}
 static Rect Bounds(IntPtr window){Rect r;if(!GetWindowRect(window,out r))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return r;}
 void Guard(){uint process;GetWindowThreadProcessId(window,out process);if(process!=owner)throw new InvalidOperationException("Resize window ownership changed.");GetWindowThreadProcessId(GetForegroundWindow(),out process);if(process!=owner)throw new InvalidOperationException("Resize input lost its owned foreground.");}
 public PanelBridgeCapture(IntPtr window,uint owner,AutomationElement workspace,string folder){
  this.window=window;this.owner=owner;this.workspace=workspace;this.folder=folder;Directory.CreateDirectory(folder);
  var r=Bounds(window);InitialWidth=r.right-r.left;area=new Rectangle(r.left-8,r.top-8,InitialWidth+360,r.bottom-r.top+16);
  File.WriteAllText(Path.Combine(folder,"origin.json"),"{\"x\":"+area.X+",\"y\":"+area.Y+",\"width\":"+area.Width+",\"height\":"+area.Height+",\"qpc_frequency\":"+Stopwatch.Frequency+"}");
 }
 public void Arm(){
  Guard();observer=new Thread(()=>{try{
   SetThreadDpiAwarenessContext(new IntPtr(-4));int index=0;
   while(!stopping){
    long begin=Stopwatch.GetTimestamp();var before=Bounds(window);string json=workspace.Current.ItemStatus;var after=Bounds(window);long end=Stopwatch.GetTimestamp();
    if(string.IsNullOrEmpty(json))throw new InvalidOperationException("Workspace presentation is unavailable.");
    string path=Path.Combine(folder,"state-"+(index++).ToString("D4")+".json");File.WriteAllText(path,json);
    states.Add(new Sample{begin=begin,end=end,width_begin=before.right-before.left,width_end=after.right-after.left,height=after.bottom-after.top,path=path});stateReady.Set();Thread.Sleep(20);
   }
  }catch(Exception error){failure=error;stopping=true;stateReady.Set();}});
  camera=new Thread(()=>{try{
   SetThreadDpiAwarenessContext(new IntPtr(-4));int index=0;
   using(var log=new StreamWriter(Path.Combine(folder,"frames.csv"))){log.WriteLine("frame,qpc_begin,qpc_end");
    while(!stopping&&index<160){long begin=Stopwatch.GetTimestamp();string name="frame-"+(index++).ToString("D4")+".png";
     using(var image=new Bitmap(area.Width,area.Height,PixelFormat.Format32bppArgb)){using(var graphics=Graphics.FromImage(image))graphics.CopyFromScreen(area.Location,System.Drawing.Point.Empty,new Size(area.Width,area.Height));image.Save(Path.Combine(folder,name),ImageFormat.Png);}
     log.WriteLine(name+","+begin+","+Stopwatch.GetTimestamp());log.Flush();cameraReady.Set();
     int remaining=50-(int)((Stopwatch.GetTimestamp()-begin)*1000/Stopwatch.Frequency);if(remaining>0)Thread.Sleep(remaining);
    }
   }
  }catch(Exception error){failure=error;stopping=true;cameraReady.Set();}});
  observer.SetApartmentState(ApartmentState.MTA);observer.IsBackground=true;camera.IsBackground=true;observer.Start();camera.Start();
  if(!stateReady.Wait(10000)||!cameraReady.Wait(10000))throw new TimeoutException("Resize observers did not arm.");
  if(failure!=null)throw failure;
 }
 public void Drag(Type injector){
  Guard();var r=Bounds(window);int y=(r.top+r.bottom)/2,x=0;
  for(int inset=2;inset<=12;inset++){
   int candidate=r.right-inset;uint process;GetWindowThreadProcessId(WindowFromPoint(new Point{x=candidate,y=y}),out process);
   long packed=((long)(y&65535)<<16)|(uint)(candidate&65535);
   if(process==owner&&SendMessage(window,0x84,IntPtr.Zero,new IntPtr(packed)).ToInt64()==11){x=candidate;break;}
  }
  if(x==0)throw new InvalidOperationException("No owned HT_RIGHT resize border was found.");
  MethodInfo move=injector.GetMethod("Move"),button=injector.GetMethod("Button");
  if(move==null||button==null)throw new InvalidOperationException("Existing mouse injector is unavailable.");
  using(var log=new StreamWriter(Path.Combine(folder,"input.csv"))){log.WriteLine("action,qpc_begin,qpc_end,x,y,width");
   move.Invoke(null,new object[]{x,y});Guard();bool down=false;
   try{
    DownQpc=Stopwatch.GetTimestamp();button.Invoke(null,new object[]{2u});down=true;log.WriteLine("down,"+DownQpc+","+Stopwatch.GetTimestamp()+","+x+","+y+","+InitialWidth);log.Flush();
    var clock=Stopwatch.StartNew();
    while(clock.ElapsedMilliseconds<3000){
     Guard();double phase=(clock.ElapsedMilliseconds%1500)/750.0;int next=x+(int)(300*(phase<=1?phase:2-phase));
     long begin=Stopwatch.GetTimestamp();move.Invoke(null,new object[]{next,y});var current=Bounds(window);
     log.WriteLine("move,"+begin+","+Stopwatch.GetTimestamp()+","+next+","+y+","+(current.right-current.left));log.Flush();Thread.Sleep(16);
    }
   }finally{if(down){UpQpc=Stopwatch.GetTimestamp();button.Invoke(null,new object[]{4u});log.WriteLine("up,"+UpQpc+","+Stopwatch.GetTimestamp()+",0,0,0");}}
  }
 }
 public void Dispose(){stopping=true;if(observer!=null&&!observer.Join(10000))throw new TimeoutException("Resize state observer did not retire.");if(camera!=null&&!camera.Join(10000))throw new TimeoutException("Resize camera did not retire.");stateReady.Dispose();cameraReady.Dispose();if(failure!=null)throw failure;}
}
