param(
 [Parameter(Mandatory)][string]$Executable,
 [Parameter(Mandatory)][string]$Project,
 [Parameter(Mandatory)][string]$OutputDirectory,
 [string]$PresentMon=(Join-Path $env:USERPROFILE '.local/tools/presentmon/2.5.1/PresentMon-2.5.1-x64.exe'),
 [int]$Diameter=18,
 [int]$Seconds=10,
 [switch]$SkipPresentMon
)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$Executable=(Resolve-Path $Executable).Path;$Project=(Resolve-Path $Project).Path
$directory=Split-Path -Parent $Executable
$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
if(Test-Path $OutputDirectory){throw 'Use a fresh result directory'}
New-Item -ItemType Directory $OutputDirectory|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_LATENCY_TRACE','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_WINDOWS_PRESENT_MODE','CAPY_WINDOWS_NO_VSYNC_WAIT')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
try{
 foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $OutputDirectory 'profile'
 $env:CAPY_LATENCY_TRACE='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $app=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $OutputDirectory 'stderr.log')
}finally{foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}}
$app.Id|Set-Content (Join-Path $OutputDirectory 'process-id.txt')
$deadline=[DateTime]::UtcNow.AddSeconds(60)
do{Start-Sleep -Milliseconds 150;$app.Refresh();if($app.HasExited){throw 'Benchmark app exited'}}while(!$app.MainWindowHandle -and [DateTime]::UtcNow -lt $deadline)
. (Join-Path $PSScriptRoot 'windows-pen-ui.ps1') -ProcessId $app.Id
Wait-Until {Test-Path (Join-Path $directory 'presentation-probe.json')} 'Renderer did not become ready' 90
Wait-Until {$m=Get-Content (Join-Path $directory 'presentation-probe.json') -Raw|ConvertFrom-Json;$m.process_id -eq $app.Id} 'Stale renderer metadata' 90
Open-Project $Project
Wait-Until {$field=Find 'tool-setting-size';$field -and $field.Current.IsEnabled} 'Project did not become editable' 90
Invoke-Id 'tool-subtool-0'
$brushName=(Find 'tool-subtool-0').Current.Name
if($brushName -notmatch 'G[- ]?Pen'){throw "Expected G-Pen, got $brushName"}
$size=Find 'tool-setting-size';$size.SetFocus();$size.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue([string]$Diameter)
(Find 'tool-setting-opacity').SetFocus();Invoke-Id 'canvas-fit'
Wait-Until {$size.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq "$Diameter.0 px"} 'Brush size was not committed'
$canvas=Find 'Drawing canvas' -Name
[PenBenchUi]::ShowWindow($app.MainWindowHandle,5)|Out-Null
[PenBenchUi]::SetForegroundWindow($app.MainWindowHandle)|Out-Null
Start-Sleep -Seconds 3
$bounds=$canvas.Current.BoundingRectangle
$cx=[int]($bounds.X+$bounds.Width/2);$cy=[int]($bounds.Y+$bounds.Height/2)
$meta=Get-Content (Join-Path $directory 'presentation-probe.json') -Raw|ConvertFrom-Json
[pscustomobject]@{process_id=$app.Id;brush_name=$brushName;diameter=$Diameter;project_sha256=(Get-FileHash $Project).Hash;exe_sha256=(Get-FileHash $Executable).Hash;dll_sha256=(Get-FileHash (Join-Path $directory 'layer_windows.dll')).Hash;surface=$meta;seconds=$Seconds;rate_hz=240;center=@($cx,$cy);radii=@(200,120);qpc_frequency=[Diagnostics.Stopwatch]::Frequency;display=(& (Join-Path $repo 'apps/layer-windows/scripts/probe-displays.ps1')|ConvertFrom-Json)}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $OutputDirectory 'capture.json')
Add-Type -Path (Join-Path $PSScriptRoot 'WindowsPenMotion.cs')
if(!$SkipPresentMon){
$pm=Start-Process -FilePath $PresentMon -ArgumentList @('--process_id',$app.Id,'--timed',($Seconds+5),'--terminate_after_timed','--no_console_stats','--no_track_input','--v1_metrics','--qpc_time_ms','--session_name',"CapyPen-$($app.Id)",'--output_file',('"'+(Join-Path $OutputDirectory 'presents.csv')+'"')) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $OutputDirectory 'presentmon.log') -RedirectStandardError (Join-Path $OutputDirectory 'presentmon-error.log')
Start-Sleep -Seconds 1
if($pm.HasExited -and $pm.ExitCode -ne 0){throw 'PresentMon capture could not start; inspect its log'}
}
[WindowsPenMotion]::Run([uint32]$app.Id,$cx,$cy,200,120,$Seconds,240,(Join-Path $OutputDirectory 'injected.csv'))
if(!$SkipPresentMon){
$pm.WaitForExit(30000)|Out-Null
if(!$pm.HasExited){throw 'PresentMon did not finish'}
}else{Start-Sleep -Seconds 2}
$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|ForEach-Object {$_.Current.Name}|Where-Object {$_ -match 'Invalid|normalized|chronological|failed|overflow|panic|unavailable|Nonfinite'}|Set-Content (Join-Path $OutputDirectory 'errors.txt')
# Request normal close; discard only the synthetic unsaved benchmark stroke.
[PenBenchUi]::PostMessage($app.MainWindowHandle,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
Wait-Until {$discard=Find "Discard Changes" -Name;$discard -or $app.HasExited} 'Missing benchmark close confirmation'
if(!$app.HasExited){$discard=Find "Discard Changes" -Name;$discard.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
$app.WaitForExit(30000)|Out-Null
if(!$app.HasExited){throw 'Benchmark app did not finish'}
$prefix='latency-'+$meta.window_id
Get-ChildItem (Join-Path $directory ($prefix+'-*'))|Copy-Item -Destination $OutputDirectory
Get-ChildItem (Join-Path $directory ('prediction-'+$app.Id+'-*.json'))|Copy-Item -Destination $OutputDirectory
if($app.ExitCode -ne 0){throw "Benchmark app exited with $($app.ExitCode)"}
if((Test-Path (Join-Path $OutputDirectory 'errors.txt')) -and (Get-Item (Join-Path $OutputDirectory 'errors.txt')).Length -gt 0){throw 'Benchmark application reported an error; inspect errors.txt'}
Write-Output "Captured $OutputDirectory"
