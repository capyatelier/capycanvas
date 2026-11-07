$ErrorActionPreference='Stop'
$t=$null;$e=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'icon-orpc.ps1'),[ref]$t,[ref]$e)
if($e.Count){throw ($e|Out-String)}
$branches=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.IfStatementAst] -and $node.Extent.Text -like '*UNREACHED without terminal receipt*'},$true))
if($branches.Count -ne 1){throw 'Missing-terminal branch must be unique'}
$actual=[scriptblock]::Create($branches[0].Extent.Text)
$Output=Join-Path ([IO.Path]::GetTempPath()) ('capy-icon-driver-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($Output)|Out-Null
try{
    foreach($nativeExit in @(0,-1073741189)){
        $name='control';$result=Join-Path $Output 'absent.json'
        $process=[pscustomobject]@{ExitCode=$nativeExit}
        $record=[ordered]@{pid=10164;start_ticks=[long]638980000000000001;executable='C:\owned\CapyCanvas.exe';case=$name}
        $caught=$null
        try{& $actual}catch{$caught=$_}
        $saved=Get-Content -Raw (Join-Path $Output 'control-process.json')|ConvertFrom-Json
        if(!$caught -or $caught.Exception.Message -notlike '*UNREACHED without terminal receipt*' -or $caught.Exception.Message -notlike "*native ExitCode $nativeExit*"){throw 'Missing receipt lost its native exit failure'}
        if($saved.status -cne 'UNREACHED' -or $saved.pid -ne 10164 -or $saved.start_ticks -ne $record.start_ticks -or $saved.exit_code -ne $nativeExit){throw 'Missing receipt lost exact owned process evidence'}
    }
}finally{Remove-Item -LiteralPath $Output -Recurse -Force}
Write-Output 'Actual icon driver retains owned process identity and native exit on absent terminal receipt'

$statements=$ast.EndBlock.Statements
$type=$statements|Where-Object {$_.Extent.Text -like "if(!('CapyIconEnvironment'*"}
if(@($type).Count -ne 1){throw 'Native environment binding must be unique'}
& ([scriptblock]::Create($type.Extent.Text))
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyIconEnvironmentProbe {
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]
    public static extern uint GetEnvironmentVariableW(string name,IntPtr buffer,uint size);
}
'@
$namesStatement=$statements|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -ceq '$names'}
$saveStatement=$statements|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -ceq '$saved'}
$environmentStatement=$statements|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -ceq '$environment'}
$snapshotStatement=$statements|Where-Object {$_ -is [Management.Automation.Language.ForEachStatementAst] -and $_.Extent.Text.Contains('$saved[$name]')}
$mainTry=$statements|Where-Object {$_ -is [Management.Automation.Language.TryStatementAst]}
$clearStatement=$mainTry.Body.Statements|Where-Object {$_ -is [Management.Automation.Language.ForEachStatementAst] -and $_.Extent.Text -like '*Could not clear icon fixture environment*'}
if(@($environmentStatement).Count -ne 1 -or @($namesStatement).Count -ne 1 -or @($saveStatement).Count -ne 1 -or @($snapshotStatement).Count -ne 1 -or @($clearStatement).Count -ne 1 -or @($mainTry).Count -ne 1){throw 'Actual environment lifecycle must be unique'}
$clear=[scriptblock]::Create($environmentStatement.Extent.Text+"`n"+$saveStatement.Extent.Text+"`n"+$snapshotStatement.Extent.Text+"`n"+$clearStatement.Extent.Text)
$restore=[scriptblock]::Create(($mainTry.Finally.Statements|ForEach-Object {$_.Extent.Text}) -join "`n")
. ([scriptblock]::Create($namesStatement.Extent.Text))
$initialEnvironment=[Environment]::GetEnvironmentVariables('Process')
$original=@{};foreach($name in $names){$original[$name]=if($initialEnvironment.Contains($name)){$initialEnvironment[$name]}else{$null}}
try{
    foreach($seed in @($null,'','original-value')){
        foreach($name in $names){
            $set=if($null -eq $seed){[CapyIconEnvironment]::SetEnvironmentVariableW($name,[NullString]::Value)}else{[CapyIconEnvironment]::SetEnvironmentVariableW($name,$seed)}
            if(!$set){throw 'Could not seed native environment'}
        }
        . $clear
        foreach($name in $names){
            $size=[CapyIconEnvironmentProbe]::GetEnvironmentVariableW($name,[IntPtr]::Zero,0)
            $nativeError=[Runtime.InteropServices.Marshal]::GetLastWin32Error()
            if($size -ne 0 -or $nativeError -ne 203 -or $null -ne [Environment]::GetEnvironmentVariable($name)){throw 'Actual driver did not delete native fixture flag'}
        }
        . $restore
        foreach($name in $names){
            $size=[CapyIconEnvironmentProbe]::GetEnvironmentVariableW($name,[IntPtr]::Zero,0)
            $nativeError=[Runtime.InteropServices.Marshal]::GetLastWin32Error()
            $values=[Environment]::GetEnvironmentVariables('Process');$present=$values.Contains($name);$value=$values[$name]
            if($null -eq $seed){if($size -ne 0 -or $nativeError -ne 203 -or $present -or $null -ne $value){throw 'Actual driver did not restore absent flag'}}
            elseif($size -ne $seed.Length+1 -or !$present -or $value -cne $seed){throw 'Actual driver did not restore present exact flag'}
        }
    }
}finally{
    foreach($name in $names){
        if($null -eq $original[$name]){$null=[CapyIconEnvironment]::SetEnvironmentVariableW($name,[NullString]::Value)}else{$null=[CapyIconEnvironment]::SetEnvironmentVariableW($name,$original[$name])}
    }
}
Write-Output 'Actual icon driver deletes native flags and restores absent, empty and populated values exactly'
