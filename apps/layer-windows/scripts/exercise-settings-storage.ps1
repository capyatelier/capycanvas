param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/settings-storage/'+[Guid]::NewGuid().ToString('N'))
$settingsProfile=Join-Path $run 'profile'
[IO.Directory]::CreateDirectory($settingsProfile)|Out-Null
$settingsFile=Join-Path $settingsProfile 'settings.json'
$script:review=$null
$script:launch=0
$locked=$null
function Saved {
    try {Get-Content -LiteralPath $settingsFile -Raw | ConvertFrom-Json} catch {}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {if(& $Condition){return};Start-Sleep -Milliseconds 75}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))
    $scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    $script:found=$null
    Wait-Until {$script:found=Find $Name $Type;$null -ne $script:found} "Missing control: $Name"
    $script:found
}
function Invoke-Control([string]$Name) {
    (Control $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Focus-Control($Entry) {
    $Entry.SetFocus()
    Wait-Until {$Entry.Current.HasKeyboardFocus} 'Native control did not receive focus'
}
function Edit([string]$Name,[string]$Value) {
    if($Name -like '* theme base color' -and !(Find $Name ([System.Windows.Automation.ControlType]::Edit))){
        $swatch='setting-'+$Name.Split(' ')[0].ToLowerInvariant()+'_base-swatch-*'
        $script:found=$null
        Wait-Until {
            $script:found=@($scope.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Custom'))|Where-Object {$_.Current.AutomationId -like $swatch})[0]
            $null -ne $script:found
        } "Missing custom swatch for $Name"
        $script:found.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    }
    $entry=Control $Name ([System.Windows.Automation.ControlType]::Edit)
    Focus-Control $entry
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)
    try { Wait-Until {$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $Value} 'Native draft was not updated' }
    catch { throw ("Draft {0}: expected '{1}', got '{2}', focused={3}, offscreen={4}" -f $Name,$Value,$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value,$entry.Current.HasKeyboardFocus,$entry.Current.IsOffscreen) }
}
function Commit-Color {
    Focus-Control @($scope.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Custom'))|Where-Object {$_.Current.AutomationId -like 'setting-dark_base-swatch-*'})[0]
}
function Open-Preferences {
    $script:scope=$root
    $script:found=$null
    Wait-Until {$script:found=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'settings-button'));$null -ne $script:found} 'Settings button is missing'
    $script:found.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    $script:scope=Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)
}
function Close-Preferences {
    Invoke-Control 'Close'
    $script:scope=$root
    Wait-Until {!(Find 'Preferences' ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
}
function Start-App {
    $script:launch++
    $script:stderr=Join-Path $run ("launch-$launch.stderr.log")
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;profile=$settingsProfile}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/settings-storage-review.json')
    Write-Output "Owned settings review $($review.Id), launch $launch"
    Wait-Until {
        $review.Refresh()
        if($review.HasExited){throw 'Review app exited during startup'}
        $review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready
    } 'Review startup did not complete' 45
    if(!(Model).windows_isolated_settings){throw 'Refusing to change a non-isolated settings profile'}
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $script:scope=$root
}
function Check-Exit {
    if(!$review.WaitForExit(8000)){throw 'Authorized settings close did not finish'}
    if($review.ExitCode -ne 0){throw ('Native settings exit code: '+$review.ExitCode)}
    if((Get-Item -LiteralPath $stderr).Length -ne 0){throw 'Review runtime stderr requires inspection'}
    $script:review=$null
}
function Close-App {
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    Check-Exit
}
function Close-Error {
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'preferences-close-error'))
}
function Wait-CloseError {
    Wait-Until {
        $review.Refresh();if($review.HasExited){throw 'Failed preferences save closed the window'}
        $state=(Model).windows_settings_close;$dialog=Close-Error
        $state.requested -and !$state.ready -and !$state.busy -and $state.error -and $dialog -and !$dialog.Current.IsOffscreen
    } 'Preferences close recovery did not appear'
}
function Request-CloseError {
    $script:scope=$root
    $review.CloseMainWindow()|Out-Null
    Wait-CloseError
}
function Recovery-Choice([string]$Name) {
    $dialog=Close-Error
    if(!$dialog){throw 'Preferences recovery dialog is missing'}
    $choice=$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button)))
    $choice.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
try {
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=$settingsProfile
    $env:CAPY_TRACE_UI='1'
    $env:CAPY_SMOKE_TEST='1'

    Start-App
    if(Test-Path -LiteralPath $settingsFile){throw 'Missing settings must not cause an initial write'}
    Open-Preferences
    Edit 'Dark theme base color' '#203040'
    if((Model).state.settings.dark_base -eq '#203040'){throw 'Text draft was already committed; close-time commit was not exercised'}
    Close-App
    if((Saved).dark_base -ne '#203040'){throw 'Closing lost the active text draft'}
    $stamp=[IO.File]::GetLastWriteTimeUtc($settingsFile)

    Start-App
    if((Model).state.settings.dark_base -ne '#203040'){throw 'Restart did not restore saved color'}
    if([IO.File]::GetLastWriteTimeUtc($settingsFile) -ne $stamp){throw 'Restore echoed a save'}
    if(@((Model).state.requests | Where-Object {$_.kind.type -eq 'save_settings'}).Count){throw 'Restore queued a save request'}
    Open-Preferences
    Invoke-Control 'Pen & Input'
    Edit 'Pressure response' '1.75'
    if((Model).state.settings.pressure_gamma -eq 1.75){throw 'Numeric draft was already committed; close-time commit was not exercised'}
    Close-App
    if((Saved).pressure_gamma -ne 1.75){throw 'Closing lost the active numeric draft'}

    Start-App
    if((Model).state.settings.pressure_gamma -ne 1.75){throw 'Restart did not restore numeric settings'}
    Open-Preferences
    $locked=[IO.File]::Open($settingsFile,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
    Edit 'Dark theme base color' '#304050'
    Commit-Color
    Wait-Until {(Model).state.host_error} 'A failed save did not reach the shared error state'
    $message=(Model).state.host_error
    $null=Control $message ([System.Windows.Automation.ControlType]::Text)
    if((Saved).dark_base -ne '#203040'){throw 'Failed replacement damaged the last saved file'}
    Close-Preferences
    $workspaceId=(Model).windows_workspace.id
    Request-CloseError
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'preferences-close-error.png') -ClientOnly *> (Join-Path $run 'preferences-close-error.json')
    Recovery-Choice 'Keep open'
    Wait-Until {!(Model).windows_settings_close.requested -and !(Model).state.document_file.close_ready -and !(Close-Error)} 'Keep open did not cancel preferences close'
    if((Model).state.settings.dark_base -ne '#304050' -or (Model).windows_workspace.id -ne $workspaceId -or (Model).windows_workspace.close_requested){throw 'Keep open lost preferences or released the workspace'}
    Wait-Until {
        $canvas=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Drawing canvas'))
        $canvas -and $canvas.Current.IsEnabled
    } 'Preferences did not release canvas input'
    if((Control 'Undo').Current.IsEnabled){throw 'The isolated document already has undo history'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action 'Test stroke'
    Wait-Until {(Control 'Undo').Current.IsEnabled} 'The controlled stroke did not reach the shared undo history'
    Invoke-Control 'Undo'
    Wait-Until {!(Control 'Undo').Current.IsEnabled} 'The controlled stroke could not be undone after a save failure'
    Request-CloseError
    $attempt=(Model).windows_settings_close.attempt
    Recovery-Choice 'Retry'
    Wait-Until {(Model).windows_settings_close.attempt -gt $attempt} 'Retry did not start a new save attempt'
    Wait-CloseError
    if((Saved).dark_base -ne '#203040'){throw 'Failed close retry changed the saved file'}
    $locked.Dispose();$locked=$null
    Recovery-Choice 'Retry'
    Check-Exit
    if((Saved).dark_base -ne '#304050'){throw 'Close retry did not save the retained preferences'}
    Start-App
    if((Model).state.settings.dark_base -ne '#304050'){throw 'Restart lost preferences saved by close retry'}
    Open-Preferences
    Edit 'Dark theme base color' '#405060'
    Commit-Color
    Wait-Until {(Saved).dark_base -eq '#405060' -and !(Model).state.host_error -and !(Model).error} 'A later successful save did not clear the storage error'
    Close-App

    # This profile and all changed values belong to this fixture. Exercise an
    # explicit preference discard; the drawing is still the clean new document.
    Start-App
    $savedHash=(Get-FileHash -LiteralPath $settingsFile).Hash
    Open-Preferences
    $locked=[IO.File]::Open($settingsFile,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
    Edit 'Dark theme base color' '#456789'
    Commit-Color
    Wait-Until {(Model).state.host_error} 'Discard fixture did not fail its write'
    Close-Preferences
    Request-CloseError
    if((Model).state.document_file.modified){throw 'Preference discard fixture has unexpected drawing changes'}
    Recovery-Choice 'Close without saving'
    Check-Exit
    $locked.Dispose();$locked=$null
    if((Get-FileHash -LiteralPath $settingsFile).Hash -ne $savedHash){throw 'Explicit discard changed the last saved preferences'}
    $invalid='{"version":999,"retain":"isolated recovery fixture"}'
    [IO.File]::WriteAllText($settingsFile,$invalid)
    Start-App
    if(!(Model).error){throw 'Unreadable settings were not reported'}
    if([IO.File]::ReadAllText($settingsFile) -ne $invalid){throw 'Loading changed the unreadable file'}
    Open-Preferences
    Edit 'Dark theme base color' '#506070'
    Commit-Color
    Wait-Until {(Saved).dark_base -eq '#506070' -and !(Model).error} 'Could not save valid preferences after an unreadable file'
    $recovery=@(Get-ChildItem -LiteralPath $settingsProfile -Filter 'settings.recovery.*.json')
    if($recovery.Count -ne 1 -or [IO.File]::ReadAllText($recovery[0].FullName) -ne $invalid){throw 'Unreadable settings were not preserved exactly'}
    Close-App

    Start-App
    $validGamma=(Saved).pressure_gamma
    Open-Preferences
    Invoke-Control 'Pen & Input'
    Edit 'Pressure response' '1 / 0'
    Close-App
    if((Saved).pressure_gamma -ne $validGamma){throw 'An invalid numeric draft replaced the valid saved setting'}
    [pscustomobject]@{
        text_draft_on_close='passed'
        numeric_draft_on_close='passed'
        invalid_numeric_draft_preserves_saved_value='passed'
        restart_restore_without_save_echo='passed'
        write_failure_preserves_previous_file='passed'
        visible_error_and_later_recovery='passed'
        canvas_continues_after_write_failure='passed'
        failed_close_keeps_preferences_and_workspace='passed'
        repeated_close_retry_and_restart='passed'
        explicit_discard_preserves_saved_file='passed'
        unreadable_file_recovery='passed'
        scope='isolated native UI Automation; not OS pen delivery or a performance benchmark'
    } | ConvertTo-Json
} catch {
    if($review){
        $review.Refresh()
        if(!$review.HasExited){try{& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure.json')}catch{}}
    }
    throw
} finally {
    if($locked){$locked.Dispose()}
    Exit-CapyEnvironment
}