param(
    [Parameter(Mandatory)][int]$ProcessId,
    [ValidateRange(5,60)][int]$Seconds=20,
    [string]$PresentMon,
    [string]$OutputDirectory
)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
if(!$PresentMon){$PresentMon=Join-Path $env:USERPROFILE '.local/tools/presentmon/2.5.1/PresentMon-2.5.1-x64.exe'}
$PresentMon=(Resolve-Path -LiteralPath $PresentMon).Path
$app=Get-Process -Id $ProcessId
if([IO.Path]::GetFileName($app.Path) -ne 'CapyCanvas.exe'){throw 'The target must be the Capy Canvas probe process.'}
$probePath=Join-Path (Split-Path -Parent $app.Path) 'presentation-probe.json'
$probeFile=Get-Item -LiteralPath $probePath
$probe=Get-Content -LiteralPath $probePath -Raw | ConvertFrom-Json
if($probe.process_id -ne $ProcessId -or $probeFile.LastWriteTime -lt $app.StartTime){throw 'Probe metadata does not belong to this app launch.'}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyPresentationMonitor {
    [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
    [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)]
    public struct Info {
        public uint size;public Rect monitor,work;public uint flags;
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)]public string device;
    }
    [DllImport("user32.dll")]static extern IntPtr MonitorFromWindow(IntPtr window,uint flags);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern bool GetMonitorInfo(IntPtr monitor,ref Info info);
    public static string Device(IntPtr window) {
        var info=new Info{size=(uint)Marshal.SizeOf(typeof(Info))};
        if(!GetMonitorInfo(MonitorFromWindow(window,2),ref info))throw new Exception("Cannot identify the app display.");
        return info.device;
    }
}
'@
$device=[CapyPresentationMonitor]::Device($app.MainWindowHandle)
$displays=& (Join-Path $PSScriptRoot 'probe-displays.ps1') | ConvertFrom-Json
$display=$displays | Where-Object device -eq $device
if(!$display -or $display.refreshHz -lt 120){throw 'Move the probe to an active display running at 120 Hz or higher.'}
if(!$OutputDirectory){$OutputDirectory=Join-Path $repo ('artifacts/windows/presentation/'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss'))}
$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$csv=Join-Path $OutputDirectory 'frames.csv'
$log=Join-Path $OutputDirectory 'capture.log'
[pscustomobject]@{
    scope=$probe.scope
    process_id=$ProcessId
    seconds=$Seconds
    probe=$probe
    display=$display
    executable_sha256=(Get-FileHash -LiteralPath $app.Path -Algorithm SHA256).Hash
    runtime_sha256=(Get-FileHash -LiteralPath (Join-Path (Split-Path -Parent $app.Path) 'layer_windows.dll') -Algorithm SHA256).Hash
    presentmon_sha256=(Get-FileHash -LiteralPath $PresentMon -Algorithm SHA256).Hash
    started_utc=[DateTime]::UtcNow.ToString('o')
    input_tracking=$false
} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'capture.json')
# Never attach to all processes, log raw keyboard input, restart elevated, or
# stop another tracing session. The caller controls any required elevation.
& $PresentMon --process_id $ProcessId --timed $Seconds --terminate_after_timed --no_console_stats --no_track_input --qpc_time_ms --session_name "CapyCanvas-$PID" --output_file $csv *> $log
if($LASTEXITCODE -ne 0) {
    Get-Content -LiteralPath $log -Tail 8
    throw 'PresentMon capture failed. If Windows denied ETW access, run this capture script in Administrator PowerShell; keep the app at normal privilege.'
}
$currentProbe=Get-Content -LiteralPath $probePath -Raw | ConvertFrom-Json
if($currentProbe.ready_qpc_ns -ne $probe.ready_qpc_ns){throw 'The probe was resized or reconfigured during capture. Keep the window fixed and repeat.'}
if(!(Test-Path -LiteralPath $csv)){throw 'PresentMon did not write frame records.'}
$rows=Import-Csv -LiteralPath $csv
if(!$rows){throw 'PresentMon captured no frames.'}
if($rows | Where-Object {[int]$_.ProcessID -ne $ProcessId}){throw 'Capture unexpectedly contains another process; do not publish it.'}
function Address([string]$Value){[Convert]::ToUInt64(($Value -replace '^0x',''),16)}
$addresses=@($probe.swap_chain_addresses | ForEach-Object {Address $_})
$canvas=@($rows | Where-Object {(Address $_.SwapChainAddress) -in $addresses})
if(!$canvas.Count){throw 'No captured swap chain matches the native canvas identity; no performance conclusion can be drawn.'}
[pscustomobject]@{
    scope=$probe.scope
    canvas_frame_records=$canvas.Count
    total_app_frame_records=$rows.Count
    local_output=$OutputDirectory
    result='Captured; display cadence and dropped frames still need analysis. This is not input latency or painting acceptance.'
} | ConvertTo-Json
