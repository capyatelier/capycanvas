param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -AssemblyName System.Drawing
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/file-activation/'+[Guid]::NewGuid().ToString('N'))
$elsewhere=Join-Path $run 'elsewhere'
[IO.Directory]::CreateDirectory($elsewhere)|Out-Null
$CapyTraceDirectory=$run
$review=$null;$other=$null
if(!('CapyFileDrag' -as [type])){Add-Type -TypeDefinition @"
using System;using System.ComponentModel;using System.Runtime.InteropServices;using System.Threading;
public static class CapyFileDrag {
 [StructLayout(LayoutKind.Sequential)] struct Mouse {public int dx,dy;public uint data,flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)] struct Input {[FieldOffset(0)]public uint type;[FieldOffset(8)]public Mouse mouse;}
 [StructLayout(LayoutKind.Sequential)] struct Rect {public int left,top,right,bottom;}
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 [DllImport("user32.dll")] static extern int GetSystemMetrics(int code);
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 public static volatile Exception Failure;
 static void Send(uint flags,int x,int y){
  var mouse=new Mouse{dx=(x-GetSystemMetrics(76))*65535/(GetSystemMetrics(78)-1),dy=(y-GetSystemMetrics(77))*65535/(GetSystemMetrics(79)-1),flags=flags|0xC001};
  if(SendInput(1,new[]{new Input{mouse=mouse}},40)!=1)throw new Win32Exception(Marshal.GetLastWin32Error());
 }
 public static Thread Press(IntPtr source,int x,int y){
  Rect rect;if(!GetWindowRect(source,out rect))throw new Win32Exception(Marshal.GetLastWin32Error());
  int sx=(rect.left+rect.right)/2,sy=(rect.top+rect.bottom)/2;
  Send(0,sx,sy);Send(2,sx,sy);
  Failure=null;
  var glide=new Thread(()=>{
   try{
    Thread.Sleep(400);
    for(int step=1;step<=16;step++){Send(0,sx+(x-sx)*step/16,sy+(y-sy)*step/16);Thread.Sleep(40);}
    for(int dwell=0;dwell<12;dwell++){Send(0,x+dwell%2,y);Thread.Sleep(60);}
   }catch(Exception error){Failure=error;}
   finally{try{Send(4,x,y);}catch(Exception error){Failure=Failure??error;}}
  });
  glide.Start();return glide;
 }
}
"@}
function Drop-Files([string[]]$Paths,[string]$Target){
    [CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $bounds=(Control $Target -Arranged).Current.BoundingRectangle
    $shell=[powershell]::Create();$shell.Runspace=[runspacefactory]::CreateRunspace();$shell.Runspace.ApartmentState='STA';$shell.Runspace.Open()
    try{
        $null=$shell.AddScript({param($Paths,$X,$Y)
            Add-Type -AssemblyName System.Windows.Forms,System.Drawing
            [CapyFileDrag]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
            $source=[Windows.Forms.Form]@{FormBorderStyle='None';ShowInTaskbar=$false;TopMost=$true;StartPosition='Manual';Bounds=[Drawing.Rectangle]::new($X-40,$Y+160,80,80)}
            $source.Show();[Windows.Forms.Application]::DoEvents()
            $glide=[CapyFileDrag]::Press($source.Handle,$X,$Y);[Windows.Forms.Application]::DoEvents()
            $effect=$source.DoDragDrop([Windows.Forms.DataObject]::new([Windows.Forms.DataFormats]::FileDrop,[string[]]$Paths),[Windows.Forms.DragDropEffects]::Copy)
            if(!$glide.Join(10000)){throw 'The file drag did not release'}
            $source.Close();if([CapyFileDrag]::Failure){throw [CapyFileDrag]::Failure};[string]$effect
        }).AddArgument($Paths).AddArgument([int]($bounds.X+$bounds.Width/2)).AddArgument([int]($bounds.Y+$bounds.Height/2))
        $result=$shell.Invoke();if($shell.HadErrors){throw ($shell.Streams.Error|Out-String)};[string]$result[-1]
    }finally{$shell.Runspace.Dispose();$shell.Dispose()}
}
function Tabs {@((Model).windows_tabs.tabs)}
function Image([string]$Name,[Drawing.Color]$Color){
    $path=Join-Path $run $Name
    $bitmap=[Drawing.Bitmap]::new(24,16,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try{for($x=0;$x -lt 24;$x++){for($y=0;$y -lt 16;$y++){$bitmap.SetPixel($x,$y,$Color)}};$bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
    $path
}
function Windows {$path=Join-Path $run "windows-$($review.Id).json";if([IO.File]::Exists($path)){@((Read-Snapshot $path).windows)}else{@()}}
function Forward([string]$Directory,[string[]]$Arguments){
    $start=@{FilePath=$Executable;WorkingDirectory=$Directory;PassThru=$true}
    if($Arguments){$start.ArgumentList=$Arguments|ForEach-Object {'"'+$_+'"'}}
    $launch=Start-Process @start
    if(!$launch.WaitForExit(20000)){Stop-Process -Id $launch.Id -Force;throw 'A launch did not hand itself to the running app'}
    if($launch.ExitCode -ne 0){throw "A forwarding launch failed: $($launch.ExitCode)"}
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle
    Write-Output "Owned file activation review $($review.Id): $run"
    Wait-Until {$m=Model;$m.brush_ready -and $m.state.theme -eq $Theme -and $m.windows_workspace.ready -and !$m.windows_workspace.busy -and @($m.state.commands|Where-Object {$_.id -eq 'open_document' -and $_.enabled}).Count} 'File activation review did not start' 90
    $before=@(Tabs).Count
    $first=Image 'First 日本語.png' ([Drawing.Color]::FromArgb(255,200,40,40))
    $second=Image 'Second drawing.png' ([Drawing.Color]::FromArgb(255,40,60,200))
    Forward $elsewhere @($first,$second)
    Wait-Until {@(Tabs).Count -eq $before+2 -and !(Model).state.document_file.busy} 'Forwarded files did not open as drawings in the running window' 60
    $titles=@(Tabs|ForEach-Object title)
    if($titles[-2] -notlike 'First*' -or $titles[-1] -notlike 'Second drawing*'){throw "Forwarded drawings opened out of order: $($titles -join ', ')"}
    if(@((Model).windows_tabs.tabs)[-1].id -ne (Model).windows_tabs.selected){throw 'The last forwarded drawing is not selected'}
    $null=Image 'Relative.png' ([Drawing.Color]::FromArgb(255,40,160,60))
    Forward $run @('Relative.png')
    Wait-Until {@(Tabs).Count -eq $before+3 -and (@(Tabs)[-1].title -like 'Relative*')} 'A relative path was not resolved by the launching process' 60
    $original=@(Windows)
    Forward $elsewhere @()
    Wait-Until {@(Windows).Count -eq 2} 'A launch without files did not open a new window in the running app' 60
    if(@(Tabs).Count -ne $before+3){throw 'A launch without files changed the existing window'}
    $added=@(Windows|Where-Object {$_.hwnd -notin $original.hwnd})[0]
    [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr][int64]$added.hwnd).GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
    Wait-Until {@(Windows).Count -eq 1} 'The window opened by a later launch did not close' 30
    $env:CAPY_STORAGE_DIR=Join-Path $run 'other-profile'
    $other=Start-Process -FilePath $Executable -WorkingDirectory $elsewhere -ArgumentList ('"'+$first+'"') -PassThru
    Start-Sleep -Seconds 6
    if($other.HasExited){throw 'A launch with other app storage was forwarded to this app'}
    Stop-Process -Id $other.Id -Force;$other.WaitForExit()
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    if(@(Tabs).Count -ne $before+3){throw 'Another profile changed this window'}
    $drawing=@{window=$null}
    Wait-Until {$drawing.window=Owned-DrawingWindow $review;$null -ne $drawing.window} 'The drawing window is unavailable'
    $root=$drawing.window.Root;$drawingWindow=$drawing.window.Handle
    [CapyWindowApi]::SetForegroundWindow($drawingWindow)|Out-Null
    $saved=Join-Path $run 'Dropped drawing.capy'
    Save-ProjectAs $saved
    $photo=Image 'Dropped photo.png' ([Drawing.Color]::FromArgb(255,220,160,30))
    $source=Model;$layers=@($source.state.layers|Select-Object id,name,paint_revision,mask_revision)|ConvertTo-Json -Compress
    $revision=$source.state.document_file.revision;$tab=$source.windows_tabs.selected;$count=@(Tabs).Count
    $effect=Drop-Files @($photo) 'drawing-canvas'
    if($effect -ne 'Copy'){throw "The canvas refused the dropped photo: $effect"}
    Wait-Until {$m=Model;$m.state.layer_tools.tool -eq 'transform' -and @($m.state.layers).Count -eq @($source.state.layers).Count+1 -and !$m.state.document_file.busy} 'The canvas photo drop did not open Place' 60
    if(@(Tabs).Count -ne $count -or (Model).windows_tabs.selected -ne $tab){throw 'Canvas placement opened or selected another drawing'}
    Invoke-Id 'canvas-bar-apply_transform'
    Wait-Until {$m=Model;$m.state.layer_tools.tool -ne 'transform' -and $m.state.document_file.revision -gt $revision -and @($m.state.layers).Count -eq @($source.state.layers).Count+1} 'Place did not accept the dropped photo' 60
    $placed=(Model).state.document_file.revision
    Invoke 'Undo' -Name
    Wait-Until {$m=Model;$m.state.document_file.revision -gt $placed -and (@($m.state.layers|Select-Object id,name,paint_revision,mask_revision)|ConvertTo-Json -Compress) -ceq $layers} 'One Undo did not restore the drawing before the photo drop' 60
    $effect=Drop-Files @($photo,$saved) 'drawing-tabs'
    if($effect -ne 'Copy'){throw "The drawing tabs refused an image and a drawing: $effect"}
    Wait-Until {@(Tabs).Count -eq $before+5 -and !(Model).state.document_file.busy} 'Files dropped on the drawing tabs did not open as drawings' 60
    $titles=@(Tabs|Select-Object -Last 2|ForEach-Object title)
    if($titles[0] -notlike 'Dropped photo*' -or $titles[1] -notlike 'Dropped drawing*'){throw "Dropped files opened out of order: $($titles -join ', ')"}
    if(@(Tabs)[-1].id -ne (Model).windows_tabs.selected){throw 'The last dropped drawing is not selected'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
    if(!$review.WaitForExit(20000)){throw 'File activation review did not close'}
    if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
    [pscustomobject]@{forwarded_in_order='passed';relative_path='passed';launch_opens_window='passed';storage_isolated='passed';tab_drop='passed';canvas_photo_drop_place_and_undo='passed';theme=$Theme;evidence=$run}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
} finally {
    if($other -and !$other.HasExited){Stop-Process -Id $other.Id -Force}
    if($review){$review.Refresh();if(!$review.HasExited){Stop-Process -Id $review.Id -Force}}
    Exit-CapyEnvironment
}
