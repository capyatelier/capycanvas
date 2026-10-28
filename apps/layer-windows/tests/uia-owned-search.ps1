$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$path=Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'
$ast=[Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
namespace CapyOwnedSearchTest {
 public enum TreeScope{Children,Descendants}
 public class Condition{public virtual bool Matches(Node n){return true;}}
 public class PropertyCondition:Condition {
  string property;object value;public PropertyCondition(object p,object v){property=(string)p;value=v;}
  public override bool Matches(Node n){return Object.Equals(property=="pid"?(object)n.ProcessId:property=="id"?n.AutomationId:n.Name,value);}
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
  public double VerticalScrollPercent;public int Resets,Steps;public bool VerticallyScrollable{get{return true;}}public Scroll Current{get{return this;}}
  public void SetScrollPercent(double horizontal,double vertical){VerticalScrollPercent=vertical;Resets++;}
  public void ScrollBy(ScrollAmount horizontal,ScrollAmount vertical){VerticalScrollPercent=Math.Max(0,Math.Min(100,VerticalScrollPercent+(vertical==ScrollAmount.SmallDecrement?-20:20)));Steps++;}
 }
 public class Pattern {
  public Scroll Value;public Pattern(Scroll value){Value=value;}public Scroll Current{get{return Value;}}
  public void SetScrollPercent(double horizontal,double vertical){Value.SetScrollPercent(horizontal,vertical);}
  public void Scroll(ScrollAmount horizontal,ScrollAmount vertical){Value.ScrollBy(horizontal,vertical);}
 }
 public class TreeWalker{public static TreeWalker ControlViewWalker=new TreeWalker();public Node GetParent(Node node){return node.Parent;}}
 public static class Driver{public static int GetDpiForWindow(IntPtr window){return 96;}}
 public class Node {
  public string AutomationId,Name;public int ProcessId;bool offscreen;Rect bounds;public Node Parent;public Scroll Provider,TargetScroll;public int? ContentY;
  public bool HasKeyboardFocus;public IntPtr NativeWindowHandle=new IntPtr(1);
  public Rect BoundingRectangle{get{if(ContentY.HasValue){int y=ContentY.Value-(int)(TargetScroll.VerticalScrollPercent*2);return y>=100||y+20<=0?new Rect():new Rect(0,y,80,20);}return bounds;}set{bounds=value;}}
  public bool IsOffscreen{get{return ContentY.HasValue?BoundingRectangle.IsEmpty:offscreen;}set{offscreen=value;}}
  public void SetFocus(){HasKeyboardFocus=true;}
  public bool TryGetCurrentPattern(object pattern,out object result){result=Provider==null?null:new Pattern(Provider);return result!=null;}
  public Node Current{get{return this;}}public List<Node> Children=new List<Node>();
  public Node(string id,int pid,int x,int y){AutomationId=id;Name=id;ProcessId=pid;BoundingRectangle=new Rect(x,y);}
  public Node[] FindAll(TreeScope scope,Condition condition){var found=new List<Node>();foreach(var child in Children){if(condition.Matches(child))found.Add(child);if(scope==TreeScope.Descendants)found.AddRange(child.FindAll(scope,condition));}return found.ToArray();}
  public Node FindFirst(TreeScope scope,Condition condition){var nodes=FindAll(scope,condition);return nodes.Length==0?null:nodes[0];}
 }
 public static class AutomationElement {
  public static object AutomationIdProperty="id",NameProperty="name",ControlTypeProperty="type",ProcessIdProperty="pid";
  public static Node RootElement=new Node("desktop",0,0,0);
 }
 public class ElementNotAvailableException:Exception{}
 public class Review{public int Id=33;public bool HasExited=false;public int ExitCode=0;public void Refresh(){}}
}
'@
foreach($name in @('Find','Control','Wait-Until')){
    $fn=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name}.GetNewClosure(),$true))
    if($fn.Count -ne 1){throw 'Missing actual UIA helper'}
    . ([scriptblock]::Create($fn[0].Extent.Text.Replace('System.Windows.Automation.','CapyOwnedSearchTest.')))
}
$review=[CapyOwnedSearchTest.Review]::new();$root=[CapyOwnedSearchTest.Node]::new('first-window',33,209,197)
$second=[CapyOwnedSearchTest.Node]::new('second-window',33,356,344);$popup=[CapyOwnedSearchTest.Node]::new('popup',33,0,0)
$other=[CapyOwnedSearchTest.Node]::new('drawing-canvas',33,356,344);$second.Children.Add($other)
$menu=[CapyOwnedSearchTest.Node]::new('color-swatch-edit',33,10,10);$popup.Children.Add($menu)
foreach($window in @($root,$second,$popup)){[CapyOwnedSearchTest.AutomationElement]::RootElement.Children.Add($window)}
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
if((Control 'color-swatch-edit' -Arranged) -ne $menu){throw 'Default context-menu popup lookup changed'}
$secondList=[CapyOwnedSearchTest.Node]::new('layer-list',33,356,344);$secondRow=[CapyOwnedSearchTest.Node]::new('layer-7-content',33,366,354);$secondList.Children.Add($secondRow);$second.Children.Add($secondList)
foreach($offscreen in @($false,$true)){
    $root.Children.Clear()
    if($offscreen){$hiddenList=[CapyOwnedSearchTest.Node]::new('layer-list',33,209,197);$hiddenList.IsOffscreen=$true;$root.Children.Add($hiddenList)}
    if(Find 'layer-7-content' -Visible -NoPopups){throw 'Root-only layer row escaped through recursive layer-list lookup'}
}
$root.Children.Clear();$firstList=[CapyOwnedSearchTest.Node]::new('layer-list',33,209,197);$firstRow=[CapyOwnedSearchTest.Node]::new('layer-7-content',33,219,207);$firstList.Children.Add($firstRow);$root.Children.Add($firstList)
if((Control 'layer-7-content' -Arranged -NoPopups) -ne $firstRow -or $firstRow.Current.BoundingRectangle.X -ne 219 -or $firstRow.Current.BoundingRectangle.Y -ne 207){throw 'Root-only layer row did not retain current-root bounds'}
Write-Output 'Owned UIA search passed: actual Find/Control AST waits for current-root canvas, excludes second/foreign windows and preserves default menu fallback; no UI or GPU.'

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
