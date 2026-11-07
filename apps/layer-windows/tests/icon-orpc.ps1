param([Parameter(Mandatory)][string]$Executable,[Parameter(Mandatory)][string]$Output)
$ErrorActionPreference='Stop'
if(!$env:CAPY_PRIVATE_DESKTOP){throw 'Icon ORPC regression requires the normal private test desktop.'}
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$Output=Join-Path ([IO.Path]::GetFullPath($Output)) ([guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($Output)|Out-Null
Write-Output "Icon ORPC artifacts: $Output"
foreach($theme in @('light','dark')){
    if(!(Test-Path -LiteralPath (Join-Path (Split-Path $Executable) "Assets/icons/$theme/layer-undo-symbolic.svg") -PathType Leaf)){throw 'Icon regression asset is missing.'}
}
$names=@('CAPY_ICON_ORPC_FIXTURE','CAPY_ICON_ORPC_CONTROL','CAPY_NAVIGATOR_LAYOUT','CAPY_SCROLL_INDICATORS','CAPY_FEEDBACK_SDK')
if(!('CapyIconEnvironment' -as [type])){
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyIconEnvironment {
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]
    public static extern bool SetEnvironmentVariableW(string name,string value);
}
'@
}
$environment=[Environment]::GetEnvironmentVariables('Process')
$saved=@{};foreach($name in $names){$saved[$name]=if($environment.Contains($name)){$environment[$name]}else{$null}}
try {
    foreach($name in $names){if(![CapyIconEnvironment]::SetEnvironmentVariableW($name,[NullString]::Value)){throw 'Could not clear icon fixture environment'}}
    foreach($control in @($true,$false)){
        $name=if($control){'control'}else{'icon'}
        $result=Join-Path $Output ($name+'.json')
        $env:CAPY_ICON_ORPC_FIXTURE=$result
        $env:CAPY_ICON_ORPC_CONTROL=if($control){'1'}else{$null}
        $process=Start-Process -FilePath $Executable -WorkingDirectory $Output -PassThru
        $record=[ordered]@{pid=$process.Id;start_ticks=$process.StartTime.ToUniversalTime().Ticks;executable=$Executable;case=$name;diagnostic_ceiling_seconds=300}
        $record|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $Output ($name+'-process.json'))
        Write-Output "Icon ORPC $name PID $($record.pid)"
        if(!$process.WaitForExit(300000)){
            $live=Get-Process -Id $record.pid -ErrorAction SilentlyContinue
            $same=$false
            if($live){try{$same=$live.StartTime.ToUniversalTime().Ticks -eq $record.start_ticks}catch{}}
            $deadline=[ordered]@{status='UNREACHED';pid=$record.pid;diagnostic_ceiling_seconds=300;owned_identity_matches=[bool]$same;stop_requested=$false}
            $deadlinePath=Join-Path $Output ($name+'-deadline.json')
            $deadline|ConvertTo-Json|Set-Content -LiteralPath $deadlinePath
            if($same){try{Stop-Process -Id $record.pid -ErrorAction Stop;$deadline.stop_requested=$true}finally{$deadline|ConvertTo-Json|Set-Content -LiteralPath $deadlinePath}}
            throw "Icon ORPC $name UNREACHED at its 300-second diagnostic ceiling; not a regression RED."
        }
        $process.Refresh()
        if(!(Test-Path -LiteralPath $result -PathType Leaf)){
            $record.exit_code=$process.ExitCode
            $record.status='UNREACHED'
            $record|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $Output ($name+'-process.json'))
            $nativeCode=([long]$process.ExitCode -band 0xffffffff).ToString('X8')
            throw "Icon ORPC $name UNREACHED without terminal receipt; PID $($record.pid), start $($record.start_ticks), native ExitCode $($process.ExitCode) (0x$nativeCode); not a regression RED."
        }
        $receipt=Get-Content -Raw -LiteralPath $result|ConvertFrom-Json
        if($receipt.process_id -ne $process.Id -or $receipt.control -ne $control -or !$receipt.terminal -or $receipt.calibration_callbacks -le 0 -or $process.ExitCode -ne $receipt.exit_code){throw "Icon ORPC $name failed setup or calibration; not a regression RED."}
        if('ui_outgoing_get_size_callbacks_before_close' -cnotin $receipt.PSObject.Properties.Name){throw 'Icon ORPC pre-close measurement is absent; not a regression RED.'}
        if(!$control -and ($receipt.published_sources -ne 2 -or !$receipt.pixels_match)){throw 'Icon ORPC load did not publish both exact theme sources; not a regression RED.'}
        if($control -and $receipt.ui_outgoing_get_size_callbacks_before_close -ne 0){throw 'No-icon control has outgoing UI ORPC; measurement is inconclusive.'}
        if($receipt.exit_code -eq 2){throw "Icon ORPC $name failed setup; not a regression RED."}
        if(!$control -and $receipt.ui_outgoing_get_size_callbacks_before_close -gt 0){throw 'Icon load made an outgoing ORPC call on the UI STA'}
        if($receipt.exit_code -ne 0 -or $receipt.ui_outgoing_get_size_callbacks_before_close -ne 0){throw "Icon ORPC $name failed."}
    }
    Write-Output 'Real icon load completes both theme sources without outgoing UI ORPC; calibrated channel hook and no-icon control passed'
} finally {
    foreach($name in $names){
        $restored=if($null -eq $saved[$name]){[CapyIconEnvironment]::SetEnvironmentVariableW($name,[NullString]::Value)}else{[CapyIconEnvironment]::SetEnvironmentVariableW($name,$saved[$name])}
        if(!$restored){throw 'Could not restore icon fixture environment'}
    }
}
