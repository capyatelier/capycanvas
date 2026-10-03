param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$registry=[IO.File]::ReadAllText((Join-Path $repo 'crates/layer-ui/src/localization_languages.rs'))
$languages=@([Text.RegularExpressions.Regex]::Matches($registry,'\("[^"\r\n]+", "([^"\r\n]+)", "([^"\r\n]+)", "[^"\r\n]+"\)')|ForEach-Object {[pscustomobject]@{tag=$_.Groups[1].Value;name=$_.Groups[2].Value}})
if(!$languages.Count){throw 'Shared language registry is empty'}
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/localization/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $run 'profile'))|Out-Null
function Windows {try{(Get-Content (Join-Path $run "windows-$($review.Id).json") -Raw|ConvertFrom-Json).windows}catch{@()}}
function Model($Window=$current){try{(Get-Content (Join-Path $run "ui-state-$($review.Id)-$($Window.id).json") -Raw|ConvertFrom-Json).model}catch{$null}}
function Use-Window($Window){
 $script:current=$Window;$script:root=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Window.hwnd)
 [CapyRowPointer]::SetForegroundWindow([IntPtr]$Window.hwnd)|Out-Null
 Wait-Until {[CapyRowPointer]::GetForegroundWindow() -eq [IntPtr]$Window.hwnd} 'Review window did not own foreground input' 10
}
function Ready($Window){Wait-Until {(Model $Window).brush_ready -and (Model $Window).windows_workspace.ready} 'Window did not prepare' 120}
function Language-Choice([string]$Tag){
 $row=@((Model).preferences.pages.groups.rows|Where-Object id -eq 'language')[0]
 $name=if($Tag -eq 'system'){$row.kind.options[0]}else{(@($languages|Where-Object tag -eq $Tag)[0]).name}
 if(!$name){throw "Shared language registry has no tag: $Tag"}
 $index=[Array]::IndexOf([object[]]$row.kind.options,$name)
 if($index -lt 0){throw "Native language choices have no tag: $Tag"}
 $choice=$row.kind.options[$index]
 (Control 'preference-choice-language').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 (Control $choice -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
 $expected=if($Tag -eq 'system'){'en'}else{$Tag}
 Wait-Until {(Model).windows_active_tag -eq $expected} "Language preference did not publish $Tag" 30
}
try{
 Enter-CapyEnvironment
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Join-Path $env:CAPY_SETTINGS_DIRECTORY 'settings.json'),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle;Write-Output "Owned localization review $($review.Id): $run"
 Wait-Until {@(Windows).Count -eq 1} 'First window did not register' 45
 $first=@(Windows)[0];Ready $first;Use-Window $first
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'file'
 Invoke 'New Window' -Name
 Wait-Until {@(Windows).Count -eq 2} 'Second window did not register' 45
 $second=@(Windows|Where-Object id -ne $first.id)[0];Ready $second
 Use-Window $first;(Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17),78)
 $width=Control 'document-width';$identity=$width.GetRuntimeId() -join ':'
 $width.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('１２＋3 {draft}')
 $document=(Model $first).state.document_file;$gpu=(Model $first).windows_gpu_generation
 Use-Window $second
 if(Find 'settings-button' -Visible){Invoke 'settings-button'}else{
  & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'edit';Invoke 'Preferences' -Name
 }
 $language=Control 'preference-choice-language';$choiceIdentity=$language.GetRuntimeId() -join ':'
 $seen=@()
 foreach($tag in @($languages.tag)+@('en','system','ja')){
  Language-Choice $tag
  $expected=if($tag -eq 'system'){'en'}else{$tag}
  Wait-Until {(Model $first).windows_active_tag -eq $expected} "Inactive window did not adopt $tag" 30
  if(($language.GetRuntimeId() -join ':') -ne $choiceIdentity){throw 'Language choice replaced its native control'}
  if(($width.GetRuntimeId() -join ':') -ne $identity){throw 'Language publication replaced the dirty numeric field'}
  if($width.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -ne '１２＋3 {draft}'){throw 'Language publication changed the numeric draft'}
  if((Model $first).state.document_file.epoch -ne $document.epoch -or (Model $first).state.document_file.revision -ne $document.revision -or (Model $first).windows_gpu_generation -ne $gpu){throw 'Language publication changed the document or GPU owner'}
  $row=@((Model).preferences.pages.groups.rows|Where-Object id -eq 'language')[0]
  if($language.Current.Name -ne $row.title){throw 'Language accessibility name differs from the shared current copy'}
  $seen+=$tag
 }
 Invoke (@((Model).preferences.pages|Where-Object id -eq 'shortcuts')[0].title) -Name
 Wait-Until {(Model).preferences.page -eq 'shortcuts' -and (Find 'shortcuts-search')} 'Keyboard Shortcuts did not open'
 foreach($pair in @(@('shortcuts-search','Search shortcuts'),@('keymap-menu','Keymap options'))){
  $named=(Control $pair[0]).Current.Name
  if(!$named -or $named -eq $pair[1]){throw "$($pair[0]) kept its English name after the language changed"}
 }
 Use-Window $first
 $zoom=(Control 'canvas-view-info').Current.Name
 if(!$zoom -or $zoom -eq 'Zoom'){throw 'The zoom readout kept its English name after the language changed'}
 foreach($pair in @(@('canvas-action-bar','Canvas actions'),@('canvas-bar-more','More'),@('layer-add-mask','Add mask'),@('layer-delete','Delete selected layers'))){
  $named=(Control $pair[0]).Current.Name
  if(!$named -or $named -eq $pair[1]){throw "$($pair[0]) kept its English name after the language changed"}
 }
 foreach($english in @('Move panel group','Resize panel','Drawing workspace')){if(Find $english -Name){throw "$english kept its English name after the language changed"}}
 foreach($layer in @((Model $first).state.layers)){
  $label=Find "layer-$($layer.id)-label"
  if(!$label -or $label.Current.Name -ne $layer.label){throw "Layer row $($layer.id) lost its name after the language changed"}
 }
 Use-Window $second
 foreach($scene in @(@{window=$first;name='new-drawing'},@{window=$second;name='preferences'})){
  Use-Window $scene.window
  Start-Sleep -Milliseconds 250
  & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -WindowHandle $scene.window.hwnd -Output (Join-Path $run ($scene.name+'.png')) -ClientOnly -Composed *> (Join-Path $run ($scene.name+'-capture.json'))
 }
 Invoke 'CloseButton'
 Wait-Until {!(Model).preferences} 'Preferences did not close' 10
 Wait-Until {try{(Control 'drawing-canvas').SetFocus();$true}catch{$false}} 'Canvas did not become focusable after closing Preferences' 15
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17,16),78)
 Wait-Until {@(Windows).Count -eq 3} 'Future window did not register' 45
 $third=@(Windows|Where-Object {$_.id -ne $first.id -and $_.id -ne $second.id})[0];Ready $third
 if((Model $third).windows_active_tag -ne 'ja'){throw 'Future window used a stale language'}
 [pscustomobject]@{theme=$Theme;language_choices=$seen;retained_numeric_draft='passed';retained_preference_control='passed';inactive_window='passed';future_window='passed';document_and_gpu_generation='passed';scope='WinUI SDK/WARP controls and injected keys; genuine TSF and hardware frame pacing require Windows hardware';evidence=$run}|ConvertTo-Json -Depth 4|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force}
 Exit-CapyEnvironment
}
