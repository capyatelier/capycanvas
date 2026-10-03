param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
& (Join-Path $PSScriptRoot 'exercise-localization.ps1') -Executable $Executable -Theme $Theme -LanguageLimit 2
