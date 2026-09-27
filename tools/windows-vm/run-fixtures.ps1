param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$Output,
    [string]$Name,
    [int]$TimeoutMinutes = 20
)
$ErrorActionPreference = 'Stop'
$scripts = (Resolve-Path (Join-Path $PSScriptRoot '../../apps/layer-windows/scripts')).Path
$runs = [ordered]@{ shortcuts = $null }
foreach ($script in Get-ChildItem (Join-Path $scripts 'exercise-*.ps1')) {
    $fixture = $script.BaseName -replace '^exercise-'
    $parameters = [System.Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref]$null, [ref]$null).ParamBlock.Parameters
    if ('Executable' -notin $parameters.Name.VariablePath.UserPath) { continue }
    $runs[$fixture] = @()
    foreach ($parameter in $parameters) {
        $option = $parameter.Name.VariablePath.UserPath
        if ($parameter.StaticType -eq [switch]) { $runs["${fixture}:$option"] = @("-$option") }
        $choices = ($parameter.Attributes | Where-Object { $_.TypeName.Name -eq 'ValidateSet' }).PositionalArguments.Value
        foreach ($choice in $choices | Where-Object { $_ -ne $parameter.DefaultValue.Value }) { $runs["${fixture}:$choice"] = @("-$option", $choice) }
    }
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

function Stop-Review {
    Get-Process CapyCanvas -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process CapyCanvas -ErrorAction SilentlyContinue | Wait-Process -Timeout 30
}

function Start-Shortcuts {
    $saved = @{}
    foreach ($variable in 'CAPY_SMOKE_TEST', 'CAPY_TRACE_UI', 'CAPY_SETTINGS_DIRECTORY') { $saved[$variable] = [Environment]::GetEnvironmentVariable($variable) }
    $env:CAPY_SMOKE_TEST = '1'
    $env:CAPY_TRACE_UI = '1'
    $env:CAPY_SETTINGS_DIRECTORY = Join-Path $Output 'shortcuts-profile'
    try { $review = Start-Process $Executable -WorkingDirectory (Split-Path $Executable) -PassThru } finally { foreach ($entry in $saved.GetEnumerator()) { [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value) } }
    $directory = Split-Path $Executable
    $deadline = (Get-Date).AddMinutes(2)
    while (!($state = Get-ChildItem (Join-Path $directory "ui-state-$($review.Id)-*.json") -ErrorAction SilentlyContinue | Select-Object -First 1)) {
        if ((Get-Date) -gt $deadline) { throw 'The shortcuts review did not publish its UI state.' }
        Start-Sleep -Milliseconds 250
    }
    @('-File', (Join-Path $scripts 'exercise-shortcuts.ps1'), '-ProcessId', $review.Id, '-StateFile', $state.FullName)
}

New-Item -ItemType Directory -Force $Output | Out-Null
"$($area.Width)x$($area.Height) work area at $([CapyVm.Native]::GetDpiForSystem()) dpi" | Set-Content (Join-Path $Output 'display.txt')
$env:LAYER_TEST_SOFTWARE_GPU = '1'
$env:CAPY_WAIT_SCALE = '3'
$env:NO_COLOR = '1'
$selected = if ($Name) { $Name -split ',' } else { @() }
$results = Join-Path $Output 'results.jsonl'
foreach ($run in $runs.GetEnumerator()) {
    $fixture = ($run.Key -split ':')[0]
    if ($selected -and $run.Key -notin $selected -and $fixture -notin $selected) { continue }
    Stop-Review
    $log = Join-Path $Output ($run.Key -replace ':', '-')
    $started = Get-Date
    try {
        $arguments = if ($null -eq $run.Value) { Start-Shortcuts } else { @('-File', (Join-Path $scripts "exercise-$fixture.ps1"), '-Executable', $Executable) + $run.Value }
        $process = Start-Process (Get-Process -Id $PID).Path -ArgumentList (@('-NoProfile', '-Sta') + $arguments) -NoNewWindow -PassThru -RedirectStandardOutput "$log.log" -RedirectStandardError "$log.err"
        $null = $process.Handle
        $finished = $process.WaitForExit($TimeoutMinutes * 60000)
        if (!$finished) { & taskkill.exe /T /F /PID $process.Id | Out-Null }
        $exit = if ($finished) { $process.ExitCode } else { 'timeout' }
        $errors = @(Get-Content "$log.err")
        $marker = [array]::FindLastIndex([string[]]$errors, [Predicate[string]]{ param($line) $line -match '^\s*\|\s*~' })
        $message = (@($errors | Select-Object -Skip ($marker + 1)) -match '\S' -replace '^\s*\|\s*', '') -join ' '
    } catch {
        $exit = 'runner'
        $message = $_.Exception.Message
    }
    if ($exit -ne 0) { Save-Screen "$log.png" }
    [ordered]@{ name = $run.Key; exit = $exit; seconds = [math]::Round(((Get-Date) - $started).TotalSeconds); message = $message } |
        ConvertTo-Json -Compress | Add-Content $results
}
Stop-Review
Set-Content (Join-Path $Output 'complete') ''
