Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
if(!('CapyWindowApi' -as [type])){Add-Type -TypeDefinition @"
using System;using System.Runtime.InteropServices;using System.Text;
public static class CapyWindowApi {
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
    }while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Within=$root,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    if(!$Within){$Within=$root}
    $found=$null
    if('first' -eq $script:CapyFind){$found=$Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)}
    else{
        $items=$Within.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)
        foreach($item in $items){if(!$item.Current.IsOffscreen){return $item}}
        if('prefer-visible' -eq $script:CapyFind -and $items.Count){$found=$items[0]}
    }
    if(!$found -and $script:CapyPopups -and $Within -eq $root){
        $owned=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
        $found=[System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$owned)
    }
    $found
}
function Control([string]$Value,[switch]$Name,$Within=$root,$Type,[int]$Seconds=$script:CapyWaitSeconds){
    $hit=@{item=$null}
    Wait-Until {$hit.item=Find $Value -Name:$Name -Within $Within -Type $Type;$null -ne $hit.item} "Missing control: $Value" $Seconds
    $hit.item
}
function Invoke([string]$Value,[switch]$Name,$Within=$root){
    $item=Control $Value -Name:$Name -Within $Within;$pattern=$null
    if($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
    else{$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
}
function Read-Snapshot([string]$Path){
    $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader=[IO.StreamReader]::new($stream)
    try{$reader.ReadToEnd()|ConvertFrom-Json}finally{$reader.Dispose()}
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
function Capture([string]$Name,[switch]$WithModel){
    if($script:CapyCaptureDelay){Start-Sleep -Milliseconds $script:CapyCaptureDelay}
    & (Join-Path $script:CapyScripts 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
    if($WithModel){(Model)|ConvertTo-Json -Depth 80|Set-Content (Join-Path $run ($Name+'-model.json'))}
}
function Enter-CapyEnvironment([string[]]$Names=@()){
    $script:CapyEnvironment=@{}
    foreach($name in @('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY')+$Names){
        $script:CapyEnvironment[$name]=[Environment]::GetEnvironmentVariable($name,'Process')
        Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue
    }
}
function Exit-CapyEnvironment{
    if(!$script:CapyEnvironment){return}
    foreach($entry in $script:CapyEnvironment.GetEnumerator()){
        if($null -eq $entry.Value){Remove-Item -LiteralPath ('Env:'+$entry.Key) -ErrorAction SilentlyContinue}
        else{[Environment]::SetEnvironmentVariable($entry.Key,$entry.Value,'Process')}
    }
}
function Invoke-Id([string]$Id){
    $hit=@{item=$null}
    Wait-Until {$hit.item=Find $Id;$hit.item -and $hit.item.Current.IsEnabled} "Missing enabled control: $Id"
    $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Open-Project([string]$Path){
    & (Join-Path $script:CapyScripts 'open-application-menu.ps1') -Root $root -Name 'File'
    Invoke-Id 'open_document'
    $hit=@{edit=$null}
    Wait-Until {$hit.edit=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit'),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')));$null -ne $hit.edit} 'Missing Open picker' 45
    [CapyWindowApi]::Path([IntPtr]$hit.edit.Current.NativeWindowHandle,$Path)
    $button=(Find 'Open' -Name).FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1'))
    [CapyWindowApi]::PostMessage([IntPtr]$button.Current.NativeWindowHandle,245,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
    Wait-Until {!(Find 'Open' -Name)} 'Open did not finish' 90
}
