$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$path=Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'
$ast=[Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
namespace CapyOwnedSearchTest {
 public enum TreeScope{Children,Descendants}
 public static class ControlType{public static object Menu="menu",MenuItem="menu-item";}
 public class Condition{public virtual bool Matches(Node n){return true;}}
 public class PropertyCondition:Condition {
  string property;object value;public PropertyCondition(object p,object v){property=(string)p;value=v;}
  public override bool Matches(Node n){return Object.Equals(property=="pid"?(object)n.ProcessId:property=="id"?n.AutomationId:property=="scroll_available"?(object)(n.Provider!=null):property=="type"?n.ControlType:n.Name,value);}
 }
 public class AndCondition:Condition {
  Condition[] values;public AndCondition(params Condition[] v){values=v;}
  public override bool Matches(Node n){foreach(var v in values)if(!v.Matches(n))return false;return true;}
 }
 public struct Rect {
  public int X,Y,Width,Height;public bool IsEmpty{get{return Width==0||Height==0;}}public int Top{get{return Y;}}
  public Rect(int x,int y){X=x;Y=y;Width=1414;Height=986;}
  public Rect(int x,int y,int width,int height){X=x;Y=y;Width=width;Height=height;}
  public bool Contains(Rect other){return !other.IsEmpty&&other.X>=X&&other.Y>=Y&&other.X+other.Width<=X+Width&&other.Y+other.Height<=Y+Height;}
  public void Intersect(Rect other){int right=Math.Min(X+Width,other.X+other.Width),bottom=Math.Min(Y+Height,other.Y+other.Height);X=Math.Max(X,other.X);Y=Math.Max(Y,other.Y);Width=Math.Max(0,right-X);Height=Math.Max(0,bottom-Y);}
  public void Inflate(int x,int y){X-=x;Y-=y;Width+=2*x;Height+=2*y;}
 }
 public enum ScrollAmount{NoAmount,SmallDecrement,SmallIncrement}
 public static class ScrollPattern{public static object Pattern="scroll";public static double NoScroll=-1;}
 public class Scroll {
  public double VerticalScrollPercent;public int Resets,Steps;public bool VerticallyScrollable=true;public Scroll Current{get{return this;}}
  public void SetScrollPercent(double horizontal,double vertical){VerticalScrollPercent=vertical;Resets++;}
  public void ScrollBy(ScrollAmount horizontal,ScrollAmount vertical){VerticalScrollPercent=Math.Max(0,Math.Min(100,VerticalScrollPercent+(vertical==ScrollAmount.SmallDecrement?-20:20)));Steps++;}
 }
 public class Pattern {
  public Scroll Value;public Pattern(Scroll value){Value=value;}public Scroll Current{get{return Value;}}
  public void SetScrollPercent(double horizontal,double vertical){Value.SetScrollPercent(horizontal,vertical);}
  public void Scroll(ScrollAmount horizontal,ScrollAmount vertical){Value.ScrollBy(horizontal,vertical);}
 }
 public class TreeWalker{public static TreeWalker ControlViewWalker=new TreeWalker();public Node GetParent(Node node){return node.Parent;}}
 public static class Driver {
  public static IntPtr Foreground;public static int Activations;
  public static int GetDpiForWindow(IntPtr window){return 96;}
  public static bool SetForegroundWindow(IntPtr window){Foreground=window;Activations++;return true;}
  public static IntPtr GetForegroundWindow(){return Foreground;}
 }
 public class Node {
  public string AutomationId,Name;public object ControlType="control";public int ProcessId;bool offscreen;Rect bounds;public Node Parent;public Scroll Provider,TargetScroll;public int? ContentY;
  public bool HasKeyboardFocus;public IntPtr NativeWindowHandle=new IntPtr(1);
  public Rect BoundingRectangle{get{if(ContentY.HasValue){int y=ContentY.Value-(int)(TargetScroll.VerticalScrollPercent*2);return y>=100||y+20<=0?new Rect():new Rect(0,y,80,20);}return bounds;}set{bounds=value;}}
  public bool IsOffscreen{get{return ContentY.HasValue?BoundingRectangle.IsEmpty:offscreen;}set{offscreen=value;}}
  public void SetFocus(){HasKeyboardFocus=true;}
  public bool TryGetCurrentPattern(object pattern,out object result){result=Provider==null?null:new Pattern(Provider);return result!=null;}
  public object GetCurrentPattern(object pattern){object result;if(!TryGetCurrentPattern(pattern,out result))throw new InvalidOperationException("Missing pattern");return result;}
  public Node Current{get{return this;}}public List<Node> Children=new List<Node>();
  public Node(string id,int pid,int x,int y){AutomationId=id;Name=id;ProcessId=pid;BoundingRectangle=new Rect(x,y);}
  public Node[] FindAll(TreeScope scope,Condition condition){if(AutomationId=="desktop")throw new InvalidOperationException("Desktop search is forbidden");var found=new List<Node>();foreach(var child in Children){if(condition.Matches(child))found.Add(child);if(scope==TreeScope.Descendants)found.AddRange(child.FindAll(scope,condition));}return found.ToArray();}
  public Node FindFirst(TreeScope scope,Condition condition){var nodes=FindAll(scope,condition);return nodes.Length==0?null:nodes[0];}
 }
 public static class AutomationElement {
  public static object AutomationIdProperty="id",NameProperty="name",ControlTypeProperty="type",ProcessIdProperty="pid",IsScrollPatternAvailableProperty="scroll_available";
  public static Node RootElement=new Node("desktop",0,0,0);
  public static Node FromHandle(IntPtr window){return RootElement.Children.Find(node=>node.NativeWindowHandle==window);}
 }
 public class ElementNotAvailableException:Exception{}
 public class Review{public int Id=33;public IntPtr MainWindowHandle=new IntPtr(132146);public bool HasExited=false;public int ExitCode=0;public void Refresh(){}}
}
'@
foreach($name in @('Test-CapySearchItem','Find','Control','Wait-Until')){
    $fn=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name}.GetNewClosure(),$true))
    if($fn.Count -ne 1){throw 'Missing actual UIA helper'}
    . ([scriptblock]::Create($fn[0].Extent.Text.Replace('System.Windows.Automation.','CapyOwnedSearchTest.')))
}
$review=[CapyOwnedSearchTest.Review]::new();$root=[CapyOwnedSearchTest.Node]::new('first-window',33,209,197)
$second=[CapyOwnedSearchTest.Node]::new('second-window',33,356,344);$popup=[CapyOwnedSearchTest.Node]::new('popup',33,0,0)
$other=[CapyOwnedSearchTest.Node]::new('drawing-canvas',33,356,344);$second.Children.Add($other)
$popup.ControlType=[CapyOwnedSearchTest.ControlType]::Menu;$popup.Parent=$root
$menu=[CapyOwnedSearchTest.Node]::new('color-swatch-edit',33,10,10);$menu.Parent=$popup;$popup.Children.Add($menu)
foreach($window in @($root,$second)){[CapyOwnedSearchTest.AutomationElement]::RootElement.Children.Add($window)}
function Owned-ProcessWindows($Process){foreach($window in @($root,$second)){[pscustomobject]@{Root=$window;Handle=$window.NativeWindowHandle}}}
$CapyPopups=$true;$CapyFind='first';$CapyWaitSeconds=1;$CapyEach=$null
if((Find 'drawing-canvas' -Visible) -ne $other){throw 'Default owned-process fallback changed'}
foreach($offscreen in @($false,$true)){
    $root.Children.Clear()
    if($offscreen){$hidden=[CapyOwnedSearchTest.Node]::new('drawing-canvas',33,209,197);$hidden.IsOffscreen=$true;$root.Children.Add($hidden)}
    if(Find 'drawing-canvas' -Visible -NoPopups){throw 'Root-only search returned a missing/offscreen or second-window canvas'}
    $rejected=$false;try{Control 'drawing-canvas' -Arranged -NoPopups -Seconds 0|Out-Null}catch{if($_.Exception.Message -notlike 'Missing control:*'){throw};$rejected=$true}
    if(!$rejected){throw 'Root-only arranged lookup accepted a second-window canvas'}
}
$root.Children.Clear();$root.Children.Add([CapyOwnedSearchTest.Node]::new('drawing-canvas',99,209,197))
if(Find 'drawing-canvas' -Visible -NoPopups){throw 'Root-only search accepted a foreign-process peer'}
$root.Children.Clear();$first=[CapyOwnedSearchTest.Node]::new('drawing-canvas',33,209,197);$script:ticks=0
$CapyEach={$script:ticks++;if($script:ticks -eq 3){$root.Children.Add($first)}}
$selected=Control 'drawing-canvas' -Arranged -NoPopups
$CapyEach=$null
if($selected -ne $first -or $selected.Current.BoundingRectangle.X -ne 209 -or $selected.Current.BoundingRectangle.Y -ne 197 -or $script:ticks -lt 5){throw 'Root-only search did not wait for the independently known current-root bounds'}
$root.Children.Add($popup)
if((Control 'color-swatch-edit' -Arranged) -ne $menu){throw 'Default context-menu popup lookup changed'}
$bottom=$root.Current.BoundingRectangle.Y+$root.Current.BoundingRectangle.Height
$nativeMenu=[CapyOwnedSearchTest.Node]::new('native-menu',33,250,$bottom-5);$nativeMenu.ControlType=[CapyOwnedSearchTest.ControlType]::Menu;$nativeMenu.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(250,$bottom-5,424,163);$nativeMenu.Parent=$root;$root.Children.Add($nativeMenu)
$outside=[CapyOwnedSearchTest.Node]::new('outside-owner-item',33,262,$bottom+20);$outside.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(262,$bottom+20,408,45);$outside.Parent=$nativeMenu;$nativeMenu.Children.Add($outside)
if((Find 'outside-owner-item' -Visible) -ne $outside){throw 'Native menu item outside the document lost its actual popup surface'}
if(Find 'outside-owner-item' -Visible -NoPopups){throw 'Root-only search accepted a popup outside document bounds'}
$outside.IsOffscreen=$true
if(Find 'outside-owner-item' -Visible){throw 'Popup search accepted an offscreen menu item'}
$outside.IsOffscreen=$false;$nativeMenu.IsOffscreen=$true
if(Find 'outside-owner-item' -Visible){throw 'Popup search accepted an offscreen menu surface'}
$nativeMenu.IsOffscreen=$false;$outside.ProcessId=99
if(Find 'outside-owner-item' -Visible){throw 'Popup search accepted a foreign-process menu item'}
$outside.ProcessId=33;$outside.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(262,$bottom+200,408,45)
if(Find 'outside-owner-item' -Visible){throw 'Popup search accepted an item beyond its menu bounds'}
$detached=[CapyOwnedSearchTest.Node]::new('detached-popup',33,2500,0);$detached.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(2500,0,200,200);$detached.ControlType=[CapyOwnedSearchTest.ControlType]::Menu;$detached.Parent=$root;$root.Children.Add($detached)
$detachedItem=[CapyOwnedSearchTest.Node]::new('detached-item',33,2510,10);$detachedItem.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(2510,10,50,40);$detachedItem.Parent=$detached;$detached.Children.Add($detachedItem)
if((Find 'detached-item' -Visible) -ne $detachedItem){throw 'Detached owned popup was clipped against document bounds'}
if(Find 'detached-item' -Visible -NoPopups){throw 'Root-only search escaped into a detached popup'}
$unregistered=[CapyOwnedSearchTest.Node]::new('unregistered-window',33,2500,0)
$unregisteredItem=[CapyOwnedSearchTest.Node]::new('unregistered-item',33,2510,10);$unregisteredItem.Parent=$unregistered;$unregistered.Children.Add($unregisteredItem);[CapyOwnedSearchTest.AutomationElement]::RootElement.Children.Add($unregistered)
if(Find 'unregistered-item' -Visible){throw 'Unregistered owned window was discovered'}
$secondList=[CapyOwnedSearchTest.Node]::new('layer-list',33,356,344);$secondRow=[CapyOwnedSearchTest.Node]::new('layer-7-content',33,366,354);$secondList.Children.Add($secondRow);$second.Children.Add($secondList)
foreach($offscreen in @($false,$true)){
    $root.Children.Clear()
    if($offscreen){$hiddenList=[CapyOwnedSearchTest.Node]::new('layer-list',33,209,197);$hiddenList.IsOffscreen=$true;$root.Children.Add($hiddenList)}
    if(Find 'layer-7-content' -Visible -NoPopups){throw 'Root-only layer row escaped through recursive layer-list lookup'}
}
$root.Children.Clear();$firstList=[CapyOwnedSearchTest.Node]::new('layer-list',33,209,197);$firstRow=[CapyOwnedSearchTest.Node]::new('layer-7-content',33,219,207);$firstList.Children.Add($firstRow);$root.Children.Add($firstList)
if((Control 'layer-7-content' -Arranged -NoPopups) -ne $firstRow -or $firstRow.Current.BoundingRectangle.X -ne 219 -or $firstRow.Current.BoundingRectangle.Y -ne 207){throw 'Root-only layer row did not retain current-root bounds'}
Write-Output 'Owned UIA search passed: actual Find/Control AST waits for current-root canvas, excludes second/foreign windows and preserves registered menu roots; no UI or GPU.'

$CapyFind='prefer-visible'
foreach($bounds in @([CapyOwnedSearchTest.Rect]::new(0,0,0,0),[CapyOwnedSearchTest.Rect]::new(-100000,197))){
    $root.Children.Clear()
    $measurement=[CapyOwnedSearchTest.Node]::new('navigator-overview',33,209,197);$measurement.BoundingRectangle=$bounds;$root.Children.Add($measurement)
    $arranged=[CapyOwnedSearchTest.Node]::new('navigator-overview',33,219,207);$root.Children.Add($arranged)
    if((Find 'navigator-overview' -Visible -NoPopups) -ne $arranged -or (Control 'navigator-overview' -Arranged -NoPopups) -ne $arranged){throw 'UIA selected an unarranged measurement peer reported as onscreen'}
}
Write-Output 'Owned UIA search rejects empty and off-window measurement peers even when IsOffscreen is false.'

$reveal=@($ast.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Reveal'},$true))
if($reveal.Count -ne 1){throw 'Missing actual Reveal helper'}
$scrollPattern=@($ast.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Scroll-Pattern'},$true))
if($scrollPattern.Count -ne 1){throw 'Missing actual Scroll-Pattern helper'}
. ([scriptblock]::Create($scrollPattern[0].Extent.Text.Replace('System.Windows.Automation.','CapyOwnedSearchTest.').Replace('[CapyRowPointer]','[CapyOwnedSearchTest.Driver]')))
. ([scriptblock]::Create($reveal[0].Extent.Text.Replace('System.Windows.Automation.','CapyOwnedSearchTest.').Replace('[CapyRowPointer]','[CapyOwnedSearchTest.Driver]')))
function Native-Bounds([string]$Id,[double]$Scale){[CapyOwnedSearchTest.Rect]::new(0,0,100,100)}
foreach($case in @(
    @{name='empty above at bottom';position=10;percent=100;resets=1},
    @{name='empty below at top';position=270;percent=0;resets=0},
    @{name='already visible';position=40;percent=0;resets=0;steps=0},
    @{name='partially above';position=30;percent=20;resets=0},
    @{name='partially below';position=90;percent=0;resets=0},
    @{name='absent target';position=10000;percent=100;resets=1;reject=$true}
)){
    $root=[CapyOwnedSearchTest.Node]::new('root',33,0,0);$root.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100)
    $group=[CapyOwnedSearchTest.Node]::new('workspace-group-24',33,0,0);$group.Parent=$root;$root.Children.Add($group)
    $provider=[CapyOwnedSearchTest.Node]::new('PART_ScrollPresenter',33,0,0);$provider.Parent=$group;$group.Children.Add($provider)
    $provider.Provider=[CapyOwnedSearchTest.Scroll]::new();$provider.Provider.VerticalScrollPercent=$case.percent
    $item=[CapyOwnedSearchTest.Node]::new('target',33,0,0);$item.Parent=$provider;$item.TargetScroll=$provider.Provider;$item.ContentY=$case.position;$provider.Children.Add($item)
    $failure=$null;$actual=$null
    try{$actual=Reveal 'target'}catch{$failure=$_}
    if($provider.Provider.Resets -ne $case.resets){throw "Reveal reset its provider incorrectly: $($case.name)"}
    if($case.reject){if(!$failure -or $actual){throw 'Reveal accepted an absent target'};continue}
    if($failure){throw $failure}
    if($actual -ne $item -or $actual.Current.IsOffscreen -or !([CapyOwnedSearchTest.Rect]::new(0,0,100,100)).Contains($actual.Current.BoundingRectangle)){throw "Reveal accepted a clipped target: $($case.name)"}
    if($case.ContainsKey('steps') -and $provider.Provider.Steps -ne $case.steps){throw 'Reveal scrolled an already visible target'}
    if($item.HasKeyboardFocus){throw 'Reveal changed focus without its Focus switch'}
}
Write-Output 'Actual Reveal passed: empty targets above/below, visible/partial bounds and absent rejection; at most one real-provider reset and unchanged focus.'

$root=[CapyOwnedSearchTest.Node]::new('root',33,0,0);$root.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100)
$parked=[CapyOwnedSearchTest.Node]::new('target',33,-100000,0);$parked.Parent=$root;$root.Children.Add($parked)
$viewport=[CapyOwnedSearchTest.Node]::new('panel-body-scroll-24',33,0,0);$viewport.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100)
$viewport.Parent=$root;$viewport.Provider=[CapyOwnedSearchTest.Scroll]::new();$root.Children.Add($viewport)
$appearing=[CapyOwnedSearchTest.Node]::new('target',33,0,0);$appearing.Parent=$viewport;$appearing.TargetScroll=$viewport.Provider;$appearing.ContentY=160
$CapyEach={if($viewport.Provider.VerticalScrollPercent -ge 40 -and !$viewport.Children.Contains($appearing)){$viewport.Children.Add($appearing)}}
try{$revealed=Reveal 'target' -ViewportId 'panel-body-scroll-24'}finally{$CapyEach=$null}
if($revealed -ne $appearing -or $viewport.Provider.Steps -ne 2 -or $appearing.Current.IsOffscreen -or $parked.HasKeyboardFocus){throw 'Owned viewport did not reveal an unrealized target while rejecting a parked duplicate'}
$viewport.Children.Clear();$viewport.Provider=[CapyOwnedSearchTest.Scroll]::new()
$rejected=$false
try{Reveal 'target' -ViewportId 'panel-body-scroll-24'|Out-Null}catch{if($_.Exception.Message -notlike '*did not scroll into view*' -and $_.Exception.Message -notlike '*stayed clipped*'){throw};$rejected=$true}
if(!$rejected){throw 'Owned viewport reveal accepted a parked target outside its subtree'}
Write-Output 'Owned viewport Reveal passed: unrealized target appeared after native scrolling, parked duplicate remained excluded, and an absent live target was rejected.'

$root=[CapyOwnedSearchTest.Node]::new('root',33,0,0);$root.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100)
$parked=[CapyOwnedSearchTest.Node]::new('target',33,-100000,0);$parked.Parent=$root;$root.Children.Add($parked)
$viewport=[CapyOwnedSearchTest.Node]::new('panel-body-scroll-24',33,0,0);$viewport.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100);$viewport.Parent=$root;$root.Children.Add($viewport)
$visible=[CapyOwnedSearchTest.Node]::new('target',33,0,0);$visible.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,20,80,20);$visible.Parent=$viewport;$viewport.Children.Add($visible)
if((Reveal 'target' -ViewportId 'panel-body-scroll-24') -ne $visible){throw 'Owned viewport required a scroll provider for an already visible target'}
Write-Output 'Owned viewport Reveal accepts an already visible live target without a native scroll provider or parked duplicate.'

$root=[CapyOwnedSearchTest.Node]::new('root',33,0,0);$root.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100)
$group=[CapyOwnedSearchTest.Node]::new('workspace-group-24',33,0,0);$group.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100);$group.Parent=$root;$root.Children.Add($group)
$wrong=[CapyOwnedSearchTest.Node]::new('PART_ScrollPresenter',33,50,50);$wrong.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(50,50,40,40);$wrong.Provider=[CapyOwnedSearchTest.Scroll]::new();$wrong.Parent=$group;$group.Children.Add($wrong)
$provider=[CapyOwnedSearchTest.Node]::new('PART_ScrollPresenter',33,0,0);$provider.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100);$provider.Provider=[CapyOwnedSearchTest.Scroll]::new();$provider.Parent=$group;$group.Children.Add($provider)
$parked=[CapyOwnedSearchTest.Node]::new('target',33,-100000,0);$parked.Parent=$root;$root.Children.Add($parked)
$appearing=[CapyOwnedSearchTest.Node]::new('target',33,0,0);$appearing.Parent=$provider;$appearing.TargetScroll=$provider.Provider;$appearing.ContentY=160
$CapyEach={if($provider.Provider.VerticalScrollPercent -ge 40 -and !$provider.Children.Contains($appearing)){$provider.Children.Add($appearing)}}
try{$revealed=Reveal 'target' -ViewportId 'panel-body-scroll-24'}finally{$CapyEach=$null}
if($revealed -ne $appearing -or $provider.Provider.Steps -ne 2 -or $wrong.Provider.Steps -ne 0){throw 'Group fallback did not use the native-matched owned scroll provider'}
Write-Output 'Owned group Reveal passed: missing ScrollView peer, matched native viewport provider, and excluded sibling and parked peers.'
$provider.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(50,50,40,40);$provider.Provider=[CapyOwnedSearchTest.Scroll]::new();$appearing.TargetScroll=$provider.Provider;$script:retryTicks=0
$CapyEach={
 $script:retryTicks++
 if($script:retryTicks -eq 5){$provider.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100)}
}
try{$revealed=Reveal 'target' -ViewportId 'panel-body-scroll-24'}finally{$CapyEach=$null}
if($revealed -ne $appearing -or $provider.Provider.Steps -ne 2 -or $script:retryTicks -lt 5){throw 'Owned viewport did not wait for its native-matched scroll provider'}
Write-Output 'Owned Reveal waits for a native-matched scroll provider before scrolling an unrealized target.'
$provider.Provider=[CapyOwnedSearchTest.Scroll]::new();$provider.Provider.VerticallyScrollable=$false
$rejected=$false
try{Reveal 'missing-target' -ViewportId 'panel-body-scroll-24'|Out-Null}catch{if($_.Exception.Message -ne 'missing-target has no vertically scrollable native provider'){throw};$rejected=$true}
if(!$rejected -or $provider.Provider.Steps -ne 0){throw 'Reveal tried to scroll a native provider without vertical extent'}
Write-Output 'Owned Reveal rejects a native provider without vertical extent before calling Scroll.'

$paletteAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-palettes.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$menuItems=@($paletteAst.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Menu-Items'},$true))
if($menuItems.Count -ne 1){throw 'Missing actual palette menu census'}
. ([scriptblock]::Create($menuItems[0].Extent.Text.Replace('System.Windows.Automation.','CapyOwnedSearchTest.')))
$ControlType=[CapyOwnedSearchTest.ControlType]
[CapyOwnedSearchTest.AutomationElement]::RootElement.Children.Clear()
$root=[CapyOwnedSearchTest.Node]::new('registered-palette-window',33,0,0);$root.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100)
[CapyOwnedSearchTest.AutomationElement]::RootElement.Children.Add($root)
$surface=[CapyOwnedSearchTest.Node]::new('menu-surface',33,250,250);$surface.ControlType=$ControlType::Menu
$surface.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(250,250,100,100);$surface.Parent=$root;$root.Children.Add($surface)
$visible=[CapyOwnedSearchTest.Node]::new('visible-menu-item',33,260,260);$visible.ControlType=$ControlType::MenuItem
$visible.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(260,260,80,26);$visible.Parent=$surface;$surface.Children.Add($visible)
$empty=[CapyOwnedSearchTest.Node]::new('unarranged-menu-item',33,0,0);$empty.ControlType=$ControlType::MenuItem
$empty.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,0,0);$empty.Parent=$surface;$surface.Children.Add($empty)
if(@(Menu-Items).Count -ne 1 -or @(Menu-Items)[0] -ne $visible){throw 'Palette menu census accepted an unarranged peer reported as onscreen'}
$surface.IsOffscreen=$true
if(@(Menu-Items).Count){throw 'Palette menu census accepted peers on a hidden menu surface'}
$surface.IsOffscreen=$false;$visible.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(1000,1000,80,26)
if(@(Menu-Items).Count){throw 'Palette menu census accepted peers outside their menu surface'}
Write-Output 'Palette menu census retains visible owned items and rejects unarranged, hidden-surface and out-of-surface peers.'

$persistenceAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-persistence.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$launch=$persistenceAst.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Launch'},$true)
$statements=@($launch.Body.EndBlock.Statements)
$ready=@(for($i=0;$i -lt $statements.Count;$i++){if($statements[$i].Extent.Text.Contains("'Canvas did not start'")){$i}})
if($ready.Count -ne 1 -or $statements[$ready[0]+1].Left.Extent.Text -ne '$script:root'){throw 'Missing actual persistence readiness and root assignment'}
& {
    $review=[CapyOwnedSearchTest.Review]::new()
    $overlay=[CapyOwnedSearchTest.Node]::new('input-overlay',33,0,0);$overlay.NativeWindowHandle=$review.MainWindowHandle
    $drawing=[CapyOwnedSearchTest.Node]::new('drawing-window',33,48,48);$drawing.NativeWindowHandle=[IntPtr]131956
    [CapyOwnedSearchTest.AutomationElement]::RootElement.Children.Clear()
    foreach($window in @($overlay,$drawing)){[CapyOwnedSearchTest.AutomationElement]::RootElement.Children.Add($window)}
    $script:root=$overlay;$script:persistenceResolutions=0;$script:persistenceWaitSeconds=$null;$script:persistenceInsideWait=$false;$actualWait=${function:Wait-Until}
    function Model{[pscustomobject]@{brush_ready=$true}}
    function Owned-DrawingWindow($Process){if($Process -ne $review -or !$script:persistenceInsideWait){throw 'Persistence resolved outside its readiness wait or for a different process'};$script:persistenceResolutions++;[pscustomobject]@{Root=$drawing;Handle=$drawing.NativeWindowHandle}}
    function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds){$script:persistenceWaitSeconds=$Seconds;$script:persistenceInsideWait=$true;try{& $actualWait $Condition $Message $Seconds}finally{$script:persistenceInsideWait=$false}}
    $startup=@();$index=$ready[0]
    if($index -gt 0 -and $statements[$index-1] -is [Management.Automation.Language.AssignmentStatementAst] -and $statements[$index-1].Left.Extent.Text -eq '$owned'){$startup+=@($statements[$index-1])}
    $startup+=@($statements[$index],$statements[$index+1])
    . ([scriptblock]::Create(($startup.Extent.Text -join "`n").Replace('System.Windows.Automation.','CapyOwnedSearchTest.')))
    if($script:root -ne $drawing){throw 'Persistence launch selected the input overlay instead of its owned drawing'}
    if($script:persistenceResolutions -ne 1 -or $script:persistenceWaitSeconds -ne 45){throw 'Persistence launch did not resolve its drawing within the original readiness wait'}
}
Write-Output 'Persistence launch uses its owned drawing when MainWindowHandle names an input overlay, within its original readiness deadline.'

$localizationAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-localization.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$useWindow=@($localizationAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Use-Window'},$true))
if($useWindow.Count -ne 1){throw 'Missing actual localization window switch'}
& {
    . ([scriptblock]::Create($useWindow[0].Extent.Text.Replace('[CapyRowPointer]','[CapyOwnedSearchTest.Driver]')))
    $review=[CapyOwnedSearchTest.Review]::new();$window=[pscustomobject]@{id=7;hwnd=131956};$run='private-owned-search'
    $prepared=[CapyOwnedSearchTest.Node]::new('prepared-window',33,0,0);$prepared.NativeWindowHandle=[IntPtr]$window.hwnd
    $discovered=[CapyOwnedSearchTest.Node]::new('discovered-window',33,0,0);$discovered.NativeWindowHandle=$prepared.NativeWindowHandle
    $script:windowRegistrations=0;$script:windowResolutions=0;[CapyOwnedSearchTest.Driver]::Activations=0
    function Register-CapyFixtureWindow($Process,[long]$WindowHandle,[string]$Directory){
        if($Process -ne $review -or $WindowHandle -ne $window.hwnd -or $Directory -ne $run){throw 'Window registration changed its owned inputs'}
        $script:windowRegistrations++
    }
    function Owned-DrawingWindow($Process,[long]$WindowHandle,[string]$TraceDirectory){
        if($Process -ne $review -or $WindowHandle -ne $window.hwnd -or $TraceDirectory -ne $run){throw 'Window discovery changed its owned inputs'}
        $script:windowResolutions++;[pscustomobject]@{Root=$discovered}
    }
    Use-Window $window $prepared
    if($script:windowRegistrations -ne 0 -or $script:windowResolutions -ne 0){throw 'Prepared window switch performed root discovery or registration'}
    if($script:current -ne $window -or $script:root -ne $prepared -or [CapyOwnedSearchTest.Driver]::Activations -ne 1 -or [CapyOwnedSearchTest.Driver]::GetForegroundWindow() -ne [IntPtr]$window.hwnd){throw 'Prepared window switch did not retain its exact root and activate its owned HWND'}
    Use-Window $window
    if($script:windowRegistrations -ne 1 -or $script:windowResolutions -ne 1 -or $script:root -ne $discovered -or $script:current -ne $window -or [CapyOwnedSearchTest.Driver]::Activations -ne 2){throw 'Unprepared window switch changed its discovery or activation path'}
    foreach($invalid in @(@{pid=99;hwnd=$window.hwnd},@{pid=33;hwnd=132146})){
        $foreign=[CapyOwnedSearchTest.Node]::new('invalid-window',$invalid.pid,0,0);$foreign.NativeWindowHandle=[IntPtr]$invalid.hwnd
        $rejected=$false
        try{Use-Window $window $foreign}catch{if($_.Exception.Message -ne 'Prepared window root does not belong to its registered owner'){throw};$rejected=$true}
        if(!$rejected -or [CapyOwnedSearchTest.Driver]::Activations -ne 2 -or $script:windowRegistrations -ne 1 -or $script:windowResolutions -ne 1 -or $script:root -ne $discovered -or $script:current -ne $window){throw 'Invalid prepared root changed activation, discovery or the current owned window'}
    }
}
Write-Output 'Localization window switching retains prepared owned roots without rediscovery, preserves ordinary discovery, and rejects foreign PID or HWND before activation.'
