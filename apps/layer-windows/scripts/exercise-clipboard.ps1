param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=30;$CapyFind='prefer-visible';$CapyPopups=$true
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'PackageFixture.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/clipboard/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{theme=$Theme}
function Sta([scriptblock]$Script,[object[]]$Arguments=@()){
 $shell=[powershell]::Create();$shell.Runspace=[runspacefactory]::CreateRunspace();$shell.Runspace.ApartmentState='STA';$shell.Runspace.Open()
 try{$null=$shell.AddScript('Add-Type -AssemblyName System.Windows.Forms,System.Drawing').AddStatement().AddScript($Script);foreach($a in $Arguments){$null=$shell.AddArgument($a)};$result=$shell.Invoke();if($shell.HadErrors){throw ($shell.Streams.Error|Out-String)};$result}
 finally{$shell.Runspace.Dispose();$shell.Dispose()}
}
function Formats{@(Sta {$data=[Windows.Forms.Clipboard]::GetDataObject();if($data){$data.GetFormats()}})}
function Clip-Size{
 $size=Sta {$data=[Windows.Forms.Clipboard]::GetDataObject();$stream=$data.GetData('PNG');if($stream){$image=[Drawing.Image]::FromStream($stream);try{"$($image.Width)x$($image.Height)"}finally{$image.Dispose()}}}
 [string]$size
}
function Start-Review([string]$Name){
 $script:stderr=Join-Path $run "$Name-stderr.log"
 $script:review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
 $null=$review.Handle
 Write-Output "Owned clipboard review $($review.Id): $run"
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Review did not start' 90
 if((Model).state.theme -ne $Theme){throw 'The clipboard review did not apply the requested theme'}
 $script:drawingWindow=$native.window.Handle;$script:root=$native.window.Root
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null;[CapyRowPointer]::Initialize([uint32]$review.Id)
}
function Close-Review{
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item -LiteralPath $stderr).Length){throw "Native clipboard stderr needs inspection: $stderr"}
}
function Active-Tab{@((Model).state.tabs|Where-Object active)[0]}
function Document-Identity{
 $view=Model;$m=$view.state;$stamp=@($view.windows_tabs.session_stamps|Where-Object id -eq $view.windows_tabs.selected)[0].stamp
 [ordered]@{drawing=$stamp|Select-Object artwork,checkpoint,revision,working_generation;file=$m.document_file|Select-Object revision,modified,location;layers=$m.layers|Select-Object id,label,group,adjustment_effect,visible,locked,paint_revision,mask_revision,object_count;depth=$view.color_panel.document_depth}|ConvertTo-Json -Depth 20 -Compress
}
function New-Image([scriptblock]$Action,[string]$Extent,[string]$Name,[int]$Objects=-1,[string[]]$Sources=@()){
 $source=(Model).windows_tabs.selected;$count=@((Model).windows_tabs.tabs).Count;$identity=Document-Identity
 $identity|Set-Content (Join-Path $run ($Name+'-source-before.json'))
 & $Action
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq $count+1 -and (Model).windows_tabs.selected -ne $source -and (Model).brush_ready -and (Requests) -eq 0 -and !(Model).state.document_file.busy} "$Name did not open a new drawing" 90
 $tab=Active-Tab
 if("$($tab.width)x$($tab.height)" -ne $Extent -or !(Model).state.document_file.modified -or (Model).state.document_file.location){throw "$Name did not open an unsaved drawing at the clipboard bounds"}
 if((Model).state.layer_tools.tool -eq 'transform'){throw "$Name opened placement handles"}
 if($Objects -ge 0 -and ((Model).state.layers|Measure-Object -Property object_count -Sum).Sum -ne $Objects){throw "$Name did not preserve the clipboard image objects"}
 Capture "$Name-$Theme" -WithModel
 $saved=Join-Path $run "$Name 日本語.capy";Save-ProjectAs $saved
 if($Sources.Count){
  $members=[CapyPackageFixture]::Read($saved)
  $manifest=[Text.Encoding]::UTF8.GetString(@($members|Where-Object Key -eq 'manifest.json')[0].Value)|ConvertFrom-Json -Depth 100
  $extents=@($manifest.objects|Where-Object type -eq 'capy.image/1'|ForEach-Object {$_.data.extent -join 'x'}|Sort-Object)
  if(($extents -join ',') -ne (($Sources|Sort-Object) -join ',')){throw "$Name changed the source image extents"}
  foreach($object in $manifest.objects|Where-Object type -eq 'capy.image-object/1'){
   $affine=if($null -eq $object.data.affine){@(1,0,0,1,0,0)}else{$object.data.affine}
   if(($affine[0..3] -join ',') -ne '1,0,0,1'){throw "$Name scaled or rotated a clipboard image"}
  }
 }
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'close_document'
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq $count -and (Model).windows_tabs.selected -eq $source -and (Model).brush_ready -and !(Model).state.document_file.busy} "$Name did not return to its source drawing" 90
 $returned=Document-Identity;$returned|Set-Content (Join-Path $run ($Name+'-source-after.json'))
 if($returned -ne $identity){throw "$Name changed the source drawing"}
 $checks[$Name]=$Extent
}
function Layers{@((Model).state.layers).Count}
function Requests{@((Model).state.requests).Count}
function Chord([uint16[]]$Modifiers,[uint16]$Key){
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 [CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)
}
function Copied([scriptblock]$Action,[string]$Message){
 Sta {[Windows.Forms.Clipboard]::SetText('awaiting copy')}|Out-Null;& $Action
 Wait-Until {(Formats) -contains 'art.capycanvas.clip.nonce' -and (Formats) -contains 'PNG' -and (Requests) -eq 0} $Message
}
function Settled{Wait-Until {(Requests) -eq 0 -and !(Model).state.document_file.busy} 'The clipboard request did not finish'}
function Tool([string]$Command){
 Invoke-Id (Tool-Tile $Command)
}
function Paste-In-Place{
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'edit'
 (Control 'Paste in Place' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Menu-Item([string]$Name){
 $condition=[System.Windows.Automation.AndCondition]::new(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
 [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
try {
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 Start-Review 'clipboard'
 $document=@((Model).state.tabs|Where-Object active)[0];$extent="$($document.width)x$($document.height)"
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action 'Test stroke'
 Wait-Until {(Model).state.document_file.modified} 'Stroke not acknowledged'
 $count=Layers
 Copied {Chord @(0x11) 0x43} 'Ctrl+C did not write the PNG and its nonce'
 if(!(Clip-Size)){throw 'The copied PNG does not decode'}
 if((Layers) -ne $count){throw 'Copy changed the layers'}
 $checks.keyboard_copy_writes_png_and_nonce=Clip-Size
 $bitmapSize=[string](Sta {$image=[Windows.Forms.Clipboard]::GetImage();if($image){try{"$($image.Width)x$($image.Height)"}finally{$image.Dispose()}}})
 if($bitmapSize -ne (Clip-Size)){throw 'The standard Bitmap consumer did not receive the copied image'}
 $checks.standard_bitmap_consumer=$bitmapSize
 New-Image {Chord @(0x11,0x12) 0x4e} (Clip-Size) 'internal-paste-as-new-image'
 Chord @(0x11) 0x56;Settled
 Wait-Until {(Layers) -eq $count+1} 'Ctrl+V did not paste the window copy'
 if((Model).state.layer_tools.tool -eq 'transform'){throw 'Pasting the window copy opened placement handles'}
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count} 'One Undo did not remove the pasted copy'
 Paste-In-Place;Settled
 Wait-Until {(Layers) -eq $count+1} 'Paste in Place did not paste the window copy'
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count} 'One Undo did not remove Paste in Place'
 $checks.own_paste_and_paste_in_place='passed'
 Chord @(0x11) 0x41;Wait-Until {(Model).state.layer_tools.has_selection} 'Ctrl+A did not select the canvas'
 $revision=(Model).state.document_file.revision
 Copied {Chord @(0x11) 0x58} 'Ctrl+X did not write the clipboard'
 Wait-Until {(Model).state.document_file.revision -gt $revision -and (Clip-Size) -eq $extent} 'Ctrl+X did not copy the selected canvas and erase it'
 Chord @(0x11) 0x56;Settled
 Wait-Until {(Layers) -eq $count+1} 'Ctrl+V did not paste the cut pixels'
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count} 'Undo did not remove the pasted cut'
 $checks.cut_selected_canvas=Clip-Size
 Tool 'lasso';Wait-Until {(Model).state.canvas_bar.context.kind -eq 'selection' -and ((Find 'canvas-bar-menu-copy') -or (Find 'canvas-bar-more'))} 'The selection bar did not appear'
 if(Find 'canvas-bar-menu-copy'){Invoke 'canvas-bar-menu-copy'}else{
  Invoke 'canvas-bar-more';$copy=@{item=$null};Wait-Until {$copy.item=Menu-Item 'Copy';$copy.item} 'The selection bar overflow did not offer Copy'
  $copy.item.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 }
 $merged=@{item=$null};Wait-Until {$merged.item=Menu-Item 'Copy Merged';$merged.item} 'Copy did not list Copy Merged'
 Copied {$pattern=$null;if($merged.item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}else{$merged.item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}} 'Copy Merged did not write the clipboard'
 if((Clip-Size) -ne $extent){throw 'Copy Merged did not copy the visible canvas'}
 Capture 'selection-copy'
 $checks.selection_bar_copy_merged=Clip-Size
 Chord @(0x11) 0x44;Wait-Until {!((Model).state.layer_tools.has_selection)} 'Deselect did not clear the pixel selection before image-object checks'
 $foreign=Join-Path $run 'foreign.png'
 $bitmap=[Drawing.Bitmap]::new(40,30);try{$g=[Drawing.Graphics]::FromImage($bitmap);$g.Clear([Drawing.Color]::FromArgb(255,30,160,90));$g.Dispose();$bitmap.Save($foreign,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
 Sta {param($path)$image=[Drawing.Image]::FromFile($path);try{[Windows.Forms.Clipboard]::SetImage($image)}finally{$image.Dispose()}} @($foreign)|Out-Null
 New-Image {& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit';Invoke-Id 'paste_as_new_image'} '40x30' 'external-paste-as-new-image'
 Paste-In-Place;Settled
 try{Wait-Until {(Layers) -eq $count+1} 'Paste in Place did not paste the image from another application'}catch{throw "$_ formats=$((Formats) -join ',') notice=$((Model).state.notice|ConvertTo-Json -Compress -Depth 4) requests=$(Requests)"}
 if((Model).state.layer_tools.editing_layer.object_count -ne 1){throw 'The image from another application did not become an image layer'}
 Chord @() 0x4f
 Wait-Until {((Model).state.commands|Where-Object id -eq 'move').selected -and ((Model).state.commands|Where-Object id -eq 'copy').enabled} 'Move did not target the pasted image'
 Copied {Chord @(0x11) 0x43} 'Copy did not deliver the selected image'
 New-Image {Chord @(0x11,0x12) 0x4e} (Clip-Size) 'structured-paste-as-new-image' 1 @('40x30')
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count} 'One Undo did not remove the pasted image'
 $checks.foreign_image_paste_in_place='passed'
 $wide=Join-Path $run 'wide.png'
 $bitmap=[Drawing.Bitmap]::new(80,20);try{$g=[Drawing.Graphics]::FromImage($bitmap);$g.Clear([Drawing.Color]::FromArgb(255,210,50,30));$g.Dispose();$bitmap.Save($wide,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
 Sta {param($first,$second)$files=[Collections.Specialized.StringCollection]::new();$null=$files.Add($first);$null=$files.Add($second);[Windows.Forms.Clipboard]::SetFileDropList($files)} @($foreign,$wide)|Out-Null
 New-Image {Chord @(0x11,0x12) 0x4e} '80x30' 'external-batch-as-new-image' 2 @('40x30','80x20')
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x42)
 Wait-Until {((Model).state.commands|Where-Object id -eq 'brush').selected -and @((Model).state.tool_settings).Count} 'B did not return to the brush after the image paste'
 Sta {[Windows.Forms.Clipboard]::SetText('clipboard sentinel')}|Out-Null
 $entry=Control ('tool-setting-'+@((Model).state.tool_settings)[0].id)
 $entry.SetFocus();Wait-Until {$entry.Current.HasKeyboardFocus} 'The brush size field did not take focus'
 [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x41);[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x43);Start-Sleep -Milliseconds 400
 $text=[string](Sta {[Windows.Forms.Clipboard]::GetText()})
 if((Requests) -ne 0 -or $text -eq 'clipboard sentinel' -or (Formats) -contains 'art.capycanvas.clip.nonce'){throw "A focused text field lost Ctrl+C to the canvas: $text"}
 $checks.text_field_keeps_ctrl_c=$text
 Close-Review
 $env:CAPY_STORAGE_DIR=Join-Path $run 'startup-profile'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 Start-Review 'startup'
 Sta {param($path)$image=[Drawing.Image]::FromFile($path);try{[Windows.Forms.Clipboard]::SetImage($image)}finally{$image.Dispose()}} @($foreign)|Out-Null
 Chord @(0x11) 0x56
 Wait-Until {$tab=Active-Tab;$tab.width -eq 40 -and $tab.height -eq 30 -and (Requests) -eq 0 -and !(Model).state.document_file.busy -and (Model).brush_ready} 'Startup Paste did not use the clipboard dimensions' 90
 if(!(Model).state.document_file.modified -or (Model).state.document_file.location -or (Model).state.layer_tools.tool -eq 'transform'){throw 'Startup Paste did not open an unsaved drawing without placement handles'}
 Capture "startup-paste-$Theme" -WithModel
 Save-ProjectAs (Join-Path $run 'Startup paste.capy')
 $source=(Model).windows_tabs.selected;$tabCount=@((Model).windows_tabs.tabs).Count;$layerCount=Layers
 Copied {Chord @(0x11) 0x43} 'The newly pasted drawing could not be copied'
 Chord @(0x11) 0x56
 Wait-Until {(Layers) -eq $layerCount+1 -and (Requests) -eq 0} 'Ordinary Paste did not add to the saved drawing'
 if((Model).windows_tabs.selected -ne $source -or @((Model).windows_tabs.tabs).Count -ne $tabCount){throw 'Paste into the saved drawing opened another tab'}
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $layerCount} 'Undo did not remove the pasted layer'
 $checks.startup_paste_and_saved_drawing='passed'
 Close-Review
 [PSCustomObject]$checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'result.json')
}catch{
 if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
