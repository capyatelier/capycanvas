param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
$null=[CapyCanvasTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/transparency/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
$ControlType=[System.Windows.Automation.ControlType]
Add-Type -AssemblyName System.Drawing
Add-Type -Name GlassWindow -Namespace Capy -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,int[] point);'
function Model {
    try {
        if(!$script:statePath){
            foreach($path in [IO.Directory]::EnumerateFiles($directory,('ui-state-'+$review.Id+'-*.json'))){
                if([IO.File]::GetLastWriteTimeUtc($path) -lt $review.StartTime.ToUniversalTime()){continue}
                $value=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
                if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){$script:statePath=$path;break}
            }
        }
        if($script:statePath){$value=Get-Content -LiteralPath $script:statePath -Raw|ConvertFrom-Json;if($value.process_id -eq $review.Id){return $value.model}}
    }catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{try{if(& $Condition){return}}catch [System.Windows.Automation.ElementNotAvailableException]{};$review.Refresh();if($review.HasExited){throw 'Owned selection review exited unexpectedly'};Start-Sleep -Milliseconds 40}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Id,[switch]$Name,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Id)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    foreach($entry in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){if(!$entry.Current.IsOffscreen){return $entry}}
}
function Control([string]$Id,[switch]$Name,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Id -Name:$Name -Type $Type;$null -ne $hit.item} "Missing native control: $Id";$hit.item
}
function Invoke([string]$Id){(Control $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Center($element){$b=$element.Current.BoundingRectangle;@{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
function Tap($at,[string]$Device='mouse'){[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up()}
function Capture([string]$Name){
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
}
function Switch-Workspace([string]$Name,[string]$Id){
    $found=@{switch=$null;menu=$null}
    Wait-Until {
        $found.switch=Find ('workspace-switch-'+$Name.ToLowerInvariant());$found.menu=Find 'header-workspace-menu'
        $found.switch -or $found.menu
    } "No workspace switcher for $Name" 15
    $switch=$found.switch
    if(!$switch){Invoke 'header-workspace-menu';$switch=Control $Name -Name -Type $ControlType::MenuItem}
    Wait-Until {try{$switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle();$true}catch{$false}} "$Name switch stayed unavailable" 20
    Wait-Until {(Model).windows_workspace.id -eq $Id -and !(Model).windows_workspace.busy} "$Name did not open" 20
}
function Preference([int]$Index){
    Invoke 'settings-button'
    $dialog=Control 'Preferences' -Name -Type $ControlType::Window
    $circle=Control ('preference-transparency-'+$Index)
    if($circle.Current.Name -ne @('Off','Low','Medium','High')[$Index]){throw "Transparency circle $Index is misnamed"}
    Invoke ('preference-transparency-'+$Index)
    Wait-Until {(Model).state.palette.glass.transparency -eq @('off','low','medium','high')[$Index]} "Transparency $Index did not apply"
    Wait-Until {$circle.Current.ItemStatus -eq 'Selected'} "Transparency $Index circle was not checked"
    $close=$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Close'),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$ControlType::Button)))
    $close.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Find 'Preferences' -Name -Type $ControlType::Window)} 'Preferences did not close'
}
function Sample([string]$Name,$Points){
    Start-Sleep -Milliseconds 900;Capture $Name
    $origin=[int[]]@(0,0);$null=[Capy.GlassWindow]::ClientToScreen($review.MainWindowHandle,$origin)
    $image=[System.Drawing.Bitmap]::new((Join-Path $run ($Name+'.png')))
    try{@($Points|ForEach-Object {$image.GetPixel($_.x-$origin[0],$_.y-$origin[1]).G})}finally{$image.Dispose()}
}
function Zoom([int]$Steps){
    (Control 'Drawing canvas' -Name).SetFocus()
    for($i=0;$i -lt $Steps;$i++){[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0xBB);Start-Sleep -Milliseconds 120}
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Transparency review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Transparency review did not start' 90
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Start-Sleep -Milliseconds 600
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Switch-Workspace 'Paint' 'builtin:workspace:illustrator'
    if((Model).state.palette.glass.transparency -ne 'low'){throw 'Low is not the default transparency'}
    Wait-Until {(Model).windows_glass.regions -ge 4} 'Paint did not publish its glass surfaces'
    $group=@((Model).layout.groups|Where-Object {$_.panels -contains 'layers'})[0]
    $scale=[Capy.GlassWindow]::GetDpiForWindow($review.MainWindowHandle)/96.;$client=[int[]]@(0,0);$null=[Capy.GlassWindow]::ClientToScreen($review.MainWindowHandle,$client)
    $bounds=$group.bounds
    $point=@{x=[int]($client[0]+($bounds.x+$bounds.width-24)*$scale);y=[int]($client[1]+($bounds.y+$bounds.height-72)*$scale)}
    $panel=[Convert]::ToInt32((Model).state.palette.panel.Substring(3,2),16)
    Invoke 'canvas-fit'
    $surround=@{};$overPaper=@{}
    foreach($level in 0,3,2,1){
        Preference $level
        $surround[$level]=(Sample ('fit-'+$level) @($point))[0]
        if([Math]::Abs($surround[$level]-$panel) -gt 1){throw "Level $level panel over the surround is $($surround[$level]), expected $panel"}
    }
    Zoom 12
    Wait-Until {$z=(Model).state.camera.zoom;$z -gt 2} 'The canvas did not zoom under the panels'
    foreach($level in 0,1,2,3){
        Preference $level
        $overPaper[$level]=(Sample ('paper-'+$level) @($point))[0]
    }
    Write-Output ("Layers panel green over the surround: "+(($surround.GetEnumerator()|Sort-Object Key|ForEach-Object {$_.Value}) -join ',')+"; over paper: "+(0..3|ForEach-Object {$overPaper[$_]}) -join ',')
    if($overPaper[0] -ne $panel){throw "Off panel over paper is $($overPaper[0]), expected $panel"}
    if(!($overPaper[1] -gt $overPaper[0] -and $overPaper[2] -gt $overPaper[1] -and $overPaper[3] -gt $overPaper[2])){throw 'Higher transparency did not show more paper'}
    Switch-Workspace 'Sketch' 'builtin:workspace:painter'
    $color=$null;foreach($zone in @((Model).header.model.zones)){foreach($entry in @($zone)){if($entry.item.control.kind -eq 'color'){$color=$entry.id}}}
    $settled=@{count=-1;since=[Diagnostics.Stopwatch]::StartNew()}
    Wait-Until {$count=(Model).windows_glass.regions;if($count -ne $settled.count){$settled.count=$count;$settled.since.Restart()};$settled.since.ElapsedMilliseconds -gt 1000} 'Sketch glass did not settle' 15
    $closed=$settled.count
    Tap (Center (Control ('header-item-'+$color)))
    Wait-Until {(Find 'tool-drawer') -and (Model).windows_glass.regions -ge $closed+2} 'The Sketch drawer and its connector did not publish glass'
    Capture 'sketch-drawer'
    [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if(!$review.WaitForExit(8000)){throw 'Transparency review did not close'}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Transparency review wrote to stderr'}
    [pscustomobject]@{default_low='passed';circles='passed';invisible_over_surround='passed';paper_by_level='passed';drawer_connector='passed';
        scope='Synthetic UIA and captured pixels; physical display and performance acceptance are separate'}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
    try{Capture 'failure'}catch{}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyCanvasTouch]::Dispose();[CapyRowPointer]::Dispose()
    if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force -ErrorAction SilentlyContinue}
    foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}
