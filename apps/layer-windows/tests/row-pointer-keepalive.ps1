param([string]$Driver=(Join-Path $PSScriptRoot '../scripts/RowPointerDriver.cs'))
$ErrorActionPreference='Stop'
Add-Type -Path $Driver
$type=[CapyRowPointer]
$flags=[Reflection.BindingFlags]'NonPublic,Static'
function Set-Contact([uint32]$Kind,[bool]$Active,[double]$Age){
    $type.GetField('failure',$flags).SetValue($null,$null)
    $type.GetField('kind',$flags).SetValue($null,$Kind)
    $type.GetField('active',$flags).SetValue($null,$Active)
    $type.GetField('lastInjection',$flags).SetValue($null,([long]([Diagnostics.Stopwatch]::GetTimestamp()-$Age*[Diagnostics.Stopwatch]::Frequency/1000)))
    $type.GetProperty('MaxGapMilliseconds').GetSetMethod($true).Invoke($null,@([double]0))|Out-Null
}
function Assert-Expired{
    $errorRecord=$null
    try{[CapyRowPointer]::Verify()}catch{$errorRecord=$_}
    if(!$errorRecord -or $errorRecord.Exception.GetBaseException() -isnot [TimeoutException] -or $errorRecord.ToString() -notmatch 'Pointer keepalive failed'){
        throw 'Verify accepted an expired synthetic contact or lost its timeout cause'
    }
}
try{
    foreach($kind in @([uint32]2,[uint32]3)){
        Set-Contact $kind $true 0
        [CapyRowPointer]::Verify()
        Set-Contact $kind $true 1000
        Assert-Expired
        $type.GetField('active',$flags).SetValue($null,$false)
        Assert-Expired
        Set-Contact $kind $true 0
        $type.GetProperty('MaxGapMilliseconds').GetSetMethod($true).Invoke($null,@([double]1000))|Out-Null
        Assert-Expired
        foreach($ending in @([uint32]0x40000,[uint32]0x40002)){
            Set-Contact $kind $true 1000
            $errorRecord=$null
            try{$type.GetMethod('InjectionAccepted',$flags).Invoke($null,@($ending,[Diagnostics.Stopwatch]::GetTimestamp()))}catch{$errorRecord=$_}
            if(!$errorRecord -or $errorRecord.Exception.GetBaseException() -isnot [TimeoutException]){throw 'Accepted Up lost the gap that expired during injection'}
            $type.GetField('active',$flags).SetValue($null,$false)
            Assert-Expired
        }
        Set-Contact $kind $true 1000
        $type.GetMethod('InjectionAccepted',$flags).Invoke($null,@([uint32]0x48000,[Diagnostics.Stopwatch]::GetTimestamp()))|Out-Null
        Set-Contact $kind $false 1000
        [CapyRowPointer]::Verify()
    }
    Set-Contact 4 $true 1000
    [CapyRowPointer]::Verify()
}finally{Set-Contact 0 $false 0}
'Synthetic contact expiry is detected and remains a Verify failure after cleanup'
