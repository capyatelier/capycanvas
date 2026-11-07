function Restore-CapyEnvironmentValue([string]$Name,$Value){
    if($null -eq $Value){Remove-Item -LiteralPath ('Env:'+$Name) -ErrorAction SilentlyContinue}
    else{[Environment]::SetEnvironmentVariable($Name,$Value,'Process')}
}
