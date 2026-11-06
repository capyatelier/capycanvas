$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
public static class CapyResizeNative {
 public struct Rect {public int left,top,right,bottom;}
 public struct Point {public int x,y;}
 public static uint Dpi=144,Owner=7;
 public static Rect Content=new Rect{left=59,top=49,right=1587,bottom=1437};
 public static Rect Canvas=Content;
 public static bool GetWindowRect(IntPtr window,out Rect rect){rect=new Rect{left=48,top=48,right=1598,bottom=1448};return true;}
 public static bool GetClientRect(IntPtr window,out Rect rect){rect=new Rect{right=1528,bottom=1389};return true;}
 public static bool ClientToScreen(IntPtr window,ref Point point){point=new Point{x=59,y=48};return true;}
 public static uint GetDpiForWindow(IntPtr window){return Dpi;}
 public static Rect ContentRect(IntPtr window){return Content;}
}
'@
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'UIA fixture does not parse'}
$viewport=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-CanvasViewport'},$true)
if(!$viewport){throw 'Native viewport acknowledgement helper is missing'}
$wait=$viewport.Body.Find({param($node)$node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'Wait-Until'},$true)
if(!$wait){throw 'Native viewport acknowledgement predicate is missing'}
$predicateText=$wait.CommandElements[1].Extent.Text.Replace('CapyWindowApi','CapyResizeNative')
$predicate=& ([scriptblock]::Create('{ $OuterSize=@($Width,$Height);'+$predicateText.Substring(1)))
function Read-Snapshot([string]$Path){if($Path -cne 'owned-state'){throw 'Resize read another window snapshot'};$script:resizeSnapshot|ConvertTo-Json -Depth 8 -Compress|ConvertFrom-Json}
$drawing=[pscustomobject]@{Handle=[IntPtr]1;Id=11;Root=[pscustomobject]@{}}
$drawing.Root|Add-Member -MemberType ScriptMethod -Name FindFirst -Value {
 param($Scope,$Condition)
 if($Condition.Property -ne [System.Windows.Automation.AutomationElement]::AutomationIdProperty -or $Condition.Value -cne 'drawing-canvas'){throw 'Resize must use the owned canvas automation ID'}
 $r=[CapyResizeNative]::Canvas
 [pscustomobject]@{Current=[pscustomobject]@{ProcessId=[CapyResizeNative]::Owner;BoundingRectangle=[pscustomobject]@{Left=$r.left;Top=$r.top;Right=$r.right;Bottom=$r.bottom;Width=$r.right-$r.left;Height=$r.bottom-$r.top}}}
}
$handle=[IntPtr]1;$ProcessId=7;$Process=[pscustomobject]@{Id=7};$registered=@([pscustomobject]@{id=11});$statePath='owned-state';$Width=1550;$Height=1400
function Set-ResizeSnapshot([uint32]$Dpi,[int]$CanvasWidth=1528,[int]$CanvasHeight=1388){
 [CapyResizeNative]::Dpi=$Dpi
 $scale=$Dpi/96.
 $script:resizeSnapshot=[pscustomobject]@{process_id=7;window_id=11;model=[pscustomobject]@{windows_isolated_settings=$true;layout=[pscustomobject]@{viewport=@([double][single]($CanvasWidth/$scale),[double][single]($CanvasHeight/$scale))};state=[pscustomobject]@{camera=[pscustomobject]@{viewport=@($CanvasWidth,$CanvasHeight)}}}}
}
foreach($dpi in 144,96,192){
 Set-ResizeSnapshot $dpi
 if(!(& $predicate)){throw "The native XAML canvas with its reserved client border was not acknowledged at DPI $dpi"}
 $resizeSnapshot.model.state.camera.viewport[1]--
 if(& $predicate){throw 'Resize accepted a camera one physical pixel shorter than the arranged canvas'}
 Set-ResizeSnapshot $dpi
 $resizeSnapshot.model.layout.viewport[1]=[single](1387/($dpi/96.))
 if(& $predicate){throw 'Resize accepted a logical viewport for the previous physical canvas size'}
 Set-ResizeSnapshot $dpi
 $resizeSnapshot.model.layout.viewport[1]=[single](1388.25/($dpi/96.))
 if(& $predicate){throw 'Resize accepted an inconsistent logical viewport inside the same physical pixel'}
 Set-ResizeSnapshot $dpi
 $r=[CapyResizeNative]::Canvas;$r.bottom--;[CapyResizeNative]::Canvas=$r
 Set-ResizeSnapshot $dpi 1528 1387
 if(& $predicate){throw 'Resize accepted a real uncovered bottom row inside the XAML content'}
 [CapyResizeNative]::Canvas=[CapyResizeNative]::Content
 Set-ResizeSnapshot $dpi
}
$r=[CapyResizeNative]::Canvas;$r.top++;[CapyResizeNative]::Canvas=$r
Set-ResizeSnapshot 144 1528 1387
if(& $predicate){throw 'Resize accepted a real uncovered top row inside the XAML content'}
[CapyResizeNative]::Canvas=[CapyResizeNative]::Content
Set-ResizeSnapshot 144
$r=[CapyResizeNative]::Content;$r.right--;[CapyResizeNative]::Content=$r;[CapyResizeNative]::Canvas=$r
Set-ResizeSnapshot 144 1527 1388
if(& $predicate){throw 'Resize accepted old XAML content bounds after the native window changed'}
$r.right++;[CapyResizeNative]::Content=$r;[CapyResizeNative]::Canvas=$r
Set-ResizeSnapshot 144
$Height--
if(& $predicate){throw 'Resize accepted another requested outer size'}
$Height++
$resizeSnapshot.model.windows_isolated_settings=$false
if(& $predicate){throw 'Resize accepted a nonisolated snapshot'}
Set-ResizeSnapshot 144
$resizeSnapshot.process_id=8
if(& $predicate){throw 'Resize accepted another process snapshot'}
Set-ResizeSnapshot 144
$resizeSnapshot.window_id=12
if(& $predicate){throw 'Resize accepted another window snapshot'}
Set-ResizeSnapshot 144
[CapyResizeNative]::Owner=8
if(& $predicate){throw 'Resize accepted another process canvas'}
'Native resize acknowledgement geometry and ownership: passed'
