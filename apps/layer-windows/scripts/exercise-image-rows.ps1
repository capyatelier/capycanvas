param([Parameter(Mandatory)][string]$Executable,[ValidateSet('mouse','pen','touch')][string]$Device='mouse',[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='prefer-visible';$CapyPopups=$true;$CapyWaitSeconds=30
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'PackageFixture.cs')
Add-Type -AssemblyName System.Drawing
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/image-rows/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$checks=[ordered]@{device=$Device;theme=$Theme}
function Start-Review([string]$Log){
 $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $Log
 $null=$review.Handle
 Write-Output "Owned image row review $($review.Id): $run"
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Image row review did not start' 90
 $script:drawingWindow=$native.window.Handle;$script:root=$native.window.Root
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Start-Sleep -Milliseconds 600
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
 [CapyRowPointer]::Initialize([uint32]$review.Id)
}
function Sta([scriptblock]$Script,[object[]]$Arguments=@()){
 $shell=[powershell]::Create();$shell.Runspace=[runspacefactory]::CreateRunspace();$shell.Runspace.ApartmentState='STA';$shell.Runspace.Open()
 try{$null=$shell.AddScript('Add-Type -AssemblyName System.Windows.Forms,System.Drawing').AddStatement().AddScript($Script);foreach($a in $Arguments){$null=$shell.AddArgument($a)};$result=$shell.Invoke();if($shell.HadErrors){throw ($shell.Streams.Error|Out-String)};$result}
 finally{$shell.Runspace.Dispose();$shell.Dispose()}
}
function Package([string]$Path){
 $members=[CapyPackageFixture]::Read($Path)
 [Text.Encoding]::UTF8.GetString(@($members|Where-Object Key -eq 'manifest.json')[0].Value)|ConvertFrom-Json -Depth 100
}
function Point([string]$Id){
 $stable=@{bounds=$null;count=0}
 Wait-Until {
  $item=Find $Id
  if(!$item -or $item.Current.IsOffscreen){return $false}
  $bounds=$item.Current.BoundingRectangle
  if($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){return $false}
  if($bounds -eq $stable.bounds){$stable.count++}else{$stable.bounds=$bounds;$stable.count=0}
  $stable.count -ge 2
 } "$Id did not arrange visibly"
 @{x=[int]($stable.bounds.X+$stable.bounds.Width*.5);y=[int]($stable.bounds.Y+$stable.bounds.Height*.5)}
}
function Tap([string]$Id){$at=Point $Id;[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()}
function Menu-Item([string]$Name){
 $hit=@{item=$null};Wait-Until {$hit.item=Find $Name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem);$hit.item -and !$hit.item.Current.IsOffscreen} "Missing menu item: $Name"
 $hit.item
}
function Choose([string]$Name){
 $item=Menu-Item $Name;$item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {
  $closed=!(Find $Name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))
  $closed -and [CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow
 } 'The invoked row menu did not dismiss and return foreground to its owned drawing'
}
function Context([string]$Id){
 $at=Point $Id
 if($Device -eq 'mouse'){[CapyRowPointer]::RightClick($at.x,$at.y)}
 else{
  [CapyRowPointer]::Down($Device,$at.x,$at.y)
  Wait-Until {(Gesture).menu_open} 'Holding the row did not open its menu' 4
  [CapyRowPointer]::Up()
 }
}
function Dismiss{[CapyRowPointer]::Key(0x1b);Wait-Until {!(Gesture).menu_open} 'The row menu did not close'}

function Rows{@((Model).state.layers|Where-Object object)}
function Row([double]$Id){@((Model).state.layers|Where-Object id -eq $Id)[0]}
function Order{(@(Rows|ForEach-Object id)) -join ','}
function Gesture {try{(Find 'layer-list').Current.ItemStatus|ConvertFrom-Json}catch{}}
function Chord([uint16[]]$Modifiers,[uint16]$Key){
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 [CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)
}
function Layer-History([string]$Command,[scriptblock]$Ready){
 $revision=(Model).state.document_file.revision
 Chord @(0x11) $(if($Command -eq 'Undo'){0x5a}else{0x59})
 Wait-Until {(Model).state.document_file.revision -gt $revision -and (& $Ready)} "$Command did not restore the layer edit"
}
function Paste-Image([string]$Name,[int[]]$Rgb,[int]$Width,[int]$Height){
 $path=Join-Path $run "$Name.png"
 $bitmap=[Drawing.Bitmap]::new($Width,$Height);try{$g=[Drawing.Graphics]::FromImage($bitmap);$g.Clear([Drawing.Color]::FromArgb(255,$Rgb[0],$Rgb[1],$Rgb[2]));$g.Dispose();$bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
 $count=@((Model).state.layers).Count
 Sta {param($path)$image=[Drawing.Image]::FromFile($path);try{[Windows.Forms.Clipboard]::SetImage($image)}finally{$image.Dispose()}} @($path)|Out-Null
 Chord @(0x11,0x10) 0x56
 Wait-Until {@((Model).state.layers).Count -eq $count+1 -and (Model).state.layer_tools.editing_layer.object -and @((Model).state.requests).Count -eq 0} 'Paste did not create a separate Object layer'
}
try{
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $stderr=Join-Path $run 'stderr.log';Start-Review $stderr
 Paste-Image 'red' @(220,40,30) 640 480
 $back=(Model).state.layer_tools.editing_layer.id
 Paste-Image 'blue' @(30,60,220) 480 360
 $front=(Model).state.layer_tools.editing_layer.id
 if(@(Rows).Count -ne 2 -or $front -eq $back){throw 'Two source files did not become two ordinary Object layers'}
 foreach($id in @($front,$back)){
  Wait-Until {(Control "layer-$id-thumbnail").Current.ItemStatus -eq 'Ready'} 'Object layer thumbnail did not arrive' 45
  if(Find "layer-$id-expand"){throw 'An Object layer still exposes a child image list'}
 }
 Capture "object-layers-$Device-$Theme" -WithModel
 $checks.rows_and_previews='passed'
 Tap "layer-$front-name"
 Invoke "layer-$back-selection"
 Wait-Until {(Row $front).selected -and (Row $back).selected} 'Ordinary layer selection did not include both Object layers'
 Invoke "layer-$back-selection"
 Wait-Until {(Row $front).selected -and !(Row $back).selected} 'Ordinary layer selection did not remove the second Object layer'
 Tap "layer-$back-visibility";Wait-Until {!(Row $back).visible} 'Layer visibility did not hide its object'
 Layer-History 'Undo' {(Row $back).visible}
 $checks.selection_and_visibility='passed'
 $before=Order
 $from=Point "layer-$front-drag";$bounds=(Control "layer-row-$back" -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::Down($Device,$from.x,$from.y);[CapyRowPointer]::Move([int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.8))
 Wait-Until {(Gesture).phase -eq 'dragging' -and (Gesture).can_drop} 'Ordinary layer grip did not start the object-layer drag'
 [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
 Wait-Until {(Gesture).phase -eq 'idle'} 'Escape did not cancel the object-layer drag'
 if((Order) -ne $before){throw 'Canceled layer drag changed order'}
 $from=Point "layer-$front-drag";$bounds=(Control "layer-row-$back").Current.BoundingRectangle
 [CapyRowPointer]::Down($Device,$from.x,$from.y);[CapyRowPointer]::Move([int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.8))
 Wait-Until {(Gesture).phase -eq 'dragging' -and (Gesture).can_drop} 'Layer drag did not reach the second object'
 [CapyRowPointer]::Up();Wait-Until {(Order) -eq "$back,$front"} 'Layer drag did not reorder Object layers'
 Layer-History 'Undo' {(Order) -eq $before};Layer-History 'Redo' {(Order) -eq "$back,$front"};Layer-History 'Undo' {(Order) -eq $before}
 $checks.layer_reorder='passed'
 Tap "layer-$front-name";Context "layer-row-$front";(Menu-Item 'Organize').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand();Choose 'Duplicate'
 Wait-Until {@(Rows).Count -eq 3} 'Duplicate Layer did not duplicate the Object layer'
 Layer-History 'Undo' {@(Rows).Count -eq 2}
 Chord @(0x11) 0x43
 Wait-Until {Sta {[Windows.Forms.Clipboard]::ContainsData('art.capycanvas.clip.nonce')}} 'Layer Copy did not publish its nonce'
 Chord @(0x11) 0x56;Wait-Until {@(Rows).Count -eq 3 -and @((Model).state.requests).Count -eq 0} 'Internal Paste did not create a separate Object layer'
 Layer-History 'Undo' {@(Rows).Count -eq 2}
 Chord @(0x11) 0x58;Wait-Until {@(Rows).Count -eq 1 -and @((Model).state.requests).Count -eq 0} 'Layer Cut did not remove the selected Object layer'
 Layer-History 'Undo' {@(Rows).Count -eq 2 -and (Order) -eq $before}
 $checks.layer_duplicate_copy_cut='passed'
 Chord @(0x11) 0x41;Wait-Until {(Model).state.layer_tools.has_selection} 'Select All did not create a pixel selection'
 Chord @(0x11) 0x44;Wait-Until {!((Model).state.layer_tools.has_selection)} 'Deselect did not clear the pixel selection'
 Tap "layer-$front-name";Context "layer-row-$front";Choose 'Rasterize Layer'
 Wait-Until {!(Row $front).object -and @(Rows).Count -eq 1} 'Rasterize Layer did not replace the Object layer with paint'
 Layer-History 'Undo' {(Row $front).object -and @(Rows).Count -eq 2}
 $checks.rasterize_and_pixel_selection='passed'
 $saved=Join-Path $run 'Object layers 日本語.capy';Save-ProjectAs $saved
 $manifest=Package $saved
 if(@($manifest.objects|Where-Object type -eq 'capy.image-object/1').Count -ne 2 -or @($manifest.objects|Where-Object type -eq 'capy.object-layer/1').Count){throw 'Object layers saved superseded child-list records'}
 Open-Project $saved;Wait-Until {(Model).state.document_file.location.uri -eq $saved -and @(Rows).Count -eq 2 -and (Model).brush_ready} 'Object layers did not reopen'
 Capture "object-layers-reopened-$Device-$Theme" -WithModel
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close
 if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
 $checks.save_reopen='passed';[PSCustomObject]$checks|ConvertTo-Json
}catch{
 if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
