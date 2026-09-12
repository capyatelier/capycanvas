param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyWindowTest {
 [StructLayout(LayoutKind.Sequential)] public struct Keyboard {public ushort key,scan;public uint flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)] public struct Input {[FieldOffset(0)]public uint type;[FieldOffset(8)]public Keyboard keyboard;}
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window,uint message,UIntPtr w,IntPtr l);
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 public static void Check(uint process,IntPtr window) {
   uint owner;GetWindowThreadProcessId(window,out owner);
   if(owner!=process || !IsWindow(window))throw new Exception("Window is not owned by this review.");
 }
 public static void NewWindow(uint process,IntPtr window) {
   Check(process,window);
   if(GetForegroundWindow()!=window)throw new Exception("Review window does not own keyboard focus; no key sent.");
   var keys=new ushort[]{17,16,78,78,16,17};
   var input=new Input[6];
   for(int i=0;i<6;i++)input[i]=new Input{type=1,keyboard=new Keyboard{key=keys[i],flags=(uint)(i>=3?2:0)}};
   if(SendInput(6,input,40)!=6)throw new Exception("Windows rejected the review shortcut.");
 }
 public static void Close(uint process,IntPtr window) {
   Check(process,window);
   if(!PostMessage(window,16,UIntPtr.Zero,IntPtr.Zero))throw new Exception("Windows rejected close.");
 }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/multiwindow/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=5){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Predicate){return};$review.Refresh();if($review.HasExited){throw 'Multiwindow review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Windows {
    try{$v=Get-Content (Join-Path $directory ("windows-"+$review.Id+".json")) -Raw|ConvertFrom-Json;if($v.process_id -eq $review.Id){return @($v.windows)}}catch{}
    @()
}
function Model($Window=$current){
    try{
        $v=Get-Content (Join-Path $directory ("ui-state-"+$review.Id+"-"+$Window.id+".json")) -Raw|ConvertFrom-Json
        if($v.process_id -eq $review.Id -and $v.window_id -eq $Window.id -and $v.model.windows_isolated_settings){return $v.model}
    }catch{}
    $null
}
function Use-Window($Window){
    [CapyWindowTest]::Check([uint32]$review.Id,[IntPtr]$Window.hwnd)
    $script:current=$Window
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Window.hwnd)
}
function Find([string]$Value,[switch]$Name,$Within=$root,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Value,[switch]$Name,$Within=$root,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name -Within $Within -Type $Type;$null -ne $hit.item} "Missing $Value in window $($current.id)"
    $hit.item
}
function Invoke([string]$Value,[switch]$Name,$Within=$root){
    (Control $Value -Name:$Name -Within $Within).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Ready($Window){
    Wait-Until {(Model $Window).brush_ready -and (Model $Window).windows_workspace.ready} "Window $($Window.id) did not become ready" 45
}
function Preferences {Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)}
function Open-Preferences {
    Invoke 'Preferences' -Name
    Wait-Until {$null -ne (Preferences)} 'Preferences did not open'
}
function Close-Preferences {
    Invoke 'Close' -Name -Within (Preferences)
    Wait-Until {$null -eq (Preferences) -and !(Model).state.settings_open} 'Preferences did not close'
}
function Close-Window($Window){
    [CapyWindowTest]::Close([uint32]$review.Id,[IntPtr]$Window.hwnd)
    Wait-Until {![CapyWindowTest]::IsWindow([IntPtr]$Window.hwnd)} 'Window close exceeded five seconds'
}
try {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/multiwindow-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned multiwindow review $($review.Id)"
    Wait-Until {@(Windows).Count -eq 1} 'Initial window was not registered' 30
    $first=@(Windows)[0];Ready $first;Use-Window $first
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'file'
    Invoke 'New Window' -Name
    Wait-Until {@(Windows).Count -eq 2} 'New Window did not create a second native window'
    $second=@(Windows|Where-Object id -ne $first.id)[0];Ready $second
    if((Model $first).windows_workspace.id -eq (Model $second).windows_workspace.id){throw 'Windows share active workspace ownership'}
    Use-Window $second
    $secondWorkspace=(Model).windows_workspace.id
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    (Control 'Workspaces' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Invoke 'Manage Workspaces…' -Name
    Wait-Until {$null -ne (Find 'workspace-manager') -and !(Model).windows_workspace_manager.loading} 'Workspace manager did not open'
    $owned=(Model $first).windows_workspace.id
    (Control ('workspace-manager-row-'+$owned)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).windows_workspace_manager.selected -eq $owned -and (Model).windows_workspace_manager.apply_label -eq 'Switch to Window'} 'Owned workspace did not offer its native window'
    Invoke 'Switch to Window' -Name -Within (Control 'workspace-manager')
    Wait-Until {$null -eq (Find 'workspace-manager') -and [CapyWindowTest]::GetForegroundWindow() -eq [IntPtr]$first.hwnd} 'Workspace manager did not activate the owning window'
    if((Model $second).windows_workspace.id -ne $secondWorkspace){throw 'Window activation changed the source workspace'}
    Use-Window $first;Open-Preferences
    Use-Window $second;Open-Preferences
    Use-Window $first
    if(!(Preferences)){throw 'Second dialog displaced the first window dialog'}
    $entry=Control 'Dark theme base color' -Name -Within (Preferences) -Type ([System.Windows.Automation.ControlType]::Edit)
    $entry.SetFocus()
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#1c2c3c')
    (Control 'Light theme base color' -Name -Within (Preferences) -Type ([System.Windows.Automation.ControlType]::Edit)).SetFocus()
    Wait-Until {(Model $first).state.settings.dark_base -eq '#1c2c3c' -and (Model $second).state.settings.dark_base -eq '#1c2c3c'} 'Preference edit did not propagate to both render owners'
    Close-Preferences
    Use-Window $second
    if(!(Preferences)){throw 'Closing one dialog dismissed another window dialog'}
    $entry=Control 'Dark theme base color' -Name -Within (Preferences) -Type ([System.Windows.Automation.ControlType]::Edit)
    if($entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -ne '#1c2c3c'){throw 'Second Preferences dialog retained stale values'}
    $entry=Control 'Light theme base color' -Name -Within (Preferences) -Type ([System.Windows.Automation.ControlType]::Edit)
    $entry.SetFocus()
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#dcecfb')
    (Control 'Dark theme base color' -Name -Within (Preferences) -Type ([System.Windows.Automation.ControlType]::Edit)).SetFocus()
    Wait-Until {(Model $first).state.settings.light_base -eq '#dcecfb' -and (Model $second).state.settings.light_base -eq '#dcecfb'} 'Second window preference edit did not propagate'
    Close-Preferences
    Use-Window $first
    Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.modified} 'First window did not draw'
    if((Model $second).state.document_file.modified){throw 'Drawing dirtied the other document'}
    [CapyWindowTest]::Close([uint32]$review.Id,[IntPtr]$first.hwnd)
    Wait-Until {$null -ne (Find 'document-dialog')} 'Dirty close did not open its owning dialog'
    Use-Window $second
    Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.modified} 'Second window could not draw while the first was modal'
    Use-Window $first
    Invoke 'Cancel' -Name -Within (Control 'document-dialog')
    Wait-Until {$null -eq (Find 'document-dialog') -and @((Model).state.requests|Where-Object {$_.kind.type -eq 'document'}).Count -eq 0} 'Cancel did not keep the first window open'
    [CapyWindowTest]::Close([uint32]$review.Id,[IntPtr]$first.hwnd)
    Invoke 'Discard Changes' -Name -Within (Control 'document-dialog')
    Wait-Until {![CapyWindowTest]::IsWindow([IntPtr]$first.hwnd) -and @(Windows).Count -eq 1} 'Initial window did not close independently'
    Use-Window $second
    (Control 'Drawing canvas' -Name).SetFocus()
    [CapyWindowTest]::SetForegroundWindow([IntPtr]$second.hwnd)|Out-Null
    Wait-Until {[CapyWindowTest]::GetForegroundWindow() -eq [IntPtr]$second.hwnd} 'Second window could not become active'
    [CapyWindowTest]::NewWindow([uint32]$review.Id,[IntPtr]$second.hwnd)
    Wait-Until {@(Windows).Count -eq 2} 'Ctrl+Shift+N failed after the original window closed'
    $third=@(Windows|Where-Object id -ne $second.id)[0];Ready $third
    if((Model $third).state.settings.dark_base -ne '#1c2c3c' -or (Model $third).state.settings.light_base -ne '#dcecfb'){throw 'New window did not inherit current preferences'}
    Use-Window $second
    [CapyWindowTest]::Close([uint32]$review.Id,[IntPtr]$second.hwnd)
    Invoke 'Discard Changes' -Name -Within (Control 'document-dialog')
    Wait-Until {![CapyWindowTest]::IsWindow([IntPtr]$second.hwnd)} 'Second window did not close'
    [CapyWindowTest]::Close([uint32]$review.Id,[IntPtr]$third.hwnd)
    if(!$review.WaitForExit(5000)){throw 'Final window process exit exceeded five seconds'}
    if($review.ExitCode -ne 0){throw "Native process exited $($review.ExitCode)"}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{new_window_menu='passed';workspace_owner_activation='passed';new_window_shortcut='passed';simultaneous_dialogs='passed';shared_preferences='passed';new_window_preferences='passed';independent_documents='passed';draw_while_other_window_modal='passed';cancel_close='passed';close_original_first='passed';final_zero_exit='passed';scope='native windows in one process; controlled pointer replay and OS shortcut injection; physical input and presentation acceptance remain separate'}|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
