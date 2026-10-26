$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class CapyVmDisplay {
  const uint UseBitsPerPixel=0x40000, UseWidth=0x80000, UseHeight=0x100000, TestMode=2, Fullscreen=4;
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
  public struct Mode {
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string DeviceName;
    public ushort SpecVersion, DriverVersion, Size, DriverExtra;
    public uint Fields;
    public int PositionX, PositionY;
    public uint Orientation, FixedOutput;
    public short Color, Duplex, YResolution, TTOption, Collate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string FormName;
    public ushort LogPixels;
    public uint BitsPerPixel, Width, Height, DisplayFlags, Frequency;
    public uint IcmMethod, IcmIntent, MediaType, DitherType, Reserved1, Reserved2, PanningWidth, PanningHeight;
  }
  [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  static extern bool EnumDisplaySettings(string device, int index, ref Mode mode);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  static extern int ChangeDisplaySettingsEx(string device, ref Mode mode, IntPtr window, uint flags, IntPtr parameter);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
  static Mode Current() {
    var mode = new Mode { Size=(ushort)Marshal.SizeOf(typeof(Mode)) };
    if (!EnumDisplaySettings(null, -1, ref mode)) throw new Win32Exception(Marshal.GetLastWin32Error());
    return mode;
  }
  public static void SetSize(uint width, uint height) {
    var best=new Mode();string available="";
    for(int index=0;;index++) {
      var candidate=new Mode { Size=(ushort)Marshal.SizeOf(typeof(Mode)) };
      if(!EnumDisplaySettings(null,index,ref candidate))break;
      available+=candidate.Width+"x"+candidate.Height+"@"+candidate.BitsPerPixel+" ";
      if(candidate.BitsPerPixel!=32||candidate.Width<1920||candidate.Height<1080||candidate.Width>width||candidate.Height>height)continue;
      candidate.Fields=UseBitsPerPixel|UseWidth|UseHeight;
      if(ChangeDisplaySettingsEx(null,ref candidate,IntPtr.Zero,TestMode,IntPtr.Zero)!=0)continue;
      if((ulong)candidate.Width*candidate.Height>(ulong)best.Width*best.Height)best=candidate;
    }
    if(best.Width==0)throw new InvalidOperationException("VM has no supported GUI fixture display of at least 1920x1080. Available: "+available);
    int result=ChangeDisplaySettingsEx(null,ref best,IntPtr.Zero,Fullscreen,IntPtr.Zero);
    if(result!=0)throw new InvalidOperationException("VM display mode rejected: "+result);
    var mode=Current();
    if(mode.Width!=best.Width||mode.Height!=best.Height)throw new InvalidOperationException("VM display size did not change");
    int actualWidth=GetSystemMetrics(0), actualHeight=GetSystemMetrics(1);
    if(actualWidth!=best.Width||actualHeight!=best.Height)throw new InvalidOperationException("Private VM desktop is "+actualWidth+"x"+actualHeight+"; expected "+best.Width+"x"+best.Height);
    Console.WriteLine("VM display: "+mode.Width+"x"+mode.Height+", "+mode.BitsPerPixel+" bpp");
  }
}
'@
[CapyVmDisplay]::SetSize(2560,1600)
