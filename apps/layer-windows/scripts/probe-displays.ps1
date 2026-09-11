$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class CapyDisplayProbe {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    public struct Device {
        public int cb;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string name;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=128)] public string description;
        public uint flags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=128)] public string id;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=128)] public string key;
    }
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    static extern bool EnumDisplayDevices(string device, uint index, ref Device result, uint flags);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    static extern bool EnumDisplaySettings(string device, int mode, IntPtr settings);
    public static object[] Read() {
        var result = new List<object>();
        for (uint index=0;;index++) {
            var d = new Device { cb = Marshal.SizeOf(typeof(Device)) };
            if (!EnumDisplayDevices(null,index,ref d,0)) break;
            if ((d.flags & 1)==0) continue;
            IntPtr mode = Marshal.AllocHGlobal(220);
            try {
                Marshal.Copy(new byte[220],0,mode,220);
                Marshal.WriteInt16(mode,68,220);
                if (!EnumDisplaySettings(d.name,-1,mode)) continue;
                var monitor = new Device { cb = Marshal.SizeOf(typeof(Device)) };
                EnumDisplayDevices(d.name,0,ref monitor,0);
                result.Add(new {
                    device=d.name, adapter=d.description, monitor=monitor.description,
                    primary=(d.flags & 4)!=0, x=Marshal.ReadInt32(mode,76), y=Marshal.ReadInt32(mode,80),
                    width=Marshal.ReadInt32(mode,172), height=Marshal.ReadInt32(mode,176),
                    refreshHz=Marshal.ReadInt32(mode,184)
                });
            } finally { Marshal.FreeHGlobal(mode); }
        }
        return result.ToArray();
    }
}
'@
[CapyDisplayProbe]::Read() | ConvertTo-Json -Depth 4
