param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$Output,
    [string]$Name,
    [string]$DebuggerPath,
    [ValidateSet('release','dev-perf')][string]$RustProfile='release',
    [ValidateRange(1,1440)][int]$TimeoutMinutes = 20,
    [switch]$HardwareGpu
)
$ErrorActionPreference = 'Stop'
trap {
    New-Item -ItemType Directory -Force $Output | Out-Null
    ($_ | Out-String) + $_.ScriptStackTrace | Set-Content (Join-Path $Output 'runner-error.txt')
    exit 1
}
function Write-FixtureResult([string]$Path,[string]$Json){
    $stream=[IO.File]::Open($Path,[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::Write,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $writer=[IO.StreamWriter]::new($stream,[Text.UTF8Encoding]::new($false))
    try{$null=$stream.Seek(0,[IO.SeekOrigin]::End);$writer.WriteLine($Json);$writer.Flush()}finally{$writer.Dispose();$stream.Dispose()}
}
$runnerStarted = Get-Date
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$editorExecutable=$Executable
$controlBuilds=@{}
$scripts = (Resolve-Path (Join-Path $PSScriptRoot '../../apps/layer-windows/scripts')).Path
function Get-FixtureVariantArguments([string]$Fixture,[string]$Option,[string]$Choice){
    if($Fixture -eq 'switcher-drag' -and $Option -eq 'ContextEnding'){'-ContextHoldOnly'}
    "-$Option";$Choice
}
function ConvertTo-FixtureCommandLine([string[]]$Arguments){
    (@($Arguments|ForEach-Object {'"'+($_ -replace '(\\*)"','$1$1\"' -replace '(\\+)$','$1$1')+'"'}) -join ' ')
}
$runs = [ordered]@{ shortcuts = $null }
$debuggable = @()
foreach ($script in Get-ChildItem (Join-Path $scripts 'exercise-*.ps1')) {
    $fixture = $script.BaseName -replace '^exercise-'
    $parameters = [System.Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref]$null, [ref]$null).ParamBlock.Parameters
    if ('Executable' -notin $parameters.Name.VariablePath.UserPath) { continue }
    if ('DebuggerPath' -in $parameters.Name.VariablePath.UserPath) { $debuggable += $fixture }
    $runs[$fixture] = @()
    foreach ($parameter in $parameters) {
        $option = $parameter.Name.VariablePath.UserPath
        if ($parameter.StaticType -eq [switch]) { $runs["${fixture}:$option"] = @("-$option") }
        $choices = ($parameter.Attributes | Where-Object { $_.TypeName.Name -eq 'ValidateSet' }).PositionalArguments.Value
        foreach ($choice in $choices | Where-Object { $null -ne $_ -and $_ -ne $parameter.DefaultValue.Value }) { $runs["${fixture}:$choice"] = @(Get-FixtureVariantArguments $fixture $option $choice) }
    }
}
function Select-Fixtures([string[]]$Available,[string[]]$Requested) {
    $Requested=@($Requested|ForEach-Object {if($_ -eq 'header-settings'){'header:catalog-only';'header:catalog-only-light'}else{$_}})
    $defaults = @($Requested | Where-Object { $_ -match '^[^:]+:default$' } | ForEach-Object { ($_ -split ':')[0] })
    foreach ($item in $Requested) {
        $known = if ($item -match '^([^:]+):default$') { $Matches[1] -in $Available } else {
            @($Available | Where-Object { $_ -eq $item -or ($_ -split ':')[0] -eq $item }).Count -gt 0
        }
        if (!$known) { throw "Unknown fixture: $item" }
    }
    @($Available | Where-Object { !$Requested -or $_ -in $Requested -or ($_ -split ':')[0] -in $Requested -or $_ -in $defaults })
}
function Control-Fixture([string]$Fixture,[string[]]$Arguments=@()){
    if($Fixture -eq 'layer-pickup' -and '-PendingDropOnly' -in $Arguments){return 'LayerDrop'}
    switch($Fixture){'button-feedback'{'Feedback'} 'sdk-feedback'{'Feedback'} 'scroll-indicators'{'Feedback'} 'navigator-allocation'{'Feedback'} 'popup'{'Popup'} default{$null}}
}
function Get-FixtureCompanionArguments([string]$Fixture,[string[]]$Arguments,[scriptblock]$BuildControl){
    if($Fixture -eq 'layer-pickup' -and '-PendingDropOnly' -notin $Arguments -and '-ClippedEdgesOnly' -notin $Arguments){'-DropFixture';& $BuildControl 'LayerDrop'}
}
$selected = if ($Name) { $Name -split ',' } else { @() }
$planned = @(Select-Fixtures $runs.Keys $selected)
if (!$planned.Count) { throw 'No fixtures selected.' }
function Get-FixtureTimeoutSeconds([int]$Count,[int]$Minutes){
    [int][Math]::Min(86400,[long]$Count*($Minutes*60+120))
}
if(!$env:CAPY_PRIVATE_DESKTOP){
    & (Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/test-desktop.ps1') -Script $PSCommandPath -Parameters $PSBoundParameters -TimeoutSeconds (Get-FixtureTimeoutSeconds $planned.Count $TimeoutMinutes)
    exit
}
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -Namespace CapyVm -Name Native -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
[DllImport("user32.dll")] public static extern uint GetDpiForSystem();
[DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
[DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);
[DllImport("user32.dll")] public static extern IntPtr GetSystemMenu(IntPtr window, bool revert);
[DllImport("user32.dll")] public static extern bool DeleteMenu(IntPtr menu, uint position, uint flags);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
'@
[CapyVm.Native]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
[CapyVm.Native]::SetConsoleCtrlHandler([IntPtr]::Zero, $true) | Out-Null
$console = [CapyVm.Native]::GetConsoleWindow()
[CapyVm.Native]::DeleteMenu([CapyVm.Native]::GetSystemMenu($console, $false), 0xF060, 0) | Out-Null
$area = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
[CapyVm.Native]::SetWindowPos($console, [IntPtr]::Zero, $area.Right - 640, $area.Bottom - 240, 640, 240, 0x14) | Out-Null

function Save-Screen([string]$Path) {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = [System.Drawing.Bitmap]::new($bounds.Width, $bounds.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
        $bitmap.Save($Path)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

function Reviews {
    $owners=@(foreach($file in Get-ChildItem -LiteralPath $Output -Filter '*-context.json' -File){
        $context=Get-Content -LiteralPath $file.FullName -Raw|ConvertFrom-Json
        @($context.process_id)+@($context.windows|ForEach-Object {$_.process_id})
    })|Where-Object {$_}|Select-Object -Unique
    $paths=@($editorExecutable)+@($controlBuilds.Values)
    foreach($owner in $owners){
        $process=Get-Process -Id $owner -ErrorAction SilentlyContinue
        if($process -and $process.Path -in $paths -and $process.StartTime -ge $runnerStarted){$process}
    }
}
function Stop-Review {
    $owned = @(Reviews)
    $owned | Stop-Process -Force
    $owned | Wait-Process -Timeout 30 -ErrorAction SilentlyContinue
}
function Save-Evidence([string]$Log) {
    if (!(Test-Path "$Log-context.json")) {
        if ($HardwareGpu) { throw "Missing fixture context: $Log-context.json" }
        return
    }
    $context = Get-Content "$Log-context.json" -Raw | ConvertFrom-Json
    $evidence = "$Log-evidence"
    New-Item -ItemType Directory -Force $evidence | Out-Null
    $owners = @($context.process_id) + @(Reviews | ForEach-Object Id)
    $folders = @($context.trace_directory, $context.run, (Split-Path $Executable)) | Where-Object { $_ } | Select-Object -Unique
    foreach ($folder in $folders) {
        foreach ($owner in $owners | Where-Object { $_ } | Select-Object -Unique) {
            Get-ChildItem -LiteralPath $folder -File | Where-Object {
                $_.Name -match "^(ui-state|camera-state|search-state|windows|prediction|latency)-$owner(-|\.)"
            } | Copy-Item -Destination $evidence
        }
    }
    Stop-Review
    if ($context.run -and (Test-Path -LiteralPath $context.run)) {
        & robocopy.exe $context.run $evidence /E /R:0 /W:0 /NFL /NDL /NJH /NJS | Out-Null
        if ($LASTEXITCODE -ge 8) { throw "Could not preserve fixture evidence: $($context.run)" }
    }
    if ($HardwareGpu) {
        . (Join-Path $PSScriptRoot 'test-hardware-traces.ps1')
        Assert-HardwareTraces -Directory $evidence -Output "$Log-adapters.json"
    }
}

function Start-Shortcuts {
    $saved = @{}
    foreach ($variable in 'CAPY_SMOKE_TEST', 'CAPY_TRACE_UI', 'CAPY_STORAGE_DIR') { $saved[$variable] = [Environment]::GetEnvironmentVariable($variable) }
    $env:CAPY_SMOKE_TEST = '1'
    $env:CAPY_TRACE_UI = '1'
    $env:CAPY_STORAGE_DIR = Join-Path $Output 'shortcuts-profile'
    try { $review = Start-Process $Executable -WorkingDirectory (Split-Path $Executable) -PassThru } finally { foreach ($entry in $saved.GetEnumerator()) { [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value) } }
    $directory = Split-Path $Executable
    @{process_id=$review.Id;trace_directory=$directory;profile=(Join-Path $Output 'shortcuts-profile')} | ConvertTo-Json | Set-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT
    $deadline = (Get-Date).AddMinutes(2)
    while (!($state = Get-ChildItem (Join-Path $directory "ui-state-$($review.Id)-*.json") -ErrorAction SilentlyContinue | Select-Object -First 1)) {
        if ((Get-Date) -gt $deadline) { throw 'The shortcuts review did not publish its UI state.' }
        Start-Sleep -Milliseconds 250
    }
    @('-File', (Join-Path $scripts 'exercise-shortcuts.ps1'), '-ProcessId', $review.Id, '-StateFile', $state.FullName)
}

function Build-ControlFixture([string]$controlFixture){
    if(!$controlBuilds.ContainsKey($controlFixture)){
        $directory=(Split-Path $editorExecutable)+'-'+$controlFixture
        $intermediate=(Split-Path $editorExecutable) -replace '-dev-perf$',''
        & (Join-Path $scripts 'build.ps1') -Configuration Release -RustProfile $RustProfile -ControlFixture $controlFixture -SoftwareAdapterTests:(!$HardwareGpu) -SkipRust -SkipRestore -OutputDirectory $directory -IntermediateDirectory ($intermediate+'.obj') *> (Join-Path $Output ('build-'+$controlFixture+'.log'))
        $controlBuilds[$controlFixture]=Join-Path $directory 'CapyCanvas.exe'
        @($controlBuilds.GetEnumerator()|ForEach-Object {
            $binaryDirectory=Split-Path $_.Value
            @{fixture=$_.Key;profile=$RustProfile;hardware_gpu=[bool]$HardwareGpu;source_provenance='provenance.json';files=@('CapyCanvas.exe','layer_windows.dll'|ForEach-Object {@{name=$_;sha256=(Get-FileHash -LiteralPath (Join-Path $binaryDirectory $_)).Hash}})}
        })|ConvertTo-Json -Depth 5|Set-Content (Join-Path $Output 'control-builds.json')
    }
    $controlBuilds[$controlFixture]
}

New-Item -ItemType Directory -Force $Output | Out-Null
"$($area.Width)x$($area.Height) work area at $([CapyVm.Native]::GetDpiForSystem()) dpi" | Set-Content (Join-Path $Output 'display.txt')
if ($HardwareGpu) {
    $env:LAYER_TEST_SOFTWARE_GPU = $null
    $env:CAPY_WAIT_SCALE = $null
} else {
    $env:LAYER_TEST_SOFTWARE_GPU = '1'
    $env:CAPY_WAIT_SCALE = '3'
}
$env:NO_COLOR = '1'
@{runs=$planned;timeout_minutes=$TimeoutMinutes;hardware_gpu=[bool]$HardwareGpu}|ConvertTo-Json|Set-Content (Join-Path $Output 'plan.json')
$results = Join-Path $Output 'results.jsonl'
foreach ($run in $runs.GetEnumerator()) {
    $fixture = ($run.Key -split ':')[0]
    if ($run.Key -notin $planned) { continue }
    Stop-Review
    $log = Join-Path $Output ($run.Key -replace ':', '-')
    $env:CAPY_FIXTURE_CONTEXT = "$log-context.json"
    $started = Get-Date
    try {
        $controlFixture=Control-Fixture $fixture $run.Value
        $Executable=$editorExecutable
        if($controlFixture){
            $Executable=Build-ControlFixture $controlFixture
        }
        if ($DebuggerPath -and $fixture -notin $debuggable) { throw "Fixture $fixture does not support console debugging." }
        $arguments = if ($null -eq $run.Value) { Start-Shortcuts } else { @('-File', (Join-Path $scripts "exercise-$fixture.ps1"), '-Executable', $Executable) + $run.Value }
        $arguments += @(Get-FixtureCompanionArguments $fixture $run.Value ${function:Build-ControlFixture})
        if ($DebuggerPath) { $arguments += @('-DebuggerPath', $DebuggerPath) }
        $process = Start-Process (Get-Process -Id $PID).Path -ArgumentList (ConvertTo-FixtureCommandLine (@('-NoProfile', '-Sta') + $arguments)) -NoNewWindow -PassThru -RedirectStandardOutput "$log.log" -RedirectStandardError "$log.err"
        $null = $process.Handle
        $finished = $process.WaitForExit($TimeoutMinutes * 60000)
        if (!$finished) {
            Save-Screen "$log.png"
            & taskkill.exe /T /F /PID $process.Id | Out-Null
        }
        $exit = if ($finished) { $process.ExitCode } else { 'timeout' }
        $errors = @(Get-Content "$log.err")
        $marker = [array]::FindLastIndex([string[]]$errors, [Predicate[string]]{ param($line) $line -match '^\s*\|\s*~' })
        $message = (@($errors | Select-Object -Skip ($marker + 1)) -match '\S' -replace '^\s*\|\s*', '') -join ' '
    } catch {
        $exit = 'runner'
        $message = $_.Exception.Message
    }
    try {
        if ($exit -ne 0 -and !(Test-Path "$log.png")) { Save-Screen "$log.png" }
        Save-Evidence $log
    } catch { $exit = 'evidence'; $message += " $($_.Exception.Message)" }
    $resultJson = [ordered]@{ name = $run.Key; exit = $exit; seconds = [math]::Round(((Get-Date) - $started).TotalSeconds); message = $message } |
        ConvertTo-Json -Compress
    Write-FixtureResult $results $resultJson
}
Stop-Review
Set-Content (Join-Path $Output 'complete') ''
