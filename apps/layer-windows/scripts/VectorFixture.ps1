function Key([uint16]$Code){[CapyRowPointer]::Key([uint32]$review.Id,$Code)}
function Snapshot{$hit=@{model=$null};Wait-Until {$hit.model=Model;$null -ne $hit.model} 'The UI state is unavailable';$hit.model}
function Rows{@((Snapshot).state.layers)}
function Active{@((Snapshot).state.layers|Where-Object selected)[0]}
function Vector($Row){[bool]($Row -and $Row.content_icon -eq 'layer-path-symbolic')}
function Vectors{@((Snapshot).state.layers|Where-Object content_icon -eq 'layer-path-symbolic')}
function Position([long]$Id){$rows=Rows;for($i=0;$i -lt $rows.Count;$i++){if($rows[$i].id -eq $Id){return $i}};-1}
function Revision{(Snapshot).state.document_file.revision}
function Label([string]$Id){@((Snapshot).state.commands|Where-Object id -eq $Id)[0].label}
function Selected([string]$Id){[bool]@((Snapshot).state.commands|Where-Object id -eq $Id)[0].selected}
function Brush-Ready([long]$Preset){$m=Snapshot;$m.state.brush.preset -eq $Preset -and $m.brush_ready}
function Center($Element){$r=$Element.Current.BoundingRectangle;@([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}
function Click($Element){
 $at=Center $Element
 [CapyRowPointer]::Down('mouse',$at[0],$at[1]);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 120
}
function Park{
 $button=Find 'settings-button' -Visible
 $at=if($button){$b=$button.Current.BoundingRectangle;@(([int]$b.X+2),([int]$b.Y+2))}else{$b=$root.Current.BoundingRectangle;@(([int]$b.X+16),([int]$b.Y+16))}
 [CapyRowPointer]::Hover($at[0],$at[1]);Start-Sleep -Milliseconds 300
}
function Grab{
 $b=$root.Current.BoundingRectangle;$x=[Math]::Max(0,[int]$b.X);$y=[Math]::Max(0,[int]$b.Y)
 $bitmap=[Drawing.Bitmap]::new([int]($b.Right-$x),[int]($b.Bottom-$y));$graphics=[Drawing.Graphics]::FromImage($bitmap)
 try{$graphics.CopyFromScreen($x,$y,0,0,$bitmap.Size)}finally{$graphics.Dispose()}
 $bitmap
}
function Save-Grab($Bitmap,[string]$Name){try{$Bitmap.Save((Join-Path $run ($Name+'.png')),[Drawing.Imaging.ImageFormat]::Png)}finally{$Bitmap.Dispose()}}
function Camera-Key($Camera){@($Camera.viewport,$Camera.translation,$Camera.zoom,$Camera.rotation,$Camera.flipped)|ConvertTo-Json -Depth 5 -Compress}
function Take-Projection{
 $seen=@{key=$null;stable=0}
 Wait-Until {$next=Camera-Key (Model).state.camera;if($next -eq $seen.key){$seen.stable++}else{$seen.key=$next;$seen.stable=0};$seen.stable -ge 4} 'The fitted camera did not settle'
 $camera=(Model).state.camera;$bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 if($camera.rotation -ne 0 -or $camera.flipped[0] -or $camera.flipped[1]){throw 'The journey requires an unrotated canvas'}
 $script:projection=@{camera=$camera;bounds=$bounds;key=(Camera-Key $camera)}
 $projection|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $run 'projection.json')
}
function Check-Projection{
 if((Camera-Key (Model).state.camera) -ne $projection.key -or (Control 'drawing-canvas' -Arranged).Current.BoundingRectangle -ne $projection.bounds){throw 'The canvas or camera moved during the journey'}
}
function Screen-Point($At){
 $c=$projection.camera;$b=$projection.bounds;$w=$c.work_area
 $vx=$At[0]*$c.zoom+$c.translation[0];$vy=$At[1]*$c.zoom+$c.translation[1]
 if($w -and ($vx -lt $w[0] -or $vy -lt $w[1] -or $vx -gt $w[0]+$w[2] -or $vy -gt $w[1]+$w[3])){throw "Document point $($At -join ',') lies outside the canvas work area"}
 @([int][Math]::Round($b.X+$vx*$b.Width/$c.viewport[0]),[int][Math]::Round($b.Y+$vy*$b.Height/$c.viewport[1]))
}
function Patch($At,[int]$Radius=4){
 $p=Screen-Point $At;$size=2*$Radius+1
 $bitmap=[Drawing.Bitmap]::new($size,$size);$graphics=[Drawing.Graphics]::FromImage($bitmap)
 try{
  $graphics.CopyFromScreen($p[0]-$Radius,$p[1]-$Radius,0,0,$bitmap.Size)
  $level=256;$darkest=$null;$sum=@(0,0,0)
  for($y=0;$y -lt $size;$y++){for($x=0;$x -lt $size;$x++){
   $c=$bitmap.GetPixel($x,$y);$top=[Math]::Max([int]$c.R,[Math]::Max([int]$c.G,[int]$c.B))
   if($top -lt $level){$level=$top;$darkest=@([int]$c.R,[int]$c.G,[int]$c.B)}
   $sum[0]+=$c.R;$sum[1]+=$c.G;$sum[2]+=$c.B
  }}
  [pscustomobject]@{screen=$p;darkest=$darkest;level=$level;mean=@($sum|ForEach-Object {[Math]::Round($_/($size*$size),1)})}
 }finally{$graphics.Dispose();$bitmap.Dispose()}
}
function Classified($Sample,[string]$Kind){
 switch($Kind){
  'ink'{$Sample.level -lt 110}
  'mark'{$Sample.darkest[0]+$Sample.darkest[1]+$Sample.darkest[2] -lt 450}
  'green'{$Sample.mean[1]-[Math]::Max($Sample.mean[0],$Sample.mean[2]) -gt 40}
 }
}
function Expect([string]$Check,$At,[ValidateSet('ink','mark','green')][string]$Kind,[bool]$Wanted=$true,[int]$Limit=30,[switch]$Deferred){
 Check-Projection
 $seen=@{sample=$null}
 try{Wait-Until {$seen.sample=Patch $At;[bool](Classified $seen.sample $Kind) -eq $Wanted} $Check $Limit}
 catch{
  $failure="$Check at $($At -join ','): wanted $Kind=$Wanted, darkest $($seen.sample.darkest -join ','), mean $($seen.sample.mean -join ',')"
  if(!$Deferred){throw "$failure; $($_.Exception.Message)"}
  $findings.Add($failure)
 }
 $observations.Add([ordered]@{check=$Check;kind=$Kind;wanted=$Wanted;at=$At;screen=$seen.sample.screen;darkest=$seen.sample.darkest;mean=$seen.sample.mean})
}
function Line($From,$To,[int]$Steps=20){@(for($i=0;$i -le $Steps;$i++){,@(($From[0]+($To[0]-$From[0])*$i/$Steps),($From[1]+($To[1]-$From[1])*$i/$Steps))})}
function Stroke($Points,[switch]$Refused){
 Check-Projection
 $screen=@(foreach($point in $Points){,(Screen-Point $point)})
 if(![CapyRowPointer]::SetForegroundWindow($drawingWindow)){throw 'The owned drawing did not accept foreground input'}
 $script:contact=[Diagnostics.Stopwatch]::StartNew()
 [CapyRowPointer]::Down('pen',$screen[0][0],$screen[0][1])
 try{
  for($i=1;$i -lt $screen.Count;$i++){Start-Sleep -Milliseconds 16;[CapyRowPointer]::Move($screen[$i][0],$screen[$i][1])}
  Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up()
 }finally{[CapyRowPointer]::Cancel()}
 if(!$Refused){Park}
}
function Search([string]$Query,[scriptblock]$Pick,[string]$Wanted){
 if(![CapyRowPointer]::SetForegroundWindow($drawingWindow)){throw 'The owned drawing did not accept foreground input'}
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x4B)
 Wait-Until {Find 'command-search' -Visible} 'Command search did not open'
 $entry=Control 'command-search' -Arranged
 Wait-Until {$entry.Current.HasKeyboardFocus} 'Command search did not take keyboard focus' 5
 $admission=@{native_value=$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value;native_focus=$entry.Current.HasKeyboardFocus;shared_query=(Search-State).query}
 $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Query)
 $hit=@{index=-1;result=$null}
 try{Wait-Until {
  $view=(Search-State)
  if(!$view -or $view.query -ne $Query){return $false}
  $results=@($view.results)
  for($i=0;$i -lt $results.Count;$i++){if(& $Pick $results[$i]){$hit.index=$i;$hit.result=$results[$i];return $true}}
  $false
 } "Command search did not offer $Wanted"}catch{
  @{expected=$Query;before=$admission;native_value=$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value;native_focus=$entry.Current.HasKeyboardFocus;shared_query=(Search-State).query}|ConvertTo-Json|Set-Content (Join-Path $run 'command-search-failure.json')
  throw
 }
 Wait-Until {(Control ('command-result-'+$hit.index) -Arranged).Current.Name -eq $hit.result.label} "Native command search did not present '$($hit.result.label)'"
 $row=Control ('command-result-'+$hit.index) -Arranged
 $row.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {!(Find 'command-search' -Visible) -and !(Search-State)} 'Command search did not close'
 $hit.result
}
function Select-Brush([string]$Brush){
 $pick={param($result) $result.label -in @($Brush,"$Brush brush") -and $result.id -like 'action:*"select_brush"*'}.GetNewClosure()
 $result=Search $Brush $pick "the $Brush brush"
 $preset=[long](($result.id -replace '^action:')|ConvertFrom-Json).id
 Wait-Until {Brush-Ready $preset} "$Brush did not become the ready brush" 120
 $preset
}
function Select-Tool([string]$Id){
 if(!(Selected $Id)){
  Invoke (Tool-Tile $Id)
  Wait-Until {(Model).canvas_ready -and (Selected $Id)} "$Id did not become the tool"
 }
 if((Model).state.customization.drawer){
  (Control 'drawing-canvas').SetFocus();Key 0x1B
  Wait-Until {!(Model).state.customization.drawer} 'The tool drawer did not close'
 }
 Wait-Until {(Model).brush_ready} "$Id did not become ready" 120
 Park
}
function Tool-Item([string]$List,[scriptblock]$Match,[string]$Wanted){
 $hit=@{index=-1;item=$null}
 Wait-Until {
  $items=@((Model).state.tool_set.$List)
  for($i=0;$i -lt $items.Count;$i++){if(& $Match $items[$i]){$hit.index=$i;$hit.item=$items[$i];return $true}}
  $false
 } "The Tools list did not offer $Wanted"
 $id=$(if($List -eq 'groups'){'tool-group-'}else{'tool-subtool-'})+$hit.index
 $native=@{name=$null}
 try{Wait-Until {$button=Find $id;$native.name=if($button){$button.Current.Name};$native.name -eq $hit.item.label} "The Tools list button $id is not labelled $($hit.item.label)"}
 catch{throw "The Tools list button $id is labelled '$($native.name)' where Rust offers '$($hit.item.label)'"}
 [pscustomobject]@{id=$id;list=$List;index=$hit.index;label=$hit.item.label;selected=[bool]$hit.item.selected;enabled=[bool]$hit.item.enabled;reason=[string]$hit.item.disabled_reason;action=$hit.item.action}
}
function Reason-Hover($Button,[string]$Reason){
  if(!$Reason -or $Button.Current.HelpText-cne$Reason){throw 'The native hover target has no current shared reason'}
  $at=Center $Button;[CapyRowPointer]::Hover($at[0],$at[1]);$tip=@{node=$null}
  $hoverCondition=[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.Condition[]]@([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ToolTip),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::IsOffscreenProperty,$false)))
  Wait-Until {$tip.node=[System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$hoverCondition);$tip.node -and $tip.node.Current.Name -ceq $Reason} 'The tool hover did not expose its shared reason'
  Start-Sleep -Milliseconds 200
  if($tip.node.Current.Name -cne $Reason -or $Button.Current.HelpText -cne $Reason){throw 'A delayed shortcut reply replaced the current tool reason'}
  Park
}
function Tool-Availability($Choice,[switch]$Hover){
 $button=Reveal $Choice.id;$spec=@((Snapshot).state.tool_set.($Choice.list))[$Choice.index]
 if($button.Current.Name -cne $spec.label -or $button.Current.IsEnabled -ne [bool]$spec.enabled -or $button.Current.HelpText -cne [string]$spec.disabled_reason){throw 'The native tool availability differs from the shared choice'}
 if($Hover){if(!$spec.enabled){throw 'The reason-bearing tool choice is not still enabled'};Reason-Hover $button $spec.disabled_reason}
 $button
}
function Tool-TextPixels($Choice,[string]$Name){
 $button=Tool-Availability $Choice;Park
 $text=Control $Choice.label -Name -Within $button -Type ([System.Windows.Automation.ControlType]::Text) -Arranged
 $box=$button.Current.BoundingRectangle;$glyph=$text.Current.BoundingRectangle;$window=$root.Current.BoundingRectangle
 if(!$box.Contains($glyph)){throw 'The tool text leaves its owned button'}
 $bitmap=Grab;$origin=@([Math]::Max(0,[int]$window.X),[Math]::Max(0,[int]$window.Y));$contrast=0
 try{
  $background=$bitmap.GetPixel([int]($box.Right-2-$origin[0]),[int]($box.Y+$box.Height/2-$origin[1]))
  for($y=[int][Math]::Ceiling($glyph.Top);$y-lt[Math]::Floor($glyph.Bottom);$y++){for($x=[int][Math]::Ceiling($glyph.Left);$x-lt[Math]::Floor($glyph.Right);$x++){
   $pixel=$bitmap.GetPixel($x-$origin[0],$y-$origin[1]);$contrast=[Math]::Max($contrast,([Math]::Abs([int]$pixel.R-$background.R)+[Math]::Abs([int]$pixel.G-$background.G)+[Math]::Abs([int]$pixel.B-$background.B)))
  }}
  $bitmap.Save((Join-Path $run ($Name+'.png')),[Drawing.Imaging.ImageFormat]::Png)
 }finally{$bitmap.Dispose()}
 [pscustomobject]@{identity=($button.GetRuntimeId()-join':');bounds=@($box.X,$box.Y,$box.Width,$box.Height);background=$background.ToArgb();contrast=$contrast;reason=$button.Current.HelpText;enabled=$button.Current.IsEnabled}
}
function Choose-Tool-Item($Choice,[switch]$Acknowledged){
 if($Choice.selected){return $true}
 Click (Reveal $Choice.id);Park
 $ack=@{native=$false;shared=$false}
 try{Wait-Until {$button=Find $Choice.id;$ack.native=[bool]($button -and $button.Current.ItemStatus);$ack.shared=[bool]@((Model).state.tool_set.($Choice.list))[$Choice.index].selected;$ack.native -or $ack.shared} "$($Choice.label) did not become selected"}
 catch{if($Acknowledged){throw}}
 $ack.native -or $ack.shared
}
function Set-Color([string]$Hex){
 Invoke-Id 'color-edit'
 Wait-Until {Find 'edit-color-apply'} 'Edit Color did not open'
 Invoke 'edit-color-hex';$entry=Control 'edit-color-hex-entry';$entry.SetFocus()
 $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Hex);Key 0x0D
 Invoke 'edit-color-apply';Wait-Until {!(Find 'edit-color-apply')} 'Edit Color did not close'
 $rgb=@(1,3,5|ForEach-Object {[Convert]::ToInt32($Hex.Substring($_,2),16)/255})
 Wait-Until {$c=(Model).state.brush.color;$c -and [Math]::Abs($c[0]-$rgb[0]) -lt .01 -and [Math]::Abs($c[1]-$rgb[1]) -lt .01 -and [Math]::Abs($c[2]-$rgb[2]) -lt .01} "The current color did not become $Hex"
}
function New-VectorLayer{
 $before=@(Rows).Count;$button=Control 'layer-new-vector' -Arranged
 if(!$button.Current.IsEnabled){throw 'New vector layer is disabled'}
 Click $button;Park
 Wait-Until {@(Rows).Count -eq $before+1 -and (Vector (Active))} 'The New vector layer button did not add an active vector layer'
 $id=(Active).id
 $null=Control "layer-$id-name"
 $id
}
function Footer-Check([string]$Name){
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96
 $frame=(Control 'layer-footer' -Arranged).Current.BoundingRectangle;$inside=$frame;$inside.Inflate(1,1)
 $allocated=Native-Bounds 'layer-footer' $scale;$width=$allocated.Width/$scale
 $ids=[ordered]@{new_layer='layer-new';new_vector_layer='layer-new-vector';new_group='layer-new-group';new_selection_layer='layer-new-selection';add_mask='layer-add-mask';add_filter='layer-add-filter';import_image='layer-import';delete='layer-delete'}
 $slots=@((Snapshot).state.layer_tools.footer)
 if(($slots.button -join ',') -ne ($ids.Keys -join ',')){throw 'The shared Layers footer slots differ from the native fixture controls'}
 $hidden=@($slots|Where-Object {$width+.5 -lt $_.min_width})
 $record=[ordered]@{scale=$scale;allocated_width=$width;footer=@($frame.X,$frame.Y,$frame.Width,$frame.Height);allocated=@($allocated.X,$allocated.Y,$allocated.Width,$allocated.Height);buttons=@();overflow=@()}
 $previous=$null
 foreach($slot in @($slots)+@([pscustomobject]@{button='actions';min_width=0})){
  $id=if($slot.button -eq 'actions'){'layer-actions'}else{$ids[$slot.button]}
  $expected=$width+.5 -ge $slot.min_width
  $button=Find $id -Visible
  if([bool]$button -ne $expected){$findings.Add("The Layers footer shows $id=$([bool]$button), expected $expected at $width DIP with minimum $($slot.min_width)")}
  if(!$button){$record.buttons+=[ordered]@{id=$id;shown=$false;expected=$expected;min_width=$slot.min_width};continue}
  $b=$button.Current.BoundingRectangle
  $record.buttons+=[ordered]@{id=$id;shown=$true;expected=$expected;min_width=$slot.min_width;name=$button.Current.Name;enabled=$button.Current.IsEnabled;bounds=@($b.X,$b.Y,$b.Width,$b.Height)}
  if(!$button.Current.Name){$findings.Add("The Layers footer button $id has no accessible name")}
  if(!$inside.Contains($b) -or [Math]::Abs($b.Width-26*$scale) -gt 2 -or [Math]::Abs($b.Height-26*$scale) -gt 2){
   $findings.Add("The Layers footer clips $id at $([Math]::Round($b.X)),$([Math]::Round($b.Y)) $([Math]::Round($b.Width))x$([Math]::Round($b.Height)) within $([Math]::Round($frame.X)),$([Math]::Round($frame.Y)) $([Math]::Round($frame.Width))x$([Math]::Round($frame.Height))")
  }
  if($previous -and $b.Left -lt $previous.Right-1){$findings.Add("The Layers footer overlaps $id with the button before it")}
  $previous=$b
 }
 try{
  if($hidden.Count){
   $before=(Snapshot).state.document_file|Select-Object epoch,revision,modified|ConvertTo-Json -Compress
   $selected=@((Rows|Where-Object selected).id) -join ','
   $peers=[Collections.Generic.List[object]]::new();$popups=$script:CapyPopups
   try{
    $script:CapyPopups=$true;Invoke 'layer-actions'
    foreach($slot in $hidden){
     $hit=@{item=$null}
     Wait-Until {$hit.item=Find $slot.item.label -Name -Visible -Type ([System.Windows.Automation.ControlType]::MenuItem);$null -ne $hit.item} "The Layers overflow does not show $($slot.item.label)"
     $item=$hit.item;$peers.Add($item);$b=$item.Current.BoundingRectangle
     $record.overflow+=[ordered]@{button=$slot.button;name=$item.Current.Name;expected_label=$slot.item.label;enabled=$item.Current.IsEnabled;expected_enabled=[bool]$slot.item.enabled;bounds=@($b.X,$b.Y,$b.Width,$b.Height)}
     if($item.Current.Name -ne $slot.item.label -or $item.Current.IsEnabled -ne [bool]$slot.item.enabled){throw "The Layers overflow mislabels or mis-enables $($slot.button)"}
     if($b.IsEmpty -or $b.Width -le 0 -or $b.Height -le 0){throw "The Layers overflow did not arrange $($slot.item.label)"}
    }
    Capture "footer-overflow-$Name-$Theme" -WithModel -Composed
   }finally{
    try{
     Key 0x1B
     foreach($peer in $peers){Wait-Until {try{$bounds=$peer.Current.BoundingRectangle;$peer.Current.IsOffscreen -or $bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0}catch [System.Windows.Automation.ElementNotAvailableException]{$true}} 'The Layers overflow did not close'}
    }finally{$script:CapyPopups=$popups}
   }
   Park
   $after=(Snapshot).state.document_file|Select-Object epoch,revision,modified|ConvertTo-Json -Compress
   if($after -ne $before -or (@((Rows|Where-Object selected).id) -join ',') -ne $selected){throw 'Inspecting the Layers overflow changed the drawing or selected layers'}
  }
 }finally{$record|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $run "layers-footer-$Name.json")}
}
function Notice-Parts{
 if(!$script:noticeFrame){
  $script:noticeFrame=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'canvas-notice'))
  if(!$script:noticeFrame){return [ordered]@{}}
 }
 $request=[System.Windows.Automation.CacheRequest]::new()
 foreach($property in [System.Windows.Automation.AutomationElement]::AutomationIdProperty,[System.Windows.Automation.AutomationElement]::NameProperty,[System.Windows.Automation.AutomationElement]::BoundingRectangleProperty,[System.Windows.Automation.AutomationElement]::IsOffscreenProperty,[System.Windows.Automation.AutomationElement]::IsKeyboardFocusableProperty,[System.Windows.Automation.AutomationElement]::IsEnabledProperty,[System.Windows.Automation.AutomationElement]::HelpTextProperty){$request.Add($property)}
 $active=$request.Activate()
 try{$items=$script:noticeFrame.FindAll([System.Windows.Automation.TreeScope]::Subtree,[System.Windows.Automation.Condition]::TrueCondition)}finally{$active.Dispose()}
 $parts=[ordered]@{}
 foreach($item in $items){$c=$item.Cached;if($c.AutomationId -like 'canvas-notice*' -and !$c.IsOffscreen){$parts[$c.AutomationId]=[pscustomobject]@{name=$c.Name;bounds=$c.BoundingRectangle;focusable=$c.IsKeyboardFocusable;enabled=$c.IsEnabled;reason=$c.HelpText}}}
 $parts
}
function Notice-Presentation($Parts,$View,$Bitmap){
 $notice=$View.state.notice;$offers=@($notice.actions);$window=$root.Current.BoundingRectangle;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 if(!$offers.Count -or !$offers[0].accent -or @($offers|Where-Object accent).Count-ne1){throw 'The shared notice does not identify exactly its first suggested action'}
 $record=@(foreach($offer in $offers){
  $native=$Parts['canvas-notice-action-'+$offer.id]
  if(!$native -or $native.name-cne$offer.label -or $native.enabled-ne[bool]$offer.enabled -or $native.reason-cne[string]$offer.reason){throw 'The native notice action differs from its shared label, availability or reason'}
  $role=if($offer.accent -and $offer.enabled){'accent'}else{'panel'};$expected=[Drawing.ColorTranslator]::FromHtml($View.state.palette.$role)
  $at=@([int]($native.bounds.Left+6*$scale),[int]($native.bounds.Y+$native.bounds.Height/2))
  $pixel=$Bitmap.GetPixel($at[0]-[Math]::Max(0,[int]$window.X),$at[1]-[Math]::Max(0,[int]$window.Y))
  if($pixel.ToArgb()-ne$expected.ToArgb()){throw "The $($offer.id) notice action does not render its shared $role fill"}
  [ordered]@{id=$offer.id;accent=[bool]$offer.accent;palette_role=$role;point=$at;pixel=$pixel.ToArgb();expected=$expected.ToArgb()}
 })
 $record
}
function Accept-Notice([string]$Action,[string[]]$Offers,[string]$Name,[string]$Label){
 $hit=@{parts=$null;polls=[Collections.Generic.List[long]]::new()}
 $key="canvas-notice-action-$Action"
 try{
  Wait-Until {
   $poll=[Diagnostics.Stopwatch]::StartNew();$parts=Notice-Parts;$hit.polls.Add($poll.ElapsedMilliseconds)
   if($parts.Contains('canvas-notice') -and $parts.Contains($key)){$hit.parts=$parts;$hit.native_ms=$contact.ElapsedMilliseconds}
   $null -ne $hit.parts
  } "No notice offered $Label" 30
 }catch{throw "No notice offered $Label ($($contact.ElapsedMilliseconds) ms after the contact; native frame found: $([bool]$script:noticeFrame); slowest notice query $(($hit.polls|Measure-Object -Maximum).Maximum) ms): $($_.Exception.Message)"}
 $view=Snapshot;$picture=Grab
 $clicked=$contact.ElapsedMilliseconds
 $parts=$hit.parts;$target=$parts[$key].bounds
 [CapyRowPointer]::Down('mouse',[int]($target.X+$target.Width/2),[int]($target.Y+$target.Height/2));Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up()
 try{$styles=Notice-Presentation $hit.parts $view $picture}finally{Save-Grab $picture "$Name-notice-$Theme"}
 Park
 $frame=$parts['canvas-notice'].bounds;$contained=$frame;$contained.Inflate(1,1)
 $actions=@($parts.Keys|Where-Object {$_ -like 'canvas-notice-action-*'})
 $presented=[ordered]@{name=$Name;text=$(if($parts.Contains('canvas-notice-text')){$parts['canvas-notice-text'].name});frame=@($frame.X,$frame.Y,$frame.Width,$frame.Height);native_ms=$hit.native_ms;clicked_ms=$clicked;slowest_query_ms=($hit.polls|Measure-Object -Maximum).Maximum;actions=@();native_styles=$styles}
 foreach($id in $actions){
  $button=$parts[$id]
  $presented.actions+=[ordered]@{id=($id -replace '^canvas-notice-action-');name=$button.name;bounds=@($button.bounds.X,$button.bounds.Y,$button.bounds.Width,$button.bounds.Height)}
  if(!$button.name){$findings.Add("The $Name notice's $id has no label")}
  if(!$contained.Contains($button.bounds)){$findings.Add("The $Name notice clips its $($button.name) button")}
 }
 $presented|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $run "$Name-notice.json")
 if(!$presented.text){$findings.Add("The $Name notice shows no text")}
 $ids=@($presented.actions|ForEach-Object id) -join ','
 if($ids -ne ($Offers -join ',')){throw "The $Name notice offered $ids instead of $($Offers -join ',')"}
 if($parts[$key].name -ne $Label){throw "The $Name notice labels $Action '$($parts[$key].name)' instead of '$Label'"}
 Wait-Until {!(Notice-Parts).Contains('canvas-notice')} "Choosing $($parts[$key].name) did not close the $Name notice"
 $presented
}
function Shortcut-Command([string]$Id){
 $binding=@(@((Snapshot).state.commands|Where-Object id -eq $Id)[0].bindings|Where-Object {$_.key.Length -eq 1})[0]
 if(!$binding){throw "$Id has no single-key shortcut"}
 $modifiers=[uint16[]]@($(if($binding.command){0x11}),$(if($binding.shift){0x10}),$(if($binding.alt){0x12})|Where-Object {$_})
 if(![CapyRowPointer]::SetForegroundWindow($drawingWindow)){throw 'The owned drawing did not accept foreground input'}
 (Control 'drawing-canvas').SetFocus()
 $script:contact=[Diagnostics.Stopwatch]::StartNew()
 [CapyRowPointer]::Chord([uint32]$review.Id,$modifiers,[uint16][char]$binding.key.ToUpperInvariant())
}
