param([Parameter(Mandatory)][string]$Executable,[ValidateSet('light','dark')][string]$Theme='light',[string]$DebuggerPath,[switch]$ChurnOnly)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @"
using System;using System.Runtime.InteropServices;
public static class ScrollFixtureDpi {
 [DllImport("user32.dll",SetLastError=true)]public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")]public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [StructLayout(LayoutKind.Sequential)]public struct Point {public int x,y;}
 [DllImport("user32.dll")]public static extern bool ClientToScreen(IntPtr window,ref Point point);
 public static Point Origin(IntPtr window){var p=new Point();if(!ClientToScreen(window,ref p))throw new InvalidOperationException("Owned client origin unavailable");return p;}
 [StructLayout(LayoutKind.Sequential)]public struct Rect {public int left,top,right,bottom;}
 [DllImport("user32.dll")]public static extern bool GetWindowRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")]public static extern bool GetClientRect(IntPtr window,out Rect rect);
 public static Rect Client(IntPtr window){Rect value;if(!GetClientRect(window,out value))throw new InvalidOperationException("Owned client geometry unavailable");return value;}
 public static Rect Bounds(IntPtr window){Rect value;if(!GetWindowRect(window,out value))throw new InvalidOperationException("Owned window geometry unavailable");return value;}
}
"@
$dpiSet=[ScrollFixtureDpi]::SetProcessDpiAwarenessContext([IntPtr](-4));$dpiError=if($dpiSet){0}else{[Runtime.InteropServices.Marshal]::GetLastWin32Error()}
[ScrollFixtureDpi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/scroll-indicators/'+$Theme+'-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$thumbTargets=[Collections.Generic.List[object]]::new();$debugger=$null;$owned=$null;$window=[IntPtr]::Zero;$records=[Collections.Generic.List[object]]::new()
$previous=@{};foreach($key in @('CAPY_SCROLL_INDICATORS','CAPY_FEEDBACK_THEME','CAPY_STORAGE_DIR','CAPY_FIXTURE_CONTEXT')){$previous[$key]=[Environment]::GetEnvironmentVariable($key)}
function Element([string]$Id){
    foreach($handle in [CapyWindowApi]::Windows([uint32]$owned.Id)){
        $candidate=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
        $found=$candidate.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
        if($found){return $found}
    }
}
function State{$element=Element 'indicator-status';if(!$element){$owned.Refresh();throw ('Native status unavailable; alive='+!$owned.HasExited+'; title='+$owned.MainWindowTitle)};$element.Current.ItemStatus|ConvertFrom-Json}
function Check($Condition,[string]$Name,$Data){$records.Add(@{name=$Name;pass=[bool]$Condition;data=$Data});if(!$Condition){throw $Name}}
function Ready($Value){
    foreach($family in @('view','viewer')){
        $item=$Value.$family
        if(!$item.root-or$item.vertical.mode-ne2-or$item.horizontal.mode-ne2){return $false}
    };$true
}
function Assert-Ready([string]$Stage){$value=State;Check (Ready $value) ($Stage+' native MouseIndicator') $value;return $value}
function Invoke-Action([string]$Id){(Element $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Point($Bounds){
    $origin=[ScrollFixtureDpi]::Origin($window);$scale=(State).raster
    if(!$scale){throw 'Fixture rasterization scale unavailable'}
    $x=[int](($Bounds.x+$Bounds.width/2)*$scale);$y=[int](($Bounds.y+$Bounds.height/2)*$scale);$client=[ScrollFixtureDpi]::Client($window)
    if($x-lt0-or$y-lt0-or$x-ge$client.right-or$y-ge$client.bottom){throw 'Native fixture target exceeds the owned client'}
    @(($origin.x+$x),($origin.y+$y))
}
function Mouse{
    [CapyRowPointer]::Dispose();[CapyRowPointer]::Initialize([uint32]$owned.Id)
    $p=Point (State).view.bounds;[CapyRowPointer]::Hover($p[0]+15,$p[1]+15);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Hover($p[0],$p[1])
    Wait-Until {(State).mouse} 'Mouse admission did not resume'
}
function Offset([string]$Family,[double]$Offset){
    $sample=@{last=[double]::NaN;count=0}
    Wait-Until {$current=(State).$Family.offset;if($current-eq$sample.last){$sample.count++}else{$sample.count=0;$sample.last=$current};$sample.count-ge6} "$Family prior scroll animation did not settle" 8
    $id=if($Family-eq'view'){'PART_ScrollPresenter'}else{'indicator-viewer'}
    $control=Element $id;if(!$control){throw "Missing scroll provider $id"}
    $item=(State).$Family;$maximum=$item.extent-$item.viewport
    $control.GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern).SetScrollPercent(-1,100*$Offset/$maximum)
    Wait-Until {[math]::Abs((State).$Family.offset-$Offset)-lt1} "$Family offset did not converge to $Offset"
}
try{
    $env:CAPY_SCROLL_INDICATORS='1';$env:CAPY_FEEDBACK_THEME=$Theme;$env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_FIXTURE_CONTEXT=Join-Path $run 'context.json'
    $Executable=(Resolve-Path -LiteralPath $Executable).Path
    Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $run 'executed-controller.ps1')
    Get-FileHash -LiteralPath $Executable,(Join-Path (Split-Path $Executable) 'Microsoft.UI.Xaml.dll') | Select-Object Path,Hash | ConvertTo-Json | Set-Content (Join-Path $run 'identities.json')
    $owned=Start-Process -FilePath $Executable -WorkingDirectory (Split-Path $Executable) -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log');$null=$owned.Handle
    Wait-Until {foreach($handle in [CapyWindowApi]::Windows([uint32]$owned.Id)){$candidate=[System.Windows.Automation.AutomationElement]::FromHandle($handle);$probe=$candidate.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'indicator-status'));if($probe-and$probe.Current.ItemStatus){$script:window=$handle;return $true}};$false} 'Scroll fixture did not start' 30
    @{process_pm2=$dpiSet;error=$dpiError;native=[ScrollFixtureDpi]::Bounds($window);client=[ScrollFixtureDpi]::Client($window);uia=[System.Windows.Automation.AutomationElement]::FromHandle($window).Current.BoundingRectangle.ToString()}|ConvertTo-Json -Depth 4|Set-Content (Join-Path $run 'dpi.json')
    Register-CapyFixtureWindow $owned $window.ToInt64() $run
    if($DebuggerPath){$debugger=Start-CapyDebugger $owned $DebuggerPath $run ([pscustomobject]@{Handle=$window;Root=[System.Windows.Automation.AutomationElement]::FromHandle($window)})}
    [CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyWindowApi]::SetForegroundWindow($window)|Out-Null;Mouse
    Wait-Until {Ready (State)} 'Initial parent indicator owner remains'
    Start-Sleep -Milliseconds 3300
    $initial=Assert-Ready 'settled long'
    foreach($family in @('view','viewer')){Check $initial.$family.vertical.visible ($family+' mouse overflow admitted') $initial.$family}
    Invoke-Action 'indicator-undefined'
    Wait-Until {(State).undefined.requests-eq1} 'Undefined visual-state callbacks did not complete'
    $undefined=(State).undefined
    Check (!$undefined.view-and!$undefined.viewer) 'Undefined visual states return false for both native controls' $undefined
    if(!$ChurnOnly){
    foreach($device in @('pen','touch')){
        $p=Point (State).view.bounds
        [CapyRowPointer]::Down($device,$p[0],$p[1])
        try{Wait-Until {$s=State;!$s.mouse-and!$s.view.vertical.visible-and!$s.viewer.vertical.visible} "$device bars remained admitted";$null=Assert-Ready ($device+' hidden')}finally{[CapyRowPointer]::Up()}
        Mouse
    }
    foreach($family in @('view','viewer')){Offset $family 350}
    Invoke-Action 'indicator-short'
    Wait-Until {$s=State;$s.view.extent-eq400-and$s.viewer.extent-eq400-and$s.view.viewport-eq170-and$s.viewer.viewport-eq170-and$s.view.offset-ge0-and$s.view.offset-le($s.view.extent-$s.view.viewport)-and$s.viewer.offset-ge0-and$s.viewer.offset-le($s.viewer.extent-$s.viewer.viewport)} 'Shrinking range did not clamp'
    $short=Assert-Ready 'short clamp'
    Invoke-Action 'indicator-long'
    Wait-Until {(State).view.extent-gt700} 'Long range did not return'
    $restored=Assert-Ready 'long retained offset'
    foreach($family in @('view','viewer')){
        Check ([math]::Abs($restored.$family.offset-$short.$family.offset)-lt1) ($family+' growth preserves clamped offset') $restored.$family
        Check ($restored.$family.root-eq$initial.$family.root-and$restored.$family.vertical.id-eq$initial.$family.vertical.id) ($family+' retained root and controller') $restored.$family
        Offset $family 0
        $p=Point (State).$family.bounds;[CapyRowPointer]::Wheel($p[0],$p[1],-240)
        Wait-Until {(State).$family.offset-gt1} "$family wheel did not scroll"
        $null=Assert-Ready ($family+' wheel')
        Offset $family 0
        $control=Element $(if($family-eq'view'){'PART_ScrollPresenter'}else{'indicator-viewer'})
        Invoke-Action ('indicator-focus-'+$family);[CapyRowPointer]::KeyAt(0x22,$p[0],$p[1])
        Wait-Until {(State).$family.offset-gt1} "$family PageDown did not scroll"
        $null=Assert-Ready ($family+' keyboard')
        Offset $family 0
        $p=Point (State).$family.vertical.bounds;[CapyRowPointer]::Hover($p[0],$p[1]);Start-Sleep -Milliseconds 650
        $thumb=(State).$family.vertical.thumb;if(!$thumb){throw "$family native thumb not exposed"}
                $p=Point $thumb
        $hit=[System.Windows.Automation.AutomationElement]::FromPoint([System.Windows.Point]::new($p[0],$p[1]));$chain=@()
        for($node=$hit;$node;$node=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($node)){$chain+=@{name=$node.Current.Name;id=$node.Current.AutomationId;type=$node.Current.ControlType.ProgrammaticName;bounds=$node.Current.BoundingRectangle.ToString()};if($node.Current.NativeWindowHandle-eq$window.ToInt32()){break}}
        $thumbTargets.Add(@{family=$family;point=$p;thumb=$thumb;chain=$chain;before=(State).$family})
        [CapyRowPointer]::Down('mouse',$p[0],$p[1])
        try{for($i=1;$i-le10;$i++){[CapyRowPointer]::Move($p[0],$p[1]+$i*5);Start-Sleep -Milliseconds 16}}finally{[CapyRowPointer]::Up()}
        Wait-Until {(State).$family.offset-gt1} "$family thumb did not scroll"
        $thumbState=Assert-Ready ($family+' native thumb')
        Check (@($thumbState.presses|Where-Object {$_.family-eq('indicator-'+$family)-and$_.source-eq'ThumbVisual'}).Count-gt0) ($family+' native thumb press reached intended scroll control') $thumbState.presses
    }
    Invoke-Action 'indicator-fit'
    Wait-Until {$s=State;$s.view.extent-le$s.view.viewport-and$s.viewer.extent-le$s.viewer.viewport-and!$s.view.vertical.visible-and!$s.viewer.vertical.visible} 'Fitting content exposed false bars'
    $null=Assert-Ready 'content fits'
    Invoke-Action 'indicator-long';Wait-Until {(State).view.extent-gt700} 'Long content not restored'
    $final=Assert-Ready 'final long'
    Check (@($final.writes).Count-eq@($initial.writes).Count) 'No extra indicator mode writes' $final.writes
    foreach($family in @('view','viewer')){
        Check ($final.$family.root-eq$initial.$family.root-and$final.$family.vertical.id-eq$initial.$family.vertical.id-and$final.$family.horizontal.id-eq$initial.$family.horizontal.id) ($family+' final native identities retained') $final.$family
    }
    }
    Invoke-Action 'indicator-churn'
    Wait-Until {$s=State;if($s.churn.error){throw $s.churn.error};!$s.churn.running-and$s.churn.cycle-eq48} 'Bounded native scroll teardown churn did not complete' 20
    Mouse
    Wait-Until {Ready (State)} 'Replaced controls did not restore native indicator ownership' 8
    $churn=Assert-Ready 'teardown churn'
    Check ($churn.churn.reparents-eq32-and$churn.churn.replacements-eq16-and$churn.churn.transitions-ge96) 'Reparent and replacement cycles exercised native indicator states' $churn.churn
    foreach($family in @('view','viewer')){Check $churn.$family.vertical.visible ($family+' overflow remains visible after churn') $churn.$family;Offset $family 0}
    [CapyWindowApi]::PostMessage($window,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
    if(!$owned.WaitForExit(8000)){throw 'Fixture did not close normally'}
    Check ($owned.ExitCode-eq0) 'Clean exit' $owned.ExitCode
    Check ((Get-Item (Join-Path $run 'stderr.log')).Length-eq0) 'Empty stderr' $null
    @{passed=$records.Count;theme=$Theme;exe_sha256=(Get-FileHash $Executable).Hash;exit_code=$owned.ExitCode}|ConvertTo-Json|Set-Content (Join-Path $run 'result.json')
    "Scroll indicators passed $($records.Count) checks ($Theme); artifacts: $run"
}finally{
    ConvertTo-Json -InputObject @($thumbTargets.ToArray()) -Depth 20|Set-Content (Join-Path $run 'thumb-targets.json')
    ConvertTo-Json -InputObject @($records.ToArray()) -Depth 30|Set-Content (Join-Path $run 'checks.json')
    if($owned-and!$owned.HasExited){try{State|ConvertTo-Json -Depth 30|Set-Content (Join-Path $run 'failure-state.json')}catch{}}
    [CapyRowPointer]::Dispose();foreach($key in $previous.Keys){[Environment]::SetEnvironmentVariable($key,$previous[$key])}
    if($owned-and!$owned.HasExited){[CapyWindowApi]::PostMessage($window,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$owned.WaitForExit(8000)){'Forced cleanup after failure'|Set-Content (Join-Path $run 'cleanup-failure.txt');Stop-Process -Id $owned.Id}}
}
