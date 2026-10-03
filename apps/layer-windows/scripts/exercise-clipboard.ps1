param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=30
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/clipboard/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{}
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
function Layers{@((Model).state.layers).Count}
function Requests{@((Model).state.requests).Count}
function Chord([uint16[]]$Modifiers,[uint16]$Key){
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)
}
function Copied([scriptblock]$Action,[string]$Message){
 Sta {[Windows.Forms.Clipboard]::SetText('awaiting copy')}|Out-Null;& $Action
 Wait-Until {(Formats) -contains 'art.capycanvas.clip.nonce' -and (Formats) -contains 'PNG' -and (Requests) -eq 0} $Message
}
function Settled{Wait-Until {(Requests) -eq 0 -and !(Model).state.document_file.busy} 'The clipboard request did not finish'}
function Tool([string]$Command){
 $target=@{id=$null};Wait-Until {foreach($panel in (Model).panels){foreach($tile in $panel.tiles){if($tile.control.command -eq $Command){$target.id="tile-$($panel.id)-$($tile.id)";return $true}}};$false} "No $Command tile"
 Invoke-Id $target.id
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
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned clipboard review $($review.Id): $run"
 Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null;[CapyRowPointer]::Initialize([uint32]$review.Id)
 $document=@((Model).state.tabs|Where-Object active)[0];$extent="$($document.width)x$($document.height)"
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action 'Test stroke'
 Wait-Until {(Model).state.document_file.modified} 'Stroke not acknowledged'
 $count=Layers
 Copied {Chord @(0x11) 0x43} 'Ctrl+C did not write the PNG and its nonce'
 if(!(Clip-Size)){throw 'The copied PNG does not decode'}
 if((Layers) -ne $count){throw 'Copy changed the layers'}
 $checks.keyboard_copy_writes_png_and_nonce=Clip-Size
 Chord @(0x11) 0x56;Settled
 Wait-Until {(Layers) -eq $count+1} 'Ctrl+V did not paste the window copy'
 if((Model).state.layer_tools.tool -eq 'transform'){throw 'Pasting the window copy opened placement handles'}
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count} 'One Undo did not remove the pasted copy'
 Chord @(0x11,0x10) 0x56;Settled
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
 $foreign=Join-Path $run 'foreign.png'
 $bitmap=[Drawing.Bitmap]::new(40,30);try{$g=[Drawing.Graphics]::FromImage($bitmap);$g.Clear([Drawing.Color]::FromArgb(255,30,160,90));$g.Dispose();$bitmap.Save($foreign,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
 Sta {param($path)$image=[Drawing.Image]::FromFile($path);try{[Windows.Forms.Clipboard]::SetImage($image)}finally{$image.Dispose()}} @($foreign)|Out-Null
 Chord @(0x11,0x10) 0x56;Settled
 try{Wait-Until {(Layers) -eq $count+1} 'Paste in Place did not paste the image from another application'}catch{throw "$_ formats=$((Formats) -join ',') notice=$((Model).state.notice|ConvertTo-Json -Compress -Depth 4) requests=$(Requests)"}
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count} 'One Undo did not remove the pasted image'
 $checks.foreign_image_paste_in_place='passed'
 Sta {[Windows.Forms.Clipboard]::SetText('clipboard sentinel')}|Out-Null
 $entry=Control ('tool-setting-'+@((Model).state.tool_settings)[0].id)
 $entry.SetFocus();Wait-Until {$entry.Current.HasKeyboardFocus} 'The brush size field did not take focus'
 [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x41);[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x43);Start-Sleep -Milliseconds 400
 $text=[string](Sta {[Windows.Forms.Clipboard]::GetText()})
 if((Requests) -ne 0 -or $text -eq 'clipboard sentinel' -or (Formats) -contains 'art.capycanvas.clip.nonce'){throw "A focused text field lost Ctrl+C to the canvas: $text"}
 $checks.text_field_keeps_ctrl_c=$text
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'Native clipboard stderr needs inspection'}
 [PSCustomObject]$checks|ConvertTo-Json
}catch{
 if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
