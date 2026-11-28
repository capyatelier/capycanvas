param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/launch-review/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$review=$null
function Drawing-Window{
    $owned=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id)
    $canvas=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'drawing-canvas')
    $cover=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'launch-splash')
    $status=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'canvas-status')
    $tabs=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'drawing-tabs')
    foreach($window in [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$owned)){
        if($window.Current.ClassName -cne 'WinUIDesktopWin32WindowClass'){continue}
        $drawing=$window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$canvas)
        if(!$drawing){continue}
        $splash=$window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$cover)
        if($splash -and !$splash.Current.IsOffscreen){continue}
        $message=$window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$status)
        $drawings=$window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$tabs)
        if(($message -and !$message.Current.IsOffscreen -and $message.Current.Name) -or
            ($drawings -and !$drawings.Current.IsOffscreen -and $drawing.Current.IsEnabled)){return $window}
    }
}
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru
    $null=$review.Handle
    Wait-Until {$script:window=Drawing-Window;$null -ne $window} 'The app did not open its drawing window.' 60
    $title=$window.Current.Name
    if(![CapyWindowApi]::PostMessage([IntPtr]$window.Current.NativeWindowHandle,0x0010,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'The app did not accept Close.'}
    if(!$review.WaitForExit(15000)){throw 'The app did not close within 15 seconds.'}
    if($review.ExitCode -ne 0){throw "The app closed with exit code $($review.ExitCode)."}
    [ordered]@{executable=$Executable;title=$title;exit_code=$review.ExitCode;scope='Launch and close only. Painting needs a hardware GPU and is checked on devices.'}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'result.json')
}finally{
    if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force;$review.WaitForExit()}
    Exit-CapyEnvironment
}
