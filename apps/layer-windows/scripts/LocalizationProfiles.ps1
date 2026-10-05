function Catalog-Text([string]$Tag,[string]$Id,[string]$Domain='color-features'){
 $source=[IO.File]::ReadAllText((Join-Path $repo ("assets/locales/$Tag/$Domain.ftl")))
 $match=[Text.RegularExpressions.Regex]::Match($source,'(?m)^'+[Text.RegularExpressions.Regex]::Escape($Id)+' = ([^\r\n]+)\r?$')
 if(!$match.Success){throw "Expected a simple canonical fixture message: $Id"};$match.Groups[1].Value
}
function Selected-Option($Control){@($Control.GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern).Current.GetSelection())[0]}
function Choose-Option([string]$Id,[string]$Name){
 $box=Control $Id;$pattern=$box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern);$pattern.Expand()
 (Control $Name -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
 Wait-Until {(Selected-Option (Control $Id)).Current.Name -eq $Name} 'Native option did not acknowledge the requested semantic selection' 15
 $current=(Control $Id).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
 if($current.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Expanded){$current.Collapse()}
}
function Check-Copy([string]$Id,[string]$Expected,[string]$Identity){
 $control=Control $Id
 if(($control.GetRuntimeId() -join ':') -ne $Identity -or $control.Current.Name -ne $Expected){throw "Retained native control/caption changed incorrectly: $Id"}
}
function Check-VisibleText([string]$Id,[string]$Expected){
 $control=Control $Id -Arranged;$current=$control.Current
 if($current.ControlType -eq [System.Windows.Automation.ControlType]::ComboBox){
  if($control.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::Collapsed){throw "Native selected-content check requires a closed combo: $Id"}
  $selected=@($control.GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern).Current.GetSelection())
  if($selected.Count -ne 1){throw "Native closed combo has no single selected content provider: $Id"}
  $content=$selected[0].Current;$bounds=$content.BoundingRectangle
  if($content.Name -ne $Expected -or $content.IsOffscreen -or $bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0 -or !$current.BoundingRectangle.Contains($bounds)){throw "Native visible selected content stayed stale for $Id; expected '$Expected', actual '$($content.Name)', offscreen=$($content.IsOffscreen), content=$bounds, combo=$($current.BoundingRectangle)"}
 }elseif($current.ControlType -eq [System.Windows.Automation.ControlType]::Text){
  if($current.Name -ne $Expected){throw "Native visible text stayed stale for $Id; expected '$Expected'"}
 }elseif(!(Find $Expected -Name -Within $control -Type ([System.Windows.Automation.ControlType]::Text) -Visible)){throw "Native visible content stayed stale for $Id; expected '$Expected'"}
}
function Surface-Languages([string]$Surface,[scriptblock]$Assert){
 foreach($choice in $seen){
  Use-Window $second;Language-Choice $choice.index|Out-Null
  Wait-Until {(Model $first).windows_active_tag -eq $choice.tag} 'Retained workflow did not adopt the new context' 30
  Use-Window $first;& $Assert
  Capture-Window $first ($choice.tag+'-'+$Surface)
 }
}
function Search-Command([string]$Query,[string]$Id){
 Open-Search;(Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Query)
 Wait-Until {(Search).query -eq $Query -and (Find 'command-result-0').Current.Name -eq ((Model).state.commands|Where-Object id -eq $Id).label} "Native search did not resolve $Id" 20
 Key 13
}
function Profile-Surfaces{
 $icc=Join-Path $run 'nameless-profile.icc';$photo=Join-Path $run 'Tiếng Việt İı ไทย source.png'
 [IO.File]::WriteAllBytes($icc,[Convert]::FromBase64String('AAACTGxjbXMEQAAAbW50clJHQiBYWVogB+oACgADAAYALwApYWNzcEFQUEwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAPbWAAEAAAAA0y1sY21zAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAALenp6egAAAQgAAAA2Y3BydAAAAUAAAABMd3RwdAAAAYwAAAAUY2hhZAAAAaAAAAAsclhZWgAAAcwAAAAUYlhZWgAAAeAAAAAUZ1hZWgAAAfQAAAAUclRSQwAAAggAAAAgZ1RSQwAAAggAAAAgYlRSQwAAAggAAAAgY2hybQAAAigAAAAkbWx1YwAAAAAAAAABAAAADGVuVVMAAAAaAAAAHABzAFIARwBCACAAYgB1AGkAbAB0AC0AaQBuAABtbHVjAAAAAAAAAAEAAAAMZW5VUwAAADAAAAAcAE4AbwAgAGMAbwBwAHkAcgBpAGcAaAB0ACwAIAB1AHMAZQAgAGYAcgBlAGUAbAB5WFlaIAAAAAAAAPbWAAEAAAAA0y1zZjMyAAAAAAABDEIAAAXe///zJQAAB5MAAP2Q///7of///aIAAAPcAADAblhZWiAAAAAAAABvoAAAOPUAAAOQWFlaIAAAAAAAACSfAAAPhAAAtsNYWVogAAAAAAAAYpcAALeHAAAY2XBhcmEAAAAAAAMAAAACZmYAAPKnAAANWQAAE9AAAApbY2hybQAAAAAAAwAAAACj1wAAVHsAAEzNAACZmgAAJmYAAA9c'))
 [IO.File]::WriteAllBytes($photo,[Convert]::FromBase64String('iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAABdWlDQ1BJQ0MgUHJvZmlsZQAAeJx1kT1LA0EQhh8TNRIVCy1ELK6IomBQFMRSY5EmSIgKfjWXM5cISTzuEiTaCjYWAQvRxq/Cf6CtYKsgCIogYm3pVyNyziaBBNE59ubh3X2H2VnwRNJGxqkfh0w2Z8fCIW1ufkHzveDHSyOD9OuGY01EoxH+jc876lS+Dapa/5/7M5rXJaCuSXjUsOycsHRDZC1nKd4W7jBS+rLwofCALQ0KXyk9XuZnxckyvyu2Z2KT4FE1tWQNx2vYSNkZ4T7hQCadNyr9qJu0JLKz05K7ZHXjECNMCI04eVZIkyMoOSsz+9s3VPJNsSoeQ/4WBWxxJEmJd0DUvFRNSDZFT8iXpqDm/nuejjkyXK7eEoKGJ9d96wHfDnwXXffryHW/j8H7CBfZqn9V5jT2IXqxqgUOoG0Tzi6rWnwXzreg88HSbb0keWV5TBNeT6F1HtpvwL9YnlVln5N7mNmQJ7qGvX3olfNtSz+jdWhi8y0eYwAAAZ1pVFh0WE1MOmNvbS5hZG9iZS54bXAAAAAAADx4OnhtcG1ldGEgeG1sbnM6eD0iYWRvYmU6bnM6bWV0YS8iPjxyZGY6UkRGIHhtbG5zOnJkZj0iaHR0cDovL3d3dy53My5vcmcvMTk5OS8wMi8yMi1yZGYtc3ludGF4LW5zIyI+PHJkZjpEZXNjcmlwdGlvbiB4bWxuczpkYz0iaHR0cDovL3B1cmwub3JnL2RjL2VsZW1lbnRzLzEuMS8iPjxkYzpjcmVhdG9yPjxyZGY6U2VxPjxyZGY6bGk+UHJpdmF0ZSBsb2NhbGl6YXRpb24gZml4dHVyZTwvcmRmOmxpPjwvcmRmOlNlcT48L2RjOmNyZWF0b3I+PGRjOnJpZ2h0cz48cmRmOkFsdD48cmRmOmxpIHhtbDpsYW5nPSJ4LWRlZmF1bHQiPkdlbmVyYXRlZCB0ZXN0IGltYWdlPC9yZGY6bGk+PC9yZGY6QWx0PjwvZGM6cmlnaHRzPjwvcmRmOkRlc2NyaXB0aW9uPjwvcmRmOlJERj48L3g6eG1wbWV0YT59360kAAAAGUlEQVR4nGOUTDnBQApgIkn1qIZRDUNKAwAUYwFl4JulLgAAAABJRU5ErkJggg=='))
 Use-Window $first;Menu-Command 'view' 'soft_proof'
 Wait-Until {(Find 'proof-panel-mode')} 'Retained native Proof panel did not open' 30
 if(!(Find 'proof-panel-setup' -Visible)){Invoke-Id 'proof-panel-mode-print'}
 Wait-Until {$view=Model;$view -and $view.windows_proof_form.mode -eq 'print' -and (Find 'proof-panel-setup' -Visible).Current.IsEnabled} 'Native Print mode did not acknowledge an enabled setup control' 30
 Control 'proof-panel-setup' -Arranged|Out-Null
 Invoke-Id 'proof-panel-setup'
 Wait-Until {(Model).windows_document.kind -eq 'proof' -and (Find 'proof-profile')} 'Retained proof setup did not open' 45
 Invoke (Fresh-Model).windows_document.details.feature_copy.profile.add_profile_dialog -Name
 Choose-Path $icc
 Wait-Until {(Find 'proof-profile') -and @((Model).windows_document.details.profiles).Count -eq 1} 'Private nameless ICC import did not return to proof setup' 45
 $entry=(Fresh-Model).windows_document.details.profiles[0];$entryId=$entry.id
 Choose-Option 'proof-profile' $entry.name
 $proof=Control 'proof-profile';$proofIdentity=$proof.GetRuntimeId() -join ':'
 $intent=Control 'proof-intent';$intentIdentity=$intent.GetRuntimeId() -join ':'
 $profileOptionIdentity=(Selected-Option $proof).GetRuntimeId() -join ':';$intentOptionIdentity=(Selected-Option $intent).GetRuntimeId() -join ':';$simulationOptionIdentity=(Selected-Option (Control 'proof-simulation')).GetRuntimeId() -join ':'
 $settings=(Fresh-Model).windows_document.details.settings|ConvertTo-Json -Depth 30 -Compress
 $document=(Fresh-Model).state.document_file|ConvertTo-Json -Depth 20 -Compress;$gpu=(Fresh-Model).windows_gpu_generation
 Surface-Languages 'retained-proof' {
  $view=Fresh-Model $first $choice.tag;$details=$view.windows_document.details;$entry=@($details.profiles|Where-Object id -eq $entryId)[0]
  Check-Copy 'proof-profile' $details.form.copy.profile $proofIdentity
  Check-Copy 'proof-intent' $details.form.copy.intent $intentIdentity
  $profileCaption=Catalog-Text $choice.tag 'color-features-profile-embedded';$intentCaption=Catalog-Text $choice.tag 'color-features-proof-relative';$simulationCaption=Catalog-Text $choice.tag 'color-features-proof-black-ink'
  foreach($pair in @(@('proof-profile',$profileCaption,$profileOptionIdentity),@('proof-intent',$intentCaption,$intentOptionIdentity),@('proof-simulation',$simulationCaption,$simulationOptionIdentity))){if((Selected-Option (Control $pair[0])).Current.Name -ne $pair[1] -or ((Selected-Option (Control $pair[0])).GetRuntimeId() -join ':') -ne $pair[2]){throw 'Native Proof selected option lost its identity or current canonical caption'};Check-VisibleText $pair[0] $pair[1]}
  if((Selected-Option (Control 'proof-profile')).Current.Name -ne $entry.name -or ($details.settings|ConvertTo-Json -Depth 30 -Compress) -ne $settings -or ($view.state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $document -or $view.windows_gpu_generation -ne $gpu){throw 'Language switching changed retained proof selection, raw settings or document/GPU ownership'}
 }
 Invoke (Fresh-Model).windows_document.details.feature_copy.profile.manage -Name
 Wait-Until {(Find 'document-profile-library')} 'Native profile library did not open' 30
 $libraryIdentity=(Control 'document-profile-library').GetRuntimeId() -join ':'
 Surface-Languages 'retained-profile-library' {
  $details=(Fresh-Model $first $choice.tag).windows_document.details;$entry=@($details.profiles|Where-Object id -eq $entryId)[0]
  Check-Copy 'document-profile-library' $details.feature_copy.profile.library_title $libraryIdentity
  if((Control 'profile-library-help').Current.Name -ne (Catalog-Text $choice.tag 'color-features-profile-library-help')){throw 'Profile library help stayed in an earlier language'}
  $expected=$entry.name+' · '+$entry.state;if($entry.issue){$expected+=' · '+$entry.issue}
  if((Selected-Option (Control 'document-profile-library')).Current.Name -ne $expected){throw 'Profile metadata row did not use the current shared projected visibility'}
  if($entry.name -ne (Catalog-Text $choice.tag 'color-features-profile-embedded')){throw 'Nameless ICC fallback was stored as a previous localized literal'}
  Check-VisibleText 'document-profile-library' $expected
 }
 Invoke 'CloseButton';Wait-Until {(Find 'proof-profile')} 'Profile library did not return to the retained proof draft' 30
 Invoke 'CloseButton';Wait-Until {$view=Model;$view -and !$view.windows_document} 'Proof draft did not cancel' 30
 Menu-Command 'file' 'open_document';Choose-Path $photo
 Wait-Until {$view=Model;$view -and $view.state.layer_tools.editing_layer.label -eq [IO.Path]::GetFileNameWithoutExtension($photo) -and (($view.state.commands|Where-Object id -eq 'repair_source_profile').enabled -or ($view.state.commands|Where-Object id -eq 'apply_transform').enabled)} 'Private source photo did not become editable' 90
 if(((Fresh-Model).state.commands|Where-Object id -eq 'apply_transform').enabled){Search-Command 'apply transform' 'apply_transform'}
 Wait-Until {((Model).state.commands|Where-Object id -eq 'repair_source_profile').enabled} 'Imported source did not expose profile repair' 45
 Search-Command 'repair source profile' 'repair_source_profile'
 Wait-Until {(Find 'document-source-profile') -and (Find 'document-profile-choice')} 'Source repair dialog did not open' 45
 $sourceIdentity=(Control 'document-source-profile').GetRuntimeId() -join ':';$choiceIdentity=(Control 'document-profile-choice').GetRuntimeId() -join ':'
 $sourceDocument=(Fresh-Model).state.document_file|ConvertTo-Json -Depth 20 -Compress
 Surface-Languages 'retained-source-profile' {
  $view=Fresh-Model $first $choice.tag;$details=$view.windows_document.details
  if(($view.state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $sourceDocument){throw 'Retained source repair edited document history during publication'}
  Check-Copy 'document-profile-choice' $details.feature_copy.color.current_source $choiceIdentity
  if($details.feature_copy.color.current_source -ne (Catalog-Text $choice.tag 'color-features-color-current-source')){throw 'Source profile row did not use the canonical current copy'}
  Check-VisibleText 'document-source-current-label' $details.feature_copy.color.current_source
  if((Control 'document-source-profile').Current.Name -ne $details.source_profile -or ((Control 'document-source-profile').GetRuntimeId() -join ':') -ne $sourceIdentity){throw 'Source profile caption or retained label did not match the current shared metadata'}
 }
 Invoke 'CloseButton';Wait-Until {$view=Model;$view -and !$view.windows_document} 'Source repair did not cancel' 30
 Menu-Command 'file' 'export_document'
 Wait-Until {(Find 'export-profile') -and (Find 'export-quality')} 'Retained export form did not open' 45
 if(!(Fresh-Model).windows_document.details.form.metadata){throw 'Private source image did not import the expected XMP metadata'}
 $exportIdentity=(Control 'export-profile').GetRuntimeId() -join ':';$qualityIdentity=(Control 'export-quality').GetRuntimeId() -join ':'
 Choose-Option 'export-format' (Catalog-Text (Fresh-Model).windows_active_tag 'color-features-export-format-jpeg')
 Scroll-Position (Control 'export-metadata') 100
 $metadata=Control 'export-metadata' -Arranged;$metadataIdentity=$metadata.GetRuntimeId() -join ':'
 $metadataOptionIdentity=(Selected-Option $metadata).GetRuntimeId() -join ':'
 $removeLocation=Control 'export-remove-location' -Arranged;$removeLocationIdentity=$removeLocation.GetRuntimeId() -join ':'
 $removeLocation.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {$removeLocation.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::Off} 'Dirty metadata location choice did not commit' 15
 $quality=Control 'export-quality';$qualityEntry=$quality.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Edit))
 if(!$qualityEntry -or !$qualityEntry.Current.IsEnabled){throw 'JPEG quality did not expose its retained native numeric editor'}
 $qualityEntry.SetFocus();$qualityEntry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('0')
 $preset=Control 'export-preset-name';Select-Draft $preset $literalName;$presetIdentity=$preset.GetRuntimeId() -join ':'
 Invoke 'PrimaryButton'
 Wait-Until {$view=Model;$view -and $view.windows_active_tag -and (Find 'export-validation').Current.Name -eq (Catalog-Text $view.windows_active_tag 'color-export-quality-range') -and $view.windows_document.stage -eq 'options'} 'Actual invalid JPEG quality did not retain a typed validation error' 30
 Select-Draft $preset $literalName
 $recipe=(Fresh-Model).windows_document.details.recipe|ConvertTo-Json -Depth 40 -Compress
 Surface-Languages 'retained-export' {
  $view=Fresh-Model $first $choice.tag;$details=$view.windows_document.details
  Check-Copy 'export-profile' $details.form.copy.profile $exportIdentity
  Check-Copy 'export-quality' $details.form.copy.quality $qualityIdentity
  Check-Copy 'export-metadata' (Catalog-Text $choice.tag 'color-features-export-metadata') $metadataIdentity
  Check-Copy 'export-remove-location' (Catalog-Text $choice.tag 'color-features-export-remove-location') $removeLocationIdentity
  if((Selected-Option (Control 'export-metadata')).Current.Name -ne (Catalog-Text $choice.tag 'color-features-export-metadata-all') -or ((Selected-Option (Control 'export-metadata')).GetRuntimeId() -join ':') -ne $metadataOptionIdentity -or (Control 'export-remove-location').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off){throw 'Metadata publication changed retained option identity, selection or dirty location value'}
  if((Control 'export-validation').Current.Name -ne (Catalog-Text $view.windows_active_tag 'color-export-quality-range') -or (Selected-Option (Control 'export-format')).Current.Name -ne (Catalog-Text $view.windows_active_tag 'color-features-export-format-jpeg') -or (Value $qualityEntry) -ne '0'){throw 'Retained export validation, dirty format or numeric value did not reproject without editing the draft'}
  if(($details.recipe|ConvertTo-Json -Depth 40 -Compress) -ne $recipe -or (Value (Control 'export-preset-name')) -ne $literalName -or ((Control 'export-preset-name').GetRuntimeId() -join ':') -ne $presetIdentity -or (Selection (Control 'export-preset-name')) -ne $literalName){throw 'Export publication changed retained raw recipe or Unicode preset draft/selection'}
 }
 Invoke 'CloseButton';Wait-Until {$view=Model;$view -and !$view.windows_document} 'Export options did not cancel' 30
 Use-Window $textWindow
}
