$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$path=Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'
$ast=[Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
namespace CapyOwnedSearchTest {
 public enum TreeScope{Children,Descendants}
 public static class ControlType{public static object Menu="menu",MenuItem="menu-item",Button="button";}
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
  public int X,Y,Width,Height;public bool IsEmpty{get{return Width==0||Height==0;}}public int Top{get{return Y;}}public int Bottom{get{return Y+Height;}}
  public Rect(int x,int y){X=x;Y=y;Width=1414;Height=986;}
  public Rect(int x,int y,int width,int height){X=x;Y=y;Width=width;Height=height;}
  public bool Contains(Rect other){return !other.IsEmpty&&other.X>=X&&other.Y>=Y&&other.X+other.Width<=X+Width&&other.Y+other.Height<=Y+Height;}
  public void Intersect(Rect other){int right=Math.Min(X+Width,other.X+other.Width),bottom=Math.Min(Y+Height,other.Y+other.Height);X=Math.Max(X,other.X);Y=Math.Max(Y,other.Y);Width=Math.Max(0,right-X);Height=Math.Max(0,bottom-Y);}
  public void Inflate(int x,int y){X-=x;Y-=y;Width+=2*x;Height+=2*y;}
 }
 public enum ScrollAmount{NoAmount,SmallDecrement,SmallIncrement}
 public static class InvokePattern{public static object Pattern="invoke";}
 public static class TogglePattern{public static object Pattern="toggle";}
 public class ActionPattern {
  Node node;public ActionPattern(Node value){node=value;}
  public void Invoke(){node.Activate(false);}public void Toggle(){node.Activate(true);}
 }
 public static class ScrollPattern{public static object Pattern="scroll";public static double NoScroll=-1;}
 public class Scroll {
  public double VerticalScrollPercent,VerticalViewSize=50;public int Resets,Steps;public bool VerticallyScrollable=true;public Scroll Current{get{return this;}}
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
  public static IntPtr Foreground;public static int Activations,Injected;
  public static IntPtr PostedWindow,PostedLparam;public static UIntPtr PostedWparam;public static uint PostedMessage;public static int Posts;public static bool PostResult=true;
  public static bool PostMessage(IntPtr window,uint message,UIntPtr wparam,IntPtr lparam){PostedWindow=window;PostedMessage=message;PostedWparam=wparam;PostedLparam=lparam;Posts++;return PostResult;}
  public static void Down(string device,int x,int y){Injected++;}public static void Up(bool hover){Injected++;}
  public static int GetDpiForWindow(IntPtr window){return 96;}
  public static bool SetForegroundWindow(IntPtr window){Foreground=window;Activations++;return true;}
  public static IntPtr GetForegroundWindow(){return Foreground;}
 }
 public static class GeometryDriver{public static int GetDpiForWindow(IntPtr window){return 144;}}
 public class Node {
  public string AutomationId,Name;public object ControlType="control";public int ProcessId;bool offscreen;Rect bounds;public Node Parent;public Scroll Provider,TargetScroll;public int? ContentY;public bool UnclippedBounds;
  public bool HasKeyboardFocus;public IntPtr NativeWindowHandle=new IntPtr(1);
  public bool Enabled=true,InvokeAvailable,ToggleAvailable;public DateTime EnableAtUtc=DateTime.MinValue;public int Invocations,Toggles,DisabledReads;
  public bool IsEnabled{get{bool ready=Enabled&&DateTime.UtcNow>=EnableAtUtc;if(!ready)DisabledReads++;return ready;}}
  public void Activate(bool toggle){if(!IsEnabled)throw new InvalidOperationException("Disabled action invoked");if(toggle)Toggles++;else Invocations++;}
  public Rect BoundingRectangle{get{if(ContentY.HasValue){int y=ContentY.Value-(int)(TargetScroll.VerticalScrollPercent*2);return !UnclippedBounds&&(y>=100||y+20<=0)?new Rect():new Rect(0,y,80,20);}return bounds;}set{bounds=value;}}
  public bool IsOffscreen{get{return ContentY.HasValue?BoundingRectangle.IsEmpty:offscreen;}set{offscreen=value;}}
  public void SetFocus(){HasKeyboardFocus=true;}
  public bool TryGetCurrentPattern(object pattern,out object result){if((Object.Equals(pattern,InvokePattern.Pattern)&&InvokeAvailable)||(Object.Equals(pattern,TogglePattern.Pattern)&&ToggleAvailable)){result=new ActionPattern(this);return true;}result=Provider==null?null:new Pattern(Provider);return result!=null;}
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
 public class Review{public int Id=33;public IntPtr MainWindowHandle=new IntPtr(132146);public bool HasExited=false;public int ExitCode=0;public void Refresh(){}public bool CloseMainWindow(){return Driver.PostMessage(MainWindowHandle,0x0010,UIntPtr.Zero,IntPtr.Zero);}}
}
'@
foreach($name in @('Test-CapySearchItem','Find','Control','Wait-Until','Invoke')){
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

$invokeFailures=[Collections.Generic.List[string]]::new()
foreach($byName in @($false,$true)){
    $root.Children.Clear();$button=[CapyOwnedSearchTest.Node]::new('deferred-action',33,219,207)
    $button.Name='Deferred action';$button.Parent=$root;$button.InvokeAvailable=$true;$root.Children.Add($button)
    $value=if($byName){$button.Name}else{$button.AutomationId}
    $button.EnableAtUtc=[DateTime]::UtcNow.AddMilliseconds(150)
    $failure=$null;try{Invoke $value -Name:$byName}catch{$failure=$_}
    if($failure -or $button.Invocations -ne 1 -or !$button.DisabledReads){$invokeFailures.Add("Delayed action $value was not awaited: $failure")}
}
$caption=[CapyOwnedSearchTest.Node]::new('caption',33,219,207);$caption.Name=$button.Name;$root.Children.Insert(0,$caption)
$button.ControlType=[CapyOwnedSearchTest.ControlType]::Button
Invoke $button.Name -Name -Within $root -Type ([CapyOwnedSearchTest.ControlType]::Button)
if($button.Invocations -ne 2){throw 'Shared Invoke lost the requested control type or scope'}
$root.Children.Clear();$button=[CapyOwnedSearchTest.Node]::new('disabled-action',33,219,207)
$button.Name='Never apply';$button.Parent=$root;$button.InvokeAvailable=$true;$button.Enabled=$false;$root.Children.Add($button)
$failure=$null;try{Invoke $button.Name -Name}catch{$failure=$_}
if(!$failure -or $failure.Exception.Message -cne 'Never apply remained disabled' -or $button.Invocations){$invokeFailures.Add("Permanently disabled action had the wrong outcome: $failure")}
$button.Enabled=$true;$button.InvokeAvailable=$false;$button.ToggleAvailable=$true
Invoke $button.AutomationId
if($button.Toggles -ne 1){throw 'Shared Invoke lost its enabled Toggle fallback'}
if($invokeFailures.Count){throw ($invokeFailures -join [Environment]::NewLine)}
Write-Output 'Actual Invoke waits for delayed enabled state by ID/name, rejects a permanently disabled named action and preserves Toggle fallback.'

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
function Workspace-Root {[pscustomobject]@{Current=[pscustomobject]@{ItemStatus='{"elements":[]}'}}}
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

$revealFailures=[Collections.Generic.List[string]]::new()
foreach($case in @(
    @{name='inflated presenter';body=$true;top=0;height=300;steps=2},
    @{name='nested scroller';body=$true;top=20;height=60;steps=3},
    @{name='panel without body';body=$false;top=0;height=300;steps=0}
)){
    $root=[CapyOwnedSearchTest.Node]::new('root',33,0,0);$root.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,300)
    $group=[CapyOwnedSearchTest.Node]::new('workspace-group-24',33,0,0);$group.Parent=$root;$root.Children.Add($group)
    $provider=[CapyOwnedSearchTest.Node]::new('PART_ScrollPresenter',33,0,$case.top)
    $provider.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,$case.top,100,$case.height);$provider.Parent=$group;$group.Children.Add($provider)
    $provider.Provider=[CapyOwnedSearchTest.Scroll]::new()
    $item=[CapyOwnedSearchTest.Node]::new('target',33,0,0);$item.Parent=$provider;$provider.Children.Add($item)
    $item.ContentY=160;$item.UnclippedBounds=$true;$item.TargetScroll=$provider.Provider
    $script:traceReads=0;$script:nativeReads=0
    function Workspace-Root {
        $script:traceReads++
        $elements=if($case.body){@(@{id='panel-body-scroll-24'})}else{@()}
        [pscustomobject]@{Current=[pscustomobject]@{ItemStatus=(@{elements=$elements}|ConvertTo-Json -Depth 4 -Compress)}}
    }
    function Native-Bounds([string]$Id,[double]$Scale){
        $script:nativeReads++
        if(!$case.body){throw 'Absent body viewport must not be awaited'}
        if($Id -cne 'panel-body-scroll-24'){throw 'Reveal derived the wrong body viewport'}
        [CapyOwnedSearchTest.Rect]::new(0,0,100,100)
    }
    $failure=$null;$actual=$null
    try{$actual=Reveal 'target'}catch{$failure=$_}
    if($failure -or $actual -ne $item -or $provider.Provider.Steps -ne $case.steps -or $script:traceReads -ne 1 -or (!$case.body -and $script:nativeReads)){
        $revealFailures.Add("Owned body clip $($case.name): steps=$($provider.Provider.Steps), trace=$script:traceReads, native=$script:nativeReads, failure=$failure")
    }
}
if($revealFailures.Count){throw ($revealFailures -join [Environment]::NewLine)}
function Native-Bounds([string]$Id,[double]$Scale){[CapyOwnedSearchTest.Rect]::new(0,0,100,100)}
Write-Output 'Reveal intersects the published body viewport with inflated or nested scrollers and never awaits an unpublished body.'

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

$closeOwned=@($ast.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Close-OwnedDrawing'},$true))
if($closeOwned.Count -ne 1){throw 'Missing actual owned drawing close helper'}
& {
    . ([scriptblock]::Create($closeOwned[0].Extent.Text.Replace('[Diagnostics.Process]','[CapyOwnedSearchTest.Review]').Replace('[CapyWindowApi]','[CapyOwnedSearchTest.Driver]')))
    $review=[CapyOwnedSearchTest.Review]::new();$registered=[IntPtr]131956;$state=@{available=$true;resolutions=0}
    function Owned-DrawingWindow($Process,[long]$WindowHandle){
        if($Process -ne $review -or ($WindowHandle -and $WindowHandle -ne $registered.ToInt64())){throw 'Close changed its owned window inputs'}
        $state.resolutions++;if($state.available){[pscustomobject]@{Handle=$registered}}
    }
    [CapyOwnedSearchTest.Driver]::Posts=0
    foreach($handle in @(0,$registered.ToInt64())){
        if(!(Close-OwnedDrawing $review $handle)){throw 'Registered drawing close did not return the native post result'}
        if([CapyOwnedSearchTest.Driver]::PostedWindow -ne $registered -or [CapyOwnedSearchTest.Driver]::PostedWindow -eq $review.MainWindowHandle -or [CapyOwnedSearchTest.Driver]::PostedMessage -ne 0x0010 -or [CapyOwnedSearchTest.Driver]::PostedWparam -ne [UIntPtr]::Zero -or [CapyOwnedSearchTest.Driver]::PostedLparam -ne [IntPtr]::Zero){throw 'Close targeted the input overlay instead of posting WM_CLOSE to the registered drawing'}
    }
    if($state.resolutions -ne 2 -or [CapyOwnedSearchTest.Driver]::Posts -ne 2){throw 'Close did not resolve and post once per owned drawing request'}
    $state.available=$false
    if((Close-OwnedDrawing $review) -or [CapyOwnedSearchTest.Driver]::Posts -ne 2){throw 'Close posted to an unavailable owned drawing'}
    $state.available=$true;[CapyOwnedSearchTest.Driver]::PostResult=$false
    try{if(Close-OwnedDrawing $review){throw 'Close hid a rejected native WM_CLOSE'}}finally{[CapyOwnedSearchTest.Driver]::PostResult=$true}
}
Write-Output 'Owned close posts WM_CLOSE to its registered drawing instead of MainWindowHandle input overlays and retains native failure results.'

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
    if($script:CapyStateFile -ne (Join-Path $run "ui-state-$($review.Id)-7.json")){throw 'Prepared localization root did not bind its owned presentation trace'}
    if($script:windowRegistrations -ne 0 -or $script:windowResolutions -ne 0){throw 'Prepared window switch performed root discovery or registration'}
    if($script:current -ne $window -or $script:root -ne $prepared -or [CapyOwnedSearchTest.Driver]::Activations -ne 1 -or [CapyOwnedSearchTest.Driver]::GetForegroundWindow() -ne [IntPtr]$window.hwnd){throw 'Prepared window switch did not retain its exact root and activate its owned HWND'}
    Use-Window $window
    if($script:CapyStateFile -ne (Join-Path $run "ui-state-$($review.Id)-7.json")){throw 'Discovered localization root did not bind its owned presentation trace'}
    if($script:windowRegistrations -ne 1 -or $script:windowResolutions -ne 1 -or $script:root -ne $discovered -or $script:current -ne $window -or [CapyOwnedSearchTest.Driver]::Activations -ne 2){throw 'Unprepared window switch changed its discovery or activation path'}
    foreach($invalid in @(@{pid=99;hwnd=$window.hwnd},@{pid=33;hwnd=132146})){
        $foreign=[CapyOwnedSearchTest.Node]::new('invalid-window',$invalid.pid,0,0);$foreign.NativeWindowHandle=[IntPtr]$invalid.hwnd
        $rejected=$false
        try{Use-Window $window $foreign}catch{if($_.Exception.Message -ne 'Prepared window root does not belong to its registered owner'){throw};$rejected=$true}
        if(!$rejected -or [CapyOwnedSearchTest.Driver]::Activations -ne 2 -or $script:windowRegistrations -ne 1 -or $script:windowResolutions -ne 1 -or $script:root -ne $discovered -or $script:current -ne $window){throw 'Invalid prepared root changed activation, discovery or the current owned window'}
    }
    $other=[pscustomobject]@{id=8;hwnd=132146}
    $otherRoot=[CapyOwnedSearchTest.Node]::new('other-owned-window',33,0,0);$otherRoot.NativeWindowHandle=[IntPtr]$other.hwnd
    Use-Window $other $otherRoot
    if($script:current -ne $other -or $script:root -ne $otherRoot -or $script:CapyStateFile -ne (Join-Path $run "ui-state-$($review.Id)-8.json")){throw 'Switching localization windows retained the previous presentation trace'}
    & {
        $script:windowResolutions=0
        $beforeActivation=[CapyOwnedSearchTest.Driver]::Activations
        function Owned-DrawingWindow($Process,[long]$WindowHandle,[string]$TraceDirectory){
            if($Process -ne $review -or $WindowHandle -ne $window.hwnd -or $TraceDirectory -ne $run){throw 'Pending root discovery changed its owned inputs'}
            if([CapyOwnedSearchTest.Driver]::Activations -ne $beforeActivation){throw 'Localization activated before its native root became available'}
            $script:windowResolutions++
            if($script:windowResolutions -ge 2){[pscustomobject]@{Root=$discovered}}
        }
        function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds){
            if($Seconds -ne 10){throw 'Localization window handoff changed its deadline'}
            foreach($step in 1..2){if(& $Condition){return}}
            throw $Message
        }
        Use-Window $window
        if($script:windowResolutions -ne 2 -or $script:root -ne $discovered -or $script:current -ne $window -or $script:CapyStateFile -ne (Join-Path $run "ui-state-$($review.Id)-7.json")){throw 'Localization proceeded before its owned native root became available'}
        if([CapyOwnedSearchTest.Driver]::Activations -ne $beforeActivation+1){throw 'Localization activated an unavailable root or repeated its accepted activation'}
        $beforeActivation=[CapyOwnedSearchTest.Driver]::Activations
        $previousRoot=$script:root;$previousWindow=$script:current;$previousState=$script:CapyStateFile
        function Owned-DrawingWindow($Process,[long]$WindowHandle,[string]$TraceDirectory){
            if([CapyOwnedSearchTest.Driver]::Activations -ne $beforeActivation){throw 'Unavailable native root reached foreground activation'}
        }
        $rejected=$false
        try{Use-Window $window}catch{if($_.Exception.Message -ne 'Review window did not own foreground input'){throw};$rejected=$true}
        if(!$rejected -or [CapyOwnedSearchTest.Driver]::Activations -ne $beforeActivation -or $script:root -ne $previousRoot -or $script:current -ne $previousWindow -or $script:CapyStateFile -ne $previousState){throw 'Unavailable localization root advanced or discarded the previous owned binding'}
    }
}
Write-Output 'Localization window switching retains prepared owned roots without rediscovery, preserves ordinary discovery, and rejects foreign PID or HWND before activation.'

$hdrAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-hdr.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$captionWait=@($hdrAst.FindAll({param($node)$node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'Wait-Until' -and @($node.CommandElements|Where-Object {$_ -is [Management.Automation.Language.StringConstantExpressionAst] -and $_.Value -eq 'Window title lacks the unsaved marker'}).Count -eq 1},$true))
if($captionWait.Count -ne 1){throw 'Missing actual HDR dirty-window caption assertion'}
$condition=@($captionWait[0].CommandElements|Where-Object {$_ -is [Management.Automation.Language.ScriptBlockExpressionAst]})
if($condition.Count -ne 1){throw 'HDR dirty caption assertion has no unique predicate'}
$predicate=[scriptblock]::Create($condition[0].ScriptBlock.EndBlock.Extent.Text)
$dirty=[string][char]0x2022+' Untitled'
foreach($case in @(@{owned=$dirty;other='';expected=$true},@{owned='Untitled';other=$dirty;expected=$false})){
    $root=[pscustomobject]@{Current=[pscustomobject]@{Name=$case.owned;NativeWindowHandle=[IntPtr]42}}
    $review=[pscustomobject]@{MainWindowTitle=$case.other;MainWindowHandle=[IntPtr]99};$review|Add-Member -MemberType ScriptMethod -Name Refresh -Value {}
    if([bool](& $predicate) -ne $case.expected){throw 'HDR dirty-caption assertion read another process window instead of its registered drawing'}
}
Write-Output 'HDR dirty caption observes the owned drawing while preserving both dirty and clean title checks.'

$toolbarAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-toolbar-components.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$capChecks=@($toolbarAst.FindAll({param($n)$n -is [Management.Automation.Language.ForEachStatementAst] -and $n.Variable.Extent.Text -eq '$text' -and $n.Extent.Text.Contains("throw 'Slider value/unit touches its cap edge'")},$true))
if($capChecks.Count -ne 1){throw 'Missing actual slider cap geometry assertion'}
$statements=@($capChecks[0].Parent.Statements);$last=[array]::IndexOf($statements,$capChecks[0])
$start=@(for($i=0;$i -lt $last;$i++){if($statements[$i] -is [Management.Automation.Language.AssignmentStatementAst] -and $statements[$i].Left.Extent.Text -eq '$cap'){$i}})[-1]
if($null -eq $start){throw 'Missing actual slider cap geometry measurements'}
if($start -gt 0 -and $statements[$start-1] -is [Management.Automation.Language.AssignmentStatementAst] -and $statements[$start-1].Left.Extent.Text -eq '$scale'){$start--}
$capGeometry=[scriptblock]::Create(($statements[$start..$last].Extent.Text -join "`n").Replace('[CapyRowPointer]','[CapyOwnedSearchTest.GeometryDriver]'))
& {
    function Rectangle([double]$X,[double]$Y,[double]$Width,[double]$Height){[pscustomobject]@{Left=$X;Top=$Y;Right=$X+$Width;Bottom=$Y+$Height}}
    $tile=[pscustomobject]@{id=17};$drawingWindow=[IntPtr]1;$scale=1.5
    $native=@{
        'slider-cap-17'=(Rectangle 9 (197.3333282470703*1.5) 81 81)
        'slider-value-17'=(Rectangle (17.1875*1.5) (202.88735961914062*1.5) (31.625*1.5) (19.5078125*1.5))
        'slider-unit-17'=(Rectangle (25.32291603088379*1.5) (224.39517211914062*1.5) (15.354169845581055*1.5) (19.5078125*1.5))
    }
    $rounded=@{'slider-cap-17'=(Rectangle 9 296 81 81);'slider-value-17'=(Rectangle 26 304 48 30);'slider-unit-17'=(Rectangle 38 336 24 30)}
    function Box([string]$Id){$rounded[$Id]}
    function Native-Bounds([string]$Id,[double]$Scale){if($Scale -ne 1.5 -or !$native.ContainsKey($Id)){throw 'Unexpected slider native geometry request'};$native[$Id]}
    . $capGeometry
    $original=$native['slider-value-17'];$native['slider-value-17']=Rectangle $original.Left ($native['slider-cap-17'].Top+5*$scale) ($original.Right-$original.Left) ($original.Bottom-$original.Top)
    $rejected=$false
    try{. $capGeometry}catch{if($_.Exception.Message -ne 'Slider value/unit touches its cap edge'){throw};$rejected=$true}
    if(!$rejected){throw 'Slider cap assertion accepted less than its original inset'}
    $native['slider-value-17']=$original;$original=$native['slider-unit-17'];$native['slider-unit-17']=Rectangle $original.Left ($native['slider-value-17'].Bottom-1) ($original.Right-$original.Left) ($original.Bottom-$original.Top)
    $rejected=$false
    try{. $capGeometry}catch{if($_.Exception.Message -ne 'A long Medium vertical value did not stack its unit'){throw};$rejected=$true}
    if(!$rejected){throw 'Slider cap assertion accepted an overlapping stacked unit'}
}
Write-Output 'Slider cap geometry uses native arranged bounds and retains its original inset and stacked-unit assertions.'

$scopesAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-scopes.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$scopesReveal=@($scopesAst.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Reveal'},$true))
if($scopesReveal.Count -ne 1){throw 'Missing actual scopes Reveal helper'}
& {
    . ([scriptblock]::Create($scopesReveal[0].Extent.Text.Replace('System.Windows.Automation.','CapyOwnedSearchTest.').Replace('[CapyRowPointer]','[CapyOwnedSearchTest.Driver]')))
    $drawingWindow=[IntPtr]1
    $viewport=[CapyOwnedSearchTest.Node]::new('panel-body-scroll-14',33,0,0)
    $viewport.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100);$viewport.Provider=[CapyOwnedSearchTest.Scroll]::new()
    $chart=[CapyOwnedSearchTest.Node]::new('histogram-chart',33,0,60);$chart.Parent=$viewport
    $chart.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,60,80,40)
    $state=@{arranged=[CapyOwnedSearchTest.Rect]::new(0,60,80,80);waits=0}
    function Control([string]$Id){if($Id -ne 'histogram-chart'){throw 'Unexpected scopes target'};$chart}
    function Native-Bounds([string]$Id,[double]$Scale){if($Id -ne 'histogram-chart' -or $Scale -ne 1){throw 'Unexpected scopes native bounds request'};$state.arranged}
    function Start-Sleep([int]$Milliseconds){if($Milliseconds -ne 200){throw 'Unexpected scopes sleep'}}
    function Wait-Until([scriptblock]$Condition,[string]$Message){
        $state.waits++
        if(& $Condition){throw 'Scopes Reveal accepted clipped UIA bounds as its scroll acknowledgment'}
        $state.arranged=[CapyOwnedSearchTest.Rect]::new(0,16,80,80)
        if(!(& $Condition)){throw $Message}
    }
    $visible=Reveal 'histogram-chart'
    if($viewport.Provider.VerticalScrollPercent -le 0){throw 'Scopes Reveal did not scroll the full native chart hidden below its clipped UIA rectangle'}
    if($state.waits -ne 1 -or $visible.Bottom -ne 100 -or $visible.Height -ne 100){throw 'Scopes Reveal did not acknowledge native chart bounds within its viewport'}
}
Write-Output 'Scopes Reveal scrolls the full native chart despite clipped UIA bounds and acknowledges its native arranged position.'
$scopePlot=@($scopesAst.FindAll({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$plot' -and $n.Right.Extent.Text.Contains("'-chart'")},$true))
if($scopePlot.Count -ne 1){throw 'Missing actual scope plot geometry'}
$statements=@($scopePlot[0].Parent.Statements);$start=[array]::IndexOf($statements,$scopePlot[0]);if($start -gt 0 -and $statements[$start-1].Extent.Text.Contains("'-chart'")){$start--}
$last=@(for($i=$start;$i -lt $statements.Count;$i++){if($statements[$i] -is [Management.Automation.Language.ForEachStatementAst] -and $statements[$i].Extent.Text.Contains('exceeds the plot width')){$i}})
if($last.Count -ne 1){throw 'Missing actual scope width assertions'}
$scopeGeometry=[scriptblock]::Create(($statements[$start..$last[0]].Extent.Text -join "`n"))
& {
    $drawingWindow=[IntPtr]1;$tag='en';$width=1500;$native=@{};$measured=@{}
    function Scope-Rect([double]$X,[double]$Y,[double]$Width,[double]$Height){[pscustomobject]@{Left=$X;Top=$Y;Right=$X+$Width;Bottom=$Y+$Height;Width=$Width;Height=$Height}}
    function Control([string]$Id,[switch]$Arranged){if(!$Arranged -or !$native.ContainsKey($Id)){throw 'Scope geometry lost its arranged control precondition'};$measured[$Id]=$true;[pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=$(if($Id.EndsWith('-chart')){Scope-Rect ((100.25+4/1.5)*$scale) (50*$scale) (11/1.5*$scale) (28*$scale)}else{$native[$Id]})}}}
    function Native-Bounds([string]$Id,[double]$Scale){if(!$measured[$Id] -or $Scale -ne $expectedScale){throw 'Scope geometry did not arrange its owned native target'};$native[$Id]}
    foreach($scale in @(1,1.5,2)){
        $expectedScale=$scale
        foreach($prefix in 'histogram','waveform'){
            $native.Clear();$measured.Clear()
            $native[$prefix+'-chart']=Scope-Rect (100.25*$scale) (50*$scale) (225.5*$scale) (160*$scale)
            $native[$prefix+'-log']=Scope-Rect (100.25*$scale) (214*$scale) (95.5*$scale) (26*$scale)
            $native[$prefix+'-status']=Scope-Rect (100.25*$scale) (246.25*$scale) (150*$scale) (19.5*$scale)
            $native[$prefix+'-shadows']=Scope-Rect (273.75*$scale) (244*$scale) (26*$scale) (26*$scale)
            $native[$prefix+'-highlights']=Scope-Rect (299.75*$scale) (244*$scale) (26*$scale) (26*$scale)
            . $scopeGeometry
            foreach($id in 'log','status','shadows','highlights'){
                $before=$native[$prefix+'-'+$id];$native[$prefix+'-'+$id]=Scope-Rect $before.Left $before.Top ($native[$prefix+'-chart'].Right-$before.Left+1.25) $before.Height
                $rejected=$false;try{. $scopeGeometry}catch{if(!$_.Exception.Message.StartsWith("$tag $prefix-$id at $width pixels exceeds the plot width")){throw};$rejected=$true}
                if(!$rejected){throw 'Scope geometry accepted a native control more than one pixel outside the plot'}
                $native[$prefix+'-'+$id]=$before
            }
        }
    }
}
Write-Output 'Scope width assertions use full native arranged bounds despite clipped UIA charts at 100, 150 and 200 percent, rejecting real native overflow.'


& {
    $switcher=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-switcher-drag.ps1'),[ref]$tokens,[ref]$errors)
    if(@($errors).Count){throw ($errors|Out-String)}
    $action=@($switcher.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Swipe-DeleteAction'},$true))
    if($action.Count -ne 1){throw 'Missing actual native swipe Delete action'}
    . ([scriptblock]::Create($action[0].Extent.Text.Replace('[CapyRowPointer]','[CapyOwnedSearchTest.Driver]')))
    $row=[pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=[pscustomobject]@{Right=1308;Y=807;Height=84}}}
    $button=[pscustomobject]@{AutomationId='workspace-manager-swipe-delete-custom'};$state=@{visible=$false;by_name=$false;taps=0}
    $scale=1.5;$Device='touch';$run='C:\owned'
    function Manager {@{rows=@(@{id='custom';options=@(@{action=@{type='delete'};label='Delete'})})}}
    function Control([string]$Id,[switch]$Arranged){$row}
    function Find([string]$Value,[switch]$Name,$Within,[switch]$Visible){
        if($state.visible -and (($Name -and $state.by_name) -or (!$Name -and !$state.by_name))){$button}
    }
    function Native-TapAt($Control,[double]$Drift){if($Control -ne $button){throw 'Swipe Delete targeted another control'};$state.taps++}
    function Gesture {@{}}
    function Set-Content {param($Path)process{}}
    [CapyOwnedSearchTest.Driver]::Injected=0;$rejected=$false
    try{Swipe-DeleteAction 'custom'}catch{if($_.Exception.Message -ne 'Native swipe Delete action is unavailable'){throw};$rejected=$true}
    if(!$rejected -or $state.taps -ne 0 -or [CapyOwnedSearchTest.Driver]::Injected -ne 0){throw 'Missing native Swipe Delete injected into another control'}
    foreach($byName in $false,$true){$state.visible=$true;$state.by_name=$byName;Swipe-DeleteAction 'custom'}
    if($state.taps -ne 2 -or [CapyOwnedSearchTest.Driver]::Injected -ne 0){throw 'Visible native swipe Delete actions did not retain their direct targets'}
}
Write-Output 'Native swipe Delete rejects an absent action before input and retains visible ID and label targets.'

$tiltAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-tilt.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$headerCheck=@($tiltAst.FindAll({param($node)$node -is [Management.Automation.Language.IfStatementAst] -and $node.Extent.Text.Contains('title and Close do not share a centerline')},$true))
if($headerCheck.Count -ne 1){throw 'Missing actual response header centerline assertion'}
$statements=@($headerCheck[0].Parent.Statements);$last=[array]::IndexOf($statements,$headerCheck[0])
$start=@(for($i=0;$i -lt $last;$i++){if($statements[$i] -is [Management.Automation.Language.AssignmentStatementAst] -and $statements[$i].Left.Extent.Text -in @('$headerBoxes','$title')){$i}})[0]
if($null -eq $start){throw 'Missing actual response header measurements'}
$headerGeometry=[scriptblock]::Create(($statements[$start..$last].Extent.Text -join "`n"))
& {
    $review=[pscustomobject]@{Id=7};$drawingWindow=[IntPtr]1
    $geometry=@{}
    function Control([string]$Id,[switch]$Arranged){[pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=[pscustomobject]@{Top=$(if($Id.EndsWith('-title')){if($scale -eq 1.5){175}else{$geometry.titleTop}}else{$geometry.closeTop});Height=24*$scale}}}}
    function Native-Bounds([string]$Id,[double]$Scale){if($Scale -notin @(1,1.5,2) -or $Id -notin @("pen-$kind-title","pen-$kind-close")){throw 'Unexpected response header geometry request'};[pscustomobject]@{Top=$(if($Id.EndsWith('-title')){$geometry.titleTop}else{$geometry.closeTop});Height=$(if($Id.EndsWith('-title')){19.5078125*$Scale}else{24*$Scale})}}
    function Workspace-Root{[pscustomobject]@{Current=[pscustomobject]@{ItemStatus=(@{elements=@(foreach($part in 'title','close'){@{id="pen-$kind-$part";bounds=@{x=0;y=$geometry[($part+'Top')]/$scale;width=$(if($part -eq 'title'){320}else{24});height=24};actual_bounds=@{x=0;y=$geometry[($part+'Top')]/$scale;width=$(if($part -eq 'title'){87.7279}else{24});height=$(if($part -eq 'title'){19.5078125}else{24})}}})}|ConvertTo-Json -Depth 6 -Compress)}}}
    function Screen-Bounds($Bounds){[pscustomobject]@{Top=$Bounds.y*$scale;Height=$Bounds.height*$scale}}
    function Evidence([string]$Name,$Value){if($Name -ne 'response-header-failure'){throw 'Unexpected response header evidence'}}
    foreach($scale in @(1,1.5,2)){
        $geometry.closeTop=118*$scale;$geometry.titleTop=(118-11*96/72*262/4096)*$scale
        foreach($kind in @('pressure','tilt')){
            . $headerGeometry
            $top=$geometry.titleTop;$geometry.titleTop=$geometry.closeTop-1.25*$scale;$rejected=$false
            try{. $headerGeometry}catch{if(!$_.Exception.Message.StartsWith("$kind title and Close do not share a centerline")){throw};$rejected=$true}
            if(!$rejected){throw 'Response header accepted a native centerline gap greater than one DIP'}
            $geometry.titleTop=$top
        }
    }
}
Write-Output 'Response headers use allocated native rows at 100, 150 and 200 percent and retain their one-DIP centerline bound.'

$paperAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/layer-paper.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$textureSpan=@($paperAst.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Texture-Span'},$true))
if($textureSpan.Count -ne 1){throw 'Missing actual Texture-Span geometry'}
$rowAssignment=@($textureSpan[0].Body.FindAll({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$rowEvidence'},$true))
if($rowAssignment.Count -ne 1){throw 'Missing actual texture row evidence assignment'}
$statements=@($rowAssignment[0].Parent.Statements);$last=[array]::IndexOf($statements,$rowAssignment[0])
$start=@(for($i=0;$i -lt $last;$i++){if($statements[$i] -is [Management.Automation.Language.AssignmentStatementAst] -and $statements[$i].Left.Extent.Text -eq '$rows'){$i}})[-1]
if($null -eq $start){throw 'Missing actual texture row grouping'}
$rowGeometry=[scriptblock]::Create("param(`$tiles,`$viewport)`n"+($statements[$start..$last].Extent.Text -join "`n")+"`n`$rowEvidence")
& {
    foreach($case in @('rounded-spacing','viewport-edge','partial-tile','heterogeneous','top-aligned','bottom-touching')){
        $viewport=[CapyOwnedSearchTest.Rect]::new(830,393,900,661)
        $tiles=@(for($i=0;$i -lt 4;$i++){
            $top=@(462,636,808,982)[$i];$height=if($i -eq 3){72}else{156};$left=850
            if($i -eq 1 -and $case -eq 'viewport-edge'){$left=829}
            if($i -eq 1 -and $case -eq 'partial-tile'){$height=74}
            $previewHeight=if($height -lt 156){$height-4}else{120}
            [pscustomobject]@{id=$i;bounds=[CapyOwnedSearchTest.Rect]::new($left,$top,128,$height);preview=[CapyOwnedSearchTest.Rect]::new(($left+4),($top+4),120,$previewHeight)}
        })
        if($case -eq 'heterogeneous'){
            $viewport=[CapyOwnedSearchTest.Rect]::new(740,313,1079,793)
            $tiles=@(for($row=0;$row -lt 4;$row++){
                $count=if($row -eq 3){3}else{6}
                for($column=0;$column -lt $count;$column++){
                    $top=@(313,452,659,866)[$row];$left=770+171*$column
                    $height=if($row -eq 0){118}elseif($row -eq 3 -and $column -eq 2){210}else{186}
                    $previewTop=if($row -eq 0){313}else{$top+5};$previewHeight=if($row -eq 0){81}else{144}
                    [pscustomobject]@{id=6*$row+$column;bounds=[CapyOwnedSearchTest.Rect]::new($left,$top,155,$height);preview=[CapyOwnedSearchTest.Rect]::new(($left+5),$previewTop,144,$previewHeight)}
                }
            })
        }
        if($case -in @('top-aligned','bottom-touching')){
            $viewport=[CapyOwnedSearchTest.Rect]::new(740,313,1079,793)
            $top=if($case -eq 'top-aligned'){313}else{920}
            $tiles=@(
                [pscustomobject]@{id=0;bounds=[CapyOwnedSearchTest.Rect]::new(770,$top,155,186);preview=[CapyOwnedSearchTest.Rect]::new(775,($top+5),144,144)}
                [pscustomobject]@{id=1;bounds=[CapyOwnedSearchTest.Rect]::new(770,520,155,186);preview=[CapyOwnedSearchTest.Rect]::new(775,525,144,144)}
            )
        }
        $rowEvidence=@(& $rowGeometry $tiles $viewport);$complete=@($rowEvidence|Where-Object full)
        if($case -eq 'heterogeneous'){
            if($complete.Count -ne 3 -or $rowEvidence[0].full -or !$rowEvidence[1].full -or !$rowEvidence[2].full -or !$rowEvidence[3].full){throw 'Texture row geometry misclassified heterogeneous preview bounds'}
        }elseif($case -eq 'top-aligned'){
            if($complete.Count -ne 2 -or !$rowEvidence[0].full){throw 'Texture row geometry rejected a top-aligned square preview at its natural inset'}
        }elseif($case -eq 'bottom-touching'){
            if($complete.Count -ne 1 -or $rowEvidence[1].full){throw 'Texture row geometry accepted a bottom-touching tile'}
        }else{
            $expected=if($case -eq 'rounded-spacing'){3}else{2}
            if($complete.Count -ne $expected -or $rowEvidence[3].full -or !$rowEvidence[0].full -or !$rowEvidence[2].full){throw "Texture row geometry misclassified $case native bounds"}
            if($case -ne 'rounded-spacing' -and $rowEvidence[1].full){throw "Texture row geometry accepted $case"}
        }
    }
}
Write-Output 'Texture rows classify unclipped square previews at their natural inset, retaining heterogeneous, aligned, bottom-edge and partial cases without input.'

$footerAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/LocalizationProperties.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$footer=@($footerAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Layers-Footer'},$true))
$localizationAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-localization.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$footerCalls=@($localizationAst.FindAll({param($node)$node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'Layers-Footer'},$true))
if($footer.Count -ne 1 -or $footerCalls.Count -ne 2){throw 'Missing actual retained footer setup and locale checks'}
Add-Type -TypeDefinition @'
using System;
namespace CapyFooterTest {
 public static class Driver {
  public static int Dpi=96,Contacts,Moves,Origin,Delta;public static bool Modal;public static string Order;
  public static int GetDpiForWindow(IntPtr window){return Dpi;}
  public static void Initialize(uint process){Order="initialize";}
  public static void Down(string device,int x,int y){if(Modal)throw new InvalidOperationException("Footer injected beneath the retained modal");Order+=",down";Contacts++;Origin=x;}
  public static void Move(int x,int y){Order+=",move";Moves++;Delta=Origin-x;}
  public static void Up(){Order+=",up";}public static void Dispose(){Order+=",dispose";}
 }
}
'@
& {
    . ([scriptblock]::Create($footer[0].Extent.Text.Replace('[CapyRowPointer]','[CapyFooterTest.Driver]')))
    $prepareFooter=[scriptblock]::Create($footerCalls[0].Extent.Text);$checkFooter=[scriptblock]::Create($footerCalls[1].Extent.Text)
    $review=[pscustomobject]@{Id=33};$script:current=[pscustomobject]@{hwnd=1}
    $footerState=@{width=296;extent=0;ackWidth=$true;ackExtent=$true;arranged=0}
    function Model{
        $width=$footerState.width;if($footerState.ackWidth){$width+=[CapyFooterTest.Driver]::Delta/$scale}
        $extent=$footerState.extent;if([CapyFooterTest.Driver]::Moves -and $footerState.ackExtent){$extent=$width+6}
        @{layout=@{groups=@(@{id=16;active='layers';bounds=@{x=700;y=400;width=$width;height=270}});dividers=@(@{id=11;axis='horizontal';fixed=$false;bounds=@{x=694;y=90;width=6;height=560}})};state=@{workspace=@{layout=@{bands=@(@{id=11;extent=$extent})}};layer_tools=@{footer=@(@{button='new_selection_layer';min_width=262})}}}|ConvertTo-Json -Depth 10|ConvertFrom-Json
    }
    function Fresh-Model{Model}
    function Control([string]$Id,[switch]$Arranged){
        if($Id -eq 'layer-new-selection'){$footerState.arranged++;return 'selection button'}
        if($Id -ne 'divider-11' -or !$Arranged){throw 'Unexpected footer geometry lookup'}
        [pscustomobject]@{Current=@{BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(100,200,(6*$scale),(560*$scale))}}
    }
    function Wait-Until($Predicate,[string]$Message){if(!(& $Predicate)){throw $Message}}
    foreach($scale in @(1,1.5,2)){
        [CapyFooterTest.Driver]::Dpi=96*$scale;[CapyFooterTest.Driver]::Modal=$false
        [CapyFooterTest.Driver]::Contacts=0;[CapyFooterTest.Driver]::Moves=0;[CapyFooterTest.Driver]::Delta=0
        $footerState.width=296;$footerState.extent=0;$footerState.ackWidth=$true;$footerState.ackExtent=$true;$footerState.arranged=0
        & $prepareFooter
        if([CapyFooterTest.Driver]::Contacts -ne 1 -or [CapyFooterTest.Driver]::Moves -ne 1 -or [CapyFooterTest.Driver]::Delta -ne 12*$scale -or [CapyFooterTest.Driver]::Order -ne 'initialize,down,move,up,dispose' -or (Model).state.workspace.layout.bands[0].extent -le 0){throw 'English-wide automatic footer column was not explicitly prepared before the retained modal'}
        [CapyFooterTest.Driver]::Modal=$true;& $checkFooter
        if([CapyFooterTest.Driver]::Contacts -ne 1 -or $footerState.arranged -ne 2){throw 'Retained modal footer check injected input or omitted its arranged control assertion'}
        $footerState.ackWidth=$false;$footerState.width=242;$rejected=$false
        try{& $checkFooter}catch{if(!$_.Exception.Message.StartsWith('Layers did not expose its complete footer:')){throw};$rejected=$true}
        if(!$rejected -or [CapyFooterTest.Driver]::Contacts -ne 1){throw 'Narrow retained footer did not fail without background input'}
        [CapyFooterTest.Driver]::Modal=$false;[CapyFooterTest.Driver]::Moves=0;[CapyFooterTest.Driver]::Delta=0
        $footerState.width=296;$footerState.ackWidth=$true;$footerState.ackExtent=$false;$rejected=$false
        try{& $prepareFooter}catch{if(!$_.Exception.Message.StartsWith('Layers did not expose its complete footer:')){throw};$rejected=$true}
        if(!$rejected){throw 'Footer setup accepted width without acknowledging the authored band extent'}
    }
}
Write-Output 'Retained footer setup authors its sufficient column width before the modal; locale checks preserve the width bound without background input.'

$switcherHoldAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-switcher-drag.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$switcherDrag=@($switcherHoldAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Drag'},$true))
if($switcherDrag.Count -ne 1){throw 'Missing actual switcher drag journey'}
$switcherHold=@($switcherDrag[0].Body.FindAll({param($node)$node -is [Management.Automation.Language.IfStatementAst] -and $node.Clauses[0].Item1.Extent.Text -eq '$Hold'},$true))
if($switcherHold.Count -ne 1){throw 'Missing actual held-menu assertion'}
$heldMenuCheck=[scriptblock]::Create(($switcherHold[0].Clauses[0].Item2.Statements|ForEach-Object {$_.Extent.Text}) -join "`n")
& {
    $Device='touch';$menu=[pscustomobject]@{AutomationId='workspace-manager-move-up'}
    $publication=@{case='';lookups=0;visibleRequests=0;after=0}
    function Gesture {[pscustomobject]@{phase='held'}}
    function Find([string]$Id,[switch]$Visible){
        if($Id -ne 'workspace-manager-move-up'){throw 'Held menu changed its native target'}
        $publication.lookups++;if($Visible){$publication.visibleRequests++}
        if($publication.case -eq 'missing' -or ($publication.case -eq 'hidden' -and $Visible)){return}
        if($publication.lookups -ge $publication.after){$menu}
    }
    function Wait-Until([scriptblock]$Predicate,[string]$Message,[double]$Timeout){
        foreach($sample in 1..3){if(& $Predicate){return}}
        throw $Message
    }
    foreach($case in 'delayed','ready','hidden','missing'){
        $publication.case=$case;$publication.lookups=0;$publication.visibleRequests=0;$publication.after=if($case -eq 'delayed'){2}else{1}
        $rejected=$false
        try{& $heldMenuCheck}catch{if($_.Exception.Message -ne 'Hold did not open a native context menu'){throw};$rejected=$true}
        if($rejected -ne ($case -in @('hidden','missing'))){throw "Held menu did not preserve its $case publication assertion"}
        if($publication.lookups -lt $publication.after -or $publication.visibleRequests -ne $publication.lookups){throw 'Held menu did not observe its actual visible native target'}
    }
}
Write-Output 'Workspace held-menu assertion observes delayed native publication and rejects hidden or missing menus without input.'

$layoutAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-workspace-layout.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$idleFps=@($layoutAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Idle-Fps'},$true))
if($idleFps.Count -ne 1){throw 'Missing actual idle Diagnostics assertion'}
& {
    . ([scriptblock]::Create($idleFps[0].Extent.Text))
    $samples=@{}
    function Control([string]$Id){
        if($Id -ne 'renderer-stat-2'){throw 'Unexpected idle Diagnostics reading'}
        if($samples.after){$samples.fpsReads++}
        [pscustomobject]@{Current=@{Name=$(if($samples.after){$samples.fps}else{'0'})}}
    }
    function Drawing-Frames{$samples.frameReads++;if($samples.frameReads -gt 2){throw 'Idle Diagnostics sampled Frames more than once after the interval'};if($samples.after){$samples.frames}else{10}}
    function Start-Sleep([int]$Milliseconds){if($Milliseconds -ne 1500){throw 'Idle Diagnostics changed its sample interval'};$samples.sleeps++;$samples.after=$true}
    function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds){if($Seconds -ne 15 -or !(& $Condition)){throw $Message}}
    foreach($case in @(@{frames=10;fps='0';fail=$false},@{frames=11;fps='0';fail=$true},@{frames=10;fps='7';fail=$true})){
        $samples.after=$false;$samples.frames=$case.frames;$samples.fps=$case.fps;$samples.frameReads=0;$samples.fpsReads=0;$samples.sleeps=0;$failed=$false
        try{$result=Idle-Fps}catch{
            if($_.Exception.Message -ne "Idle Diagnostics created canvas drawing updates or reported active FPS: frames_before=10 frames_after=$($case.frames) fps=$($case.fps)"){throw}
            $failed=$true
        }
        if($failed -ne $case.fail -or (!$failed -and $result -ne 10)){throw 'Idle Diagnostics changed its stable, moving-frame or active-FPS verdict'}
        if($samples.frameReads -ne 2 -or $samples.fpsReads -ne 1 -or $samples.sleeps -ne 1){throw 'Idle Diagnostics did not report its single post-interval samples'}
    }
}
Write-Output 'Idle Diagnostics preserves stable, moving-frame and active-FPS verdicts and reports its single before/after frame and FPS samples.'

$runtimeAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-runtime-filters.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$runtimePreview=@($runtimeAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Preview-Hash'},$true))
if($runtimePreview.Count -ne 1){throw 'Missing actual runtime filter preview capture'}
Add-Type -AssemblyName System.Drawing,WindowsBase
Add-Type -TypeDefinition @'
using System;
namespace CapyRuntimePreviewTest {
 public static class Capture {
  public struct Rect {public int left,top,right,bottom;}
  public static int Captures;
  public static bool GetWindowRect(IntPtr window,out Rect rect){rect=new Rect{right=64,bottom=64};return true;}
  public static bool PrintWindow(IntPtr window,IntPtr dc,uint flags){Captures++;return true;}
 }
}
'@
& {
    . ([scriptblock]::Create($runtimePreview[0].Extent.Text.Replace('[CapyRuntimeFilterCapture','[CapyRuntimePreviewTest.Capture')))
    $registeredWindow=[pscustomobject]@{Handle=[IntPtr]::Zero}
    $geometry=@{finds=0;arranged=0;boundsReads=0;sleeps=0;pending=$true;emptyPending=$true;invalid=$false}
    function Start-Sleep([int]$Milliseconds){
        $geometry.sleeps++
        if($geometry.emptyPending){throw 'Missing control: filter-preview-example:tent_blur'}
        Microsoft.PowerShell.Utility\Start-Sleep -Milliseconds $Milliseconds
    }
    function Find([string]$Value,[switch]$Name,$Within,$Type,[switch]$Visible,[switch]$NoPopups){
        if($Value -cne 'filter-preview-example:tent_blur'){throw 'Unexpected runtime preview target'}
        $geometry.finds++;if($Visible){$geometry.arranged++}
        $geometry.bounds=if($geometry.emptyPending -or $geometry.finds -lt 3){[Windows.Rect]::Empty}elseif($geometry.invalid){[Windows.Rect]::new([double]::PositiveInfinity,8,32,32)}else{[Windows.Rect]::new(8,8,32,32)}
        $current=[pscustomobject]@{ItemStatus=$(if($geometry.pending){'Pending'}else{'Ready'});IsOffscreen=$false}
        $current|Add-Member -MemberType ScriptProperty -Name BoundingRectangle -Value {$geometry.boundsReads++;$geometry.bounds}
        [pscustomobject]@{Current=$current}
    }
    if((Preview-Hash 'example:tent_blur') -ne '' -or $geometry.finds -ne 1 -or $geometry.arranged -or $geometry.boundsReads -or $geometry.sleeps -or [CapyRuntimePreviewTest.Capture]::Captures){throw 'Pending empty runtime preview entered geometry lookup, waiting or capture'}
    $geometry.pending=$false;$geometry.emptyPending=$false;$geometry.finds=0
    $hash=Preview-Hash 'example:tent_blur'
    if($geometry.finds -lt 3 -or [Convert]::FromBase64String($hash).Length -ne 32 -or [CapyRuntimePreviewTest.Capture]::Captures -ne 1){throw 'Runtime preview did not wait for arranged geometry before hashing its actual crop'}
    $geometry.pending=$true
    if((Preview-Hash 'example:tent_blur') -ne '' -or [CapyRuntimePreviewTest.Capture]::Captures -ne 1){throw 'Pending runtime preview attempted a pixel capture'}
    $geometry.pending=$false;$geometry.invalid=$true;$failure=$null
    try{Preview-Hash 'example:tent_blur'|Out-Null}catch{$failure=$_}
    if(!$failure -or $failure.FullyQualifiedErrorId -ne 'InvalidCastIConvertible'){throw 'Runtime preview suppressed invalid geometry instead of retaining its conversion failure'}
}
Write-Output 'Runtime preview returns pending empty controls without geometry reads or waits, arranges ready controls and retains invalid crop failures.'

& {
    Add-Type -AssemblyName WindowsBase
    Add-Type -TypeDefinition @'
using System;
namespace CapyHeaderMenuTest {
 public enum TreeScope {Descendants}
 public static class Condition {public static object TrueCondition=new object();}
 public static class ControlType {public static string Button="button",Text="text",MenuItem="menuitem";}
 public static class TreeWalker {public static object RawViewWalker;}
 public static class Pointer {public static uint Dpi=96;public static uint GetDpiForWindow(IntPtr value){return Dpi;}}
}
'@
    $headerAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/LocalizationProperties.ps1'),[ref]$tokens,[ref]$errors)
    if(@($errors).Count){throw ($errors|Out-String)}
    foreach($name in @('Header-MenuCaption','Header-MenuState')){
        $function=@($headerAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name}.GetNewClosure(),$true))
        if($name -eq 'Header-MenuCaption' -and !$function.Count){continue}
        if($function.Count -ne 1){throw "Missing actual header function: $name"}
        . ([scriptblock]::Create($function[0].Extent.Text.Replace('System.Windows.Automation.','CapyHeaderMenuTest.').Replace('[CapyRowPointer]','[CapyHeaderMenuTest.Pointer]')))
    }
    $wait=@($headerAst.FindAll({param($node)$node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'Wait-Until' -and $node.Extent.Text.Contains("'Retained menu did not open its current shared command copy'")},$true))
    if($wait.Count -ne 1){throw 'Missing actual retained View-menu publication predicate'}
    $menuPredicate=[scriptblock]::Create($wait[0].CommandElements[1].ScriptBlock.EndBlock.Extent.Text.Replace('System.Windows.Automation.','CapyHeaderMenuTest.'))
    $review=[pscustomobject]@{Id=33};$script:current=[pscustomobject]@{hwnd=7};$run='header-regression-unused';$identities=@{}
    $root=[pscustomobject]@{Current=[pscustomobject]@{Name=('Retained '+[char]0xb7+' Capy Canvas')}}
    function Wait-Until([scriptblock]$Condition,[string]$Message){if(!(& $Condition)){throw $Message}}
    function Catalog-Text($Tag,$Id,$Domain){if($Id -eq 'common-app-name'){'Capy Canvas'}elseif($Id -eq 'native-header-menus'){'Menus'}else{throw 'Unexpected header copy'}}
    function Check-Identities($Ids){$script:identityChecks++}
    function Set-Content {param($LiteralPath,$Encoding,[Parameter(ValueFromPipeline)]$Value) process{}}
    function Model{$view}
    function Control([string]$Value){$nodes[$Value]}
    function Find([string]$Value,[switch]$Name,$Within,$Type,[switch]$Visible){if($Name){$headings[$Value]}elseif($Value -like 'application-menu-*' -and $script:compact){$null}else{$nodes[$Value]}}
    function Header-TestNode($Name,$Box,$Children=@(),[string]$Type='text'){
        $node=[pscustomobject]@{Current=[pscustomobject]@{Name=$Name;ProcessId=33;BoundingRectangle=$Box;IsOffscreen=$false;ControlType=$Type;ClassName=$(if($Type -eq 'text'){'TextBlock'}else{''})};Children=@($Children);Parent=$null;Key=(++$script:nodeKey)}
        foreach($child in $node.Children){$child.Parent=$node}
        $node|Add-Member ScriptMethod FindAll {param($Scope,$Condition)foreach($child in $this.Children){$child;$child.FindAll($Scope,$Condition)}}
        $node|Add-Member ScriptMethod GetRuntimeId {@(1,$this.Key)}
        $node
    }
    $walker=[pscustomobject]@{};$walker|Add-Member ScriptMethod GetParent {param($Element)$Element.Parent};[CapyHeaderMenuTest.TreeWalker]::RawViewWalker=$walker
    function Reset-Header([bool]$Compact,[int]$Dpi){
        $script:compact=$Compact;$script:current=[pscustomobject]@{hwnd=7};[CapyHeaderMenuTest.Pointer]::Dpi=$Dpi;$script:identityChecks=0;$scale=$Dpi/96.
        $specs=@(foreach($id in @('file','edit','image','layer','select','filter','view','window')){[pscustomobject]@{id=$id;label=$id}})
        $script:view=[pscustomobject]@{windows_active_tag='it';application_menus=$specs;header=[pscustomobject]@{editing=$false;model=[pscustomobject]@{zones=@(@([pscustomobject]@{id=2;item=[pscustomobject]@{kind='menu_labels'}}))}}}
        $width=if($Compact){36}else{504};$script:bounds=[pscustomobject]@{x=48;y=6;width=$width;height=36}
        $metric=[pscustomobject]@{id=2;width=$(if($Compact){704.860523223877}else{504});compact=36}
        $script:presentation=[pscustomobject]@{metrics=@($metric);geometry=[pscustomobject]@{items=@([pscustomobject]@{id=2;bounds=$bounds})};actual_items=@([pscustomobject]@{id=2;bounds=($bounds|ConvertTo-Json|ConvertFrom-Json)})}
        $script:nodes=@{'title-bar'=(Header-TestNode 'Title bar' ([Windows.Rect]::new(100,50,1600*$scale,48*$scale)));'application-menus'=(Header-TestNode 'Menus' ([Windows.Rect]::new(100+48*$scale,50+6*$scale,36*$scale,36*$scale)) @() 'button');'fit_canvas'=(Header-TestNode 'Fit' ([Windows.Rect]::new(900,200,100,30)))}
        $nodes['title-bar'].Current|Add-Member ScriptProperty ItemStatus {$script:presentation|ConvertTo-Json -Depth 12 -Compress}
        $script:headings=@{};$i=0
        foreach($spec in $specs){
            $box=[Windows.Rect]::new(100+(53+62*$i)*$scale,50+11*$scale,60*$scale,26*$scale)
            $text=Header-TestNode $spec.label ([Windows.Rect]::new($box.X+8*$scale,$box.Y+4*$scale,44*$scale,18*$scale))
            $nodes['application-menu-'+$spec.id]=Header-TestNode $spec.label $box @($text) 'button'
            $row=[Windows.Rect]::new(172,140+32*$i,200,32);$caption=Header-TestNode $spec.label ([Windows.Rect]::new(180,$row.Y+5,120,22))
            $headings[$spec.label]=Header-TestNode $spec.label $row @($caption) 'menuitem';$i++
        }
        $script:fit=[pscustomobject]@{label='Fit'}
    }
    foreach($dpi in @(96,144,192)){
        Reset-Header $true $dpi
        $current=Header-MenuState $view $identities
        if(!$current.compact -or $identityChecks -or !(& $menuPredicate) -or $current.caption_observations.Count -ne 8){throw "Compact headings did not preserve all eight current captions at $dpi DPI"}
        Reset-Header $false $dpi
        $script:current=[pscustomobject]@{hwnd=7};$record=Header-MenuState $view $identities
        if($record.compact -or $identityChecks -ne 1 -or $record.widths.Count -ne 8){throw "Expanded geometry or retained identity check was bypassed at $dpi DPI"}
        $bounds.width-=.75;$presentation.actual_items[0].bounds.width-=.75
        $near=Header-MenuState $view $identities
        if($near.compact -or $identityChecks -ne 2){throw "Near-natural .75 DIP geometry changed the expanded contract at $dpi DPI"}
    }
    Reset-Header $true 144;$current=Header-MenuState $view $identities
    $nestedText=Header-TestNode 'Fit' ([Windows.Rect]::new(900,200,100,20))
    $nested=Header-TestNode 'Fit' ([Windows.Rect]::new(892,195,180,32)) @($nestedText) 'menuitem';$nested.Parent=$headings['view']
    $headings['view'].Children+=@($nested)
    if(!(& $menuPredicate)){throw 'Compact heading validator treated expanded submenu text as its own caption'}
    foreach($bad in @('missing heading','foreign heading','overlapping heading','clipped caption','stale own caption')){
        Reset-Header $true 144;$current=Header-MenuState $view $identities
        switch($bad){
            'missing heading' {$headings.Remove('window')}
            'foreign heading' {$headings['window'].Current.ProcessId=99}
            'overlapping heading' {$headings['window'].Current.BoundingRectangle=[Windows.Rect]::new(172,140,200,32)}
            'clipped caption' {$headings['window'].Children[0].Current.BoundingRectangle=[Windows.Rect]::new(0,0,120,22)}
            'stale own caption' {$headings['window'].Children[0].Current.Name='Old window'}
        }
        if(& $menuPredicate){throw "Compact menu accepted $bad"}
    }
    foreach($bad in @('allocation mismatch','undersized compact control','wrong compact caption','inline padding')){
        Reset-Header ($bad -ne 'inline padding') 144
        switch($bad){
            'allocation mismatch' {$presentation.actual_items[0].bounds.x+=1.01}
            'undersized compact control' {$nodes['application-menus'].Current.BoundingRectangle=[Windows.Rect]::new(172,59,1,54)}
            'wrong compact caption' {$nodes['application-menus'].Current.Name='Old menus'}
            'inline padding' {$nodes['application-menu-file'].Children[0].Current.BoundingRectangle=[Windows.Rect]::new(180,72,63,27)}
        }
        $failed=$false
        try{Header-MenuState $view $identities|Out-Null}catch{if($_.Exception.Message -notlike 'Retained application menus did not present*'){throw};$failed=$true}
        if(!$failed){throw "Header accepted $bad"}
    }
}
Write-Output 'Retained HeaderMenus covers inline and compact captions and geometry without dropping hidden inline identities after expansion.'


$bubbleStampAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-tool-bubbles.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$bubbleControls=@($bubbleStampAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Controls'},$true))
$stablePixels=@($ast.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-StablePixels'},$true))
if($bubbleControls.Count -ne 1 -or $stablePixels.Count -ne 1){throw 'Missing actual bubble dab journey or pixel stability helper'}
$controlStatements=@($bubbleControls[0].Body.EndBlock.Statements)
$commitIndex=@(0..($controlStatements.Count-1)|Where-Object {$controlStatements[$_].Extent.Text -eq 'Drag $from $to'})
if($commitIndex.Count -ne 1){throw 'Missing actual bubble arc commit boundary'}
$dabPrefix=[scriptblock]::Create(($controlStatements[0..($commitIndex[0]-1)].Extent.Text -join "`n").Replace('[CapyRowPointer]','[CapyStampReadinessTest.Driver]'))
Add-Type -TypeDefinition @'
namespace CapyStampReadinessTest {
 public static class Driver {
  public static int Moves;
  public static void Down(string device,int x,int y){}
  public static void Move(int x,int y){Moves++;}
 }
}
'@
& {
    . ([scriptblock]::Create($stablePixels[0].Extent.Text))
    $Device='mouse';$Theme='dark';$Id=23
    $image=[CapyOwnedSearchTest.Node]::new('bubble-stamp-23',33,340,260)
    $image.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(340,260,144,144)
    $timing=@{now=0;publish=0;cancel=-1;canceled=$false;imageSeen=$false;stableReads=0;visibleLookups=0;halo=$false}
    function Start-Sleep([int]$Milliseconds){$timing.now+=$Milliseconds}
    function Wait-PresentedFrame{}
    function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds){
        foreach($sample in 1..20){if(& $Condition){return};Start-Sleep -Milliseconds 100}
        throw $Message
    }
    function Find([string]$Value,[switch]$Name,$Within,$Type,[switch]$Visible,[switch]$NoPopups){
        if($Value -ne 'bubble-stamp-23' -or !$Visible){throw 'Dab readiness did not request its actual visible Image'}
        $timing.visibleLookups++
        if($timing.now -ge $timing.publish){$timing.imageSeen=$true;$image}
    }
    function View([long]$Id){[pscustomobject]@{rings=@(@{enabled=$true;setting='hardness'});stamp=$(if([CapyStampReadinessTest.Driver]::Moves -and !$timing.canceled){@{center=@(-42.6666259765625,-32);diameter=96.052490234375}})}}
    function Model{[pscustomobject]@{state=@{tool_settings=@(@{id='hardness';value=$(if([CapyStampReadinessTest.Driver]::Moves -and !$timing.canceled){.8}else{.2})})}}}
    function Ring-Point([long]$Id,$Ring,[double]$Fraction){@{x=320;y=240}}
    function Relative([long]$Id,$Point){
        if($Point[0] -ne (-42.6666259765625+96.052490234375*.3) -or $Point[1] -ne -32){throw 'Dab journey changed its published sample'}
        @{x=340;y=260}
    }
    function Patch-Pixels($Point){
        $timing.stableReads++
        if($timing.canceled){if($timing.halo){'halo-after-cancel'}else{'background'}}
        elseif($timing.now -ge $timing.publish){'dab'}else{'background'}
    }
    function Capture([string]$Name,[switch]$Composed,[switch]$WithModel){if($Name -ne 'held-profile-dab' -or !$Composed -or !$WithModel){throw 'Dab journey changed its held capture'}}
    function Cancel-Contact{$timing.cancel=$timing.now;$timing.canceled=$true}
    $verdicts=@()
    foreach($case in @(@{name='delayed';publish=600;halo=$false},@{name='immediate';publish=0;halo=$false},@{name='never';publish=[int]::MaxValue;halo=$false},@{name='halo-only';publish=[int]::MaxValue;halo=$true})){
        $timing.now=0;$timing.publish=$case.publish;$timing.cancel=-1;$timing.canceled=$false;$timing.imageSeen=$false;$timing.stableReads=0;$timing.visibleLookups=0;$timing.halo=$case.halo
        [CapyStampReadinessTest.Driver]::Moves=0;$failure=$null
        try{& $dabPrefix}catch{$failure=$_}
        $verdicts+=@{case=$case.name;failure=$(if($failure){$failure.Exception.Message}else{$null});cancel_ms=$timing.cancel;image_seen=$timing.imageSeen;visible_lookups=$timing.visibleLookups;pixel_reads=$timing.stableReads}
    }
    $verdicts|ConvertTo-Json -Compress|Write-Output
    foreach($verdict in $verdicts){
        if($verdict.case -in @('never','halo-only')){
            if($verdict.failure -ne 'Missing control: bubble-stamp-23' -or $verdict.cancel_ms -ge 0 -or $verdict.pixel_reads){throw 'A never-visible dab reached cancellation or the halo-only pixel assertion'}
        }elseif($verdict.failure -or !$verdict.image_seen -or !$verdict.visible_lookups -or ($verdict.case -eq 'delayed' -and $verdict.cancel_ms -lt 600)){throw 'Dab journey canceled before observing its delayed arranged Image'}
    }
}
Write-Output 'Bubble dab journey waits for its arranged Image before stable pixels and rejects never-visible and halo-only previews.'
