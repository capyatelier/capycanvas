$ErrorActionPreference='Stop'
$scripts=Join-Path $PSScriptRoot '../scripts';$tokens=$null;$errors=$null
function Tree([string]$Name){$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $scripts $Name),[ref]$tokens,[ref]$errors);if(@($errors).Count){throw($errors|Out-String)};$ast}
function Function-Text($Ast,[string]$Name){$fn=@($Ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $Name}.GetNewClosure(),$true));if($fn.Count -ne 1){throw "Missing actual function: $Name"};$fn[0].Extent.Text}
$helpers=Tree 'CapyUia.ps1';$idle=Tree 'exercise-idle-brush.ps1'
Add-Type -AssemblyName System.Drawing,WindowsBase
Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
namespace CapyProjectionTest {
 public enum TreeScope{Element,Descendants}public enum AutomationElementMode{None,Full}
 public class Condition{public virtual bool Matches(Info v){return true;}}
 public class PropertyCondition:Condition{string key;object value;public PropertyCondition(object k,object v){key=(string)k;value=v;}public override bool Matches(Info v){return Object.Equals(key=="pid"?(object)v.ProcessId:v.AutomationId,value);}}
 public class AndCondition:Condition{Condition[] values;public AndCondition(params Condition[] v){values=v;}public override bool Matches(Info v){foreach(var c in values)if(!c.Matches(v))return false;return true;}}
 public class OrCondition:Condition{Condition[] values;public OrCondition(params Condition[] v){values=v;}public override bool Matches(Info v){foreach(var c in values)if(c.Matches(v))return true;return false;}}
 public static class Automation{public static Condition RawViewCondition=new Condition();}
 public static class ControlType{public static string Edit="Edit",Button="Button";}
 public static class AutomationElement{public static object AutomationIdProperty="id",NameProperty="name",IsEnabledProperty="enabled",ItemStatusProperty="status",ControlTypeProperty="type",IsOffscreenProperty="offscreen",ProcessIdProperty="pid";}
 public class CacheRequest {
  public static CacheRequest Active;public static int Created,Disposed;public TreeScope TreeScope;public Condition TreeFilter;public AutomationElementMode AutomationElementMode;public List<object> Properties=new List<object>();
  public CacheRequest(){Created++;AutomationElementMode=AutomationElementMode.Full;}public void Add(object p){Properties.Add(p);}public IDisposable Activate(){if(Active!=null)throw new Exception("Nested batch");Active=this;return new Scope();}
  class Scope:IDisposable{public void Dispose(){Active=null;Disposed++;}}
 }
 public class Info {
  public string AutomationId,Name,ItemStatus,ControlType;public bool IsEnabled=true,IsOffscreen;public int ProcessId=33;
  public Info(string id,string name,string status,string type){AutomationId=id;Name=name;ItemStatus=status;ControlType=type;}
  public Info Copy(){return (Info)MemberwiseClone();}
 }
 public class Element{Info info;public Element(Info v){info=v.Copy();}public Info Cached{get{return info;}}public Info Current{get{throw new Exception("Current fallback forbidden");}}}
 public class Root {
  public int Batches;public List<Info> Rows=new List<Info>();
  public Element[] FindAll(TreeScope scope,Condition filter){var c=CacheRequest.Active;if(scope!=TreeScope.Descendants||c==null||c.TreeScope!=TreeScope.Element||c.AutomationElementMode!=AutomationElementMode.None||c.TreeFilter!=Automation.RawViewCondition||c.Properties.Count!=7)throw new Exception("Missing fresh cache-only batch contract");foreach(var p in new[]{"id","name","enabled","status","type","offscreen","pid"})if(!c.Properties.Contains(p))throw new Exception("Missing cached property");Batches++;var rows=new List<Element>();foreach(var row in Rows)if(filter.Matches(row))rows.Add(new Element(row));return rows.ToArray();}
 }
 public struct Rect{public int X,Y,Width,Height;public Rect(int x,int y,int w,int h){X=x;Y=y;Width=w;Height=h;}}
 public class CanvasInfo{public bool IsEnabled=true,IsOffscreen;public int ProcessId=33;public Rect BoundingRectangle=new Rect(100,200,96,96);}
 public class Canvas{public CanvasInfo Value=new CanvasInfo();public int CurrentReads;public CanvasInfo Current{get{CurrentReads++;return Value;}}public object Cached{get{throw new Exception("Canvas values must remain live");}}public void SetFocus(){}}
}
public static class CapyRowPointer{public static IntPtr Foreground=new IntPtr(22);public static int X,Y;public struct Point{public int x,y;}public struct CursorInfo{public Point position;}public static CursorInfo Cursor(){return new CursorInfo{position=new Point{x=X,y=Y}};}public static IntPtr GetForegroundWindow(){return Foreground;}}
'@
foreach($name in @('Read-ControlSnapshot','Snapshot-Control')){. ([scriptblock]::Create((Function-Text $helpers $name).Replace('System.Windows.Automation.','CapyProjectionTest.')))}
. ([scriptblock]::Create((Function-Text $idle 'Check-BrushProjection').Replace('System.Windows.Automation.','CapyProjectionTest.')))
$review=[pscustomobject]@{Id=33};$root=[CapyProjectionTest.Root]::new();$CapyFind='prefer-visible';$script:modelReads=0;$script:attempts=1;$script:attemptHook=$null
function Model{$script:modelReads++;$script:model}
function Find{throw 'Per-row Find forbidden in projection snapshot'}
function Wait-Until([scriptblock]$Condition,[string]$Message){foreach($i in 1..$script:attempts){if($script:attemptHook){& $script:attemptHook $i};if(& $Condition){return}};throw $Message}
function Copy-Value($Value){$Value|ConvertTo-Json -Depth 30 -Compress|ConvertFrom-Json}
$good='{"state":{"brush":{"preset":10,"tool":"blend"},"tool_set":{"groups":[{"label":"Blend","enabled":true,"selected":true,"action":{"type":"choose_tool_variant","variant":{"type":"command","command":"blend"}}},{"label":"Clone","enabled":true,"selected":false,"action":{"type":"choose_tool_variant","variant":{"type":"command","command":"clone"}}}],"subtools":[{"label":"Blend set","enabled":true,"selected":true,"action":{"type":"select_tool_group","group":"blend"}},{"label":"Wet paint","enabled":true,"selected":false,"action":{"type":"select_tool_group","group":"wet_paint"}},{"label":"Effects","enabled":true,"selected":false,"action":{"type":"select_tool_group","group":"effects"}},{"label":"Smudge","enabled":true,"selected":true,"action":{"type":"select_brush","id":10}},{"label":"Soft Blender","enabled":true,"selected":false,"action":{"type":"select_brush","id":45}}]},"tool_settings":[{"id":"size","label":"Size"},{"id":"flow","label":"Strength"}]}}'|ConvertFrom-Json
$rows=@(@('tool-group-0','Blend','Selected','Button'),@('tool-group-1','Clone','','Button'),@('tool-subtool-0','Blend set','Selected','Button'),@('tool-subtool-1','Wet paint','','Button'),@('tool-subtool-2','Effects','','Button'),@('tool-subtool-3','Smudge','Selected','Button'),@('tool-subtool-4','Soft Blender','','Button'),@('tool-setting-size','Edit Size','','Edit'),@('tool-setting-flow','Edit Strength','','Edit'))
$preset=[pscustomobject]@{label='Smudge';id=10;group='blend'};$cases=[Collections.Generic.List[string]]::new()
function Reset-Case{
 $script:model=Copy-Value $good;$root.Rows.Clear();foreach($r in $rows){$root.Rows.Add([CapyProjectionTest.Info]::new($r[0],$r[1],$r[2],$r[3]))}
 $root.Batches=0;$script:modelReads=0;$script:attempts=1;$script:attemptHook=$null;[CapyProjectionTest.CacheRequest]::Created=0;[CapyProjectionTest.CacheRequest]::Disposed=0
}
function Probe([string]$Name,[scriptblock]$Mutation,[bool]$Accepted){
 Reset-Case;& $Mutation;$accepted=$true
 try{$null=Check-BrushProjection $preset}catch{if($_.Exception.Message -notlike 'Native categories,*'){throw};$accepted=$false}
 if($accepted -ne $Accepted -or $root.Batches -ne $script:modelReads -or $root.Batches -lt 1 -or [CapyProjectionTest.CacheRequest]::Created -ne $root.Batches -or [CapyProjectionTest.CacheRequest]::Disposed -ne $root.Batches){throw "Wrong projection/batch outcome: $Name"};$cases.Add($Name)
}
Probe 'valid independent hierarchy and cached native values' {} $true
Probe 'wrong native name' {$root.Rows[0].Name='Wrong'} $false
Probe 'wrong native enabled' {$root.Rows[0].IsEnabled=$false} $false
Probe 'wrong native selected status' {$root.Rows[0].ItemStatus=''} $false
Probe 'extra native subtool' {$root.Rows.Add([CapyProjectionTest.Info]::new('tool-subtool-5','Extra','','Button'))} $false
Probe 'extra native group' {$root.Rows.Add([CapyProjectionTest.Info]::new('tool-group-2','Extra','','Button'))} $false
Probe 'foreign native PID' {$root.Rows[0].ProcessId=99} $false
Probe 'wrong settings control type' {$root.Rows[8].ControlType='Button'} $false
Probe 'disabled setting' {$root.Rows[8].IsEnabled=$false} $false
Probe 'missing setting' {$root.Rows.RemoveAt(8)} $false
Probe 'wrong setting caption' {$root.Rows[8].Name='Wrong'} $false
Probe 'wrong shared preset' {$script:model.state.brush.preset=45} $false
Probe 'two selected variant groups' {$script:model.state.tool_set.groups[1].selected=$true} $false
Probe 'wrong variant with matching cached status' {$script:model.state.tool_set.groups[0].action.variant.command='clone'} $false
Probe 'wrong requested category' {$script:model.state.tool_set.subtools[0].action.group='effects'} $false
Probe 'no selected category' {$script:model.state.tool_set.subtools[0].selected=$false} $false
Probe 'two selected presets' {$script:model.state.tool_set.subtools[4].selected=$true} $false
Probe 'fresh model and batch on retry' {$root.Rows[0].Name='Wrong';$script:model.state.brush.preset=45;$script:attempts=2;$script:attemptHook={param($i)if($i -eq 2){$root.Rows[0].Name='Blend';$script:model.state.brush.preset=10}}} $true
Reset-Case;$records=@(Read-ControlSnapshot @('tool-group-0'));$root.Rows[0].Name='Changed after snapshot'
if($records[0].Name -cne 'Blend'){throw 'Phase snapshot retained mutable provider data'}
$hidden=[pscustomobject]@{AutomationId='same';Name='hidden';ControlType='Edit';IsOffscreen=$true};$visible=[pscustomobject]@{AutomationId='same';Name='visible';ControlType='Edit';IsOffscreen=$false}
if((Snapshot-Control @($hidden,$visible) 'same').Name -cne 'visible'){throw 'First-visible selection changed'}
$CapyFind='first';if((Snapshot-Control @($hidden,$visible) 'same').Name -cne 'hidden'){throw 'First selection changed'}
$CapyFind='prefer-visible';if((Snapshot-Control @($hidden) 'same').Name -cne 'hidden'){throw 'Prefer-visible fallback changed'}
$typed=[pscustomobject]@{AutomationId='same';Name='text';ControlType='Button';IsOffscreen=$false}
if((Snapshot-Control @($typed,$visible) 'same' -Type 'Edit').Name -cne 'visible'){throw 'Type filtering changed'}
$start=($idle.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Start-Review'},$true)).Body.EndBlock.Statements
$reset=@($start|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -eq '$script:idleCanvas' -and $_.Right.Extent.Text -eq '$null'})
$acquire=@($start|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -eq '$script:idleCanvas' -and $_.Right.Extent.Text.Contains("Control 'drawing-canvas'")})
$launch=$idle.Find({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Start-Process'},$true)
if($reset.Count -ne 1 -or $acquire.Count -ne 1 -or $reset[0].Extent.StartOffset -ge $launch.Extent.StartOffset -or !$acquire[0].Extent.Text.Contains('-Arranged -NoPopups')){throw 'Canvas lifetime/reset/owned acquisition changed'}
$script:discoveries=0;$script:nextCanvas=[CapyProjectionTest.Canvas]::new()
function Control([string]$Id,[switch]$Arranged,[switch]$NoPopups){if($Id -cne 'drawing-canvas' -or !$Arranged -or !$NoPopups){throw 'Unexpected cached canvas acquisition'};$script:discoveries++;$script:nextCanvas}
$script:idleCanvas=[CapyProjectionTest.Canvas]::new()
. ([scriptblock]::Create($reset[0].Extent.Text));if($null -ne $idleCanvas){throw 'Old review canvas survived reset'}
. ([scriptblock]::Create($acquire[0].Extent.Text));$first=$idleCanvas;$script:nextCanvas=[CapyProjectionTest.Canvas]::new()
. ([scriptblock]::Create($reset[0].Extent.Text));. ([scriptblock]::Create($acquire[0].Extent.Text))
if($idleCanvas -eq $first -or $discoveries -ne 2){throw 'New review did not acquire its own canvas once'}
$ownerGuard=@($start|Where-Object {$_ -is [Management.Automation.Language.IfStatementAst] -and $_.Extent.Text.Contains('The canvas peer does not belong')})
if($ownerGuard.Count -ne 1){throw 'Owned cached-canvas acquisition guard missing'}
$idleCanvas.Value.ProcessId=99;$rejected=$false;try{. ([scriptblock]::Create($ownerGuard[0].Extent.Text))}catch{if($_.Exception.Message -cne 'The canvas peer does not belong to the owned review'){throw};$rejected=$true}
if(!$rejected){throw 'Foreign cached canvas was accepted'};$idleCanvas.Value.ProcessId=33
foreach($name in @('Camera-Key','Document-Hash')){. ([scriptblock]::Create((Function-Text $idle $name)))}
$script:model=[pscustomobject]@{state=[pscustomobject]@{camera=[pscustomobject]@{viewport=@(96,96);translation=@(0,0);zoom=1;rotation=0;flipped=@($false,$false);work_area=@(0,0,96,96)};layer_tools=[pscustomobject]@{has_selection=$false;quick_mask=$false};settings_open=$false;requests=@();customization=[pscustomobject]@{drawer=$null;control=$null};command_search=$null}}
$projection=@{bounds=$idleCanvas.Current.BoundingRectangle;camera=(Camera-Key $script:model.state.camera);work_area='0,0,96,96';document_crop=[Drawing.Rectangle]::new(0,0,96,96)};$drawingWindow=[IntPtr]22
$samplePoints=@(foreach($y in 0..23){foreach($x in 0..31){,@((101+2*$x),(201+2*$y))}})
$expected=@(foreach($i in 0..767){[Drawing.Color]::FromArgb(255,($i%256),[int][Math]::Floor($i/256),(($i*17)%256)).ToArgb()}) -join ','
$script:pixelMutation=$false
function Fill-Pixels($Bitmap){foreach($i in 0..767){$p=$samplePoints[$i];$Bitmap.SetPixel($p[0]-100,$p[1]-200,[Drawing.Color]::FromArgb(255,($i%256),[int][Math]::Floor($i/256),(($i*17)%256)))};if($script:pixelMutation){$Bitmap.SetPixel(0,0,[Drawing.Color]::FromArgb(255,1,2,3))}}
$pixels=Function-Text $idle 'Pixels';$copy='$g.CopyFromScreen([int]$bounds.X,[int]$bounds.Y,0,0,$bitmap.Size)'
if(!$pixels.Contains($copy)){throw 'Original screen copy changed'};. ([scriptblock]::Create($pixels.Replace($copy,'Fill-Pixels $bitmap')))
$bytes=[byte[]]::new(96*96*4)
foreach($i in 0..767){$x=1+2*($i%32);$y=1+2*[int][Math]::Floor($i/32);$at=4*($y*96+$x);$bytes[$at]=($i*17)%256;$bytes[$at+1]=[int][Math]::Floor($i/256);$bytes[$at+2]=$i%256;$bytes[$at+3]=255}
$sha=[Security.Cryptography.SHA256]::Create();try{$expectedDense=[BitConverter]::ToString($sha.ComputeHash($bytes)).Replace('-','')}finally{$sha.Dispose()}
$expectedSignature=$expected+';document_argb_sha256='+$expectedDense
$idleCanvas.CurrentReads=0;$script:modelReads=0;$beforeDiscoveries=$discoveries
foreach($i in 1..2){$value=@(Pixels);if($value.Count -ne 1 -or $value[0].Split(';')[0] -cne $expected -or $value[0] -cne $expectedSignature){throw 'Cached canvas changed exact768 ordered pixel string'}}
if($idleCanvas.CurrentReads -ne 2 -or $script:modelReads -ne 2 -or $discoveries -ne $beforeDiscoveries){throw 'Pixels cached mutable bounds/camera or rediscovered canvas'}
$script:pixelMutation=$true;$changed=Pixels
if($changed -ceq $expectedSignature -or $changed.Split(';')[0] -cne $expected){throw 'Dense oracle missed a one-pixel change invisible to all768 samples'}
$script:pixelMutation=$false;if((Pixels) -cne $expectedSignature){throw 'Dense oracle failed exact pixel restoration'}
function Reject-Pixels([string]$Name,[scriptblock]$Mutation){& $Mutation;$rejected=$false;try{Pixels|Out-Null}catch{if($_.Exception.Message -notlike '*composed*'){throw};$rejected=$true};if(!$rejected){throw "Pixel guard weakened: $Name"}}
Reject-Pixels 'bounds' {$idleCanvas.Value.BoundingRectangle=[CapyProjectionTest.Rect]::new(101,200,96,96)}
$idleCanvas.Value.BoundingRectangle=[CapyProjectionTest.Rect]::new(100,200,96,96)
Reject-Pixels 'camera' {$script:model.state.camera.translation=@(1,0)};$script:model.state.camera.translation=@(0,0)
Reject-Pixels 'foreground' {[CapyRowPointer]::Foreground=[IntPtr]23};[CapyRowPointer]::Foreground=[IntPtr]22
Reject-Pixels 'work area' {$script:model.state.camera.work_area=@(1,0,95,96)};$script:model.state.camera.work_area=@(0,0,96,96)
Reject-Pixels 'disabled canvas' {$idleCanvas.Value.IsEnabled=$false};$idleCanvas.Value.IsEnabled=$true
Reject-Pixels 'offscreen canvas' {$idleCanvas.Value.IsOffscreen=$true};$idleCanvas.Value.IsOffscreen=$false
Reject-Pixels 'selection overlay' {$script:model.state.layer_tools.has_selection=$true};$script:model.state.layer_tools.has_selection=$false
Reject-Pixels 'pointer in crop' {[CapyRowPointer]::X=120;[CapyRowPointer]::Y=220};[CapyRowPointer]::X=0;[CapyRowPointer]::Y=0
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
. ([scriptblock]::Create((Function-Text $helpers 'Read-ControlSnapshot').Replace('function Read-ControlSnapshot','function Real-ReadControlSnapshot')))
$root=[pscustomobject]@{}
$root|Add-Member ScriptMethod FindAll {param($Scope,$Condition)
 if($Scope -ne [System.Windows.Automation.TreeScope]::Descendants -or [System.Windows.Automation.CacheRequest]::Current.AutomationElementMode -ne [System.Windows.Automation.AutomationElementMode]::None){throw 'Actual UIA cache activation differs'}
 [pscustomobject]@{Cached=[pscustomobject]@{AutomationId='one';Name='Known';IsEnabled=$true;ItemStatus='Selected';ControlType=[System.Windows.Automation.ControlType]::Edit;IsOffscreen=$false;ProcessId=33}}
}
$actual=@(Real-ReadControlSnapshot @('one','two'));if($actual.Count -ne 1 -or $actual[0].Name -cne 'Known'){throw 'Actual UIA API constructor/cache-only activation failed'}
Write-Output "UIA projection snapshot passed: $($cases.Count) hierarchy/cache cases, fresh cached-only batch/visibility semantics, per-review canvas reset, live per-pixel bounds/camera/foreground, exact768 prefix and independent dense ARGB hash with nonsampled-pixel change/restoration; no UI/GPU."
