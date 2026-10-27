param([Parameter(Mandatory)][string]$Executable,[ValidateSet('light','dark')][string]$Theme='light')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$repo=(Resolve-Path(Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/sdk-feedback/'+$Theme+'-'+[guid]::NewGuid().ToString('N'));[IO.Directory]::CreateDirectory($run)|Out-Null
$review=$null;$window=[IntPtr]::Zero;$records=[Collections.Generic.List[object]]::new();$passed=0
function Element([string]$Id){
 $windows=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
 foreach($w in $windows){foreach($item in $w.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))){if(!$item.Current.IsOffscreen){return $item}}}
}
function State([string]$Id){$control=Element $Id;if($control-and$control.Current.ItemStatus){$control.Current.ItemStatus|ConvertFrom-Json}}
function Check($condition,[string]$name){if(!$condition){throw $name};$script:passed++}
function Invoke-Action([string]$Id){(Element $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke();Start-Sleep -Milliseconds 120}
function Record([string]$Id,[string]$phase){$s=State $Id;$records.Add(@{id=$Id;phase=$phase;state=$s;bounds=(Element $Id).Current.BoundingRectangle});$s}
function Equal-Color($a,$b){($a-join',')-eq($b-join',')}
function Over($base,$ink,[int]$amount){
 $a=$ink[0]/255.*$amount/255.;$b=$base[0]/255.*(1-$a);$alpha=$a+$b
 $values=@([math]::Round($alpha*255,[MidpointRounding]::AwayFromZero));foreach($i in 1..3){$values+=if($alpha-gt0){[math]::Round(($base[$i]*$b+$ink[$i]*$a)/$alpha,[MidpointRounding]::AwayFromZero)}else{0}};$values
}
function Choice([string]$Id,[string]$phase){
 Start-Sleep -Milliseconds 100;$s=Record $Id $phase
 Check ($s.template_retained-and$s.resources_retained) "$phase retained SDK template/resources"
 $amount=switch($s.common){PointerOver{20} Pressed{41} default{0}};$expected=Over $s.base $s.ink $amount
 Check (Equal-Color $s.background $expected) "$phase effective SDK background $($s.common): actual $($s.background-join',') expected $($expected-join',')"
 Check (Equal-Color $s.glyph $s.ink) "$phase effective SDK glyph follows ink"
 if(!$s.enabled){Check ([math]::Abs($s.content_opacity-92./255)-lt.005) "$phase disabled content opacity"};$s
}
function Position([string]$Id){$b=(Element $Id).Current.BoundingRectangle;@([int]($b.Left+$b.Width/2),[int]($b.Top+$b.Height/2),[int]($b.Left-18),[int]($b.Top+$b.Height/2))}
function Dismiss-Choice([string]$Id){if((State $Id).open){[CapyRowPointer]::Key([uint32]$review.Id,0x1B)};Wait-Until {!(State $Id).open} 'Native ComboBox popup did not dismiss'}
function Choice-Contacts([string]$Id){
 foreach($device in @('mouse','pen','touch')){
  $p=Position $Id;[CapyRowPointer]::Hover($p[0],$p[1]);Wait-Until {(State $Id).common-eq'PointerOver'} "$Id hover not reached";$null=Choice $Id ($device+' hover')
  [CapyRowPointer]::Down($device,$p[0],$p[1]);try{Wait-Until {(State $Id).common-eq'Pressed'} "$Id press not reached";$null=Choice $Id ($device+' held')}finally{[CapyRowPointer]::Move($p[2],$p[3]);[CapyRowPointer]::Up()}
  Dismiss-Choice $Id;[CapyRowPointer]::Hover($p[2],$p[3]);Wait-Until {(State $Id).common-eq'Normal'} "$Id did not retire held state";$null=Choice $Id ($device+' canceled')
 }
}
function Keyboard-Choice([string]$Id){
 $before=State $Id;(Element $Id).SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x73);Wait-Until {(State $Id).open} 'F4 did not open native choice'
 [CapyRowPointer]::Key([uint32]$review.Id,0x28);[CapyRowPointer]::Key([uint32]$review.Id,0x0D);Wait-Until {!(State $Id).open-and(State $Id).selected-eq1} 'Native keyboard choice did not commit'
 $s=Choice $Id 'keyboard selection';Check ($s.selections-eq$before.selections+1) 'Keyboard selection emitted exactly one change'
}
function Footer([string]$Id,[string]$phase,[int]$alpha,[bool]$contact){Start-Sleep -Milliseconds 100;$s=Record $Id $phase;Check ($s.template-and$s.alpha-eq$alpha-and$s.contact-eq$contact) "$phase footer state actual alpha=$($s.alpha), contact=$($s.contact)";$s}
try{
 Enter-CapyEnvironment @('CAPY_FEEDBACK_SDK','CAPY_FEEDBACK_THEME');$env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_FEEDBACK_SDK='1';$env:CAPY_FEEDBACK_THEME=$Theme
 $Executable=(Resolve-Path $Executable).Path;$review=Start-Process -FilePath $Executable -WorkingDirectory (Split-Path $Executable) -WindowStyle Hidden -PassThru -RedirectStandardError(Join-Path $run 'stderr.log');$null=$review.Handle
 Wait-Until {foreach($h in [CapyWindowApi]::Windows([uint32]$review.Id)){$candidate=[System.Windows.Automation.AutomationElement]::FromHandle($h);if($candidate.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'sdk-choice-root'))){$script:window=$h;$script:root=$candidate;return $true}};$false} 'SDK fixture did not start' 30
 [CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null;[CapyWindowApi]::SetForegroundWindow($window)|Out-Null;[CapyRowPointer]::Initialize([uint32]$review.Id)
 Wait-Until {(State 'sdk-choice-root').common} 'SDK fixture metadata unavailable'
 Choice-Contacts 'sdk-choice-root';Keyboard-Choice 'sdk-choice-root'
 $identity=(Element 'sdk-choice-root').GetRuntimeId()-join':';Invoke-Action 'sdk-root-theme';$null=Choice 'sdk-choice-root' 'retained theme';Invoke-Action 'sdk-root-role';$null=Choice 'sdk-choice-root' 'retained drawing role'
 Check ($identity-eq((Element 'sdk-choice-root').GetRuntimeId()-join':')) 'Root choice identity retained'
 Invoke-Action 'sdk-root-disable';$s=Choice 'sdk-choice-root' 'disabled root';Check (!$s.enabled-and!$s.open) 'Disabled native choice stays closed';Invoke-Action 'sdk-root-disable'
 Invoke-Action 'sdk-show';Wait-Until {(State 'sdk-choice-dialog').common-and(State 'sdk-footer-primary').template} 'SDK ContentDialog controls did not load'
 Choice-Contacts 'sdk-choice-dialog';Keyboard-Choice 'sdk-choice-dialog'
 foreach($device in @('mouse','pen','touch')){
  $id='sdk-footer-primary';$p=Position $id;[CapyRowPointer]::Hover($p[0],$p[1]);$null=Footer $id ($device+' footer hover') 20 $false
  [CapyRowPointer]::Down($device,$p[0],$p[1]);try{$null=Footer $id ($device+' footer held') 41 $true}finally{[CapyRowPointer]::Move($p[2],$p[3]);[CapyRowPointer]::Up()};[CapyRowPointer]::Hover($p[2],$p[3]);$null=Footer $id ($device+' footer canceled') 0 $false
 }
 $primaryIdentity=(Element 'sdk-footer-primary').GetRuntimeId()-join':';$secondaryIdentity=(Element 'sdk-footer-secondary').GetRuntimeId()-join':';$accent=(State 'sdk-footer-primary').background
 Invoke-Action 'sdk-default';$a=State 'sdk-footer-primary';$b=State 'sdk-footer-secondary';Check ((Equal-Color $b.background $accent)-and!(Equal-Color $a.background $accent)) 'Default role moves retained accent'
 Invoke-Action 'sdk-dialog-theme';$null=Choice 'sdk-choice-dialog' 'retained popup theme';Invoke-Action 'sdk-dialog-role';$null=Choice 'sdk-choice-dialog' 'retained popup role'
 Check ($primaryIdentity-eq((Element 'sdk-footer-primary').GetRuntimeId()-join':')-and$secondaryIdentity-eq((Element 'sdk-footer-secondary').GetRuntimeId()-join':')) 'Footer identities retained'
 Invoke-Action 'sdk-dialog-disable';$s=Footer 'sdk-footer-secondary' 'disabled footer' 0 $false;Check (!$s.enabled-and[math]::Abs($s.content_opacity-92./255)-lt.005) 'Disabled footer content opacity';Invoke-Action 'sdk-dialog-disable'
 $before=State 'sdk-footer-secondary';(Element 'sdk-dialog-input').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x0D);Wait-Until {(State 'sdk-footer-secondary').secondary-eq$before.secondary+1} 'Native DefaultButton Enter did not activate Secondary'
 Check ((State 'sdk-footer-primary').primary-eq$before.primary) 'DefaultButton Enter did not double activate'
 Invoke-Action 'sdk-default';$before=State 'sdk-footer-primary';(Element 'sdk-dialog-input').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x0D);Wait-Until {(State 'sdk-footer-primary').primary-eq$before.primary+1} 'Native DefaultButton Enter did not activate Primary'
 Check ((State 'sdk-footer-secondary').secondary-eq$before.secondary) 'Primary Enter did not double activate'
 $p=Position 'sdk-footer-close';[CapyRowPointer]::Down('mouse',$p[0],$p[1]);[CapyRowPointer]::Up();Wait-Until {!(Element 'sdk-choice-dialog')} 'Native Close footer did not close dialog'
 [CapyWindowApi]::PostMessage($window,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$review.WaitForExit(8000)-or$review.ExitCode-ne0){throw 'SDK fixture did not close cleanly'};if((Get-Item(Join-Path $run 'stderr.log')).Length){throw 'SDK fixture stderr requires inspection'}
 @{passed=$passed;exit_code=$review.ExitCode;executable=$Executable;sha256=(Get-FileHash $Executable).Hash}|ConvertTo-Json|Set-Content(Join-Path $run 'result.json');Write-Output "SDK feedback passed $passed assertions ($Theme); $run"
}catch{$_|Out-String|Set-Content(Join-Path $run 'failure.txt');try{& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -WindowHandle $window.ToInt64() -Output(Join-Path $run 'failure.png') -Composed -CaptureOnly}catch{};throw}
finally{$records|ConvertTo-Json -Depth 10|Set-Content(Join-Path $run 'states.json');[CapyRowPointer]::Dispose();if($review-and!$review.HasExited){[CapyWindowApi]::PostMessage($window,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$review.WaitForExit(8000)){Stop-Process -Id $review.Id}};Exit-CapyEnvironment}
