param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=20
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/zoom-readout/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{}
function Zoom{(Model).state.camera.zoom}
function Near([double]$Value,[double]$Expected){[Math]::Abs($Value-$Expected) -lt .001}
function Center($Element){$r=$Element.Current.BoundingRectangle;@([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}
function Tap([string]$Device,$Element){$at=Center $Element;[CapyRowPointer]::Down($Device,$at[0],$at[1]);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 150}
function Focused{(Find 'drawing-canvas').Current.HasKeyboardFocus}
function Open-Readout([string]$Device){
 Tap $Device (Control 'canvas-view-info')
 Wait-Until {Zoom-Item 'zoom-fit_canvas'} "$Device did not open the zoom menu"
 if(!(Focused)){throw "Opening the zoom menu with $Device took focus from the canvas"}
}
function Choose([string]$Device,[string]$Id){
 $item=@{value=$null};Wait-Until {$item.value=Zoom-Item $Id;$item.value -and !$item.value.Current.IsOffscreen} "Missing zoom item $Id"
 Tap $Device $item.value
 Wait-Until {!(Zoom-Item 'zoom-fit_canvas')} "Choosing $Id did not close the zoom menu"
}
try {
 Enter-CapyEnvironment
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned zoom readout review $($review.Id): $run"
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Zoom review did not start' 45
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 (Control 'drawing-canvas').SetFocus()
 Wait-Until {Focused} 'The canvas did not take focus'

 Open-Readout 'mouse'
 Capture 'zoom-menu-dark'
 Choose 'mouse' 'zoom-200'
 Wait-Until {Near (Zoom) 2} '200% did not set the zoom to 2'
 if(!(Focused)){throw 'Choosing 200% took focus from the canvas'}
 $checks.mouse_preset='passed'

 Open-Readout 'touch'
 Choose 'touch' 'zoom-actual_pixels'
 Wait-Until {Near (Zoom) 1} 'Actual Pixels did not set the zoom to 1'
 $checks.touch_actual_pixels='passed'

 Open-Readout 'pen'
 Choose 'pen' 'zoom-50'
 Wait-Until {Near (Zoom) .5} '50% did not set the zoom to 0.5'
 $checks.pen_preset='passed'

 Open-Readout 'mouse'
 $field=Control 'zoom-field'
 $field.SetFocus()
 $field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('250')
 [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {Near (Zoom) 2.5} 'Typing 250 did not set the zoom to 2.5'
 $checks.typed_zoom='passed'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {!(Zoom-Item 'zoom-fit_canvas')} 'Escape did not close the zoom menu'
 (Control 'drawing-canvas').SetFocus()

 Open-Readout 'mouse'
 Tap 'mouse' (Control 'canvas-view-info')
 Wait-Until {!(Zoom-Item 'zoom-fit_canvas')} 'A second press did not close the zoom menu'
 $checks.second_press_closes='passed'
 Open-Readout 'touch'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {!(Zoom-Item 'zoom-fit_canvas') -and (Focused)} 'Escape did not close the zoom menu with focus on the canvas'
 $checks.escape_keeps_canvas_focus='passed'

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
