param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=20
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/size-dialogs/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$ControlType=[System.Windows.Automation.ControlType]
$checks=[ordered]@{}
function Draft{(Model).state.layer_tools.canvas_size}
function Image-Draft{(Model).state.layer_tools.image_size}
function Size{$tab=@((Model).state.tabs)[0];@([int]$tab.width,[int]$tab.height)}
function Center($Element){$r=$Element.Current.BoundingRectangle;@([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}
function Tap($Element){$at=Center $Element;[CapyRowPointer]::Down('mouse',$at[0],$at[1]);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 150}
function Menu-Item([string]$Id){
    $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id)
    foreach($item in [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){
        if($item.Current.ProcessId -eq $review.Id -and !$item.Current.IsOffscreen){return $item}
    }
}
function Desktop-Button([string]$Name){
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$ControlType::Button))
    foreach($scope in @($root,[System.Windows.Automation.AutomationElement]::RootElement)){$hit=$scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition);if($hit){return $hit}}
}
function Open-SizeDialog([string]$Command,[scriptblock]$Opened){
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
    $image=@{item=$null};Wait-Until {$image.item=Menu-Item 'menu-image';$image.item} 'Edit menu has no Image submenu'
    $image.item.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $entry=@{item=$null};Wait-Until {$entry.item=Menu-Item $Command;$entry.item} "Image submenu has no $Command"
    $entry.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until $Opened "$Command did not open the shared draft"
}
function Open-CanvasSize{Open-SizeDialog 'canvas_size' {(Draft) -and (Find 'canvas-size-width')}}
function Open-ImageSize{Open-SizeDialog 'image_size' {(Image-Draft) -and (Find 'image-size-width')}}
function Field([string]$Id){(Control $Id -Type $ControlType::Edit).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value}
function Pixels([string]$Id){[int]((Field $Id) -replace '[^0-9.-]','')}
try {
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle
    Write-Output "Owned size dialog review $($review.Id): $run"
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Size dialog review did not start' 45
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $original=Size;$wider=$original[0]+200

    Open-CanvasSize
    if((Pixels 'canvas-size-width') -ne $original[0] -or (Pixels 'canvas-size-height') -ne $original[1]){throw 'Canvas Size did not start at the current size'}
    if((Desktop-Button 'Apply').Current.IsEnabled){throw 'Apply was enabled before the size changed'}
    Capture 'open'
    $checks.opens_from_edit_image='passed'

    $width=Control 'canvas-size-width' -Type $ControlType::Edit
    $width.SetFocus()
    $width.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue([string]$wider)
    Wait-Until {(Draft).values[0] -eq $wider -and (Draft).can_apply} 'Typing a width did not reach the draft'
    Wait-Until {(Control 'canvas-size-message').Current.Name -match "$wider"} 'The message did not show the new size'
    Wait-Until {(Desktop-Button 'Apply').Current.IsEnabled} 'Apply did not enable'
    $checks.typed_width_updates_message='passed'

    Tap (Control 'canvas-size-anchor-top_left')
    Wait-Until {(Draft).anchor -eq 'top_left'} 'The anchor cell did not choose Top left'
    if((Draft).values[0] -ne $wider){throw 'Choosing an anchor lost the typed width'}
    if([System.Windows.Automation.AutomationElement]::FocusedElement.Current.AutomationId -like 'canvas-size-anchor-*'){throw 'An anchor cell took focus'}
    $checks.anchor_cells='passed'

    $relative=Control 'canvas-size-relative'
    $relative.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Draft).relative -and (Draft).values[0] -eq 200 -and (Draft).values[1] -eq 0} 'Relative did not convert the draft to changes'
    Wait-Until {(Pixels 'canvas-size-width') -eq 200} 'The width field did not show the relative change'
    $relative.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {!(Draft).relative -and (Draft).values[0] -eq $wider} 'Clearing Relative did not restore absolute sizes'
    $unit=Control 'canvas-size-unit' -Type $ControlType::ComboBox
    $unit.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Percent' -Name -Type $ControlType::ListItem).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Draft).unit -eq 'percent' -and [Math]::Abs((Draft).values[1]-100) -lt .01} 'Percent did not convert the draft'
    $unit.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Pixels' -Name -Type $ControlType::ListItem).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Draft).unit -eq 'pixels' -and (Draft).values[0] -eq $wider} 'Pixels did not restore the draft'
    Capture 'edited'
    $checks.relative_and_units='passed'

    (Desktop-Button 'Apply').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Draft) -and !(Find 'canvas-size-width')} 'Apply did not close Canvas Size'
    Wait-Until {$s=Size;$s[0] -eq $wider -and $s[1] -eq $original[1]} 'Apply did not resize the canvas'
    Wait-Until {(Control 'drawing-canvas').Current.IsEnabled} 'Apply did not release the canvas'
    (Control 'drawing-canvas').SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5A)
    Wait-Until {$s=Size;$s[0] -eq $original[0]} 'Undo did not restore the canvas size'
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x10),0x5A)
    Wait-Until {$s=Size;$s[0] -eq $wider} 'Redo did not resize the canvas again'
    $checks.apply_undo_redo='passed'

    Open-CanvasSize
    $height=Control 'canvas-size-height' -Type $ControlType::Edit
    $height.SetFocus()
    $height.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('64')
    Wait-Until {(Draft).values[1] -eq 64} 'Typing a height did not reach the draft'
    [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {(Draft) -and (Draft).values[1] -eq $original[1]} 'Escape in the field did not revert the typed height'
    [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Draft) -and !(Find 'canvas-size-width')} 'Escape did not cancel Canvas Size'
    if((Size)[1] -ne $original[1]){throw 'Cancel changed the canvas'}
    Wait-Until {(Control 'drawing-canvas').Current.IsEnabled} 'Cancel did not release the canvas'
    $checks.escape_cancels='passed'

    $current=Size
    Open-ImageSize
    if((Pixels 'image-size-width') -ne $current[0] -or (Pixels 'image-size-height') -ne $current[1]){throw 'Image Size did not start at the current size'}
    if(!(Image-Draft).constrain -or !(Find 'image-size-resolution')){throw 'Image Size did not open with constrained proportions and a resolution'}
    $resampleLabel=@((Image-Draft).resamples)[1].label;$resampleValue=@((Image-Draft).resamples)[1].resample
    $resample=Control 'image-size-resample' -Type $ControlType::ComboBox
    $resample.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $resampleLabel -Name -Type $ControlType::ListItem).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Image-Draft).resample -eq $resampleValue} 'Choosing a resampling method did not reach the draft'
    $half=[int]($current[0]/2);$halfHeight=[Math]::Round($current[1]*$half/$current[0])
    $width=Control 'image-size-width' -Type $ControlType::Edit
    $width.SetFocus()
    $width.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue([string]$half)
    Wait-Until {(Image-Draft).values[0] -eq $half -and [Math]::Abs((Image-Draft).values[1]-$halfHeight) -le 1 -and (Image-Draft).can_apply} 'Constrained width did not scale the height'
    Wait-Until {[Math]::Abs((Pixels 'image-size-height')-$halfHeight) -le 1} 'The height field did not follow the constrained width'
    Wait-Until {(Control 'image-size-message').Current.Name -match "$half"} 'The Image Size message did not show the new size'
    if((Image-Draft).resample -ne $resampleValue){throw 'Typing a width lost the resampling choice'}
    Capture 'image-size-edited'
    $checks.image_size_constrains_and_resamples='passed'

    (Desktop-Button ((Image-Draft).apply_label)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Image-Draft) -and !(Find 'image-size-width')} 'Apply did not close Image Size'
    Wait-Until {$s=Size;$s[0] -eq $half -and [Math]::Abs($s[1]-$halfHeight) -le 1} 'Apply did not resample the image'
    Wait-Until {(Control 'drawing-canvas').Current.IsEnabled} 'Image Size did not release the canvas'
    (Control 'drawing-canvas').SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5A)
    Wait-Until {$s=Size;$s[0] -eq $current[0] -and $s[1] -eq $current[1]} 'Undo did not restore the image size'
    $checks.image_size_apply_undo='passed'

    Open-ImageSize
    [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Image-Draft) -and !(Find 'image-size-width')} 'Escape did not cancel Image Size'
    Wait-Until {(Control 'drawing-canvas').Current.IsEnabled} 'Image Size cancel did not release the canvas'
    $checks.image_size_escape_cancels='passed'

    [CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
    if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
    $checks.evidence=$run
    [pscustomobject]$checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
} catch {
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
} finally {
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
