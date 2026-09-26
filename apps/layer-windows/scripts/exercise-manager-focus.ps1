param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyManagerKeys {
 [StructLayout(LayoutKind.Sequential)] public struct Keyboard {public ushort key,scan;public uint flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)] public struct Input {[FieldOffset(0)]public uint type;[FieldOffset(8)]public Keyboard keyboard;}
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 public static void Key(uint process,ushort key) {
   uint owner;GetWindowThreadProcessId(GetForegroundWindow(),out owner);
   if(owner!=process)throw new Exception("Review does not own keyboard focus; no key sent.");
   var input=new[]{new Input{type=1,keyboard=new Keyboard{key=key}},new Input{type=1,keyboard=new Keyboard{key=key,flags=2}}};
   if(SendInput(2,input,40)!=2)throw new Exception("Windows rejected the review key.");
 }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/manager-focus/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Manager {(Model).windows_workspace_manager}
function Layout {(Model).state.workspace | ConvertTo-Json -Depth 80 -Compress}
function Edit([string]$Id,[string]$Value){(Control $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)}
function Button([string]$Name){
    Control $Name -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button)
}
function Choose([string]$Name){(Button $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Settled {
    Wait-Until {$v=Manager;$null -ne $v -and !$v.loading -and !$v.busy} 'Manager did not settle'
    if((Manager).error){throw (Manager).error}
}
function Closed {
    Wait-Until {$null -eq (Manager) -and $null -eq (Find 'workspace-manager')} 'Manager did not close'
}
function Menu([string]$Name){
    Closed
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    (Control 'Workspaces' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Invoke $Name -Name
    Wait-Until {$null -ne (Find 'workspace-manager')} 'Native manager did not open'
    Settled
}
function Select-Row([string]$Id){
    $item=Control ('workspace-manager-row-'+$Id)
    $item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $Id -and !(Manager).loading} 'Selected preview did not settle'
    if((Manager).error){throw (Manager).error}
    $item
}
function Row([string]$Title){(Manager).rows|Where-Object title -eq $Title|Select-Object -First 1}
function Launch([string]$Label){
    $script:stderr=Join-Path $run ($Label+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/manager-focus-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned manager review $($review.Id) ($Label)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Manager review did not start' 45
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
}
function Close {
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
}
try {
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    Launch 'first'
    Menu 'Manage Workspaces…'
    $original=(Manager).selected
    Choose 'Cancel';Closed
    $first=$review;$firstRoot=$root
    Launch 'second'
    $second=$review;$secondRoot=$root
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/manager-focus-review.json'),(@{processes=@($first.Id,$second.Id);run=$run}|ConvertTo-Json))
    if((Model).windows_workspace.id -eq $original){throw 'Second window reused the first window workspace'}
    $secondName=(Model).windows_workspace.name
    Menu 'Manage Workspaces…'
    $secondWorkspace=(Manager).selected
    if($secondWorkspace -eq $original){throw 'Concurrent windows share one active workspace'}
    $null=Select-Row $original
    if((Manager).apply_label -ne 'Switch to Window'){throw 'Owned workspace did not offer Switch to Window'}
    Capture 'owned-workspace'
    Choose 'Switch to Window'
    Wait-Until {$null -eq (Find 'workspace-manager')} 'Switch to Window did not dismiss the source manager'
    Wait-Until {
        [uint32]$foregroundProcess=0
        [CapyManagerKeys]::GetWindowThreadProcessId([CapyManagerKeys]::GetForegroundWindow(),[ref]$foregroundProcess)|Out-Null
        $foregroundProcess -eq $first.Id
    } 'Windows did not activate the workspace owner'
    if((Model).windows_workspace.name -ne $secondName){throw 'Focusing another window changed the source workspace'}
    $review=$first;$root=$firstRoot;$stderr=Join-Path $run 'first-stderr.log'
    Close
    $review=$second;$root=$secondRoot;$stderr=Join-Path $run 'second-stderr.log'
    Close
    [pscustomobject]@{independent_workspaces='passed';owned_workspace_action='passed';native_window_activation='passed';source_workspace_retained='passed';zero_exit='passed';scope='two isolated native application processes; same-process New Window and physical input remain separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
