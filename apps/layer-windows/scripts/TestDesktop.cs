using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;

public static class CapyPrivateDesktop {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    struct Startup {
        public uint size; public string reserved, desktop, title;
        public uint x,y,width,height,charsX,charsY,fill,flags;
        public ushort show,reservedCount; public IntPtr reservedData,input,output,error;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct ProcessInfo {public IntPtr process,thread;public uint id,threadId;}
    [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateDesktop(string name,IntPtr device,IntPtr mode,uint flags,uint access,IntPtr security);
    [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr OpenDesktop(string name,uint flags,bool inherit,uint access);
    [DllImport("user32.dll",SetLastError=true)] static extern IntPtr OpenInputDesktop(uint flags,bool inherit,uint access);
    [DllImport("user32.dll",SetLastError=true)] static extern bool SwitchDesktop(IntPtr desktop);
    [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcess(string application,StringBuilder command,IntPtr processSecurity,IntPtr threadSecurity,bool inherit,uint flags,IntPtr environment,string directory,ref Startup startup,out ProcessInfo process);
    [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr security,string name);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job,int kind,byte[] data,uint size);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr job,IntPtr process);
    [DllImport("kernel32.dll",SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle,uint milliseconds);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process,out uint code);
    [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr process,uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    public static int Run(string powershell,string script,string directory,string desktopName,int seconds) {
        IntPtr original=OpenInputDesktop(0,false,0x101), desktop=IntPtr.Zero,job=IntPtr.Zero;
        ProcessInfo child=new ProcessInfo();bool switched=false;
        if(original==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
        try {
            desktop=desktopName=="Default"?OpenDesktop(desktopName,0,false,0x1ff):CreateDesktop(desktopName,IntPtr.Zero,IntPtr.Zero,0,0x1ff,IntPtr.Zero);
            if(desktop==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
            job=CreateJobObject(IntPtr.Zero,null);
            if(job==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
            var limits=new byte[144];Array.Copy(BitConverter.GetBytes(0x2000u),0,limits,16,4);
            if(!SetInformationJobObject(job,9,limits,144))throw new Win32Exception(Marshal.GetLastWin32Error());
            var startup=new Startup{size=(uint)Marshal.SizeOf(typeof(Startup)),desktop="winsta0\\"+desktopName};
            var command=new StringBuilder("\""+powershell+"\" -NoProfile -ExecutionPolicy RemoteSigned -File \""+script+"\"");
            if(!CreateProcess(powershell,command,IntPtr.Zero,IntPtr.Zero,false,0x08000004,IntPtr.Zero,directory,ref startup,out child))throw new Win32Exception(Marshal.GetLastWin32Error());
            if(!AssignProcessToJobObject(job,child.process))throw new Win32Exception(Marshal.GetLastWin32Error());
            if(!SwitchDesktop(desktop))throw new Win32Exception(Marshal.GetLastWin32Error());
            switched=true;
            if(ResumeThread(child.thread)==uint.MaxValue)throw new Win32Exception(Marshal.GetLastWin32Error());
            var timer=Stopwatch.StartNew();
            while(WaitForSingleObject(child.process,200)==258)if(timer.Elapsed.TotalSeconds>seconds)throw new TimeoutException("Private desktop test exceeded its deadline.");
            uint code;if(!GetExitCodeProcess(child.process,out code))throw new Win32Exception(Marshal.GetLastWin32Error());
            return (int)code;
        } finally {
            if(child.process!=IntPtr.Zero){uint code;if(GetExitCodeProcess(child.process,out code)&&code==259)TerminateProcess(child.process,1);}
            if(job!=IntPtr.Zero)CloseHandle(job);
            var restoreFailure=switched&&!SwitchDesktop(original)?new Win32Exception(Marshal.GetLastWin32Error(),"Could not restore the original input desktop."):null;
            if(child.thread!=IntPtr.Zero)CloseHandle(child.thread);
            if(child.process!=IntPtr.Zero)CloseHandle(child.process);
            if(desktop!=IntPtr.Zero)CloseDesktop(desktop);
            CloseDesktop(original);
            if(restoreFailure!=null)throw restoreFailure;
        }
    }
}
