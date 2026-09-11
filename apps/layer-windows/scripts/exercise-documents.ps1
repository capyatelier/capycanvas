param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CapyDocumentControls {
    [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
    [DllImport("user32.dll",SetLastError=true)] public static extern bool PostMessage(IntPtr h,uint message,UIntPtr w,IntPtr l);
    [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,StringBuilder text,uint flags,uint timeout,out UIntPtr result);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint process);
    public static void TypeText(IntPtr edit,string text) {
        UIntPtr result;
        if(SendMessageTimeout(edit,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot select picker text");
        if(SendMessageTimeout(edit,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot clear picker text");
        foreach(char c in text)
            if(SendMessageTimeout(edit,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot type picker text");
        var actual=new StringBuilder(32768);if(SendMessageTimeout(edit,13,new UIntPtr((uint)actual.Capacity),actual,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot verify native picker filename");
        if(actual.ToString()!=text)throw new Exception("The native picker filename did not match the test path");
    }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/document-ui/'+[Guid]::NewGuid().ToString('N'))
$settingsProfile=Join-Path $run 'profile'
[IO.Directory]::CreateDirectory($settingsProfile)|Out-Null
$stateFile=Join-Path $directory 'ui-state.json'
$first=Join-Path $run 'Drawing one 日本語.capy'
$second=Join-Path $run 'Drawing two.capy'
$corrupt=Join-Path $run 'Corrupt.capy'
[IO.File]::WriteAllText($corrupt,'synthetic invalid project')
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
try {
$env:CAPY_SETTINGS_DIRECTORY=$settingsProfile
$env:CAPY_TRACE_UI='1'
$env:CAPY_SMOKE_TEST='1'
$env:CAPY_TEST_DISPLAY='1'
$env:CAPY_TEST_PRIMARY='1'
Remove-Item Env:CAPY_PRESENT_PROBE -ErrorAction SilentlyContinue
$stderr=Join-Path $run 'stderr.log'
$review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
[IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/document-ui-review.pid'),[string]$review.Id)
Write-Output "Document review process $($review.Id)"
function Model {
    try {$snapshot=Get-Content -LiteralPath $stateFile -Raw|ConvertFrom-Json;if($snapshot.process_id -eq $review.Id){return $snapshot.model}}catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {if(& $Condition){return};$review.Refresh();if($review.HasExited){throw 'Document review exited unexpectedly'};Start-Sleep -Milliseconds 75}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Wait-Closed([string]$Message) {
    # Preserve the five-second bound and reject silent native teardown faults.
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {$review.Refresh();if($review.HasExited){break};Start-Sleep -Milliseconds 25}while($watch.Elapsed.TotalSeconds -lt 5)
    $review.Refresh();if(!$review.HasExited){throw $Message}
    if($review.ExitCode -ne 0){throw ("Native review exit code: 0x{0:X8}" -f [uint32]($review.ExitCode -band 0xffffffffL))}
}
function Find-Name([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))
    $scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Find-Id([string]$Id) {
    $scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
}
function Control([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    $script:found=$null;Wait-Until {$script:found=Find-Name $Name $Type;$null -ne $script:found} "Missing control: $Name";$script:found
}
function Invoke-Control([string]$Name) {(Control $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function File-Command([string]$Id) {
    $script:scope=$root
    Wait-Until {(Find-Name 'File').Current.IsEnabled} 'File menu stayed disabled'
    Invoke-Control 'File'
    Wait-Until {Find-Id $Id} "File command not found: $Id"
    (Find-Id $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function New-Dialog {
    $script:scope=$root
    $script:scope=Control 'New drawing' ([System.Windows.Automation.ControlType]::Window)
}
function Set-Size([string]$Width,[string]$Height) {
    (Find-Id 'document-width').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Width)
    (Find-Id 'document-height').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Height)
}
function Idle {Wait-Until {$current=Model;$current -and !$current.state.document_file.busy} 'Document request did not finish' 45}
function Confirm-Dialog {
    $script:scope=$root
    Wait-Until {@((Model).state.requests|Where-Object {$_.kind.request.type -eq 'confirm_close'}).Count -eq 1} 'Missing shared unsaved request'
    $title=((Model).state.requests|Where-Object {$_.kind.request.type -eq 'confirm_close'}).kind.request.title
    $script:scope=Control $title ([System.Windows.Automation.ControlType]::Window)
}
function Picker([string]$Name) {
    $script:scope=$root
    $script:scope=Control $Name ([System.Windows.Automation.ControlType]::Window)
    if($scope.Current.ClassName -ne '#32770' -or $scope.Current.ProcessId -ne $review.Id){throw 'Picker does not belong to the isolated review'}
}
function Picker-Button([string]$Id) {
    $item=$scope.FindFirst([System.Windows.Automation.TreeScope]::Children,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
    if(!$item -or $item.Current.ClassName -ne 'Button'){throw 'Missing native picker button'}
    $handle=[IntPtr]$item.Current.NativeWindowHandle
    $owner=[uint32]0;[CapyDocumentControls]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
    if($owner -ne $review.Id){throw 'Native picker button has an unexpected owner'}
    # These standard HWND controls expose no UIA patterns on some Windows builds.
    if(![CapyDocumentControls]::PostMessage($handle,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Cannot invoke native picker button'}
}
function Choose-Path([string]$Path) {
    if(!(Split-Path -Parent $Path).Equals($run,[StringComparison]::OrdinalIgnoreCase)){throw 'File fixture must stay in its owned directory'}
    $entry=$null
    Wait-Until {
        $script:pickerEntry=$scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.AndCondition]::new(
                [System.Windows.Automation.OrCondition]::new(
                    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'),
                    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')),
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
        $null -ne $script:pickerEntry
    } 'Missing review picker filename'
    $entry=$script:pickerEntry
    if($entry.Current.ProcessId -ne $review.Id){throw 'Wrong picker filename owner'}
    [CapyDocumentControls]::TypeText([IntPtr]$entry.Current.NativeWindowHandle,$Path)
    Picker-Button '1'
}
function Draw {
    $script:scope=$root
    $revision=(Model).state.document_file.revision
    Invoke-Control 'Test stroke'
    Wait-Until {(Model).state.document_file.modified -and (Model).state.document_file.revision -gt $revision} 'Controlled stroke did not modify the drawing'
}
Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'No review window' 30
$root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
$script:scope=$root
Wait-Until {(Model).brush_ready} 'Canvas startup did not finish' 45
if(!(Model).windows_isolated_settings){throw 'Review must use an isolated profile'}
File-Command 'new_document';New-Dialog
Set-Size '2+' '96';Invoke-Control 'Create'
Wait-Until {(Find-Id 'document-error').Current.Name} 'Invalid expression did not show validation'
if((Model).state.document_file.epoch -ne 0){throw 'Invalid size replaced the drawing'}
Set-Size '64*2' '96';Invoke-Control 'Create';Idle
Wait-Until {(Model).state.tabs[0].width -eq 128 -and (Model).state.tabs[0].height -eq 96} 'New drawing did not use shared expressions'
File-Command 'save_document';Picker 'Save As';Picker-Button '2';Idle
if((Model).state.document_file.location){throw 'Cancel acknowledged an unsaved drawing'}
File-Command 'save_document';Picker 'Save As';Choose-Path $first;Idle
Wait-Until {(Test-Path -LiteralPath $first) -and (Model).state.document_file.location.uri -eq $first} 'Save did not write the chosen Unicode path'
$hash=(Get-FileHash -LiteralPath $first).Hash
Draw
$exported=Join-Path $run 'Export 日本語.png'
File-Command 'export_document';Picker 'Save As';Picker-Button '2';Idle
if(!(Model).state.document_file.modified -or (Model).state.document_file.location.uri -ne $first){
    throw 'Cancelled export changed the project checkpoint'
}
File-Command 'export_document';Picker 'Save As';Choose-Path $exported;Idle
Wait-Until {Test-Path -LiteralPath $exported} 'Export did not create the chosen PNG'
$png=[IO.File]::ReadAllBytes($exported)
if($png.Length -lt 45 -or [Convert]::ToHexString($png[0..7]) -ne '89504E470D0A1A0A' -or
    [Convert]::ToHexString($png[12..23]) -ne '494844520000008000000060' -or
    [Convert]::ToHexString($png[($png.Length-8)..($png.Length-1)]) -ne '49454E44AE426082'){
    throw 'Export must be a complete PNG of the 128 by 96 document'
}
if(!(Model).state.document_file.modified -or (Model).state.document_file.location.uri -ne $first){
    throw 'PNG export incorrectly acknowledged a project save'
}

File-Command 'save_document';Idle
Wait-Until {$current=Model;$current -and !$current.state.document_file.modified} 'Save to existing location did not clear the captured checkpoint'
if((Get-FileHash -LiteralPath $first).Hash -eq $hash){throw 'Save did not update the source project'}
File-Command 'save_document_as';Picker 'Save As';Picker-Button '2';Idle
if((Model).state.document_file.location.uri -ne $first){throw 'Cancelled Save As changed location'}
File-Command 'save_document_as';Picker 'Save As';Choose-Path $second;Idle
Wait-Until {(Test-Path -LiteralPath $second) -and (Model).state.document_file.location.uri -eq $second} 'Save As did not adopt the new path'
$epoch=(Model).state.document_file.epoch
File-Command 'open_document';Picker 'Open';Choose-Path $corrupt;Idle
Wait-Until {(Model).state.host_error -or (Model).error} 'Corrupt Open did not report failure'
if((Model).state.document_file.epoch -ne $epoch -or (Model).state.document_file.location.uri -ne $second){throw 'Corrupt Open replaced the live drawing'}
Draw
File-Command 'new_document';Confirm-Dialog;Invoke-Control 'Cancel';Idle
if((Model).state.document_file.epoch -ne $epoch -or !(Model).state.document_file.modified){throw 'Cancelled replacement lost edits'}
File-Command 'new_document';Confirm-Dialog;Invoke-Control 'Discard Changes';New-Dialog;Invoke-Control 'Cancel';Idle
if((Model).state.document_file.epoch -ne $epoch -or !(Model).state.document_file.modified){throw 'Cancelled New after discard approval lost the current drawing'}
File-Command 'open_document';Confirm-Dialog;Invoke-Control 'Save';Picker 'Open';Choose-Path $first;Idle
Wait-Until {(Model).state.document_file.epoch -gt $epoch -and (Model).state.document_file.location.uri -eq $first} 'Save-before-Open did not adopt the selected file'
if((Model).state.document_file.modified){throw 'Opened project is unexpectedly dirty'}
Draw
$script:scope=$root;Invoke-Control 'Preferences'
$script:scope=Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)
$entry=Control 'Dark theme base color' ([System.Windows.Automation.ControlType]::Edit)
$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#223344')
$review.CloseMainWindow()|Out-Null
Confirm-Dialog;Invoke-Control 'Cancel';Idle
Wait-Until {(Model).state.settings.dark_base -eq '#223344'} 'Close request lost the active Preferences draft'
if((Model).state.document_file.close_ready -or !(Model).state.document_file.modified){throw 'Cancel closed or cleared the dirty drawing'}
$script:scope=$root
$review.CloseMainWindow()|Out-Null
Confirm-Dialog;Invoke-Control 'Save'
Wait-Closed 'Saved close exceeded five seconds'
if((Get-Item -LiteralPath $stderr).Length){throw 'Native review reported stderr'}
# A fresh untitled drawing exercises close -> Save picker -> Cancel, then
# explicit Discard. The first launch above verifies durable close to a saved path.
$stderr=Join-Path $run 'untitled.stderr.log'
$review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
[IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/document-ui-review.pid'),[string]$review.Id)
Write-Output "Untitled close review process $($review.Id)"
Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'No untitled review window' 30
$root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
$script:scope=$root
Wait-Until {(Model).brush_ready} 'Untitled canvas did not finish starting' 45
Draw
File-Command 'close_document';Confirm-Dialog;Invoke-Control 'Save';Picker 'Save As';Picker-Button '2';Idle
if(!(Model).state.document_file.modified -or (Model).state.document_file.location -or (Model).state.document_file.close_ready){
    throw 'Cancelling the close-time picker lost the untitled drawing'
}
File-Command 'close_document';Confirm-Dialog;Invoke-Control 'Cancel';Idle
$review.CloseMainWindow()|Out-Null
Confirm-Dialog;Invoke-Control 'Discard Changes'
Wait-Closed 'Discarded close exceeded five seconds'
if((Get-Item -LiteralPath $stderr).Length){throw 'Untitled native review reported stderr'}
[PSCustomObject]@{
    shared_new_size_and_validation='passed'
    save_cancel_and_unicode_path='passed'
    png_export_cancel_dimensions_and_checkpoint='passed'
    save_existing_and_save_as='passed'
    corrupt_open_preserves_live_document='passed'
    replacement_cancel_and_save_before_open='passed'
    preferences_draft_and_cancelled_close='passed'
    durable_save_then_close='passed'
    untitled_close_picker_cancel_and_discard='passed'
    scope='isolated native controls and pickers; controlled stroke replay, not physical pen or cadence acceptance'
}|ConvertTo-Json
} finally {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
