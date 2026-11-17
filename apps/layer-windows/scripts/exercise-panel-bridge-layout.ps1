param([Parameter(Mandatory)][string]$Executable,[ValidateSet('light','dark')][string]$Theme='light')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Windows.Forms
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path;$directory=Split-Path $Executable
$run=Join-Path $repo ('artifacts/windows/panel-bridge-layout/'+$Theme+'-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$tokens=$null;$parseErrors=$null
$oracle=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'exercise-column-stacks.ps1'),[ref]$tokens,[ref]$parseErrors)
if($parseErrors.Count){throw 'Column bridge oracle parse failed'}
foreach($name in @('Presentation','Column','Screen','At','Tap','Same-Bounds','Check-Open')){
    $function=$oracle.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq$name},$true)
    if(!$function){throw "Missing shared column oracle $name"}
    Invoke-Expression $function.Extent.Text
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyBridgeWindow {
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr window,uint message,IntPtr wparam,IntPtr lparam);
    [StructLayout(LayoutKind.Sequential)] struct Rect {public int left,top,right,bottom;}
    [DllImport("user32.dll",SetLastError=true)] static extern bool GetWindowRect(IntPtr window,out Rect rectangle);
    public static int[] Bounds(IntPtr window){Rect r;if(!GetWindowRect(window,out r))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return new[]{r.left,r.top,r.right-r.left,r.bottom-r.top};}
    public static bool Caption(IntPtr window,int x,int y){return SendMessage(window,0x84,IntPtr.Zero,new IntPtr(((long)(y&65535)<<16)|(uint)(x&65535))).ToInt64()==2;}

}
'@
Add-Type -AssemblyName System.Drawing,WindowsBase
Add-Type -Path (Join-Path $PSScriptRoot 'PanelBridgeCapture.cs') -ReferencedAssemblies @('System.dll',[Drawing.Bitmap].Assembly.Location,[Windows.Automation.AutomationElement].Assembly.Location,[Windows.Automation.AutomationProperty].Assembly.Location,[Windows.Rect].Assembly.Location)
$Device='mouse';$review=$null;$window=[IntPtr]::Zero
$checks=[Collections.Generic.List[object]]::new();$stderr=Join-Path $run 'stderr.log'
function Check($condition,[string]$name,$data){
    $checks.Add(@{name=$name;pass=[bool]$condition;data=$data})
    if(!$condition){throw $name}
}
function Bridge-FrameRange($History,[long]$After,[long]$Through){
    if($null -eq $History.frame -or $null -eq $History.samples -or $null -eq $History.overflow -or $History.overflow -ne 0 -or $Through -gt $History.frame -or $Through -lt $After){throw 'Native bridge frame evidence is missing or overflowed'}
    $selected=@($History.samples|Where-Object {$_.last_frame -gt $After -and $_.first_frame -le $Through})
    $next=$After+1;$first=$true
    foreach($entry in $selected){
        if($null -eq $entry.first_frame -or $null -eq $entry.last_frame -or $entry.first_frame -gt $next -or $entry.last_frame -lt $next -or (!$first -and $entry.first_frame -ne $next)){throw 'Native bridge frame evidence skipped or reordered a frame'}
        $first=$false;$next=[Math]::Min($Through,$entry.last_frame)+1
    }
    if($next -ne $Through+1){throw 'Native bridge frame evidence skipped or reordered a frame'}
    $selected
}
function Bridge-EventRange($History,[long]$After,[long]$Through){
    if($null -eq $History.serial -or $null -eq $History.overflow -or $History.overflow -ne 0 -or $Through -gt $History.serial -or $Through -lt $After){throw 'Native bridge boundary evidence is missing or overflowed'}
    $selected=@($History.samples|Where-Object {$_.serial -gt $After -and $_.serial -le $Through})
    $next=$After+1;$qpc=0L
    foreach($entry in $selected){
        if($null -eq $entry.serial -or $entry.serial -ne $next -or $entry.kind -notin @('ownership','chrome-sent','chrome-ack','chrome-reset') -or $null -eq $entry.accepted -or $entry.qpc -le 0 -or $entry.qpc -lt $qpc){throw 'Native bridge boundary evidence skipped or reordered an event'}
        $next++;$qpc=[long]$entry.qpc
    }
    if($next -ne $Through+1){throw 'Native bridge boundary evidence skipped or reordered an event'}
    $selected
}
function Assert-BridgeEvents($History,[long]$After,[long]$Through,$Continuing,[long]$PreSizingRegions,[long]$SnapshotQpc,[long]$SizingStart=0,[long]$SizingEnd=0){
    $events=@(Bridge-EventRange $History $After $Through)
    foreach($entry in $events){
        Check ($entry.qpc -le $SnapshotQpc) 'Every bridge boundary precedes its source snapshot' $entry
        if($SizingStart -gt 0){
            if($entry.qpc -lt $SizingStart){Check (!$entry.window_resizing -and !$entry.native_resize_pending) 'Every pre-sizing boundary retains GPU ownership' $entry}
            elseif($entry.qpc -lt $SizingEnd){Check $entry.window_resizing 'Every active sizing boundary retains native ownership' $entry}
            else{Check (!$entry.window_resizing) 'Every post-sizing boundary has left native sizing' $entry}
        }
        Assert-BridgeFrameOwners $entry $Continuing $PreSizingRegions
    }
    $events
}
function Assert-BridgeFrameOwners($Entry,$Continuing,[long]$PreSizingRegions){
    $receivers=@($Entry.groups|Where-Object {$_.id -in $Continuing})
    Check ($receivers.Count -eq $Continuing.Count -and @($Continuing|Where-Object {$_ -notin $Entry.projected}).Count -eq 0 -and $Entry.bridges -gt 0) 'Every source interval retains three measured bridge receivers' $Entry
    if($Entry.window_resizing){
        Check ($PreSizingRegions -gt 0 -and $Entry.glass_regions -ge $PreSizingRegions) 'Every active source snapshot retains the pre-sizing glass regions' $Entry
        Check (@($receivers|Where-Object {!$_.native_bridge_background -or $_.gpu_background}).Count -eq 0) 'Every active source snapshot retains three native tint owners' $Entry
    }elseif($Entry.native_resize_pending){
        Check (@($receivers|Where-Object {!$_.native_bridge_background -or $_.gpu_background}).Count -eq 0) 'Post-sizing handback keeps three native owners until acknowledgment' $Entry
    }else{
        Check (@($receivers|Where-Object {!$_.gpu_background -or $_.native_bridge_background}).Count -eq 0) 'Acknowledged native frames retain three GPU owners' $Entry
    }
}
function Save-State([string]$phase){
    $model=Model;$status=Find 'canvas-status'
    @{model=$model;presentation=Presentation;canvas_status=if($status){$status.Current.Name}else{''}}|
        ConvertTo-Json -Depth 90|Set-Content (Join-Path $run ($phase+'.json'))
    Check (!$model.windows_workspace.error-and(!$status-or!$status.Current.Name)) ($phase+' health') @{workspace_error=$model.windows_workspace.error;canvas_status=if($status){$status.Current.Name}else{''}}
}
function Open-Column([string]$phase){
    if(!(Column 12).open){Tap 'column-icon-layers'}
    Check-Open 12
    Check $true ($phase+' native geometry, bridge radii/IDs and matching GPU acknowledgment') $null
    Save-State $phase
}
function Close-Column([string]$phase){
    if((Column 12).open){Tap 'column-icon-layers'}
    Wait-Until {
        if((Column 12).open){return $false}
        foreach($group in (Column 12).groups){foreach($icon in $group.icons){
            if((Control ('column-icon-'+$icon.panel)).Current.ItemStatus-eq'Selected'){return $false}
        }}
        @((Presentation).panel_bridge_frame.bridges|Where-Object {$_[0]-eq12}).Count-eq0
    } 'Closed column retained selection or bridge receivers'
    Check $true ($phase+' closed selection and bridge retirement') $null
    Save-State $phase
}
function Retained-Controls{
    $result=@{}
    foreach($id in @('drawing-canvas','column-icon-navigator','column-icon-properties','column-icon-layers','panel-tab-layers','panel-tab-navigator','navigator-overview')){
        $result[$id]=(Control $id -Arranged).GetRuntimeId()-join':'
    }
    $result
}
function Resize-BridgeWindow([int]$Width,[int]$Height){
    $work=[System.Windows.Forms.Screen]::FromHandle($window).WorkingArea
    $root.GetCurrentPattern([System.Windows.Automation.TransformPattern]::Pattern).Move($work.Left+40,$work.Top+40)
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $window.ToInt64() -Action Resize -Width ([Math]::Min($Width,$work.Width-40)) -Height ([Math]::Min($Height,$work.Height-40))
}
try{
    Enter-CapyEnvironment @('CAPY_LATENCY_TRACE')
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_LATENCY_TRACE='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json))
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle;$owned=@{window=$null}
    Wait-Canvas {$owned.window=Owned-DrawingWindow $review;$owned.window-and(Review-Ready)-and!(Model).windows_workspace.busy} 'Owned panel bridge fixture did not start' 90
    $root=$owned.window.Root;$window=$owned.window.Handle
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyWindowApi]::ShowWindow($window,9)|Out-Null
    Resize-BridgeWindow 1500 1300
    [CapyRowPointer]::SetForegroundWindow($window)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Use-PaintWorkspace
    Check ((Model).windows_workspace.id-eq'builtin:workspace:illustrator') 'Explicit fresh Paint workspace' $null
    for($cycle=1;$cycle-le3;$cycle++){Open-Column ('cycle-'+$cycle+'-open');Close-Column ('cycle-'+$cycle+'-closed')}
    Open-Column 'before-resize';$retained=Retained-Controls
    $continuing=@((Presentation).groups|Where-Object gpu_background|ForEach-Object id)
    Check ($continuing.Count -eq 3) 'Real border resize starts with three presented receivers' $continuing
    $dividerFolder=Join-Path $run 'stack-divider';[IO.Directory]::CreateDirectory($dividerFolder)|Out-Null;$at=At 'divider-13'
    $dividerStart=Presentation;$dividerBefore=$dividerStart.bridge_frames.frame;$dividerEventBefore=$dividerStart.bridge_events.serial
    [CapyRowPointer]::Down('mouse',$at.x,$at.y)
    try{
        foreach($offset in @(30,60,30,0,-30,-60,-30,0)){
            $before=Presentation;$priorRevision=$before.chrome_revision;$priorFrame=$before.bridge_frames.frame
            [CapyRowPointer]::Move($at.x,$at.y+$offset)
            Wait-Until {
                $native=Presentation;$last=@($native.bridge_frames.samples)[-1]
                $native.chrome_revision -gt $priorRevision -and $native.bridge_frames.frame -gt $priorFrame -and
                $last.chrome_revision -eq $native.chrome_revision -and $last.presented_chrome_revision -eq $last.chrome_revision
            } 'Stack-divider placement did not reach an acknowledged native frame'
        }
    }finally{[CapyRowPointer]::Up()}
    $dividerDrain=[Diagnostics.Stopwatch]::GetTimestamp()
    Wait-Until {$native=Presentation;$native.snapshot_qpc -ge $dividerDrain -and $native.bridge_frames.frame -ge $dividerBefore -and $native.bridge_events.serial -ge $dividerEventBefore} 'Stack-divider history did not publish its final drain'
    $dividerState=Presentation;$dividerHistory=$dividerState.bridge_frames
    $dividerSamples=@(Bridge-FrameRange $dividerHistory $dividerBefore $dividerHistory.frame)
    $dividerSamples|ConvertTo-Json -Depth 40|Set-Content (Join-Path $dividerFolder 'presentations.json')
    Check ($dividerSamples.Count -gt 0 -and @($dividerSamples.chrome_revision|Sort-Object -Unique).Count -gt 1) 'Stack-divider frames include changing bridge revisions' $dividerSamples.Count
    foreach($native in $dividerSamples){
        Assert-BridgeFrameOwners $native $continuing 0
        $receivers=@($native.groups|Where-Object {$_.id -in $continuing})
        Check (!$native.window_resizing -and !$native.native_resize_pending -and @($receivers|Where-Object {!$_.gpu_background -or $_.native_bridge_background}).Count -eq 0) 'Stack-divider receivers retain GPU ownership without native backing' $native
    }
    $dividerEvents=@(Assert-BridgeEvents $dividerState.bridge_events $dividerEventBefore $dividerState.bridge_events.serial $continuing 0 $dividerState.snapshot_qpc)
    $dividerEvents|ConvertTo-Json -Depth 40|Set-Content (Join-Path $dividerFolder 'boundaries.json')
    Check ($dividerEvents.Count -gt 0 -and @($dividerEvents|Where-Object {$_.kind -eq 'chrome-sent'}).Count -gt 0 -and @($dividerEvents|Where-Object {$_.kind -eq 'chrome-ack'}).Count -gt 0) 'Stack-divider boundaries include native send and acknowledgment decisions' $dividerEvents.Count
    @{native_frames=($dividerHistory.frame-$dividerBefore);boundary_events=$dividerEvents.Count;outstanding_revision_segments=@($dividerSamples|Where-Object {$_.chrome_revision -ne $_.presented_chrome_revision}).Count;forced_pending_coverage='real WorkspaceView text-layout send/send/stale/current acknowledgment and resize-handback proof';scope='contiguous native Rendering history and every ownership/send/ack boundary; event serials are not frame IDs'}|ConvertTo-Json|Set-Content (Join-Path $dividerFolder 'coverage.json')
    Open-Column 'stack-divider-settled'
    $moveStart=[CapyBridgeWindow]::Bounds($window);$caption=$null
    $captionY=[int]($root.Current.BoundingRectangle.Top+(Model).titlebar_insets[2]*([CapyRowPointer]::GetDpiForWindow($window)/96.)*.5)
    for($x=$moveStart[0]+100;$x-lt$moveStart[0]+$moveStart[2]-200;$x+=8){
        if([CapyBridgeWindow]::Caption($window,$x,$captionY)){$caption=@($x,$captionY);break}
    }
    Check ($null-ne$caption) 'Titlebar move uses measured native caption hit region' $caption
    $moveBefore=Presentation;$moveSamples=[Collections.Generic.List[object]]::new()
    [CapyRowPointer]::Down('mouse',$caption[0],$caption[1])
    try{
        for($step=1;$step-le6;$step++){
            [CapyRowPointer]::Move($caption[0]+12*$step,$caption[1]+6*$step)
            Wait-Until {$box=[CapyBridgeWindow]::Bounds($window);$box[0]-ge$moveStart[0]+8*$step} 'SendInput titlebar drag did not move owned window'
            $native=Presentation;$receivers=@($native.groups|Where-Object {$_.id -in $continuing})
            $moveSamples.Add(@{qpc=[Diagnostics.Stopwatch]::GetTimestamp();step=$step;bounds=[CapyBridgeWindow]::Bounds($window);window_resizing=$native.window_resizing;native_resize_pending=$native.native_resize_pending;chrome_revision=$native.chrome_revision;presented_chrome_revision=$native.presented_chrome_revision;receivers=$receivers})
            Check (!$native.window_resizing -and !$native.native_resize_pending -and $native.chrome_revision -eq $native.presented_chrome_revision -and $receivers.Count -eq $continuing.Count -and @($receivers|Where-Object {!$_.gpu_background -or $_.native_bridge_background}).Count -eq 0) 'Titlebar movement retains presented GPU receivers' $moveSamples[$moveSamples.Count-1]
        }
    }finally{[CapyRowPointer]::Up();$moveSamples|ConvertTo-Json -Depth 30|Set-Content (Join-Path $run 'titlebar-move-samples.json')}
    $moveDrain=[Diagnostics.Stopwatch]::GetTimestamp()
    Wait-Until {$native=Presentation;$native.snapshot_qpc -ge $moveDrain -and $native.bridge_frames.frame -ge $moveBefore.bridge_frames.frame -and $native.bridge_events.serial -ge $moveBefore.bridge_events.serial} 'Titlebar history did not publish its final drain'
    $moveAfter=Presentation
    $moveFrames=@(Bridge-FrameRange $moveAfter.bridge_frames $moveBefore.bridge_frames.frame $moveAfter.bridge_frames.frame)
    foreach($entry in $moveFrames){Assert-BridgeFrameOwners $entry $continuing 0}
    $moveEvents=@(Assert-BridgeEvents $moveAfter.bridge_events $moveBefore.bridge_events.serial $moveAfter.bridge_events.serial $continuing 0 $moveAfter.snapshot_qpc)
    @{frames=$moveFrames;boundaries=$moveEvents;rendering_after=$moveBefore.bridge_frames.frame;rendering_through=$moveAfter.bridge_frames.frame;scope='Contiguous native Rendering intervals and every native boundary event'}|ConvertTo-Json -Depth 40|Set-Content (Join-Path $run 'titlebar-source-history.json')
    $moveEnd=[CapyBridgeWindow]::Bounds($window)
    Check ($moveEnd[0] -gt $moveStart[0]+30 -and $moveEnd[2] -eq $moveStart[2] -and $moveEnd[3] -eq $moveStart[3]) 'Real titlebar drag changes position without resizing' $moveEnd
    Save-State 'titlebar-move-settled';Capture 'titlebar-move' -Composed -WithModel
    Resize-BridgeWindow 1500 1000
    $preSizingRegions=(Model).windows_glass.regions
    $resizeBefore=Presentation;$motionFolder=Join-Path $run 'continuous-resize'
    $observer=[PanelBridgeCapture]::new($window,[uint32]$review.Id,(Workspace-Root),$motionFolder,(State-File))
    try{$observer.Arm();$observer.Drag([CapyWindowExercise])}
    finally{$observer.Dispose()}
    $samples=@($observer.States)
    $samples|ConvertTo-Json -Depth 10|Set-Content (Join-Path $motionFolder 'samples.json')
    @{down_qpc=$observer.DownQpc;up_qpc=$observer.UpQpc;qpc_frequency=[Diagnostics.Stopwatch]::Frequency;initial_width=$observer.InitialWidth}|ConvertTo-Json|Set-Content (Join-Path $motionFolder 'gesture.json')
    $observed=@($samples|Where-Object {$_.end -ge $observer.DownQpc -and $_.begin -le $observer.UpQpc -and ($_.width_begin -ne $observer.InitialWidth -or $_.width_end -ne $observer.InitialWidth)})
    $boundary=@($samples|Where-Object {($_.begin -lt $observer.DownQpc -and $_.end -ge $observer.DownQpc) -or ($_.begin -le $observer.UpQpc -and $_.end -gt $observer.UpQpc) -or ($_.width_begin -eq $observer.InitialWidth -and $_.width_end -ne $observer.InitialWidth)})
    $boundary|ConvertTo-Json -Depth 10|Set-Content (Join-Path $motionFolder 'boundary-intervals.json')
    Wait-Until {$native=Presentation;$native.snapshot_qpc -ge $observer.UpQpc -and !$native.window_resizing -and !$native.native_resize_pending -and $native.bridge_frames.frame -ge $resizeBefore.bridge_frames.frame -and $native.bridge_events.serial -ge $resizeBefore.bridge_events.serial} 'Real border resize did not return surfaces to the GPU'
    $settled=Presentation;$sizingStart=[long]$settled.native_sizing_start_qpc;$sizingEnd=[long]$settled.native_sizing_end_qpc
    Check ($sizingStart -ge $observer.DownQpc -and $sizingEnd -ge $sizingStart -and $sizingEnd -ge $observer.UpQpc) 'Native sizing source markers bracket the actual contact' @{start=$sizingStart;end=$sizingEnd;down=$observer.DownQpc;up=$observer.UpQpc}
    $history=$settled.bridge_frames
    if($null -eq $history.overflow -or $history.overflow -ne 0){throw 'Native bridge frame evidence is missing or overflowed'}
    $resizeFrames=@(Bridge-FrameRange $history $resizeBefore.bridge_frames.frame $history.frame)
    $classified=@($resizeFrames|Where-Object {$_.last_qpc -ge $observer.DownQpc})
    $resizeEvents=@(Assert-BridgeEvents $settled.bridge_events $resizeBefore.bridge_events.serial $settled.bridge_events.serial $continuing $preSizingRegions $settled.snapshot_qpc $sizingStart $sizingEnd)
    $resizeEvents|ConvertTo-Json -Depth 40|Set-Content (Join-Path $motionFolder 'boundaries.json')
    Check ($resizeEvents.Count -gt 0 -and @($resizeEvents|Where-Object window_resizing).Count -gt 0 -and @($resizeEvents|Where-Object {$_.kind -eq 'chrome-ack'}).Count -gt 0) 'Real border resize retains active ownership and acknowledgment boundaries' $resizeEvents.Count
    $classified|ConvertTo-Json -Depth 30|Set-Content (Join-Path $motionFolder 'source-intervals.json')
    $firstActive=@($classified|Where-Object window_resizing|Select-Object -First 1)
    Check ($firstActive.Count -eq 1) 'Real border resize observes its first active source snapshot' $firstActive
    @{first_source_active=$firstActive;sizing_start_qpc=$sizingStart;sizing_end_qpc=$sizingEnd;input='input.csv';frames='frames.csv';origin='origin.json';scope='native Rendering ownership and source revisions, independent of UIA call intervals'}|ConvertTo-Json -Depth 30|Set-Content (Join-Path $motionFolder 'first-motion.json')
    foreach($entry in $classified){
        Check ($entry.first_qpc -gt 0 -and $entry.last_qpc -ge $entry.first_qpc -and $entry.last_qpc -le $settled.snapshot_qpc) 'Every presentation snapshot has an ordered source timestamp' $entry
        if($entry.first_qpc -lt $sizingStart){
            Check (!$entry.window_resizing -and !$entry.native_resize_pending) 'Every pre-sizing source snapshot retains three GPU owners' $entry
        }elseif($entry.first_qpc -lt $sizingEnd){
            Check $entry.window_resizing 'Every active source snapshot retains three native tint owners' $entry
        }else{Check (!$entry.window_resizing) 'Every post-sizing source snapshot has left native sizing' $entry}
        Assert-BridgeFrameOwners $entry $continuing $preSizingRegions
    }
    $native=Presentation
    $layoutFrames=$native.resize_layout_frames;$frameSamples=@($layoutFrames.samples)
    Check ($layoutFrames.count -gt 0 -and $layoutFrames.overflow -eq 0 -and $frameSamples.Count -eq $layoutFrames.count) 'Every native resize Rendering tick has a bounded layout size record' $layoutFrames
    Check (@($frameSamples.width|Sort-Object -Unique).Count -gt 1) 'Real border resize has fully observed active motion' $frameSamples.Count
    $mismatches=@($frameSamples|Where-Object mismatch);$invalid=@($frameSamples|Where-Object {!$_.valid})
    @{theme=$Theme;stage='synchronous resize layout';scope='native CompositionTarget.Rendering during real sizing contact';count=$frameSamples.Count;mismatch_count=$mismatches.Count;invalid_viewports=$invalid.Count;overflow=$layoutFrames.overflow;tolerance=$layoutFrames.tolerance;first_mismatch=@($mismatches|Select-Object -First 1);samples=$frameSamples}|
        ConvertTo-Json -Depth 20|Set-Content (Join-Path $motionFolder 'layout-frames.json')
    Check ($mismatches.Count -eq 0 -and $invalid.Count -eq 0) 'Every Rendering tick applies the current logical viewport' @{mismatches=$mismatches;invalid=$invalid}
    $bounds=$native.resize_panel_bounds;$boundSamples=@($bounds.samples)
    $expectedBounds=($frameSamples|Measure-Object -Property panels -Sum).Sum+($frameSamples|Measure-Object -Property columns -Sum).Sum
    Check ($bounds.overflow -eq 0 -and $bounds.count -eq $boundSamples.Count -and $bounds.count -eq $expectedBounds -and $bounds.count -gt 0) 'Every Rendering tick records every visible docked panel allocation' $bounds
    foreach($frame in $frameSamples){
        $allocated=@($boundSamples|Where-Object {$_.qpc -eq $frame.qpc})
        $groups=@($allocated|Where-Object {$_.kind -eq 'group'});$columns=@($allocated|Where-Object {$_.kind -eq 'column'})
        Check ($groups.Count -eq $frame.panels -and $columns.Count -eq $frame.columns -and @($continuing|Where-Object {$_ -notin $groups.group}).Count -eq 0 -and @($columns|Where-Object {$_.group -eq 12}).Count -eq 1) 'Every Rendering tick retains all three native receivers and their collapsed column' @{frame=$frame;allocated=$allocated}
    }
    $outside=@($boundSamples|Where-Object outside)
    @{theme=$Theme;scope='allocated visible native docked groups and collapsed columns';count=$boundSamples.Count;overflow=$bounds.overflow;outside_count=$outside.Count;first_outside=@($outside|Select-Object -First 1);samples=$boundSamples}|
        ConvertTo-Json -Depth 20|Set-Content (Join-Path $motionFolder 'panel-containment.json')
    Check ($outside.Count -eq 0 -and @($frameSamples|Where-Object {$_.outside -ne 0}).Count -eq 0) 'Every visible docked panel and column remains inside the current viewport on every Rendering tick' $outside
    Write-Output ('Resize layout {0}: mismatch=0/{1}, outside=0/{2}' -f $Theme,$frameSamples.Count,$boundSamples.Count)
    Open-Column 'continuous-resize-settled'

    foreach($size in @(@(1800,1100),@(1600,1000))){
        $phase='resize-'+$size[0]
        Resize-BridgeWindow $size[0] $size[1]
        Wait-Until {[math]::Abs($root.Current.BoundingRectangle.Width-$size[0])-le2} 'Owned native width did not change'
        Open-Column $phase;$current=Retained-Controls
        foreach($id in $retained.Keys){Check ($current[$id]-eq$retained[$id]) ($phase+' retains '+$id) $null}
    }
    Capture 'paint-resized' -Composed -WithModel
    for($cycle=1;$cycle-le2;$cycle++){
        Use-Workspace 'builtin:workspace:painter' 'Sketch'
        Wait-Until {(Model).windows_workspace.id-eq'builtin:workspace:painter'-and!(Model).windows_workspace.busy} 'Sketch workspace did not adopt'
        Save-State ('switch-'+$cycle+'-sketch')
        Use-PaintWorkspace
        Close-Column ('switch-'+$cycle+'-paint-closed')
        Open-Column ('switch-'+$cycle+'-paint-open')
    }
    Capture 'paint-returned' -Composed -WithModel
    $surfaces=(Presentation).surface_frames
    Check ($surfaces.frames -gt 0 -and $surfaces.groups -gt 0 -and $surfaces.tab_frames -gt 0) 'Rendering trace observes native fills and tab labels' $surfaces
    Check ($surfaces.mismatched_frames -eq 0 -and $surfaces.maximum_offset_px -le .01) 'Every Rendering tick keeps panel fill edges with their XAML frame' $surfaces
    Check ($surfaces.escaped_label_frames -eq 0) 'Every Rendering tick keeps tab labels inside their tabs' $surfaces
    $surfaces|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'surface-frames.json')
    $tabbed=@((Model).layout.groups|Where-Object {$_.panels.Count-gt1}|Select-Object id,active,panels)
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $window.ToInt64() -Action Close
    Check ($review.WaitForExit(8000)-and$review.ExitCode-eq0) 'Normal owned process exit' $null
    Check ((Get-Item -LiteralPath $stderr).Length-eq0) 'Empty native stderr' $null
    @{passed=$true;theme=$Theme;checks=$checks.Count;tabbed_fixture=$tabbed;exe_sha256=(Get-FileHash $Executable).Hash;dll_sha256=(Get-FileHash (Join-Path $directory 'layer_windows.dll')).Hash;exit_code=$review.ExitCode;scope='Real SendInput stack-divider and right-border drags; continuing GPU owners during divider motion; native receiver ownership through sizing handback; ordered input/state/raw composed captures retained; unsampled frames and performance unqualified'}|
        ConvertTo-Json -Depth 20|Set-Content (Join-Path $run 'result.json')
    Write-Output "Panel bridge layout PASS $Theme $run"
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace)
    if($review-and!$review.HasExited){try{Capture 'failure' -Composed -WithModel;Save-State 'failure-state'}catch{}}
    throw
}finally{
    $checks|ConvertTo-Json -Depth 20|Set-Content (Join-Path $run 'checks.json')
    [CapyRowPointer]::Dispose()
    if($review-and!$review.HasExited){[CapyWindowApi]::PostMessage($window,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$review.WaitForExit(8000)){Stop-Process -Id $review.Id}}
    Exit-CapyEnvironment
}
