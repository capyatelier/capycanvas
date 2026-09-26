param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/prediction/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_LATENCY_TRACE','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
 try {$value=Get-Content -LiteralPath (Join-Path $run 'ui-state.json') -Raw|ConvertFrom-Json;if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){$value.model}}catch{}
}
function Row([string]$Title){@((Model).preferences.pages.groups.rows|Where-Object title -eq $Title)[0]}
function Control([string]$Title,[string]$Class){$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Title))|Where-Object {$_.Current.ClassName -eq $Class}|Select-Object -First 1}
function Switch([string]$Title){Control $Title 'ToggleSwitch'}
function Algorithms {
 $combo=Control 'Prediction algorithm' 'ComboBox'
 $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 Wait-Until {$combo.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'ComboBoxItem'))} 'Prediction algorithm list did not open'
 $items=@($combo.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'ComboBoxItem')))
 $items[0].GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
 $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Collapse()
 @($items|ForEach-Object {$_.Current.Name})
}
function Open-Preferences {
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
 Invoke-Id 'settings'
 Wait-Until {(Model).preferences} 'Preferences did not open'
 Wait-Until {(Find 'Pen & Input' -Name)} 'Input preferences tab did not appear'
 (Find 'Pen & Input' -Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {(Switch 'Use Windows stroke prediction')} 'Windows prediction switch not found'
}
function Close-Preferences {
 (Find 'CloseButton').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {!((Model).preferences)} 'Preferences did not close'
}
function Draw([string]$Device){
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 # Preferences and the input pane can move or resize the window. Read current
 # physical bounds after they close instead of reusing startup coordinates.
 Start-Sleep -Milliseconds 300
 $canvas=(Find 'Drawing canvas' -Name).Current.BoundingRectangle
 $model=Model;$area=$model.layout.work_area;$density=$canvas.Width/$model.layout.viewport[0]
 $x=[int]($canvas.X+($area.x+$area.width*.4)*$density);$y=[int]($canvas.Y+($area.y+$area.height*.5)*$density)
 [CapyRowPointer]::Down($Device,$x,$y)
 for($i=1;$i -le 60;$i++){[CapyRowPointer]::Move($x+2*$i,$y+[int](12*[Math]::Sin($i*.08)));Start-Sleep -Milliseconds 8}
 [CapyRowPointer]::Up()
 Start-Sleep -Milliseconds 200
}
try {
 foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_LATENCY_TRACE='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned prediction review $($review.Id): $run"
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
 . (Join-Path $repo 'tools/performance/windows-pen-ui.ps1') -ProcessId $review.Id
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Prediction review did not start'
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 Wait-Until {(Find 'tool-setting-size').Current.IsEnabled} 'Brush did not become editable'
 $size=Find 'tool-setting-size';$size.SetFocus();$size.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('18')
 (Find 'tool-setting-opacity').SetFocus()
 Open-Preferences
 $native=Row 'Use Windows stroke prediction'
 if(!$native.enabled -or !$native.kind.active){throw 'Native Windows predictor is not enabled by default'}
 if((Row 'Prediction amount').enabled){throw 'Manual prediction time should be disabled while Windows predicts'}
 $algorithm=Row 'Prediction algorithm'
 if($algorithm.enabled -or (Control 'Prediction algorithm' 'ComboBox').Current.IsEnabled){throw 'Prediction algorithm should be disabled while Windows predicts'}
 if(@($algorithm.kind.options).Count -ne 1 -or $algorithm.kind.options[0] -ne 'Smooth Motion (Optimized)' -or $algorithm.kind.selected -ne 0){throw "Unexpected prediction algorithms: $($algorithm.kind.options -join ', ')"}
 (Switch 'Use Windows stroke prediction').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {!(Row 'Use Windows stroke prediction').kind.active -and (Row 'Prediction amount').enabled} 'Native prediction switch did not select the engine fallback'
 Wait-Until {(Row 'Prediction algorithm').enabled -and (Control 'Prediction algorithm' 'ComboBox').Current.IsEnabled} 'Prediction algorithm did not enable for the engine fallback'
 $algorithms=@(Algorithms)
 if($algorithms.Count -ne 1 -or $algorithms[0] -ne 'Smooth Motion (Optimized)'){throw "Prediction algorithm list shows: $($algorithms -join ', ')"}
 Wait-Until {(Row 'Prediction algorithm').kind.selected -eq 0} 'Smooth Motion was not selected'
 Close-Preferences
 Draw 'pen'
 Open-Preferences
 if((Row 'Use Windows stroke prediction').kind.active){throw 'Reopening preferences lost the disabled prediction choice'}
 (Switch 'Use Windows stroke prediction').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Row 'Use Windows stroke prediction').kind.active -and !(Row 'Prediction amount').enabled} 'Native prediction did not enable'
 Close-Preferences
 foreach($device in @('mouse','touch','pen')){Draw $device}
 Open-Preferences
 $native=Row 'Use Windows stroke prediction'
 if(!$native.enabled -or !$native.kind.active){throw 'Mouse/touch/pen input disabled native prediction'}
 Close-Preferences
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
 $prediction=Get-ChildItem (Join-Path $run ('prediction-'+$review.Id+'-*.json'))|Get-Content -Raw|ConvertFrom-Json
 if(!$prediction -or $prediction.platform_prediction_frames -le 0 -or $prediction.engine_prediction_frames -le 0){throw 'The runtime did not render both native predictions and the engine fallback'}
 $saved=Get-Content -LiteralPath (Join-Path $env:CAPY_SETTINGS_DIRECTORY 'settings.json') -Raw|ConvertFrom-Json
 if($saved.prediction_algorithm -ne 'optimized' -or !$saved.platform_prediction){throw "Saved prediction settings: $($saved.prediction_algorithm), native $($saved.platform_prediction)"}
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 [pscustomobject]@{settings_toggle=$true;algorithm='optimized';survives_mouse_touch_pen=$true;prediction=$prediction;evidence=$run}|ConvertTo-Json -Depth 5|Tee-Object -FilePath (Join-Path $run 'results.json')
} finally {
 [CapyRowPointer]::Dispose()
 foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}
