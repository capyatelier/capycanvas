param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/notice/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{}
function Owned([string]$Value,$Type){
 $condition=[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Value),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
 if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
 [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Shown([string]$Id){$item=Find $Id;if($item -and !$item.Current.IsOffscreen){$item}}
function Notice{(Model).state.notice}
function NoticeText{$text=Shown 'canvas-notice-text';if($text -and (Shown 'canvas-notice')){$text.Current.Name}}
function Center($Element){$r=$Element.Current.BoundingRectangle;@([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}
function Click([string]$Device,[int[]]$At){[CapyRowPointer]::Down($Device,$At[0],$At[1]);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 120}
function Focused{(Find 'drawing-canvas').Current.HasKeyboardFocus}
function Lock([bool]$Value){
 $pattern=(Control 'layer-lock').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
 if(($pattern.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On) -ne $Value){$pattern.Toggle()}
 Wait-Until {[bool](Model).state.layer_tools.editing_layer.locked -eq $Value} "Layer lock did not become $Value"
 (Find 'drawing-canvas').SetFocus()
}
try {
 Enter-CapyEnvironment
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned notice review $($review.Id): $run"
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Notice review did not start'
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 Wait-Until {$b=(Find 'drawing-canvas').Current.BoundingRectangle;$b.Width -gt 1200} 'Maximized canvas did not settle' 10
 Fit-Canvas;Start-Sleep -Milliseconds 300
 (Find 'Test stroke' -Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {(Model).state.document_file.modified} 'Controlled drawing did not finish'
 $canvas=(Find 'drawing-canvas').Current.BoundingRectangle
 $point=@([int]($canvas.X+$canvas.Width*.5),[int]($canvas.Y+$canvas.Height*.4))
 if(Shown 'canvas-notice'){throw 'A notice showed before any refusal'}

 (Find 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Key([uint32]$review.Id,0x4F)
 Wait-Until {((Model).state.commands|Where-Object id -eq 'move').selected} 'O did not choose Move'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x41)
 Wait-Until {(Model).state.layer_tools.has_selection -and (Shown 'canvas-bar-scale_rotate')} 'Select All did not show the selection bar with Transform'
 Lock $true
 $reason='The active layer is locked'
 $hint=@{value=$null}
 Wait-Until {$item=Shown 'canvas-bar-scale_rotate';$hint.value=((Model).state.commands|Where-Object id -eq 'scale_rotate').disabled_reason
  $item -and !$item.Current.IsEnabled -and $hint.value -and $item.Current.HelpText -eq $hint.value} 'The disabled Transform item did not carry its published reason'
 foreach($device in @('touch','pen','mouse')){
  Click $device (Center (Control 'canvas-bar-scale_rotate' -Arranged))
  Wait-Until {$null -ne (Owned $hint.value ([System.Windows.Automation.ControlType]::ToolTip))} "$device tap on a disabled bar item did not reveal its reason" 5
  Click 'mouse' (Center (Shown 'panel-tab-layers'))
  Wait-Until {$null -eq (Owned $hint.value ([System.Windows.Automation.ControlType]::ToolTip))} "The next $device contact did not hide the revealed reason" 5
 }
 if((Model).state.canvas_bar.context.kind -ne 'selection'){throw 'Tapping a disabled item changed the bar'}
 $checks.disabled_bar_reason_revealed_by_mouse_touch_pen='passed'

 Click 'mouse' $point
 Wait-Until {(NoticeText) -eq $reason -and (Notice).text -eq $reason} 'Move on a locked layer did not show the notice'
 if(Shown 'canvas-notice-action'){throw 'The locked-layer refusal offered an action'}
 if((Model).state.host_error){throw "The refusal reached host_error: $((Model).state.host_error)"}
 if(!(Focused)){throw 'The notice took focus from the canvas'}
 $notice=(Find 'canvas-notice').Current.BoundingRectangle
 if([Math]::Abs(($notice.X+$notice.Width/2)-($canvas.X+$canvas.Width/2)) -gt ($canvas.Width*.25)){throw 'The notice is not centered over the canvas'}
 Capture 'locked-notice-dark'
 $first=(Notice).id
 Click 'pen' $point
 Wait-Until {(Notice).id -gt $first -and (NoticeText) -eq $reason} 'A repeated refusal did not show again'
 $checks.refusal_notice_repeats='passed'

 Lock $false
 Click 'touch' $point
 Wait-Until {!(Shown 'canvas-notice') -and !(Notice)} 'The next canvas contact did not dismiss the notice'
 $checks.next_contact_dismisses='passed'

 Lock $true
 Click 'mouse' $point
 Wait-Until {(NoticeText) -eq $reason} 'The refusal did not show once more'
 $shown=[Diagnostics.Stopwatch]::StartNew()
 Wait-Until {!(Shown 'canvas-notice')} 'The notice did not time out' 8
 if($shown.Elapsed.TotalSeconds -lt 3){throw "The notice hid after $($shown.Elapsed.TotalSeconds) s"}
 Wait-Until {!(Notice)} 'The timeout did not dismiss the shared notice' 5
 if((Model).state.host_error){throw 'The timeout decline reported an error'}
 $checks.timeout_dismisses_quietly='passed'
 Lock $false

 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x44)
 Wait-Until {!(Model).state.layer_tools.has_selection} 'Deselect did not clear the selection'
 $count=(Model).state.layers.Count;Invoke 'layer-new'
 Wait-Until {(Model).state.layers.Count -eq $count+1} 'New layer was not created'
 (Find 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x57)
 Wait-Until {((Model).state.commands|Where-Object id -eq 'auto_select').selected} 'W did not choose Auto Select'
 $source=Control 'tool-action-selection_reference'
 $source.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
 Wait-Until {((Model).state.commands|Where-Object id -eq 'selection_reference').selected} 'Reference sampling was not chosen'
 (Find 'drawing-canvas').SetFocus()
 Click 'mouse' $point
 Wait-Until {(NoticeText) -eq 'This tool samples reference layers, and none is marked' -and (Shown 'canvas-notice-action')} 'The Wand refusal did not offer a reference'
 $action=Shown 'canvas-notice-action'
 if($action.Current.Name -notmatch '^Use .+ as Reference$'){throw "Unexpected notice action: $($action.Current.Name)"}
 if(!(Focused)){throw 'The notice action took focus from the canvas'}
 $frame=(Find 'canvas-notice').Current.BoundingRectangle;$line=(Find 'canvas-notice-text').Current.BoundingRectangle;$offer=$action.Current.BoundingRectangle
 if($line.Top -lt $frame.Top -or $line.Bottom -gt $frame.Bottom -or $line.Right -gt $offer.Left){throw 'The notice text is clipped or overlaps its action'}
 if([Math]::Abs(($line.Top+$line.Bottom)/2-($frame.Top+$frame.Bottom)/2) -gt 3 -or [Math]::Abs(($offer.Top+$offer.Bottom)/2-($frame.Top+$frame.Bottom)/2) -gt 3){throw 'The notice text and action are not vertically centered'}
 Capture 'reference-notice-dark'
 Click 'pen' (Center $action)
 Wait-Until {@((Model).state.layers|Where-Object reference).Count -eq 1 -and !(Shown 'canvas-notice') -and !(Notice)} 'The notice action did not mark one reference layer'
 if(!(Focused)){throw 'Accepting the notice took focus from the canvas'}
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
 Wait-Until {@((Model).state.layers|Where-Object reference).Count -eq 0} 'One Undo did not remove the reference mark'
 $checks.reference_action_one_undo_step='passed'
 $lifecycle=Join-Path $run 'lifecycle.log'
 if((Test-Path $lifecycle) -and (Select-String -Path $lifecycle -Pattern 'gpu_recovery_started' -SimpleMatch -Quiet)){throw 'A contact reached the GPU loss test control'}

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
 if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force}
 Exit-CapyEnvironment
}
