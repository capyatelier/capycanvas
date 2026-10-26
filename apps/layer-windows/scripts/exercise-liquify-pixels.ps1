param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light','sessions-light')][string]$Theme='dark',[switch]$SessionsOnly)
$ErrorActionPreference='Stop'
if($Theme -eq 'sessions-light'){$Theme='light';$SessionsOnly=$true}
$waitScale=$env:CAPY_WAIT_SCALE
try {
 $env:CAPY_WAIT_SCALE=$null
 $script=Join-Path $PSScriptRoot 'exercise-idle-brush.ps1'
 $parameters=@{Executable=$Executable;Theme=$Theme}
 if($SessionsOnly){$parameters.SessionsOnly=$true}else{$parameters.LiquifyOnly=$true}
 if($env:CAPY_PRIVATE_DESKTOP){& $script @parameters}
 else{& (Join-Path $PSScriptRoot 'test-desktop.ps1') -Script $script -Parameters $parameters -TimeoutSeconds 900}
} finally {
 $env:CAPY_WAIT_SCALE=$waitScale
}
