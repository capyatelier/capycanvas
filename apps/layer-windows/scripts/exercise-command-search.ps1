param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/command-search/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
$checks=[ordered]@{}
function Model {
 try {$value=Get-Content -LiteralPath (Join-Path $run 'ui-state.json') -Raw|ConvertFrom-Json;if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){$value.model}}catch{}
}
function Visible([string]$Id){$item=Find $Id;if($item -and !$item.Current.IsOffscreen){$item}}
function Capture([string]$Name){& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -ClientOnly -Output (Join-Path $run ($Name+'.png')) *> (Join-Path $run ($Name+'.json'))}
function Size {((Model).state.tool_settings|Where-Object id -eq 'size').value}
function Canvas-Point([double]$FractionX,[double]$FractionY){
 $canvas=(Find 'Drawing canvas' -Name).Current.BoundingRectangle
 @([int]($canvas.X+$canvas.Width*$FractionX),[int]($canvas.Y+$canvas.Height*$FractionY))
}
function Open-Search {
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x4B)
 Wait-Until {Visible 'command-search'} 'Primary+K did not open command search' 10
 Wait-Until {[System.Windows.Automation.AutomationElement]::FocusedElement.Current.AutomationId -eq 'command-search'} 'Command search did not take keyboard focus' 5
}
function Query([string]$Text){
 (Find 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
}
function Row([int]$Index){Visible ('command-result-'+$Index)}
function Key([uint16]$Code){[CapyRowPointer]::Key([uint32]$review.Id,$Code)}
function Closed {Wait-Until {!(Visible 'command-bar') -and !(Visible 'command-search')} 'Command search did not close' 5}
try {
 foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned command search review $($review.Id): $run"
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
 . (Join-Path $repo 'tools/performance/windows-pen-ui.ps1') -ProcessId $review.Id
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Command search review did not start'
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 Wait-Until {Find 'Drawing canvas' -Name} 'Drawing canvas did not appear' 10
 (Find 'Drawing canvas' -Name).SetFocus()

 Open-Search
 $frame=(Find 'command-bar').Current.BoundingRectangle;$canvas=(Find 'Drawing canvas' -Name).Current.BoundingRectangle
 $window=$root.Current.BoundingRectangle;$scale=$canvas.Width/(Model).layout.viewport[0]
 $expectedWidth=[Math]::Min(560,[Math]::Max(240,$window.Width/$scale-48))
 if([Math]::Abs($frame.Width/$scale-$expectedWidth) -gt 8){throw "Command bar width $($frame.Width/$scale) differs from $expectedWidth"}
 if([Math]::Abs(($frame.X+$frame.Width/2)-($window.X+$window.Width/2)) -gt 4*$scale){throw 'Command bar is not centered'}
 $top=($frame.Y-$window.Y)/$scale
 if($top -lt 47 -or $top -gt 193){throw "Command bar opens $top DIP down, outside 48-192"}
 if(!(Row 0)){throw 'Opening did not show recent or default results'}
 $checks.open_centered_and_focused='passed'
 Capture 'open'

 Query 'brush size'
 Wait-Until {(Row 0) -and (Row 0).Current.Name -match 'size'} 'Brush size did not rank first' 5
 $detail=(Find 'command-search-detail').Current.Name
 if($detail -notmatch 'Current' -or $detail -notmatch 'Range'){throw "Parameter help did not describe the value and range: $detail"}
 Capture 'query'
 Key 0x0D
 Wait-Until {!(Visible 'command-result-0') -and (Visible 'command-search')} 'Enter did not open parameter entry' 5
 Key 0x1B
 Wait-Until {(Row 0) -and (Find 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq 'brush size'} 'Escape did not return from parameter entry to the query' 5
 Key 0x0D
 Wait-Until {!(Visible 'command-result-0')} 'Enter did not reopen parameter entry' 5
 Query '30'
 Key 0x0D
 Closed
 Wait-Until {(Size) -eq 30} 'Parameter entry did not set the brush size' 5
 if([System.Windows.Automation.AutomationElement]::FocusedElement.Current.Name -ne 'Drawing canvas'){throw 'Closing did not restore canvas focus'}
 $checks.parameter_entry_and_back='passed'

 Open-Search
 if((Find 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -ne ''){throw 'Reopening did not start with an empty query'}
 Query 'layer'
 Wait-Until {(Row 1)} 'Layer query did not show several results' 5
 Key 0x28
 Wait-Until {(Row 1).Current.ItemStatus -eq 'Selected' -and (Row 0).Current.ItemStatus -ne 'Selected'} 'Down did not move the selection' 5
 Key 0x26
 Wait-Until {(Row 0).Current.ItemStatus -eq 'Selected'} 'Up did not move the selection' 5
 $checks.keyboard_selection='passed'
 Query 'zzqqxx'
 Wait-Until {!(Row 0) -and (Find 'No matching commands' -Name)} 'Empty results did not explain themselves' 5
 Key 0x1B
 Closed
 $checks.escape_closes='passed'

 $modified=(Model).state.document_file.modified
 Open-Search
 $point=Canvas-Point .5 .8
 [CapyRowPointer]::Down('mouse',$point[0],$point[1]);[CapyRowPointer]::Move($point[0]+40,$point[1]);[CapyRowPointer]::Up()
 Closed
 Start-Sleep -Milliseconds 300
 if((Model).state.document_file.modified -ne $modified){throw 'Outside dismissal painted on the canvas'}
 $checks.outside_dismissal_does_not_paint='passed'

 Open-Search
 Query 'fit canvas'
 Wait-Until {(Row 0) -and (Row 0).Current.Name -match 'Fit'} 'Fit canvas did not rank first' 5
 $row=(Row 0).Current.BoundingRectangle
 [CapyRowPointer]::Down('touch',[int]($row.X+$row.Width/2),[int]($row.Y+$row.Height/2));[CapyRowPointer]::Up()
 Closed
 $checks.touch_activation='passed'

 Open-Search
 Query 'redo'
 Wait-Until {(Row 0) -and (Row 0).Current.Name -match 'Redo'} 'Redo did not rank first' 5
 $help=(Row 0).Current.HelpText;$detail=(Find 'command-search-detail').Current.Name
 if(!$help -or $detail -ne $help){throw "Unavailable Redo did not explain itself in the footer: '$detail' vs '$help'"}
 Key 0x0D
 Wait-Until {(Visible 'command-search')} 'Disabled command closed the search' 5
 Key 0x1B
 Closed
 $checks.disabled_reason='passed'

 Invoke-Id 'panel-tab-palettes'
 $swatch=@{item=$null}
 Wait-Until {
  $swatch.item=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|
   Where-Object {$_.Current.AutomationId.StartsWith('palette-swatch-') -and !$_.Current.IsOffscreen}|Select-Object -First 1
  $null -ne $swatch.item
 } 'Palette swatches did not appear' 10
 $swatch.item.SetFocus()
 Open-Search
 Query 'undo'
 Wait-Until {(Row 0) -and (Row 0).Current.Name -eq 'Undo Color Reorder'} 'Palette focus did not route Undo to color reorder history' 5
 Key 0x1B
 Closed
 Wait-Until {[System.Windows.Automation.AutomationElement]::FocusedElement.Current.AutomationId -eq $swatch.item.Current.AutomationId} 'Closing did not restore palette focus' 5
 (Find 'Drawing canvas' -Name).SetFocus()
 Open-Search
 Query 'undo'
 Wait-Until {(Row 0) -and (Row 0).Current.Name -eq 'Undo'} 'Canvas focus did not restore artwork Undo' 5
 Key 0x1B
 Closed
 $checks.palette_focus_scope='passed'

 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 $checks.evidence=$run
 [pscustomobject]$checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
} catch {
 if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
} finally {
 [CapyRowPointer]::Dispose()
 if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force}
 foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}
