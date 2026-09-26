param([Parameter(Mandatory)][string]$Executable,[ValidateSet('touch','pen','mouse')][string]$Device='touch')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/tab-pickup/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Read-Snapshot([string]$Path){
    $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader=[IO.StreamReader]::new($stream)
    try{$reader.ReadToEnd()}finally{$reader.Dispose()}
}
function Model {
    try {
        if(!$script:statePath){
            # Per-window snapshots use atomic replacement. The compatibility
            # ui-state.json path is a direct write and can be read mid-frame.
            foreach($candidate in [IO.Directory]::EnumerateFiles($directory,("ui-state-"+$review.Id+"-*.json"))){
                if([IO.File]::GetLastWriteTimeUtc($candidate) -lt $review.StartTime.ToUniversalTime()){continue}
                $initial=Read-Snapshot $candidate|ConvertFrom-Json
                if($initial.process_id -eq $review.Id -and $initial.model.windows_isolated_settings){
                    $script:statePath=$candidate;$script:windowId=$initial.window_id;break
                }
            }
        }
        if(!$script:statePath){return}
        $value=Read-Snapshot $script:statePath|ConvertFrom-Json
        if($value.process_id -eq $review.Id -and $value.window_id -eq $script:windowId -and $value.model.windows_isolated_settings){$value.model}
    }catch{}
}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {
        [CapyRowPointer]::Verify()
        if(& $Predicate){return}
        $review.Refresh();if($review.HasExited){throw 'Owned tab review exited unexpectedly'}
        Start-Sleep -Milliseconds 50
    }while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Within=$root,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $candidates=$Within.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)
    $found=$null
    foreach($candidate in $candidates){
        if(!$candidate.Current.IsOffscreen){$found=$candidate;break}
    }
    if(!$found -and $candidates.Count){$found=$candidates[0]}
    if(!$found -and $Within -eq $root){
        # A cascaded WinUI menu can live in a separate UIA fragment.
        # Search only this fixture's owned process, including its popup HWNDs.
        $owned=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
        $found=[System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$owned)
    }
    $found
}
function Control([string]$Value,[switch]$Name,$Within=$root,$Type){
    $hit=@{item=$null}
    Wait-Until {$hit.item=Find $Value -Name:$Name -Within $Within -Type $Type;$null -ne $hit.item} "Missing $Value"
    $hit.item
}
function Invoke([string]$Value,[switch]$Name,$Within=$root){
    (Control $Value -Name:$Name -Within $Within).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Point([string]$Id,[switch]$Name) {
    $stable=@{bounds=$null;count=0}
    Wait-Until {
        $item=Find $Id -Name:$Name
        if(!$item -or $item.Current.IsOffscreen){return $false}
        $bounds=$item.Current.BoundingRectangle
        if($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){return $false}
        if($bounds -eq $stable.bounds){$stable.count++}else{$stable.bounds=$bounds;$stable.count=0}
        $stable.count -ge 2
    } "Source $Id did not arrange visibly"
    # A row-root source uses its transparent left padding, outside child buttons.
    $x=if($Id -like 'layer-row-*'){$stable.bounds.X+2}else{$stable.bounds.X+$stable.bounds.Width*.5}
    @{x=[int]$x;y=[int]($stable.bounds.Y+$stable.bounds.Height*.5)}
}

function Gesture {try{(Find 'Drawing workspace' -Name).Current.HelpText|ConvertFrom-Json}catch{}}
function Presentation {try{(Find 'Drawing workspace' -Name).Current.ItemStatus|ConvertFrom-Json}catch{}}
function Layout {(Model).layout|ConvertTo-Json -Depth 80 -Compress}
function Panel-Group([string]$Panel) {(Model).layout.groups|Where-Object {$_.panels -contains $Panel}}
function Workspace-History([string]$Id) {
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    Invoke $Id
}
function Tap([string]$Id) {
    $at=Point $Id
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 35
    [CapyRowPointer]::Up()
}
function Cancel-Contact {
    if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}
    else{[CapyRowPointer]::Cancel()}
}
function Tear-Off([string]$Panel,[switch]$Gradual,[switch]$Cancel) {
    $script:case="$Panel-$(if($Gradual){'gradual'}else{'direct'})-$(if($Cancel){'cancel'}else{'commit'})"
    $before=Layout
    $at=Point "panel-tab-$Panel";$to=Point 'Drawing canvas' -Name
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 35
    if($Gradual){
        for($i=1;$i -le 12;$i++){
            [CapyRowPointer]::Move([int]($at.x+($to.x-$at.x)*$i/12),[int]($at.y+($to.y-$at.y)*$i/12))
            Start-Sleep -Milliseconds 18
        }
    }else{[CapyRowPointer]::Move($to.x,$to.y)}
    Wait-Until {(Gesture).phase -eq 'dragging' -and (Gesture).captured} "$case did not retain the contact over canvas"
    $gesture=Gesture
    if($gesture.device -ne $Device -or $gesture.requires_hold -or $gesture.menu_open){throw "$case used incorrect pickup arbitration"}
    # Continue the same contact after its original tab has been reparented.
    Start-Sleep -Milliseconds 200
    [CapyRowPointer]::Move($to.x+30,$to.y+15)
    Wait-Until {(Gesture).phase -eq 'dragging' -and (Gesture).captured} "$case lost capture after reparenting"
    if($Cancel){
        Cancel-Contact
        Wait-Until {(Gesture).phase -eq 'idle' -and (Layout) -eq $before} "$case did not restore the workspace"
    }else{
        [CapyRowPointer]::Up()
        Wait-Until {(Gesture).phase -eq 'idle' -and (Panel-Group $Panel).floating} "$case did not finish the tear-off"
        $after=Layout
        Workspace-History 'undo_workspace';Wait-Until {(Layout) -eq $before} "$case did not undo in one step"
        Workspace-History 'redo_workspace';Wait-Until {(Layout) -eq $after} "$case did not redo in one step"
        Workspace-History 'undo_workspace';Wait-Until {(Layout) -eq $before} "$case did not restore the fixture"
    }
    if((Gesture).menu_open -or (Model).state.document_file.modified){throw "$case changed the drawing or retained a menu"}
    Write-Output "$case passed"
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'stdout.log') -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;device=$Device}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/tab-pickup-review.json')
    Write-Output "Owned tab pickup review $($review.Id) ($Device)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Tab review did not start' 45
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $script:case='short-tab-selection'
    Tap 'panel-tab-sizes'
    Wait-Until {(Panel-Group 'sizes').active -eq 'sizes'} 'Short tab click did not select Sizes'
    Tap 'panel-tab-tool_settings'
    Wait-Until {(Panel-Group 'tool_settings').active -eq 'tool_settings'} 'Short tab click did not restore Properties'
    Tear-Off 'layers'
    Tear-Off 'layers' -Cancel
    Tear-Off 'tool_settings' -Gradual
    Tear-Off 'tool_settings' -Gradual -Cancel
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{device=$Device;short_click='passed';direct_tearoff='passed';gradual_tearoff='passed';retained_contact='passed';capture_cancel='passed';workspace_undo_redo='passed';zero_exit='passed';scope='OS-injected input; physical digitizers and latency remain separate'}|ConvertTo-Json
}catch{
    $failure=$_
    @{case=$script:case;error=$failure.ToString();gesture=(Gesture);presentation=(Presentation)}|ConvertTo-Json -Depth 60|Set-Content (Join-Path $run 'failure.json')
    if($review -and !$review.HasExited){
        & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure-capture.json')
    }
    throw $failure
}finally{
    [CapyRowPointer]::Dispose()
    foreach($name in $names){
        if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
        else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
    }
}
