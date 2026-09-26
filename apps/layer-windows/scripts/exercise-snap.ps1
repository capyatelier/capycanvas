param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=10
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class CapySnapWindow {
    [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
    [StructLayout(LayoutKind.Sequential)] public struct Monitor {public int size;public Rect bounds,work;public uint flags;}
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out Rect rect);
    [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h,int command);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int height,bool repaint);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr h,uint flags);
    [DllImport("user32.dll")] static extern bool GetMonitorInfo(IntPtr h,ref Monitor info);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h,int attribute,out Rect rect,int size);
    public static Rect Frame(IntPtr h) {
        Rect rect;Marshal.ThrowExceptionForHR(DwmGetWindowAttribute(h,9,out rect,16));return rect;
    }
    public static Rect WorkArea(IntPtr h) {
        var info=new Monitor{size=Marshal.SizeOf(typeof(Monitor))};
        if(!GetMonitorInfo(MonitorFromWindow(h,2),ref info))throw new Exception("No monitor work area.");
        return info.work;
    }
}
"@
[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/snap/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Signature {
    $model=Model
    @($model.state.document_file,$model.state.layers,$model.state.brush,$model.windows_workspace.id)|ConvertTo-Json -Depth 60 -Compress
}
function Same-Rect($a,$b) {
    [Math]::Abs($a.left-$b.left) -le 2 -and [Math]::Abs($a.top-$b.top) -le 2 -and
        [Math]::Abs($a.right-$b.right) -le 2 -and [Math]::Abs($a.bottom-$b.bottom) -le 2
}
function Ready {
    Wait-Until {
        $model=Model;$client=[CapySnapWindow+Rect]::new()
        if(!$model -or [CapySnapWindow]::IsIconic($handle) -or ![CapySnapWindow]::GetClientRect($handle,[ref]$client)){return $false}
        $canvas=Find 'Drawing canvas' -Name;$origin=[CapyRowPointer+Point]::new()
        if(!$canvas -or ![CapyRowPointer]::ClientToScreen($handle,[ref]$origin)){return $false}
        $bounds=$canvas.Current.BoundingRectangle
        $model.canvas_ready -and $model.brush_ready -and
            $canvas -and $canvas.Current.IsEnabled -and !$canvas.Current.IsOffscreen -and
            [Math]::Abs($model.state.camera.viewport[0]-$bounds.Width) -le .01 -and
            [Math]::Abs($model.state.camera.viewport[1]-$bounds.Height) -le .01 -and
            [Math]::Abs($bounds.Left-$origin.x) -le 1 -and [Math]::Abs($bounds.Top-$origin.y) -le 1 -and
            [Math]::Abs($bounds.Right-$origin.x-$client.right) -le 1 -and
            [Math]::Abs($bounds.Bottom-$origin.y-$client.bottom) -le 1
    } 'Canvas did not settle at the native client size' 45
    $model=Model
    if($model.error -or $model.state.host_error -or $model.titlebar_insets[1] -le 0 -or $model.titlebar_insets[2] -le 0){throw 'Invalid native caption or editor error after resize'}
}
function Patch-Hash {
    $client=[CapySnapWindow+Rect]::new()
    if(![CapySnapWindow]::GetClientRect($handle,[ref]$client)){throw 'Cannot measure client'}
    $bitmap=[Drawing.Bitmap]::new($client.right,$client.bottom)
    try{
        $graphics=[Drawing.Graphics]::FromImage($bitmap)
        try{$dc=$graphics.GetHdc();try{if(![CapySnapWindow]::PrintWindow($handle,$dc,3)){throw 'Cannot capture canvas'}}finally{$graphics.ReleaseHdc($dc)}}finally{$graphics.Dispose()}
        $patch=$bitmap.Clone($sample,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try{
            $stream=[IO.MemoryStream]::new();$hash=[Security.Cryptography.SHA256]::Create()
            try{$patch.Save($stream,[Drawing.Imaging.ImageFormat]::Png);[Convert]::ToHexString($hash.ComputeHash($stream.ToArray()))}finally{$hash.Dispose();$stream.Dispose()}
        }finally{$patch.Dispose()}
    }finally{$bitmap.Dispose()}
}
function Stable-Patch {
    $watch=[Diagnostics.Stopwatch]::StartNew();$last=Patch-Hash;$stable=0
    do{
        Start-Sleep -Milliseconds 100;$next=Patch-Hash
        if($next -eq $last){$stable++}else{$stable=0};$last=$next
        if($stable -ge 3){return $last}
    }while($watch.Elapsed.TotalSeconds -lt 5)
    throw 'Canvas pixels did not settle for comparison'
}
function Check-Paint([string]$Name) {
    $model=Model;$area=$model.state.camera.work_area
    # Sample the stroke interior; its tip and normal hover cursor end outside this rectangle.
    $width=[int][Math]::Min(200,[Math]::Floor($area[2]*.6));$height=[int][Math]::Min(100,[Math]::Floor($area[3]*.8))
    if($width -lt 64 -or $height -lt 40){throw 'Snap canvas is too small for the drawing fixture'}
    $script:sample=[Drawing.Rectangle]::new([int]($area[0]+($area[2]-$width)/2),[int]($area[1]+($area[3]-$height)/2),$width,$height)
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($handle,[ref]$origin)){throw 'Cannot locate canvas'}
    $bounds=(Find 'Drawing canvas' -Name).Current.BoundingRectangle
    $script:sample.Offset([int]($bounds.Left-$origin.x),[int]($bounds.Top-$origin.y))
    $parkX=[int]($bounds.Left+$area[0]+$area[2]/2);$parkY=[int]($bounds.Top+$area[1])+20
    [CapyRowPointer]::SetForegroundWindow($handle)|Out-Null
    foreach($device in @('mouse','pen')){
        [CapyRowPointer]::Hover(($parkX+4),$parkY)
        [CapyRowPointer]::Hover($parkX,$parkY)
        $before=Stable-Patch;$revision=(Model).state.document_file.revision;$modified=(Model).state.document_file.modified
        $x=$origin.x+$sample.X-20;$y=$origin.y+$sample.Y+[int]($sample.Height/2)
        [CapyRowPointer]::Down($device,$x,$y)
        try{for($i=1;$i -le 28;$i++){[CapyRowPointer]::Move(($x+[int]($i*($sample.Width+40)/28)),($y+[int]([Math]::Min(12,$sample.Height/4)*[Math]::Sin($i/4))));Start-Sleep -Milliseconds 8}}finally{[CapyRowPointer]::Up()}
        Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified} 'Resized canvas did not accept a stroke'
        Wait-Until {(Patch-Hash) -ne $before} 'Resized stroke did not appear in the canvas'
        $painted=Stable-Patch;Capture ($Name+'-'+$device)
        $revision=(Model).state.document_file.revision;Invoke 'Undo' -Name
        Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified -eq $modified -and (Patch-Hash) -eq $before} 'One Undo did not restore original canvas pixels'
        $revision=(Model).state.document_file.revision;Invoke 'Redo' -Name
        Wait-Until {(Model).state.document_file.revision -gt $revision -and (Patch-Hash) -eq $painted} 'One Redo did not restore stroke pixels'
        $revision=(Model).state.document_file.revision;Invoke 'Undo' -Name
        Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified -eq $modified -and (Patch-Hash) -eq $before} 'Final Undo did not restore the pre-stroke drawing'
        [CapyRowPointer]::Verify()
        Write-Output "$Name ${device}: visible stroke and one-step Undo/Redo passed"
    }
}
$states=[Collections.Generic.List[object]]::new()
function Record([string]$Name,[string]$Before) {
    Ready
    if((Signature) -ne $Before){throw "Window transition changed the document, layers, brush or workspace: $Name"}
    $model=Model
    $states.Add(@{name=$Name;frame=[CapySnapWindow]::Frame($handle);camera=$model.state.camera;caption=$model.titlebar_insets;scale=([CapyRowPointer]::GetDpiForWindow($handle)/96.)})
    Capture $Name
    Check-Paint $Name
}
try{
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;executable=$Executable;sha256=(Get-FileHash -LiteralPath $Executable).Hash}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $run 'owner.json')
    Write-Output "Owned Snap review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Isolated Snap review did not start' 45
    $handle=$review.MainWindowHandle;$root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Ready
    if((Model).state.document_file.modified){throw 'Snap fixture requires a clean document'}
    $revision=(Model).state.document_file.revision;Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified} 'Seed drawing did not finish'
    $work=[CapySnapWindow]::WorkArea($handle)
    foreach($side in @('left','right')){
        [CapySnapWindow]::ShowWindowAsync($handle,9)|Out-Null
        if(![CapySnapWindow]::MoveWindow($handle,$work.left+48,$work.top+48,[int](($work.right-$work.left)*.7),[int](($work.bottom-$work.top)*.85),$true)){throw 'Cannot restore fixture placement'}
        Ready;$before=Signature
        [CapyRowPointer]::SetForegroundWindow($handle)|Out-Null
        [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x5B),$(if($side -eq 'left'){0x25}else{0x27}))
        $expected=[CapySnapWindow+Rect]::new();$expected.top=$work.top;$expected.bottom=$work.bottom
        $middle=[int](($work.left+$work.right)/2)
        $expected.left=if($side -eq 'left'){$work.left}else{$middle};$expected.right=if($side -eq 'left'){$middle}else{$work.right}
        Wait-Until {(Same-Rect ([CapySnapWindow]::Frame($handle)) $expected)} "Windows did not Snap to the $side half"
        Record $side $before
        $before=Signature;$snapped=[CapySnapWindow]::Frame($handle)
        [CapySnapWindow]::ShowWindowAsync($handle,6)|Out-Null
        Wait-Until {[CapySnapWindow]::IsIconic($handle)} 'Snapped window did not minimize'
        [CapySnapWindow]::ShowWindowAsync($handle,9)|Out-Null
        Wait-Until {![CapySnapWindow]::IsIconic($handle) -and (Same-Rect ([CapySnapWindow]::Frame($handle)) $snapped)} 'Minimize/restore lost the Snap placement'
        Record ($side+'-restored') $before
    }
    $before=Signature;[CapySnapWindow]::ShowWindowAsync($handle,3)|Out-Null
    Wait-Until {[CapySnapWindow]::IsZoomed($handle)} 'Snapped window did not maximize'
    Record 'maximized' $before
    $before=Signature;[CapySnapWindow]::ShowWindowAsync($handle,9)|Out-Null
    Wait-Until {![CapySnapWindow]::IsZoomed($handle)} 'Maximized window did not restore'
    Record 'restored-after-maximize' $before
    $revision=(Model).state.document_file.revision;Invoke 'Undo' -Name
    Wait-Until {(Model).state.document_file.revision -gt $revision -and !(Model).state.document_file.modified} 'One final Undo did not remove the preserved seed drawing'
    $review.CloseMainWindow()|Out-Null
    if(!$review.WaitForExit(5000) -or $review.ExitCode -ne 0){throw 'Clean Snap review did not close with zero exit within five seconds'}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Snap runtime stderr requires inspection'}
    @{states=$states;monitor_work_area=$work;native_snap='Windows+Left/Right';preservation='seeded drawing/layers/brush/workspace unchanged by transitions; one final Undo returns to clean';painting='OS-injected mouse and pen, visible stroke interiors, exact sampled-pixel one-step Undo/Redo';close='zero exit within five seconds';scope='single available display; no physical pen, mixed-DPI or 120 Hz acceptance'}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $run 'result.json')
    Write-Output "Snap acceptance passed: $run"
}catch{
    if($review -and !$review.HasExited -and $root){try{Capture 'failure'}catch{}}
    throw
}finally{
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
