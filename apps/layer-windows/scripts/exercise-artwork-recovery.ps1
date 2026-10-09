param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=60
$CapyCacheModel=$true
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/session-restart/'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$review=$null
function Find-Name([string]$Name){
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name))
}
function Invoke-Control([string]$Name){
    Invoke $Name -Name
}
function Find-Id([string]$Id){
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
}
function Command([string]$Menu,[string]$Id){
    Wait-Until {((Model).state.commands|Where-Object id -eq $Id).enabled} "Command is unavailable: $Id"
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name $Menu
    Wait-Until {$script:control=Find-Id $Id;$control -and $control.Current.IsEnabled} "Missing command: $Id"
    Invoke $Id
}
function Start-Review([string]$Label,[switch]$AllowFailure){
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run "$Label.stderr.log")
    $null=$review.Handle;$native=@{window=$null}
    Wait-Until {$native.window=Owned-DrawingWindow $review;$null -ne $native.window} 'No review window'
    $script:drawingWindow=$native.window.Handle;$script:root=$native.window.Root
    Wait-Until {$m=Model;$m.canvas_ready -and $m.brush_ready -and $m.windows_workspace.ready -and !$m.windows_recovery.busy -and !$m.windows_recovery.restoring} 'Session did not finish reopening' 120
    if(!(Model).windows_isolated_settings){throw 'Session test requires isolated settings'}
    if(!$AllowFailure -and (Model).windows_recovery.error){throw (Model).windows_recovery.error}
    if(Find-Name 'Restore drawing'){throw 'Ordinary restart presented a recovery prompt'}
}
function Crash-Review {Stop-Process -Id $review.Id;$review.WaitForExit()}
function Close-Review {
    Wait-Until {$canvas=Find-Id 'drawing-canvas';$canvas -and $canvas.Current.IsEnabled} 'Native document dialog did not finish closing'
    $owned=Owned-DrawingWindow $review $drawingWindow.ToInt64()
    if(![CapyWindowApi]::PostMessage($owned.Handle,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'The owned drawing window did not accept Close'}
    Wait-Until {$review.Refresh();if($review.HasExited){return $true};Start-Sleep -Milliseconds 250;$review.HasExited -or (Model).windows_settings_close.requested} 'Review window did not accept close'
    if(!$review.WaitForExit(15000)){throw 'Session flush did not complete window close'}
    if($review.ExitCode -ne 0){throw "Clean close failed: $($review.ExitCode)"}
}
function Session-File {
    @(Get-ChildItem (Join-Path $env:CAPY_STORAGE_DIR 'state/sessions') -Filter session.json -Recurse)|Sort-Object {(Get-Content $_.FullName -Raw|ConvertFrom-Json).generation} -Descending|Select-Object -First 1
}
function Session-Index {
    $file=Session-File;if($file){Get-Content $file.FullName -Raw|ConvertFrom-Json}
}
function Settled-Checkpoint([int]$Count){
    Wait-Until {$index=Session-Index;$index -and @($index.drawings).Count -eq $Count -and !(Model).windows_recovery.busy} 'Session checkpoint did not become durable' 120
}
function Disk-Signature {
    @(Get-ChildItem (Join-Path $env:CAPY_STORAGE_DIR 'state/sessions') -Filter '*.json' -Recurse|Sort-Object FullName|ForEach-Object{@{path=$_.FullName;bytes=$_.Length;written=$_.LastWriteTimeUtc.Ticks}})|ConvertTo-Json -Compress
}
function Failed-Source {
    $directory=Split-Path -Parent $head
    @(Get-ChildItem -LiteralPath $directory -File -Recurse|Where-Object {$_.Name -notin @('restore-error.txt','.lock')}|Sort-Object FullName|ForEach-Object {
        @{path=[IO.Path]::GetRelativePath($directory,$_.FullName);bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash}
    })|ConvertTo-Json -Compress
}
function Recovery-Diagnostic([string]$Label) {
    $diagnostic=Join-Path (Split-Path -Parent $head) 'restore-error.txt'
    Wait-Until {(Test-Path -LiteralPath $diagnostic) -and ![string]::IsNullOrWhiteSpace((Model).error)} 'Recovery did not publish its durable diagnostic'
    $detail=[IO.File]::ReadAllText($diagnostic);$recoveryMessage=(Model).error
    if([string]::IsNullOrWhiteSpace($detail) -or !$recoveryMessage.Contains($detail)){throw 'Recovery status lost the stored failure detail'}
    Wait-Until {$native=Find 'canvas-status' -Visible;$native -and $native.Current.Name -eq $recoveryMessage} 'Native recovery status did not publish the stored failure'
    $status=Control 'canvas-status' -Arranged
    if($status.Current.Name -ne $recoveryMessage){throw 'Native recovery status differs from the reported failure'}
    $source=Failed-Source
    $record=@{detail=$detail;message=$recoveryMessage;diagnostic_sha256=(Get-FileHash -LiteralPath $diagnostic).Hash;source=$source;status=@{name=$status.Current.Name;bounds=$status.Current.BoundingRectangle;runtime_id=($status.GetRuntimeId() -join ':')}}
    $record|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($Label+'-diagnostic.json'))
    if($source -ne $failedSource){throw 'Recovery changed the failed drawing source bytes'}
    $record
}
function Signature {
    $m=Model
    @{order=@($m.windows_tabs.tabs.id);active=$m.windows_tabs.selected;extent=@($m.state.tabs[0].width,$m.state.tabs[0].height);modified=$m.state.document_file.modified;
      undo=[bool](($m.state.commands|Where-Object id -eq 'undo').enabled);redo=[bool](($m.state.commands|Where-Object id -eq 'redo').enabled)}|ConvertTo-Json -Compress
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    Start-Review 'first'
    if((Model).state.theme -ne $Theme){
        Invoke 'settings-button'
        $preferences=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
        (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
        $choice=if($Theme -eq 'light'){'Light'}else{'Dark'}
        (Control $choice -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
        Wait-Until {(Model).state.theme -eq $Theme} 'Theme choice did not apply'
        Invoke 'CloseButton' -Within $preferences
        Wait-Until {!(Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)) -and (Control 'drawing-canvas').Current.IsEnabled} 'Preferences did not finish closing'
    }
    Invoke-Control 'Test pen'
    Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled} 'Drawing did not create Undo history'
    Command 'File' 'new_document'
    Wait-Until {Find-Id 'document-width'} 'New drawing did not open'
    (Control 'document-width').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('96')
    (Control 'document-height').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('72')
    Invoke-Control 'Create'
    Wait-Until {@((Model).windows_tabs.tabs).Count -eq 2 -and !(Model).state.document_file.busy} 'New drawing did not retain the first tab'
    Invoke-Control 'Test pen'
    Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled} 'Second drawing did not create Undo history'
    Command 'Edit' 'undo'
    Wait-Until {((Model).state.commands|Where-Object id -eq 'redo').enabled} 'Undo did not create Redo history'
    Start-Sleep -Seconds 3
    Settled-Checkpoint 2
    $before=Signature
    $active=(Model).windows_tabs.selected
    Crash-Review
    Start-Review 'crash-reopen'
    if((Signature) -ne $before){throw 'Crash restart changed tab order, active drawing, size, modified state or Undo/Redo'}
    if(!(Model).state.document_file.recovered){throw 'Crash restart did not mark the drawing recovered'}
    Capture 'crash-reopened' -WithModel -Composed
    Command 'Edit' 'redo'
    Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled} 'Restored Redo did not apply'
    Command 'Edit' 'undo'
    Wait-Until {(Signature) -eq $before} 'Restored Undo did not return to the captured state'
    Command 'Edit' 'redo'
    Wait-Until {(Model).state.document_file.modified} 'Restored edit did not mark the drawing modified'
    $before=Signature
    Command 'File' 'close_document'
    Wait-Until {Find-Name 'Cancel'} 'Explicit drawing close did not ask about unsaved changes'
    Invoke-Control 'Cancel'
    Wait-Until {@((Model).windows_tabs.tabs).Count -eq 2 -and !(Model).state.document_file.close_ready} 'Cancelled close removed a drawing'
    Close-Review
    Start-Review 'clean-reopen'
    if((Signature) -ne $before){throw 'Orderly restart changed the editing session'}
    if((Model).windows_tabs.selected -ne $active){throw 'Orderly restart changed the active tab'}
    Capture 'clean-reopened' -WithModel -Composed
    $index=Session-Index
    $generation=$index.generation
    $disk=Disk-Signature
    Start-Sleep -Seconds 5
    if((Session-Index).generation -ne $generation -or (Disk-Signature) -ne $disk){throw 'An idle session kept writing checkpoints'}
    Command 'File' 'save_document'
    $picker=Control 'Save As' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    Wait-Until {
        $script:entry=$picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.OrCondition]::new(
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'),
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
        $null -ne $script:entry
    } 'Missing Save picker filename'
    $savedPath=Join-Path $run 'Saved drawing.capy'
    [CapyWindowApi]::Path([IntPtr]$entry.Current.NativeWindowHandle,$savedPath)
    Invoke-PickerButton $picker
    Wait-Until {!(Model).state.document_file.busy -and !(Model).state.document_file.modified -and !(Model).state.document_file.recovered} 'Save did not acknowledge the restored drawing'
    Wait-Until {(Control 'drawing-canvas').Current.IsEnabled} 'Save picker did not finish closing'
    $savedBytes=[IO.File]::ReadAllBytes($savedPath)
    Close-Review
    Start-Review 'intact-original'
    if((Model).state.document_file.modified){throw 'An unchanged saved original restored as modified'}
    Close-Review
    Remove-Item -LiteralPath $savedPath
    Start-Review 'missing-original'
    if(!(Model).state.document_file.modified){throw 'A missing original left its only private copy safe to close silently'}
    Command 'File' 'close_document'
    Wait-Until {Find-Name 'Cancel'} 'A missing original did not protect explicit Close'
    Invoke-Control 'Cancel'
    Wait-Until {@((Model).windows_tabs.tabs).Count -eq 2 -and !(Model).state.document_file.close_ready} 'Cancel removed the missing original private copy'
    [IO.File]::WriteAllBytes($savedPath,$savedBytes)
    Command 'File' 'save_document'
    Wait-Until {!(Model).state.document_file.busy -and !(Model).state.document_file.modified} 'Saving a restored missing original did not acknowledge its copy'
    $savedBytes=[IO.File]::ReadAllBytes($savedPath)
    Close-Review
    $changedBytes=[byte[]]$savedBytes.Clone()
    $changedBytes[$changedBytes.Length-1]=$changedBytes[$changedBytes.Length-1] -bxor 1
    [IO.File]::WriteAllBytes($savedPath,$changedBytes)
    Start-Review 'changed-original'
    if(!(Model).state.document_file.modified){throw 'A changed original left its private copy safe to close silently'}
    Command 'File' 'close_document'
    Wait-Until {Find-Name 'Cancel'} 'A changed original did not protect explicit Close'
    Invoke-Control 'Cancel'
    Wait-Until {@((Model).windows_tabs.tabs).Count -eq 2 -and !(Model).state.document_file.close_ready} 'Cancel removed the changed original private copy'
    if([Convert]::ToBase64String([IO.File]::ReadAllBytes($savedPath)) -ne [Convert]::ToBase64String($changedBytes)){throw 'Restoring a private copy changed its original'}
    [IO.File]::WriteAllBytes($savedPath,$savedBytes)
    Command 'File' 'save_document'
    Wait-Until {!(Model).state.document_file.busy -and !(Model).state.document_file.modified} 'Saving the protected private copy did not clear modified state'
    Invoke-Control 'Test pen'
    Wait-Until {(Model).state.document_file.modified} 'Editing the saved drawing did not create unsaved work'
    Command 'File' 'close_document'
    Wait-Until {Find-Name 'Discard Changes'} 'Explicit close did not offer Discard'
    Invoke-Control 'Discard Changes'
    Wait-Until {@((Model).windows_tabs.tabs).Count -eq 1} 'Explicit close did not remove the selected drawing'
    Close-Review
    Start-Review 'after-close'
    if(@((Model).windows_tabs.tabs).Count -ne 1){throw 'Restart resurrected an explicitly closed drawing'}
    $remaining=Signature
    Close-Review
    $manifest=Get-ChildItem (Join-Path $env:CAPY_STORAGE_DIR 'state/sessions') -Filter session.json -Recurse|Sort-Object LastWriteTimeUtc -Descending|Select-Object -First 1
    $original=[IO.File]::ReadAllBytes($manifest.FullName)
    [IO.File]::WriteAllText($manifest.FullName,'incomplete membership')
    Start-Review 'corrupt-index' -AllowFailure
    if(!(Model).windows_recovery.error){throw 'Corrupt session membership was silently replaced'}
    Start-Sleep -Seconds 2
    if([IO.File]::ReadAllText($manifest.FullName) -ne 'incomplete membership'){throw 'Corrupt session source was overwritten'}
    Capture 'retained-corrupt-source' -WithModel
    [IO.File]::WriteAllBytes($manifest.FullName,$original)
    Invoke-Control 'Retry Storage'
    Wait-Until {!(Model).windows_recovery.busy -and !(Model).windows_recovery.restoring -and !(Model).windows_recovery.error -and (Signature) -eq $remaining} 'Retry did not reopen the preserved drawing' 120
    Command 'File' 'new_document'
    Wait-Until {Find-Id 'document-width'} 'New drawing did not open'
    Invoke-Control 'Create'
    Wait-Until {@((Model).windows_tabs.tabs).Count -eq 2 -and !(Model).state.document_file.busy} 'New drawing did not open beside the restored one'
    Invoke-Control 'Test pen'
    Start-Sleep -Seconds 3
    Settled-Checkpoint 2
    $index=Session-Index
    $session=(Session-File).DirectoryName
    Close-Review
    $damaged=@($index.drawings|Where-Object id -ne $index.active)[0]
    $head=Join-Path $session "$($damaged.key)/head.json"
    $headBytes=[IO.File]::ReadAllBytes($head)
    [IO.File]::WriteAllText($head,'unreadable')
    $failedSource=Failed-Source
    Start-Review 'unreadable-drawing'
    Wait-Until {Find-Name 'Recover drawing'} 'An unreadable drawing did not ask what to do'
    if(@((Model).windows_tabs.tabs).Count -ne 1){throw 'An unreadable drawing kept the readable one closed'}
    $failedDiagnostic=Recovery-Diagnostic 'unreadable-drawing'
    Capture 'unreadable-drawing' -WithModel -Composed
    Invoke-Control 'Later'
    Wait-Until {!(Find-Name 'Recover drawing')} 'Later did not dismiss the recovery choice'
    Close-Review
    Start-Review 'retry-drawing'
    Wait-Until {Find-Name 'Recover drawing'} 'A drawing left for later was not offered again'
    $retriedDiagnostic=Recovery-Diagnostic 'retry-drawing'
    Capture 'retry-drawing' -WithModel -Composed
    if($retriedDiagnostic.detail -ne $failedDiagnostic.detail -or $retriedDiagnostic.message -ne $failedDiagnostic.message -or $retriedDiagnostic.diagnostic_sha256 -ne $failedDiagnostic.diagnostic_sha256){throw 'Restart replaced the durable recovery diagnostic'}
    [IO.File]::WriteAllBytes($head,$headBytes)
    Invoke-Control 'Retry Storage'
    Wait-Until {@((Model).windows_tabs.tabs).Count -eq 2 -and !(Model).windows_recovery.busy} 'Retry did not reopen the repaired drawing' 120
    Wait-Until {!(Test-Path -LiteralPath (Join-Path (Split-Path -Parent $head) 'restore-error.txt'))} 'Successful recovery did not clear the saved failure detail'
    Start-Sleep -Seconds 3
    Settled-Checkpoint 2
    Close-Review
    [IO.File]::WriteAllText($head,'unreadable')
    Start-Review 'discard-drawing'
    Wait-Until {Find-Name 'Recover drawing'} 'An unreadable drawing did not ask again'
    Invoke-Control 'Discard recovery copy'
    Settled-Checkpoint 1
    Close-Review
    Start-Review 'after-discard'
    if((Find-Name 'Recover drawing') -or @((Model).windows_tabs.tabs).Count -ne 1){throw 'A discarded drawing came back'}
    Close-Review
    foreach($log in Get-ChildItem $run -Filter '*.stderr.log'){if($log.Length){throw "Native error in $($log.Name)"}}
    [pscustomobject]@{theme=$Theme;automatic_crash_and_clean_restart='passed';tabs_active_and_history='passed';save_cancel_and_discard='passed';missing_and_changed_original_close_protection='passed';closed_drawing_stays_closed='passed';idle_write_coalescing='passed';corrupt_source_preserved_and_retried='passed';unreadable_drawing_later_retry_discard='passed';durable_failure_detail_and_source='passed';scope='isolated Windows UI on selected adapter'}|ConvertTo-Json
} catch {
    [Console]::Error.WriteLine($_.ScriptStackTrace)
    throw
} finally {
    if($review){$review.Refresh();if(!$review.HasExited){Stop-Process -Id $review.Id}}
    Exit-CapyEnvironment
}
