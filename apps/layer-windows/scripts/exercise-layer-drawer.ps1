param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -AssemblyName System.Drawing
$CapyFind='prefer-visible';$CapyPopups=$true
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/layer-drawer/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Press([string]$Id){
 if($Id -like 'header-item-*'){
  $itemId=[long]($Id -replace '^header-item-','');$zone=-1
  for($i=0;$i -lt 3;$i++){if(@((Model).header.model.zones[$i]|Where-Object id -eq $itemId).Count){$zone=$i;break}}
  Wait-Until {(Find $Id -Visible) -or (Find ('header-overflow-'+$zone) -Visible)} "Header item $itemId did not arrange"
  if(!(Find $Id -Visible)){Invoke ('header-overflow-'+$zone);$Id='header-overflow-item-'+$itemId}
  $r=(Control $Id -Arranged).Current.BoundingRectangle
  [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
  [CapyRowPointer]::Down('mouse',[int]($r.X+$r.Width*.5),[int]($r.Y+$r.Height*.5));[CapyRowPointer]::Up()
 }
 elseif($Id -like 'tile-*'){$r=Bounds $Id;[CapyRowPointer]::Down('mouse',[int]($r.X+$r.Width*.5),[int]($r.Y+$r.Height*.5));[CapyRowPointer]::Up()}
 elseif($Id -like '*-swipe-*'){$r=Bounds $Id;[CapyRowPointer]::Down('touch',[int]($r.X+$r.Width*.5),[int]($r.Y+$r.Height*.5));[CapyRowPointer]::Up()}
 else{Invoke $Id}
}
function Bounds([string]$Id){(Control $Id -Arranged).Current.BoundingRectangle}
function Visible-DrawerBounds {
 $frame=Bounds 'tool-drawer';$footer=Bounds 'layer-properties-disclosure'
 [Windows.Rect]::new($frame.X,$frame.Y,$frame.Width,$footer.Bottom-$frame.Top)
}
function Drawer {try{(Control 'tool-drawer').Current.ItemStatus|ConvertFrom-Json}catch{}}
function Layer([long]$Id){@((Model).state.layers|Where-Object id -eq $Id)[0]}
function Dialog-Action([string]$Id,[string]$Name){
 (Control $Name -Name -Within (Control $Id) -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Custom-Toolbar {
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
 (Control 'Quick Access Toolbars' -Name).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 Press 'new_toolbar'
 (Control 'workspace-manager-name').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Combined panels review')
 Dialog-Action 'workspace-manager' 'Add to Workspace'
 $target=@{panel=$null}
 Wait-Until {$target.panel=@((Model).panels|Where-Object title -eq 'Combined panels review')[0];$target.panel} 'Custom toolbar did not enter the workspace'
 $panel=$target.panel.id
 Wait-Until {!(Find 'workspace-manager') -and (Control 'drawing-canvas').Current.IsEnabled} 'Workspace manager did not close before toolbar input'
 Start-Sleep -Milliseconds 250
 (Control ('ribbon-grip-'+$panel) -Arranged).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
 Invoke ('Add Tools'+[char]0x2026) -Name
 foreach($query in @('Layers panel','Brush color','Color panel')){
  (Control 'tool-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($query)
  Wait-Until {(Model).picker.query -eq $query} 'Toolbar picker did not adopt its search'
  $choices=@((Model).picker.choices|Where-Object {$_.control.kind -eq 'color' -or ($_.control.kind -eq 'panel' -and $_.control.panel -in @('layers','color'))})
  foreach($choice in $choices){$control=$choice.control;$suffix=if($control.kind -eq 'color'){'color-0'}else{'panel-'+$control.panel}
   (Control ('picker-choice-'+$suffix)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
  }
 }
 Wait-Until {(Model).picker.selected_count -eq 3} 'Custom toolbar did not select Layers and both Color controls'
 Dialog-Action 'tool-picker' ((Model).picker.confirm_label)
 Wait-Until {@(@((Model).panels|Where-Object id -eq $panel)[0].tiles).Count -eq 3} 'Custom toolbar did not receive three controls'
 foreach($tile in @(@((Model).panels|Where-Object id -eq $panel)[0].tiles)){
  Press ('tile-'+$panel+'-'+$tile.id)
  if($tile.control.panel -eq 'layers'){
   Wait-Until {(Find 'tool-drawer' -Visible) -and (Model).state.customization.drawer.layer_properties -eq $false} 'Custom toolbar Layers did not open the combined inspector'
  }else{
   Wait-Until {(ConvertTo-Json -InputObject (Model).state.customization.drawer.columns -Compress) -eq '[["color","palettes"]]'} 'Custom toolbar Color control did not open Color and Palettes'
  }
  Capture ('custom-toolbar-'+$tile.id+'-'+$Theme) -Composed -WithModel
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  Wait-Until {!(Model).state.customization.drawer} 'Custom toolbar drawer did not close'
 }
}
function Swipe([long]$Id,[int]$Distance){
 $r=Bounds $(if($Distance -lt 0){"layer-$Id-name"}else{"layer-row-$Id"});$x=[int]$(if($Distance -lt 0){$r.X+$r.Width*.5}else{$r.Left+12});$y=[int]($r.Y+$r.Height*.5)
 $row=Bounds "layer-row-$Id"
 [CapyRowPointer]::Down('touch',$x,$y);Start-Sleep -Milliseconds 35
 for($i=1;$i -le 8;$i++){$next=[int][Math]::Min([Math]::Max(($x+[int]($Distance*$i/8)),$row.Left+4),$row.Right-4);[CapyRowPointer]::Move($next,$y);Start-Sleep -Milliseconds 12}
 $list=Control 'layer-list';$beforeUp=$list.Current.ItemStatus
 [CapyRowPointer]::Up()
 Start-Sleep -Milliseconds 250
 $list=Control 'layer-list';@{distance=$Distance;before_up=$beforeUp;status=$list.Current.ItemStatus;lock_visible=[bool](Find "layer-$Id-swipe-lock" -Visible);gesture=$list.Current.HelpText;drawer=(Drawer)}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ('swipe-'+$Id+'-'+$Distance+'.json'))
}
function Test-AddPaper {
 $original=@((Model).state.layers);$paper=@($original|Where-Object content_icon -eq 'layer-paper-symbolic')
 if($paper.Count -ne 1){throw 'The Paper row fixture needs one bottom Paper'}
 Press ("layer-"+$paper[0].id+'-name');Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $paper[0].id} 'Paper did not select'
 Press 'layer-delete'
 Wait-Until {(Model).state.layer_tools.add_paper -and @((Model).state.layers).Count -eq $original.Count-1} 'Deleting Paper did not publish Add paper'
 $row=Control 'layer-add-paper' -Arranged;$box=$row.Current.BoundingRectangle
 $last=@((Model).state.layers)[-1];$above=Bounds ("layer-row-"+$last.id)
 if([Math]::Abs($box.Top-$above.Bottom) -gt 1 -or [Math]::Abs($box.Height-$above.Height) -gt 1){throw 'Add paper did not share the last native layer row geometry'}
 if($row.Current.Name -ne (Model).state.layer_tools.add_paper.label){throw 'Add paper did not present its shared label'}
 Capture ('drawer-add-paper-'+$Theme) -Composed -WithModel
 foreach($device in @('mouse','pen','touch','Space','Enter')){
  $row=Control 'layer-add-paper' -Arranged;$box=$row.Current.BoundingRectangle
  $x=[int]($box.X+$box.Width*.5);$y=[int]($box.Y+$box.Height*.5)
  if($device -in @('mouse','pen','touch')){
   $revision=(Model).state.document_file.revision
   [CapyRowPointer]::Down($device,$x,$y);Start-Sleep -Milliseconds 35
   [CapyRowPointer]::Move($x,[int]($above.Y+$above.Height*.5));[CapyRowPointer]::Up();Start-Sleep -Milliseconds 250
   if((Model).state.document_file.revision -ne $revision -or !(Model).state.layer_tools.add_paper){throw "$device release outside Add paper changed the drawing"}
   [CapyRowPointer]::Down($device,$x,$y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()
  }else{
   Focus-Keyboard $row
   [CapyRowPointer]::Key([uint32]$review.Id,$(if($device -eq 'Space'){0x20}else{0x0D}))
  }
  Wait-Until {!((Model).state.layer_tools.add_paper) -and @((Model).state.layers).Count -eq $original.Count -and @((Model).state.layers)[-1].content_icon -eq 'layer-paper-symbolic'} "$device did not create one bottom Paper"
  if(Find 'layer-add-paper' -Visible){throw 'Add paper stayed visible after Paper was created'}
  Invoke 'Undo' -Name;Wait-Until {(Model).state.layer_tools.add_paper -and @((Model).state.layers).Count -eq $original.Count-1} "$device Paper did not Undo in one step"
  Invoke 'Redo' -Name;Wait-Until {!((Model).state.layer_tools.add_paper) -and @((Model).state.layers).Count -eq $original.Count} "$device Paper did not Redo in one step"
  Invoke 'Undo' -Name;Wait-Until {(Model).state.layer_tools.add_paper -and @((Model).state.layers).Count -eq $original.Count-1} "$device Paper did not return to its absent state"
  if((Model).state.customization.drawer.layer_properties){
   Press 'layer-properties-disclosure'
   Wait-Until {!(Model).state.customization.drawer.layer_properties -and (Control 'layer-add-paper' -Arranged)} 'All layers did not restore the arranged Add paper row'
  }
 }
 Invoke 'Undo' -Name;Wait-Until {@((Model).state.layers).Count -eq $original.Count -and @((Model).state.layers)[-1].id -eq $paper[0].id} 'Paper fixture did not restore its original row'
}
function Check-Footer {
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 $footer=Bounds 'layer-footer';$first=Bounds 'layer-new';$last=Bounds 'layer-actions'
 $drawer=Visible-DrawerBounds
 $panel=Bounds 'drawer-panel-layers'
 if([Math]::Abs($panel.Left-$drawer.Left)/$scale -gt 1 -or [Math]::Abs($drawer.Right-$panel.Right)/$scale -gt 1){throw 'Layers rows do not run edge to edge in the drawer'}
 if([Math]::Abs(($first.Left-$footer.Left)-($footer.Right-$last.Right)) -gt 1 -or [Math]::Abs(($first.Left-$panel.Left)/$scale-6) -gt 1 -or [Math]::Abs(($panel.Right-$last.Right)/$scale-6) -gt 1){throw 'Layers footer actions do not keep their six-point inset and alignment'}
 $row=@((Model).state.layers)[0].id
 $properties=Bounds 'layer-properties-icon';$chevron=Bounds 'layer-properties-chevron'
 $aligned=[ordered]@{eye=(Bounds "layer-$row-visibility").Left;alpha_lock=(Bounds 'layer-alpha_lock').Left;new_layer=$first.Left;properties=$properties.Left+($properties.Width-26*$scale)/2}
 foreach($key in $aligned.Keys){if([Math]::Abs(($aligned[$key]-$panel.Left)/$scale-6) -gt 1){throw "The Layers $key icon slot does not start six points in: $(($aligned[$key]-$panel.Left)/$scale)"}}
 if([Math]::Abs(($drawer.Right-($chevron.Right-($chevron.Width-26*$scale)/2))/$scale-6) -gt 1){throw 'The Layer properties chevron does not mirror the six-point inset'}
 $ids=@{new_layer='layer-new';new_vector_layer='layer-new-vector';new_group='layer-new-group';new_selection_layer='layer-new-selection';add_mask='layer-add-mask';add_filter='layer-add-filter';import_image='layer-import';delete='layer-delete'}
 $width=$panel.Width/$scale;$previous=$null;$hidden=@()
 foreach($slot in @((Model).state.layer_tools.footer)){
  $id=$ids[$slot.button];$button=Find $id -Visible
  if($width+.5 -lt $slot.min_width){
   if($button){throw "Footer keeps $id beside the actions below its $($slot.min_width)-point width at $width points"}
   $hidden+=$slot.item.label;continue
  }
  if(!$button){throw "Footer hides $id at $width points"}
  $r=$button.Current.BoundingRectangle
  if([Math]::Abs($r.Width/$scale-26) -gt .5 -or [Math]::Abs($r.Height/$scale-26) -gt .5){throw "Footer action does not occupy a 26-point slot: $id"}
  if($previous -and [Math]::Abs(($r.Left-$previous.Right)/$scale-2) -gt .5){throw "Footer action gap differs from the two-point compound spacing: $id"}
  $previous=$r
 }
 if($hidden.Count){
  Press 'layer-actions'
  foreach($label in $hidden){Wait-Until {Find $label -Name -Visible} "The Layers actions menu does not list $label"}
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  Wait-Until {!(Find $hidden[0] -Name -Visible)} 'The Layers actions menu did not close'
 }
 $disclosure=Control 'layer-properties-disclosure' -Arranged;$label=Bounds 'layer-properties-disclosure-label';$r=$disclosure.Current.BoundingRectangle
 $labelInset=($label.Left-$r.Left)/$scale
 if([Math]::Abs($labelInset-38) -gt .5){throw 'Properties disclosure does not keep its six-point inset, 26-point icon slot and six-point gap'}
 $records=@($disclosure.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;bounds=$_.Current.BoundingRectangle}})
 @{disclosure=$r;label=$label;label_inset=$labelInset;descendants=$records}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'disclosure-geometry.json')
 $text=Control 'layer-properties-disclosure-label';$pattern=$null
 if($text.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern,[ref]$pattern)){
  $weight=$pattern.DocumentRange.GetAttributeValue([System.Windows.Automation.TextPattern]::FontWeightAttribute)
  if($weight -is [int] -and $weight -ne 400){throw "Properties disclosure does not use normal text weight: $weight"}
 }
}
try {
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 [IO.Directory]::CreateDirectory($env:CAPY_STORAGE_DIR)|Out-Null
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $review=Start-Process $Executable -WorkingDirectory $directory -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle;$native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Layer drawer review did not start' 90
 $root=$native.window.Root;$drawingWindow=$native.window.Handle
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Wait-Until {$canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$window=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Current.WindowVisualState;$window -eq [System.Windows.Automation.WindowVisualState]::Maximized -and [Math]::Abs((Model).state.camera.viewport[0]-$canvas.Width) -lt .1} 'Maximized layer drawer viewport did not settle'
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 $workspace='builtin:workspace:painter'
 if((Model).windows_workspace.id -ne $workspace){
  $choice=Find ('workspace-switch-'+$workspace)
  if(!$choice -or $choice.Current.IsOffscreen){Press 'header-workspace-menu';$choice=Control 'Sketch' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
  $choice.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
  Wait-Until {(Model).windows_workspace.id -eq $workspace -and !(Model).windows_workspace.busy} 'Sketch did not open'
 }
 $entry=@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.control.kind -eq 'panel' -and $_.item.control.panel -eq 'layers'})[0]
 if(!$entry){throw 'Sketch header lacks its Layers control'}
 Press ('header-item-'+$entry.id)
 Wait-Until {(Model).state.customization.drawer.layer_properties -eq $false -and (Find 'tool-drawer' -Visible)} 'Header Layers did not open combined inspector'
 $brush=@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.control.command -eq 'drawing_brush'})[0]
 if(!$brush){throw 'Sketch header lacks its Brush control'}
 Press ('header-item-'+$brush.id)
 Wait-Until {(ConvertTo-Json -InputObject (Model).state.customization.drawer.columns -Compress) -eq '[["brush_sets"],["tools"],["tool_settings"]]'} 'Layers to Brush did not adopt the shared three-column drawer'
 Wait-Until {(Control 'drawer-panel-brush_sets' -Arranged) -and (Control 'drawer-panel-tools' -Arranged) -and (Control 'drawer-panel-tool_settings' -Arranged)} 'Brush drawer native bodies did not appear after Layers'
 $brushDrawer=Control 'tool-drawer' -Arranged;$brushIdentity=$brushDrawer.GetRuntimeId() -join ':'
 $categories=@((Model).state.tool_panels.brush_sets.groups)
 $category=@($categories|Where-Object {$_.enabled -and !$_.selected})[0]
 if(!$category){throw 'Sketch drawer has no other brush category'}
 $categoryId='brush-set-'+$category.icon
 (Control $categoryId -Within $brushDrawer -Arranged).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {@((Model).state.tool_panels.brush_sets.groups|Where-Object {$_.icon -eq $category.icon -and $_.selected}).Count -eq 1} 'Drawer brush category did not select'
 if(((Control 'tool-drawer').GetRuntimeId() -join ':') -ne $brushIdentity){throw 'Changing the brush category recreated the open drawer'}
 Capture ('drawer-brush-switch-'+$Theme) -Composed -WithModel
 Press ('header-item-'+$entry.id)
 Wait-Until {(Model).state.customization.drawer.layer_properties -eq $false -and (Find 'layer-new' -Visible)} 'Layers did not reopen after Brush'
 Check-Footer
 $layerDrawer=Control 'tool-drawer' -Arranged;$layerIdentity=$layerDrawer.GetRuntimeId() -join ':'
 $other=@((Model).state.layers|Where-Object {$_.id -ne (Model).state.layer_tools.editing_layer.id})[0]
 if(!$other){throw 'Layers drawer has no second editing target'}
 Press ("layer-"+$other.id+'-name')
 Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $other.id} 'Drawer row did not change the editing target'
 if(((Control 'tool-drawer').GetRuntimeId() -join ':') -ne $layerIdentity){throw 'Selecting a layer recreated the open drawer'}
 $row=Bounds ("layer-"+$other.id+'-name')
 [CapyRowPointer]::DoubleClick([int]($row.X+$row.Width*.5),[int]($row.Y+$row.Height*.5))|Out-Null
 Wait-Until {Find ("layer-"+$other.id+'-rename') -Visible} 'Layer double tap did not start rename'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 if(!(Model).state.customization.drawer -or ((Control 'tool-drawer').GetRuntimeId() -join ':') -ne $layerIdentity){throw 'Layer double tap dismissed or recreated the drawer'}
 Test-AddPaper
 $paper=@((Model).state.layers|Where-Object content_icon -eq 'layer-paper-symbolic')[0]
 $paperEye=Bounds ("layer-"+$paper.id+'-visibility')
 $null=Wait-StablePixels {Screen-Pixels ([int]($paperEye.X+$paperEye.Width*.5)) ([int]($paperEye.Y+$paperEye.Height*.5))}
 Capture ('drawer-initial-'+$Theme) -Composed -WithModel
 $before=Visible-DrawerBounds;$initial=@((Model).state.layers).Count
 for($i=$initial;$i -lt 6;$i++){Press 'layer-new'}
 Wait-Until {@((Model).state.layers).Count -eq 6} 'Six-row floor fixture did not publish'
 Wait-Until {[Math]::Abs((Visible-DrawerBounds).Height-$before.Height) -lt 1} 'Layer drawer grew below its six-row floor'
 $before=Visible-DrawerBounds;$count=6
 Press 'layer-new';Wait-Until {@((Model).state.layers).Count -eq $count+1} 'New layer did not publish'
 Wait-Until {(Visible-DrawerBounds).Height -gt $before.Height} 'Added layer did not immediately grow the drawer'
 $grown=Visible-DrawerBounds
 $id=(Model).state.layer_tools.editing_layer.id
 $disclosure=(Model).state.layer_properties.disclosure
 if(!$disclosure.label -or $disclosure.return_label -ne 'All layers'){throw 'Layer disclosure lacks its return label'}
 Press 'layer-properties-disclosure'
 Wait-Until {(Model).state.customization.drawer.layer_properties -and (Control 'layer-properties-disclosure').Current.Name -eq 'All layers'} 'Properties page did not replace the list'
 Wait-Until {(Visible-DrawerBounds).Height -lt $before.Height+2} 'Properties did not return to the compact floor'
 Capture ('drawer-properties-'+$Theme) -Composed -WithModel
 Press 'layer-properties-disclosure';Wait-Until {!(Model).state.customization.drawer.layer_properties} 'All layers did not restore the list'
 Wait-Until {[Math]::Abs((Visible-DrawerBounds).Height-$grown.Height) -lt 1} 'All layers did not settle at the grown list height'
 Wait-Until {$row=Bounds "layer-row-$id";$list=Bounds 'layer-list';$row.Top -ge $list.Top -and $row.Bottom -le $list.Bottom} 'Restored row is outside its arranged list'
 Swipe $id -230
 Wait-Until {Find "layer-$id-swipe-lock" -Visible} 'Left swipe did not reveal Lock'
 foreach($action in @('lock','duplicate','delete')){$item=Control "layer-$id-swipe-$action";if(!$item.Current.IsEnabled){throw "Paint swipe action unavailable: $action"}}
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 $widths=@('lock','duplicate','delete'|ForEach-Object {(Bounds "layer-$id-swipe-$_").Width/$scale})
 if(@($widths|Where-Object {[Math]::Abs($_-72) -gt 1}).Count){throw 'Swipe actions do not share equal 72-point widths'}
 $row=Bounds "layer-row-$id"
 foreach($action in @('lock','duplicate','delete')){$bounds=Bounds "layer-$id-swipe-$action";if([Math]::Abs($bounds.Top-$row.Top)/$scale -gt 1 -or [Math]::Abs($bounds.Bottom-$row.Bottom)/$scale -gt 1){throw "Swipe $action does not fill the row height"}}
 if([Math]::Abs((Bounds "layer-$id-swipe-delete").Right-$row.Right)/$scale -gt 1){throw 'Swipe actions do not reach the row edge'}
 Capture ('drawer-actions-'+$Theme) -Composed -WithModel
 $palette=(Model).state.palette;$base=[Drawing.ColorTranslator]::FromHtml($palette.bg);$ink=[Drawing.ColorTranslator]::FromHtml($palette.text)
 $fills=@{}
 foreach($action in @('lock','duplicate')){
  $bounds=Bounds "layer-$id-swipe-$action";$path=Join-Path $run ("drawer-$action-fill-$Theme.png")
  $null=Screen-Pixels ([int]($bounds.X+$bounds.Width*.5)) ([int]($bounds.Y+6*$scale)) $path
  $bitmap=[Drawing.Bitmap]::new($path);$alpha=if($action -eq 'lock'){.16}else{.24};$maximum=0.
  try{foreach($y in 44..52){foreach($x in 44..52){$pixel=$bitmap.GetPixel($x,$y);foreach($channel in @('R','G','B')){$maximum=[Math]::Max($maximum,[Math]::Abs($pixel.$channel-((1-$alpha)*$base.$channel+$alpha*$ink.$channel)))}}}}finally{$bitmap.Dispose()}
  $fills[$action]=@{maximum_error=$maximum;alpha=$alpha;bounds=$bounds}
 }
 $fills|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run 'drawer-action-fills.json')
 if(@($fills.Values|Where-Object maximum_error -gt 2).Count){throw 'Swipe action fills retain the moving row selection or transparency'}
 Swipe $id 230;Wait-Until {!(Find "layer-$id-swipe-lock" -Visible)} 'Reverse swipe did not close actions'
 if((Layer $id).locked -or @((Model).state.layers).Count -ne $count+1){throw 'Reverse swipe changed the drawing'}
 Swipe $id -230;Press "layer-$id-swipe-lock";Wait-Until {(Layer $id).locked} 'Swipe Lock did not reach shared state'
 Swipe $id -230;Wait-Until {(Control "layer-$id-swipe-lock").Current.Name -eq 'Unlock'} 'Locked swipe did not relabel Unlock'
 Press "layer-$id-swipe-lock";Wait-Until {!(Layer $id).locked} 'Swipe Unlock did not reach shared state'
 Swipe $id -230;Press "layer-$id-swipe-duplicate";Wait-Until {@((Model).state.layers).Count -eq $count+2} 'Swipe Duplicate did not insert a layer'
 Invoke 'Undo' -Name;Wait-Until {@((Model).state.layers).Count -eq $count+1} 'Duplicate did not Undo in one step'
 Swipe $id -230;Press "layer-$id-swipe-delete";Wait-Until {!(Layer $id)} 'Swipe Delete did not remove its owner'
 Invoke 'Undo' -Name;Wait-Until {Layer $id} 'Delete did not Undo in one step'
 Press 'layer-new-group';Wait-Until {(Model).state.layer_tools.editing_layer.group} 'New group did not become the target'
 if((Model).state.layer_properties.disclosure.label -ne 'Group properties'){throw 'Group target lacks its Group properties disclosure'}
 Capture ('drawer-group-'+$Theme) -Composed -WithModel
 Press 'layer-properties-disclosure';Wait-Until {(Model).state.customization.drawer.layer_properties} 'Group properties did not open'
 if(Find 'properties-add-filter' -Visible){throw 'Group properties offered an ineligible Add Filter'}
 Press 'layer-properties-disclosure'
 Wait-Until {!(Model).state.customization.drawer.layer_properties -and (Control 'layer-new' -Arranged)} 'All layers did not restore its arranged New action'
 Press 'layer-new'
 for($repeat=0;$repeat -lt 3;$repeat++){
  Press 'layer-new-group';Wait-Until {(Model).state.layer_tools.editing_layer.group} 'Repeated Group did not become the target'
  Press 'layer-properties-disclosure';Wait-Until {(Model).state.customization.drawer.layer_properties} 'Repeated Group properties did not open'
  Start-Sleep -Milliseconds 70
  Press 'layer-properties-disclosure'
  Wait-Until {!(Model).state.customization.drawer.layer_properties -and (Control 'layer-new' -Arranged)} 'Rapid Group return did not restore the arranged New action'
  Press 'layer-new';Wait-Until {!(Model).state.layer_tools.editing_layer.group} 'New after rapid Group return did not become the target'
 }
 Press 'layer-add-filter'
 (Control 'Blur' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 Invoke 'Gaussian Blur' -Name
 Wait-Until {(Model).state.layer_tools.editing_layer.adjustment_effect} 'Gaussian Blur did not become the target'
 if((Model).state.layer_properties.disclosure.label -ne 'Gaussian Blur' -or (Model).state.layer_properties.disclosure.summary -notmatch 'Radius'){throw 'Filter disclosure did not show its filter type and authored Radius'}
 Wait-Until {(Model).state.customization.drawer.layer_properties} 'Inserted filter did not open its Properties page'
 if(Find 'properties-add-filter' -Visible){throw 'Filter properties offered an ineligible Add Filter'}
 Capture ('drawer-filter-'+$Theme) -Composed -WithModel
 Press 'layer-properties-disclosure'
 Wait-Until {!(Model).state.customization.drawer.layer_properties -and (Control 'layer-properties-disclosure').Current.Name -eq 'Gaussian Blur'} 'All layers did not restore the filter disclosure'
 for($i=0;$i -lt 18;$i++){Press 'layer-new'}
 Wait-Until {@((Model).state.layers).Count -ge $count+20} 'Overflow fixture layers did not publish'
 $overflowFrame=Control 'tool-drawer' -Arranged
 $overflowFooter=Control 'layer-properties-disclosure' -Arranged
 $overflowWatch=[Diagnostics.Stopwatch]::StartNew()
 $settled=@{height=0.;count=0;last=$null}
 Wait-Until {
  $frame=$overflowFrame.Current;$footer=$overflowFooter.Current
  $frameBounds=$frame.BoundingRectangle;$footerBounds=$footer.BoundingRectangle
  $frameOffscreen=$frame.IsOffscreen;$footerOffscreen=$footer.IsOffscreen
  if($frameOffscreen -or $footerOffscreen -or $frameBounds.IsEmpty -or $footerBounds.IsEmpty -or $frameBounds.Width -le 0 -or $frameBounds.Height -le 0 -or $footerBounds.Width -le 0 -or $footerBounds.Height -le 0 -or $footerBounds.Bottom -le $frameBounds.Top){
   $settled.count=0
   $settled.last=@{frame=$frameBounds;footer=$footerBounds;frame_offscreen=$frameOffscreen;footer_offscreen=$footerOffscreen;count=0;elapsed_ms=$overflowWatch.Elapsed.TotalMilliseconds}
   return $false
  }
  $native=[Windows.Rect]::new($frameBounds.X,$frameBounds.Y,$frameBounds.Width,$footerBounds.Bottom-$frameBounds.Top)
  $drawer=$frame.ItemStatus|ConvertFrom-Json;$shared=$drawer.placement.bounds
  $matches=$shared -and [Math]::Abs($native.Height/$scale-$shared.height) -le 1
  if(!$matches){$settled.count=0}
  elseif([Math]::Abs($settled.height-$shared.height) -lt .5){$settled.count++}else{$settled.height=$shared.height;$settled.count=0}
  $settled.last=@{native=$native;shared=$shared;frame_offscreen=$frameOffscreen;footer_offscreen=$footerOffscreen;count=$settled.count;elapsed_ms=$overflowWatch.Elapsed.TotalMilliseconds}
  $settled.count -ge 3
 } 'Overflow drawer native and shared placement did not settle together'
 $box=Visible-DrawerBounds;$window=$root.Current.BoundingRectangle
 @{window=$window;native=$box;shared=(Drawer);viewport=(Model).layout.viewport;rows=@((Model).state.layers).Count}|ConvertTo-Json -Depth 14|Set-Content (Join-Path $run 'overflow-geometry.json')
 if($box.Height/$scale -gt (Model).layout.viewport[1]*.85+1){throw 'Layer drawer exceeds the shared 85-percent viewport cap'}
 if($box.Bottom -gt $window.Bottom+1 -or $box.Top -lt $window.Top-1){throw 'Layer drawer exceeds the visible window'}
 $scroll=Control 'layer-list';$pattern=Scroll-Pattern $scroll
 if(!$pattern.Current.VerticallyScrollable){throw 'Capped layer drawer cannot scroll its overflow'}
 Capture ('drawer-overflow-'+$Theme) -Composed -WithModel
 $color=@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.control.kind -eq 'color'})[0]
 if(!$color){throw 'Sketch header lacks its Color control'}
 Press ('header-item-'+$color.id)
 Wait-Until {(ConvertTo-Json -InputObject (Model).state.customization.drawer.columns -Compress) -eq '[["color","palettes"]]'} 'Color did not open its shared Color and Palettes drawer'
 Capture ('drawer-color-pair-'+$Theme) -Composed -WithModel
 Custom-Toolbar
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
 [pscustomobject]@{theme=$Theme;header='passed';growth='passed';properties='passed';footer='passed';add_paper='passed';swipe_actions='passed';group='passed';overflow='passed';scope='Native UI and synthetic touch; adapter and presentation performance require separate evidence'}|ConvertTo-Json
}catch{
 try{if($settled -and $settled.last){$settled.last|ConvertTo-Json -Depth 14|Set-Content (Join-Path $run 'overflow-failure-comparison.json')}}catch{}
 try{$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|ForEach-Object {@{id=$_.Current.AutomationId;name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;bounds=$_.Current.BoundingRectangle;offscreen=$_.Current.IsOffscreen}}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run 'failure-native-tree.json')}catch{}
 try{Capture 'failure' -Composed -WithModel}catch{}
 ($_|Out-String)+$_.ScriptStackTrace|Set-Content (Join-Path $run 'failure.txt');throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
