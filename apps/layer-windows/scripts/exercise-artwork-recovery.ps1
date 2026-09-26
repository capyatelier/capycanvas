param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/artwork-recovery/'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
$review=$null
try {
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    function Model {
        try{$s=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($s.process_id -eq $review.Id){$s.model}}catch{}
    }
    function Wait-Until([scriptblock]$Check,[string]$Message,[int]$Seconds=45){
        $watch=[Diagnostics.Stopwatch]::StartNew()
        do{if(& $Check){return};$review.Refresh();if($review.HasExited){throw "Application exited: $($review.ExitCode)"};Start-Sleep -Milliseconds 80}while($watch.Elapsed.TotalSeconds -lt $Seconds)
        throw $Message
    }
    function Find-Name([string]$Name){
        $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name))
    }
    function Invoke-Control([string]$Name){
        Wait-Until {$script:control=Find-Name $Name;$control -and $control.Current.IsEnabled} "Missing control: $Name"
        $control.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    }
    function Start-Review([string]$Label){
        $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run "$Label.stderr.log")
        Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'No review window'
        $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
        Wait-Until {$m=Model;$m.brush_ready -and $m.windows_workspace.ready} 'Canvas did not become ready' 60
        if(!(Model).windows_isolated_settings){throw 'Recovery test requires isolated settings'}
    }
    function Crash-Review {
        # Kill only the process this test launched, preserving its recovery copy.
        Stop-Process -Id $review.Id;$review.WaitForExit()
    }
    function Close-Review {
        $review.CloseMainWindow()|Out-Null
        if(!$review.WaitForExit(10000)){throw 'Clean close did not finish'}
        if($review.ExitCode -ne 0){throw "Clean close failed: $($review.ExitCode)"}
    }
    $copies=Join-Path $env:CAPY_SETTINGS_DIRECTORY 'recovery'
    Start-Review 'original'
    Invoke-Control 'Test pen'
    Wait-Until {(Model).state.document_file.modified} 'Controlled drawing did not become modified'
    Wait-Until {@(Get-ChildItem $copies -Filter '*.capy' -ErrorAction SilentlyContinue).Count -eq 1} 'No durable artwork checkpoint'
    $original=(Get-ChildItem $copies -Filter '*.capy').FullName
    $extent=@((Model).state.tabs[0].width,(Model).state.tabs[0].height)
    Crash-Review
    Start-Review 'restore'
    Wait-Until {(Model).windows_recovery.offer} 'Restart did not offer the unfinished drawing'
    Invoke-Control 'Restore drawing'
    Wait-Until {$m=Model;$m.state.document_file.modified -and !$m.windows_recovery.offer -and !$m.windows_recovery.busy} 'Restored drawing did not get a durable replacement checkpoint' 60
    if((Model).state.document_file.location){throw 'Recovery incorrectly acknowledged a manual save'}
    if(@((Model).windows_tabs.tabs).Count -ne 2){throw 'Restore did not retain the existing drawing in another tab'}
    if((@((Model).state.tabs[0].width,(Model).state.tabs[0].height) -join ',') -ne ($extent -join ',')){throw 'Recovered drawing extent changed'}
    if(Test-Path -LiteralPath $original){throw 'Original recovery copy remained after the durable replacement'}
    Crash-Review
    Start-Review 'later'
    Wait-Until {(Model).windows_recovery.offer} 'Restored checkpoint was not recoverable after a second crash'
    Invoke-Control 'Later'
    Wait-Until {!(Model).windows_recovery.offer} 'Later did not release the recovery offer'
    Close-Review
    if(@(Get-ChildItem $copies -Filter '*.capy').Count -ne 1){throw 'Later did not preserve the unadopted recovery copy'}
    Start-Review 'discard'
    Wait-Until {(Model).windows_recovery.offer} 'Later did not preserve the offer across restart'
    Invoke-Control 'Discard recovery copy'
    Wait-Until {!(Model).windows_recovery.offer -and @(Get-ChildItem $copies -Filter '*.capy').Count -eq 0} 'Explicit discard did not retire the recovery copy'
    Close-Review
    function Copies {@(Get-ChildItem $copies -Filter '*.capy' -ErrorAction SilentlyContinue).Count}
    Start-Review 'first-copy'
    Invoke-Control 'Test pen'
    Wait-Until {(Model).state.document_file.modified -and (Copies) -eq 1} 'No first unfinished copy'
    Crash-Review
    Start-Review 'second-copy'
    Wait-Until {(Model).windows_recovery.offer} 'The first unfinished copy was not offered'
    Invoke-Control 'Later'
    Wait-Until {!(Model).windows_recovery.offer} 'Later did not release the first unfinished copy'
    Invoke-Control 'Test pen'
    Wait-Until {(Model).state.document_file.modified -and (Copies) -eq 2} 'No second unfinished copy'
    Crash-Review
    Start-Review 'copies'
    Wait-Until {(Model).windows_recovery.offer} 'Restart did not offer the newest unfinished copy'
    $newest=(Model).windows_recovery.offer
    Invoke-Control 'Later'
    Wait-Until {$offer=(Model).windows_recovery.offer;$offer -and $offer -ne $newest} 'Later did not offer the next unfinished copy'
    Invoke-Control 'Discard recovery copy'
    Wait-Until {!(Model).windows_recovery.offer -and (Copies) -eq 1} 'Discarding the next copy did not keep only the copy saved for later'
    Start-Sleep -Seconds 2
    if((Model).windows_recovery.offer){throw 'A copy kept for later was offered again in the same session'}
    Close-Review
    if((Copies) -ne 1 -or !(Test-Path -LiteralPath (Join-Path $copies "$newest.capy"))){throw 'Later did not preserve the newest unfinished copy'}
    foreach($log in Get-ChildItem $run -Filter '*.stderr.log'){if($log.Length){throw "Native error in $($log.Name)"}}
    [pscustomobject]@{checkpoint_crash_restore='passed';durable_origin_retirement='passed';later_survives_clean_close='passed';explicit_discard='passed';multiple_copies_in_turn='passed';scope='isolated native Windows UI and controlled stroke replay'}|ConvertTo-Json
} finally {
    if($review){$review.Refresh();if(!$review.HasExited){Stop-Process -Id $review.Id}}
    foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}
