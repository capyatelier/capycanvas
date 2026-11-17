$ErrorActionPreference='Stop'
function Wait-Canvas([scriptblock]$Condition,[string]$Message,[int]$Seconds,[switch]$Closing){
 $call=@($Condition,$Message);if($PSBoundParameters.ContainsKey('Seconds')){$call+=,$Seconds}
 if($Closing){Wait-Until @call -Closing}else{Wait-Until @call}
}
& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-effects.ps1'),[ref]$tokens,[ref]$errors)
 $wait=$source.Find({param($n)$n-is[Management.Automation.Language.CommandAst]-and$n.GetCommandName()-eq'Wait-Canvas'-and@($n.CommandElements|Where-Object {$_.Value-eq'Filter slider release did not finish its gesture'}).Count-eq1},$true)
 $body=$wait.Parent.Parent.Statements;$index=[array]::IndexOf(@($body),$wait.Parent)
 $sequence=($body[$index..($index+1)]|ForEach-Object {$_.Extent.Text})-join"`n"
 function Model {
  $script:filterReads++;$committed=$script:filterReads-ge3-and$script:filterCase-ne'preview'
  $stamp=@{id=1;stamp=@{checkpoint=$(if($committed){14}else{12})}}
  @{windows_tabs=@{selected=$(if($script:filterCase-eq'wrong-owner'){2}else{1});session_stamps=@(if($script:filterCase-ne'missing-stamp'){$stamp})};state=@{document_file=@{epoch=0};commands=@(@{id='undo';enabled=$true});layer_properties=@{controls=@(@{key='midtones_red';value=@{value=$(if($script:filterReads-gt3){42}elseif($committed){39}else{30})}})}}}
 }
 function Property($Key){(Model).state.layer_properties.controls|Where-Object key -eq $Key}
 function Wait-Until([scriptblock]$Condition,[string]$Message){foreach($poll in 1..4){if(& $Condition){return}};throw $Message}
 foreach($case in 'delayed','preview','wrong-owner','missing-stamp'){
  $script:filterCase=$case;$script:filterReads=0;$document=1;$epoch=0;$checkpoint=12;$released=@{value=$null};$edited=$null
  $failure=$null;try{. ([scriptblock]::Create($sequence))}catch{$failure=$_}
  if($case-eq'delayed'){if($failure-or$edited-ne39){throw "Redo reference preceded the committed filter value: edited=$edited failure=$failure"}}
  elseif(!$failure-or$failure.Exception.Message-cne'Filter slider release did not finish its gesture'-or$null-ne$edited){throw "Uncommitted or foreign filter snapshot became the redo reference: $case"}
 }
 'Filter redo reference uses the current drawing committed checkpoint, not retained Undo availability or a later model read.'
}

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/LocalizationProperties.ps1'),[ref]$tokens,[ref]$errors)
 $copy=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Copy-Panel'},$true)
 $branch=$copy.Body.Find({param($n)$n-is[Management.Automation.Language.IfStatementAst]-and$n.Extent.Text-like'*layout.collapsed*'},$true).Clauses[0].Item2
 $invoke=$branch.Statements|Where-Object {$_.Extent.Text-like'Invoke *'}|Select-Object -First 1
 $sequence=($branch.Statements|Where-Object {$_.Extent.StartOffset-lt$invoke.Extent.StartOffset}|ForEach-Object {$_.Extent.Text})-join"`n"
 $sequence=$sequence.Replace('[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)','Send-PanelContext ([uint32]$review.Id) ([uint16[]]@(0x10)) 0x79')
 function Control($Id,[switch]$Arranged){if($Id-ne'column-icon-tool_settings'-or!$Arranged){throw 'Unexpected column icon lookup'};$script:panelIcon}
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll-lt4;$poll++){if($poll-eq2-and!$script:panelLost){$script:panelIcon.Current.HasKeyboardFocus=$true};if(& $Condition){return}};throw $Message}
 function Send-PanelContext($Process,$Modifiers,$Key){if(!$script:panelIcon.Current.HasKeyboardFocus){throw 'Context key preceded column focus'};if($Process-ne33-or$Modifiers[0]-ne0x10-or$Key-ne0x79){throw 'Unexpected column context key'};$script:panelKeys++}
 $Id='tool_settings';$review=@{Id=33}
 foreach($lost in $false,$true){
  $script:panelLost=$lost;$script:panelKeys=0;$script:panelIcon=[pscustomobject]@{Current=[pscustomobject]@{HasKeyboardFocus=$false}}
  $script:panelIcon|Add-Member ScriptMethod SetFocus {}
  $failure=$null;try{. ([scriptblock]::Create($sequence))}catch{$failure=$_}
  if($lost){if(!$failure-or$failure.Exception.Message-cne'Collapsed column icon did not receive focus'-or$script:panelKeys-ne0){throw 'Missing column focus admitted a context key'}}
  elseif($failure-or$script:panelKeys-ne1){throw "Collapsed column context key did not await focus: $failure"}
 }
 'Collapsed column context: delayed focus precedes Shift+F10; missing focus prevents the key.'
}

& {
 Add-Type -TypeDefinition @'
namespace CapyShortcutFocusTest {
 public class Node {public Node Parent;}
 public class TreeWalker {
  public static TreeWalker ControlViewWalker=new TreeWalker();
  public Node GetParent(Node value){return value.Parent;}
 }
 public static class ControlType {public static object Window="window";}
 public static class AutomationElement {
  public static Node Outside=new Node(),Dialog=new Node(),Child=new Node{Parent=Dialog};
  public static int Reads;public static bool Lost;
  public static Node FocusedElement {get {Reads++;return Lost||Reads<=3?Outside:Child;}}
 }
}
'@
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-shortcut-page.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $end=$source.Find({param($n)$n-is[Management.Automation.Language.CommandAst]-and$n.GetCommandName()-eq'Wait-Until'-and@($n.CommandElements|Where-Object {$_.Value-eq'Escape on the root page did not close Preferences'}).Count-eq1},$true)
 if(!$end){throw 'Missing actual Preferences root Escape assertion'}
 $statements=@($end.Parent.Statements);$index=[array]::IndexOf($statements,$end.Parent)
 if($index-lt0){
  $statements=@($end.Parent.Parent.Statements);$index=[array]::IndexOf($statements,$end.Parent)
 }
 if($index-lt1){throw 'Missing actual root Escape sequence'}
 $first=$index-1
 if($index-ge2-and$statements[$index-2].Extent.Text-like'*Preferences did not take keyboard focus before Escape*'){$first=$index-2}
 $sequence=($statements[$first..($index-1)]|ForEach-Object {$_.Extent.Text})-join"`n"
 function Control($Id,[switch]$Name,$Type){[CapyShortcutFocusTest.AutomationElement]::Dialog}
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll-lt8;$poll++){if(& $Condition){return}};throw $Message}
 function Press($Key){
  if($Key-ne0x1b){throw 'Unexpected shortcut input'}
  if([CapyShortcutFocusTest.AutomationElement]::Reads-le3-or[CapyShortcutFocusTest.AutomationElement]::Lost){throw 'Escape preceded Preferences keyboard focus'}
  $script:shortcutEscapes++
 }
 foreach($lost in $false,$true){
  [CapyShortcutFocusTest.AutomationElement]::Reads=0;[CapyShortcutFocusTest.AutomationElement]::Lost=$lost;$script:shortcutEscapes=0
  $failure=$null;try{. ([scriptblock]::Create($sequence.Replace('System.Windows.Automation.','CapyShortcutFocusTest.')))}catch{$failure=$_}
  if($lost){if(!$failure-or$failure.Exception.Message-cne'Preferences did not take keyboard focus before Escape'-or$script:shortcutEscapes-ne0){throw 'Missing Preferences focus reached Escape'}}
  elseif($failure-or$script:shortcutEscapes-ne1){throw "Actual root Escape did not await dialog descendant focus: $failure"}
 }
 'Actual Preferences root Escape: delayed descendant focus admitted once; missing dialog focus prevents Escape.'
}

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
 public static class Clock{public static long GetTimestamp(){return 100;}}
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
foreach($name in @('Test-CapySearchItem','Find','Control','Wait-Scale','Wait-CapyCondition','Wait-Until','Invoke','Review-Ready','Input-Ready')){
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
    function Model{[pscustomobject]@{brush_ready=$true;windows_workspace=[pscustomobject]@{ready=$true}}}
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
$scopesReveal=@($scopesAst.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Reveal-Chart'},$true))
if($scopesReveal.Count -ne 1){throw 'Missing actual scopes Reveal-Chart helper'}
& {
    . ([scriptblock]::Create($scopesReveal[0].Extent.Text.Replace('System.Windows.Automation.','CapyOwnedSearchTest.').Replace('[CapyRowPointer]','[CapyOwnedSearchTest.Driver]').Replace('[Diagnostics.Stopwatch]','[CapyOwnedSearchTest.Clock]')))
    $drawingWindow=[IntPtr]1
    $viewport=[CapyOwnedSearchTest.Node]::new('panel-body-scroll-14',33,0,0)
    $viewport.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,0,100,100);$viewport.Provider=[CapyOwnedSearchTest.Scroll]::new()
    $chart=[CapyOwnedSearchTest.Node]::new('histogram-chart',33,0,60);$chart.Parent=$viewport
    $chart.BoundingRectangle=[CapyOwnedSearchTest.Rect]::new(0,60,80,40)
    $state=@{arranged=[CapyOwnedSearchTest.Rect]::new(0,60,80,80);qpc=90;waits=0}
    function Control([string]$Id){if($Id -ne 'histogram-chart'){throw 'Unexpected scopes target'};$chart}
    function Workspace-Root{[pscustomobject]@{Current=[pscustomobject]@{ItemStatus=(@{snapshot_qpc=$state.qpc}|ConvertTo-Json -Compress)}}}
    function Native-Bounds([string]$Id,[double]$Scale,$Snapshot=$null){
        if($Id -ne 'histogram-chart' -or $Scale -ne 1){throw 'Unexpected scopes native bounds request'}
        if($Snapshot -and $Snapshot.snapshot_qpc -ne $state.qpc){throw 'Scopes bounds did not use the acknowledged snapshot'}
        $state.arranged
    }
    function Start-Sleep([int]$Milliseconds){if($Milliseconds -ne 200){throw 'Unexpected scopes sleep'}}
    function Wait-Until([scriptblock]$Condition,[string]$Message){
        $state.waits++
        if(& $Condition){throw 'Scopes Reveal-Chart accepted clipped UIA bounds as its scroll acknowledgment'}
        $state.arranged=[CapyOwnedSearchTest.Rect]::new(0,16,80,80)
        foreach($qpc in @(0,90,100)){
            $state.qpc=$qpc
            if(& $Condition){throw 'Scopes Reveal-Chart accepted pre-request native bounds'}
        }
        $state.qpc=101;$state.arranged=[CapyOwnedSearchTest.Rect]::new(0,60,80,80)
        if(& $Condition){throw 'Scopes Reveal-Chart accepted a fresh clipped chart'}
        $state.arranged=[CapyOwnedSearchTest.Rect]::new(0,16,80,80)
        if(!(& $Condition)){throw $Message}
    }
    $visible=Reveal-Chart 'histogram-chart'
    if($viewport.Provider.VerticalScrollPercent -le 0){throw 'Scopes Reveal-Chart did not scroll the full native chart hidden below its clipped UIA rectangle'}
    if($state.waits -ne 1 -or $visible.Bottom -ne 100 -or $visible.Height -ne 100){throw 'Scopes Reveal-Chart did not acknowledge native chart bounds within its viewport'}
}
Write-Output 'Scopes Reveal-Chart scrolls the full native chart despite clipped UIA bounds and acknowledges its native arranged position.'
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

$pickupAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-workspace-pickup.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
& {
    foreach($definition in @($pickupAst.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.FunctionDefinitionAst]})){
        . ([scriptblock]::Create($definition.Extent.Text.Replace('System.Windows.Automation.','CapyOwnedSearchTest.').Replace('[CapyRowPointer]','[CapyOwnedSearchTest.Driver]')))
    }
    $Device='touch';$review=$null;$script:case='column-release';$down=[CapyOwnedSearchTest.Driver]::Injected;$native=@{ticks=0;heldReads=0;unavailable=0}
    $run=Join-Path ([IO.Path]::GetTempPath()) ('capy-workspace-pickup-'+[guid]::NewGuid().ToString('N'));$null=New-Item -ItemType Directory -Path $run
    function Point([string]$Id){@{x=40;y=60}}
    function Start-Sleep{param($Milliseconds)$native.ticks++}
    function Gesture{
        if([CapyOwnedSearchTest.Driver]::Injected -eq $down){return [pscustomobject]@{phase='idle';generation=69}}
        if($native.ticks -eq 1){$native.heldReads++;if($native.heldReads -gt 1){$native.unavailable++;throw [CapyOwnedSearchTest.ElementNotAvailableException]::new()}}
        [pscustomobject]@{phase=$(if($native.ticks){'held'}else{'pressed'});generation=70;source=[pscustomobject]@{kind='panel';panel='sizes'};device='touch';requires_hold=$true;captured=$true;menu_open=$native.ticks -gt 1}
    }
    try{
        Hold 'column-icon-sizes'
        $held=Get-Content -Raw (Join-Path $run 'column-release-held.json')|ConvertFrom-Json
        if([CapyOwnedSearchTest.Driver]::Injected -ne $down+1 -or $native.unavailable -ne 1 -or $held.phase -ne 'held' -or !$held.captured){throw 'Workspace hold did not record the native gesture its wait accepted'}
    }finally{Remove-Item -LiteralPath $run -Recurse -Force}
}
Write-Output 'Workspace pickup hold asserts on the gesture its wait accepted while the opening menu retires a UIA element.'

$layoutAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-workspace-layout.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$idleFps=@($layoutAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Idle-Fps'},$true))
if($idleFps.Count -ne 1){throw 'Missing actual idle Diagnostics assertion'}
& {
    . ([scriptblock]::Create($idleFps[0].Extent.Text))
    $samples=@{}
    function Renderer-Stats {
        $samples.frameReads++
        if($samples.frameReads -gt 2){throw 'Idle Diagnostics read more than its before and after snapshots'}
        if($samples.after){$samples.fpsReads++}
        [pscustomobject]@{frames=$(if($samples.after){$samples.frames}else{10});rows=@($null,$null,[pscustomobject]@{value=$(if($samples.after){$samples.fps}else{'0'})})}
    }
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
    function Has-Tint{throw 'Preview geometry control unexpectedly requested tint analysis'}
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
    function Test-CanvasProgress($Previous){$false}
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

$layerUnitTokens=$null;$layerUnitErrors=$null
$layerUnitAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/layer-relationships.ps1'),[ref]$layerUnitTokens,[ref]$layerUnitErrors)
if(@($layerUnitErrors).Count){throw 'Layers unit fixture parse failed'}
$layerUnitFunction=@($layerUnitAst.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Test-LayerUnit'},$true))
if($layerUnitFunction.Count -ne 1){throw 'Missing actual Layers intrinsic unit test'}
$layerPlainAssignment=@($layerUnitFunction[0].FindAll({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$plain'},$true))
$layerUnitGuard=@($layerUnitFunction[0].FindAll({param($n)$n -is [Management.Automation.Language.IfStatementAst] -and $n.Extent.Text.Contains('Layers unit follows realized content:')},$true))
if($layerPlainAssignment.Count -ne 1 -or $layerUnitGuard.Count -ne 1){throw 'Missing actual Layers reference assignment or unit assertion'}
foreach($layerUnitDpi in 144,96,192){foreach($layerUnitCase in 'plain','native-short','native-tall','wrong-unit'){
    & {
        param($Assignment,$Guard,$Dpi,$Case)
        $top=72;$scale=$Dpi/96.;$percent=0;$m=@{state=@{theme='light'}}
        $state=@{nativeHeight=40.;nativeCalls=0}
        if($Case -eq 'native-short'){$state.nativeHeight=39.}
        if($Case -eq 'native-tall'){$state.nativeHeight=41.}
        $measurement=@{scroll=@{unit_height=$(if($Case -eq 'wrong-unit'){41.}else{40.})}}
        function Control([string]$Id,[switch]$Arranged){
            if($Id -ne 'layer-row-72' -or !$Arranged){throw 'Layers reference lost its arranged row identity'}
            [pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=[pscustomobject]@{Height=40*$scale-1}}}
        }
        function Native-Bounds([string]$Id,[double]$Scale){
            if($Id -ne 'layer-row-72' -or $Scale -ne $Dpi/96.){throw 'Layers native reference lost its row identity or DPI'}
            $state.nativeCalls++;[pscustomobject]@{Height=$state.nativeHeight*$Scale}
        }
        $failure=$null
        if($Dpi -eq 144 -and $Case -eq 'plain'){[Console]::WriteLine('Layers reference operands: native_unit=40 DIP, UIA=59 px, native_arranged=60 px, DPI=144')}
        try{. ([scriptblock]::Create($Assignment));. ([scriptblock]::Create($Guard))}catch{$failure=$_}
        if($Case -eq 'plain'){
            if($failure){throw $failure}
            if($state.nativeCalls -ne 1 -or $plain.Height -ne 40*$scale){throw 'Layers unit did not use its actual native row extent'}
        }elseif(!$failure -or !$failure.Exception.Message.StartsWith('Layers unit follows realized content:')){
            if($failure){throw $failure};throw "Layers unit accepted $Case at $Dpi DPI"
        }
    } $layerPlainAssignment[0].Extent.Text $layerUnitGuard[0].Extent.Text $layerUnitDpi $layerUnitCase
}}
Write-Output 'Actual Layers intrinsic reference uses its native row at 100/150/200 percent, preserves the unit assertion and rejects wrong native heights or units.'

$placementAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-header-placement.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw 'Header placement fixture parse failed'}
$menuTransition=@($placementAst.FindAll({param($n)$n -is [Management.Automation.Language.IfStatementAst] -and $n.Extent.Text.Contains("Check-Caption 'menus-compact'")},$true))
if($menuTransition.Count -ne 1){throw 'Missing actual HeaderPlacement menu focus transition'}
Add-Type -TypeDefinition @'
using System;
namespace CapyHeaderPlacementTest {
 public struct Rect {
  public int X,Y,Width,Height;public int left{get{return X;}}public int top{get{return Y;}}public int right{get{return X+Width;}}public int bottom{get{return Y+Height;}}public bool IsEmpty{get{return Width==0||Height==0;}}
  public Rect(int x,int y,int width,int height){X=x;Y=y;Width=width;Height=height;}
 }
 public static class Driver {
  public static int Moves;public static bool Fits,Arranged,Delayed,Inset;public static string Focus;
  public static CapyHeaderPlacementTest.Rect Rectangle,Published,Desktop;
  public static bool MoveWindow(IntPtr window,int x,int y,int width,int height,bool repaint){
   Rectangle=new CapyHeaderPlacementTest.Rect(x,y,width,height);Moves++;if(!Delayed)Publish();return true;
  }
  public static bool GetWindowRect(IntPtr window,out Rect rectangle){rectangle=Rectangle;return true;}
  public static void Publish(){
   Published=Rectangle;
   if(Focus=="application-menu-file"&&!Wide)Focus="application-menus";
   else if(Focus=="application-menus"&&Wide)Focus="application-menu-file";
  }
  public static bool Wide{get{return Published.Width==Desktop.Width;}}
 }
 public class Screen {
  public static Screen FromHandle(IntPtr window){return new Screen();}
  public CapyHeaderPlacementTest.Rect WorkingArea{get{return Driver.Desktop;}}
 }
 public class Window {
  public Window Current{get{return this;}}
  public CapyHeaderPlacementTest.Rect BoundingRectangle{get{return Driver.Inset?new Rect(Driver.Published.X+7,Driver.Published.Y,Driver.Published.Width-14,Driver.Published.Height-7):Driver.Published;}}
 }
 public class Button {
  string id;public Button(string value){id=value;}public Button Current{get{return this;}}
  public bool HasKeyboardFocus{get{return Driver.Focus==id;}}
  public CapyHeaderPlacementTest.Rect BoundingRectangle{get{return new CapyHeaderPlacementTest.Rect(0,0,Driver.Arranged?100:0,26);}}
  public void SetFocus(){Driver.Focus=id;}
 }
}
'@
foreach($placementDpi in 96,144,192){foreach($placementCase in 'fits','delayed','does-not-fit','unarranged','frame-inset'){
    & {
        param($Text,$Dpi,$Case)
        $Text=$Text.Replace('[CapyWindowApi+Rect]::new()','[CapyHeaderPlacementTest.Rect]::new(0,0,0,0)').Replace('[CapyWindowExercise]','[CapyHeaderPlacementTest.Driver]').Replace('[CapyWindowApi]','[CapyHeaderPlacementTest.Driver]').Replace('[Windows.Forms.Screen]','[CapyHeaderPlacementTest.Screen]')
        $Text=$Text.Replace('Add-Type -AssemblyName System.Windows.Forms','$null')
        [CapyHeaderPlacementTest.Driver]::Moves=0
        [CapyHeaderPlacementTest.Driver]::Delayed=$Case -eq 'delayed';[CapyHeaderPlacementTest.Driver]::Inset=$Case -eq 'frame-inset'
        [CapyHeaderPlacementTest.Driver]::Fits=$Case -ne 'does-not-fit';[CapyHeaderPlacementTest.Driver]::Arranged=$Case -ne 'unarranged'
        [CapyHeaderPlacementTest.Driver]::Focus='';[CapyHeaderPlacementTest.Driver]::Desktop=[CapyHeaderPlacementTest.Rect]::new(-2560,30,2560,1410)
        $original=[CapyHeaderPlacementTest.Rect]::new(40,40,[int](1200*$Dpi/96.),[int](700*$Dpi/96.))
        [CapyHeaderPlacementTest.Driver]::Rectangle=$original;[CapyHeaderPlacementTest.Driver]::Published=$original
        $root=[CapyHeaderPlacementTest.Window]::new();$drawingWindow=[IntPtr]1
        $events=[Collections.Generic.List[string]]::new();$checks=[Collections.Generic.List[string]]::new();$gaps=[Collections.Generic.List[string]]::new()
        function Entries{@{id=2;item=@{kind='menu_labels'}}}
        function Model{@{application_menus=@(@{id='file'})}}
        function Presentation{
            $width=if([CapyHeaderPlacementTest.Driver]::Wide -and [CapyHeaderPlacementTest.Driver]::Fits){542.}else{36.}
            @{metrics=@(@{id=2;width=542.});geometry=@{items=@(@{id=2;bounds=@{width=$width}})}}
        }
        function Find([string]$Id,[switch]$Visible){
            if(($Id -eq 'application-menus' -and ![CapyHeaderPlacementTest.Driver]::Wide) -or ($Id -eq 'application-menu-file' -and [CapyHeaderPlacementTest.Driver]::Wide -and [CapyHeaderPlacementTest.Driver]::Fits)){[CapyHeaderPlacementTest.Button]::new($Id)}
        }
        function Control([string]$Id,[switch]$Arranged){
            if($Id -eq 'application-menu-file' -and (![CapyHeaderPlacementTest.Driver]::Wide -or ![CapyHeaderPlacementTest.Driver]::Fits)){throw 'Missing control: application-menu-file'}
            $events.Add($Id);[CapyHeaderPlacementTest.Button]::new($Id)
        }
        function Check($Condition,[string]$Name,$Data){$checks.Add($Name);if(!$Condition){throw $Name}}
        function Check-Caption([string]$Phase){$events.Add($Phase)}
        function Wait-Until([scriptblock]$Condition,[string]$Message){
            foreach($sample in 0..2){if(& $Condition){return};[CapyHeaderPlacementTest.Driver]::Publish()}
            throw $Message
        }
        function Resize-Header([int]$Width){
            if($Width -ne 800){throw 'Changed original compact transition width'}
            [CapyHeaderPlacementTest.Driver]::MoveWindow($drawingWindow,40,40,[int]($Width*$Dpi/96.),[int](700*$Dpi/96.),$true)|Out-Null
            [CapyHeaderPlacementTest.Driver]::Publish()
        }
        $failure=$null
        try{&([scriptblock]::Create($Text))}catch{$failure=$_}
        $actual=[CapyHeaderPlacementTest.Driver]::Rectangle
        if($actual.X -ne $original.X -or $actual.Y -ne $original.Y -or $actual.Width -ne $original.Width -or $actual.Height -ne $original.Height){throw 'Header menu transition did not restore its original owned rectangle'}
        if($Case -in @('fits','delayed','frame-inset')){
            if($failure){throw $failure}
            if([CapyHeaderPlacementTest.Driver]::Moves -ne 4 -or [CapyHeaderPlacementTest.Driver]::Focus -ne 'application-menus' -or $gaps.Count -or $checks -notcontains 'Menu compact transition preserves keyboard focus' -or $checks -notcontains 'Menu expansion restores keyboard focus'){throw 'Header menu transition omitted a resize or focus assertion'}
            if(($events|Where-Object {$_ -like 'menus-*'}) -join ',' -ne 'menus-wide,menus-compact,menus-expanded'){throw 'Header menu transition lost caption checks'}
        }elseif(!$failure -or $failure.Exception.Message -ne 'Desktop width does not present expanded menu labels'){
            if($failure){throw $failure};throw "Header menu transition accepted $Case"
        }
    } $menuTransition[0].Extent.Text $placementDpi $placementCase
}}
Write-Output 'Actual HeaderPlacement menu transition uses the available desktop at 100/150/200 percent, acknowledges delayed native geometry, retains focus/caption assertions, restores native outer bounds across frame insets and rejects unavailable expanded native geometry.'

$bubbleTokens=$null;$bubbleErrors=$null
$bubbleAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/BubbleVisualChecks.ps1'),[ref]$bubbleTokens,[ref]$bubbleErrors)
if(@($bubbleErrors).Count){throw ($bubbleErrors|Out-String)}

& {
    $transition=$bubbleAst.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Assert-BubbleVisualBeadTransitions'},$true)
    if(!$transition){throw 'Missing actual bead transition fixture'}
    $physicalAssignment=@($transition.FindAll({param($node)$node-is[Management.Automation.Language.AssignmentStatementAst]-and$node.Left.Extent.Text-eq'$physical'},$true))
    if($physicalAssignment.Count-ne1){throw 'Missing unique actual bead physical coordinate conversion'}
    . ([scriptblock]::Create($physicalAssignment[0].Extent.Text))
    foreach($case in @(
        @{scale=[decimal]1;origin=@{X=-200.25;Y=80.5};bounds=@{x=-3.5;y=4.25;width=36.5;height=18.25};expected=@(-203.75,84.75,36.5,18.25)},
        @{scale=[decimal]1.5;origin=@{X=40.125;Y=-100.75};bounds=@{x=2.5;y=-3.25;width=10.5;height=20.25};expected=@(43.875,-105.625,15.75,30.375)},
        @{scale=[decimal]2;origin=@{X=-10.5;Y=-20.25};bounds=@{x=-2.25;y=-4.5;width=8.125;height=6.75};expected=@(-15,-29.25,16.25,13.5)}
    )){
        $origin=[pscustomobject]$case.origin;$scale=$case.scale
        $actual=@(& $physical ([pscustomobject]$case.bounds))
        if($actual.Count-ne4){throw 'Bead physical conversion did not return four scalar coordinates'}
        foreach($axis in 0..3){if($actual[$axis]-isnot[ValueType]-or$actual[$axis]-ne$case.expected[$axis]){throw "Bead physical conversion changed coordinate $axis at scale $scale"}}
    }
    'Actual bead physical conversion preserves four scalar coordinates at 100/150/200 percent with negative and fractional geometry.'
}
Add-Type -TypeDefinition @'
namespace CapyBubbleOverlayTest {
 public static class Input {
  public static int X=100;public static bool Held=false;
  public static void Hover(int x,int y){}
  public static void Down(string device,int x,int y){}
  public static void Move(int x,int y){X=x;Held=true;}
  public static void Key(uint process,int key){Held=false;}
  public static void Cancel(){}
 }
}
'@
& {
    foreach($name in @('Assert-BubbleVisualStates','Measure-BubbleVisualOverlay')){
        $definition=@($bubbleAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name}.GetNewClosure(),$true))
        if($definition.Count -ne 1){throw "Missing actual $name fixture function"}
        . ([scriptblock]::Create($definition[0].Extent.Text.Replace('[CapyRowPointer]','[CapyBubbleOverlayTest.Input]')))
    }
    $phaseHelper=@($bubbleAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-BubbleOverlayPhase'},$true))
    if($phaseHelper.Count -eq 1){. ([scriptblock]::Create($phaseHelper[0].Extent.Text))}
    $script:overlayPhase='idle';$script:overlayMode='normal';$script:phaseReads=0;$script:pixelReads=0;$script:badColor=$false
    $script:run=Join-Path ([IO.Path]::GetTempPath()) ('capy-bubble-overlay-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($script:run)|Out-Null
    $script:review=[pscustomobject]@{Id=33};$script:scale=1;$script:w=[pscustomobject]@{X=0;Y=0}
    $script:checks=[Collections.Generic.List[string]]::new()
    function Workspace-Root {
        if($script:overlayMode -eq 'wrong-owner'){return $null}
        $id=if($script:overlayMode -eq 'wrong-id'){'tool-bubble-overlay-24'}else{'tool-bubble-overlay-23'}
        $elements=if($script:overlayMode -eq 'missing'){@()}else{@(@{id=$id;overlay_phase=$script:overlayPhase})}
        [pscustomobject]@{Current=[pscustomobject]@{ItemStatus=(@{elements=$elements}|ConvertTo-Json -Depth 5 -Compress)}}
    }
    function Wait-Until([scriptblock]$Condition,[string]$Message){
        foreach($attempt in 1..3){
            if($Message -like 'Selected bubble overlay did not apply*'){$script:phaseReads++}
            if(& $Condition){return}
            if($script:overlayMode -eq 'normal' -and $Message -match '^Selected bubble overlay did not apply (idle|hover|pressed)'){$script:overlayPhase=$Matches[1]}
        }
        throw $Message
    }
    function View([long]$Id){[pscustomobject]@{selected=$true;enabled=$true;kind='dial'}}
    function Relative([long]$Id,$Offset){if($script:outsideOverlay -and $Offset[0] -eq -10){return [pscustomobject]@{x=600;y=600}};[pscustomobject]@{x=(118+$Offset[0]);y=(107+$Offset[1])}}
    function Center([long]$Id){[pscustomobject]@{x=[CapyBubbleOverlayTest.Input]::X;y=100}}
    function Control([string]$Id,[switch]$Arranged){[pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=0}}}
    function Model{[pscustomobject]@{state=[pscustomobject]@{palette=[pscustomobject]@{text='#202020'}}}}
    function Bubble-VisualState([long]$Id){'unchanged'}
    function Bubble-VisualBounds($Element){if([CapyBubbleOverlayTest.Input]::Held){return ,@(48.,48.,104.,104.)};return ,@(50.,50.,100.,100.)}
    function Patch-Pixels($Point){'stable'}
    function Wait-BubbleNative([long]$Id,[string]$Part,[string]$Overlay,[string]$Message){
        if($Part -cne 'face'){throw 'Overlay acknowledgement lost its native face scope'}
        Wait-Until {
            $workspace=Workspace-Root;if(!$workspace){return $false}
            $elements=($workspace.Current.ItemStatus|ConvertFrom-Json).elements
            $owned=@($elements|Where-Object id -eq "tool-bubble-overlay-$Id")
            $owned.Count -eq 1 -and $owned[0].overlay_phase -ceq $Overlay
        } $Message
    }
    function Wait-BubbleVisualLayout{}
    function Read-BubbleVisualPatch($Point,[string]$Path){
        $script:pixelReads++;if(!$script:phaseReads){throw 'Overlay pixels sampled before applied phase'}
        $value=switch($script:overlayPhase){'hover'{188};'pressed'{if($script:badColor){200}else{176}};default{200}}
        return ,@((1..81)|ForEach-Object {$pixel=if($script:popupShadow -and $script:overlayPhase -eq 'pressed' -and $Point.x -eq 100 -and $Point.y -eq 100 -and $_ -gt 78){0}else{$value};@{r=$pixel;g=$pixel;b=$pixel}})
    }
    function Screen-Pixels([int]$X,[int]$Y,[string]$Path){'screen'}
    function Capture([string]$Name,[switch]$Composed,[switch]$WithModel){}
    try{
        $script:overlayMode='normal';$script:overlayPhase='idle';$script:phaseReads=0;$script:pixelReads=0
        [CapyBubbleOverlayTest.Input]::X=100;[CapyBubbleOverlayTest.Input]::Held=$false
        Assert-BubbleVisualStates 23
        if($script:phaseReads -lt 9 -or $script:pixelReads -lt 6 -or $checks -notcontains 'selected-face-hover-pressed-pixels-and-fixed-size') {throw 'Actual bubble sequence did not acknowledge delayed applied phases'}
        foreach($mode in @('wrong-owner','wrong-id','missing','wrong-phase')){
            $script:overlayMode=$mode;$script:overlayPhase='idle';$before=$script:pixelReads;$rejected=$false
            try{Wait-BubbleOverlayPhase 23 'pressed'}catch{if($_.Exception.Message -notlike 'Selected bubble overlay did not apply pressed for 23*'){throw};$rejected=$true}
            if(!$rejected -or $script:pixelReads -ne $before){throw "Overlay $mode reached pixels before applied acknowledgement"}
        }
        $script:overlayMode='normal'
        $script:overlayPhase='idle';$script:badColor=$true;$script:pixelReads=0
        [CapyBubbleOverlayTest.Input]::X=100;[CapyBubbleOverlayTest.Input]::Held=$false
        $rejected=$false
        try{Assert-BubbleVisualStates 23}catch{if($_.Exception.Message -notlike 'Selected bubble pressed pixels do not match its text-color overlay:*'){throw};$rejected=$true}
        if(!$rejected){throw 'Applied phase accepted wrong overlay pixels'}
        $script:badColor=$false;$script:popupShadow=$true;$script:outsideOverlay=$false;$script:overlayPhase='idle';$script:pixelReads=0
        [CapyBubbleOverlayTest.Input]::X=100;[CapyBubbleOverlayTest.Input]::Held=$false
        Assert-BubbleVisualStates 23
        $script:popupShadow=$false;$script:outsideOverlay=$true;$script:overlayPhase='idle';$script:pixelReads=0;$rejected=$false
        [CapyBubbleOverlayTest.Input]::X=100;[CapyBubbleOverlayTest.Input]::Held=$false
        try{Assert-BubbleVisualStates 23}catch{if($_.Exception.Message -cne 'Overlay sample patch is not inside the native face'){throw};$rejected=$true}
        if(!$rejected){throw 'Overlay sampler accepted a patch outside the native face'}
        'Actual pressed overlay excludes anchored popup shadow, keeps contact input, and rejects wrong colors and outside-face measurement.'
    }finally{Remove-Item -LiteralPath $script:run -Recurse -Force -ErrorAction SilentlyContinue}
}
Write-Output 'Actual bubble overlay sequence waits for applied Fill before composed pixels, rejects missing and wrong owned phase, and retains its pressed pixel assertion.'
& {
    foreach($name in @('Assert-BubbleHdrFocus','Measure-BubbleVisualOverlay')){
        $definition=@($bubbleAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name}.GetNewClosure(),$true))
        if($definition.Count -ne 1){throw "Missing actual $name fixture function"}
        . ([scriptblock]::Create($definition[0].Extent.Text))
    }
    $scale=1.5;$run='unused';$hdrEvidence=[Collections.Generic.List[object]]::new()
    $darkBaseline=@(@(164,149,136),@(122,92,70),@(190,140,101),@(204,149,108),@(205,150,109),@(205,150,109),@(206,151,110),@(207,151,110),@(208,152,110))
    $darkFocused=@(@(164,149,136),@(122,92,70),@(190,140,101),@(228,202,184),@(243,236,231),@(245,240,237),@(222,187,162),@(209,155,116),@(208,152,110))
    $lightBaseline=@(@(123,100,82),@(148,108,79),@(197,144,104),@(204,150,109),@(205,150,109),@(206,151,109),@(207,151,110),@(207,152,110),@(208,152,111))
    $lightFocused=@(@(123,100,82),@(148,108,79),@(192,140,102),@(83,70,64),@(61,56,56),@(67,60,57),@(186,137,102),@(207,152,110),@(208,152,111))
    function Packet([long]$Id){
        if($case -eq 'retained-light'){return @{color=@{bounds=@{x=-108.28423309326172;y=-107.78117370605469};intensity=@{point=@(-48.07900619506836,110.57414245605469);geometry=@{marker_radius=8.062493324279785}}}}}
        @{color=@{bounds=@{x=-108.2843017578125;y=-107.78123474121094};intensity=@{point=@(-48.079036712646484,110.57421875);geometry=@{marker_radius=8.062499046325684}}}}
    }
    function Relative([long]$Id,$Point){@{x=1485;y=974}}
    function Control([string]$Id,[switch]$Arranged){
        $control=[pscustomobject]@{Current=@{BoundingRectangle=@{X=$(if($case -eq 'retained-light'){1380.}else{1377.});Y=$(if($case -eq 'retained-light'){645.}else{646.})}}}
        $control|Add-Member ScriptMethod SetFocus {};return $control
    }
    function Model{@{state=@{palette=@{text=$(if($case -eq 'retained-light'){'#2e2e32'}else{'#fafafb'})}}}}
    function Wait-BubbleNative{}
    function Focus-Keyboard($Element){}
    function Assert-BubbleHdrNoTooltip([long]$Id,[string]$Stage){}
    function Start-Sleep([int]$Milliseconds){}
    function Settled-Capture([string]$Name){}
    function Read-BubbleVisualPatch($Point,[string]$Path){
        if($Point.x -ne $expectedAnchor[0] -or $Point.y -ne $expectedAnchor[1]){throw 'HDR focus capture lost its retained physical geometry'}
        $row=if($Path -like '*after.png'){$focused}else{$baseline}
        if($Path -like '*after.png' -and $case -eq 'absent'){$row=$baseline}
        if($Path -like '*after.png' -and $case -eq 'wrong-color'){$row=@((0..8)|ForEach-Object {,@(250,100,237)})}
        if($Path -like '*after.png' -and $case -eq 'wrong-alpha'){$row=@((0..8)|ForEach-Object {,@(228,200,180)})}
        if($Path -like '*after.png' -and $case -eq 'outside-radius'){$row=@($baseline|ForEach-Object {,@($_)});$row[5]=$baseline[5];$row[7]=@(245,240,237)}
        return ,@((0..80)|ForEach-Object {$rgb=$row[$_%9];@{r=$rgb[0];g=$rgb[1];b=$rgb[2]}})
    }
    function Bubble-HdrPixel($Point,[string]$Name){
        if($Name -like 'hdr-focus-marker-*'){
            if($Point.x -ne $expectedInterior[0] -or $Point.y -ne $expectedInterior[1]){throw 'HDR focus lost its retained marker interior'}
            return @{r=$(if($Name -like '*after' -and $case -eq 'marker'){1}else{0});g=0;b=0}
        }
        (Read-BubbleVisualPatch $Point ($Name+'.png'))[40]
    }
    foreach($case in @('retained','retained-light','absent','wrong-color','wrong-alpha','outside-radius','marker')){
        $baseline=if($case -eq 'retained-light'){$lightBaseline}else{$darkBaseline};$focused=if($case -eq 'retained-light'){$lightFocused}else{$darkFocused}
        $expectedAnchor=if($case -eq 'retained-light'){@(1488,973)}else{@(1485,974)};$expectedInterior=if($case -eq 'retained-light'){@(1470,973)}else{@(1467,974)}
        $failure=$null;try{Assert-BubbleHdrFocus 25}catch{$failure=$_}
        if($case -in @('retained','retained-light')){
            if($failure){throw $failure}
            $receipt=$hdrEvidence[$hdrEvidence.Count-1]
            if([Math]::Abs($receipt.overlay.maximum_error-.58823529411765) -gt 1e-10){throw 'HDR focus did not select its numeric minimum error'}
            if($receipt.overlay.alpha -ne 230/255. -or $receipt.overlay.maximum_error -gt 4 -or $receipt.marker_interior.maximum_error -ne 0){throw 'HDR focus changed its color, alpha or marker-interior oracle'}
        }else{
            $message=if($case -eq 'marker'){'HDR focus ring changed its marker interior'}else{'HDR focus ring does not use its shared marker position and GTK text overlay'}
            if(!$failure -or $failure.Exception.Message -cne $message){throw "HDR focus accepted $case or failed outside its oracle: $failure"}
        }
    }
}
Write-Output 'Actual HDR focus assertion selects the numeric minimum in both retained dark/light candidate orders and rejects absent, wrong-color, wrong-alpha, displaced rings and a changed marker interior.'


& {
    Add-Type -TypeDefinition @'
using System.Collections.Generic;
namespace CapyBubbleReopenTest {
 public static class Driver {
  public static List<string> Taps=new List<string>();public static List<int> Times=new List<int>();public static int Clock;
  public static void Tap(string device,int x,int y,int hold){Taps.Add(device+":"+x+":"+y+":"+hold);Times.Add(Clock);}
 }
}
'@
    foreach($name in 'Find-BubbleVisualRetirement','Assert-BubbleVisualTransition','Wait-BubbleVisualTransition','Invoke-BubbleVisualReopen'){
        $definition=$bubbleAst.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq$name}.GetNewClosure(),$true)
        if(!$definition){throw "Missing actual $name fixture function"}
        . ([scriptblock]::Create($definition.Extent.Text.Replace('[CapyRowPointer]','[CapyBubbleReopenTest.Driver]')))
    }
    function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll-lt5;$poll++){[CapyBubbleReopenTest.Driver]::Clock+=1000;if(& $Condition){return}};throw $Message}
    function Bubble-VisualFrames {
        $state.reads++;$count=[CapyBubbleReopenTest.Driver]::Taps.Count
        if($count-lt1-or$count-gt2){throw 'Reopen sampled native retirement outside its two taps'}
        $retiring=@{serial=2;frame=20;epoch=1;owner=100;accessible=200;bubble=27;part=$state.part;phase='retiring';value=1;target=0}
        switch($case){
            'wrong-bubble'{$retiring.bubble=28}
            'wrong-part'{$retiring.part='card-wrong'}
            'missing-epoch'{$retiring.Remove('epoch')}
            'missing-owner'{$retiring.owner=0}
        }
        $events=@()
        if([CapyBubbleReopenTest.Driver]::Clock-ge3000-and$case-ne'missing-retirement'){$events+=,$retiring}
        if($count-eq2){
            $active=@{serial=4;frame=21;epoch=1;owner=100;accessible=200;bubble=27;part=$state.part;phase='active';value=0;target=1}
            if($case-eq'wrong-epoch'){$active.epoch=2}
            if($case-eq'wrong-owner'){$active.owner=101}
            if($case-eq'completed'){$events+=,@{serial=3;frame=21;epoch=1;owner=100;accessible=200;bubble=27;part=$state.part;phase='removed';value=0;target=0};$active.owner=101;$active.accessible=201}
            $events+=,$active
        }
        @{overflow=0;transitions=$events}
    }
    foreach($part in @('card',@{kind='swatch';index=6})){
        foreach($device in 'mouse','pen','touch'){
            foreach($case in 'delayed','completed','wrong-bubble','wrong-part','missing-epoch','missing-owner','wrong-epoch','wrong-owner','missing-retirement'){
                [CapyBubbleReopenTest.Driver]::Taps.Clear();[CapyBubbleReopenTest.Driver]::Times.Clear();[CapyBubbleReopenTest.Driver]::Clock=0
                $state=@{reads=0;part=$(if($part-is[string]){$part}else{'bead:'+($part|ConvertTo-Json -Compress)})};$failure=$null;$result=$null
                try{$result=Invoke-BubbleVisualReopen 27 $part 1 $device @{x=10;y=20} @{x=30;y=40}}catch{$failure=$_}
                $taps=@([CapyBubbleReopenTest.Driver]::Taps)
                if(!$taps.Count-or$taps[0]-cne($device+':10:20:10')){throw 'Reopen changed its outside tap or contact duration'}
                if($case-in@('delayed','completed')){
                    if($failure){throw $failure}
                    if(($taps-join',')-cne($device+':10:20:10,'+$device+':30:40:0')-or$state.reads-ne4-or([CapyBubbleReopenTest.Driver]::Times-join',')-cne'0,3000'){throw 'Delayed native acknowledgement repeated or reordered pointer taps'}
                    if($result.phase-cne$(if($case-eq'completed'){'completed'}else{'interrupted'})){throw 'Delayed publication changed native lifecycle classification'}
                }else{
                    $expected=switch($case){
                        'wrong-owner'{'Interrupted native bubble retirement replaced its retained peer'}
                        {$_-in@('missing-owner','missing-epoch')}{'Native bubble retirement has no owned identity'}
                        default{'Native bubble did not record an ordered owned retirement and reopen'}
                    }
                    if(!$failure-or$failure.Exception.Message-cne$expected){throw "Reopen accepted $case or lost its native acknowledgement failure: $failure"}
                    $afterTap=$case-in@('wrong-epoch','wrong-owner')
                    if($taps.Count-ne$(if($afterTap){2}else{1})){throw 'Reopen tapped again before an owned retirement acknowledgement'}
                }
            }
        }
    }
    'Actual card and bead reopen paths accept a retirement first published at simulated 3 seconds and preserve both pointer tap packets while rejecting missing/foreign native identity.'
}


& {
    foreach($name in 'Bubble-VisualBounds','Wait-BubbleVisualBead','Bubble-VisualFrames','Find-BubbleVisualRetirement','Assert-BubbleVisualTransition','Assert-BubbleVisualAppliedPlacement','Assert-BubbleVisualOrbitReturn'){
        $definition=$bubbleAst.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq$name}.GetNewClosure(),$true)
        if(!$definition){throw "Missing actual $name fixture function"}
        . ([scriptblock]::Create($definition.Extent.Text))
    }
    function Orbit-Peer($Values,[bool]$Empty=$false){[pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=[pscustomobject]@{X=$Values[0];Y=$Values[1];Width=$Values[2];Height=$Values[3];IsEmpty=$Empty}}}}
    $transition=$bubbleAst.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Assert-BubbleVisualBeadTransitions'},$true)
    $outward=$transition.Find({param($node)$node-is[Management.Automation.Language.CommandAst]-and$node.GetCommandName()-eq'Wait-Until'-and@($node.CommandElements|Where-Object {$_.Value-eq'Held palette orbit did not move beyond its original position'}).Count-eq1},$true)
    $turnedBounds=$transition.Find({param($node)$node-is[Management.Automation.Language.AssignmentStatementAst]-and$node.Left.Extent.Text-eq'$evidence.orbit.turned_bounds'},$true)
    if(!$outward-or!$turnedBounds){throw 'Missing actual native orbit return observation sequence'}
    $observe=$bubbleAst.Extent.Text.Substring($outward.Extent.EndOffset,$turnedBounds.Extent.EndOffset-$outward.Extent.EndOffset)
    $Id=27;$script:orbitLookups=0;$script:orbitReturning=$false;$evidence=@{orbit=@{}}
    $peer=Orbit-Peer @([double]::PositiveInfinity,[double]::PositiveInfinity,[double]::NegativeInfinity,[double]::NegativeInfinity) -Empty $true
    $live=Orbit-Peer @(100.,0.,36.,36.)
    $bead=[pscustomobject]@{label='Swatch 6';placement=@{bead=@{kind='swatch';index=6};center=@(100,0);size=36}}
    function View([long]$Bubble){
        if($Bubble-ne27){throw 'Orbit observed another bubble'}
        $current=if($script:orbitReturning){[pscustomobject]@{label='Swatch 6';placement=@{bead=@{kind='swatch';index=6};center=@(20,0);size=36}}}else{$bead}
        @{beads=@($current)}
    }
    function Bubble-VisualBead([long]$Bubble,$Bead,[switch]$Pending){
        if($Bubble-ne27-or$Bead.label-ne'Swatch 6'-or$Bead.placement.bead.kind-ne'swatch'-or$Bead.placement.bead.index-ne6){throw 'Orbit reacquired another logical bead'}
        $script:orbitLookups++;$live
    }
    function Start-Sleep([int]$Milliseconds){}
    function Wait-Until($Condition,$Message){for($i=0;$i-lt5;$i++){if(& $Condition){return}};throw $Message}
    . ([scriptblock]::Create($observe))
    if($script:orbitLookups-ne1-or$peer-ne$live-or($evidence.orbit.turned_bounds-join',')-ne'100,0,36,36'){throw 'Orbit retained its retired outward peer'}
    foreach($invalid in @((Orbit-Peer @(0.,0.,36.,36.) -Empty $true),(Orbit-Peer @(0.,0.,0.,36.)),(Orbit-Peer @(0.,0.,36.,-1.)),(Orbit-Peer @([double]::NaN,0.,36.,36.)),(Orbit-Peer @(0.,[double]::PositiveInfinity,36.,36.)))){
        $failure=$null;try{$null=Bubble-VisualBounds $invalid}catch{$failure=$_}
        if(!$failure-or$failure.Exception.Message-notlike'Native bubble visual *'){throw 'Native visual geometry accepted invalid bounds'}
    }
    foreach($case in 'snap','slow','stationary','short','gap','tween','later-tween','invalid','early-wrong-size-recovers','early-wrong-target-recovers'){
        $turned=@(100.,0.,36.,36.);$maximum=0.
        $samples=@(0..2|ForEach-Object {@{frame=11+$_;ms=10.*($_+1);bounds=@(20.,0.,36.,36.);expected=@(20.,0.,36.,36.)}})
        $message=switch($case){
            'slow'{foreach($sample in $samples){$sample.ms*=100};$null}
            'stationary'{foreach($sample in $samples){$sample.bounds=$turned;$sample.expected=$turned};'Native orbit return did not acknowledge movement'}
            'short'{$samples=$samples[0..1];'Native orbit return needs at least two samples after its first movement'}
            'gap'{$samples[1].frame=13;$samples[2].frame=14;'Native orbit return did not retain ordered distinct frames'}
            'tween'{$samples[0].bounds=@(50.,0.,36.,36.);'Returning inside press slop animated native bead bounds instead of snapping:*'}
            'early-wrong-target-recovers'{$samples[0].bounds=@(50.,0.,36.,36.);'Returning inside press slop animated native bead bounds instead of snapping:*'}
            'early-wrong-size-recovers'{$samples[0].bounds=@(20.,0.,48.,48.);'Returning inside press slop animated native bead bounds instead of snapping:*'}
            'later-tween'{$maximum=3.;'Returning inside press slop animated native bead bounds instead of snapping:*'}
            'invalid'{$samples[0].bounds[2]=[double]::NaN;'Native orbit return retained invalid frame geometry'}
        }
        $failure=$null;try{$result=Assert-BubbleVisualOrbitReturn $samples $turned $maximum}catch{$failure=$_}
        if($case-in@('snap','slow')){if($failure){throw $failure};if($result.maximum_post_change_error_px-ne0-or$result.post_change_samples-ne2){throw 'Native orbit lost exact first-frame and two-later-frame geometry'}}
        elseif(!$failure-or$failure.Exception.Message-notlike$message){throw "Native orbit accepted $case return geometry: $failure"}
    }
    $returnWait=$transition.Find({param($node)$node-is[Management.Automation.Language.CommandAst]-and$node.GetCommandName()-eq'Wait-Until'-and@($node.CommandElements|Where-Object {$_.Value-eq'Native bead did not follow the held orbit back inside the starting slop'}).Count-eq1},$true)
    if(!$returnWait){throw 'Missing actual shared-placement frame acknowledgement'}
    $script:orbitReturning=$true
    $binding=@{after=10;epoch=1;source='{"kind":"control"}';part='bead:{"kind":"swatch","index":6}';bubble=27;target=@(20,0);size=36;scale=1}
    function Workspace-Root{
        $script:orbitReads++
        $samples=@(0..2|ForEach-Object {@{frame=11+$_;bounds=@{x=20;y=0;width=36;height=36};expected=@{x=20;y=0;width=36;height=36}}})
        $row=@{token=11;applied_frame=10;first_frame=11;last_frame=13;count=[Math]::Min(3,$script:orbitReads);bubble=27;part='bead:{"kind":"swatch","index":6}';target=@(20,0);size=@(36,36);epoch=1;source=@{kind='control'};active=$true;owner=100;accessible=200;identity_changed=$false;scale=1;samples=$samples;maximum_error_px=0}
        switch($script:orbitSchedule){
            'wrong-bead'{$row.part='bead:{"kind":"palette","index":6}'};'wrong-target'{$row.target=@(40,0)};'stale'{$row.token=1};'short'{$row.count=2}
            'wrong-size'{$row.size=@(40,40)};'wrong-source'{$row.source=@{kind='medium'}};'wrong-epoch'{$row.epoch=2}
            'late-first'{$row.first_frame=12;$row.last_frame=14;foreach($sample in $samples){$sample.frame++}}
            'missing-middle'{$row.last_frame=14};'owner-recovered'{$row.identity_changed=$true};'scale'{$row.scale=1.5}
        }
        [pscustomobject]@{Current=@{ItemStatus=(@{bubble_frames=@{frame=14;serial=20;placement_serial=11;overflow=0;layouts=@($row)}}|ConvertTo-Json -Depth 12 -Compress)}}
    }
    foreach($script:orbitSchedule in 'slow','wrong-bead','wrong-target','stale','short','wrong-size','wrong-source','wrong-epoch','late-first','missing-middle','owner-recovered','scale'){
        $script:orbitReads=0;$observed=@{receipt=$null;bead=$null};$failure=$null
        try{. ([scriptblock]::Create($returnWait.Extent.Text))}catch{$failure=$_}
        if($script:orbitSchedule-eq'slow'){if($failure){throw $failure};if($script:orbitReads-ne3-or$observed.receipt.count-ne3){throw 'Orbit did not wait for three frames of the returned shared placement'}}
        else{
            $expected=switch($script:orbitSchedule){
                {$_-in@('wrong-size','wrong-source','wrong-epoch')}{'Native orbit frame changed its applied shared placement identity or size'}
                {$_-in@('late-first','missing-middle')}{'Native orbit frame evidence omitted its first applied frames'}
                {$_-in@('owner-recovered','scale')}{'Native orbit return changed its native owner, peer or scale'}
                default{'Native bead did not follow the held orbit back inside the starting slop'}
            }
            if(!$failure-or$failure.Exception.Message-cne$expected){throw "Orbit accepted $script:orbitSchedule acknowledgement: $failure"}
        }
    }
    foreach($case in 'interrupted','completed','completed-address-reused','early-replace','wrong-peer','wrong-epoch','incomplete-removal','overflow'){
        $retiring=@{serial=2;frame=20;epoch=1;owner=100;accessible=200;bubble=27;part='card';phase='retiring';value=1;target=0}
        $active=@{serial=4;frame=21;epoch=1;owner=100;accessible=200;bubble=27;part='card';phase='active';value=0;target=1}
        $events=@($retiring);$overflow=0
        switch($case){
            {$_-in@('completed','completed-address-reused','incomplete-removal')}{$events+=@{serial=3;frame=21;epoch=1;owner=100;accessible=200;bubble=27;part='card';phase='removed';value=0;target=0}}
        }
        switch($case){
            'completed'{$active.owner=101;$active.accessible=201}
            'early-replace'{$active.owner=101}
            'wrong-peer'{$active.accessible=201}
            'wrong-epoch'{$active.epoch=2}
            'incomplete-removal'{$events[1].value=.5}
            'overflow'{$overflow=1}
        }
        $failure=$null;try{$result=Assert-BubbleVisualTransition @{overflow=$overflow;transitions=@($events)+@($active)} 27 'card' 1}catch{$failure=$_}
        if($case-in@('interrupted','completed','completed-address-reused')){
            if($failure){throw $failure};$phase=if($case-eq'interrupted'){'interrupted'}else{'completed'}
            if($result.phase-cne$phase-or$result.peer_reused-ne($phase-eq'interrupted')){throw 'Native bubble transition changed its identity contract'}
        }elseif(!$failure){throw "Native bubble transition accepted $case"}
    }
    foreach($maximum in @($null,[double]::NaN,[double]::PositiveInfinity,-1)){
        $failure=$null;try{Assert-BubbleVisualOrbitReturn @(@{frame=1;bounds=@(20,0,36,36);expected=@(20,0,36,36)}) @(100,0,36,36) $maximum}catch{$failure=$_}
        if(!$failure-or$failure.Exception.Message-cne'Native orbit return omitted its all-frame geometry bound'){throw 'Native orbit accepted missing or invalid all-frame evidence'}
    }
    $coverage=$transition.Find({param($node)$node-is[Management.Automation.Language.InvokeMemberExpressionAst]-and$node.Member.Extent.Text-eq'Add'-and$node.Extent.Text.Contains('native-palette-bead-')},$true)
    if(!$coverage){throw 'Missing actual live bead lifecycle coverage label'}
    $checks=[Collections.Generic.List[string]]::new()
    foreach($phase in 'interrupted','completed'){
        $evidence=@{revival=@{transition=@{phase=$phase}}};. ([scriptblock]::Create($coverage.Extent.Text))
    }
    if(($checks -join ',') -cne 'native-palette-bead-interrupted-retirement-and-exact-state-restoration,native-palette-bead-completed-retirement-and-exact-state-restoration'){throw 'Completed live bead retirement claimed an interrupted within-duration revival'}
    'Actual orbit frame acknowledgement preserves shared identity, first-frame snap, all-frame error and peer lifecycle across slow reads.'
}

& {
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-zoom-readout.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $actual=$source.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Navigation-Double'},$true)
    . ([scriptblock]::Create($actual.Extent.Text))
    function Navigation-Command($Command){}
    function Navigation-Selected($Command){$true}
    function Navigation-Focus{}
    function Camera{@{zoom=2;rotation=1}}
    function Near($First,$Second){$false}
    function Changed-Camera($Before,$Message,$Condition){Camera}
    function Wait-Until($Condition,$Message){if(!(& $Condition)){throw $Message}}
    function Tool-Tile($Command){$script:requestedNavigationTile=$Command;'tile-'+$Command}
    function Control($Id,[switch]$Arranged){throw 'navigation tile reached'}
    function Model{throw 'Navigation tile control unexpectedly read a model'}
    function Center{throw 'Navigation tile control unexpectedly measured a center'}
    function Assert-Camera{throw 'Navigation tile control unexpectedly compared cameras'}
    function Capture{throw 'Navigation tile control unexpectedly captured pixels'}
    $script:requestedNavigationTile=$null
    try{Navigation-Double 'zoom';throw 'Navigation did not request its tile'}catch{if($_.Exception.Message-ne'navigation tile reached'){throw}}
    if($script:requestedNavigationTile-ne'zoom'){throw 'Zoom double action selected another tool tile'}
    $cases=[Collections.Generic.List[string]]::new()
    function Navigation-Double($Command,$HeaderId=0){$cases.Add("${Command}:$HeaderId")}
    $headerId=71
    $loop=$source.Find({param($node)
        $node-is[Management.Automation.Language.ForEachStatementAst]-and
        $null-ne$node.Body.Find({param($child)$child-is[Management.Automation.Language.CommandAst]-and$child.GetCommandName()-eq'Navigation-Double'},$true)
    },$true)
    . ([scriptblock]::Create($loop.Extent.Text))
    if(($cases-join',')-ne'zoom:0,rotate_view:0,rotate_view:71,hand:0,hand:71'){throw 'Navigation did not cover each tile and its supported header action'}
}

& {
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-tool-variations.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $slot=@{id=43};$choices=@(@{label='Lasso';enabled=$true},@{label='Rectangle';enabled=$false})
    $published=@{state=@{customization=@{drawer=@{anchor=@{tile=43};tool_set=@{groups=@();subtools=$choices}}}}}
    function Model{$published}
    function Wait-Until($Condition,$Message){if(!(& $Condition)){throw $Message}}
    $wait=$source.Find({param($node)$node-is[Management.Automation.Language.CommandAst]-and$node.GetCommandName()-eq'Wait-Until'-and@($node.CommandElements|Where-Object {$_.Value-eq'Grouped tool did not open its scoped drawer'}).Count-eq1},$true)
    . ([scriptblock]::Create($wait.Extent.Text))
    $assignment=$source.Find({param($node)$node-is[Management.Automation.Language.AssignmentStatementAst]-and$node.Left-is[Management.Automation.Language.VariableExpressionAst]-and$node.Left.VariablePath.UserPath-in'groups','subtools'},$true)
    . ([scriptblock]::Create($assignment.Extent.Text))
    $drawer=@{};$seen=[Collections.Generic.List[string]]::new()
    function Control($Id,$Within,[switch]$Arranged){
        if($Id-notmatch'^tool-subtool-(\d+)$'){throw 'Selection drawer requested a group button'}
        $seen.Add($Id);$choice=$choices[[int]$Matches[1]]
        @{Current=@{Name=$choice.label;IsEnabled=$choice.enabled}}
    }
    $loop=$source.Find({param($node)$node-is[Management.Automation.Language.ForStatementAst]-and$null-ne$node.Body.Find({param($child)$child-is[Management.Automation.Language.ThrowStatementAst]-and$child.Extent.Text-like'*Drawer did not present the shared sibling choices*'},$true)},$true)
    . ([scriptblock]::Create($loop.Extent.Text))
    if(($seen-join',')-ne'tool-subtool-0,tool-subtool-1'){throw 'Selection drawer did not check every shared sibling row'}
}

& {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
public static class CapyImageDragPacingTest {
 public static int Clock,Last,Cancelled;
 public static bool FailMove;
 public static List<string> Events=new List<string>();
 public static void Down(string device,int x,int y){Last=Clock;Events.Add("down:"+device+":"+x+","+y);}
 public static void Move(int x,int y){if(Clock-Last<8)throw new InvalidOperationException("Unpaced image drag");Last=Clock;if(FailMove)throw new InvalidOperationException("Owned injection refused");Events.Add("move:"+x+","+y);}
 public static void Up(){if(Clock-Last<8)throw new InvalidOperationException("Unpaced image release");Events.Add("up");}
 public static void Cancel(){Cancelled++;}
 public static void Reset(){Clock=Last=Cancelled=0;FailMove=false;Events.Clear();}
}
'@
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-image-rows.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $actual=$source.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Drag-Image'},$true)
    . ([scriptblock]::Create($actual.Extent.Text.Replace('[CapyRowPointer]','[CapyImageDragPacingTest]')))
    function Start-Sleep([int]$Milliseconds){[CapyImageDragPacingTest]::Clock+=$Milliseconds}
    foreach($Device in 'mouse','pen','touch'){
        [CapyImageDragPacingTest]::Reset()
        Drag-Image @(100,200) @(180,240)
        $expected="down:${Device}:100,200,move:110,205,move:120,210,move:130,215,move:140,220,move:150,225,move:160,230,move:170,235,move:180,240,up"
        if(([CapyImageDragPacingTest]::Events-join',')-ne$expected-or[CapyImageDragPacingTest]::Cancelled-ne1){throw 'Image drag changed its native path or cleanup'}
    }
    [CapyImageDragPacingTest]::Reset();[CapyImageDragPacingTest]::FailMove=$true;$rejected=$false
    try{Drag-Image @(100,200) @(180,240)}catch{if($_.Exception.ToString()-notlike'*Owned injection refused*'){throw};$rejected=$true}
    if(!$rejected-or[CapyImageDragPacingTest]::Cancelled-ne1-or[CapyImageDragPacingTest]::Events.Count-ne1){throw 'Refused image injection retried or escaped cleanup'}
}

& {
    $tokens=$null;$errors=$null
    $path=Join-Path $PSScriptRoot '../scripts/LocalizationProperties.ps1'
    $source=[Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $notice=$source.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Notice-Surface'},$true)
    if(!$notice){throw 'Notice-Surface is missing'}
    $guard=@($notice.Body.FindAll({param($node)$node-is[Management.Automation.Language.IfStatementAst]-and$node.Clauses.Count-eq1-and$node.Clauses[0].Item1.Extent.Text-eq'$NoticeOnly'},$true))
    if($guard.Count-ne1){throw 'Italian memory refusal stage is not gated to NoticeOnly'}
    $italian=$guard[0].Find({param($node)$node-is[Management.Automation.Language.AssignmentStatementAst]-and$node.Extent.Text-like'$italian=1+*'},$true)
    $drawing=$guard[0].Find({param($node)$node-is[Management.Automation.Language.CommandAst]-and$node.GetCommandName()-eq'New-DrawingPixels'},$true)
    $flat=$guard[0].Find({param($node)$node-is[Management.Automation.Language.CommandAst]-and$node.GetCommandName()-eq'Menu-Command'-and$node.Extent.Text-like"*'flat_all_areas'*"},$true)
    if(!$italian-or!$drawing-or!$flat){throw 'NoticeOnly gate did not contain the complete Italian drawing refusal stage'}
}

& {
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $helper=$source.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Assert-NoDrawingMemoryRefusal'},$true)
    if(!$helper){throw 'Drawing memory refusal helper is missing'}
    . ([scriptblock]::Create($helper.Extent.Text))
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-localization.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $idle=$source.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Idle'},$true)
    if(!$idle){throw 'Localization document wait is missing'}
    . ([scriptblock]::Create($idle.Extent.Text))
    $script:refusalView=$null
    function Model {$script:refusalView}
    function Catalog-Text([string]$Tag,[string]$Id,[string]$Domain){
        if($Domain-ne'documents'){throw 'Wrong catalog domain'}
        switch("$Tag/$Id"){
            'it/documents-refusal-admission-budget' {'it-budget';return}
            'it/documents-too-large-memory' {'it-size';return}
            default {throw 'Unexpected refusal catalog lookup'}
        }
    }
    function Wait-Until([scriptblock]$Check,[string]$Message,[int]$Seconds=0){if(!(& $Check)){throw $Message}}
    function Refusal-View([string]$Text,[bool]$Busy){
        [pscustomobject]@{windows_active_tag='it';state=[pscustomobject]@{
            notice=$(if($Text){[pscustomobject]@{text=$Text}}else{$null})
            new_drawing=[pscustomobject]@{page=[pscustomobject]@{memory='78 MB available'}}
            document_file=[pscustomobject]@{busy=$Busy}
            canvas_status="Too large for this device's available memory"
        }}
    }
    function Expect-IdleError([string]$Expected){
        $actual=$null
        try{Idle}catch{$actual=$_.Exception.Message}
        if($actual-cne$Expected){throw "Expected '$Expected', got '$actual'"}
    }
    $script:refusalView=Refusal-View 'it-size' $true;Expect-IdleError 'Memory refusal: it-size; 78 MB available'
    $script:refusalView=Refusal-View 'it-budget' $false;Expect-IdleError 'Memory refusal: it-budget; 78 MB available'
    $script:refusalView=Refusal-View 'en-size' $true;Expect-IdleError 'Document operation did not finish'
    $script:refusalView=Refusal-View 'other-notice' $true;Expect-IdleError 'Document operation did not finish'
    $script:refusalView=Refusal-View $null $true;Expect-IdleError 'Document operation did not finish'
    $script:refusalView=Refusal-View $null $false;Idle
}

& {
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-vector-layers.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $actual=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Sketch-VectorRoutes'},$true)
    . ([scriptblock]::Create($actual.Extent.Text.Replace('[System.Windows.Automation.TogglePattern]','[CapyOwnedSearchTest.TogglePattern]')))
    function Capture{throw 'Workspace route control unexpectedly captured pixels'}
    function Use-PaintWorkspace{throw 'Workspace route control unexpectedly returned to Paint'}
    function Reason-Hover{throw 'Workspace route control unexpectedly continued past its route'}
    function Shortcut-Command{throw 'Workspace route control unexpectedly continued past its route'}
    function Click{throw 'Workspace route control unexpectedly continued past its route'}
    function Selected{throw 'Workspace route control unexpectedly continued past its route'}
    function Key{throw 'Workspace route control unexpectedly continued past its route'}
    function Park{throw 'Workspace route control unexpectedly continued past its route'}
    function Take-Projection{throw 'Workspace route control unexpectedly continued past its route'}
    function Active{throw 'Workspace route control unexpectedly continued past its route'}
    function Fit-Canvas{throw 'Workspace route control unexpectedly fit the canvas'}
    $toggle=[pscustomobject]@{};$toggle|Add-Member ScriptMethod Toggle {}
    $lock=[pscustomobject]@{};$lock|Add-Member ScriptMethod GetCurrentPattern {param($p)$toggle}
    function Control($Id,[switch]$Arranged){if($Id-ne'layer-lock'){throw 'Unexpected control before workspace route'};$lock}
    function Model{@{state=@{layer_tools=@{editing_layer=@{locked=$true}}}}}
    function Wait-Until($Condition,$Message){if(!(& $Condition)){throw $Message}}
    function Find($Id,[switch]$Visible){$null}
    function Invoke($Id){throw 'One-shot compact workspace fallback executed'}
    function Use-Workspace($Id,$Name){if($Id-ne'builtin:workspace:painter'-or$Name-ne'Sketch'){throw 'Wrong shared workspace route'};throw 'Shared Sketch route reached'}
    $routed=$false;try{Sketch-VectorRoutes 7}catch{if($_.Exception.Message-ne'Shared Sketch route reached'){throw};$routed=$true}
    if(!$routed){throw 'Vector fixture did not use shared workspace readiness'}
}

& {
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-multiwindow.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    foreach($name in 'Ready','Use-Window'){
        $actual=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq$name},$true)
        . ([scriptblock]::Create($actual.Extent.Text.Replace('[CapyWindowTest]','[CapyOwnedSearchTest.Driver]')))
    }
    $review=@{Id=33};$directory='owned-registry';$window=@{id=9;hwnd=77};$seen=@{reads=0;polls=0}
    function Model($Window){@{brush_ready=$true;windows_workspace=@{ready=$true}}}
    function Owned-ProcessWindows($Process,$WindowHandle){
        if($Process.Id-ne33-or$WindowHandle-ne77){throw 'Review readiness queried another window'}
        @{Root=[CapyOwnedSearchTest.Node]::new('owned-root',33,0,0)}
    }
    function Owned-DrawingWindow($Process,$WindowHandle,$TraceDirectory){
        if($Process.Id-ne33-or$WindowHandle-ne77-or$TraceDirectory-ne'owned-registry'){throw 'Ready queried another window registry'}
        $seen.reads++;if($seen.reads-ge3){@{Root='owned-root'}}
    }
    function Wait-Until($Condition,$Message,$Seconds){for($i=0;$i-lt3;$i++){$seen.polls++;if(& $Condition){return}};throw $Message}
    Ready $window
    if($seen.reads-ne3-or$seen.polls-ne3){throw 'Ready accepted UI state before owned registry readiness'}
}

& {
    $tokens=$null;$errors=$null
    $documents=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-documents.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $admissions=[ordered]@{'Review-Ready'=@{wait={Review-Ready};admit='review'};'Input-Ready'=@{wait={Input-Ready};admit='input'}}
    foreach($message in 'Light review did not start','Canvas startup did not finish','Session did not reopen','Untitled canvas did not finish starting'){
        $wait=@($documents.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Wait-Canvas' -and $n.CommandElements.Count -ge 3 -and $n.CommandElements[2].Value -ceq $message}.GetNewClosure(),$true))
        if($wait.Count -ne 1){throw "Missing actual documents startup admission: $message"}
        $admissions[$message]=@{wait=$wait[0].CommandElements[1].ScriptBlock.GetScriptBlock();admit='input'}
    }
    function Model{[pscustomobject]@{brush_ready=$true;windows_workspace=[pscustomobject]@{ready=$state.workspace};launch=[pscustomobject]@{visible=$state.launch};state=[pscustomobject]@{theme='light'};windows_recovery=[pscustomobject]@{busy=$false;restoring=$false}}}
    function Owned-ProcessWindows($Process){if($Process.Id -ne 33){throw 'Review readiness queried another process'};foreach($window in $state.windows){[pscustomobject]@{Root=$window;Handle=$window.NativeWindowHandle}}}
    function Drawing([bool]$Cover,[bool]$Offscreen,[bool]$Enabled){
        $window=[CapyOwnedSearchTest.Node]::new('drawing-window',33,209,197)
        if($Cover){$splash=[CapyOwnedSearchTest.Node]::new('launch-splash',33,209,197);$splash.IsOffscreen=$Offscreen;$window.Children.Add($splash)}
        $canvas=[CapyOwnedSearchTest.Node]::new('drawing-canvas',33,209,240);$canvas.Enabled=$Enabled;$window.Children.Add($canvas)
        $window
    }
    $ready=Drawing $false $false $true
    $failures=[Collections.Generic.List[string]]::new()
    foreach($state in @(
        @{name='brush ready while the workspace is unready';workspace=$false;launch=$false;windows=@($ready);review=$false;input=$false},
        @{name='shared ready while the launch is visible';workspace=$true;launch=$true;windows=@(Drawing $true $false $false);review=$false;input=$false},
        @{name='shared launch hidden while the native cover takes clicks';workspace=$true;launch=$false;windows=@(Drawing $true $false $true);review=$false;input=$false},
        @{name='a modal prompt disables the native drawing canvas';workspace=$true;launch=$false;windows=@(Drawing $false $false $false);review=$true;input=$false},
        @{name='another owned window still covered';workspace=$true;launch=$false;windows=@($ready,(Drawing $true $false $true));review=$false;input=$false},
        @{name='no registered drawing window';workspace=$true;launch=$false;windows=@();review=$false;input=$false},
        @{name='offscreen native cover';workspace=$true;launch=$false;windows=@(Drawing $true $true $true);review=$true;input=$true},
        @{name='every clause met';workspace=$true;launch=$false;windows=@($ready);review=$true;input=$true}
    )){
        foreach($admission in $admissions.GetEnumerator()){
            $admit=$state[$admission.Value.admit]
            if([bool](& $admission.Value.wait) -ne $admit){$failures.Add("$($admission.Key) $(if($admit){'rejected'}else{'admitted'}) $($state.name)")}
        }
    }
    if($failures.Count){throw ($failures -join [Environment]::NewLine)}
    'Review readiness waits for shared brush and workspace readiness, the shared launch, a registered drawing window and the native launch cover; input admission and the documents startups also wait for the enabled native drawing canvas.'
}

& {
 Add-Type -TypeDefinition @'
public static class CapyEdgeReleaseTest {
 public static int Phase,Ups,Downs;
 public static void Down(string device,int x,int y){Phase=1;Downs++;}
 public static void Move(int x,int y){}
 public static void Hover(int x,int y){}
 public static void Up(){Phase=2;Ups++;}
}
'@
 $tokens=$null;$errors=$null
 $helpers=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $release=$helpers.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Wait-NativeGestureRelease'},$true)
 if($release){. ([scriptblock]::Create($release.Extent.Text))}
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-edge-reach.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 foreach($name in 'Swipe','Settled'){
  $fn=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq$name}.GetNewClosure(),$true)
  if(!$fn){throw "Missing actual edge function $name"}
  . ([scriptblock]::Create($fn.Extent.Text.Replace('[CapyRowPointer]','[CapyEdgeReleaseTest]')))
 }
 function Screen($X,$Y){@{x=$X;y=$Y}}
 function Start-Sleep([int]$Milliseconds){}
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll-lt10;$poll++){if(& $Condition){return}};throw $Message}
 function Model{
  $script:edgeLayoutReads++
  [pscustomobject]@{state=[pscustomobject]@{workspace=[pscustomobject]@{layout='constant'}}}
 }
 function Gesture{
  $phase=[CapyEdgeReleaseTest]::Phase
  $ended=$script:edgeEscaped
  if($phase-eq2){$script:edgeReleasePolls++;$ended=$ended-or(!$script:edgeLost-and$script:edgeReleasePolls-gt3)}
  if($ended){$script:edgeDelivered=$true}
  $edge=if($script:edgeMode-ne'root'-and$phase-gt0-and!$ended){@{claimed=$true;serial=11}}else{$null}
  $reply=if($ended){@{serial=11;phase=$(if($script:edgeEscaped){'cancel'}else{'up'});time=$(if($script:edgeEscaped){10}else{20})}}else{@{serial=11;phase='move';time=10}}
  if($phase-eq2-and$script:edgeStale){$edge=$null;$reply=@{serial=10;phase='cancel';time=9}}
  if($phase-eq2-and$script:edgeMode-eq'same-time-up'){$edge=$null;$reply=@{serial=11;phase='up';time=10};$script:edgeDelivered=$true}
  if($phase-eq2-and$script:edgeMode-eq'stale-up'){$edge=$null;$reply=@{serial=10;phase='up';time=30}}
  @{edge=$edge;pointer_id=$(if($phase-gt0-and!$ended){1}else{$null});edge_reply=$reply;edge_contact=@{time=$(if($phase-gt0-and$script:edgeMode-ne'root'){2}else{1})};generation=$(if($phase-gt0){6}else{5});last_down=@{left=$true;in_contact=($phase-eq1);position=@(10,20);pointer=1};last_release=@{generation=$(if($ended-and!$script:edgeEscaped){6}else{5})};last_cancel=@{generation=$(if($ended-and$script:edgeEscaped){7}else{0})}}
 }
 $Device='mouse'
 foreach($mode in 'edge','root','same-time-up','escape','stale-up','stale-edge','lost-edge','lost-root'){
  [CapyEdgeReleaseTest]::Phase=0;[CapyEdgeReleaseTest]::Ups=0;[CapyEdgeReleaseTest]::Downs=0
  $script:edgeMode=if($mode-eq'lost-root'){'root'}else{$mode}
  $script:edgeStale=$mode-eq'stale-edge';$script:edgeLost=$mode-like'lost-*'-or$script:edgeStale-or$mode-eq'stale-up';$script:edgeEscaped=$false;$script:edgeDelivered=$false;$script:edgeReleasePolls=0;$script:edgeLayoutReads=0
  $during=if($mode-eq'escape'){{ $script:edgeEscaped=$true;$null=Gesture }}else{$null}
  $failure=$null;try{Swipe @{x=10;y=20} @{x=30;y=40} -Steps 2 -During $during}catch{$failure=$_}
  if([CapyEdgeReleaseTest]::Downs-ne1-or[CapyEdgeReleaseTest]::Ups-ne1){throw 'Swipe repeated an injected contact'}
  if($script:edgeLost){
   if(!$failure-or$failure.Exception.Message-cne'Native gesture release was not acknowledged'-or$script:edgeLayoutReads-ne0){throw 'Stable layout accepted a missing terminal release'}
  }elseif($failure-or!$script:edgeDelivered-or$script:edgeLayoutReads-ne4){throw "Swipe returned before native release acknowledgement: $mode; $failure"}
 }
 'Actual Swipe: stable layout cannot acknowledge delayed or missing release; native edge, root divider and acknowledged Escape settle once after their terminal trace.'
 $script:noContactPolls=0
 function Gesture{
  $script:noContactPolls++
  @{edge=$null;pointer_id=$(if($script:noContactPolls-lt3){1}else{$null});last_release=@{generation=$(if($script:noContactPolls-lt3){5}else{6})};last_cancel=@{generation=0}}
 }
 Wait-NativeGestureRelease @{edge=$null;pointer_id=$null;phase='finishing'} 5
 if($script:noContactPolls-ne0){throw 'A contact already absent at release polled Gesture'}
 Wait-NativeGestureRelease @{edge=$null;pointer_id=1;phase='idle'} 5
 if($script:noContactPolls-ne3){throw 'A live root pointer did not await its terminal generation'}
 'Release acknowledgement: absent edge and pointer return without polling; a live root pointer still waits for terminal generation.'
}

& {
 Add-Type -TypeDefinition @'
public static class CapyEdgeSweepTest {
 public struct Point {public int x,y;}
 public static bool GetCursorPos(out Point point){point=new Point{x=X,y=Y};return true;}
 public static int Phase,Downs,Cancels,Escapes,Moves,X,Y,StartX;public static bool Active,SharedClaimed,Committed;public static string Device;
 public static void Down(string device,int x,int y){Phase=1;Active=true;Downs++;Device=device;X=device=="mouse"?x-1:x;StartX=X;Y=y;}
 public static void Move(int x,int y){Moves++;X=Device=="mouse"?x+1:x;Y=y;}
 public static void Cancel(){Committed=Device=="mouse"&&SharedClaimed&&Escapes==0;Phase=2;Active=false;Cancels++;}
 public static void Key(int key){if(key!=27)throw new System.Exception("Unexpected sweep key");Phase=2;Escapes++;}
 public static int GetDpiForWindow(object window){return 144;}
 public static bool Exists(string path){return true;}
}
'@
 $tokens=$null;$errors=$null
 $helpers=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 foreach($name in 'Wait-NativeGestureRelease','Wait-PresentedFrame'){
  $fn=$helpers.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq$name}.GetNewClosure(),$true)
  if(!$fn){throw "Missing actual sweep helper $name"}
  . ([scriptblock]::Create($fn.Extent.Text.Replace('[IO.File]','[CapyEdgeSweepTest]')))
 }
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-edge-reach.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 foreach($name in 'Escape-Edge','Settled','Contact-Receipt','Within'){
  $fn=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq$name}.GetNewClosure(),$true)
  if(!$fn){throw "Missing actual sweep function $name"}
  . ([scriptblock]::Create($fn.Extent.Text.Replace('[CapyRowPointer]','[CapyEdgeSweepTest]')))
 }
 $sweep=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Check-ReachSweep'},$true)
 $geometryChecks=@($sweep.Body.FindAll({param($n)$n-is[Management.Automation.Language.IfStatementAst]-and$n.Extent.Text.Contains('Canceled edge sweep changed the arranged divider geometry')},$true)|Sort-Object {$_.Extent.StartOffset})
 if($geometryChecks.Count-ne2){throw 'Missing actual sweep geometry assertions'}
 $statements=@($geometryChecks[1].Parent.Statements);$last=[array]::IndexOf($statements,$geometryChecks[1])
 $sequence=($statements[0..$last]|ForEach-Object {$_.Extent.Text})-join"`n"
 $sequence=$sequence.Replace('[CapyRowPointer]','[CapyEdgeSweepTest]').Replace('[CapyRowPointer+Point]','[CapyEdgeSweepTest+Point]')
 function Read-CanvasProgress{throw 'Non-idle edge sweep unexpectedly requested renderer idle progress'}
 function Start-Sleep([int]$Milliseconds){}
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll-lt12;$poll++){if(& $Condition){return}};throw $Message}
 function Gesture{
  $phase=[CapyEdgeSweepTest]::Phase;$ended=$phase-eq2;$moved=[CapyEdgeSweepTest]::Moves-gt0
  if($ended){$script:sweepTerminalReads++}
  $script:sweepAcknowledged=$ended-and$script:sweepMode-ne'stale-terminal'-and$script:sweepTerminalReads-ge4
  if($phase-eq1-and$moved){$script:sweepMotionReads++}
  $settled=$phase-eq1-and(($moved-and$script:sweepMode-in'settled','settled-error')-or(!$moved-and$script:sweepMode-eq'settled-ineligible'-and$script:sweepDownReads-ge1))
  if($phase-eq1-and!$moved){$script:sweepDownReads++}
  $late=$phase-eq1-and!$moved-and$script:sweepMode-eq'late-down'-and$script:sweepDownReads-le2
  $edge=if($phase-eq1-and!$settled-and!$late-and$script:sweepMode-notin'denied','canvas'){
   @{serial=21;claimed=($moved-and$script:sweepMotionReads-ge4-and$script:sweepMode-ne'unclaimed');contact=$(if($script:sweepMode-like'*ineligible*'){'pickup'}else{'free'});vertical=$false;position=@((510+([CapyEdgeSweepTest]::X-[CapyEdgeSweepTest]::StartX)/1.5),(720+([CapyEdgeSweepTest]::Y-200)/1.5))}
  }
  if($edge-and$script:sweepMode-eq'wrong-edge-serial'){$edge.serial=22}
  if($edge-and$script:sweepMode-eq'wrong-position'){$edge.position[0]+=1/1.5}
  $reply=@{serial=21;phase='down';time=100;bits=8}
  if($moved){
   $reply.phase=if($script:sweepMotionReads-eq1-or$script:sweepMode-eq'old-down'){'down'}else{'move'}
   $reply.time=if($reply.phase-eq'down'){100}elseif($script:sweepMotionReads-lt4-or$script:sweepMode-eq'earlier-move'){109}else{110}
   $reply.bits=if($edge.claimed){33}else{10}
  }
  if($settled){$reply.phase=if($moved){'move'}else{'down'};$reply.time=if($moved){102}else{100};$reply.bits=if($script:sweepMode-eq'settled-error'){-1}else{0}}
  if($script:sweepMode-eq'wrong-serial'){$reply.serial=20}
  if($script:sweepMode-eq'ineligible-wrong-down'){$reply.time=99}
  if($ended){$reply=@{serial=$(if($script:sweepAcknowledged){21}else{20});phase=$(if([CapyEdgeSweepTest]::Escapes){'cancel'}else{'up'});time=111;bits=0}}
  $contact=@{pointer=1;time=$(if($phase-eq0-or$late-or$script:sweepMode-in'stale-down','canvas'){99}else{100});primary=$true;position=@(510,720)}
  @{edge=$edge;pointer_id=$null;edge_reply=$reply;edge_contact=$contact;edge_input=@{pointer=$(if($script:sweepMode-eq'wrong-pointer'){2}else{1});time=$(if($script:sweepMode-eq'stale-move-input'){100}else{110});position=@(210,320)};last_edge=@{phase=$(if([CapyEdgeSweepTest]::Escapes){'cancel'}else{'up'});claimed=([CapyEdgeSweepTest]::Escapes-gt0)};last_release=@{generation=1};last_cancel=@{generation=2}}
 }
 function Model{
  if($script:sweepMode-in'denied','canvas','settled','settled-error','settled-ineligible'-and[CapyEdgeSweepTest]::Phase-eq2){$script:sweepAcknowledged=$true}
  if($script:sweepMode-in'ineligible','settled-ineligible','ineligible-wrong-down','denied','canvas'-and[CapyEdgeSweepTest]::Phase-eq2){$script:sweepAcknowledged=$true;$script:sweepPublished=101}
  if(!$script:sweepAcknowledged){$script:sweepEarlyModelReads++}else{$script:sweepModelReads++}
  if($script:sweepAcknowledged-and$script:sweepModelReads-ge6-and$script:sweepMode-ne'missing-layout'-and![CapyEdgeSweepTest]::Committed){$script:sweepPublished=101}
  @{state=@{workspace=@{layout=$(if($script:sweepPublished-eq101){'restored'}else{'preview'})}}}
 }
 function Layout{@{groups=@()}}
 function Append-SweepRow($Row){$script:sweepRows.Add($Row)}
 function State-File{'owned/ui-state-7-8.json'}
 function Read-Snapshot([string]$Path){
  if((Split-Path -Leaf $Path)-like'presentation-state-*'){
   $script:sweepPresentationReads++
   if($script:sweepPublished-eq101-and$script:sweepPresentationReads-ge3-and$script:sweepMode-ne'missing-presentation'){$script:sweepDisplayed=101}
   return @{process_id=7;presentation_epoch=$script:sweepPublished;required=$script:sweepPublished;displayed=$script:sweepDisplayed}
  }
  @{process_id=7;presentation_epoch=$script:sweepPublished;presented=$script:sweepPublished}
 }
 function Reach-Geometry{
  $script:sweepGeometryReads++
  if($script:sweepDisplayed-eq101-and$script:sweepMode-ne'wrong-geometry'){'restored-divider'}else{'changed-divider'}
 }
 $review=@{Id=7};$window=8;$at=@{x=100;y=200};$sample=@{axis='horizontal';x=509.9;y=720.1;dx=-6;dy=0};$baseline='"restored"';$geometry='restored-divider';$active=@{}
 foreach($Device in 'mouse','touch'){
 $modes=if($Device-eq'mouse'){@('delayed','late-down','wrong-serial','wrong-edge-serial','wrong-position','stale-move-input','old-down','earlier-move','wrong-pointer','stale-down','unclaimed','settled','settled-error','ineligible','settled-ineligible','ineligible-wrong-down','denied','canvas','stale-terminal','missing-layout','missing-presentation','wrong-geometry')}else{@('delayed','late-down','stale-terminal','missing-layout','missing-presentation','wrong-geometry')}
 foreach($mode in $modes){
  $surfaces=if($mode-eq'canvas'){@(@{x=0;y=0;width=500;height=700})}else{@(@{x=500;y=700;width=20;height=40})}
  $script:sweepMode=$mode;$script:sweepTerminalReads=0;$script:sweepMotionReads=0;$script:sweepDownReads=0;$script:sweepAcknowledged=$false;$script:sweepEarlyModelReads=0;$script:sweepModelReads=0;$script:sweepPresentationReads=0;$script:sweepGeometryReads=0;$script:sweepPublished=100;$script:sweepDisplayed=100
  $rows=[Collections.Generic.List[object]]::new();$script:sweepRows=[Collections.Generic.List[object]]::new()
  [CapyEdgeSweepTest]::Phase=0;[CapyEdgeSweepTest]::Downs=0;[CapyEdgeSweepTest]::Cancels=0;[CapyEdgeSweepTest]::Escapes=0;[CapyEdgeSweepTest]::Moves=0;[CapyEdgeSweepTest]::Active=$false;[CapyEdgeSweepTest]::Committed=$false;[CapyEdgeSweepTest]::SharedClaimed=$mode-notin'unclaimed','settled','settled-error','ineligible','settled-ineligible','ineligible-wrong-down','denied','canvas'
  $failure=$null;try{. ([scriptblock]::Create($sequence))}catch{$failure=$_}
  if([CapyEdgeSweepTest]::Committed){throw 'Actual mouse sweep skipped cancellation and committed the delayed shared claim with ordinary Up'}
  if($mode-eq'wrong-geometry'-and(!$failure-or!$failure.Exception.Message.Contains('before: <restored-divider>; after: <changed-divider>'))){throw 'Sweep geometry failure omitted the compared divider geometry'}
  $expected=switch($mode){{$_-in'wrong-serial','wrong-edge-serial','wrong-position','stale-move-input','old-down','earlier-move','wrong-pointer'}{'Native edge sweep request was not acknowledged'}'stale-terminal'{'Native gesture release was not acknowledged'}'missing-layout'{'Canceled edge sweep did not acknowledge its exact workspace layout'}'missing-presentation'{'The display did not show the latest canvas frame'}'wrong-geometry'{'Canceled edge sweep changed the arranged divider geometry'}{$_-in'unclaimed','settled','settled-error'}{'Edge sweep lost or retained a contact:'}'stale-down'{'Native contact Down was not acknowledged at the requested point'}}
  $early=$expected-in'Native edge sweep request was not acknowledged','Native contact Down was not acknowledged at the requested point'
  $escapes=if($early-or$mode-in'unclaimed','settled','settled-error','ineligible','settled-ineligible','ineligible-wrong-down','denied','canvas'){0}else{1}
  if([CapyEdgeSweepTest]::Downs-ne1-or[CapyEdgeSweepTest]::Cancels-ne[int](!$early)-or[CapyEdgeSweepTest]::Escapes-ne$escapes){throw "Sweep repeated or skipped native contact cancellation: $mode; $failure"}
  if(!$expected){
   if($failure-or$script:sweepEarlyModelReads-ne0-or$script:sweepGeometryReads-ne2-or$script:sweepDisplayed-ne101-or$script:sweepRows.Count-ne1){throw "Actual edge sweep checked geometry before acknowledged rollback presentation: $mode; $failure"}
  }elseif(!$failure-or!$failure.Exception.Message.StartsWith($expected)-or($mode-notin'wrong-geometry','unclaimed','settled','settled-error'-and$script:sweepGeometryReads-ne0)){throw "Actual edge sweep accepted an unacknowledged or incorrect cancellation: $mode; $failure"}
 }
 }
 'Actual edge sweep: 22 mouse and six touch cases await the Down receipt on edge surfaces only, then acknowledged eligible claims and exact geometry.'
}

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/LocalizationProperties.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $numeric=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Preference-Numeric'},$true)
 $surface=$numeric.Body.Find({param($n)$n-is[Management.Automation.Language.CommandAst]-and$n.GetCommandName()-eq'Surface-Languages'},$true)
 $body=@($surface.CommandElements|Where-Object {$_-is[Management.Automation.Language.ScriptBlockExpressionAst]})[0].ScriptBlock
 $statements=@($body.EndBlock.Statements)
 $anchor=@($statements|Where-Object {$_-is[Management.Automation.Language.AssignmentStatementAst]-and$_.Left.Extent.Text-eq'$title'-and$_.Extent.Text-like"*number-caption-setting-number-pan_speed*"})
 if($anchor.Count-ne1){throw 'Missing actual numeric title tooltip sequence'}
 $first=[array]::IndexOf($statements,$anchor[0]);$nodes=@()
 for($index=$first;$index-lt$statements.Count;$index++){$nodes+=,$statements[$index];if($statements[$index]-is[Management.Automation.Language.TryStatementAst]){break}}
 if($nodes.Count-ne7-or$nodes[-1]-isnot[Management.Automation.Language.TryStatementAst]){throw 'Incomplete actual title and field tooltip sequence'}
 $sequence=($nodes|ForEach-Object {$_.Extent.Text})-join"`n"
 if($sequence-notlike"*'Retained native error tooltip did not use the current canonical copy' 10*"){throw 'Missing actual retained numeric tooltip hover and assertion sequence'}
 $assertTooltip=[scriptblock]::Create($sequence.Replace('System.Windows.Automation.','CapyTooltipContextTest.').Replace('[CapyRowPointer]','[CapyTooltipContextTest.Driver]').Replace('[CapyRowPointer+Point]','[CapyTooltipContextTest.Driver+Point]'))
 Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
namespace CapyTooltipContextTest {
 public static class AutomationElement {public static object HelpTextProperty="help",NameProperty="name",AutomationIdProperty="id",BoundingRectangleProperty="bounds";}
 public class Rect {public double X,Y,Width,Height;public bool IsEmpty;public double Left{get{return X;}}public double Top{get{return Y;}}public Rect(double x,double y,double width,double height){X=x;Y=y;Width=width;Height=height;}}
 public class Field {
  public int HelpReads,BoundsReads,HoverReads;public bool FailHelp,FailBounds;public string Name,AutomationId;public Rect HoverBounds,FailureBounds;
  public Field Current{get{return this;}}public Rect BoundingRectangle{get{HoverReads++;return HoverBounds;}}
  public object GetCurrentPropertyValue(object property){
   if(Object.Equals(property,AutomationElement.HelpTextProperty)){Driver.ReadOrder.Add("help");HelpReads++;if(FailHelp)throw new InvalidOperationException("field provider unavailable");return "Espressione non valida";}
   if(Object.Equals(property,AutomationElement.BoundingRectangleProperty)){Driver.ReadOrder.Add("bounds");BoundsReads++;if(FailBounds)throw new InvalidOperationException("bounds provider unavailable");return FailureBounds;}
   return Object.Equals(property,AutomationElement.NameProperty)?Name:AutomationId;
  }
 }
 public static class Driver {
  public struct Point {public int x,y;}public static int CursorReads;public static bool MissingCursor,FailCursor,TitleAcknowledged;public static Point Position;public static List<string> Hovers=new List<string>(),ReadOrder=new List<string>();
  public static void Hover(int x,int y){if(Hovers.Count==1&&!TitleAcknowledged)throw new InvalidOperationException("Field hover preceded title acknowledgement");Hovers.Add(x+","+y);}
  public static bool GetCursorPos(out Point point){ReadOrder.Add("cursor");CursorReads++;point=Position;if(FailCursor)throw new InvalidOperationException("cursor provider unavailable");return !MissingCursor;}
 }
}
'@
 $choice=@{tag='it'};$review=@{Id=33}
 function Catalog-Text($Tag,$Id,$Domain){[CapyTooltipContextTest.Driver]::ReadOrder.Add('catalog');$state.catalogReads++;if($Tag-cne'it'-or$Id-cne'numeric-invalid-expression'-or$Domain-cne'common'){throw 'Unexpected tooltip catalog lookup'};'Espressione non valida'}
 function Control($Id){if($Id-cne'number-caption-setting-number-pan_speed'){throw 'Unexpected tooltip hover owner'};$heading}
 function Numeric-Tooltip{if([CapyTooltipContextTest.Driver]::Hovers.Count-eq1){return @{Current=@{Name=$heading.Name}}};if($case-eq'success'){@{Current=@{Name='Espressione non valida'}}}else{@{Current=@{Name='stale copy'}}}}
 function Capy-Tooltips($ProcessId){
  $state.inventoryReads++;[CapyTooltipContextTest.Driver]::ReadOrder.Add('inventory');if($ProcessId-ne33){throw 'Wrong tooltip owner'}
  if($case-eq'provider'){throw 'tooltip provider unavailable'}
  if($case-eq'wrong'){foreach($pair in @(@('stale copy','numeric-tip'),@('other tooltip','other-tip'))){$tip=[CapyTooltipContextTest.Field]::new();$tip.Name=$pair[0];$tip.AutomationId=$pair[1];$tip}}
 }
 function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds){
  if($Seconds-ne10){throw 'Tooltip assertion changed its deadline'}
  if($Message-eq'Native numeric title tooltip did not acknowledge hover'){if(!(& $Condition)){throw $Message};[CapyTooltipContextTest.Driver]::TitleAcknowledged=$true;return}
  if($case-ne'unrelated'-and(& $Condition)){return}
  try{if($case-eq'unrelated'){throw 'unrelated predicate failure'};throw $Message}catch{
   [CapyTooltipContextTest.Driver]::ReadOrder.Clear();$state.original=$_;$state.stack=$_.ScriptStackTrace
   if($case-in@('wrong','unrelated')){$_.ErrorDetails=[Management.Automation.ErrorDetails]::new('Prior tooltip detail')}
   $state.details=if($_.ErrorDetails){$_.ErrorDetails.Message}else{$_.Exception.Message}
   throw $state.original
  }
 }
 foreach($case in 'empty','unchanged','wrong','provider','field','geometry','cursor','missing','empty-bounds','success','unrelated'){
  $heading=[CapyTooltipContextTest.Field]::new();$heading.Name='Pan speed';$heading.HoverBounds=[CapyTooltipContextTest.Rect]::new(100.25,200.5,240.75,32.25)
  $control=[CapyTooltipContextTest.Field]::new();$control.HoverBounds=[CapyTooltipContextTest.Rect]::new(910.25,440.5,60.75,34.25);$control.FailureBounds=[CapyTooltipContextTest.Rect]::new(920.75,447.25,62.5,36.75)
  if($case-eq'unchanged'){$control.FailureBounds=$control.HoverBounds}
  if($case-eq'missing'){$control.FailureBounds=$null}
  if($case-eq'empty-bounds'){$control.FailureBounds=[CapyTooltipContextTest.Rect]::new(0,0,0,0);$control.FailureBounds.IsEmpty=$true}
  $control.FailHelp=$case-eq'field';$control.FailBounds=$case-eq'geometry'
  [CapyTooltipContextTest.Driver]::Hovers.Clear();[CapyTooltipContextTest.Driver]::TitleAcknowledged=$false;[CapyTooltipContextTest.Driver]::ReadOrder.Clear();[CapyTooltipContextTest.Driver]::CursorReads=0;[CapyTooltipContextTest.Driver]::MissingCursor=$case-eq'missing';[CapyTooltipContextTest.Driver]::FailCursor=$case-eq'cursor'
  $position=[CapyTooltipContextTest.Driver+Point]::new();$position.x=941;$position.y=458;[CapyTooltipContextTest.Driver]::Position=$position
  $state=@{inventoryReads=0;catalogReads=0;original=$null;stack=$null};$failure=$null
  try{& $assertTooltip}catch{$failure=$_}
  if(([CapyTooltipContextTest.Driver]::Hovers-join';')-cne'221,217;941,458'-or$heading.HoverReads-ne1-or$control.HoverReads-ne1){throw 'Tooltip context changed or repeated its actual hover geometry reads or moves'}
  if($case-eq'success'){
   if($failure-or$control.HelpReads-or$control.BoundsReads-or[CapyTooltipContextTest.Driver]::CursorReads-or$state.inventoryReads-or$state.catalogReads-ne1){throw 'Successful tooltip assertion performed diagnostic reads'}
   continue
  }
  if(![object]::ReferenceEquals($failure.Exception,$state.original.Exception)-or$failure.ScriptStackTrace-cne$state.stack){throw "Tooltip context replaced the original exception or stack: $case"}
  if($case-eq'unrelated'){
   if($failure.Exception.Message-cne'unrelated predicate failure'-or$failure.ErrorDetails.Message-cne'Prior tooltip detail'-or$control.HelpReads-or$control.BoundsReads-or[CapyTooltipContextTest.Driver]::CursorReads-or$state.inventoryReads-or$state.catalogReads){throw 'Tooltip context changed an unrelated exception'}
   continue
  }
  if($failure.Exception.Message-cne'Retained native error tooltip did not use the current canonical copy'-or!$failure.ErrorDetails-or!$failure.ErrorDetails.Message.StartsWith($state.details+"`n")){throw "Tooltip context lost the original assertion or details: $case"}
  if(([CapyTooltipContextTest.Driver]::ReadOrder-join',')-cne'cursor,bounds,catalog,help,inventory'){throw 'Tooltip timeout did not sample cursor and current bounds before catalog, HelpText and inventory reads'}
  $context=$failure.ErrorDetails.Message.Substring(($state.details+"`n").Length)|ConvertFrom-Json
  if($context.tag-cne'it'-or$context.expected-cne'Espressione non valida'-or$control.HelpReads-ne1-or$control.BoundsReads-ne1-or[CapyTooltipContextTest.Driver]::CursorReads-ne1-or$state.inventoryReads-ne1-or$state.catalogReads-ne2){throw 'Tooltip failure omitted canonical copy/tag or repeated diagnostic reads'}
  if($case-eq'field'){if($context.help_text-notlike'*field provider unavailable*'){throw 'Tooltip context lost the field provider failure'}}
  elseif($context.help_text-cne'Espressione non valida'){throw 'Tooltip failure omitted current localized field HelpText'}
  if($case-eq'wrong'){
   if($context.tooltips.Count-ne2-or$context.tooltips[0].Name-cne'stale copy'-or$context.tooltips[0].AutomationId-cne'numeric-tip'-or$context.tooltips[1].Name-cne'other tooltip'-or$context.tooltips[1].AutomationId-cne'other-tip'){throw 'Tooltip failure omitted visible owned tooltip names or IDs'}
  }elseif($case-eq'provider'){if($context.tooltips-notlike'*tooltip provider unavailable*'){throw 'Tooltip context lost the inventory provider failure'}}
  elseif(@($context.tooltips).Count-ne0-or!$failure.ErrorDetails.Message.Contains('"tooltips":[]')){throw 'Tooltip failure did not report an empty owned inventory'}
  if($context.hover_away_bounds.x-ne100.25-or$context.hover_away_bounds.y-ne200.5-or$context.hover_away_bounds.width-ne240.75-or$context.hover_away_bounds.height-ne32.25-or$context.hover_field_bounds.x-ne910.25-or$context.hover_field_bounds.y-ne440.5-or$context.hover_field_bounds.width-ne60.75-or$context.hover_field_bounds.height-ne34.25){throw 'Tooltip failure lost the exact rectangles used by its two actual hovers'}
  if($case-eq'cursor'){if($context.cursor-notlike'*cursor provider unavailable*'){throw 'Tooltip context lost the cursor provider failure'}}
  elseif($case-eq'missing'){if($context.cursor-notlike'*GetCursorPos returned no position*'-or$context.field_bounds-cne'Unavailable: field rectangle missing or empty'){throw 'Tooltip failure invented missing cursor or field coordinates'}}
  elseif($context.cursor.x-ne941-or$context.cursor.y-ne458){throw 'Tooltip failure omitted the current cursor position'}
  if($case-eq'geometry'){if($context.field_bounds-notlike'*bounds provider unavailable*'){throw 'Tooltip context lost the bounds provider failure'}}
  elseif($case-eq'empty-bounds'){if($context.field_bounds-cne'Unavailable: field rectangle missing or empty'){throw 'Tooltip failure invented an empty field rectangle'}}
  elseif($case-ne'missing'){
   $expected=$control.FailureBounds
   if($context.field_bounds.x-ne$expected.X-or$context.field_bounds.y-ne$expected.Y-or$context.field_bounds.width-ne$expected.Width-or$context.field_bounds.height-ne$expected.Height){throw 'Tooltip failure replaced current bounds with cached hover bounds'}
  }
 }
 'Actual retained numeric tooltip assertion preserves original failure and both hover rectangles, samples current cursor/bounds only on its exact timeout, and retains missing/provider errors without success-path diagnostic reads.'
}

& {
 $fit=$ast.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Fit-Canvas'},$true)
 if(!$fit){throw 'Missing actual Fit-Canvas'}
 Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
namespace CapyFitReceiptTest {
 public static class InvokePattern {public static object Pattern="invoke";}
 public class Node {
  public string Id;public int ProcessId=33;public bool IsEnabled=true,IsKeyboardFocusable=true,HasKeyboardFocus=true,IsOffscreen=false;
  public Node Current{get{return this;}}public void SetFocus(){Driver.Focus++;}
  public Node GetCurrentPattern(object pattern){if(!Object.Equals(pattern,InvokePattern.Pattern)||ProcessId!=33)throw new Exception("Foreign Fit command owner");return this;}
  public void Invoke(){if(ProcessId!=33)throw new Exception("Foreign Fit invocation");Driver.Commands.Add(Id);Driver.Stage=Id=="fit_canvas"?1:2;Driver.Reads=0;Driver.MenuOpen=false;}
 }
 public static class Driver {
  public static int Stage,Reads,Focus,Chords;public static bool MenuOpen;public static IntPtr Foreground;public static List<string> Commands=new List<string>();
  public static bool SetForegroundWindow(IntPtr window){if(window!=new IntPtr(333))throw new Exception("Foreign Fit foreground owner");Foreground=window;return true;}
  public static void Chord(uint process,ushort[] modifiers,ushort key){if(process!=33||Foreground!=new IntPtr(333)||modifiers.Length!=1||modifiers[0]!=17||key!=48)throw new Exception("Foreign Fit shortcut owner");Chords++;Stage=3;Reads=0;}
 }
}
'@
 $body=$fit.Extent.Text.Replace('System.Windows.Automation.','CapyFitReceiptTest.').Replace('[CapyWindowApi]','[CapyFitReceiptTest.Driver]').Replace('[CapyRowPointer]','[CapyFitReceiptTest.Driver]').Replace("(Join-Path `$script:CapyScripts 'open-application-menu.ps1')",'Native-OpenMenu')
 . ([scriptblock]::Create($body))
 $review=[pscustomobject]@{Id=33};$root=[CapyFitReceiptTest.Node]::new();$drawingWindow=[IntPtr]333;$StateFile='owned-state'
 function State-File{'owned-state'}
 function Find($Id){if($Id-cne'drawing-canvas'){throw 'Fit focused another control'};$root}
 function Owned-DrawingWindow($Process){if($Process.Id-ne33){throw 'Fit queried another process'};[pscustomobject]@{Handle=[IntPtr]333;Root=$root;Id=11}}
 function Wait-CanvasViewport($Drawing,$Process,$StatePath,$OuterSize){if($Process.Id-ne33){throw 'Fit viewport queried another owner'};$seen.viewport++}
 function Native-OpenMenu($Root,$Name,$Caption){if(![object]::ReferenceEquals($Root,$seen.root)-or$Name-cne'View'-or$Caption-cne'View'){throw 'Fit opened another menu'};$seen.menus++}
 function Zoom-Item($Id){if($Id-notin@('fit_canvas','zoom_in')){throw 'Fit queried another command'};$node=[CapyFitReceiptTest.Node]::new();$node.Id=$Id;$node}
 function Next-FitReceipt{
  $stage=[CapyFitReceiptTest.Driver]::Stage;$read=[CapyFitReceiptTest.Driver]::Reads++
  $raw=if($stage-eq0){3}elseif($stage-eq1){if($read-lt3){4}else{5}}elseif($stage-eq2){if($read-eq0){5}else{6}}else{7}
  $nav=@{3=1;4=1;5=2;6=3;7=4}[$raw]
  $zoom=if($raw-le4){.5}elseif($raw-eq6){.9296812415122986}else{.657383918762207}
  if($case-eq'same-value'-and$raw-le4){$zoom=.657383918762207}
  if(($case-eq'missing-fit'-and$stage-eq1)-or($case-eq'missing-zoom'-and$stage-eq2)-or($case-eq'missing-shortcut'-and$stage-eq3)){$nav=$stage;$raw=3+$stage}
  if($case-eq'automatic-only'){$nav=1}
  if($case-eq'skipped-navigation'-and$stage-eq1-and$raw-eq5){$nav=3}
  if($case-eq'stale-receipt'-and$stage-gt0){$nav=1;$raw=3}
  $receipt=[pscustomobject]@{process_id=33;window_id=11;camera=[pscustomobject]@{revision=$raw;zoom=$zoom;viewport=@(2560,1528);work_area=@(501,135,1558,1384)};navigation=[pscustomobject]@{drawing_id=22;artwork='artwork';epoch=0;revision=$nav}}
  if($stage-ge2){switch($case){'changed-viewport'{$receipt.camera.viewport[0]--};'changed-work-area'{$receipt.camera.work_area[2]--}}}
  if($stage-gt0){switch($case){'foreign-process'{$receipt.process_id=34};'foreign-window'{$receipt.window_id=12};'foreign-drawing'{$receipt.navigation.drawing_id=23};'foreign-artwork'{$receipt.navigation.artwork='other'};'foreign-epoch'{$receipt.navigation.epoch=1}}}
  $receipt
 }
 function Camera-Receipt{$receipt=Next-FitReceipt;$seen.receipts.Add($receipt);$receipt}
 function Model{$receipt=Next-FitReceipt;[pscustomobject]@{state=[pscustomobject]@{camera=$receipt.camera};application_menus=@([pscustomobject]@{id='view';label='View'})}}
 function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds){for($poll=0;$poll-lt12;$poll++){if(& $Condition){return}};throw $Message}
 foreach($case in 'scheduled','same-value','missing-fit','missing-zoom','missing-shortcut','automatic-only','stale-receipt','skipped-navigation','changed-viewport','changed-work-area','foreign-process','foreign-window','foreign-drawing','foreign-artwork','foreign-epoch'){
  [CapyFitReceiptTest.Driver]::Foreground=[IntPtr]::Zero;[CapyFitReceiptTest.Driver]::Stage=0;[CapyFitReceiptTest.Driver]::Reads=0;[CapyFitReceiptTest.Driver]::Focus=0;[CapyFitReceiptTest.Driver]::Chords=0;[CapyFitReceiptTest.Driver]::Commands.Clear()
  $seen=@{root=$root;viewport=0;menus=0;receipts=[Collections.Generic.List[object]]::new()};$failure=$null
  try{Fit-Canvas}catch{$failure=$_}
  if($case-in@('scheduled','same-value')){
   if(([CapyFitReceiptTest.Driver]::Commands-join',')-cne'fit_canvas,zoom_in'-or[CapyFitReceiptTest.Driver]::Chords-ne1-or$seen.menus-ne2){throw 'Fit changed its native command deliveries'}
   if($failure){throw $failure}
   if(([CapyFitReceiptTest.Driver]::Commands-join',')-cne'fit_canvas,zoom_in'-or[CapyFitReceiptTest.Driver]::Chords-ne1-or$seen.menus-ne2-or$seen.viewport-ne1-or[CapyFitReceiptTest.Driver]::Focus-ne2){throw 'Fit did not preserve one delivery per menu command and shortcut, viewport acknowledgement and both focus checks'}
  }elseif(!$failure){throw "Fit accepted $case without its owned navigation acknowledgement"}
 }
 'Actual Fit retains command-owned same-packet camera receipts through resize, same-value Fit and both command paths, and rejects missing acknowledgements and foreign identities.'
 & {
  function State-File{}
  function Wait-CanvasViewport{throw 'Trace-free Fit requested a traced viewport'}
  function Camera-Receipt{throw 'Trace-free Fit requested a camera receipt'}
  function Model{throw 'Trace-free Fit requested a model'}
  function Native-OpenMenu($Root,$Name,$Caption){
   if(![object]::ReferenceEquals($Root,$seen.root)-or$Name-cne'View'-or$Caption){throw 'Trace-free Fit opened another menu'}
   $seen.menus++;[CapyFitReceiptTest.Driver]::MenuOpen=$true
  }
  function Zoom-Item($Id){if([CapyFitReceiptTest.Driver]::MenuOpen){$node=[CapyFitReceiptTest.Node]::new();$node.Id=$Id;$node}}
  function Control($Id){
   if($Id-eq'drawing-canvas'){return $root}
   if($Id-ne'canvas-camera'){throw 'Trace-free Fit read another control'}
   $stage=[CapyFitReceiptTest.Driver]::Stage;$read=[CapyFitReceiptTest.Driver]::Reads++
   $name=if($stage-eq2-and($read-ge2-or$case-eq'missing-shortcut')-and$case-ne'missing-zoom'){'93%'}elseif($stage-eq3-and($read-lt2-or$case-eq'missing-shortcut')){'93%'}else{'66%'}
   [pscustomobject]@{Current=[pscustomobject]@{Name=$name}}
  }
  foreach($case in 'scheduled','missing-zoom','missing-shortcut'){
   [CapyFitReceiptTest.Driver]::Stage=0;[CapyFitReceiptTest.Driver]::Reads=0;[CapyFitReceiptTest.Driver]::Focus=0;[CapyFitReceiptTest.Driver]::Chords=0;[CapyFitReceiptTest.Driver]::MenuOpen=$false;[CapyFitReceiptTest.Driver]::Commands.Clear()
   $seen=@{root=$root;menus=0};$failure=$null
   try{Fit-Canvas}catch{$failure=$_}
   if($case-eq'scheduled'){
    if($failure){throw $failure}
    if(([CapyFitReceiptTest.Driver]::Commands-join',')-cne'fit_canvas,zoom_in'-or[CapyFitReceiptTest.Driver]::Chords-ne1-or[CapyFitReceiptTest.Driver]::Focus-ne2-or$seen.menus-ne2){throw 'Trace-free Fit changed command cardinality or focus checks'}
   }else{
    $expected=if($case-eq'missing-zoom'){'Zoom In did not acknowledge a distinct camera'}else{'Fit did not restore the native camera readout'}
    if(!$failure-or$failure.Exception.Message-cne$expected){throw "Trace-free Fit accepted missing native acknowledgement: $case; $failure"}
   }
  }
  'Actual trace-free Fit restores the native readout through View commands and Ctrl+0; missing Zoom In or shortcut acknowledgement fails.'
 }
}

& {
 $reader=$ast.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Camera-Receipt'},$true)
 if(!$reader){throw 'Missing actual camera receipt reader'}
 . ([scriptblock]::Create($reader.Extent.Text))
 $review=[pscustomobject]@{Id=33};$drawingWindow=[IntPtr]333
 $statePath=Join-Path $PSScriptRoot 'ui-state-33-11.json'
 function State-File{$statePath}
 function Read-Snapshot($Path){
  $reads.Add((Split-Path -Leaf $Path))
  switch(Split-Path -Leaf $Path){
   'ui-state-33-11.json'{$state}
   'windows-33.json'{$registry}
   'camera-state-33-11.json'{$packet}
   default{throw 'Camera receipt read an unrelated file'}
  }
 }
 foreach($case in 'owned','state-process','state-window','registry-process','registry-hwnd','registry-duplicate','not-isolated','packet-process','packet-window','missing-navigation','missing-camera'){
  $reads=[Collections.Generic.List[string]]::new()
  $state=[pscustomobject]@{process_id=33;window_id=11;model=[pscustomobject]@{windows_isolated_settings=$true;state=[pscustomobject]@{camera=[pscustomobject]@{revision=2;zoom=.25}}}}
  $registry=[pscustomobject]@{process_id=33;windows=@([pscustomobject]@{hwnd=333;id=11})}
  $packet=[pscustomobject]@{process_id=33;window_id=11;camera=[pscustomobject]@{revision=7;zoom=.657383918762207;viewport=@(2560,1528);work_area=@(501,135,1558,1384)};navigation=[pscustomobject]@{drawing_id=22;artwork='artwork';epoch=0;revision=4}}
  switch($case){
   'state-process'{$state.process_id=34};'state-window'{$state.window_id=12};'registry-process'{$registry.process_id=34};'registry-hwnd'{$registry.windows[0].hwnd=334}
   'registry-duplicate'{$registry.windows+=@($registry.windows[0])};'not-isolated'{$state.model.windows_isolated_settings=$false}
   'packet-process'{$packet.process_id=34};'packet-window'{$packet.window_id=12};'missing-navigation'{$packet.navigation=$null};'missing-camera'{$packet.camera=$null}
  }
  $failure=$null;$receipt=$null;try{$receipt=Camera-Receipt}catch{$failure=$_}
  if($case-eq'owned'){
   if($failure-or![object]::ReferenceEquals($receipt,$packet)-or$receipt.camera.revision-ne7-or$receipt.navigation.revision-ne4-or($reads-join',')-cne'ui-state-33-11.json,windows-33.json,camera-state-33-11.json'){throw 'Camera receipt combined stale model camera with another navigation packet or repeated its reads'}
  }elseif($case-in@('missing-navigation','missing-camera')){
   if($failure-or$receipt){throw 'Camera receipt accepted incomplete same-packet metadata'}
  }elseif(!$failure-or$receipt){throw "Camera receipt accepted $case ownership"}
 }
 'Actual camera receipt reader retains one packet camera/navigation and validates isolated PID, registered HWND and window identity.'
}
& {
 $tokens=$null;$errors=$null
 $localization=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-localization.ps1'),[ref]$tokens,[ref]$errors)
 if(@($errors).Count){throw ($errors|Out-String)}
 $reader=$ast.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Camera-Receipt'},$true)
 if(!$reader){throw 'Missing actual camera receipt reader'}
 . ([scriptblock]::Create($reader.Extent.Text))
 $definitions=@($localization.FindAll({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Fit-View'},$true))
 foreach($definition in $definitions){. ([scriptblock]::Create($definition.Extent.Text))}
 $sites=@($localization.FindAll({param($n)$n-is[Management.Automation.Language.CommandAst]-and($n.GetCommandName()-eq'Fit-View'-or($n.GetCommandName()-eq'Menu-Command'-and@($n.CommandElements.Value)-contains'fit_canvas'))},$true)|Where-Object {$site=$_;!@($definitions|Where-Object {$site.Extent.StartOffset-ge$_.Extent.StartOffset-and$site.Extent.EndOffset-le$_.Extent.EndOffset}).Count})
 if($sites.Count-ne3){throw "Localization drawing and pixel sampling lost a Fit step: $($sites.Count)"}
 $review=[pscustomobject]@{Id=33};$script:current=[pscustomobject]@{id=11;hwnd=333}
 $statePath=Join-Path $PSScriptRoot 'ui-state-33-11.json'
 function State-File{$statePath}
 function Model{[pscustomobject]@{state=[pscustomobject]@{camera=[pscustomobject]@{revision=13}}}}
 function Menu-Command([string]$Menu,[string]$Id){if($Menu-cne'view'-or$Id-cne'fit_canvas'){throw 'Localization Fit invoked another command'};$fit.invoked++;$fit.reads=0}
 function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds){for($poll=0;$poll-lt12;$poll++){if(& $Condition){return}};throw $Message}
 function Read-Snapshot($Path){
  switch(Split-Path -Leaf $Path){
   'ui-state-33-11.json'{return [pscustomobject]@{process_id=33;window_id=11;model=[pscustomobject]@{windows_isolated_settings=$true}}}
   'windows-33.json'{return [pscustomobject]@{process_id=33;windows=@([pscustomobject]@{hwnd=333;id=11})}}
   'camera-state-33-11.json'{}
   default{throw 'Localization Fit read an unrelated file'}
  }
  $read=$fit.reads++;$landed=$fit.invoked-and$read-ge2-and$case-ne'missing'
  $revision=if($landed){14}elseif($case-eq'stale'-and!$fit.invoked-and$read-lt2){12}else{13}
  $navigation=$revision-9;if($landed-and$case-eq'skipped'){$navigation++}
  $drawing=if($revision-eq12-or($landed-and$case-eq'foreign-drawing')){21}else{22}
  $fit.observed=$fit.observed-or$landed
  [pscustomobject]@{process_id=33;window_id=11;camera=[pscustomobject]@{revision=$revision;zoom=.657383918762207;viewport=@(2560,1528);work_area=@(501,135,1558,1384)};navigation=[pscustomobject]@{drawing_id=$drawing;artwork='artwork';epoch=0;revision=$navigation}}
 }
 foreach($site in $sites){
  $case='late';$fit=@{invoked=0;reads=0;observed=$false}
  . ([scriptblock]::Create($site.Extent.Text))
  if($fit.invoked-ne1-or!$fit.observed){throw "Localization drawing or pixel sampling continued before Fit acknowledged its camera: $($site.Extent.Text)"}
 }
 $expected=@{stale='';skipped='Fit camera acknowledgement skipped the requested navigation';'foreign-drawing'='Fit camera acknowledgement changed its document';missing='View Fit did not acknowledge its camera'}
 foreach($case in 'stale','skipped','foreign-drawing','missing'){
  $fit=@{invoked=0;reads=0;observed=$false};$failure=$null
  try{Fit-View}catch{$failure=$_}
  if($fit.invoked-ne1-or"$($failure.Exception.Message)"-cne$expected[$case]){throw "Localization Fit mishandled $case`: $failure"}
 }
 'Actual localization Fit steps wait for the next owned navigation receipt after a fresh pre-Fit receipt, and reject skipped, foreign or missing acknowledgements.'
}
& {
 $drawerAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-drawer-layout.ps1'),[ref]$null,[ref]$null)
 $open=$drawerAst.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Open-Header'},$true)
 if(!$open){throw 'Missing actual drawer Open-Header path'}
 . ([scriptblock]::Create($open.Extent.Text))
 function Model{[pscustomobject]@{header=@{model=@{zones=@(@(),@(),@(@{id=12}))}}}}
 function Header{
  $bounds=@{x=985.3333740234375;y=8;width=48;height=48}
  if($script:headerCase-eq'overflow'){@{geometry=@{hidden=@(@(),@(),@(12));overflow=@($null,$null,$bounds)};actual_items=@()}}
  else{@{geometry=@{hidden=@(@(),@(),@());overflow=@($null,$null,$null)};actual_items=@(@{id=12;bounds=$bounds})}}
 }
 function Find([string]$Id,[switch]$Visible){
  if($Id-eq'tool-drawer'){return $script:headerOpened}
  if($Id-eq'header-item-12'){
   $script:headerFinds++
   if($script:headerCase-eq'overflow'-or($script:headerCase-eq'transient'-and$script:headerFinds-eq1)){return $null}
   return [pscustomobject]@{Current=@{IsOffscreen=$false;BoundingRectangle=@{Width=48;Height=48}}}
  }
  if($Id-eq'header-overflow-2'-and$script:headerCase-eq'overflow'){return [pscustomobject]@{Current=@{IsOffscreen=$false;BoundingRectangle=@{Width=48;Height=48}}}}
  if($Id-eq'header-overflow-item-12'-and$script:headerMenu){return $true}
 }
 function Tap([string]$Id){
  $script:headerTaps.Add($Id)
  if($Id-eq'header-overflow-2'){
   if($script:headerCase-ne'overflow'){throw 'Missing control: header-overflow-2'}
   $script:headerMenu=$true;return
  }
  if($Id-eq'header-item-12'-or($Id-eq'header-overflow-item-12'-and$script:headerMenu)){$script:headerOpened=$true;return}
  throw "Unexpected header target: $Id"
 }
 function Drawer{@{placement=@{bounds=@{height=400}}}}
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll-lt4;$poll++){if(& $Condition){return}};throw $Message}
 function Start-Sleep{}
 foreach($case in @('direct','overflow','transient')){
  $script:headerCase=$case;$script:headerFinds=0;$script:headerMenu=$false;$script:headerOpened=$false;$script:headerTaps=[Collections.Generic.List[string]]::new()
  try{Open-Header @{id=12}}catch{
   if($case-eq'transient'-and$_.Exception.Message-eq'Missing control: header-overflow-2'){throw 'A transient direct header item selected nonexistent overflow'}
   throw
  }
  $expected=if($case-eq'overflow'){'header-overflow-2,header-overflow-item-12'}else{'header-item-12'}
  if(($script:headerTaps-join',')-cne$expected){throw "Header readiness changed input cardinality: $case"}
 }
 'Actual drawer header path waits for native direct-item readiness and preserves genuine overflow with one input per target.'
}

& {
 Add-Type -TypeDefinition @'
namespace CapyInertiaFixtureTest {
 public static class Input {
  public static string Mode="normal",Phase="idle",Device="touch";
  public static int Downs,Generation,LastCancel,Reads,BeforeOpposite,LastX,LastY;
  public static bool Active,Held,Moved;
  public static double Offset;
  public static void Reset(string mode){Mode=mode;Phase="idle";Device="touch";Downs=Generation=LastCancel=Reads=BeforeOpposite=LastX=LastY=0;Active=Held=Moved=false;Offset=50;}
  public static void Initialize(uint process){if(Device!="touch")throw new System.ArgumentException("Invalid inertia device");if(process!=33)throw new System.Exception("Foreign inertia owner");}
  static void Validate(uint id){if(id!=1||Device!="touch")throw new System.ArgumentException("Invalid inertia id or device");}
  public static void Down(uint id,int x,int y){Validate(id);if(Active)throw new System.Exception("Invalid inertia contact");LastX=x;LastY=y;Downs++;Generation++;Reads=0;Active=true;Held=Moved=false;Phase="pressed";}
  static void Observe(){if(Downs==2&&Active&&!Moved){Reads++;if(Mode!="unheld"&&Reads>=5){Held=true;Phase="held";}}}
  static void Motion(int dy){
   if(!Active)throw new System.Exception("Missing inertia contact");
   if(Downs==2&&!Moved)BeforeOpposite=Reads;
   Moved=true;
   if(Held)return;
   if(Phase=="pressed"){LastCancel=Generation;if(Downs==2&&Mode=="stale")LastCancel--;Generation++;Phase=Downs==2&&Mode=="missing"?"pressed":"idle";}
   Offset-=dy*.04;
  }
  public static double MoveTimed(uint id,int dx,int dy,int milliseconds,bool release){if(milliseconds<=0)throw new System.ArgumentOutOfRangeException("milliseconds");Move(id,LastX+dx,LastY+dy);if(release)Up(id);return milliseconds;}
  public static void Move(uint id,int x,int y){Validate(id);Motion(y-LastY);LastX=x;LastY=y;}
  public static void Up(uint id){Validate(id);if(!Active)throw new System.Exception("Missing inertia release");Active=false;}
  public static void Dispose(){Active=false;}
  public static double Percent(){Observe();if(Downs==1&&!Active)Offset+=.1;return Offset;}
  public static string Native(){
   Observe();string device=Downs==2&&Mode=="device"?"pen":"touch";
   return "{\"device\":\""+device+"\",\"phase\":\""+Phase+"\",\"generation\":"+Generation+",\"touch_contacts\":"+(Phase=="idle"?0:1)+",\"pointer_id\":"+(Phase=="idle"?"null":"41")+",\"last_cancel\":{\"reason\":\"native_scroll\",\"generation\":"+LastCancel+"}}";
  }
 }
}
'@
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-tool-bubbles.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $actual=@($source.FindAll({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Native-InertiaInterruption'},$true))
 if($actual.Count-ne1){throw 'Missing actual Native-InertiaInterruption'}
 . ([scriptblock]::Create($actual[0].Extent.Text.Replace('[CapyCanvasTouch]','[CapyInertiaFixtureTest.Input]')))
 function Settle-LayerScroll($List,$Pattern,[long]$Contact=0,[switch]$Held,[Nullable[double]]$TargetPercent){}
 function Read-PanelScrollReceipt($List){@{contact=[CapyInertiaFixtureTest.Input]::Downs}}
 function Settled-LayerState{
  @{state=@{layers=@(@{id=1},@{id=2});document_file=@{revision=7};layer_tools=@{editing_layer=@{id=1}}};windows_tabs=@{selected=3;session_stamps=@(@{id=3;stamp=@{checkpoint=9}})}}
 }
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll-lt10;$poll++){if(& $Condition){return}};throw $Message}
 function Start-Sleep([int]$Milliseconds){}
 $native=[pscustomobject]@{BoundingRectangle=[pscustomobject]@{X=0;Y=0;Width=100;Height=100}}
 $native|Add-Member ScriptProperty ItemStatus {[CapyInertiaFixtureTest.Input]::Native()}
 $list=[pscustomobject]@{Current=$native}
 $percent=[pscustomobject]@{}
 $percent|Add-Member ScriptProperty VerticalScrollPercent {[CapyInertiaFixtureTest.Input]::Percent()}
 $scroll=[pscustomobject]@{Current=$percent}
 $scroll|Add-Member ScriptMethod SetScrollPercent {param($Horizontal,$Vertical)[CapyInertiaFixtureTest.Input]::Offset=$Vertical}
 $review=[pscustomobject]@{Id=33}
 $run=Join-Path ([IO.Path]::GetTempPath()) ('capy-native-inertia-'+[guid]::NewGuid().ToString('N'))
 $null=New-Item -ItemType Directory -Path $run
 try{
  foreach($mode in 'unheld','normal','stale','device','missing'){
   [CapyInertiaFixtureTest.Input]::Reset($mode);$checks=[Collections.Generic.List[string]]::new()
   $failure=$null;try{Native-InertiaInterruption 23 $list $scroll}catch{$failure=$_}
   if($mode-in@('unheld','normal')){
    if($failure){throw $failure}
    if(($mode-eq'normal'-and[CapyInertiaFixtureTest.Input]::BeforeOpposite-ne0)-or[CapyInertiaFixtureTest.Input]::Downs-ne3-or$checks.Count-ne1){throw 'Native inertia contact observed before movement or repeated'}
    if($mode-eq'unheld'){'Actual native inertia: unheld observer schedule passed the original opposite-direction assertion.'}
   }elseif(!$failure-or$failure.Exception.Message-cne'Native layer list did not hand the interrupting contact to scrolling'){
    throw "Native inertia accepted a noncausal contact acknowledgement: $mode; $failure"
   }
  }
 }finally{Remove-Item -LiteralPath $run -Recurse -Force}
 foreach($invalid in 'id','device'){
  [CapyInertiaFixtureTest.Input]::Reset('normal');$rejected=$false
  try{if($invalid-eq'id'){[CapyInertiaFixtureTest.Input]::Down(2,10,20)}else{[CapyInertiaFixtureTest.Input]::Device='pen';[CapyInertiaFixtureTest.Input]::Down(1,10,20)}}catch{$rejected=$true}
  if(!$rejected){throw 'Native inertia test driver accepted an invalid id or device'}
 }
 'Actual native inertia: movement precedes synchronous observers; stale, wrong-device and missing handoffs fail before offset can stand in for admission.'
}

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/BubbleVisualChecks.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $actual=@($source.FindAll({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Open-BubbleVisualChooser'},$true))
 if($actual.Count-ne1){throw 'Missing actual Open-BubbleVisualChooser'}
 . ([scriptblock]::Create($actual[0].Extent.Text))
 function Center([long]$Id){if($Id-ne23){throw 'Chooser addressed another bubble'};@{x=10;y=20}}
 function Tap($Point){
  if($Point.x-ne10-or$Point.y-ne20){throw 'Chooser missed its actual center'}
  $script:chooserTaps++
  if($script:chooserMode-like'missing-*'){return}
  if($script:chooserMode-eq'delayed-each'){$script:chooserPublishAt=$script:chooserReads+3}
  if(!$script:chooserSelected){$script:chooserSelected=$true;$script:chooserPhase=1}elseif($script:chooserMode-eq'no-sliders'){$script:chooserPhase=if($script:chooserPhase-eq0){2}else{0}}else{$script:chooserPhase=($script:chooserPhase+1)%3}
 }
 function View([long]$Id){
  if($Id-ne23){throw 'Chooser observed another bubble'}
  $script:chooserReads++
  if(($script:chooserMode-ne'delayed'-or$script:chooserReads-ge6)-and($script:chooserMode-ne'delayed-each'-or$script:chooserReads-ge$script:chooserPublishAt)){$script:chooserPublishedPhase=$script:chooserPhase;$script:chooserPublishedSelected=$script:chooserSelected}
  $items=[object[]]@()
  if($script:chooserPublishedPhase-eq2){$items=[object[]]@([pscustomobject]@{kind='medium';center=@(0,90);label='Pen';icon='pen';preview=$null;selected=$true;shown=$true;enabled=$true;tag=@{x=-45;y=110;width=90;height=24}})}
  [pscustomobject]@{id=23;kind='dial';enabled=$true;selected=[bool]$script:chooserPublishedSelected;halo=$(if($script:chooserPublishedPhase-eq1){134.0}else{$null});chooser=$items;open=$script:chooserPublishedPhase-ne0}
 }
 function Wait-BubbleVisualLayout{
  if(!@((View 23).chooser).Count){throw 'Chooser layout check preceded its state acknowledgement'}
  $script:chooserLayouts++
 }
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll-lt12;$poll++){if(& $Condition){return}};throw $Message}
 function Start-Sleep([int]$Milliseconds){}
 foreach($mode in 'immediate','delayed','delayed-each','halo','chooser','unselected','no-sliders','missing-selection','missing-controls','missing-chooser'){
  $script:chooserMode=$mode;$script:chooserReads=0;$script:chooserTaps=0;$script:chooserLayouts=0;$script:chooserPublishAt=0
  $script:chooserSelected=$mode-notin@('unselected','missing-selection');$script:chooserPublishedSelected=$script:chooserSelected
  $script:chooserPhase=if($mode-in@('halo','missing-chooser')){1}elseif($mode-eq'chooser'){2}else{0};$script:chooserPublishedPhase=$script:chooserPhase
  $failure=$null;try{Open-BubbleVisualChooser 23}catch{$failure=$_}
  if($mode-like'missing-*'){
   $message=switch($mode){'missing-selection'{'Visual scene dial did not select'}'missing-controls'{'Visual scene dial did not open its controls'}'missing-chooser'{'Visual scene dial did not open its chooser'}}
   if(!$failure-or$failure.Exception.Message-cne$message-or$script:chooserTaps-ne1-or$script:chooserLayouts-ne0){throw "Missing chooser transition was retried or accepted: $mode; $failure"}
  }else{
   if($failure){throw $failure}
   $expected=if($mode-eq'chooser'){0}elseif($mode-in@('halo','no-sliders')){1}else{2}
   if($script:chooserTaps-ne$expected-or$script:chooserLayouts-ne1){throw "Chooser repeated or skipped a semantic transition: $mode"}
   if($mode-eq'immediate'){'Actual chooser: immediate publication control passed with two distinct dial transitions.'}
  }
 }
 'Actual chooser: stage-aware selection, halo and chooser receipts acknowledge once; missing transitions never retry.'
}

& {
 $tokens=$null;$errors=$null
 $edge=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-edge-reach.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 foreach($name in 'WindowCommand','Settled'){
  $fn=$edge.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq$name}.GetNewClosure(),$true)
  if(!$fn){throw "Missing actual edge function $name"}
  $body=$fn.Extent.Text.Replace("& (Join-Path `$PSScriptRoot 'open-application-menu.ps1') -Root `$root -Name 'Window'",'Open-HistoryMenu')
  . ([scriptblock]::Create($body))
 }
 function History-Assertion([string]$Message){
  $edge.FindAll({param($n)$n-is[Management.Automation.Language.IfStatementAst]-and$n.Extent.Text.Contains("throw '$Message'")}.GetNewClosure(),$true)|Sort-Object {$_.Extent.Text.Length}|Select-Object -First 1
 }
 $firstUndo=History-Assertion 'ToolSet handoff Undo did not restore exact post-setup workspace'
 $secondUndo=History-Assertion 'ToolSet setup Undo did not restore exact original workspace'
 $redo=History-Assertion 'Canvas reset Redo did not restore the exact preferred layout'
 if(!$firstUndo-or!$secondUndo-or!$redo){throw 'Missing actual edge history assertions'}
 $statements=@($firstUndo.Parent.Statements);$first=[array]::IndexOf($statements,$firstUndo)-1
 if($statements[$first].Extent.Text-eq'Settled'){$first--}
 $last=[array]::IndexOf($statements,$secondUndo)
 if($first-lt0-or$last-lt$first){throw 'Missing actual successive Undo sequence'}
 $undoSequence=($statements[$first..$last]|ForEach-Object {$_.Extent.Text})-join"`n"
 $statements=@($redo.Parent.Statements);$last=[array]::IndexOf($statements,$redo);$first=$last-1
 if($statements[$first].Extent.Text-eq'Settled'){$first--}
 if($first-lt0){throw 'Missing actual Redo sequence'}
 $redoSequence=($statements[$first..$last]|ForEach-Object {$_.Extent.Text})-join"`n"
 function Open-HistoryMenu{$script:historyOpens++}
 function Invoke($Id){
  $commands=@('undo_workspace','undo_workspace','redo_workspace')
  if($script:historyInvokes-ge$commands.Count-or$Id-cne$commands[$script:historyInvokes]){throw 'Unexpected or repeated history action'}
  $script:historyInvokes++;$script:historyActionReads=0
 }
 function Find($Id){$null}
 function Width($Id){280}
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll-lt12;$poll++){if(& $Condition){return}};throw $Message}
 function Model{
  $script:historyReads++;$script:historyActionReads++
  if($script:historyActionReads-gt6){
   if($script:historyMode-eq'delayed'){$script:historyCurrent=@('setup','original','setup')[$script:historyInvokes-1]}
   elseif($script:historyMode-eq'wrong'){$script:historyCurrent='another'}
  }
  @{workspace_update=@{revision=$script:historyReads};state=@{workspace=@{layout=$script:historyCurrent}}}
 }
 $postSetup='"setup"';$workspace='"original"';$resetLayout='"setup"';$preferred=280;$groups=@(@{id=1})
 foreach($mode in 'delayed','missing','wrong'){
  $script:historyMode=$mode;$script:historyCurrent='handoff';$script:historyReads=0;$script:historyActionReads=0;$script:historyInvokes=0;$script:historyOpens=0
  $failure=$null;try{. ([scriptblock]::Create($undoSequence));. ([scriptblock]::Create($redoSequence))}catch{$failure=$_}
  if($mode-eq'delayed'){
   if($failure-or$script:historyInvokes-ne3-or$script:historyOpens-ne3-or$script:historyCurrent-cne'setup'){
    throw "Stable pre-command layouts acknowledged successive Undo or Redo: $failure"
   }
  }elseif(!$failure-or$failure.Exception.Message-cne'Workspace history command did not acknowledge its exact layout'-or$script:historyInvokes-ne1-or$script:historyOpens-ne1){
   throw 'Menu closure or unrelated revisions acknowledged a missing or wrong exact history result'
  }
 }
 'Actual edge history: successive Undo and Redo await exact results once each; unrelated revisions and absent or wrong targets cannot acknowledge them.'
}

& {
 Add-Type -AssemblyName WindowsBase,System.Drawing,UIAutomationClient,UIAutomationTypes
 Add-Type -TypeDefinition @'
using System;
public static class CapyCaptionOriginTest {
 public struct Point {public int x,y;}
 public struct Rect {public int left,top,right,bottom;}
 public static uint Dpi=144,Owner=7;public static int Inset;public static bool MissingBridge;
 public static uint GetDpiForWindow(IntPtr window){return Dpi;}
 public static uint GetWindowThreadProcessId(IntPtr window,ref uint owner){owner=Owner;return 1;}
 public static bool ClientToScreen(IntPtr window,ref Point point){point.x=51;point.y=85;return true;}
 public static Rect ContentRect(IntPtr window){if(MissingBridge)throw new Exception("The drawing window has no unique native XAML content bounds.");return new Rect{left=51,top=85+Inset,right=1600,bottom=1200};}
}
'@
 $scripts=Join-Path $PSScriptRoot '../scripts'
 foreach($file in 'CapyUia.ps1','capture-number-controls.ps1'){
  $tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $scripts $file),[ref]$tokens,[ref]$errors)
  if($errors.Count){throw($errors|Out-String)}
  foreach($name in $(if($file -eq 'CapyUia.ps1'){@('Native-Bounds','Capture','Read-Snapshot')}else{@('Measure-RenderedInk','Assert-ControlCaptionInk','Save-ControlCaptionInk')})){
   $definition=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name},$true)
   if(!$definition){throw "Missing actual caption helper: $name"}
   . ([scriptblock]::Create($definition.Extent.Text.Replace('[CapyRowPointer+Point]','[CapyCaptionOriginTest+Point]').Replace('[CapyRowPointer]','[CapyCaptionOriginTest]').Replace('[CapyWindowApi]','[CapyCaptionOriginTest]').Replace("'CapyRowPointer' -as [type]","'CapyCaptionOriginTest' -as [type]")))
  }
 }
 function Wait-Until([scriptblock]$Condition,[string]$Message){if(!(& $Condition)){throw $Message}}
 function Model{throw 'Caption geometry control unexpectedly captured a model'}
 function Workspace-Root {$workspace}
 function Expect-CaptionFailure([scriptblock]$Action,[string]$Message){$caught=$null;try{& $Action|Out-Null}catch{$caught=$_.Exception.Message};if(!$caught -or !$caught.Contains($Message)){throw "Caption negative did not reach '$Message': $caught"}}
 $review=[pscustomobject]@{Id=7};$root=[pscustomobject]@{Current=[pscustomobject]@{NativeWindowHandle=19;ProcessId=7}}
 $workspace=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=7;ItemStatus=''}}
 $label=[pscustomobject]@{Current=[pscustomobject]@{Name='Scale';IsOffscreen=$false;BoundingRectangle=[Windows.Rect]::new(81,865,53,30)}}
 $control=[pscustomobject]@{Current=[pscustomobject]@{Name='Scale';AutomationId='canvas-bar-scale';ProcessId=7;BoundingRectangle=[Windows.Rect]::Empty}}
 $control|Add-Member ScriptMethod FindAll {param($Scope,$Condition) @($label)}
 $folder=Join-Path ([IO.Path]::GetTempPath()) ('capy-caption-origin-'+[guid]::NewGuid().ToString('N'));$null=New-Item -ItemType Directory -Path $folder
 $nativeParameters=(Get-Command Native-Bounds).Parameters;$saveParameters=(Get-Command Save-ControlCaptionInk).Parameters
 try{
  foreach($scale in 1,1.5,2){foreach($inset in 0,1){foreach($sample in 0,1){
   [CapyCaptionOriginTest]::Dpi=[uint32](96*$scale);[CapyCaptionOriginTest]::Inset=$inset
   $physicalTop=@(792.1139831542969,777.6513977050781)[$sample];$inkTop=@(803,789)[$sample]
   $bounds=[ordered]@{x=20/$scale;y=($physicalTop-$inset)/$scale;width=110/$scale;height=39/$scale}
   $presentation=[pscustomobject]@{revision=21;model_revision=18;elements=@([pscustomobject]@{id='canvas-bar-scale';coordinate_space='window_content';actual_bounds=[pscustomobject]$bounds})}
   $snapshot=[pscustomobject]@{process_id=7;window_handle=19;dpi=96*$scale;presentation=$presentation}
   $workspace.Current.ItemStatus=$presentation|ConvertTo-Json -Depth 8 -Compress;$snapshot.presentation=$workspace.Current.ItemStatus|ConvertFrom-Json
   $control.Current.BoundingRectangle=[Windows.Rect]::new(71,85+[Math]::Floor($physicalTop),110,39)
   $label.Current.BoundingRectangle=[Windows.Rect]::new(81,85+[Math]::Floor($physicalTop)+3,53,30)
   $image=Join-Path $folder 'source.png';$bitmap=[Drawing.Bitmap]::new(180,860)
   try{$g=[Drawing.Graphics]::FromImage($bitmap);try{$g.Clear([Drawing.Color]::Black);$g.FillRectangle([Drawing.Brushes]::White,31,$inkTop,20,17)}finally{$g.Dispose()};$bitmap.Save($image,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
   $nativeCall=@{Id='canvas-bar-scale';Scale=$scale};if($nativeParameters.ContainsKey('Snapshot')){$nativeCall.Snapshot=$snapshot}
   $native=Native-Bounds @nativeCall
   $save=@{Control=$control;Window=[IntPtr]19;Image=$image;Foreground='#ffffff';Output=(Join-Path $folder 'caption')};if($saveParameters.ContainsKey('NativeBounds')){$save.NativeBounds=$native}
   $record=Save-ControlCaptionInk @save
   if($nativeParameters.ContainsKey('Snapshot')){
    if([Math]::Abs($record.control_bounds[1]-$physicalTop) -gt 1e-9){throw 'Caption native origin lost physical fractional geometry'}
    foreach($delta in -1,1){
     $bitmap=[Drawing.Bitmap]::new(180,860);try{$g=[Drawing.Graphics]::FromImage($bitmap);try{$g.Clear([Drawing.Color]::Black);$g.FillRectangle([Drawing.Brushes]::White,31,($inkTop+$delta),20,17)}finally{$g.Dispose()};$bitmap.Save($image,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
     Expect-CaptionFailure {Save-ControlCaptionInk @save} 'Control caption ink gaps exceed the nearest raster center by'
    }
   }
  }}}
  if(!$nativeParameters.ContainsKey('Snapshot')){throw 'Legacy caption unexpectedly passed both measured fractional cases'}
  $oldScripts=$script:CapyScripts;$oldDelay=$script:CapyCaptureDelay;$oldStatus=$workspace.Current.ItemStatus
  try{
   $script:CapyScripts=$folder;$script:CapyCaptureDelay=0;$run=$folder
   [IO.File]::WriteAllText((Join-Path $folder 'inspect-window.ps1'),"param(`$TraceDirectory,`$ProcessId,`$WindowHandle,`$Output,[switch]`$ClientOnly,[switch]`$Composed,[switch]`$CaptureOnly)`n'capture-provider'")
   $workspace.Current.ItemStatus='{"revision":21,"model_revision":18,"elements":[{"id":"canvas-bar-scale","coordinate_space":"window_content","actual_bounds":{"x":20.123456789012345,"y":518.43426513671875,"width":110,"height":26}}]}'
   $live=$workspace.Current.ItemStatus|ConvertFrom-Json
   Capture 'precision'
   $precision=Read-Snapshot (Join-Path $run 'precision-layout.json')
   if($precision.presentation.elements[0].actual_bounds.y -ne $live.elements[0].actual_bounds.y -or $precision.presentation.elements[0].actual_bounds.x -ne $live.elements[0].actual_bounds.x){throw 'Actual Capture and Read-Snapshot lost native floating geometry precision'}
   if($precision.process_id -ne 7 -or $precision.window_handle -ne 19 -or $precision.dpi -ne [CapyCaptionOriginTest]::Dpi){throw 'Actual Capture lost native snapshot identity'}
   $precisionBounds=Native-Bounds 'canvas-bar-scale' ([CapyCaptionOriginTest]::Dpi/96.) -Snapshot $precision
   $expectedY=[double](85+[CapyCaptionOriginTest]::Inset+$live.elements[0].actual_bounds.y*([CapyCaptionOriginTest]::Dpi/96.))
   if($precisionBounds.Y -ne $expectedY){throw 'Actual captured native bounds lost content origin or precision'}
  }finally{$script:CapyScripts=$oldScripts;$script:CapyCaptureDelay=$oldDelay;$workspace.Current.ItemStatus=$oldStatus}
  foreach($case in @(@{scale=1;top=98.75;ink=103},@{scale=1.5;top=147.625;ink=159},@{scale=2;top=196.5;ink=214})){
   $scale=$case.scale;[CapyCaptionOriginTest]::Dpi=[uint32](96*$scale);[CapyCaptionOriginTest]::Inset=1
   $local=[pscustomobject]@{x=16.75;y=100.25;width=110;height=26};$ancestor=[pscustomobject]@{x=3.25;y=-2.5}
   $arranged=[pscustomobject]@{x=$local.x+$ancestor.x;y=$local.y+$ancestor.y;width=$local.width;height=$local.height}
   $presentation=[pscustomobject]@{revision=21;model_revision=18;elements=@([pscustomobject]@{id='canvas-bar-scale';coordinate_space='window_content';actual_bounds=$arranged})}
   $snapshot=[pscustomobject]@{process_id=7;window_handle=19;dpi=96*$scale;presentation=$presentation};$workspace.Current.ItemStatus=$presentation|ConvertTo-Json -Depth 8 -Compress;$snapshot.presentation=$workspace.Current.ItemStatus|ConvertFrom-Json
   $control.Current.BoundingRectangle=[Windows.Rect]::new(51+20*$scale,85+[Math]::Floor($case.top),110*$scale,26*$scale)
   $label.Current.BoundingRectangle=[Windows.Rect]::new(51+30*$scale,85+$case.top,53*$scale,26*$scale)
   $bitmap=[Drawing.Bitmap]::new(300,860);try{$g=[Drawing.Graphics]::FromImage($bitmap);try{$g.Clear([Drawing.Color]::Black);$g.FillRectangle([Drawing.Brushes]::White,[int](31*$scale),$case.ink,20,17)}finally{$g.Dispose()};$bitmap.Save($image,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
   $nativeCall=@{Id='canvas-bar-scale';Scale=$scale;Snapshot=$snapshot};$native=Native-Bounds @nativeCall
   $save=@{Control=$control;Window=[IntPtr]19;Image=$image;Foreground='#ffffff';Output=(Join-Path $folder 'translated-caption');NativeBounds=$native}
   $record=Save-ControlCaptionInk @save
   if([Math]::Abs($record.control_bounds[0]-20*$scale) -gt 1e-9 -or [Math]::Abs($record.control_bounds[1]-$case.top) -gt 1e-9){throw 'Caption ancestor translation was lost or applied twice'}
  }
  $nativeCall.Snapshot=$snapshot
  $snapshot.presentation.revision--;Expect-CaptionFailure {Native-Bounds @nativeCall} 'Native caption bounds are stale';$snapshot.presentation.revision++
  $snapshot.process_id=8;Expect-CaptionFailure {Native-Bounds @nativeCall} 'Native caption snapshot belongs';$snapshot.process_id=7
  $snapshot.window_handle=20;Expect-CaptionFailure {Native-Bounds @nativeCall} 'Native caption snapshot belongs';$snapshot.window_handle=19
  $snapshot.dpi=96;Expect-CaptionFailure {Native-Bounds @nativeCall} 'Native caption snapshot belongs';$snapshot.dpi=192
  $nativeCall.Scale=1;Expect-CaptionFailure {Native-Bounds @nativeCall} 'Native caption snapshot belongs';$nativeCall.Scale=2
  $savedElements=$snapshot.presentation.elements;$snapshot.presentation.elements=@();Expect-CaptionFailure {Native-Bounds @nativeCall} 'Native caption arranged bounds are missing';$snapshot.presentation.elements=$savedElements
  [CapyCaptionOriginTest]::Owner=8;Expect-CaptionFailure {Native-Bounds @nativeCall} 'Native caption bounds belong';[CapyCaptionOriginTest]::Owner=7
  [CapyCaptionOriginTest]::MissingBridge=$true;Expect-CaptionFailure {Native-Bounds @nativeCall} 'unique native XAML content bounds';[CapyCaptionOriginTest]::MissingBridge=$false
  $control.Current.ProcessId=8;Expect-CaptionFailure {Save-ControlCaptionInk @save} 'Native caption control belongs';$control.Current.ProcessId=7
  $save.Remove('NativeBounds');Expect-CaptionFailure {Save-ControlCaptionInk @save} 'Canvas caption requires native arranged bounds'
  'Actual Caption helpers: floating reference, 100/150/200% content origins, displaced ink and identity negatives passed.'
 }finally{Remove-Item -LiteralPath $folder -Recurse -Force}
}

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-column-stacks.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $actual=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Drag'},$true)
 if(!$actual){throw 'Missing actual column Drag function'}
 . ([scriptblock]::Create($actual.Extent.Text.Replace('[CapyRowPointer]','[CapyEdgeReleaseTest]')))
 function Presentation{throw 'Contact capture control unexpectedly read drag presentation'}
 function Dock-Point{throw 'Contact capture control unexpectedly resolved a drop target'}
 function Capture{throw 'Contact capture control unexpectedly captured pixels'}
 function Model{throw 'Contact capture control unexpectedly read a model'}
 $Device='touch'
 function At($Id){@{x=10;y=20}}
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll-lt8;$poll++){if(& $Condition){return}};throw $Message}
 function Gesture{
  $phase=[CapyEdgeReleaseTest]::Phase
  if($phase-eq1-and$script:columnCapture.mode-eq'stale-clear'){$script:columnCapture.clears++}
  $clear=$phase-eq1-and$script:columnCapture.mode-eq'stale-clear'-and$script:columnCapture.clears-le2
  $value=[pscustomobject]@{generation=$(if($phase-eq1-and!$clear){2}else{$phase});requires_hold=$clear;device=$(if($script:columnCapture.mode-eq'wrong-device'){'pen'}else{'touch'});phase=$(if($phase-eq2-or$clear){'idle'}else{'dragging'});pointer_id=$(if($phase-eq1-and!$clear){7}else{$null});menu_open=$false}
  $value|Add-Member ScriptProperty captured {
   if([CapyEdgeReleaseTest]::Phase-ne1){throw 'Capture was queried outside the admitted contact'}
   $script:columnCapture.reads++
   $script:columnCapture.mode-ne'lost'-and$script:columnCapture.reads-ge2
  }
  $value
 }
 foreach($mode in 'delayed','stale-clear','lost','unguarded','wrong-device'){
  $script:columnCapture=@{mode=$mode;reads=0;clears=0}
  [CapyEdgeReleaseTest]::Phase=0;[CapyEdgeReleaseTest]::Downs=0;[CapyEdgeReleaseTest]::Ups=0
  $failure=$null;try{Drag 'group-grip-7' @{x=30;y=40} -Captured:($mode-ne'unguarded')}catch{$failure=$_}
  if($mode-in'delayed','stale-clear'-and$failure){throw $failure}
  if($mode-eq'lost'){
   if(!$failure-or$failure.Exception.Message-cne'Immediate custom handle did not capture its contact on press'-or$script:columnCapture.reads-ne8-or[CapyEdgeReleaseTest]::Ups-ne0){throw 'Missing column capture did not retain its original failure'}
  }elseif($mode-eq'wrong-device'){
   if(!$failure-or$failure.Exception.Message-cne'Source used the wrong device or pickup policy'-or$script:columnCapture.reads-ne0){throw 'Column pickup policy failure was weakened'}
  }else{
   $expectedReads=if($mode-eq'unguarded'){0}else{2}
   if($failure-or$script:columnCapture.reads-ne$expectedReads-or[CapyEdgeReleaseTest]::Downs-ne1-or[CapyEdgeReleaseTest]::Ups-ne1){throw "Actual column Drag did not preserve delayed capture or its optional guard: $failure"}
  }
 }
 'Actual column Drag: the new contact record, not an earlier clear, acknowledges Down; delayed capture waits; missing capture, optional guard and device policy preserved.'
}

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-tool-bubbles.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $placed=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Placed'},$true)
 if(!$placed){throw 'Missing actual bubble placement acknowledgement'}
 . ([scriptblock]::Create($placed.Extent.Text))
 function Screen([double]$X,[double]$Y){@{x=[int][Math]::Round($X*1.5);y=[int][Math]::Round($Y*1.5)}}
 function Center([long]$Id){$script:placedNative}
 function Model{[pscustomobject]@{layout=[pscustomobject]@{viewport=@(1706.6666259765625,1018.6666870117188)};state=[pscustomobject]@{workspace=[pscustomobject]@{layout=[pscustomobject]@{bubbles=[pscustomobject]@{bubbles=@([pscustomobject]@{id=23;corner=$script:placedCorner;offset=$script:placedOffset})}}}}}}
 $script:placedCorner='bottom_right';$script:placedOffset=@(-819.6358642578125,-367.50531005859375);$script:placedNative=@{x=1292;y=960}
 if(Placed 23){throw 'A card short of its paired authored placement acknowledged placement'}
 $script:placedNative=@{x=1331;y=977}
 if(!(Placed 23)){throw 'A card at its authored bottom-right placement was rejected'}
 $script:placedCorner='top_left';$script:placedOffset=@(200,100);$script:placedNative=@{x=300;y=150}
 if(!(Placed 23)){throw 'A card at its authored top-left placement was rejected'}
 'Actual bubble placement: a native card short of its paired authored placement is unplaced; both corner bases resolve.'
}

& {
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-clipboard.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $actual=$source.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Cross-Window-Clip'},$true)
    $handoff=$actual.Body.Find({param($node)$node-is[Management.Automation.Language.TryStatementAst]},$true)
    $prefix=[Collections.Generic.List[string]]::new()
    foreach($statement in $handoff.Body.Statements){
        if($statement-is[Management.Automation.Language.PipelineAst]-and$statement.PipelineElements[0]-is[Management.Automation.Language.CommandAst]-and$statement.PipelineElements[0].GetCommandName()-eq'Chord'){break}
        $prefix.Add($statement.Extent.Text)
    }
    if(!$prefix.Count){throw 'Missing actual clipboard window handoff'}
    $savedRoot=$script:root;$savedHandle=$script:drawingWindow;$savedState=$script:CapyStateFile;$savedEach=$script:CapyEach
    $sourceRoot=[CapyOwnedSearchTest.Node]::new('clipboard-source',33,0,0)
    $peerRoot=[CapyOwnedSearchTest.Node]::new('clipboard-peer',33,200,200)
    $sourceHandle=[IntPtr]101;$peerHandle=[IntPtr]202
    $sourceRoot.NativeWindowHandle=$sourceHandle;$peerRoot.NativeWindowHandle=$peerHandle
    $peer=@{id=202;hwnd=202};$review=[CapyOwnedSearchTest.Review]::new();$run='clipboard-handoff'
    $script:root=$sourceRoot;$script:drawingWindow=$sourceHandle;$script:clipboardHandoffTicks=0
    function Model{@{brush_ready=$true;windows_workspace=@{ready=$true;busy=$false}}}
    $peerRoot.Children.Add([CapyOwnedSearchTest.Node]::new('drawing-canvas',33,210,210))
    function Owned-ProcessWindows($Process,[long]$WindowHandle){
        if($Process.Id-ne33-or$WindowHandle-ne202){throw 'Clipboard readiness requested another owned window'}
        @{Root=$peerRoot;Handle=$peerHandle}
    }
    function Owned-DrawingWindow($Process,[long]$WindowHandle){
        if($Process.Id-ne33-or$WindowHandle-ne202){throw 'Clipboard handoff requested another owned window'}
        if($script:clipboardHandoffTicks-ge3){@{Root=$peerRoot;Handle=$peerHandle}}
    }
    $script:CapyEach={
        if($script:root-ne$sourceRoot-or$script:drawingWindow-ne$sourceHandle){throw 'Clipboard handoff bound its root before native readiness'}
        $script:clipboardHandoffTicks++
    }
    try{
        . ([scriptblock]::Create(($prefix-join[Environment]::NewLine)))
        if($script:clipboardHandoffTicks-ne3-or$script:root-ne$peerRoot-or$script:drawingWindow-ne$peerHandle){throw 'Clipboard handoff accepted model readiness before its native window became ready'}
    }finally{
        $script:root=$savedRoot;$script:drawingWindow=$savedHandle;$script:CapyStateFile=$savedState;$script:CapyEach=$savedEach
        Remove-Variable clipboardHandoffTicks -Scope Script
    }
}
Write-Output 'Actual clipboard handoff waits for its native owned window before binding the destination root, even when its shared model is already ready.'

& {
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-clipboard.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $actual=$source.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Chord'},$true)
    $prefix=[Collections.Generic.List[string]]::new();$dispatch=$null
    foreach($statement in $actual.Body.EndBlock.Statements){
        $dispatch=$statement.Find({param($node)$node-is[Management.Automation.Language.InvokeMemberExpressionAst]-and$node.Static-and$node.Member.Value-eq'Chord'-and$node.Expression.TypeName.FullName-eq'CapyRowPointer'},$true)
        if($dispatch){break}
        $prefix.Add($statement.Extent.Text)
    }
    if(!$dispatch-or!$prefix.Count){throw 'Missing actual clipboard keyboard dispatch'}
    $savedRoot=$script:root;$savedHandle=$script:drawingWindow;$savedEach=$script:CapyEach
    $savedPopups=$script:CapyPopups;$savedFind=$script:CapyFind;$savedForeground=[CapyOwnedSearchTest.Driver]::Foreground
    $sourceRoot=[CapyOwnedSearchTest.Node]::new('clipboard-source',33,0,0)
    $peerRoot=[CapyOwnedSearchTest.Node]::new('clipboard-peer',33,200,200)
    $sourceRoot.NativeWindowHandle=[IntPtr]101;$peerRoot.NativeWindowHandle=[IntPtr]202
    $ownCanvas=[CapyOwnedSearchTest.Node]::new('drawing-canvas',33,10,10)
    $peerCanvas=[CapyOwnedSearchTest.Node]::new('drawing-canvas',33,210,210);$peerRoot.Children.Add($peerCanvas)
    $review=[CapyOwnedSearchTest.Review]::new()
    $script:root=$sourceRoot;$script:drawingWindow=$sourceRoot.NativeWindowHandle
    $script:CapyPopups=$true;$script:CapyFind='prefer-visible';$script:clipboardCanvasTicks=0
    [CapyOwnedSearchTest.Driver]::Foreground=[IntPtr]::Zero
    function Owned-ProcessWindows($Process){foreach($window in @($sourceRoot,$peerRoot)){@{Root=$window;Handle=$window.NativeWindowHandle}}}
    $script:CapyEach={
        if($peerCanvas.HasKeyboardFocus){throw 'Clipboard canvas focus escaped its current native window'}
        $script:clipboardCanvasTicks++
        if($script:clipboardCanvasTicks-eq3){$sourceRoot.Children.Add($ownCanvas)}
    }
    try{
        . ([scriptblock]::Create(($prefix-join[Environment]::NewLine).Replace('[CapyRowPointer]','[CapyOwnedSearchTest.Driver]')))
        if(!$ownCanvas.HasKeyboardFocus-or$peerCanvas.HasKeyboardFocus-or[CapyOwnedSearchTest.Driver]::Foreground-ne$script:drawingWindow-or$script:clipboardCanvasTicks-lt3){throw 'Clipboard input did not focus its current native canvas before keyboard dispatch'}
    }finally{
        $script:root=$savedRoot;$script:drawingWindow=$savedHandle;$script:CapyEach=$savedEach
        $script:CapyPopups=$savedPopups;$script:CapyFind=$savedFind;[CapyOwnedSearchTest.Driver]::Foreground=$savedForeground
        Remove-Variable clipboardCanvasTicks -Scope Script
    }
}
Write-Output 'Actual clipboard keyboard preparation waits for its current native canvas and never focuses another registered window before dispatch.'

& {
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-clipboard.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $merged=$source.Find({param($node)$node-is[Management.Automation.Language.AssignmentStatementAst]-and$node.Left-is[Management.Automation.Language.VariableExpressionAst]-and$node.Left.VariablePath.UserPath-eq'merged'},$true)
    $start=@($source.FindAll({param($node)$node-is[Management.Automation.Language.CommandAst]-and$node.GetCommandName()-eq'Tool'-and$node.CommandElements[1].Value-eq'lasso'-and$node.Extent.StartOffset-lt$merged.Extent.StartOffset},$true))[-1]
    $text=$source.Extent.Text.Substring($start.Extent.StartOffset,$merged.Extent.StartOffset-$start.Extent.StartOffset).Replace('[System.Windows.Automation.ExpandCollapsePattern]','[CapyOwnedSearchTest.InvokePattern]')
    $savedRoot=$script:root;$savedEach=$script:CapyEach;$savedPopups=$script:CapyPopups;$savedFind=$script:CapyFind
    try{foreach($route in 'direct','overflow'){
        $script:root=[CapyOwnedSearchTest.Node]::new('clipboard-window',33,0,0)
        $barNode=[CapyOwnedSearchTest.Node]::new('canvas-action-bar',33,10,10);$script:root.Children.Add($barNode)
        $moreNode=[CapyOwnedSearchTest.Node]::new('canvas-bar-more',33,20,20);$barNode.Children.Add($moreNode)
        $copyNode=[CapyOwnedSearchTest.Node]::new('canvas-bar-menu-copy',33,30,20)
        $review=[CapyOwnedSearchTest.Review]::new();$script:CapyPopups=$true;$script:CapyFind='prefer-visible'
        $script:clipboardPlacementTicks=0;$script:clipboardPlacementInvokes=0;$script:clipboardPlacementExpands=0
        function Tool($Id){if($Id-ne'lasso'){throw 'Clipboard placement selected another tool'}}
        function Model{@{state=@{canvas_bar=@{context=@{kind='selection';generation=7};items=@(@{menu='other'},@{menu='copy'},@{menu='mask'})}};workspace_update=@{revision=10;model_revision=20}}}
        function Workspace-Root{
            $generation=if($script:clipboardPlacementTicks-lt2){6}else{7}
            $revision=if($script:clipboardPlacementTicks-eq2){9}else{10}
            $modelRevision=if($script:clipboardPlacementTicks-eq3){19}else{20}
            $shown=if($route-eq'direct'){2}else{1}
            @{Current=@{ItemStatus=(@{revision=$revision;model_revision=$modelRevision;workspace_update=@{canvas_bar=@{context=@{kind='selection';generation=$generation};layout=@{items=$shown}}}}|ConvertTo-Json -Depth 8 -Compress)}}
        }
        function Invoke($Id,$Within){
            if($script:clipboardPlacementTicks-lt4-or($route-eq'direct'-and$Id-ne'canvas-bar-menu-copy')){throw 'Clipboard selected overflow before native Copy placement was acknowledged'}
            if($Within-ne$barNode-or($route-eq'overflow'-and$Id-ne'canvas-bar-more')){throw 'Clipboard Copy invocation escaped its arranged native bar'}
            $script:clipboardPlacementInvokes++
        }
        $expand=[pscustomobject]@{};$expand|Add-Member ScriptMethod Expand {$script:clipboardPlacementExpands++}
        $menu=[pscustomobject]@{Pattern=$expand};$menu|Add-Member ScriptMethod GetCurrentPattern {param($Pattern)$this.Pattern}
        function Menu-Item($Name){if($Name-ne'Copy'-or$route-ne'overflow'-or$script:clipboardPlacementInvokes-ne1){throw 'Clipboard requested an unexpected overflow menu'};$menu}
        $script:CapyEach={
            $script:clipboardPlacementTicks++
            if($route-eq'direct'-and$script:clipboardPlacementTicks-eq5){$barNode.Children.Add($copyNode)}
        }
        . ([scriptblock]::Create($text))
        $expectedExpands=if($route-eq'overflow'){1}else{0}
        if($script:clipboardPlacementInvokes-ne1-or$script:clipboardPlacementExpands-ne$expectedExpands-or($route-eq'direct'-and$script:clipboardPlacementTicks-lt5)){throw 'Clipboard Copy placement did not invoke its acknowledged route exactly once'}
    }}finally{
        $script:root=$savedRoot;$script:CapyEach=$savedEach;$script:CapyPopups=$savedPopups;$script:CapyFind=$savedFind
        Remove-Variable clipboardPlacementTicks,clipboardPlacementInvokes,clipboardPlacementExpands -Scope Script
    }
}
Write-Output 'Actual clipboard Copy routing rejects stale native context and revisions, waits for a late direct control and invokes direct or genuine overflow exactly once.'

& {
 $tokens=$null;$errors=$null
 $penAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-pen-buttons.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $definition=@($penAst.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Select-EraserOracleBrush'},$true))
 if($definition.Count -ne 1){throw 'Missing actual Select-EraserOracleBrush helper'}
 . ([scriptblock]::Create($definition[0].Extent.Text))
 $calls=@($penAst.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -ceq 'Select-EraserOracleBrush'},$true))
 if($calls.Count -ne 1){throw 'The eraser oracle must select its brush exactly once'}
 function Tool-Tile([string]$Command){if($Command -cne 'pen'){throw 'The eraser oracle selected another tool'};'tile-tools-34'}
 function Wait-Until([scriptblock]$Condition,[string]$Message){
  $script:eraserChoice.waits.Add($Message)
  for($poll=0;$poll -lt 8;$poll++){if(& $Condition){return}}
  throw $Message
 }
 function Model {
  $c=$script:eraserChoice
  $subtools=@([pscustomobject]@{preview=2;label='Pencil'},[pscustomobject]@{preview=25;label='Pointy Pencil'},[pscustomobject]@{preview=26;label='Shading Pencil'})
  $preset=2;$ready=$false
  if($c.phase -ne 'pencil'){
   $c.modelReads++
   if($c.modelReads -gt 2){
    $subtools=@([pscustomobject]@{preview=1;label='G-Pen'},[pscustomobject]@{preview=44;label='Monoline'})
    if($c.mode -eq 'missing'){$subtools[0].preview=2}
    if($c.mode -eq 'duplicate'){$subtools[1].preview=1}
   }
  }
  if($c.phase -eq 'ack'){
   $c.ackReads++
   if($c.ackReads -gt 2 -and $c.mode -ne 'preset-not-acknowledged'){$preset=1}
   $ready=$c.ackReads -gt 3 -and $c.mode -ne 'brush-not-ready'
  }
  [pscustomobject]@{brush_ready=$ready;state=[pscustomobject]@{brush=[pscustomobject]@{preset=$preset};tool_set=[pscustomobject]@{subtools=$subtools}}}
 }
 function Find([string]$Id){
  $c=$script:eraserChoice
  if($Id -cne 'tool-subtool-0'){throw 'The eraser oracle used a label instead of the semantic G-Pen index'}
  $c.nativeReads++
  $c.nativeReady=$c.modelReads -gt 2 -and $c.nativeReads -gt 2 -and $c.mode -ne 'wrong-native-label'
  [pscustomobject]@{Current=[pscustomobject]@{Name=$(if($c.nativeReady){'G-Pen'}else{'Pencil'})}}
 }
 function Invoke-Id([string]$Id){
  $c=$script:eraserChoice
  if($Id -ceq 'tile-tools-34'){
   if($c.phase -ne 'pencil'){throw 'The eraser oracle invoked Pen more than once'}
   $c.phase='pen'
  }elseif($Id -ceq 'tool-subtool-0'){
   if(!$c.nativeReady -or $c.phase -ne 'pen'){throw 'The eraser oracle invoked the stale Pencil peer'}
   $c.phase='ack'
  }else{throw "Unexpected eraser oracle invocation: $Id"}
  $c.invokes.Add($Id)
 }
 $shared='The Pen group did not expose one G-Pen preset for the eraser oracle'
 $native='The native G-Pen choice did not match the shared eraser oracle preset'
 $ready='G-Pen did not become ready for the eraser oracle'
 foreach($case in @(@('ready',$null),@('missing',$shared),@('duplicate',$shared),@('wrong-native-label',$native),@('preset-not-acknowledged',$ready),@('brush-not-ready',$ready))){
  $script:eraserChoice=@{mode=$case[0];phase='pencil';modelReads=0;nativeReads=0;nativeReady=$false;ackReads=0;invokes=[Collections.Generic.List[string]]::new();waits=[Collections.Generic.List[string]]::new()}
  $failure=$null;try{Select-EraserOracleBrush}catch{$failure=$_.Exception.Message}
  $c=$script:eraserChoice
  if($failure -cne $case[1]){throw "Eraser oracle case $($case[0]) reached unexpected stage: $failure"}
  $expected=if($case[0] -in 'ready','preset-not-acknowledged','brush-not-ready'){'tile-tools-34,tool-subtool-0'}else{'tile-tools-34'}
  if(($c.invokes -join ',') -cne $expected){throw "Eraser oracle case $($case[0]) changed exactly-once Pen/G-Pen input"}
  if($case[0] -eq 'ready' -and ($c.nativeReads -lt 3 -or $c.ackReads -lt 4 -or ($c.waits -join '|') -cne (@($shared,$native,$ready) -join '|'))){throw 'The eraser oracle skipped delayed shared/native/brush acknowledgement'}
 }
 Remove-Variable eraserChoice -Scope Script
 'Actual eraser oracle brush helper: delayed Pen model and native G-Pen label precede exactly-once selection; missing/duplicate preview1, stale Pencil label, missing preset ACK and unready brush fail at their named stages.'
}

& {
 $tokens=$null;$errors=$null
 $drawingAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $definition=@($drawingAst.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'New-DrawingPixels'},$true))
 if($definition.Count -ne 1){throw 'Missing actual New-DrawingPixels helper'}
 $text=$definition[0].Extent.Text
 foreach($pattern in 'ExpandCollapsePattern','SelectionItemPattern','ValuePattern'){$text=$text.Replace("System.Windows.Automation.$pattern",'CapyOwnedSearchTest.InvokePattern')}
 . ([scriptblock]::Create($text))
 function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8){
  $script:drawingCase.waits.Add([pscustomobject]@{message=$Message;seconds=$Seconds})
  for($poll=0;$poll -lt 3;$poll++){if(& $Condition){return}}
  throw $Message
 }
 function Model {
  $script:drawingCase.modelReads++
  if($script:drawingCase.mode -eq 'missing-model' -or ($script:drawingCase.mode -eq 'delayed-model' -and $script:drawingCase.modelReads -le 2)){return [pscustomobject]@{state=[pscustomobject]@{new_drawing=$null}}}
  [pscustomobject]@{state=[pscustomobject]@{new_drawing=$script:drawingCase.view}}
 }
 function New-DrawingControl($Id,$Within){
  $case=$script:drawingCase
  switch($Id){
   'new-drawing-unit' {$case.unitReads++;if($case.mode -ne 'missing-unit' -and $case.unitReads -eq 1){return $case.unit}}
   'new-drawing-unit-1' {if($Within -ne $case.unit){throw 'Pixel choice lost its captured unit'};return $case.choice}
   'document-width' {if($case.mode -ne 'missing-width'){return $case.width}}
   'document-height' {return $case.height}
   default {throw "Unexpected drawing control: $Id"}
  }
 }
 function Reset-DrawingCase($Mode){
  $script:drawingCase=@{mode=$Mode;unitReads=0;modelReads=0;expanded=0;selected=0;focus=[Collections.Generic.List[string]]::new();fields=[Collections.Generic.List[string]]::new();waits=[Collections.Generic.List[object]]::new();view=[pscustomobject]@{setup=[pscustomobject]@{unit='cm'};units=@([pscustomobject]@{value='cm'},[pscustomobject]@{value='px'});values=@(10,20)}}
  if($Mode -eq 'already-pixels'){$script:drawingCase.view.setup.unit='px'}
  $unit=[pscustomobject]@{};$unit|Add-Member ScriptMethod GetCurrentPattern {param($Pattern) $this};$unit|Add-Member ScriptMethod Expand {$script:drawingCase.expanded++};$script:drawingCase.unit=$unit
  $choice=[pscustomobject]@{};$choice|Add-Member ScriptMethod GetCurrentPattern {param($Pattern) $this};$choice|Add-Member ScriptMethod Select {$script:drawingCase.selected++;if($script:drawingCase.mode -ne 'selection-stalled'){$script:drawingCase.view.setup.unit='px'}};$script:drawingCase.choice=$choice
  foreach($field in @(@('width',0),@('height',1))){
   $entry=[pscustomobject]@{id=$field[0];index=$field[1]};$entry|Add-Member ScriptMethod GetCurrentPattern {param($Pattern) $this};$entry|Add-Member ScriptProperty Current {$this};$entry|Add-Member ScriptProperty Value {if($script:drawingCase.mode -eq 'sizes-stalled'){'999'}else{[string]$script:drawingCase.view.values[$this.index]}}
   $entry|Add-Member ScriptMethod SetFocus {$script:drawingCase.focus.Add($this.id)}
   $entry|Add-Member ScriptMethod SetValue {param($Value) $script:drawingCase.fields.Add($this.id+'='+$Value);if($script:drawingCase.mode -ne 'commit-stalled'){$script:drawingCase.view.values[$this.index]=[int]$Value}}
   $script:drawingCase[$field[0]]=$entry
  }
 }
 foreach($mode in 'ready','delayed-model','already-pixels'){
  Reset-DrawingCase $mode
  New-DrawingPixels 320 240
  if($script:drawingCase.unitReads -ne 1){throw 'New drawing relooked up the ready unit'}
  if(($script:drawingCase.fields -join ',') -cne 'width=320,height=240' -or ($script:drawingCase.focus -join ',') -cne 'width,height,width' -or $script:drawingCase.view.values[0] -ne 320 -or $script:drawingCase.view.values[1] -ne 240){throw 'New drawing changed dimension field commits or focus order'}
  $expected=[int]($mode -ne 'already-pixels');if($script:drawingCase.expanded -ne $expected -or $script:drawingCase.selected -ne $expected){throw 'New drawing changed pixel-unit selection'}
  if($script:drawingCase.waits[0].message -cne 'New drawing did not open' -or $script:drawingCase.waits[0].seconds -ne 30){throw 'New drawing changed its readiness predicate bound'}
 }
 foreach($negative in @(@('missing-model','New drawing did not open'),@('missing-width','New drawing did not open'),@('missing-unit','New drawing did not open'),@('selection-stalled','New drawing did not switch to pixels'),@('sizes-stalled','New drawing did not show pixel sizes'),@('commit-stalled','New drawing did not apply pixel sizes'))){
  Reset-DrawingCase $negative[0];$caught=$null
  try{New-DrawingPixels 320 240}catch{$caught=$_.Exception.Message}
  if($caught -cne $negative[1]){throw "New drawing negative $($negative[0]) reached unexpected predicate: $caught"}
  if($script:drawingCase.waits[0].seconds -ne 30){throw 'Missing drawing readiness changed its original bound'}
  if($negative[0] -like 'missing-*' -and ($script:drawingCase.expanded -ne 0 -or $script:drawingCase.fields.Count -ne 0)){throw 'Missing drawing readiness reached creation input'}
 }
 'Actual New-DrawingPixels: captured ready unit once, retained pixel selection and field commits, missing/stalled predicates rejected.'
}

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-enclose-fill.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $definition=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Assert-FillDrawerChoices'},$true)
 if(!$definition){throw 'Missing actual Assert-FillDrawerChoices helper'}
 . ([scriptblock]::Create($definition.Extent.Text))
 $calls=@($source.FindAll({param($n)$n-is[Management.Automation.Language.CommandAst]-and$n.GetCommandName()-eq'Assert-FillDrawerChoices'},$true))
 if($calls.Count-ne1){throw 'Actual Sketch Fill sibling assertion is not called exactly once'}
 $actual=[scriptblock]::Create($calls[0].Extent.Text)
 $expected=@('fill','lasso_fill','enclose_fill','paint_unfilled');$names=@('Fill','Lasso fill','Enclose and Fill','Paint unfilled areas')
 function Model{$script:fillDrawerModel}
 function Control($Id,$Within,[switch]$Arranged){
  if($Id-eq'tool-drawer'){return 'owned-fill-drawer'}
  if($Within-cne'owned-fill-drawer'-or!$Arranged){throw 'Fill sibling lookup escaped the arranged owned drawer'}
  $script:fillDrawerLookups.Add($Id)
  if(!$script:fillDrawerRows.ContainsKey($Id)){throw "Missing control: $Id"}
  $script:fillDrawerRows[$Id]
 }
 foreach($mode in @('valid','group','mixed','missing','misordered','wrong-label','wrong-command','wrong-anchor','wrong-selection','extra')){
  $anchor=[pscustomobject]@{kind='header';id=10};$choices=@();$script:fillDrawerRows=@{};$script:fillDrawerLookups=[Collections.Generic.List[string]]::new()
  for($i=0;$i-lt4;$i++){
   $choice=[pscustomobject]@{label=$names[$i];selected=$i-eq2;action=[pscustomobject]@{type='choose_tool_variant';anchor=$anchor;variant=[pscustomobject]@{type='command';command=$expected[$i]}}};$choices+=,$choice
   $script:fillDrawerRows['tool-subtool-'+$i]=[pscustomobject]@{Current=[pscustomobject]@{Name=$names[$i]}}
  }
  $fill=[pscustomobject]@{id=10};$ids=@('tool-subtool-0','tool-subtool-1','tool-subtool-2','tool-subtool-3')
  $script:fillDrawerModel=[pscustomobject]@{state=[pscustomobject]@{customization=[pscustomobject]@{drawer=[pscustomobject]@{anchor=$anchor;tool_set=[pscustomobject]@{groups=@();subtools=$choices}}}}}
  switch($mode){
   'group'{$script:fillDrawerModel.state.customization.drawer.tool_set.groups=$choices;$script:fillDrawerModel.state.customization.drawer.tool_set.subtools=@();$ids=@('tool-group-0','tool-group-1','tool-group-2','tool-group-3')}
   'mixed'{$script:fillDrawerModel.state.customization.drawer.tool_set.groups=@($choices[0],$choices[1]);$script:fillDrawerModel.state.customization.drawer.tool_set.subtools=@($choices[2],$choices[3]);$ids=@('tool-group-0','tool-group-1','tool-subtool-0','tool-subtool-1')}
   'missing'{[void]$script:fillDrawerRows.Remove('tool-subtool-3')}
   'misordered'{$choices[0].action.variant.command='lasso_fill';$choices[1].action.variant.command='fill'}
   'wrong-label'{$script:fillDrawerRows['tool-subtool-3'].Current.Name='Different'}
   'wrong-command'{$choices[3].action.variant.command='selection_visible'}
   'wrong-anchor'{$choices[3].action.anchor=[pscustomobject]@{kind='header';id=11}}
   'wrong-selection'{$choices[2].selected=$false}
   'extra'{$script:fillDrawerModel.state.customization.drawer.tool_set.subtools+=,$choices[0]}
  }
  if($mode-in@('group','mixed')){$script:fillDrawerRows=@{};for($i=0;$i-lt4;$i++){$script:fillDrawerRows[$ids[$i]]=[pscustomobject]@{Current=[pscustomobject]@{Name=$names[$i]}}}}
  $failure=$null;try{& $actual}catch{$failure=$_}
  if($mode-in@('valid','group','mixed')){
   if($failure){throw $failure}
   if(($script:fillDrawerLookups-join',')-cne($ids-join',')){throw 'Actual Fill assertion did not check all four ordered native siblings'}
  }elseif(!$failure){throw "Actual Fill assertion accepted $mode siblings"}
 }
 'Actual Sketch Fill drawer assertion checks four ordered shared/native choices and rejects invalid rows'
}

& {
 $tokens=$null;$errors=$null
 $cursorAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-cursors.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $tool=$cursorAst.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Tool'},$true)
 if(!$tool){throw 'Missing actual cursor Tool helper'}
 $toolWait=@($tool.FindAll({param($node)$node-is[Management.Automation.Language.CommandAst]-and$node.GetCommandName()-eq'Wait-Canvas'},$true))[-1]
 $baseline=@($cursorAst.FindAll({param($node)$node-is[Management.Automation.Language.AssignmentStatementAst]-and$node.Left.Extent.Text-eq'$empty'},$true))
 $leave=@($cursorAst.FindAll({param($node)$node-is[Management.Automation.Language.CommandAst]-and$node.GetCommandName()-eq'Wait-Until'-and$node.CommandElements[-1].Extent.Text-eq"'Leaving the pointer did not clear the retained cursor'"},$true))
 if(!$toolWait-or$baseline.Count-ne1-or$leave.Count-ne1){throw 'Missing actual cursor readiness, pixel reference or leave assertion'}
 $actualReady=[scriptblock]::Create($toolWait.Extent.Text)
 $actualBaseline=[scriptblock]::Create($baseline[0].Extent.Text)
 $actualLeave=[scriptblock]::Create($leave[0].Extent.Text)
 function Find{param($Id,[switch]$Visible)$null}
 function Model{
  [pscustomobject]@{brush_ready=($script:cursorBlocked-ne'brush');canvas_ready=($script:cursorBlocked-ne'canvas');shaders_ready=($script:cursorPrepared-and$script:cursorBlocked-ne'shaders');state=[pscustomobject]@{commands=@([pscustomobject]@{id='pen';selected=$true})}}
 }
 function Pixels{if($script:cursorPrepared){'textured paper'}else{'uniform paper'}}
 function Wait-StablePixels([scriptblock]$Sample){& $Sample}
 function Wait-Until([scriptblock]$Predicate,[string]$Message){
  for($poll=0;$poll-lt2;$poll++){if(& $Predicate){return};$script:cursorPrepared=$true}
  throw $Message
 }
 $Id='pen'
 $script:cursorBlocked='';$script:cursorPrepared=$false
 . $actualReady
 . $actualBaseline
 $script:cursorPrepared=$true
 . $actualLeave
 if($empty-cne'textured paper'){throw 'Actual cursor baseline preceded complete presentation preparation'}
 foreach($blocked in 'brush','canvas','shaders'){
  $script:cursorBlocked=$blocked;$script:cursorPrepared=$false;$failure=$null
  try{. $actualReady}catch{$failure=$_}
  if(!$failure-or$failure.Exception.Message-cne'Tool did not activate: pen'){throw "Actual cursor readiness admitted missing $blocked preparation"}
 }
 'Actual cursor readiness waits for optional presentation preparation before its unchanged pixel reference and exact leave assertion.'
}

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-toolbar-library.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $helper=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-ceq'Toolbar-Action'},$true)
 if(!$helper){throw 'Missing actual Toolbar-Action'}
 . ([scriptblock]::Create($helper.Extent.Text))
 $call=$source.Find({param($n)$n-is[Management.Automation.Language.CommandAst]-and$n.GetCommandName()-ceq'Toolbar-Action'-and$n.CommandElements.Count-eq2-and$n.CommandElements[1].Value-ceq'delete'},$true)
 $statement=$call.Parent;$statements=@($statement.Parent.Statements);$index=[array]::IndexOf($statements,$statement)
 $sequence=($statements[$index..($index+2)]|ForEach-Object {$_.Extent.Text})-join"`n"
 if($sequence-notlike"*Invoke 'Cancel'*"){throw 'Missing actual delete-to-Cancel journey'}
 function Manager{
  $state=$script:toolbarPrompt
  if($state.requested-and$state.core-ge0-and$state.poll-ge$state.core){[pscustomobject]@{prompt=[pscustomobject]@{confirm='Delete'}}}else{[pscustomobject]@{prompt=$null}}
 }
 function Control($Id){if($Id-cne'workspace-manager'){throw 'Unexpected toolbar prompt owner'};'owned-manager'}
 function Find($Id,[switch]$Name,$Within,$Type,[switch]$Visible){
  if($Id-cne'Delete'-or!$Name-or$Within-cne'owned-manager'-or!$Visible-or$Type-ne[System.Windows.Automation.ControlType]::Button){throw 'Toolbar prompt acknowledgement lost its native owner/button/visibility'}
  $state=$script:toolbarPrompt;if($state.native-ge0-and$state.poll-ge$state.native){[pscustomobject]@{Current=[pscustomobject]@{IsEnabled=$true}}}
 }
 function Invoke($Id,[switch]$Name,$Within,$Type){
  $state=$script:toolbarPrompt;$state.inputs+=,$Id
  if($Id-eq'workspace-toolbar-delete'){$state.requested=$true}
  if($Id-eq'Cancel'){
   if(!$state.requested-or$state.core-lt0-or$state.native-lt0-or$state.poll-lt[Math]::Max($state.core,$state.native)){throw 'Toolbar Cancel reached the page before the native prompt'}
   $state.canceled++
  }
 }
 function Settled{}
 function Wait-Until([scriptblock]$Condition,[string]$Message){
  if($Message-cne'Toolbar delete prompt did not open'-or$args.Count){throw 'Toolbar prompt changed its normal wait bound'}
  for($i=1;$i-le6;$i++){$script:toolbarPrompt.poll=$i;if(& $Condition){return}};throw $Message
 }
 foreach($case in @(@{core=1;native=1},@{core=1;native=4},@{core=4;native=1},@{core=-1;native=1},@{core=1;native=-1})){
  $script:toolbarPrompt=@{core=$case.core;native=$case.native;poll=0;requested=$false;canceled=0;inputs=@()}
  $failure=$null;try{. ([scriptblock]::Create($sequence))}catch{$failure=$_}
  if($case.core-lt0-or$case.native-lt0){
   if(!$failure-or$failure.Exception.Message-cne'Toolbar delete prompt did not open'-or$script:toolbarPrompt.canceled-ne0){throw 'Missing toolbar prompt did not retain its failure'}
  }else{
   if($failure){throw $failure}
   if($script:toolbarPrompt.poll-ne[Math]::Max($case.core,$case.native)-or$script:toolbarPrompt.canceled-ne1){throw 'Toolbar Cancel did not await both shared and native prompt'}
  }
  if(($script:toolbarPrompt.inputs|Where-Object {$_-eq'workspace-toolbar-delete'}).Count-ne1){throw 'Toolbar delete was replayed'}
 }
 'Actual Toolbar-Action waits for its shared prompt and visible native confirm before the original Cancel journey.'
}

& {
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-tool-variations.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $actual=$source.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Sketch-Drawers'},$true)
    $ready=$actual.Body.Find({param($node)$node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'Wait-Canvas'},$true)
    $predicate=$ready.CommandElements[1].ScriptBlock.GetScriptBlock()
    $blank=$actual.Body.Find({param($node)$node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -ceq '$blank'},$true)
    if(!$blank -or $ready.Extent.EndOffset -ge $blank.Extent.StartOffset){throw 'Sketch readiness is not before the exact blank baseline'}
    function Model {$script:sample}
    foreach($canvas in @($false,$true)){foreach($brush in @($false,$true)){foreach($shaders in @($false,$true)){
        $script:sample=[pscustomobject]@{canvas_ready=$canvas;brush_ready=$brush;shaders_ready=$shaders}
        $accepted=[bool](& $predicate);$expected=$canvas -and $brush -and $shaders
        if($accepted -ne $expected){throw 'Sketch blank baseline accepted unfinished paper shaders'}
    }}}
}
'Sketch blank baseline requires canvas, brush and paper shader readiness'

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-navigation-controls.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $messages=@('Preferences did not close','Preferences did not close after Wheel Pan','Fit Reset preferences did not close','Restore Reset preferences did not close','Pinch preferences did not close and reenable the owned native canvas')
 $review=[pscustomobject]@{Id=7}
 function Invoke-Id($Id){if($Id-cne'CloseButton'){throw 'Unexpected navigation close input'};$script:navigationClose.closes++}
 function Model{
  $state=$script:navigationClose
  [pscustomobject]@{preferences=$(if($state.coreAt-lt0-or$state.poll-lt$state.coreAt){[pscustomobject]@{page='canvas'}}else{$null})}
 }
 function Find($Id,[switch]$Visible){
  $state=$script:navigationClose
  if($Id-eq'CloseButton'){if($state.closeAt-lt0-or$state.poll-lt$state.closeAt){[pscustomobject]@{Current=[pscustomobject]@{ProcessId=7}}};return}
  if($Id-cne'drawing-canvas'){throw 'Unexpected native navigation acknowledgement'}
  if(!$state.missing){[pscustomobject]@{Current=[pscustomobject]@{ProcessId=$state.pid;IsEnabled=($state.nativeAt-ge0-and$state.poll-ge$state.nativeAt)}}}
 }
 function Wait-Until([scriptblock]$Condition,[string]$Message){
  if($Message-cne$script:navigationClose.message-or$args.Count){throw 'Navigation changed its original close failure or bound'}
  for($poll=1;$poll-le8;$poll++){$script:navigationClose.poll=$poll;if(& $Condition){return}}
  throw $Message
 }
 function Reset-Zoom{
  $state=$script:navigationClose
  if($state.coreAt-lt0-or$state.poll-lt$state.coreAt-or$state.nativeAt-lt0-or$state.poll-lt$state.nativeAt-or$state.closeAt-lt0-or$state.poll-lt$state.closeAt-or$state.missing-or$state.pid-ne$review.Id){throw 'Navigation requested Zoom before native Preferences released canvas'}
  $state.zoom++
 }
 $helpers=@($source.FindAll({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-ceq'Close-Preferences'},$true))
 if($helpers.Count-ne1){throw 'Missing single actual navigation Close-Preferences helper'}
 . ([scriptblock]::Create($helpers[0].Extent.Text))
 $calls=@($source.FindAll({param($node)$node-is[Management.Automation.Language.CommandAst]-and$node.GetCommandName()-ceq'Close-Preferences'},$true))
 if($calls.Count-ne5){throw 'Navigation must use Close-Preferences for all five closes'}
 foreach($message in $messages){
  $callsForMessage=@($calls|Where-Object {$_.CommandElements.Count-eq2-and$_.CommandElements[1]-is[Management.Automation.Language.StringConstantExpressionAst]-and$_.CommandElements[1].Value-ceq$message})
  if($callsForMessage.Count-ne1){throw 'Missing actual navigation close caller or changed original failure'}
  $statement=$callsForMessage[0].Parent;$statements=@($statement.Parent.Statements);$index=[array]::IndexOf($statements,$statement)
  $nodes=@($statement);$requestsZoom=$message-ceq'Preferences did not close'
  if($requestsZoom){if($statements[$index+1].Extent.Text-cne'Reset-Zoom'){throw 'Native close no longer directly precedes actual Zoom'};$nodes+=,$statements[$index+1]}
  $sequence=($nodes|ForEach-Object {$_.Extent.Text})-join"`n"
  foreach($case in @(@{name='core-first';core=2;native=4;closed=4;pid=7},@{name='native-first';core=4;native=1;closed=1;pid=7},@{name='visible-close';core=1;native=1;closed=4;pid=7},@{name='ready';core=1;native=1;closed=1;pid=7},@{name='missing';core=1;native=1;closed=1;pid=7},@{name='wrong-owner';core=1;native=1;closed=1;pid=8},@{name='native-stalled';core=1;native=-1;closed=1;pid=7},@{name='core-stalled';core=-1;native=1;closed=1;pid=7},@{name='modal-stalled';core=1;native=1;closed=-1;pid=7})){
   $script:navigationClose=@{coreAt=$case.core;nativeAt=$case.native;closeAt=$case.closed;pid=$case.pid;missing=($case.name-eq'missing');poll=0;closes=0;zoom=0;message=$message}
   $failure=$null;try{. ([scriptblock]::Create($sequence))}catch{$failure=$_}
   $negative=$case.name-in@('missing','wrong-owner','native-stalled','core-stalled','modal-stalled')
   if($negative){
    if(!$failure-or$failure.Exception.Message-cne$message-or$script:navigationClose.poll-ne8-or$script:navigationClose.zoom-ne0){throw "Navigation $message/$($case.name) weakened its original failure: $failure"}
   }else{
    if($failure){throw $failure}
    if($script:navigationClose.poll-ne[Math]::Max($case.core,[Math]::Max($case.native,$case.closed))-or$script:navigationClose.zoom-ne[int]$requestsZoom){throw 'Actual navigation close returned before all native/shared acknowledgements'}
   }
   if($script:navigationClose.closes-ne1){throw 'Navigation changed its original CloseButton invocation'}
  }
 }
 'Actual navigation close paths: shared Preferences absent and visible owned native canvas released before continuation; all original failure messages and bounds retained.'
}

& {
 Add-Type -TypeDefinition @'
namespace CapyRowRenameTest {
 public static class Driver {
  public static bool Active;public static int Ups;
  public static void Down(string device,int x,int y){Active=true;Ups=0;}
  public static void Up(){if(!Active)throw new System.InvalidOperationException("Released twice");Active=false;Ups++;}
 }
 public static class Lease {public static string ReadClaims(object run,object storage){return "owned";}}
 public static class ValuePattern {public static object Pattern="value";}
}
'@
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-switcher-drag.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $definition=@($source.FindAll({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Held-Acknowledgement'},$true))
 if($definition.Count-ne1){throw 'Missing actual Rename acknowledgement journey'}
 $attempt=@($definition[0].Body.FindAll({param($n)$n-is[Management.Automation.Language.TryStatementAst]},$true))[0]
 $statements=@($attempt.Body.Statements)
 $start=@(0..($statements.Count-1)|Where-Object {$statements[$_].Extent.Text.StartsWith('[CapyRowPointer]::Down(')})
 $end=@(0..($statements.Count-1)|Where-Object {$statements[$_].Extent.Text-eq'$lease.Dispose()'})
 if($start.Count-ne1-or$end.Count-ne1-or$end[0]-le$start[0]){throw 'Missing actual native press through gated pending Rename sequence'}
 $sequence=[scriptblock]::Create(($statements[$start[0]..($end[0]-1)].Extent.Text-join"`n").Replace('[CapyRowPointer]','[CapyRowRenameTest.Driver]').Replace('[PrivateWorkspaceLease]','[CapyRowRenameTest.Lease]').Replace('System.Windows.Automation.ValuePattern','CapyRowRenameTest.ValuePattern'))
 function Wait-Until([scriptblock]$Condition,[string]$Message,[double]$Timeout){foreach($sample in 1..3){if(& $Condition){return}};throw $Message}
 function Model {
  $held=[CapyRowRenameTest.Driver]::Active
  [pscustomobject]@{windows_workspace=[pscustomobject]@{id='original';selected=$(if($case-eq'selection'-and$held){'next'}else{'selected'});busy=(!$held-and$case-ne'no-pending')-or($held-and$case-eq'early-submit');prompt_action=[pscustomobject]@{type='rename'};rows=@([pscustomobject]@{id='renamed';title='Old title'})}}
 }
 function Assert-AckPreview($Model,$Original,$Selected,$Layout){if($Model.windows_workspace.id-cne$Original-or$Model.windows_workspace.selected-cne$Selected){throw 'Preview changed'}}
 function Read-AckNative($Peer,$RootRuntime,[switch]$Held){if([CapyRowRenameTest.Driver]::Active-ne[bool]$Held){throw 'Native contact phase mismatch'};if($case-eq'identity'-and$Held){return 'replaced'};'retained'}
 function Assert-AckNative($Before,$After){if($Before-cne$After){throw 'Native identity changed'}}
 function Assert-AckClaim($Before,$After,$Original){if($Before-cne$After){throw 'Claim changed'}}
 foreach($case in 'ready','early-submit','selection','draft','identity','no-pending'){
  $Target='row';$AckBeforeRelease=$false;$Id='renamed';$next='next';$original='original';$selected='selected';$layout='layout';$title='Old title';$name='Exact saved name'
  $bounds=[pscustomobject]@{X=0;Y=0;Width=100;Height=30};$lease=[pscustomobject]@{Expired=$false;ElapsedMilliseconds=200};$trace=@{};$before='retained';$claims='owned';$peer='row';$rootRuntime='root';$run='unused'
  $entry=[pscustomobject]@{};$entry|Add-Member ScriptMethod GetCurrentPattern {param($Pattern)[pscustomobject]@{Current=[pscustomobject]@{Value=$(if($case-eq'draft'){'Changed'}else{$name})}}}
  $failure=$null;try{. $sequence}catch{$failure=$_}
  if($case-eq'ready'){
   if($failure-or[CapyRowRenameTest.Driver]::Active-or[CapyRowRenameTest.Driver]::Ups-ne1-or!$trace.pending_model.busy){throw "Actual row release did not establish gated Rename: $failure"}
  }elseif(!$failure){throw "Actual row Rename accepted invalid $case state"}
 }
 $heldRename=@($source.FindAll({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Held-Rename'},$true))
 $mouse=@($heldRename[0].Body.FindAll({param($n)$n-is[Management.Automation.Language.IfStatementAst]-and$n.Clauses[0].Item1.Extent.Text-eq"`$Device-eq'mouse'"},$true))
 if($mouse.Count-ne1){throw 'Missing actual acknowledgement order matrix'}
 $orders=[Collections.Generic.List[string]]::new()
 function Held-Acknowledgement($Id,$Target,$AckBeforeRelease){$orders.Add("$Target/$AckBeforeRelease")}
 $Device='mouse';. ([scriptblock]::Create($mouse[0].Extent.Text))
 if(($orders-join'|')-cne'pin/False|pin/True|apply/False|apply/True|row/False'){throw 'Native acknowledgement matrix changed its valid order coverage'}
 'Actual row Rename preserves held draft, preview, claims and native identity; only release admits gated Rename, while pin and Apply retain both acknowledgement orders.'
}

& {
 Add-Type -TypeDefinition @'
namespace CapyNumericHoverTest {
 public static class Pointer {
  public static bool TitleAcknowledged,Field;public static int Hovers;
  public static void Hover(int x,int y){++Hovers;if(x==15){if(!TitleAcknowledged)throw new System.InvalidOperationException("Field hover preceded the title tooltip acknowledgement");Field=true;}}
 }
}
'@
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/LocalizationProperties.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $function=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-ceq'Preference-Numeric'},$true)
 $anchor=$function.Find({param($n)$n-is[Management.Automation.Language.IfStatementAst]-and$n.Extent.Text-like"*Retained typed numeric error accessibility help stayed in an earlier language*"},$true)
 $statements=@($anchor.Parent.Statements);$index=[array]::IndexOf($statements,$anchor)
 if($index-lt0){throw 'Missing actual numeric tooltip journey anchor'}
 $nodes=@();for($i=$index+1;$i-lt$statements.Count;$i++){$nodes+=,$statements[$i];if($statements[$i]-is[Management.Automation.Language.TryStatementAst]){break}}
 $sequence=(($nodes|ForEach-Object {$_.Extent.Text})-join"`n").Replace('[CapyRowPointer]','[CapyNumericHoverTest.Pointer]')
 $control=[pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=[pscustomobject]@{Left=10;Top=0;Width=10;Height=10}}}
 function Control($Id,[switch]$Arranged){
  if($Id-notin@('number-caption-setting-number-pan_speed','preferences-heading')){throw 'Unexpected numeric hover target'}
  [pscustomobject]@{Current=[pscustomobject]@{Name='Pan speed';BoundingRectangle=[pscustomobject]@{Left=0;Top=0;Width=10;Height=10}}}
 }
 function Numeric-Tooltip{
  if($script:numericTitleMissing-and![CapyNumericHoverTest.Pointer]::Field){return}
  [pscustomobject]@{Current=[pscustomobject]@{Name=$(if([CapyNumericHoverTest.Pointer]::Field){'Invalid expression'}else{'Pan speed'})}}
 }
 function Catalog-Text($Tag,$Id,$Catalog){if($Id-cne'numeric-invalid-expression'){throw 'Unexpected numeric copy'};'Invalid expression'}
 function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Timeout){
  if($Timeout-ne10){throw 'Numeric tooltip changed its ten-second bound'}
  for($i=0;$i-lt4;$i++){if(& $Condition){if($Message-ceq'Native numeric title tooltip did not acknowledge hover'){[CapyNumericHoverTest.Pointer]::TitleAcknowledged=$true};return}};throw $Message
 }
 foreach($missing in @($false,$true)){
  $script:numericTitleMissing=$missing;[CapyNumericHoverTest.Pointer]::TitleAcknowledged=$false;[CapyNumericHoverTest.Pointer]::Field=$false;[CapyNumericHoverTest.Pointer]::Hovers=0
  $failure=$null;try{. ([scriptblock]::Create($sequence))}catch{$failure=$_}
  if($missing){if(!$failure-or$failure.Exception.Message-cne'Native numeric title tooltip did not acknowledge hover'-or[CapyNumericHoverTest.Pointer]::Field){throw 'Missing title tooltip did not prevent the next hover'}}else{
   if($failure){throw $failure}
   if(![CapyNumericHoverTest.Pointer]::Field-or[CapyNumericHoverTest.Pointer]::Hovers-ne2){throw 'Numeric tooltip journey changed its pointer sequence'}
  }
 }
 'Actual retained numeric tooltip journey acknowledges the title before hovering the invalid field; original field assertion retained.'
}

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-lifecycle.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $function=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-ceq'Check-Caption'},$true)
 $wait=$function.Find({param($n)$n-is[Management.Automation.Language.CommandAst]-and$n.GetCommandName()-ceq'Wait-Canvas'},$true)
 if(!$wait){throw 'Missing actual lifecycle caption settle wait'}
 $root=[pscustomobject]@{}
 $root|Add-Member ScriptMethod FindFirst {param($Scope,$Condition)[pscustomobject]@{Current=[pscustomobject]@{IsOffscreen=$false;Name=('Preparing brush'+[char]0x2026)}}}
 function Model{
  $state=$script:lifecycleReadiness
  if($state.blocker-eq'missing'){return}
  $waiting=$state.ready-lt0-or$state.poll-lt$state.ready
  [pscustomobject]@{canvas_ready=(!$waiting-or$state.blocker-ne'canvas');brush_ready=(!$waiting-or$state.blocker-ne'brush');canvas_notice=$(if($waiting-and$state.blocker-eq'notice'){'brush'}else{$null});windows_filter_load=[pscustomobject]@{pending=($waiting-and$state.blocker-eq'filters')}}
 }
 function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Timeout){
  if($Message-cne'Startup status did not settle before caption validation'-or$Timeout-ne45){throw 'Lifecycle changed its original settle failure or bound'}
  for($i=1;$i-le8;$i++){$script:lifecycleReadiness.poll=$i;if(& $Condition){return}};throw $Message
 }
 foreach($case in @(@{blocker='brush';ready=4},@{blocker='notice';ready=4},@{blocker='filters';ready=4},@{blocker='canvas';ready=4},@{blocker='brush';ready=1},@{blocker='brush';ready=-1},@{blocker='missing';ready=-1})){
  $script:lifecycleReadiness=@{blocker=$case.blocker;ready=$case.ready;poll=0}
  $failure=$null;try{. ([scriptblock]::Create($wait.Extent.Text))}catch{$failure=$_}
  if($case.ready-lt0){if(!$failure-or$failure.Exception.Message-cne'Startup status did not settle before caption validation'-or$script:lifecycleReadiness.poll-ne8){throw 'Lifecycle accepted missing/stalled readiness'}}else{
   if($failure){throw $failure}
   if($script:lifecycleReadiness.poll-ne$case.ready){throw 'Lifecycle caption settled before shared readiness despite current Preparing brush copy'}
  }
 }
 'Actual lifecycle caption wait uses shared canvas/brush/notice/filter readiness and retains its original bound and native error check.'
}

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/layer-paper.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 foreach($name in 'Texture-Begin','Texture-Observed','Texture-Open','Texture-Closed'){
  $definition=$source.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$false)
  if($definition){Invoke-Expression $definition.Extent.Text}
 }
 $review=[pscustomobject]@{Id=17};$script:phase='opening';$script:case='ready';$script:rootIdentity='owned'
 $workspace=[pscustomobject]@{};$workspace|Add-Member ScriptMethod GetRuntimeId {,$script:rootIdentity}
 $current=[pscustomobject]@{};$workspace|Add-Member NoteProperty Current $current
 $current|Add-Member ScriptProperty ProcessId {if($script:phase -eq 'closed' -and $script:case -eq 'process'){18}else{17}}
 $current|Add-Member ScriptProperty ItemStatus {
  $registered=@{id='picker-1';epoch=1;layer=2};$qpc=10
  if($script:phase -eq 'closing'){$qpc=11}
  if($script:phase -eq 'closed'){
   $registered=@{};$qpc=12
   switch($script:case){
    'missing' {return '{"snapshot_qpc":12}'}
    'malformed' {return '{'}
    'type' {$registered=$false}
    'null' {$registered=$null}
    'partial' {$registered=@{epoch=1;layer=2}}
    'successor' {$registered=@{id='picker-2';epoch=1;layer=2}}
    'stale' {$qpc=10}
    'clock-type' {$qpc='12'}
   }
  }
  @{texture_dialog=$registered;snapshot_qpc=$qpc}|ConvertTo-Json -Compress
 }
 function Workspace-Root{
  if($script:phase -eq 'closed' -and $script:case -eq 'root'){$script:rootIdentity='replacement'}
  $workspace
 }
 function Model{
  [pscustomobject]@{windows_workspace=@{id=$(if($script:phase -eq 'closed' -and $script:case -eq 'workspace'){'other'}else{'paint'})};state=@{
   layers=@(@{id=$(if($script:phase -eq 'closed' -and $script:case -eq 'layer'){3}else{2});paper=@{editable=$true}})
   document_file=@{epoch=$(if($script:phase -eq 'closed' -and $script:case -eq 'epoch'){2}else{1})}
   layer_properties=@{layer=$(if($script:case -eq 'unselected-row'){99}else{2})}
  }}
 }
 function Invoke{param($Id)if($Id -notin 'property-texture','layer-texture-2'){throw 'Paper changed its actual opener'};$script:invocations++}
 function Find{param($Id)if($script:phase -eq 'opening' -or ($script:phase -eq 'closed' -and $script:case -eq 'apply-present' -and $Id -eq 'texture-apply')){[pscustomobject]@{Current=@{IsOffscreen=$false}}}}
 function Wait-Until([scriptblock]$Ready,[string]$Message,[int]$Seconds=8){
  if($Seconds -ne 8){throw 'Paper changed its original close bound'}
  if($script:phase -eq 'opening'){
   if($Message -cne ('Choose Texture did not open from '+$script:opener) -or !(&$Ready)){throw 'Paper changed its original visible Apply/tile open assertion'}
   return
  }
  if($Message -cne 'Original Paper close assertion'){throw 'Paper changed its original close failure'}
  if(&$Ready){throw 'Texture Closed accepted hidden apply before the native registry was empty'}
  $script:phase='closed';$accepted=&$Ready
  if($script:case -in 'ready','unselected-row'){
   if(!$accepted){throw 'Texture Closed rejected its fresh owned native Closed acknowledgement'}
  }elseif($accepted){throw ('Texture Closed accepted invalid owner or acknowledgement: '+$script:case)}
 }
 foreach($case in 'ready','unselected-row','process','root','workspace','epoch','layer','type','null','partial','missing','malformed','successor','stale','clock-type','unobserved','apply-present'){
  $script:case=$case;$script:phase='opening';$script:rootIdentity='owned';$script:TextureOwner=$null;$script:invocations=0
  $script:opener=if($case -eq 'unselected-row'){'layer-texture-2'}else{'property-texture'}
  Texture-Open $script:opener
  if($case -eq 'unobserved'){$script:TextureOwner.opened=$null}
  $script:phase='closing';Texture-Closed 'Original Paper close assertion'
  if($script:invocations -ne 1){throw 'Paper replayed its actual opener'}
 }
 'Actual Paper open/close consumer requires Apply absence and fresh owned native registry removal.'
}
& {
 Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
namespace CapyEditorRegrowTest {
 public class Rect {
  public double X,Y,Width,Height;public double Left{get{return X;}}public double Top{get{return Y;}}public double Right{get{return X+Width;}}public double Bottom{get{return Y+Height;}}
  public Rect(double x,double y,double width,double height){X=x;Y=y;Width=width;Height=height;}
  public bool Contains(Rect other){return other.Left>=Left&&other.Top>=Top&&other.Right<=Right&&other.Bottom<=Bottom;}
 }
 public enum WindowVisualState {Normal,Maximized}
 public static class WindowPattern {public static object Pattern="window";}
 public static class TransformPattern {public static object Pattern="transform";}
 public static class State {
  public static int Dpi,MaxTrackHeight,MaxTrackWidth,Moves,ResizeCount;public static bool PerMonitor;
  public static double FullSide=232,ShortSide=190;public static bool ReplaceWheel;
  public static Rect Work,Bounds;public static WindowVisualState VisualState;
  public static List<Rect> Requests=new List<Rect>(),Arranged=new List<Rect>();
  public static void Reset(int dpi,Rect work,int maxHeight){Dpi=dpi;Work=work;MaxTrackHeight=maxHeight;MaxTrackWidth=(int)work.Width;Bounds=new Rect(work.Left+120,work.Top+180,1000,700);Moves=ResizeCount=0;PerMonitor=false;FullSide=232;ShortSide=190;ReplaceWheel=false;VisualState=WindowVisualState.Maximized;Requests.Clear();Arranged.Clear();}
  public static void Resize(int width,int height){
   Requests.Add(new Rect(Bounds.X,Bounds.Y,width,height));ResizeCount++;
   Bounds=new Rect(Bounds.X,Bounds.Y,Math.Min(width,MaxTrackWidth),Math.Min(height,MaxTrackHeight));
   if(Bounds.Width!=width||Bounds.Height!=height)throw new InvalidOperationException("Owned resize did not acknowledge requested "+width+"x"+height+"; Win32 maximum tracking size produced "+Bounds.Width+"x"+Bounds.Height);
  }
 }
 public static class Driver {
  public static int GetDpiForWindow(IntPtr window){return State.Dpi;}
  public static IntPtr SetThreadDpiAwarenessContext(IntPtr value){State.PerMonitor=value==new IntPtr(-4);return IntPtr.Zero;}
 }
 public class Screen {
  public Rect WorkingArea{get{return State.Work;}}
  public static Screen FromHandle(IntPtr handle){if(!State.PerMonitor)throw new InvalidOperationException("Work area queried before PMv2 awareness");return new Screen();}
 }
 public class Window {
  public Window Current{get{return this;}}public WindowVisualState WindowVisualState{get{return State.VisualState;}}public Rect BoundingRectangle{get{return State.Bounds;}}
  public object GetCurrentPattern(object pattern){if(Object.Equals(pattern,WindowPattern.Pattern))return this;if(Object.Equals(pattern,TransformPattern.Pattern))return new Transform();throw new InvalidOperationException("Unexpected native window pattern");}
  public void SetWindowVisualState(WindowVisualState value){State.VisualState=value;}
 }
 public class Transform {
  public Transform Current{get{return this;}}public bool CanMove{get{return true;}}
  public void Move(double x,double y){State.Moves++;State.Bounds=new Rect(x,y,State.Bounds.Width,State.Bounds.Height);}
 }
 public class Wheel {
  public Wheel Current{get{return this;}}
  public Rect BoundingRectangle{get{double scale=State.Dpi/96.0,side=State.Bounds.Height/scale<500?State.ShortSide:State.FullSide;return new Rect(State.Bounds.Left+20*scale,State.Bounds.Top+100*scale,side*scale,side*scale);}}
  public int[] GetRuntimeId(){return new int[]{7,State.ReplaceWheel&&State.ResizeCount>1?102:101};}
 }
}
'@
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-color-editor.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $actual=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-ceq'Check-EditorRegrow'},$true)
 if(!$actual){throw 'Missing actual Edit Color regrow journey'}
 $definition=$actual.Extent.Text.Replace('[System.Windows.Automation.','[CapyEditorRegrowTest.').Replace('[System.Windows.Forms.Screen]','[CapyEditorRegrowTest.Screen]').Replace('[Windows.Forms.Screen]','[CapyEditorRegrowTest.Screen]').Replace('[CapyRowPointer]','[CapyEditorRegrowTest.Driver]')
 . ([scriptblock]::Create($definition))
 $root=[CapyEditorRegrowTest.Window]::new();$review=[pscustomobject]@{Id=17};$drawingWindow=[IntPtr]17;$editorFrame='owned-editor';$run='regrow-test';$Theme='dark'
 function Join-Path([string]$Path,[string]$ChildPath){if($ChildPath-ceq'exercise-window.ps1'){'Invoke-RegrowResize'}else{Microsoft.PowerShell.Management\Join-Path $Path $ChildPath}}
 function Invoke-RegrowResize([int]$ProcessId,[long]$WindowHandle,[string]$Action,[int]$Width,[int]$Height){
  if($ProcessId-ne17-or$WindowHandle-ne17-or$Action-cne'Resize'){throw 'Regrow changed its owned resize route'}
  [CapyEditorRegrowTest.State]::Resize($Width,$Height)
 }
 function Open-Editor{$script:regrowOpens++}
 function Control($Id,$Within,[switch]$Arranged){
  if($Id-cne'edit-color-wheel'-or$Within-cne'owned-editor'){throw 'Regrow read outside its owned wheel'}
  $wheel=[CapyEditorRegrowTest.Wheel]::new()
  if($Arranged){
   $bounds=$wheel.Current.BoundingRectangle
   if(![CapyEditorRegrowTest.State]::Work.Contains([CapyEditorRegrowTest.State]::Bounds)-or![CapyEditorRegrowTest.State]::Work.Contains($bounds)){throw 'The regrow window or wheel lies outside its monitor work area'}
   [CapyEditorRegrowTest.State]::Arranged.Add($bounds)
  }
  $wheel
 }
 function Wait-Until([scriptblock]$Condition,[string]$Message){if(!(&$Condition)){throw $Message}}
 function Set-Content {
  param([Parameter(Position=0)][string]$Path,[Parameter(ValueFromPipeline)]$Value)
  process{if($Path-notlike'*wheel-regrow-sdr-dark.json'){throw 'Regrow wrote an unexpected receipt'};$script:regrowReceipt=$Value|ConvertFrom-Json}
 }
 function Capture([string]$Name,[switch]$WithModel){if(!$WithModel){throw 'Regrow dropped its model evidence'};$script:regrowCaptures.Add($Name)}
 function Invoke($Id){if($Id-cne'edit-color-cancel'){throw 'Regrow changed its close action'};$script:regrowCancels++}
 function Closed-Editor($Message){if($script:regrowCancels-ne1){throw $Message}}
 foreach($profile in @(@{dpi=144;work=[CapyEditorRegrowTest.Rect]::new(-1920,96,1920,1008);max=1106},@{dpi=96;work=[CapyEditorRegrowTest.Rect]::new(64,80,1600,1000);max=1106})){
  foreach($case in 'ready','full-small','full-large','short-small','short-large','replaced'){
   [CapyEditorRegrowTest.State]::Reset($profile.dpi,$profile.work,$profile.max)
   switch($case){'full-small'{[CapyEditorRegrowTest.State]::FullSide=230.9}'full-large'{[CapyEditorRegrowTest.State]::FullSide=233.1}'short-small'{[CapyEditorRegrowTest.State]::ShortSide=158.9}'short-large'{[CapyEditorRegrowTest.State]::ShortSide=231}'replaced'{[CapyEditorRegrowTest.State]::ReplaceWheel=$true}}
   $script:regrowOpens=0;$script:regrowCancels=0;$script:regrowReceipt=$null;$script:regrowCaptures=[Collections.Generic.List[string]]::new()
   $failure=$null;try{Check-EditorRegrow 'sdr'}catch{$failure=$_}
   if($case-eq'ready'){
    if($failure){throw $failure}
    if([CapyEditorRegrowTest.State]::ResizeCount-ne5-or[CapyEditorRegrowTest.State]::Arranged.Count-ne4-or[CapyEditorRegrowTest.State]::Moves-lt1-or$script:regrowOpens-ne1-or$script:regrowCancels-ne1-or[CapyEditorRegrowTest.State]::VisualState-ne[CapyEditorRegrowTest.WindowVisualState]::Maximized){throw 'Actual regrow omitted an owned stage, movement, close or window restore'}
    if(($script:regrowReceipt.stage.name-join',')-cne'tall,short,regrown,wider'-or($script:regrowCaptures-join',')-cne'wheel-sdr-tall-dark,wheel-sdr-short-dark,wheel-sdr-regrown-dark,wheel-sdr-wider-dark'){throw 'Actual regrow dropped its retained stage receipts or captures'}
    foreach($request in [CapyEditorRegrowTest.State]::Requests){if($request.Width-gt$profile.work.Width-or$request.Height-gt$profile.work.Height){throw 'Regrow requested a physical size beyond the monitor work area'}}
   }else{
    $expected=if($case-eq'replaced'){'Edit Color resize rebuilt its wheel control'}else{'Edit Color wheel did not shrink and regain its full available size'}
    if(!$failure-or$failure.Exception.Message-cne$expected){throw "Actual regrow lost its wheel size/identity oracle: $case, $failure"}
   }
  }
 }
 'Actual Edit Color regrow:144dpi workarea-fit and96dpi offset monitor pass;232 full/159..231 short and retained native wheel reject invalid output.'
}

& {
    Add-Type -TypeDefinition @'
namespace CapyCanvasSeedTest {
 public static class Driver {
  public static bool Panned;
  public static void RightDrag(int x0,int y0,int x1,int y1){Panned=true;}
 }
}
'@
    $tokens=$null;$errors=$null
    $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-canvas-touch.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $signature=$source.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq'Signature'},$true)
    if(!$signature){throw 'Missing actual canvas touch signature'}
    . ([scriptblock]::Create($signature.Extent.Text))
    $wait=$source.Find({param($n)$n-is[Management.Automation.Language.CommandAst]-and$n.GetCommandName()-eq'Wait-Canvas'-and@($n.CommandElements|Where-Object {$_.Value-eq'Visible mouse seed stroke did not finish'}).Count-eq1},$true)
    if(!$wait){throw 'Missing actual canvas touch seed acknowledgement'}
    $first=$wait
    while($first.Parent-isnot[Management.Automation.Language.StatementBlockAst]){$first=$first.Parent;if(!$first){throw 'Missing actual canvas touch seed block'}}
    $statements=@($first.Parent.Statements);$start=[array]::IndexOf($statements,$first)
    $last=@(for($i=$start;$i-lt$statements.Count;$i++){if($statements[$i]-is[Management.Automation.Language.IfStatementAst]-and$statements[$i].Extent.Text.Contains("throw 'Mouse right-drag painted'")){$i}})
    if($last.Count-ne1){throw 'Missing unchanged mouse right-drag artwork assertion'}
    $sequence=[scriptblock]::Create(($statements[$start..$last[0]].Extent.Text-join"`n").Replace('[CapyRowPointer]','[CapyCanvasSeedTest.Driver]'))
    $seedRevision=0;$ScrubOnly=$false;$cx=100;$cy=100;$seedState=@{committed=$false;sleeps=0}
    [CapyCanvasSeedTest.Driver]::Panned=$false
    function Model{
        $done=$seedState.committed-or[CapyCanvasSeedTest.Driver]::Panned
        [pscustomobject]@{state=[pscustomobject]@{document_file=[ordered]@{revision=$(if($done){1}else{0});modified=$true};layers=@([pscustomobject]@{id=1;paint_revision=$(if($done){73735}else{65537});mask_revision=0})}}
    }
    function Camera{[pscustomobject]@{translation=@($(if([CapyCanvasSeedTest.Driver]::Panned){60}else{0}),0)}}
    function Start-Sleep{param($Milliseconds)$seedState.sleeps++}
    function Wait-Until([scriptblock]$Condition,[string]$Message){
        for($poll=1;$poll-le8;$poll++){if($poll-ge3){$seedState.committed=$true};if(& $Condition){return}}
        throw $Message
    }
    . $sequence
    if(!$seedState.committed-or$seedState.sleeps){throw 'Canvas touch captured its seed before commit or used a settling sleep'}
}
Write-Output 'Canvas touch acknowledges the committed seed before mouse navigation despite early modified state and repeated equal pending signatures.'

& {
 $tokens=$null;$errors=$null
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-enclose-fill.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw($errors|Out-String)}
 $textures=$source.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Check-Textures'},$true)
 $endpoints=@(foreach($name in @('empty','filled')){
  $assignment=$textures.Find({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq ('$'+$name)},$true)
  if(!$assignment){throw "Missing actual texture endpoint: $name"}
  $statements=@($assignment.Parent.Statements);$at=[Array]::IndexOf($statements,$assignment)
  if($at -lt 1 -or $statements[$at-1].Extent.Text -cne 'Park'){throw "Texture endpoint lost cursor parking: $name"}
  $statements[$at-1].Extent.Text+';'+$assignment.Extent.Text
 })
 $comparison=$textures.Find({param($n)$n -is [Management.Automation.Language.IfStatementAst] -and $n.Clauses[0].Item1.Extent.Text -eq '$filled -eq $empty'},$true)
 if(!$comparison){throw 'Missing exact texture endpoint comparison'}
 $helper=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
 $definition=$helper.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Wait-StablePixels'},$true)
 . ([scriptblock]::Create($definition.Extent.Text))
 function Test-CanvasProgress($Previous){$false}
 function Texture-Samples{$script:textureDisplayed}
 function Park{$script:textureParks++}
 function Start-Sleep{param($Milliseconds)}
 function Wait-PresentedFrame([switch]$Idle){
  if(!$Idle){throw 'Texture endpoint did not require an idle presentation'}
  $script:textureAcknowledgements++;$script:textureDisplayed=$script:textureAuthored
 }
 foreach($schedule in @('baseline-stale','filled-stale')){
  $script:textureParks=0;$script:textureAcknowledgements=0
  $script:textureAuthored='empty';$script:textureDisplayed=if($schedule -eq 'baseline-stale'){'filled'}else{'empty'}
  $tool='fill';$percent=0
  . ([scriptblock]::Create($endpoints[0]))
  $script:textureAuthored='filled';$script:textureDisplayed=if($schedule -eq 'filled-stale'){'empty'}else{'filled'}
  . ([scriptblock]::Create($endpoints[1]))
  . ([scriptblock]::Create($comparison.Extent.Text))
  if($empty -cne 'empty' -or $filled -cne 'filled' -or $script:textureParks -ne 2 -or $script:textureAcknowledgements -ne 2){throw "Texture endpoint schedule failed: $schedule"}
 }
 'Actual texture endpoints: baseline-stale and filled-stale acknowledged idle presentation, kept cursor parking and exact hashes.'
}

& {
    $panelAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-panel-bridge-layout.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    foreach($name in 'Bridge-FrameRange','Bridge-EventRange','Assert-BridgeEvents','Assert-BridgeFrameOwners'){
        $definition=$panelAst.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq$name}.GetNewClosure(),$true)
        if(!$definition){throw "Missing actual bridge frame oracle $name"}
        . ([scriptblock]::Create($definition.Extent.Text))
    }
    function Check($Condition,$Message,$Data){if(!$Condition){throw $Message}}
    foreach($case in 'slow','gap','duplicate','overflow','short'){
        $rows=@(@{first_frame=1;last_frame=100;chrome_revision=20;presented_chrome_revision=19},@{first_frame=101;last_frame=200;chrome_revision=21;presented_chrome_revision=21})
        $history=@{frame=200;overflow=0;samples=$rows}
        switch($case){'gap'{$rows[1].first_frame=102};'duplicate'{$rows[1].first_frame=99};'overflow'{$history.overflow=1};'short'{$rows[1].last_frame=199}}
        $failure=$null;try{$actual=@(Bridge-FrameRange $history 99 200)}catch{$failure=$_}
        if($case-eq'slow'){if($failure){throw $failure};if($actual.Count-ne2){throw 'Bridge frame history lost a native source interval'}}
        elseif(!$failure){throw "Bridge frame history accepted $case evidence"}
    }
    foreach($case in 'zero','zero-missing','zero-overflow','reverse'){
        $history=@{frame=200;overflow=0;samples=@()};$after=200
        switch($case){'zero-missing'{$history.Remove('samples')};'zero-overflow'{$history.overflow=1};'reverse'{$after=201}}
        $failure=$null;try{$actual=@(Bridge-FrameRange $history $after 200)}catch{$failure=$_}
        if($case -eq 'zero'){if($failure){throw $failure};if($actual.Count -ne 0){throw 'An origin-only window move invented a native Rendering interval'}}
        elseif(!$failure){throw "Native bridge history accepted $case evidence"}
    }
    foreach($case in 'gpu','sizing','handback','missing','wrong-projection','native-outside-sizing','gpu-during-sizing','glass-lost'){
        $row=@{groups=@(1..3|ForEach-Object {@{id=$_;gpu_background=$true;native_bridge_background=$false}});projected=@(1,2,3);bridges=1;window_resizing=$false;native_resize_pending=$false;glass_regions=5}
        if($case-in@('sizing','handback','gpu-during-sizing','glass-lost')){foreach($group in $row.groups){$group.gpu_background=$false;$group.native_bridge_background=$true};if($case-eq'handback'){$row.native_resize_pending=$true}else{$row.window_resizing=$true}}
        switch($case){'missing'{$row.groups=$row.groups[0..1]};'wrong-projection'{$row.projected=@(1,2,4)};'native-outside-sizing'{$row.groups[0].native_bridge_background=$true};'gpu-during-sizing'{$row.groups[0].gpu_background=$true};'glass-lost'{$row.glass_regions=4}}
        $failure=$null;try{Assert-BridgeFrameOwners $row @(1,2,3) 5}catch{$failure=$_}
        if($case-in@('gpu','sizing','handback')){if($failure){throw $failure}}
        elseif(!$failure){throw "Native bridge ownership accepted $case evidence"}
    }
    foreach($case in 'same-frame-good','same-frame-bad-owner','gap','duplicate','overflow','short','qpc-reordered','future-qpc','unknown-kind'){
        $rows=@(1..3|ForEach-Object {@{serial=$_;kind='ownership';qpc=100;accepted=$true;chrome_revision=20;presented_chrome_revision=19;groups=@(1..3|ForEach-Object {@{id=$_;gpu_background=$true;native_bridge_background=$false}});projected=@(1,2,3);bridges=1;window_resizing=$false;native_resize_pending=$false;glass_regions=5}})
        $rows[0].kind='chrome-sent';$rows[2].kind='chrome-ack';$rows[2].accepted=$false
        $history=@{serial=3;overflow=0;samples=$rows}
        switch($case){
            'same-frame-bad-owner'{$rows[1].groups[0].gpu_background=$false;$rows[1].groups[0].native_bridge_background=$true}
            'gap'{$rows[1].serial=4};'duplicate'{$rows[1].serial=1};'overflow'{$history.overflow=1}
            'short'{$history.samples=$rows[0..1]};'qpc-reordered'{$rows[1].qpc=99};'future-qpc'{$rows[2].qpc=201};'unknown-kind'{$rows[1].kind='rendering'}
        }
        $failure=$null;try{$actual=@(Assert-BridgeEvents $history 0 3 @(1,2,3) 5 200)}catch{$failure=$_}
        if($case -eq 'same-frame-good'){if($failure){throw $failure};if($actual.Count -ne 3 -or $actual[2].accepted){throw 'Bridge boundaries lost their stale acknowledgment or same-frame ordering'}}
        elseif(!$failure){throw "Bridge boundary history accepted $case evidence"}
    }
    foreach($case in 'native-through-handback','gpu-during-handback','native-before-sizing'){
        $rows=@(1..3|ForEach-Object {@{serial=$_;kind='ownership';qpc=(99+$_);accepted=$true;groups=@(1..3|ForEach-Object {@{id=$_;gpu_background=$false;native_bridge_background=$true}});projected=@(1,2,3);bridges=1;window_resizing=$false;native_resize_pending=$true;glass_regions=5}})
        $rows[0].window_resizing=$true;$rows[0].native_resize_pending=$false
        $rows[2].native_resize_pending=$false;foreach($group in $rows[2].groups){$group.gpu_background=$true;$group.native_bridge_background=$false}
        switch($case){'gpu-during-handback'{$rows[1].groups[0].gpu_background=$true};'native-before-sizing'{$rows[0].qpc=99}}
        $failure=$null;try{$actual=@(Assert-BridgeEvents @{serial=3;overflow=0;samples=$rows} 0 3 @(1,2,3) 5 200 100 101)}catch{$failure=$_}
        if($case -eq 'native-through-handback'){if($failure){throw $failure};if($actual.Count -ne 3){throw 'Native handback boundaries were dropped'}}
        elseif(!$failure){throw "Bridge source boundaries accepted $case evidence"}
    }
    'Actual bridge oracles preserve contiguous Rendering history and every ownership/send/ack boundary, including incorrect transitions entirely between Rendering ticks.'
}

& {
 $client=Join-Path $PSScriptRoot '..'
 $workspace=Get-Content (Join-Path $client 'WorkspaceView.cpp') -Raw
 $bubbles=Get-Content (Join-Path $client 'WorkspaceBubbles.cpp') -Raw
 $canvas=Get-Content (Join-Path $client 'CanvasWindow.cpp') -Raw
 $rendering=[regex]::Match($workspace,'(?s)resizeTraceFrame\s*=\s*CompositionTarget::Rendering\b.*?(?=data->strokes\s*=)').Value
 $layout=[regex]::Match($bubbles,'(?s)void\s+WorkspaceBubbles::Impl::traceLayout\s*\(\s*\).*?(?=void\s+WorkspaceBubbles::Impl::present\s*\()').Value
 if(!$rendering -or !$layout -or $rendering -match '\btracePresentation\s*\(' -or $layout -match '\boverlayApplied\s*\('){throw 'Frame collectors must not publish full workspace snapshots'}
 $services=[regex]::Matches($canvas,'(?s)if\s*\(\s*pollServices\s*\)\s*\{(?<body>.*?)PresentNavigators\s*\(')
 if($services.Count -ne 1 -or $services[0].Groups['body'].Value -notmatch '\bQueueWorkspaceTrace\s*\('){throw 'Owner service must request workspace trace publication independently of snapshots'}
 $queue=[regex]::Match($canvas,'(?s)void\s+CanvasWindow::QueueWorkspaceTrace\s*\(\s*\).*?(?=void\s+CanvasWindow::TraceTextInput\s*\()').Value
 if(!$queue -or $queue -notmatch 'workspaceTraceQueued\.exchange\(true\)' -or $queue -notmatch 'dispatcher\.TryEnqueue' -or $queue -notmatch 'workspace->PublishTrace\s*\(' -or $queue -notmatch 'queued\.store\(false\)' -or $queue -notmatch 'workspaceTraceQueued\.store\(false\)'){throw 'Workspace trace publication must stay coalesced on the UI dispatcher'}
}

& {
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-header.ps1'),[ref]$null,[ref]$null)
 $bounds=$source.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Check-CapyBounds'},$true)
 $baseline=$bounds.Body.EndBlock.Statements|Where-Object {$_.Extent.Text -like '$layout=(Model).layout*'}
 $settle=$bounds.Find({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Wait-Until' -and @($n.CommandElements|Where-Object {$_.Value -eq 'Normal header layout did not settle before Zen'}).Count -eq 1},$true)
 if(!$baseline -or ($settle -and $settle.Extent.StartOffset -gt $baseline.Extent.StartOffset)){throw 'Missing normal header layout acknowledgement before the Zen baseline'}
 $sequence=@($(if($settle){$settle.Extent.Text}),$baseline.Extent.Text)-join"`n"
 function Model {
  $script:zenReads++;$height=if($script:zenCase -eq 'ready' -or $script:zenReads -gt 3){64}else{164}
  [pscustomobject]@{header=@{editing=$false;model=@{size='medium'};sizes=@(@{id='small';height=48},@{id='medium';height=64},@{id='large';height=80})};header_presentation_height=$(if($script:zenCase -ne 'missing'){$height});layout=@{header_height=$height}}
 }
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll -lt 6;$poll++){if(& $Condition){return}};throw $Message}
 foreach($case in 'delayed','ready','missing'){
  $script:zenCase=$case;$script:zenReads=0;$layout=$null;$failure=$null
  try{. ([scriptblock]::Create($sequence))}catch{$failure=$_}
  if($case -eq 'missing'){
   if(!$failure -or $failure.Exception.Message -cne 'Normal header layout did not settle before Zen' -or $null -ne $layout){throw 'Missing header measurement reached the Zen baseline'}
  }elseif($failure -or ($layout|ConvertFrom-Json).header_height -ne 64){throw 'Zen baseline sampled the expanded editor layout'}
 }
 'Actual Zen baseline waits for the selected normal header height; missing measurements cannot reach the exact layout comparison.'
}

& {
 $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/BubbleVisualChecks.ps1'),[ref]$null,[ref]$null)
 $function=$source.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Assert-BubbleVisualEdges'},$true)
 $journey=@($function.Body.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.TryStatementAst]})
 if($journey.Count -ne 1){throw 'Visual transparency cleanup journey is missing'}
 function Model{[pscustomobject]@{state=@{palette=@{glass=@{transparency='high'}}}}}
 function View{@{chooser=@()}}
 function Tap{}
 function Center{}
 function Wait-Until{}
 function Settled-Capture{}
 function Open-BubbleVisualChooser{}
 function Assert-BubbleVisualGeometry{}
 function Escape{}
 function Set-BubbleVisualTransparency([int]$Choice){
  if($Choice -eq 1 -and $index -ne 1){$script:cleanupCalls++;if($script:cleanupFails){throw 'cleanup-marker'};return}
  if($script:primaryFails){throw 'primary-marker'}
 }
 foreach($case in @(@{primary=$true;cleanup=$true;expected='primary-marker'},@{primary=$false;cleanup=$true;expected='cleanup-marker'},@{primary=$true;cleanup=$false;expected='primary-marker'},@{primary=$false;cleanup=$false;expected=$null})){
  $script:primaryFails=$case.primary;$script:cleanupFails=$case.cleanup;$script:cleanupCalls=0
  $modes=@('off','low','medium','high');$original='low';$Dial=1;$Color=2;$transparencyFailure=$null;$failure=$null
  try{. ([scriptblock]::Create($journey[0].Extent.Text))}catch{$failure=$_.Exception.Message}
  if($failure -cne $case.expected -or $script:cleanupCalls -ne 1){throw 'Visual transparency cleanup replaced the original error or skipped its unchanged restore'}
 }
 'Actual visual transparency journey retains the original failure and still fails on cleanup-only errors.'
}

& {
 $scopesAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-scopes.ps1'),[ref]$null,[ref]$null)
& {
    $ack=@($scopesAst.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Wait-Until' -and @($n.CommandElements|Where-Object {$_.Value -eq 'Pen did not choose Red'}).Count},$true))
    $pixels=@($scopesAst.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Wait-Until' -and @($n.CommandElements|Where-Object {$_.Value -eq 'Red counts were not drawn'}).Count},$true))
    $pick=@($scopesAst.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Pick' -and $n.Extent.Text -eq "Pick 'histogram-channel' `$channels[1] 'pen'"},$true))
    if($ack.Count -ne 1 -or $pixels.Count -ne 1 -or $pick.Count -ne 1){throw 'Missing actual Scopes Red channel sequence'}
    $sequence=[scriptblock]::Create(($pick[0].Extent.Text,$ack[0].Extent.Text,$pixels[0].Extent.Text -join "`n"))
    $channels=@('RGB','Red','Green','Blue','Luminance')
    function Pick([string]$Id,[string]$Option,[string]$Device){
        if($Id -ne 'histogram-channel' -or $Option -ne 'Red' -or $Device -ne 'pen'){throw 'Scopes changed its Red channel input'}
        $state.picks++
    }
    function View([string]$Name){if($Name -ne 'histogram'){throw 'Unexpected Scopes view'};$state.histogram}
    function Chart-Shades([string]$Id){
        $state.pixel_reads++
        if($Id -ne 'histogram-chart'){throw 'Unexpected Scopes pixel target'}
        if($null -eq $state.histogram.data){throw 'histogram-chart is not visible'}
        3
    }
    function Wait-Until([scriptblock]$Condition,[string]$Message){
        $state.waits++
        if($Message -eq 'Pen did not choose Red'){
            $reports=@([pscustomobject]@{channel=0;data=@(1)},[pscustomobject]@{channel=1;data=$null})
            if(!$state.missing){$reports+=,[pscustomobject]@{channel=1;data=@(1)}}
            foreach($report in $reports){$state.histogram=$report;if(& $Condition){return}}
            throw $Message
        }
        if($Message -ne 'Red counts were not drawn'){throw 'Scopes changed its acknowledgment failure message'}
        if(& $Condition){return}
        throw $Message
    }
    foreach($missing in $false,$true){
        $state=@{missing=$missing;picks=0;waits=0;pixel_reads=0;histogram=$null}
        $failure=$null;try{. $sequence}catch{$failure=$_}
        if($state.picks -ne 1){throw 'Scopes repeated or omitted its original pen input'}
        if($missing){
            if(!$failure -or $failure.Exception.Message -cne 'Pen did not choose Red' -or $state.waits -ne 1 -or $state.pixel_reads -ne 0){throw 'Scopes missing counts escaped its original acknowledgment bound'}
        }elseif($failure -or $state.waits -ne 2 -or $state.pixel_reads -ne 1){throw "Scopes sampled pixels before acknowledged Red counts: $failure"}
    }
}
Write-Output 'Actual Scopes Red channel sequence: pending counts defer unchanged pixel assertion; absent counts fail original acknowledgment without another input.'

}

& {
 foreach($case in @(
  @('exercise-canvas-bar.ps1','Maximized canvas did not settle'),
  @('exercise-canvas-bar.ps1','The canvas bounds did not settle after drawing'),
  @('exercise-scopes.ps1','The fitted view did not settle'),
  @('exercise-scopes.ps1','The targeted drag did not finish'),
  @('exercise-lifecycle.ps1','Modal input gate did not clear'),
  @('exercise-documents.ps1','Focused native workspace did not finish maximizing'),
  @('exercise-pen-buttons.ps1','Mouse middle drag did not pan the canvas')
 )){
  $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot ('../scripts/'+$case[0])),[ref]$tokens,[ref]$errors)
  $message=$case[1]
  $wait=@($source.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -ceq 'Wait-Until' -and @($n.CommandElements|Where-Object {$_.Value -ceq $message}).Count -eq 1}.GetNewClosure(),$true))
  if($wait.Count -ne 1){throw "Non-renderer wait was moved into canvas progress accounting: $message"}
 }
 'Native geometry, modality and camera acknowledgements stay outside renderer progress.'
}

& {
 $tokens=$null;$errors=$null
 $editing=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-canvas-editing.ps1'),[ref]$tokens,[ref]$errors)
 $actual=$editing.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Stable-Pixels'},$true)
 . ([scriptblock]::Create($actual.Extent.Text))
 $snap=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-snap.ps1'),[ref]$tokens,[ref]$errors)
 $actual=$snap.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Stable-Patch'},$true)
 . ([scriptblock]::Create($actual.Extent.Text))
 foreach($name in 'Stable-Pixels','Stable-Patch'){
  $sample=@{presented=$false;reads=0}
  function Wait-PresentedFrame([switch]$Idle,[int]$Seconds,[switch]$Unscaled,[string]$Message){
   $bound=if($name -ceq 'Stable-Pixels'){6}else{5}
   $failure=if($name -ceq 'Stable-Pixels'){'Artwork samples did not settle'}else{'Canvas pixels did not settle for comparison'}
   if(!$Idle -or !$Unscaled -or $Seconds -ne $bound -or $Message -cne $failure){throw 'Canvas sample changed its idle-content contract, bound or failure'}
   $sample.presented=$true
  }
  function Pixels{if(!$sample.presented){throw 'Canvas pixels were sampled before displayed idle content'};$sample.reads++;'exact-pixels'}
  function Patch-Hash{Pixels}
  if((& $name) -cne 'exact-pixels' -or $sample.reads -ne 1){throw 'Canvas baseline replaced app completion with repeated identical reads'}
 }
 'Canvas baseline samplers require published idle content before exactly one pixel read.'
}

& {
 $prior=$script:CapyStateFile
 try{
  foreach($file in 'exercise-multiwindow.ps1','exercise-localization.ps1'){
   $tokens=$null;$errors=$null
   $source=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot ('../scripts/'+$file)),[ref]$tokens,[ref]$errors)
   $actual=$source.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Ready'},$true)
   . ([scriptblock]::Create($actual.Extent.Text))
   $review=@{Id=33};$directory='owned-registry';$run=$directory;$window=@{id=9;hwnd=77}
   $expected=Join-Path $directory 'ui-state-33-9.json'
   function Model($Window){if($Window.id -ne 9){throw 'Ready queried another drawing'};@{owner=9}}
   function Review-Ready($View,$Handle){if($View.owner -ne 9 -or $Handle -ne 77){throw 'Ready changed its window predicate'};$true}
   function Owned-DrawingWindow($Process,$WindowHandle,$TraceDirectory){if($Process.Id -ne 33 -or $WindowHandle -ne 77 -or $TraceDirectory -cne $directory){throw 'Ready changed its owned registry predicate'};@{Root='owned'}}
   function Wait-Canvas([scriptblock]$Condition,[string]$Message,[int]$Seconds){
    if($script:CapyStateFile -cne $expected){throw 'Ready could credit progress from another owned window'}
    if($reject){throw 'Readiness failed'}
    if(!(& $Condition)){throw $Message}
   }
   foreach($reject in $false,$true){
    $script:CapyStateFile='previous-window';$failure=$null
    try{Ready $window}catch{$failure=$_.Exception.Message}
    if($script:CapyStateFile -cne 'previous-window'){throw 'Ready did not restore the caller receipt on success or failure'}
    if(($reject -and $failure -cne 'Readiness failed') -or (!$reject -and $failure)){throw "Ready receipt binding changed its endpoint: $file $failure"}
   }
  }
 }finally{$script:CapyStateFile=$prior}
 'Per-window Ready binds the exact owned receipt and restores the caller path after success or failure.'
}
