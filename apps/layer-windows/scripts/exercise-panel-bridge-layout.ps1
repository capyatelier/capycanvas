param([Parameter(Mandatory)][string]$Executable,[ValidateSet('light','dark')][string]$Theme='light')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
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
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json))
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle;$owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$owned.window-and(Model).brush_ready-and(Model).windows_workspace.ready-and!(Model).windows_workspace.busy} 'Owned panel bridge fixture did not start' 90
    $root=$owned.window.Root;$window=$owned.window.Handle
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $window.ToInt64() -Action Resize -Width 1500 -Height 1000
    [CapyWindowApi]::ShowWindow($window,9)|Out-Null
    [CapyWindowExercise]::MoveWindow($window,40,40,1500,1000,$true)|Out-Null
    [CapyRowPointer]::SetForegroundWindow($window)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Use-PaintWorkspace
    Check ((Model).windows_workspace.id-eq'builtin:workspace:illustrator') 'Explicit fresh Paint workspace' $null
    for($cycle=1;$cycle-le3;$cycle++){Open-Column ('cycle-'+$cycle+'-open');Close-Column ('cycle-'+$cycle+'-closed')}
    Open-Column 'before-resize';$retained=Retained-Controls
    $continuing=@((Presentation).groups|Where-Object gpu_background|ForEach-Object id)
    Check ($continuing.Count -eq 3) 'Real border resize starts with three presented receivers' $continuing
    $dividerFolder=Join-Path $run 'stack-divider';$at=At 'divider-13'
    $observer=[PanelBridgeCapture]::new($window,[uint32]$review.Id,(Workspace-Root),$dividerFolder,(State-File))
    try{
        $observer.Arm();[CapyRowPointer]::Down('mouse',$at.x,$at.y)
        $dividerStart=[Diagnostics.Stopwatch]::GetTimestamp();$clock=[Diagnostics.Stopwatch]::StartNew()
        try{
            while($clock.ElapsedMilliseconds -lt 3000){
                [CapyRowPointer]::Move($at.x,$at.y+[int](60*[Math]::Sin([Math]::PI*$clock.Elapsed.TotalSeconds)))
                Start-Sleep -Milliseconds 8
            }
            [CapyRowPointer]::Move($at.x,$at.y)
        }finally{$dividerEnd=[Diagnostics.Stopwatch]::GetTimestamp();[CapyRowPointer]::Up()}
    }finally{$observer.Dispose()}
    $dividerSamples=@(foreach($sample in $observer.States){
        $native=Get-Content -LiteralPath $sample.path -Raw|ConvertFrom-Json
        if($native.snapshot_qpc -ge $dividerStart -and $native.snapshot_qpc -le $dividerEnd){$native}
    })
    $dividerSamples|ConvertTo-Json -Depth 40|Set-Content (Join-Path $dividerFolder 'presentations.json')
    Check ($dividerSamples.Count -gt 0 -and @($dividerSamples.chrome_revision|Sort-Object -Unique).Count -gt 1 -and @($dividerSamples|Where-Object {$_.chrome_revision -ne $_.presented_chrome_revision}).Count -gt 0) 'Stack-divider samples include changing unacknowledged bridge revisions' $dividerSamples.Count
    foreach($native in $dividerSamples){
        $receivers=@($native.groups|Where-Object {$_.id -in $continuing})
        Check ($receivers.Count -eq $continuing.Count -and @($receivers|Where-Object {!$_.gpu_background -or $_.native_bridge_background}).Count -eq 0) 'Stack-divider receivers retain GPU ownership without native backing' @{qpc=$native.snapshot_qpc;revision=$native.chrome_revision;presented=$native.presented_chrome_revision;receivers=$receivers}
    }
    Open-Column 'stack-divider-settled'
    $moveStart=[CapyBridgeWindow]::Bounds($window);$caption=$null
    $captionY=[int]($root.Current.BoundingRectangle.Top+(Model).titlebar_insets[2]*([CapyRowPointer]::GetDpiForWindow($window)/96.)*.5)
    for($x=$moveStart[0]+100;$x-lt$moveStart[0]+$moveStart[2]-200;$x+=8){
        if([CapyBridgeWindow]::Caption($window,$x,$captionY)){$caption=@($x,$captionY);break}
    }
    Check ($null-ne$caption) 'Titlebar move uses measured native caption hit region' $caption
    $moveSamples=[Collections.Generic.List[object]]::new()
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
    $moveEnd=[CapyBridgeWindow]::Bounds($window)
    Check ($moveEnd[0] -gt $moveStart[0]+30 -and $moveEnd[2] -eq $moveStart[2] -and $moveEnd[3] -eq $moveStart[3]) 'Real titlebar drag changes position without resizing' $moveEnd
    Save-State 'titlebar-move-settled';Capture 'titlebar-move' -Composed -WithModel
    [CapyWindowExercise]::MoveWindow($window,40,40,1500,1000,$true)|Out-Null
    $preSizingRegions=(Model).windows_glass.regions
    $motionFolder=Join-Path $run 'continuous-resize'
    $observer=[PanelBridgeCapture]::new($window,[uint32]$review.Id,(Workspace-Root),$motionFolder,(State-File))
    try{$observer.Arm();$observer.Drag([CapyWindowExercise])}
    finally{$observer.Dispose()}
    $samples=@($observer.States)
    $samples|ConvertTo-Json -Depth 10|Set-Content (Join-Path $motionFolder 'samples.json')
    @{down_qpc=$observer.DownQpc;up_qpc=$observer.UpQpc;qpc_frequency=[Diagnostics.Stopwatch]::Frequency;initial_width=$observer.InitialWidth}|ConvertTo-Json|Set-Content (Join-Path $motionFolder 'gesture.json')
    $observed=@($samples|Where-Object {$_.end -ge $observer.DownQpc -and $_.begin -le $observer.UpQpc -and ($_.width_begin -ne $observer.InitialWidth -or $_.width_end -ne $observer.InitialWidth)})
    $boundary=@($samples|Where-Object {($_.begin -lt $observer.DownQpc -and $_.end -ge $observer.DownQpc) -or ($_.begin -le $observer.UpQpc -and $_.end -gt $observer.UpQpc) -or ($_.width_begin -eq $observer.InitialWidth -and $_.width_end -ne $observer.InitialWidth)})
    $boundary|ConvertTo-Json -Depth 10|Set-Content (Join-Path $motionFolder 'boundary-intervals.json')
    $motion=@($samples|Where-Object {$_.begin -ge $observer.DownQpc -and $_.end -le $observer.UpQpc -and $_.width_begin -ne $observer.InitialWidth})
    Check ($motion.Count -gt 0) 'Real border resize has fully observed active motion' $motion.Count
    Wait-Until {$native=Presentation;!$native.window_resizing -and !$native.native_resize_pending} 'Real border resize did not return surfaces to the GPU'
    $settled=Presentation;$sizingStart=[long]$settled.native_sizing_start_qpc;$sizingEnd=[long]$settled.native_sizing_end_qpc
    Check ($sizingStart -ge $observer.DownQpc -and $sizingEnd -ge $sizingStart -and $sizingEnd -ge $observer.UpQpc) 'Native sizing source markers bracket the actual contact' @{start=$sizingStart;end=$sizingEnd;down=$observer.DownQpc;up=$observer.UpQpc}
    $classified=@(foreach($sample in $samples){
        $native=Get-Content -LiteralPath $sample.path -Raw|ConvertFrom-Json
        $model=(Get-Content -LiteralPath $sample.model_path -Raw|ConvertFrom-Json).model
        $source=[long]$native.snapshot_qpc
        $interval=if($source -lt $sizingStart){'pre-sizing'}elseif($source -lt $sizingEnd){'active-sizing'}else{'post-sizing'}
        $receivers=@($native.groups|Where-Object {$_.id -in $continuing})
        $projected=@($native.panel_bridge_frame.bridges|ForEach-Object {$_[2].receivers}|ForEach-Object group|Sort-Object -Unique)
        @{sample=$sample;source_qpc=$source;glass_regions=$model.windows_glass.regions;interval=$interval;window_resizing=$native.window_resizing;native_resize_pending=$native.native_resize_pending;receivers=$receivers;projected=$projected;bridges=@($native.panel_bridge_frame.bridges).Count}
    })
    $classified|ConvertTo-Json -Depth 30|Set-Content (Join-Path $motionFolder 'source-intervals.json')
    $firstActive=@($classified|Where-Object {$_.source_qpc -ge $sizingStart -and $_.source_qpc -lt $sizingEnd}|Select-Object -First 1)
    $firstObserved=$classified|Where-Object {$_.sample.path -eq $observed[0].path}|Select-Object -First 1
    @{first_client_motion_read=$firstObserved;first_source_active=$firstActive;first_fully_active_client_read=$motion[0];sizing_start_qpc=$sizingStart;sizing_end_qpc=$sizingEnd;boundary_intervals=$boundary;input='input.csv';frames='frames.csv';origin='origin.json'}|ConvertTo-Json -Depth 30|Set-Content (Join-Path $motionFolder 'first-motion.json')
    Check ($firstActive.Count -eq 1) 'Real border resize observes its first active source snapshot' $firstActive
    foreach($entry in $classified){
        Check ($entry.source_qpc -gt 0 -and $entry.source_qpc -le $entry.sample.end) 'Every presentation snapshot has an ordered source timestamp' $entry
        Check ($entry.receivers.Count -eq 3 -and @($continuing|Where-Object {$_ -notin $entry.projected}).Count -eq 0 -and $entry.bridges -gt 0) 'Every source interval retains three measured bridge receivers' $entry
        if($entry.interval -eq 'active-sizing'){
            Check ($preSizingRegions -gt 0 -and $entry.glass_regions -ge $preSizingRegions) 'Every active source snapshot retains the pre-sizing glass regions' @{before=$preSizingRegions;entry=$entry}
            Check ($entry.window_resizing -and @($entry.receivers|Where-Object {!$_.native_bridge_background -or $_.gpu_background}).Count -eq 0) 'Every active source snapshot retains three native tint owners' $entry
        }elseif($entry.interval -eq 'pre-sizing'){
            Check (!$entry.window_resizing -and !$entry.native_resize_pending -and @($entry.receivers|Where-Object {!$_.gpu_background -or $_.native_bridge_background}).Count -eq 0) 'Every pre-sizing source snapshot retains three GPU owners' $entry
        }else{
            Check (!$entry.window_resizing) 'Every post-sizing source snapshot has left native sizing' $entry
            if($entry.native_resize_pending){
                Check (@($entry.receivers|Where-Object {!$_.native_bridge_background -or $_.gpu_background}).Count -eq 0) 'Post-sizing handback keeps three native owners until acknowledgment' $entry
            }else{
                Check (@($entry.receivers|Where-Object {!$_.gpu_background -or $_.native_bridge_background}).Count -eq 0) 'Acknowledged post-sizing snapshots restore three GPU owners' $entry
            }
        }
    }
    $native=Presentation
    $layoutFrames=$native.resize_layout_frames;$frameSamples=@($layoutFrames.samples)
    Check ($layoutFrames.count -gt 0 -and $layoutFrames.overflow -eq 0 -and $frameSamples.Count -eq $layoutFrames.count) 'Every native resize Rendering tick has a bounded layout size record' $layoutFrames
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
        [CapyWindowExercise]::MoveWindow($window,40,40,$size[0],$size[1],$true)|Out-Null
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
