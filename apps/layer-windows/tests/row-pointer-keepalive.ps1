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

& {
 Add-Type -TypeDefinition @'
namespace CapyEdgeContactTest {
 public static class Pointer {
  public static int Downs,Cancels,Hovers,X,Y;
  public static void Down(string device,int x,int y){Downs++;X=x;Y=y;}
  public static void Cancel(){Cancels++;}
  public static void Hover(int x,int y){Hovers++;}
 }
}
'@
 $tokens=$null;$errors=$null
 $ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-edge-reach.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $function=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Contact'},$true)
 if(!$function){throw 'Missing actual edge contact helper'}
 . ([scriptblock]::Create($function.Extent.Text.Replace('[CapyRowPointer]','[CapyEdgeContactTest.Pointer]')))
 function Screen([double]$X,[double]$Y){@{x=[int]$X;y=[int]$Y}}
 function Gesture{
  if([CapyEdgeContactTest.Pointer]::Cancels){return [pscustomobject]@{edge=$null}}
  if([CapyEdgeContactTest.Pointer]::Downs){return $after}
  $beforeReceipt
 }
 function Wait-Until([scriptblock]$Condition,[string]$Message){if(!(& $Condition)){throw $Message}}
 foreach($Device in 'mouse','pen','touch'){
  foreach($case in @(
   @{name='same pointer new Down';pointer=37;time=101;primary=$true;dx=0;accepted=$true},
   @{name='new pointer new Down';pointer=38;time=101;primary=$true;dx=0;accepted=$true},
   @{name='same pointer stale Down';pointer=37;time=100;primary=$true;dx=0;accepted=$false},
   @{name='new pointer stale Down';pointer=38;time=100;primary=$true;dx=0;accepted=$false},
   @{name='older Down';pointer=37;time=99;primary=$true;dx=0;accepted=$false},
   @{name='missing receipt';pointer=37;time=$null;primary=$true;dx=0;accepted=$false},
   @{name='nonprimary Down';pointer=37;time=101;primary=$false;dx=0;accepted=$false},
   @{name='different point';pointer=37;time=101;primary=$true;dx=1.01;accepted=$false}
  )){
   $beforeReceipt=[pscustomobject]@{edge=$null;edge_contact=[pscustomobject]@{pointer=37;time=100;primary=$true;position=@(10,20)}}
   $edge=[pscustomobject]@{contact='free';vertical=$false}
   $after=[pscustomobject]@{edge=$edge;edge_contact=[pscustomobject]@{pointer=$case.pointer;time=$case.time;primary=$case.primary;position=@((10+$case.dx),20)}}
   [CapyEdgeContactTest.Pointer]::Downs=0;[CapyEdgeContactTest.Pointer]::Cancels=0;[CapyEdgeContactTest.Pointer]::Hovers=0
   $failure=$null;$actual=$null
   try{$actual=Contact @{x=10;y=20}}catch{$failure=$_}
   if([CapyEdgeContactTest.Pointer]::Downs -ne 1 -or [CapyEdgeContactTest.Pointer]::Cancels -ne 1 -or
      [CapyEdgeContactTest.Pointer]::Hovers -ne [int]($Device -eq 'mouse') -or
      [CapyEdgeContactTest.Pointer]::X -ne 10 -or [CapyEdgeContactTest.Pointer]::Y -ne 20){throw 'Edge contact input or cleanup changed its point or repeated'}
   if($case.accepted){
    if($failure -or $actual -ne $edge){throw "Fresh Down was rejected: $Device/$($case.name): $failure"}
   }elseif(!$failure -or $failure.Exception.Message -cne 'Native contact Down was not acknowledged at the requested point'){
    throw "Invalid Down was accepted or lost its original failure: $Device/$($case.name)"
   }
  }
 }
 'Edge Down freshness follows the native receipt for mouse, pen and touch; pointer reuse accepted, stale/missing/nonprimary/wrong-point receipts rejected; input and cleanup once.'
}
