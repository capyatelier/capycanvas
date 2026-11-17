param([string]$Fixture=(Join-Path $PSScriptRoot '../scripts/exercise-tool-bubbles.ps1'))
$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($Fixture,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Panel fling fixture did not parse'}
$helpers=@()
foreach($name in @('Read-PanelScrollReceipt','Test-PanelScrollFling','Wait-PanelScrollFling','Test-PanelScrollSettled','Settle-LayerScroll')){
 $helper=$ast.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq$name},$true)
 if($helper){$helpers+=,$helper;Invoke-Expression $helper.Extent.Text}
}
$contacts=@($ast.FindAll({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq'Layer-ListContacts'},$true))
if($contacts.Count-ne1){throw 'Expected one actual layer-list contact journey'}
$statements=@($contacts[0].Body.EndBlock.Statements);$fling=@();$injection=@();$end=@()
foreach($i in 0..($statements.Count-1)){
 if($statements[$i]-is[Management.Automation.Language.AssignmentStatementAst]-and$statements[$i].Left.Extent.Text-eq'$fling'){$fling+=,$i}
 if($fling.Count-and$i-gt$fling[0]-and$statements[$i]-is[Management.Automation.Language.TryStatementAst]){$injection+=,$i}
 if($injection.Count-and$i-gt$injection[0]-and$statements[$i].Extent.Text-match'^Native-InertiaInterruption\b'){$end+=,$i}
}
if($fling.Count-ne1-or!$injection.Count-or!$end.Count){throw 'Expected one actual post-fling oracle boundary'}
$oracle=($statements[($injection[0]+1)..($end[0]-1)]|ForEach-Object {$_.Extent.Text})-join"`n"
$run=$PSScriptRoot;$revision=7;$scroll=[pscustomobject]@{Current=[pscustomobject]@{VerticalScrollPercent=80}}
$flingBefore=@{value=[pscustomobject]@{contact=2;serial=8}}
function Revision{7}
function Start-Sleep{}
function Set-Content{}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds,[switch]$Closing,[scriptblock]$ResetBound){
 if($ResetBound){$null=&$ResetBound}
 if(!(&$Condition)){throw $Message}
}
function Receipt{
 [pscustomobject]@{contact=3;serial=13;terminal=$true;holding=$false;running=$false;idle=$true;offset=50;scrollable_height=100;final_target=[pscustomobject]@{kind='target';target=60};last_applied=[pscustomobject]@{kind='applied';offset=50};release=[pscustomobject]@{kind='release';serial=10;tick=20;target=40;offset=35};inertia=[pscustomobject]@{kind='target';serial=11;tick=21;target=60;offset=35;inertial=$true};applied=[pscustomobject]@{kind='applied';serial=12;tick=22;target=60;offset=50}}
}
function Envelope($Receipt){@{phase='idle';generation=7;panel_scroll=$Receipt}|ConvertTo-Json -Depth 10 -Compress}
$list=[pscustomobject]@{Current=[pscustomobject]@{ItemStatus=(Envelope (Receipt))}}
Invoke-Expression $oracle
if($helpers.Count-ne5){throw 'Panel fling indexed oracle helpers are missing'}
foreach($case in @('no-inertia','before-release','no-applied-motion','stale-contact','wrong-contact','backwards-tick')){
 $receipt=Receipt
 switch($case){
  'no-inertia'{$receipt.inertia.inertial=$false}
  'before-release'{$receipt.inertia.serial=9}
  'no-applied-motion'{$receipt.applied.offset=35}
  'stale-contact'{$receipt.contact=2}
  'wrong-contact'{$receipt.contact=4}
  'backwards-tick'{$receipt.applied.tick=19}
 }
 $list.Current.ItemStatus=Envelope $receipt
 $failure=$null;try{Invoke-Expression $oracle}catch{$failure=$_}
 if(!$failure){throw "Panel fling oracle accepted $case"}
}
'Panel fling oracle retains completed app movement and rejects missing or misordered release/inertia/application receipts'
$settle=Receipt;$settle.offset=60
if(Test-PanelScrollSettled $settle 60 3){throw 'Native settle credited a completed target before ViewChanged applied it'}
$settle.last_applied.offset=60
if(!(Test-PanelScrollSettled $settle 60 3)){throw 'Native settle rejected its completed applied target'}
$settle.contact=2
if(Test-PanelScrollSettled $settle 60 3){throw 'Native settle credited a stale contact completion'}
$settle.contact=3;$settle.terminal=$false;$settle.holding=$true;$settle.running=$true
if(Test-PanelScrollSettled $settle 60 3){throw 'Native settle credited a held gesture as complete'}
if(!(Test-PanelScrollSettled $settle 60 3 -Held)){throw 'Native boundary settle rejected its held applied target'}
$settle.running=$false
if(Test-PanelScrollSettled $settle 60 3 -Held){throw 'Native boundary settle credited a Down before native pan admission'}
$settle.holding=$false;$settle.running=$false;$settle.contact=99;$settle.offset=0
if(!(Test-PanelScrollSettled $settle 0)){throw 'Nongesture readiness incorrectly required a foreign contact completion'}
$list.Current.ItemStatus=Envelope $settle
$failure=$null;try{Settle-LayerScroll $list $scroll -TargetPercent 0}catch{$failure=$_}
if(!$failure){throw 'Nongesture settle credited a queued SetScrollPercent before its own target applied'}
$scroll.Current.VerticalScrollPercent=0;Settle-LayerScroll $list $scroll -TargetPercent 0
'Native scroll settling requires its own terminal/applied target and nongesture readiness remains independent'
