using System;using System.Collections.Generic;using System.Runtime.InteropServices;
public static class CapyHardwareDisplay {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct Device {
  public int cb;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)] public string name;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=128)] public string description;
  public uint flags;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=128)] public string id;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=128)] public string key;
 }
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct Mode {
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)] public string DeviceName;
  public ushort SpecVersion,DriverVersion,Size,DriverExtra;public uint Fields;
  public int PositionX,PositionY;public uint Orientation,FixedOutput;
  public short Color,Duplex,YResolution,TTOption,Collate;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)] public string FormName;
  public ushort LogPixels;public uint BitsPerPixel,Width,Height,DisplayFlags,Frequency,IcmMethod,IcmIntent,MediaType,DitherType,Reserved1,Reserved2,PanningWidth,PanningHeight;
 }
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern bool EnumDisplayDevices(string device,uint index,ref Device result,uint flags);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern bool EnumDisplaySettings(string device,int index,ref Mode mode);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int ChangeDisplaySettingsEx(string device,ref Mode mode,IntPtr window,uint flags,IntPtr parameter);
 [DllImport("user32.dll",EntryPoint="ChangeDisplaySettingsExW",CharSet=CharSet.Unicode)]static extern int ApplyAll(string device,IntPtr mode,IntPtr window,uint flags,IntPtr parameter);
 public static Device[] Devices(){var rows=new List<Device>();for(uint i=0;;i++){var d=new Device{cb=Marshal.SizeOf(typeof(Device))};if(!EnumDisplayDevices(null,i,ref d,0))break;rows.Add(d);}return rows.ToArray();}
 [StructLayout(LayoutKind.Sequential)] struct Point { public int x,y; }
 [DllImport("user32.dll")] static extern IntPtr MonitorFromPoint(Point point,uint flags);
 [DllImport("shcore.dll")] static extern int GetScaleFactorForMonitor(IntPtr monitor,out int scale);
 public static object Verify(uint width,uint height){
  foreach(var d in Devices())if((d.flags&5)==5){
   var m=new Mode{Size=(ushort)Marshal.SizeOf(typeof(Mode))};
   if(!EnumDisplaySettings(d.name,-1,ref m))throw new Exception("Cannot read primary mode");
   if(d.description==null||!d.description.Contains("NVIDIA")||m.Width!=width||m.Height!=height||m.Frequency!=120||m.PositionX!=0||m.PositionY!=0)throw new Exception("Primary desktop is not the requested NVIDIA mode");
   int scale;int status=GetScaleFactorForMonitor(MonitorFromPoint(new Point(),2),out scale);if(status!=0)throw new Exception("Cannot read primary monitor scale: "+status);
   return new{adapter=d.description,device=d.name,primary=true,x=m.PositionX,y=m.PositionY,width=m.Width,height=m.Height,refreshHz=m.Frequency,scalePercent=scale,dpi=96*scale/100.0};
  }
  throw new Exception("No active primary display");
 }
 public static bool IsReady(uint width,uint height){try{Verify(width,height);return true;}catch{return false;}}
 public static Device SelectSource(Device[] devices){
  Device source=new Device();
  foreach(var d in devices)if(d.description!=null&&d.description.Contains("NVIDIA")&&(d.flags&1)!=0){
   if(source.name!=null)throw new Exception("Multiple active NVIDIA display sources");
   source=d;
  }
  if(source.name==null)throw new Exception("No active NVIDIA display source");
  return source;
 }
 public static object Set(uint width, uint height){
  if(IsReady(width,height))return Verify(width,height);
  var devices=Devices();Device gpu=SelectSource(devices);
  Mode best=new Mode();
  for(int i=0;;i++){var candidate=new Mode{Size=(ushort)Marshal.SizeOf(typeof(Mode))};if(!EnumDisplaySettings(gpu.name,i,ref candidate))break;if(candidate.Width==width&&candidate.Height==height&&candidate.BitsPerPixel==32&&candidate.Frequency==120)best=candidate;}
  if(best.Width==0){var candidate=new Mode{Size=(ushort)Marshal.SizeOf(typeof(Mode))};if(EnumDisplaySettings(gpu.name,-1,ref candidate)&&candidate.Width==width&&candidate.Height==height&&candidate.Frequency==120)best=candidate;}
  if(best.Width==0)throw new Exception("Requested NVIDIA desktop mode unavailable");
  best.PositionX=0;best.PositionY=0;best.Fields=0x20|0x40000|0x80000|0x100000|0x400000;
  int test=ChangeDisplaySettingsEx(gpu.name,ref best,IntPtr.Zero,2,IntPtr.Zero);if(test!=0)throw new Exception("GPU mode validation failed: "+test);
  var before=new Dictionary<string,Mode>();
  foreach(var d in devices){var old=new Mode{Size=(ushort)Marshal.SizeOf(typeof(Mode))};if((d.flags&1)!=0&&EnumDisplaySettings(d.name,-1,ref old))before[d.name]=old;}
  int staged=ChangeDisplaySettingsEx(gpu.name,ref best,IntPtr.Zero,0x10000011,IntPtr.Zero);
  if(staged!=0)throw new Exception("NVIDIA primary staging failed: "+staged);
  try{
   foreach(var d in devices){if(d.name==gpu.name||!before.ContainsKey(d.name))continue;var old=before[d.name];old.PositionX=(int)width;old.PositionY=0;old.Fields=0x20;int moved=ChangeDisplaySettingsEx(d.name,ref old,IntPtr.Zero,0x10000001,IntPtr.Zero);if(moved!=0)throw new Exception("Secondary staging failed: "+moved);}
   int applied=ApplyAll(null,IntPtr.Zero,IntPtr.Zero,0,IntPtr.Zero);
   if(applied!=0)throw new Exception("Desktop activation failed: "+applied);
   return Verify(width,height);
  }catch(Exception error){
   int restore=0;
   foreach(var d in devices){if(!before.ContainsKey(d.name))continue;var old=before[d.name];old.Fields=0x20|0x40000|0x80000|0x100000|0x400000;int status=ChangeDisplaySettingsEx(d.name,ref old,IntPtr.Zero,0x10000001u|((d.flags&4)!=0?0x10u:0u),IntPtr.Zero);if(status!=0)restore=status;}
   int applied=ApplyAll(null,IntPtr.Zero,IntPtr.Zero,0,IntPtr.Zero);
   throw new Exception("Desktop configuration failed; restore staging="+restore+", apply="+applied,error);
  }
 }
}
public static class CapyHardwareEdid {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern IntPtr LoadLibrary(string name);
 [DllImport("kernel32.dll")] static extern IntPtr GetProcAddress(IntPtr lib,string name);
 [UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate IntPtr Query(uint id);
 [UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate int Init();
 [UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate int EnumGpu([Out]IntPtr[] handles,out int count);
 [UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate int Outputs(IntPtr gpu,out uint outputs);
 [UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate int Edid(IntPtr gpu,uint output,IntPtr data);
 static Query query;
 static T Fn<T>(uint id){var p=query(id);if(p==IntPtr.Zero)throw new Exception("Missing NVAPI function");return (T)(object)Marshal.GetDelegateForFunctionPointer(p,typeof(T));}
 static bool Equal(byte[] a,byte[] b){if(a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true;}
 public static object Run(byte[] edid, byte[] owned, uint ownedOutput){
  var lib=LoadLibrary("nvapi64.dll");if(lib==IntPtr.Zero)throw new Exception("Signed NVIDIA driver NVAPI unavailable");query=(Query)Marshal.GetDelegateForFunctionPointer(GetProcAddress(lib,"nvapi_QueryInterface"),typeof(Query));
  if(Fn<Init>(0x0150e828)()!=0)throw new Exception("NVAPI initialize failed");
  IntPtr[] handles=new IntPtr[64];int count;
  if(Fn<EnumGpu>(0xe5ac921f)(handles,out count)!=0||count!=1)throw new Exception("Expected assigned single GPU");
  uint connected,active,all;
  if(Fn<Outputs>(0x1730bfc9)(handles[0],out connected)!=0||Fn<Outputs>(0xe3e89b6f)(handles[0],out active)!=0||Fn<Outputs>(0x7d554f8e)(handles[0],out all)!=0)throw new Exception("NVAPI connector enumeration failed");
  
  uint output=ownedOutput!=0?ownedOutput:all&(~all+1);if((all&output)!=output)throw new Exception("Owned connector no longer exists");if(output==0)throw new Exception("GPU has no output connectors");
  
  IntPtr memory=Marshal.AllocHGlobal(272);
  try{
   Marshal.Copy(new byte[272],0,memory,272);Marshal.WriteInt32(memory,0,0x30110);
   int before=Fn<Edid>(0x37d32e69)(handles[0],output,memory);
   int length=before==0?Marshal.ReadInt32(memory,260):0;
   var previous=new byte[Math.Min(Math.Max(length,0),256)];Marshal.Copy(IntPtr.Add(memory,4),previous,0,previous.Length);
   bool same=Equal(previous,edid),ours=owned!=null&&Equal(previous,owned);
   if(same)return new{output_mask=output,reused=true};
   if((connected!=0||active!=0||length>0)&&!ours)throw new Exception("GPU has an unowned display or EDID; refusing to replace it");
   Marshal.Copy(new byte[272],0,memory,272);Marshal.WriteInt32(memory,0,0x30110);Marshal.Copy(edid,0,IntPtr.Add(memory,4),edid.Length);Marshal.WriteInt32(memory,260,edid.Length);
   int set=Fn<Edid>(0xe83d6456)(handles[0],output,memory);
   Fn<Outputs>(0x1730bfc9)(handles[0],out connected);Fn<Outputs>(0xe3e89b6f)(handles[0],out active);
   if(set!=0)throw new Exception("NVAPI EDID override failed: "+set);
   return new{output_mask=output,reused=false};
  }finally{Marshal.FreeHGlobal(memory);Fn<Init>(0xd22bdd7e)();}
 }
}
