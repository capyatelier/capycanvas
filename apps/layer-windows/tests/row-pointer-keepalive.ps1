param([string]$Driver=(Join-Path $PSScriptRoot '../scripts/RowPointerDriver.cs'),[switch]$TypeSourceOnly)
$ErrorActionPreference='Stop'
$driverSource=[IO.File]::ReadAllText((Resolve-Path $Driver)).Replace('public static class CapyRowPointer {',@'
public static class CapyRowPointerMixed {
public static Point StubCursor;
public static readonly System.Collections.Generic.List<Mouse> StubMouse=new System.Collections.Generic.List<Mouse>();
public static readonly System.Collections.Generic.List<TypeInfo> StubPen=new System.Collections.Generic.List<TypeInfo>();
'@)
$imports=@{
 InitializeTouchInjection='return true;'
 InjectTouchInput='throw new InvalidOperationException("Unexpected touch injection.");'
 CreateSyntheticPointerDevice='return new IntPtr(1);'
 InjectSyntheticPointerInput='StubPen.AddRange(contacts);return true;'
 DestroySyntheticPointerDevice=''
 SendInput='foreach(var value in input){StubMouse.Add(value.mouse);if((value.mouse.flags&0x8000)!=0)StubCursor=new Point{x=value.mouse.dx,y=value.mouse.dy};}return count;'
 GetSystemMetrics='return code==78||code==79?65536:0;'
 GetCursorPos='point=StubCursor;return true;'
 GetForegroundWindow='return new IntPtr(1);'
 GetWindowThreadProcessId='process=71;return 1;'
 WindowFromPoint='return new IntPtr(1);'
}
foreach($method in $imports.Keys){
 $pattern='\[DllImport\([^\r\n]+\)\] ((?:public )?static) extern (\w+) '+$method+'\(([^;\r\n]*)\);'
 $matches=[regex]::Matches($driverSource,$pattern)
 if($matches.Count -ne 1){throw "Missing native boundary: $method"}
 $match=$matches[0]
 $driverSource=$driverSource.Replace($match.Value,($match.Groups[1].Value+' '+$match.Groups[2].Value+' '+$method+'('+$match.Groups[3].Value+'){'+$imports[$method]+'}'))
}
$edgeSource= @'
namespace CapyEdgeContactTest {
 public static class Pointer {
  public static int Downs,Cancels,Hovers,X,Y;
  public static void Down(string device,int x,int y){Downs++;X=x;Y=y;}
  public static void Cancel(){Cancels++;}
  public static void Hover(int x,int y){Hovers++;}
 }
}
'@
$mixedSource=$driverSource+"`n"+$edgeSource
if($TypeSourceOnly){return $mixedSource}
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
 Add-Type -TypeDefinition $mixedSource
 $type=[CapyRowPointerMixed]
 function Assert-Mouse([int]$X,[int]$Y){
  $point=$type.GetField('last',$flags).GetValue($null)
  if(!$type::Active -or $type.GetField('kind',$flags).GetValue($null) -ne 4 -or $point.x -ne $X -or $point.y -ne $Y){throw 'Pen hover changed the active mouse contact'}
 }
 $type::Initialize(71)
 $gate=$type.GetField('gate',$flags).GetValue($null)
 try{
  [Threading.Monitor]::Enter($gate)
  try{
   foreach($ending in 'Cancel','Up'){
    $type::Down('mouse',100,200)
    $type::PenTilt(15,-25)
    $type::PenHover(300,400)
    Assert-Mouse 100 200
    $frame=$type::StubPen[$type::StubPen.Count-1]
    if($frame.type -ne 3 -or $frame.pen.pointer.type -ne 3 -or $frame.pen.pointer.flags -ne 0x20002 -or
       $frame.pen.pointer.pixel.x -ne 300 -or $frame.pen.pointer.pixel.y -ne 400 -or $frame.pen.tiltX -ne 15 -or $frame.pen.tiltY -ne -25){throw 'Pen hover lost its own pointer kind, position or tilt'}
    $count=$type::StubPen.Count
    $type.GetMethod('Pulse',$flags).Invoke($null,@($null))|Out-Null
    Assert-Mouse 100 200
    $frame=$type::StubPen[$type::StubPen.Count-1]
    if($type::StubPen.Count -ne $count+1 -or $frame.pen.pointer.type -ne 3 -or $frame.pen.pointer.flags -ne 0x20002 -or $frame.pen.pointer.pixel.x -ne 300 -or $frame.pen.pointer.pixel.y -ne 400){throw 'Pen keepalive lost the independent hover'}
    $type::PenLeave()
    Assert-Mouse 100 200
    $frame=$type::StubPen[$type::StubPen.Count-1]
    if($frame.pen.pointer.flags -ne 0x20000 -or $frame.pen.pointer.pixel.x -ne 300 -or $frame.pen.pointer.pixel.y -ne 400){throw 'Pen leave did not retire the hover at its own point'}
    $type::Move(110,210)
    Assert-Mouse 110 210
    if($type::StubCursor.x -ne 110 -or $type::StubCursor.y -ne 210){throw 'Mouse move did not resume after pen leave'}
    $type::PenHover(320,420)
    $type::Move(120,220)
    Assert-Mouse 120 220
    $frame=$type::StubPen[$type::StubPen.Count-1]
    if($frame.pen.pointer.flags -ne 0x20000 -or $frame.pen.pointer.pixel.x -ne 320 -or $frame.pen.pointer.pixel.y -ne 420){throw 'Mouse movement retired the pen hover at the mouse point'}
    if($ending -eq 'Cancel'){$type::Cancel()}else{$type::Up()}
    if($type::Active -or $type::StubMouse[$type::StubMouse.Count-1].flags -ne 4){throw "Mouse $ending did not release its original button"}
    $type::Verify()
   }
  }finally{[Threading.Monitor]::Exit($gate)}
 }finally{$type::Dispose()}
 'Pen tilt, hover and leave preserve active mouse contact state, movement and Cancel/Up cleanup'
}

& {
 $tokens=$null;$errors=$null
 $ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-edge-reach.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 foreach($name in 'Contact','Contact-Receipt','Contact-Down'){
  $function=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name}.GetNewClosure(),$true)
  if(!$function){throw "Missing actual edge contact helper $name"}
  . ([scriptblock]::Create($function.Extent.Text.Replace('[CapyRowPointer]','[CapyEdgeContactTest.Pointer]')))
 }
 $review=@{Id=7}
 function Input-Trace($X,$Y){
  if($X-ne10-or$Y-ne20){throw 'Edge freshness queried another point'}
  @{hit=@(@{id='edge-control';process=7})}
 }
 function Wait-CapyInputTarget{throw 'UI edge freshness unexpectedly requested canvas readiness'}
 function Assert-CapyPointerDown{throw 'UI edge freshness bypassed its root contact receipt'}
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
