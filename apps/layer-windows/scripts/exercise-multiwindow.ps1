param([Parameter(Mandatory)][string]$Executable,[switch]$RecoverGpu,[switch]$FailPreferences)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyWindowTest {
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window,uint message,UIntPtr w,IntPtr l);
 public static void Check(uint process,IntPtr window) {
   uint owner;GetWindowThreadProcessId(window,out owner);
   if(owner!=process || !IsWindow(window))throw new Exception("Window is not owned by this review.");
 }
 public static void Close(uint process,IntPtr window) {
   Check(process,window);
   if(!PostMessage(window,16,UIntPtr.Zero,IntPtr.Zero))throw new Exception("Windows rejected close.");
 }
}
'@
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/multiwindow/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
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
function Ready($Window){
    Wait-Until {(Model $Window).brush_ready -and (Model $Window).windows_workspace.ready} "Window $($Window.id) did not become ready" 45
}
function Preferences {Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)}
function Invoke-Edit([string]$Name) {
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'edit'
    (Control $Name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Base-Entry([string]$Theme) {
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    $label="$Theme theme base color"
    $entry=Find $label -Name -Within $dialog -Type ([System.Windows.Automation.ControlType]::Edit)
    if($entry){return $entry}
    $swatch='setting-'+$Theme.ToLowerInvariant()+'_base-swatch-*'
    $hit=@{item=$null};Wait-Until {
        $hit.item=@($dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Custom'))|Where-Object {$_.Current.AutomationId -like $swatch})[0]
        $null -ne $hit.item
    } "Missing custom $Theme base swatch"
    $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Control $label -Name -Within $dialog -Type ([System.Windows.Automation.ControlType]::Edit)
}
function Open-Preferences {
    Invoke-Edit 'Preferences'
    Wait-Until {$null -ne (Preferences)} 'Preferences did not open'
}
function Close-Preferences {
    # Wait for automation to expose the dialog after a theme update before
    # passing its scope to the child-control query.
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    Invoke 'Close' -Name -Within $dialog
    Wait-Until {$null -eq (Preferences) -and !(Model).state.settings_open} 'Preferences did not close'
}
function Close-Window($Window){
    [CapyWindowTest]::Close([uint32]$review.Id,[IntPtr]$Window.hwnd)
    Wait-Until {![CapyWindowTest]::IsWindow([IntPtr]$Window.hwnd)} 'Window close exceeded five seconds'
}
try {
    Enter-CapyEnvironment
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
    Wait-Until {!(Model).windows_workspace.switcher_busy} 'Switcher refresh did not settle'
    $firstRevision=(Model $first).windows_workspace.switcher_revision
    Invoke ('workspace-manager-options-'+$owned)
    (Control 'workspace-manager-show').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {
        (Model $first).windows_workspace.switcher.id -notcontains $owned -and
        (Model $second).windows_workspace.switcher.id -notcontains $owned -and
        !(Model $first).windows_workspace.switcher_busy -and !(Model $second).windows_workspace.switcher_busy
    } 'Pin preferences did not refresh in the inactive window'
    if((Model $first).windows_workspace.id -ne $owned -or (Model $first).windows_workspace.switcher_display[0].id -ne $owned){throw 'Remote unpin changed ownership or lost the current-workspace fallback'}
    if((Model $first).windows_workspace.switcher_revision -ne $firstRevision){throw 'External refresh rebroadcast the preference edit'}
    $moveId=@((Model).windows_workspace.order)[-1]
    $before=@((Model).windows_workspace.order) -join '|'
    Invoke ('workspace-manager-options-'+$moveId)
    Invoke 'workspace-manager-move-up'
    Wait-Until {
        $source=@((Model $second).windows_workspace.order) -join '|'
        $target=@((Model $first).windows_workspace.order) -join '|'
        $source -ne $before -and $target -eq $source
    } 'Workspace order did not propagate to the inactive window'
    if((Model).windows_workspace_manager.selected -ne $secondWorkspace){throw 'Remote preferences changed the manager preview'}
    (Control ('workspace-manager-row-'+$owned)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).windows_workspace_manager.selected -eq $owned -and (Model).windows_workspace_manager.apply_label -eq 'Switch to Window'} 'Owned workspace did not offer its native window'
    Invoke 'Switch to Window' -Name -Within (Control 'workspace-manager')
    Wait-Until {$null -eq (Find 'workspace-manager') -and [CapyWindowTest]::GetForegroundWindow() -eq [IntPtr]$first.hwnd} 'Workspace manager did not activate the owning window'
    if((Model $second).windows_workspace.id -ne $secondWorkspace){throw 'Window activation changed the source workspace'}
    Use-Window $first;Open-Preferences
    Use-Window $second;Open-Preferences
    Use-Window $first
    if(!(Preferences)){throw 'Second dialog displaced the first window dialog'}
    $entry=Base-Entry 'Dark'
    $entry.SetFocus()
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#1c2c3c')
    (Base-Entry 'Light').SetFocus()
    Wait-Until {(Model $first).state.settings.dark_base -eq '#1c2c3c' -and (Model $second).state.settings.dark_base -eq '#1c2c3c'} 'Preference edit did not propagate to both render owners'
    Close-Preferences
    Use-Window $second
    if(!(Preferences)){throw 'Closing one dialog dismissed another window dialog'}
    $entry=Base-Entry 'Dark'
    if($entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -ne '#1c2c3c'){throw 'Second Preferences dialog retained stale values'}
    $entry=Base-Entry 'Light'
    $entry.SetFocus()
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#dcecfb')
    (Base-Entry 'Dark').SetFocus()
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
    if($RecoverGpu){
        function Recovery-Signature($Window){
            $state=(Model $Window).state
            $state.document_file.PSObject.Properties.Remove('revision')
            @($state.document_file,$state.camera,$state.workspace,$state.brush)|ConvertTo-Json -Depth 80 -Compress
        }
        $before=@{}
        foreach($window in @($first,$second)){
            $before[$window.id]=Recovery-Signature $window
        }
        # One removal affects the process/adapter, including its idle sibling.
        foreach($source in @($first,$second)){
            $generations=@{};$revisions=@{}
            foreach($window in @($first,$second)){
                $state=Model $window
                $generations[$window.id]=$state.windows_gpu_generation
                $revisions[$window.id]=$state.state.document_file.revision
            }
            Use-Window $source
            Invoke 'Test GPU loss' -Name
            foreach($window in @($first,$second)){
                Wait-Until {$state=Model $window;$state.windows_gpu_generation -eq $generations[$window.id]+1 -and $state.brush_ready} "Window $($window.id) did not reconstruct its GPU" 60
                if((Model $window).state.document_file.revision -ne $revisions[$window.id]){throw 'Multiwindow recovery changed a document revision'}
                Wait-Until {(Recovery-Signature $window) -eq $before[$window.id]} 'Multiwindow recovery changed document, history, camera, workspace or brush'
            }
        }
        foreach($window in @($first,$second)){
            Use-Window $window
            Invoke-Edit 'Undo'
            Wait-Until {!(Model).state.document_file.modified} 'Undo after multiwindow recovery did not restore the clean document'
            Invoke-Edit 'Redo'
            Wait-Until {(Recovery-Signature $window) -eq $before[$window.id]} 'Redo after multiwindow recovery did not restore its document'
        }
        Use-Window $first
    }
    $expectedDark='#1c2c3c'
    if($FailPreferences){
        $settingsFile=Join-Path $env:CAPY_SETTINGS_DIRECTORY 'settings.json'
        function Saved-Preferences {try{Get-Content -LiteralPath $settingsFile -Raw|ConvertFrom-Json}catch{$null}}
        Wait-Until {(Saved-Preferences).dark_base -eq $expectedDark -and (Saved-Preferences).light_base -eq '#dcecfb'} 'Shared preferences were not saved before the failure fixture'
        $savedHash=(Get-FileHash -LiteralPath $settingsFile).Hash
        # Deny replacement of this fixture's settings file, leaving the previous
        # saved bytes readable. Each native window must keep its own close state.
        $lockedPreferences=[IO.File]::Open($settingsFile,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
        Open-Preferences
        $expectedDark='#2d3e4f'
        $entry=Base-Entry 'Dark'
        $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($expectedDark)
        (Base-Entry 'Light').SetFocus()
        Wait-Until {(Model $first).state.settings.dark_base -eq $expectedDark -and (Model $second).state.settings.dark_base -eq $expectedDark -and (Model $first).state.host_error} 'Denied preference write was not published while retaining shared values'
        Close-Preferences
    }
    [CapyWindowTest]::Close([uint32]$review.Id,[IntPtr]$first.hwnd)
    Invoke 'Discard Changes' -Name -Within (Control 'document-dialog')
    if($FailPreferences){
        function Wait-PreferenceRecovery {
            Wait-Until {
                $state=(Model $first).windows_settings_close;$dialog=Find 'preferences-close-error'
                $state.requested -and !$state.ready -and !$state.busy -and $state.error -and $dialog -and !$dialog.Current.IsOffscreen
            } 'Closing preference failure did not remain in its owning window'
        }
        Wait-PreferenceRecovery
        if((Model $first).windows_workspace.close_requested){throw 'Failed preferences close released the workspace before recovery'}
        if((Get-FileHash -LiteralPath $settingsFile).Hash -ne $savedHash){throw 'Denied preference replacement changed saved bytes'}
        Use-Window $second
        if(Find 'preferences-close-error'){throw 'Preference recovery appeared in the other window'}
        Invoke-Edit 'Undo'
        Wait-Until {!(Model $second).state.document_file.modified} 'Other window could not undo while its peer awaited preference recovery'
        Invoke-Edit 'Redo'
        Wait-Until {(Model $second).state.document_file.modified} 'Other window could not redo while its peer awaited preference recovery'
        Use-Window $first
        $attempt=(Model $first).windows_settings_close.attempt
        Invoke 'Retry' -Name -Within (Control 'preferences-close-error')
        Wait-Until {(Model $first).windows_settings_close.attempt -gt $attempt} 'Failed preference retry did not start'
        Wait-PreferenceRecovery
        if((Get-FileHash -LiteralPath $settingsFile).Hash -ne $savedHash){throw 'Failed preference retry changed saved bytes'}
        $lockedPreferences.Dispose();$lockedPreferences=$null
        Invoke 'Retry' -Name -Within (Control 'preferences-close-error')
    }
    Wait-Until {![CapyWindowTest]::IsWindow([IntPtr]$first.hwnd) -and @(Windows).Count -eq 1} 'Initial window did not close independently'
    if($FailPreferences -and ((Saved-Preferences).dark_base -ne $expectedDark -or (Saved-Preferences).light_base -ne '#dcecfb')){throw 'Closing retry did not preserve both windows preference edits'}
    Use-Window $second
    (Control 'Drawing canvas' -Name).SetFocus()
    [CapyWindowTest]::SetForegroundWindow([IntPtr]$second.hwnd)|Out-Null
    Wait-Until {[CapyWindowTest]::GetForegroundWindow() -eq [IntPtr]$second.hwnd} 'Second window could not become active'
    [CapyWindowTest]::Check([uint32]$review.Id,[IntPtr]$second.hwnd)
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17,16),78)
    Wait-Until {@(Windows).Count -eq 2} 'Ctrl+Shift+N failed after the original window closed'
    $third=@(Windows|Where-Object id -ne $second.id)[0];Ready $third
    if((Model $third).state.settings.dark_base -ne $expectedDark -or (Model $third).state.settings.light_base -ne '#dcecfb'){throw 'New window did not inherit current preferences'}
    Use-Window $second
    [CapyWindowTest]::Close([uint32]$review.Id,[IntPtr]$second.hwnd)
    Invoke 'Discard Changes' -Name -Within (Control 'document-dialog')
    Wait-Until {![CapyWindowTest]::IsWindow([IntPtr]$second.hwnd)} 'Second window did not close'
    [CapyWindowTest]::Close([uint32]$review.Id,[IntPtr]$third.hwnd)
    if(!$review.WaitForExit(5000)){throw 'Final window process exit exceeded five seconds'}
    if($review.ExitCode -ne 0){throw "Native process exited $($review.ExitCode)"}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{new_window_menu='passed';workspace_owner_activation='passed';new_window_shortcut='passed';simultaneous_dialogs='passed';shared_preferences='passed';workspace_switcher_preferences='passed';inactive_window_order_and_fallback='passed';new_window_preferences='passed';independent_documents='passed';draw_while_other_window_modal='passed';cancel_close='passed';close_original_first='passed';final_zero_exit='passed';preferences_close_recovery=if($FailPreferences){'owned recovery, peer Undo/Redo, failed retry, repaired save and new-window inheritance passed'}else{'not requested'};gpu_recovery=if($RecoverGpu){'both windows recover two shared device removals, retained state and Undo/Redo'}else{'not requested'};scope='native windows in one process; controlled pointer replay and OS shortcut injection; physical input and presentation acceptance remain separate'}|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    if($lockedPreferences){$lockedPreferences.Dispose()}
    Exit-CapyEnvironment
}
