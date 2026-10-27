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
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
public static class CapyBridgeResize {
    [DllImport("user32.dll",SetLastError=true)] static extern bool MoveWindow(IntPtr window,int x,int y,int width,int height,bool repaint);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr window,uint message,IntPtr wparam,IntPtr lparam);
    public static void Begin(IntPtr window){SendMessage(window,0x0231,IntPtr.Zero,IntPtr.Zero);}
    public static void End(IntPtr window){SendMessage(window,0x0232,IntPtr.Zero,IntPtr.Zero);}
    public static Task Run(IntPtr window){return Task.Run(()=>{
        SetThreadDpiAwarenessContext(new IntPtr(-4));var timer=Stopwatch.StartNew();
        while(timer.ElapsedMilliseconds<3000){
            double phase=(timer.ElapsedMilliseconds%1500)/750.0;
            int width=1500+(int)(300*(phase<=1?phase:2-phase));
            if(!MoveWindow(window,40,40,width,1000,true))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            Thread.Sleep(16);
        }
    });}
}
'@
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
    Check ($continuing.Count -gt 0) 'Continuous resize starts with presented receivers' $continuing
    [CapyBridgeResize]::Begin($window)
    Wait-Until {
        $native=Presentation
        $native.window_resizing -and $native.gpu_panels_suppressed -and $native.panel_revision -eq $native.presented_panel_revision -and
            @($native.groups|Where-Object {$_.id -in $continuing -and (!$_.native_bridge_background -or $_.gpu_background)}).Count -eq 0
    } 'Resize did not adopt native surfaces before motion'
    Save-State 'continuous-resize-enter'
    $samples=[Collections.Generic.List[object]]::new();$motion=[CapyBridgeResize]::Run($window)
    try{
        while(!$motion.IsCompleted){
            $native=Presentation
            if($motion.IsCompleted){break}
            $receivers=@($native.groups|Where-Object {$_.id -in $continuing})
            $projected=@($native.panel_bridge_frame.bridges|ForEach-Object {$_[2].receivers}|ForEach-Object group|Sort-Object -Unique)
            $samples.Add(@{qpc=[Diagnostics.Stopwatch]::GetTimestamp();window_resizing=$native.window_resizing;gpu_panels_suppressed=$native.gpu_panels_suppressed;panel_revision=$native.panel_revision;presented_panel_revision=$native.presented_panel_revision;receivers=$receivers;frame=$native.panel_bridge_frame})
            Check ($native.window_resizing -and $native.gpu_panels_suppressed -and @($receivers|Where-Object {!$_.native_bridge_background -or $_.gpu_background}).Count -eq 0) 'Continuous resize keeps native ownership' $receivers
            Check (@($continuing|Where-Object {$_ -notin $projected}).Count -eq 0) 'Continuous resize retains measured receivers' $projected
            Check (@($native.panel_bridge_frame.bridges).Count -gt 0) 'Continuous window resize retains panel scene' $native.panel_bridge_frame
        }
    }finally{$motion.GetAwaiter().GetResult();[CapyBridgeResize]::End($window);$samples|ConvertTo-Json -Depth 30|Set-Content (Join-Path $run 'continuous-resize.json')}
    Check ($samples.Count -gt 0) 'Continuous resize sampled actual motion' $samples.Count
    Wait-Until {$native=Presentation;!$native.window_resizing -and !$native.native_resize_pending} 'Resize did not return surfaces to the GPU'
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
        Tap 'workspace-switch-builtin:workspace:painter'
        Wait-Until {(Model).windows_workspace.id-eq'builtin:workspace:painter'-and!(Model).windows_workspace.busy} 'Sketch workspace did not adopt'
        Save-State ('switch-'+$cycle+'-sketch')
        Use-PaintWorkspace
        Close-Column ('switch-'+$cycle+'-paint-closed')
        Open-Column ('switch-'+$cycle+'-paint-open')
    }
    Capture 'paint-returned' -Composed -WithModel
    $tabbed=@((Model).layout.groups|Where-Object {$_.panels.Count-gt1}|Select-Object id,active,panels)
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $window.ToInt64() -Action Close
    Check ($review.WaitForExit(8000)-and$review.ExitCode-eq0) 'Normal owned process exit' $null
    Check ((Get-Item -LiteralPath $stderr).Length-eq0) 'Empty native stderr' $null
    @{passed=$true;theme=$Theme;checks=$checks.Count;tabbed_fixture=$tabbed;exe_sha256=(Get-FileHash $Executable).Hash;dll_sha256=(Get-FileHash (Join-Path $directory 'layer_windows.dll')).Hash;exit_code=$review.ExitCode;scope='Settled geometry plus sampled continuous measured receivers and nonempty scene; composed captures retained; unsampled intermediate frames and performance unqualified'}|
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
