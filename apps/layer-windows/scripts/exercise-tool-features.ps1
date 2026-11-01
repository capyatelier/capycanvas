param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][long]$WindowHandle,[Parameter(Mandatory)][string]$StateFile,[Parameter(Mandatory)][string]$OutputDirectory,[Parameter(Mandatory)][ValidateSet('smoothing','size-keys','preview','size-controls')][string]$Journey,[ValidateSet('dark','light')][string]$Theme)
$ErrorActionPreference='Stop'
$scripts=$PSScriptRoot
. (Join-Path $scripts 'CapyUia.ps1')
$featureFind=(Get-Command Find).ScriptBlock;$featureControl=(Get-Command Control).ScriptBlock
Add-Type -AssemblyName System.Drawing
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $scripts 'RowPointerDriver.cs')}
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$review=Get-Process -Id $ProcessId;$app=$review;$directory=Split-Path -Parent $review.Path;$CapyStateFile=$StateFile;$CapyTraceDirectory=Split-Path -Parent $StateFile
$native=Owned-DrawingWindow $review -WindowHandle $WindowHandle
if(!$native){throw 'Owned drawing window unavailable'}
$root=$native.Root;$drawingWindow=$native.Handle
[IO.Directory]::CreateDirectory($OutputDirectory)|Out-Null
function Import-FeatureHelpers([string]$File,[string[]]$Names){
 $tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $scripts $File),[ref]$tokens,[ref]$errors)
 if(@($errors).Count){throw 'Original feature helper parser failed'}
 foreach($name in $Names){$fn=@($ast.FindAll({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq$name},$true));if($fn.Count-ne1){throw 'Expected one original feature helper'};$text=$fn[0].Extent.Text;if(!$text.StartsWith('function '+$name)){throw 'Original feature function signature changed'};Invoke-Expression ('function script:'+$text.Substring('function '.Length))}
}
if($Journey -in @('preview','size-controls')){
 $CapyPopups=$true
 Import-FeatureHelpers 'exercise-toolbar-components.ps1' @('Control','Box','Tile','Brush','Along','Press','Drag','Release','Switch-Workspace')
}else{
 Import-FeatureHelpers 'exercise-tools.ps1' @('Model','Wait-Until','Find','Control','Arranged','Reveal-Control','Reveal-Number','Invoke-Control','Invoke-Id','Field','Value','Focus','Draft','Select-Tool','Check-Projection')
}
Import-FeatureHelpers 'exercise-localization.ps1' @('Focus-Canvas','Open-Search','Key')
Import-FeatureHelpers 'LocalizationProfiles.ps1' @('Search-Command')
function Search{$snapshot=Read-Snapshot ($StateFile-replace'ui-state-','search-state-');if($snapshot.process_id-ne$ProcessId){throw 'Search owner mismatch'};$snapshot.search}
function Feature-Command([string]$Id){
 function Find([string]$Value,[switch]$Name,$Within=$root,$Type,[switch]$Visible){& $featureFind @PSBoundParameters}
 function Control([string]$Value,[switch]$Name,$Within=$root,$Type,[int]$Seconds=$script:CapyWaitSeconds,[switch]$Arranged){& $featureControl @PSBoundParameters}
 $ready=@{command=$null}
 Wait-Until {$commands=@((Model).state.commands|Where-Object id -eq $Id);if($commands.Count-eq1-and$commands[0].enabled){$ready.command=$commands[0];return $true};$false} ('Shared feature command unavailable: '+$Id) 8
 Search-Command $ready.command.label $Id
}
function Dismiss-FeatureDrawer{if((Model).state.customization.drawer){(Control 'drawing-canvas' $null -Id).SetFocus();[CapyRowPointer]::Key([uint32]$ProcessId,27);Wait-Until {$null-eq(Model).state.customization.drawer} 'Feature drawer did not close' 8}}
function Feature-Park{
 if($Journey -in @('preview','size-controls')){
  $button=Control ('slider-cap-'+(Tile $(if($Journey-eq'size-controls'){'brush_size_slider'}else{'brush_strength_slider'})).id)
  $bounds=$button.Current.BoundingRectangle;$x=$bounds.Left+$bounds.Width/2;$y=$bounds.Top+$bounds.Height/2
 }else{
  $bounds=$root.Current.BoundingRectangle;$x=$bounds.Left+$bounds.Width/2;$y=$bounds.Top+12
  $sampleBounds=[Windows.Rect]::new($featurePixelOrigin[0]-48,$featurePixelOrigin[1]-48,96,96)
  if($bounds.IsEmpty-or!$bounds.Contains($x,$y)-or$sampleBounds.Contains($x,$y)){throw "Owned parking point leaves window or covers original96px pixels: window=$bounds; point=$x,$y; sample=$sampleBounds"}
 }
 [CapyRowPointer]::Hover([int]$x,[int]$y)
}
function Feature-CanvasPixels{
 if([CapyRowPointer]::GetForegroundWindow()-ne$drawingWindow){throw 'Feature drawing must own foreground'}
 $canvas=if($Journey -in @('preview','size-controls')){Control 'drawing-canvas'}else{Control 'drawing-canvas' $null -Id}
 if($canvas.Current.BoundingRectangle-ne$featureCanvasBounds){throw 'Feature canvas projection changed'}
 Screen-Pixels $featurePixelOrigin[0] $featurePixelOrigin[1]
}
function Begin-FeatureEvidence{
 $v=Model;$script:featureDocument=$v.state.document_file|ConvertTo-Json -Depth 20 -Compress;$script:featureGpu=$v.windows_gpu_generation
 $canvas=if($Journey -in @('preview','size-controls')){Control 'drawing-canvas'}else{Control 'drawing-canvas' $null -Id};$script:featureCanvasBounds=$canvas.Current.BoundingRectangle
 $camera=$v.state.camera;$tab=@($v.state.tabs|Where-Object active)[0]
 if($camera.rotation-ne0-or$camera.flipped[0]-or$camera.flipped[1]){throw 'Feature pixel projection requires original unrotated document'}
 $script:featurePixelOrigin=@([int]($featureCanvasBounds.Left+($camera.translation[0]+$tab.width*$camera.zoom/2)*$featureCanvasBounds.Width/$camera.viewport[0]),[int]($featureCanvasBounds.Top+($camera.translation[1]+$tab.height*$camera.zoom/2)*$featureCanvasBounds.Height/$camera.viewport[1]))
 if(!$featureCanvasBounds.Contains([Windows.Rect]::new($featurePixelOrigin[0]-48,$featurePixelOrigin[1]-48,96,96))){throw 'Original96px feature pixel region leaves the canvas'}
 Feature-Park;$script:featureBeforePixels=Wait-StablePixels {Feature-CanvasPixels}
}
function Check-FeatureEvidence{
 $v=Model
 if(($v.state.document_file|ConvertTo-Json -Depth 20 -Compress)-cne$featureDocument-or$v.windows_gpu_generation-ne$featureGpu){throw 'Feature settings changed authored history or GPU owner'}
 Feature-Park;$after=Wait-StablePixels {Feature-CanvasPixels};if($after-cne$featureBeforePixels){throw 'Feature settings changed original96px authored canvas pixels'}
}
function Check-Smoothing{
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 $null=Reveal-Number 'smoothing_strength' 36 $scale
 $field=Field 'smoothing_strength';$identity=$field.GetRuntimeId()-join':'
 $slider=Control 'tool-setting-smoothing_strength-slider' ([System.Windows.Automation.ControlType]::Slider) -Id
 Focus $slider;$range=$slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern)
 if($range.Current.Minimum-ne0-or$range.Current.Maximum-ne1){throw 'Smoothing slider does not expose shared normalized travel'}
 $range.SetValue(.75);Wait-Until {[Math]::Abs((Value 'smoothing_strength')-.3)-lt.0001} 'Smoothing75percent travel did not produce30'
 if(((Field 'smoothing_strength').GetRuntimeId()-join':')-cne$identity-or!$slider.Current.HasKeyboardFocus){throw 'Smoothing value update replaced its editor or stole slider focus'}
 Draft 'smoothing_strength' '30';[CapyRowPointer]::Key([uint32]$ProcessId,13)
 Wait-Until {(Field 'smoothing_strength').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value-eq'30'} 'Smoothing typed30 lost its unitless formatting'
 Draft 'smoothing_strength' 'invalid';[CapyRowPointer]::Chord([uint32]$ProcessId,[uint16[]]@(17),65)
 $edit=Field 'smoothing_strength';$selection=@($edit.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern).GetSelection()|ForEach-Object {$_.GetText(-1)})-join'|'
 if($selection-cne'invalid'-or!$edit.Current.HasKeyboardFocus){throw 'Smoothing draft selection or focus lost'}
 [CapyRowPointer]::Key([uint32]$ProcessId,27);Wait-Until {(Field 'smoothing_strength').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value-eq'30'} 'Smoothing Escape did not restore30'
 $smoothing=@((Model).state.tool_settings|Where-Object id -eq 'smoothing_strength')
 if($smoothing.Count-ne1-or$smoothing[0].label-ne'Smoothing'-or$smoothing[0].group_id-or$smoothing[0].group){throw 'Smoothing must have one ungrouped control'}
 Select-Tool 'pencil';Wait-Until {(Model).brush_ready-and[math]::Abs((Value 'smoothing_strength')-.06)-lt.0001} 'Pencil smoothing default did not prepare' 8
 Select-Tool 'pen';Wait-Until {(Model).brush_ready-and[math]::Abs((Value 'smoothing_strength')-.3)-lt.0001} 'PerPen smoothing choices did not restore' 8
 Dismiss-FeatureDrawer;$null=Reveal-Number 'smoothing_strength' 36 $scale
 $caption=Arranged 'number-caption-tool-setting-smoothing_strength' ([System.Windows.Automation.ControlType]::Text);$bounds=$caption.Current.BoundingRectangle
 $null=[CapyRowPointer]::DoubleClick([int]($bounds.Left+$bounds.Width/2),[int]($bounds.Top+$bounds.Height/2))
 Wait-Until {[math]::Abs((Value 'smoothing_strength')-.06)-lt.0001} 'Native smoothing caption reset did not restore6'
 Select-Tool 'auto_select';Wait-Until {!@((Model).state.tool_settings|Where-Object id -eq 'smoothing_strength').Count} 'Selection retained smoothing controls' 8
 if(Find 'tool-setting-smoothing_strength' ([System.Windows.Automation.ControlType]::Edit) -Id){throw 'Selection retained native smoothing editor'}
 Select-Tool 'pen';Dismiss-FeatureDrawer;Feature-Command 'quick_mask'
 Wait-Until {!@((Model).state.tool_settings|Where-Object id -eq 'smoothing_strength').Count} 'Coverage mask retained smoothing' 8
 Feature-Command 'return_to_artwork';Feature-Command 'clone'
 Wait-Until {(Model).brush_ready-and@((Model).state.tool_settings|Where-Object id -eq 'smoothing_strength').Count-eq1} 'Clone smoothing did not prepare' 8
 Check-Projection;Select-Tool 'pen';Dismiss-FeatureDrawer;Check-Projection
}
function Check-SizeKeys{
 $original=Value 'size';Draft 'size' '13';[CapyRowPointer]::Key([uint32]$ProcessId,13);Wait-Until {(Value 'size')-eq13} 'Size13 did not commit'
 $downs=0;$ups=0
 foreach($step in @(@{key=221;values=@(15,17,20)},@{key=219;values=@(17,15,12,10)})){
  $held=$false
  try{foreach($expected in $step.values){
   Focus (Control 'drawing-canvas' $null -Id);[CapyRowPointer]::Hold([uint16]$step.key,$true);$held=$true;$downs++
   Wait-Until {(Value 'size')-eq$expected} 'Native size key did not follow shared preset sequence'
  }}finally{if($held){[CapyRowPointer]::Hold([uint16]$step.key,$false);$ups++}}
 }
 Draft 'size' '1100';[CapyRowPointer]::Key([uint32]$ProcessId,13);Focus (Control 'drawing-canvas' $null -Id)
 [CapyRowPointer]::Key([uint32]$ProcessId,221);Wait-Until {(Value 'size')-eq1200} 'Large size increase did not choose1200'
 [CapyRowPointer]::Key([uint32]$ProcessId,219);Wait-Until {(Value 'size')-eq1000} 'Large size decrease did not choose1000'
 Draft 'size' ([string]$original);[CapyRowPointer]::Key([uint32]$ProcessId,13);Wait-Until {(Value 'size')-eq$original} 'Size journey did not restore original parameter'
 @{accepted_key_down_calls=$downs;accepted_key_up_calls=$ups;scope='Synthetic native repeated key-downs; no physical autorepeat or latency qualification'}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $OutputDirectory 'size-key-repeat-counts.json')
}
function Preview-Bounds([string]$Id){
 $node=Control $Id;$walker=[System.Windows.Automation.TreeWalker]::RawViewWalker;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 for($hop=0;$hop-lt16-and$node;$hop++){$current=$node.Current;$box=$current.BoundingRectangle;if($current.ProcessId-ne$ProcessId){throw 'Preview ancestor owner mismatch'};if(!$current.IsOffscreen-and!$box.IsEmpty-and$box.Width/$scale-ge160-and$box.Width/$scale-le300-and[math]::Abs($box.Width-$box.Height)-le1){return $box};$node=$walker.GetParent($node)}
 throw 'Actual square preview popup ancestor unavailable'
}
function Preview-Pixels{if([CapyRowPointer]::GetForegroundWindow()-ne$drawingWindow){throw 'Preview does not own foreground'};$box=Preview-Bounds $previewId;if($box-ne$previewBounds){throw 'Preview geometry changed during pixel comparison'};Screen-Pixels $previewPixelOrigin[0] $previewPixelOrigin[1]}
function Preview-Tap([string]$Device,[double]$Position){$at=Along (Box $strengthTrack) $Position;Press $Device $at;Release}
function Check-SizeControls{
 Switch-Workspace 'Sketch' 'builtin:workspace:painter'
 foreach($tool in @('pencil','eraser')){
  Feature-Command $tool;Wait-Until {$v=Model;$v.brush_ready-and@($v.state.commands|Where-Object {$_.id-eq$tool-and$_.selected}).Count-eq1} 'Sketch tool did not prepare' 8
  $tile=Tile 'brush_size_slider';if(!$tile-or!$tile.component.numeric){throw 'Shared Sketch size slider unavailable'}
  $track='component-slider-'+$tile.id;$popup='slider-bookmark-'+$tile.id
  Begin-FeatureEvidence
  foreach($device in @('mouse','pen','touch')){
   Press $device (Along (Box $track) .25);Release
   Wait-Until {(Find $popup -Visible)-and(Model).brush_ready} 'Sketch size click did not open its prepared preview' 8
   $low=(Tile 'brush_size_slider').component.numeric.value
   $identity=(Control $track).GetRuntimeId()-join':'
   Press $device (Along (Box $track) .75);Release
   Wait-Until {(Tile 'brush_size_slider').component.numeric.value-gt$low-and(Model).brush_ready} 'Sketch size increase did not prepare' 8
   $high=(Tile 'brush_size_slider').component.numeric.value
   if((Model).state.brush.diameter-ne$high-or((Control $track).GetRuntimeId()-join':')-cne$identity){throw 'Sketch resize lost shared size or retained slider identity'}
   $from=Along (Box $track) .75;$to=Along (Box $track) .35
   Press $device $from;Drag $from $to;Release
   Wait-Until {(Tile 'brush_size_slider').component.numeric.value-lt$high-and(Model).brush_ready} 'Sketch size drag did not update' 8
   [CapyRowPointer]::Key([uint32]$ProcessId,27);Wait-Until {!(Find $popup -Visible)} 'Sketch size preview did not close' 8
   if((Model).state.host_error-or(Model).error){throw 'Sketch resize reported an error'}
  }
  Check-FeatureEvidence
 }
}
function Check-Preview{
 Switch-Workspace 'Sketch' 'builtin:workspace:painter'
 $strength=Tile 'brush_strength_slider';if(!$strength-or!$strength.component){throw 'Shared Strength slider unavailable'}
 $script:strengthTrack='component-slider-'+$strength.id;$script:previewId='slider-bookmark-'+$strength.id
 Begin-FeatureEvidence
 foreach($device in @('mouse','pen','touch')){
  Preview-Tap $device .85;Wait-Until {(Tile 'brush_strength_slider').component.numeric.value-gt.8-and(Find $previewId -Visible)} 'High Strength preview did not open' 8
  $bookmark=Control $previewId;$runtime=$bookmark.GetRuntimeId()-join':';$script:previewBounds=Preview-Bounds $previewId
  $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.;$header=$bookmark.Current.BoundingRectangle.Height
  $script:previewPixelOrigin=@([int]($previewBounds.Left+$previewBounds.Width/2),[int]($previewBounds.Top+($previewBounds.Height+$header)/2))
  if(!$previewBounds.Contains([Windows.Rect]::new($previewPixelOrigin[0]-48,$previewPixelOrigin[1]-48,96,96))){throw 'Preview96px region leaves actual popup'}
  $high=Wait-StablePixels {Preview-Pixels}
  Preview-Tap $device .25;Wait-Until {(Tile 'brush_strength_slider').component.numeric.value-lt.45} 'Low Strength input did not update shared value' 8
  $different=@{hash=$null};Wait-Until {$different.hash=Preview-Pixels;$different.hash-cne$high} 'Canonical GPU preview pixels did not change with Strength' 8
  $low=Wait-StablePixels {Preview-Pixels};if($low-ceq$high-or((Control $previewId).GetRuntimeId()-join':')-cne$runtime){throw 'Live Strength preview lost pixel difference or retained popup identity'}
  Preview-Tap $device .85;Wait-Until {(Tile 'brush_strength_slider').component.numeric.value-gt.8-and(Preview-Pixels)-ceq$high} 'Final high Strength canonical pixels did not restore' 8
  if((Wait-StablePixels {Preview-Pixels})-cne$high){throw 'Idle preview pixels did not remain exact'}
  [CapyRowPointer]::Key([uint32]$ProcessId,27);Wait-Until {!(Find $previewId -Visible)} 'Preview Escape did not close' 8
  Preview-Tap $device .85;Wait-Until {(Find $previewId -Visible)-and(Preview-Pixels)-ceq$high} 'Reopened preview did not replace stale mask with exact high result' 8
  [CapyRowPointer]::Key([uint32]$ProcessId,27);Wait-Until {!(Find $previewId -Visible)} 'Reopened preview did not close' 8
 }
 Check-FeatureEvidence
}
try{
 Wait-Until {$v=Model;$v.brush_ready-and$v.windows_workspace.ready-and!$v.windows_workspace.busy} 'Original feature startup did not complete' 45
 if(!(Model).windows_isolated_settings-or(Model).state.theme-ne$Theme){throw 'Feature theme/profile mismatch'}
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 if(![CapyRowPointer]::SetForegroundWindow($drawingWindow)){throw 'Feature foreground unavailable'}
 [CapyRowPointer]::Initialize([uint32]$ProcessId)
 if($Journey-eq'size-controls'){Check-SizeControls}elseif($Journey-eq'preview'){Check-Preview}else{Select-Tool 'pen';Wait-Until {(Model).brush_ready} 'Original Pen did not prepare' 8;Dismiss-FeatureDrawer;Begin-FeatureEvidence;if($Journey-eq'smoothing'){Check-Smoothing}else{Check-SizeKeys};Check-FeatureEvidence}
 [CapyRowPointer]::Verify()
 @{journey=$Journey;theme=$Theme;status='passed';authored_document='exact';canvas_pixels='original96px exact';original_budget='600 outer/45 startup/8 controls/20 scrollsteps/Close5 by entry';preview_idle_gpu_submission='not observed; requires renderer packet/metric qualification';physical_pressure_tilt='unverified';canonical_credit=$false}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $OutputDirectory 'feature-proof.json')
}finally{[CapyRowPointer]::Dispose()}
