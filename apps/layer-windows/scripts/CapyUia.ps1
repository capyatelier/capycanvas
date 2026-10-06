Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
if(!('CapyWindowApi' -as [type])){Add-Type -TypeDefinition @"
using System;using System.Runtime.InteropServices;using System.Text;
public static class CapyWindowApi {
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint owner);
 [DllImport("user32.dll")]public static extern bool PostMessage(IntPtr h,uint m,UIntPtr w,IntPtr l);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int n);
 [DllImport("user32.dll")]public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,UIntPtr w,IntPtr l,uint f,uint t,out UIntPtr r);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,UIntPtr w,StringBuilder l,uint f,uint t,out UIntPtr r);
 public static void Path(IntPtr h,string s){UIntPtr r;SendMessageTimeout(h,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out r);SendMessageTimeout(h,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out r);foreach(char c in s)SendMessageTimeout(h,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out r);var actual=new StringBuilder(32768);SendMessageTimeout(h,13,new UIntPtr((uint)actual.Capacity),actual,2,2000,out r);if(actual.ToString()!=s)throw new Exception("Picker path mismatch");}
}
"@}
$CapyScripts=$PSScriptRoot
$CapyWaitSeconds=8
$CapyEach=$null
$CapyFind='first'
$CapyPopups=$false
$CapyTraceDirectory=$null
$CapyStateFile=$null
$CapyCacheModel=$false
$CapyCaptureDelay=0
$CapyTrace=$null
$CapyEnvironment=$null

function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=$script:CapyWaitSeconds,[switch]$Closing){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{
        if($script:CapyEach){& $script:CapyEach}
        try{if(& $Condition){return}}catch [System.Windows.Automation.ElementNotAvailableException]{}
        if($review){$review.Refresh();if($review.HasExited){if($Closing){return};throw "Owned review exited with code $($review.ExitCode): $Message"}}
        Start-Sleep -Milliseconds 50
    }while($watch.Elapsed.TotalSeconds -lt $Seconds*$(if($env:CAPY_WAIT_SCALE){[double]$env:CAPY_WAIT_SCALE}else{1}))
    throw $Message
}
function Wait-StablePixels([scriptblock]$Sample){
    $watch=[Diagnostics.Stopwatch]::StartNew();$last=& $Sample;$stable=0
    do{
        Start-Sleep -Milliseconds 100;$next=& $Sample
        if($next -eq $last){$stable++}else{$stable=0};$last=$next
        if($stable -ge 3){return $last}
    }while($watch.Elapsed.TotalSeconds -lt 5)
    throw 'Canvas pixels did not settle for comparison'
}
function Find([string]$Value,[switch]$Name,$Within=$root,$Type,[switch]$Visible){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    if(!$Within){$Within=$root}
    $found=$null
    if(!$Visible -and 'first' -eq $script:CapyFind){$found=$Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)}
    else{
        $items=$Within.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)
        foreach($item in $items){if(!$item.Current.IsOffscreen){return $item}}
        if(!$Visible -and 'prefer-visible' -eq $script:CapyFind -and $items.Count){$found=$items[0]}
    }
    if(!$found -and $script:CapyPopups -and $Within -eq $root){
        $owned=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
        foreach($item in [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$owned)){
            if(!$item.Current.IsOffscreen){return $item}
        }
    }
    $found
}
function Control([string]$Value,[switch]$Name,$Within=$root,$Type,[int]$Seconds=$script:CapyWaitSeconds,[switch]$Arranged){
    $hit=@{item=$null;bounds=$null;stable=0}
    Wait-Until {
        $hit.item=Find $Value -Name:$Name -Within $Within -Type $Type -Visible:$Arranged
        if(!$hit.item){return $false}
        if(!$Arranged){return $true}
        $bounds=$hit.item.Current.BoundingRectangle
        if($hit.item.Current.IsOffscreen -or $bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){return $false}
        if($bounds -eq $hit.bounds){$hit.stable++}else{$hit.bounds=$bounds;$hit.stable=0}
        $hit.stable -ge 2
    } "Missing control: $Value" $Seconds
    $hit.item
}
function Scroll-Position($Control,[ValidateRange(0,100)][double]$Percent){
    $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$scroller=$walker.GetParent($Control);$scroll=$null
    while($scroller -and !$scroller.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll)){$scroller=$walker.GetParent($scroller)}
    if(!$scroll){throw 'The native options have no scroll provider'}
    if(!$scroll.Current.VerticallyScrollable){return}
    $scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,$Percent)
    Wait-Until {[Math]::Abs($scroll.Current.VerticalScrollPercent-$Percent) -lt .01} 'Native options did not acknowledge their scroll position'
}
function Invoke([string]$Value,[switch]$Name,$Within=$root){
    $item=Control $Value -Name:$Name -Within $Within;$pattern=$null
    if($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
    else{$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
}
function Read-Snapshot([string]$Path){
    for($attempt=0;$attempt -lt 20;$attempt++){
        try{
            $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
            $reader=[IO.StreamReader]::new($stream)
            try{return ($reader.ReadToEnd()|ConvertFrom-Json)}finally{$reader.Dispose()}
        }catch{
            if($attempt -eq 19 -or $_.Exception.GetBaseException() -isnot [IO.IOException]){throw}
            Start-Sleep -Milliseconds 50
        }
    }
}
function Settings-File{
    $path=Join-Path $env:CAPY_STORAGE_DIR 'config/settings.json'
    [IO.Directory]::CreateDirectory((Split-Path -Parent $path))|Out-Null
    $path
}
function Trace-File([string]$Kind='ui-state',[switch]$Isolated){
    $folder=if($script:CapyTraceDirectory){$script:CapyTraceDirectory}else{$directory}
    foreach($path in [IO.Directory]::EnumerateFiles($folder,"$Kind-$($review.Id)-*.json")){
        if([IO.File]::GetLastWriteTimeUtc($path) -lt $review.StartTime.ToUniversalTime()){continue}
        try{$value=Read-Snapshot $path}catch{continue}
        if($value.process_id -eq $review.Id -and (!$Isolated -or $value.model.windows_isolated_settings)){return $path}
    }
}
function State-File{
    if($script:CapyStateFile){return $script:CapyStateFile}
    if(!$script:CapyTrace -or $script:CapyTrace.process -ne $review.Id){$script:CapyTrace=@{process=$review.Id;path=$null;model=$null}}
    if(!$script:CapyTrace.path){$script:CapyTrace.path=Trace-File -Isolated}
    $script:CapyTrace.path
}
function Model{
    $model=$null
    try{
        $path=State-File
        if($path){
            $value=Read-Snapshot $path
            if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){
                $model=$value.model
                $camera=Join-Path (Split-Path -Parent $path) ((Split-Path -Leaf $path) -replace '^ui-state-','camera-state-')
                if($camera -ne $path -and [IO.File]::Exists($camera)){
                    try{
                        $view=Read-Snapshot $camera
                        if($view.process_id -eq $review.Id -and $view.window_id -eq $value.window_id -and $view.camera.revision -ge $model.state.camera.revision){$model.state.camera=$view.camera}
                    }catch{}
                }
            }
        }
    }catch{}
    if(!$script:CapyCacheModel -or !$script:CapyTrace){return $model}
    if($model){$script:CapyTrace.model=$model}
    if($script:CapyTrace.process -eq $review.Id){$script:CapyTrace.model}
}
function Capture([string]$Name,[switch]$WithModel,[switch]$Composed){
    if($script:CapyCaptureDelay){Start-Sleep -Milliseconds $script:CapyCaptureDelay}
    & (Join-Path $script:CapyScripts 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly -Composed:$Composed *> (Join-Path $run ($Name+'.json'))
    if($WithModel){(Model)|ConvertTo-Json -Depth 80|Set-Content (Join-Path $run ($Name+'-model.json'))}
}
function Enter-CapyEnvironment([string[]]$Names=@()){
    if($env:CAPY_FIXTURE_CONTEXT -and $run){@{run=$run}|ConvertTo-Json|Set-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT}
    $script:CapyEnvironment=@{}
    foreach($name in @('CAPY_STORAGE_DIR','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY')+$Names){
        $script:CapyEnvironment[$name]=[Environment]::GetEnvironmentVariable($name,'Process')
        Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue
    }
}
function Exit-CapyEnvironment{
    if(!$script:CapyEnvironment){return}
    if($env:CAPY_FIXTURE_CONTEXT -and $run){
        @{run=$run;process_id=$review.Id;trace_directory=$(if($script:CapyTraceDirectory){$script:CapyTraceDirectory}else{$directory});profile=$env:CAPY_STORAGE_DIR}|
            ConvertTo-Json|Set-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT
    }
    foreach($entry in $script:CapyEnvironment.GetEnumerator()){
        if($null -eq $entry.Value){Remove-Item -LiteralPath ('Env:'+$entry.Key) -ErrorAction SilentlyContinue}
        else{[Environment]::SetEnvironmentVariable($entry.Key,$entry.Value,'Process')}
    }
}
function Invoke-Id([string]$Id){
    $hit=@{item=$null}
    Wait-Until {$hit.item=Find $Id;$hit.item -and $hit.item.Current.IsEnabled} "Missing enabled control: $Id"
    Invoke $Id
}
function Tool-Tile([string]$Command){
    $target=@{id=$null}
    Wait-Until {foreach($panel in (Model).panels){foreach($tile in $panel.tiles){
        if($tile.control.command -eq $Command -or $tile.resolved_control.command -eq $Command){$target.id="tile-$($panel.id)-$($tile.id)";return $true}
    }};$false} "No $Command tile"
    $target.id
}
function Tool-Choice([string]$Command){
    $target=@{id=$null;list=$null;index=-1}
    Wait-Until {
        $set=(Model).state.tool_set
        foreach($list in @(@('groups','tool-group-'),@('subtools','tool-subtool-'))){
            $items=@($set.($list[0]))
            for($i=0;$i -lt $items.Count;$i++){$action=$items[$i].action
                if($action.command -eq $Command -or $action.variant.command -eq $Command){$target.id=$list[1]+$i;$target.list=$list[0];$target.index=$i;return $true}}
        }
        $false
    } "No $Command tool choice"
    $target
}
function Invoke-PickerButton($Picker,[string]$Id='1'){
    if($Picker.Current.ClassName -ne '#32770' -or $Picker.Current.ProcessId -ne $review.Id){throw 'Picker does not belong to the isolated review'}
    $window=[IntPtr]$Picker.Current.NativeWindowHandle;$hit=@{item=$null}
    Wait-Until {
        $fresh=[System.Windows.Automation.AutomationElement]::FromHandle($window)
        if($fresh.Current.ProcessId -ne $review.Id -or $fresh.Current.ClassName -ne '#32770'){return $false}
        $hit.item=$fresh.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
        $hit.item -and $hit.item.Current.ClassName -eq 'Button' -and $hit.item.Current.IsEnabled -and $hit.item.Current.NativeWindowHandle -ne 0
    } 'Native picker confirmation did not become ready' 15
    $handle=[IntPtr]$hit.item.Current.NativeWindowHandle;$owner=[uint32]0
    [CapyWindowApi]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
    if($owner -ne $review.Id){throw 'Native picker button has an unexpected owner'}
    if(![CapyWindowApi]::PostMessage($handle,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Cannot invoke native picker button'}
}
function Open-Project([string]$Path){
    & (Join-Path $script:CapyScripts 'open-application-menu.ps1') -Root $root -Name 'File'
    Invoke-Id 'open_document'
    $hit=@{edit=$null}
    Wait-Until {$hit.edit=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit'),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')));$null -ne $hit.edit} 'Missing Open picker' 45
    [CapyWindowApi]::Path([IntPtr]$hit.edit.Current.NativeWindowHandle,$Path)
    Invoke-PickerButton (Find 'Open' -Name)
    Wait-Until {!(Find 'Open' -Name)} 'Open did not finish' 90
}
function Zoom-Item([string]$Id){
    $owned=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
    [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$owned)
}
function Fit-Canvas{
    if(!(Find 'canvas-view-info')){return}
    Invoke-Id 'canvas-view-info'
    $hit=@{item=$null}
    Wait-Until {$hit.item=Zoom-Item 'zoom-fit_canvas';$hit.item -and $hit.item.Current.IsEnabled} 'The zoom menu did not offer Fit'
    $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Zoom-Item 'zoom-fit_canvas')} 'The zoom menu did not close after Fit'
}
function Save-ProjectAs([string]$Path){
    & (Join-Path $script:CapyScripts 'open-application-menu.ps1') -Root $root -Name 'File'
    Invoke-Id 'save_document_as'
    $hit=@{entry=$null;picker=$null}
    Wait-Until {
        $hit.picker=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'#32770'))
        if(!$hit.picker){return $false}
        $hit.entry=$hit.picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit'),
            [System.Windows.Automation.OrCondition]::new(
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148'),
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'))))
        $null -ne $hit.entry
    } 'The Save As filename edit did not appear' 45
    if($hit.picker.Current.ProcessId -ne $review.Id -or $hit.entry.Current.ProcessId -ne $review.Id){throw 'Wrong picker filename owner'}
    [CapyWindowApi]::Path([IntPtr]$hit.entry.Current.NativeWindowHandle,$Path)
    Invoke-PickerButton $hit.picker
    Wait-Until {(Test-Path -LiteralPath $Path) -and (Model).state.document_file.location.uri -eq $Path -and !(Model).state.document_file.busy -and !(Model).state.document_file.modified} 'The drawing did not save' 90
    Wait-Until {!(Find 'Save As' -Name) -and (Control 'drawing-canvas').Current.IsEnabled} 'The Save As dialog did not close'
}
function Workspace-Root{
    $named=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Drawing workspace')
    foreach($workspace in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$named)){
        if($workspace.Current.ItemStatus){return $workspace}
    }
}
