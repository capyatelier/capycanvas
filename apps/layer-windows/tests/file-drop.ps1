$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$scripts=Join-Path $PSScriptRoot '../scripts'
$fixture=Join-Path $scripts 'exercise-file-activation.ps1'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($fixture,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'File activation fixture did not parse'}
$function=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Drop-Files'},$true)
if(!$function){throw 'Missing actual drop request helper'}
$dpi='[CapyWindowApi]::GetDpiForWindow($drawingWindow)'
if(!$function.Extent.Text.Contains($dpi)){throw 'File drop lost its actual window scale boundary'}
. ([scriptblock]::Create($function.Extent.Text.Replace($dpi,'144')))
$script:arguments=[pscustomobject]@{Current=[pscustomobject]@{ItemStatus='';IsOffscreen=$true;BoundingRectangle=[pscustomobject]@{X=1200;Y=90;Width=180;Height=32}}}
$script:pattern=[pscustomobject]@{}
$script:pattern|Add-Member ScriptMethod SetValue {param($value)$script:payload=$value;$script:arguments.Current.ItemStatus=''}
$script:arguments|Add-Member ScriptMethod GetCurrentPattern {param($kind)$script:pattern}
function Control([string]$Id,[switch]$Arranged){
 if($Id -eq 'test-file-drop-arguments'){
  if($Arranged){throw 'Programmatic drop parameter is offscreen'}
  return $script:arguments
 }
 if(!$Arranged -or $Id -notin @('drawing-canvas','drawing-tabs')){throw 'Actual drop target did not use arranged geometry'}
 [pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=[pscustomobject]@{X=60;Y=90;Width=300;Height=150}}}
}
function Invoke([string]$Id,[switch]$Name){
 if($Id -cne 'Test file drop' -or !$Name){throw 'Drop request bypassed its native smoke command'}
 $reply=@{request=$script:payload;effect=$script:effect}
 if($script:failure){$reply.error=$script:failure}
 if($script:stale){$reply.request=$script:lastAcknowledged}else{$script:lastAcknowledged=$script:payload}
 $script:arguments.Current.ItemStatus=$reply|ConvertTo-Json -Compress
}
function Wait-Until([scriptblock]$Condition,[string]$Message){if(!(&$Condition)){throw $Message}}
$script:effect='Copy';$script:failure=$null;$script:stale=$false
$paths=@(('photo '+[char]0x65e5+'.png'),'saved drawing.capy')
foreach($target in 'drawing-canvas','drawing-tabs'){
 if((Drop-Files $paths $target) -cne 'Copy'){throw 'Drop request did not retain its native operation result'}
 $request=$script:payload|ConvertFrom-Json
 if(($request.paths -join '|') -cne ($paths -join '|') -or $request.point[0] -ne 100 -or $request.point[1] -ne 50){throw 'Drop request changed path order or actual-DPI point mapping'}
 if($request.target -cne $(if($target -eq 'drawing-canvas'){'canvas'}else{'tabs'})){throw 'Drop request changed its target'}
}
$script:effect='None'
if((Drop-Files $paths 'drawing-canvas') -cne 'None'){throw 'Refused data was reported as copied'}
$script:failure='Original drop failure [0x80070005]';$caught=$null
try{Drop-Files $paths 'drawing-canvas'|Out-Null}catch{$caught=$_.Exception.Message}
if($caught -cne $script:failure){throw 'Native HRESULT or message was swallowed by the fixture'}
$script:failure=$null;$script:effect='Copy';$script:stale=$false
$null=Drop-Files $paths 'drawing-canvas'
$script:stale=$true;$caught=$null
try{Drop-Files $paths 'drawing-canvas'|Out-Null}catch{$caught=$_.Exception.Message}
if($caught -cne 'The file drop command did not acknowledge its request'){throw 'A stale native drop result acknowledged a new request'}
$canvas=[IO.File]::ReadAllText((Join-Path $scripts '../CanvasWindow.cpp'))
$tabs=[IO.File]::ReadAllText((Join-Path $scripts '../DrawingTabs.h'))
$header=[IO.File]::ReadAllText((Join-Path $scripts '../HeaderView.cpp'))
if(([regex]::Matches($canvas,'CapyUi::fileDropAllowed\(')).Count -ne 2 -or !$tabs.Contains('fileDropAllowed(self->data->state,DropKind::Images,DropTarget::Tabs)') -or !$header.Contains('fileDropAllowed(impl->data->state,CapyUi::DropKind::Images,CapyUi::DropTarget::Tabs)')){throw 'Native DragOver and smoke no longer share presented-command eligibility'}
if(!$canvas.Contains(':header->FileDropAllowed()') -or !$canvas.Contains('if(allowed&&target==L"canvas")') -or !$canvas.Contains('else if(allowed)effect=')){throw 'Smoke drop bypasses presented-command eligibility'}
foreach($view in 'workspace','header'){if(!$canvas.Contains("$view`->SetReport(report)")){throw "Missing production drop reporter for $view"}}
foreach($boundary in @('co_await self->DropFiles(event.DataView(),event.GetPosition(self->panel))','effect=co_await DropFiles(content,','CapyUi::receiveFileDrop(content,CapyUi::drawingTabsFileDrop()')){
 if(!$canvas.Contains($boundary)){throw "Canvas native/smoke drop bypasses its shared boundary: $boundary"}
}
if(!$tabs.Contains('receiveFileDrop(event,drawingTabsFileDrop()')){throw 'Production drawing tabs bypass their original shared command mapping'}
$hook=$canvas.Substring($canvas.IndexOf('fire_and_forget CanvasWindow::TestFileDrop('))
$hook=$hook.Substring(0,$hook.IndexOf('void CanvasWindow::Replay('))
if($hook.IndexOf('CAPY_SMOKE_TEST') -lt 0 -or $hook.IndexOf('windows_isolated_settings') -lt 0 -or $hook.IndexOf('windows_isolated_settings') -gt $hook.IndexOf('fileDropPackage(')){throw 'The native smoke drop can access files before both real guards'}
if([IO.File]::ReadAllText($fixture) -match 'DoDragDrop|SendInput|CapyFileDrag'){throw 'Obsolete OLE transport still runs in the file activation fixture'}
'Native file drop request, target mapping, errors, matching acknowledgment and shared wiring passed'
