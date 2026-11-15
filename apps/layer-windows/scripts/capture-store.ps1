param(
 [Parameter(Mandatory=$true)][string]$Recipe,
 [Parameter(Mandatory=$true)][string]$Executable,
 [Parameter(Mandatory=$true)][string]$Output,
 [ValidateSet('paint','sketch','photo')][string]$Scene='paint',
 [ValidateSet('light','dark')][string]$Theme='light',
 [string]$Locale='en',
 [int]$Width=2400,[int]$Height=1380
)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
$scripts=Join-Path $repo 'apps/layer-windows/scripts'
. (Join-Path $scripts 'CapyUia.ps1')
Add-Type -Path (Join-Path $scripts 'RowPointerDriver.cs')
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$CapyWaitSeconds=45
$Recipe=(Resolve-Path $Recipe).Path
$Executable=(Resolve-Path $Executable).Path
$job=Get-Content $Recipe -Raw -Encoding UTF8|ConvertFrom-Json
$selected=@($job.scenes|Where-Object id -eq $Scene)
if($selected.Count -ne 1){throw 'Select one scene from the staged native store recipe'}
$selected=$selected[0]
function Recipe-Keys($Value,[string[]]$Keys){
 if((@($Value.PSObject.Properties.Name|Sort-Object) -join ',') -cne (@($Keys|Sort-Object) -join ',')){throw 'Unsupported native recipe fields'}
}
Recipe-Keys $selected @('id','workspace','source','sourceSha256','steps')
$sequence=@(foreach($recipeStep in $selected.steps){
 switch($recipeStep.type){
 'action' {
 Recipe-Keys $recipeStep @('type','action')
 switch($recipeStep.action.type){
 'invoke' {Recipe-Keys $recipeStep.action @('type','command');if($recipeStep.action.command -notin @('brush','tonal_select')){throw 'Unsupported native command'}}
 'select_brush_set' {Recipe-Keys $recipeStep.action @('type','group');if($recipeStep.action.group -ne 'oil'){throw 'Unsupported native brush set'}}
 'select_brush' {Recipe-Keys $recipeStep.action @('type','id');if($recipeStep.action.id -ne 22){throw 'Unsupported native brush preset'}}
 {$_ -in @('set_brush_size','set_brush_opacity')} {Recipe-Keys $recipeStep.action @('type','value');if(!(($recipeStep.action.value -is [ValueType]) -and $recipeStep.action.value -gt 0)){throw 'Invalid native brush value'}}
 'set_color' {Recipe-Keys $recipeStep.action @('type','rgba');if($recipeStep.action.rgba.Count -ne 4 -or $recipeStep.action.rgba[3] -ne 1 -or @($recipeStep.action.rgba|Where-Object {$_ -lt 0 -or $_ -gt 1}).Count){throw 'Unsupported native color'}}
 default {throw 'Unsupported native recipe action'}
 };'action:'+($recipeStep.action.type)+$(if($recipeStep.action.type -eq 'invoke'){':'+$recipeStep.action.command})
 }
 'frame' {Recipe-Keys $recipeStep @('type','zoom','focus');if($recipeStep.zoom -ne .96 -or $recipeStep.focus.Count -ne 2){throw 'Unsupported native framing adaptation'};'frame'}
 'show_panel' {Recipe-Keys $recipeStep @('type','panel');if($recipeStep.panel -notin @('color','properties')){throw 'Unsupported native panel'};'show_panel'}
 'select_layer' {Recipe-Keys $recipeStep @('type','name');'select_layer'}
 'layer_visibility' {Recipe-Keys $recipeStep @('type','name','visible');if($recipeStep.visible -isnot [bool]){throw 'Invalid layer visibility'};'layer_visibility'}
 'wait_histogram' {Recipe-Keys $recipeStep @('type');'wait_histogram'}
 'effect' {
 Recipe-Keys $recipeStep @('type','effect','parameters')
 if($recipeStep.effect -eq 'curves'){
 Recipe-Keys $recipeStep.parameters @('rgb');Recipe-Keys $recipeStep.parameters.rgb @('kind','value')
 $points=$recipeStep.parameters.rgb.value
 if($recipeStep.parameters.rgb.kind -ne 'curve' -or $points.Count -lt 2 -or ($points[0] -join ',') -ne '0,0' -or ($points[-1] -join ',') -ne '1,1'){throw 'Unsupported native curve endpoints'}
 foreach($point in $points){if($point.Count -ne 2 -or @($point|Where-Object {$_ -lt 0 -or $_ -gt 1}).Count){throw 'Invalid native curve point'}}
 }elseif($recipeStep.effect -eq 'vibrance'){
 Recipe-Keys $recipeStep.parameters @('vibrance');Recipe-Keys $recipeStep.parameters.vibrance @('kind','value')
 if($recipeStep.parameters.vibrance.kind -ne 'number'){throw 'Unsupported native vibrance value'}
 }else{throw 'Unsupported native effect'}
 'effect:'+($recipeStep.effect)
 }
 default {throw 'Unsupported native recipe step'}
 }
}) -join ','
$sequences=@{
 paint='action:invoke:brush,action:select_brush_set,action:select_brush,action:set_brush_size,action:set_brush_opacity,action:set_color,show_panel,select_layer,layer_visibility,layer_visibility'
 sketch='frame,action:set_color'
 photo='select_layer,action:set_color,effect:curves,effect:vibrance,show_panel,action:invoke:tonal_select,wait_histogram'
}
if($sequence -cne $sequences[$Scene]){throw 'Unsupported native recipe step ordering'}
function Recipe-Action([string]$Type){@($selected.steps|Where-Object {$_.type -eq 'action' -and $_.action.type -eq $Type})[0].action}
function Recipe-Color{$rgba=(Recipe-Action 'set_color').rgba;'#'+(($rgba[0..2]|ForEach-Object {([int][Math]::Round($_*255)).ToString('x2')}) -join '')}
$captureRoot=[IO.Path]::GetFullPath($Output)
$base=Join-Path $captureRoot $Locale
. (Join-Path $scripts 'LocalizationProfiles.ps1')
$run=Join-Path $base ($Scene+'-'+$Theme+'-run')
if(Test-Path $run){throw 'Use a new capture output directory'}
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$directory=Split-Path $Executable
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class StoreCaptureWindow {
[DllImport("user32.dll")]public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int v,bool repaint);
}
'@
function Menu-Label([string]$Id){@((Model).application_menus|Where-Object id -eq $Id)[0].label}
function Open-StoreProject([string]$Path){
 & (Join-Path $script:CapyScripts 'open-application-menu.ps1') -Root $root -Name 'File' -Caption (Menu-Label 'file')
 Invoke-Id 'open_document'
 $hit=@{edit=$null}
 Wait-Until {$hit.edit=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit'),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')));$null -ne $hit.edit} 'Missing Open picker' 45
 [CapyWindowApi]::Path([IntPtr]$hit.edit.Current.NativeWindowHandle,$Path)
 Invoke-PickerButton (Find 'Open' -Name)
 Wait-Until {!(Find 'Open' -Name)} 'Open did not finish' 90
}
function Edit([string]$Id,[string]$Text){
 $entry=Control $Id;$entry.SetFocus()
 Wait-Until {$entry.Current.HasKeyboardFocus} "Native editor did not focus: $Id"
 $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
 Wait-Until {$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $Text} "Native editor did not accept text: $Id"
}
function Set-Color([string]$Hex){
 $drawer=$false;$panel=$false
 if(!(Find 'color-edit' -Visible)){
 $color=@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.control.kind -eq 'color'})[0]
 if(!$color){
 & (Join-Path $scripts 'open-application-menu.ps1') -Root $root -Name 'Window' -Caption (Menu-Label 'window')
 Invoke (Panel-Label 'color') -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
 $panel=$true
 }else{
 $r=(Control ('header-item-'+$color.id)).Current.BoundingRectangle
 [CapyRowPointer]::Down('mouse',[int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2));[CapyRowPointer]::Up();$drawer=$true
 }
 Wait-Until {Find 'color-edit' -Visible} 'Header Color drawer did not open'
 }
 Invoke-Id 'color-edit';Wait-Until {Find 'edit-color-apply'} 'Color editor did not open'
 Invoke-Id 'edit-color-hex';Edit 'edit-color-hex-entry' $Hex
 [CapyRowPointer]::Key([uint32]$review.Id,0x0d);Invoke-Id 'edit-color-apply'
 Wait-Until {!(Find 'edit-color-apply')} 'Color editor did not close'
 if($drawer){[CapyRowPointer]::Key([uint32]$review.Id,0x1b)}
 if($panel){
 & (Join-Path $scripts 'open-application-menu.ps1') -Root $root -Name 'Window' -Caption (Menu-Label 'window')
 Invoke (Panel-Label 'color') -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
 }
}
function Select-Panel([string]$Id){
 $icon='column-icon-'+$Id
 if(Find $icon -Visible){
 $r=(Control $icon -Arranged).Current.BoundingRectangle
 $previousPopups=$script:CapyPopups;$script:CapyPopups=$true
 try{
 [CapyRowPointer]::RightClick([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))
 Invoke (Catalog-Text $Locale 'workspace-expand-column-menu' 'workspace') -Name
 Wait-Until {!@((Model).layout.collapsed_columns|Where-Object {@($_.groups.icons.panel) -contains $Id}).Count} 'Native panel column did not expand'
 }finally{$script:CapyPopups=$previousPopups}
 }
 if(@((Model).layout.groups|Where-Object {$_.active -eq $Id}).Count -eq 0){Invoke-Id $(if(Find ('drawer-tab-'+$Id)){'drawer-tab-'+$Id}else{'panel-tab-'+$Id})}
 Wait-Until {@((Model).layout.groups|Where-Object {$_.active -eq $Id}).Count -gt 0} "Panel $Id did not become active"
}
function Panel-Label([string]$Id){
 $menu=@((Model).application_menus|Where-Object id -eq 'window')[0]
 @($menu.model.sections|ForEach-Object {$_}|Where-Object {$_.action.type -eq 'customize' -and $_.action.action.type -eq 'set_panel_visible' -and $_.action.action.panel -eq $Id})[0].label
}
function Select-Filter([string]$Id){
 $menu=(Model).state.layer_tools.add_filter
 $hit=@{category=$null;label=$null}
 foreach($section in $menu.sections){foreach($category in $section){foreach($sub in $category.sections){foreach($item in $sub){if($item.action.action.effect -eq $Id){$hit.category=$category.label;$hit.label=$item.label}}}}}
 if(!$hit.label){throw "No native filter menu action for $Id"}
 Invoke-Id 'layer-add-filter'
 (Control $hit.category -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 Invoke $hit.label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
 Wait-Until {(Model).state.layer_properties.description -eq $hit.label} 'Inserted native filter did not become active'
 Select-Panel 'properties'
}
function Finite([double]$Value){![double]::IsNaN($Value) -and ![double]::IsInfinity($Value)}
function Graph-Height{200*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.}
function Show-Graph([double]$Percent=0){
    $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$node=$walker.GetParent((Control 'property-rgb-curve'))
    while($node){$scroll=$null;if($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll) -and $scroll.Current.VerticallyScrollable){break};$node=$walker.GetParent($node)}
    if($node -and $scroll.Current.VerticalScrollPercent -ne $Percent){$scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,$Percent)}
    $settled=@{bounds=$null;scroll=-1;visible=$false;enabled=$false}
    try{Wait-Until {
        $graph=Control 'property-rgb-curve';$bounds=$graph.Current.BoundingRectangle
        $visible=!$bounds.IsEmpty -and (Finite $bounds.X) -and (Finite $bounds.Y) -and (Finite $bounds.Width) -and (Finite $bounds.Height) -and $bounds.Width -gt 0 -and $bounds.Height -gt (6*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.) -and !$graph.Current.IsOffscreen
        $same=$visible -and $graph.Current.IsEnabled -and $bounds -eq $settled.bounds
        if(!$visible -and $node -and $scroll.Current.VerticallyScrollable){
            $currentPercent=$scroll.Current.VerticalScrollPercent
            if($Percent -ge 50 -and $currentPercent -gt 0){$scroll.Scroll([System.Windows.Automation.ScrollAmount]::NoAmount,[System.Windows.Automation.ScrollAmount]::SmallDecrement)}
            elseif($Percent -lt 50 -and $currentPercent -lt 100){$scroll.Scroll([System.Windows.Automation.ScrollAmount]::NoAmount,[System.Windows.Automation.ScrollAmount]::SmallIncrement)}
        }
        $settled.bounds=$bounds;$settled.visible=$visible;$settled.enabled=$graph.Current.IsEnabled;if($scroll){$settled.scroll=$scroll.Current.VerticalScrollPercent}
        $same
    } 'The visible curve graph did not settle after scrolling'}
    catch{throw "Curve graph did not settle: bounds=$($settled.bounds), scroll=$($settled.scroll), visible=$($settled.visible), enabled=$($settled.enabled)"}
}
function Property([string]$Key){(Model).state.layer_properties.controls|Where-Object key -eq $Key}
function Set-Curves{
 $curve=@($selected.steps|Where-Object {$_.type -eq 'effect' -and $_.effect -eq 'curves'})[0].parameters.rgb.value
 foreach($point in @($curve|Select-Object -Skip 1|Select-Object -SkipLast 1)){
 Show-Graph $(if($point[1] -lt .5){100}else{0})
 $r=(Control 'property-rgb-curve').Current.BoundingRectangle;$top=if($point[1] -lt .5){$r.Bottom-(Graph-Height)}else{$r.Y}
 $x=[int]($r.X+$point[0]*$r.Width);$y=[int]($top+(1-$point[1])*(Graph-Height))
 $beforeCount=(Property 'rgb').value.value.Count
 [CapyRowPointer]::Down('mouse',$x,$y);[CapyRowPointer]::Up()
 Wait-Until {$p=Property 'rgb';$null -ne $p.curve.selected -and $p.value.value.Count -eq $beforeCount+1 -and [Math]::Abs($p.value.value[$p.curve.selected][0]-$point[0]) -lt .02 -and [Math]::Abs($p.value.value[$p.curve.selected][1]-$point[1]) -lt .02} 'Inserted curve point did not select'
 Edit 'property-rgb-input' ([string]($point[0]*255));[CapyRowPointer]::Key([uint32]$review.Id,0x0d)
 Wait-Until {[Math]::Abs((Property 'rgb').curve.input.value-$point[0]) -lt 0.00001} 'Native curve input did not commit'
 Edit 'property-rgb-output' ([string]($point[1]*255));[CapyRowPointer]::Key([uint32]$review.Id,0x0d)
 Wait-Until {[Math]::Abs((Property 'rgb').curve.output.value-$point[1]) -lt 0.00001} 'Native curve output did not commit'
 }
}
function Select-Command([string]$Command){
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17),0x4b)
 $label=@((Model).state.commands|Where-Object id -eq $Command)[0].label
 Edit 'command-search' $label
 Wait-Until {$result=Find 'command-result-0';$result -and $result.Current.IsEnabled -and $result.Current.Name -eq $label} "Command search has no $Command"
 Invoke 'command-result-0'
 Wait-Until {!(Find 'command-search') -and @((Model).state.commands|Where-Object {$_.id -eq $Command -and $_.selected}).Count -eq 1} "Native command did not activate: $Command"
}
function Select-ShowcaseBrush{
 if((Recipe-Action 'select_brush').id -ne 22 -or (Recipe-Action 'select_brush_set').group -ne 'oil'){throw 'The native showcase adapter supports Loaded Oil; extend it for a new recipe preset'}
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x4b)
 Wait-Until {Find 'command-search' -Visible} 'Command search did not open'
 Edit 'command-search' (Catalog-Text $Locale 'brush-preset-loaded-oil' 'tools')
 $hit=@{index=-1}
 Wait-Until {
 $search=Search-State
 for($i=0;$i -lt @($search.results).Count;$i++){
 $item=$search.results[$i];if($item.id -notlike 'action:*'){continue};$action=$item.id.Substring(7)|ConvertFrom-Json
 if($action.type -eq 'select_brush' -and $action.id -eq 22){$hit.index=$i;return $true}
 };$false
 } 'Loaded Oil command did not appear'
 Invoke-Id ('command-result-'+$hit.index)
 Wait-Until {(Model).state.brush.preset -eq 22 -and (Model).brush_ready} 'Loaded Oil did not select' 120
}
$profileDirectory=$null
try{
 Enter-CapyEnvironment
 $profileDirectory=Join-Path $run ('profile-'+[guid]::NewGuid().ToString('N'));$env:CAPY_STORAGE_DIR=$profileDirectory
 $env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 [IO.File]::WriteAllText((Settings-File),(@{theme=$Theme;language=@{Explicit=$Locale}}|ConvertTo-Json -Depth 5))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Wait-Until {$script:drawing=Owned-DrawingWindow $review;$drawing -and (Model).canvas_ready -and (Review-Ready)} 'Native editor did not start' 120
 $root=$drawing.Root;$drawingWindow=$drawing.Handle
 Wait-Until {(Model).windows_active_tag -eq $Locale -and (Model).state.settings.language.Explicit -eq $Locale} 'Native app did not load requested locale'
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 [StoreCaptureWindow]::MoveWindow($drawing.Handle,40,40,$Width,$Height,$true)|Out-Null
 $workspace=$selected.workspace
 Use-Workspace ('builtin:workspace:'+$workspace) (@{paint='Paint';sketch='Sketch';photo='Photo'}[$Scene])
 $source=$selected.source
 $sourcePath=[IO.Path]::GetFullPath((Join-Path (Split-Path $Recipe) $source))
 if((Get-FileHash $sourcePath -Algorithm SHA256).Hash.ToLower() -ne $selected.sourceSha256){throw 'Staged source artwork differs from its recipe'}
 Open-StoreProject $sourcePath
 Wait-Until {(Model).canvas_ready -and !(Model).state.document_file.busy -and @((Model).state.tabs|Where-Object {$_.active -and $_.title -like ([IO.Path]::GetFileNameWithoutExtension($source)+'*') -and $_.width -gt 0 -and $_.height -gt 0}).Count -eq 1} 'Imported artwork did not settle' 120
 Fit-Canvas
 foreach($step in $selected.steps){
 switch($step.type){
 'action' {
 switch($step.action.type){
 'invoke' {Select-Command $step.action.command}
 'select_brush_set' {Select-ShowcaseBrush}
 'select_brush' {Wait-Until {(Model).state.brush.preset -eq $step.action.id} 'Recipe brush preset did not become active'}
 'set_brush_size' {Edit 'tool-setting-size' ([string]$step.action.value);[CapyRowPointer]::Key([uint32]$review.Id,0x0d);Wait-Until {[Math]::Abs((Model).state.brush.diameter-$step.action.value) -lt .001} 'Recipe brush size did not commit'}
 'set_brush_opacity' {Edit 'tool-setting-opacity' ([string]($step.action.value*100));[CapyRowPointer]::Key([uint32]$review.Id,0x0d);Wait-Until {[Math]::Abs((Model).state.brush.opacity-$step.action.value) -lt .00001} 'Recipe brush opacity did not commit'}
 'set_color' {Set-Color (Recipe-Color);Wait-Until {$actual=(Model).state.brush.color;for($i=0;$i -lt 4;$i++){if([Math]::Abs($actual[$i]-$step.action.rgba[$i]) -gt .002){return $false}};$true} 'Recipe color did not commit'}
 }
 }
 'show_panel' {Select-Panel $step.panel}
 'select_layer' {
 $layer=@((Model).state.layers|Where-Object label -eq $step.name)
 if($layer.Count -ne 1){throw 'Recipe layer did not resolve uniquely'}
 Select-Panel 'layers';Invoke-Id ('layer-'+$layer[0].id+'-content')
 Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $layer[0].id} 'Recipe layer selection did not commit'
 }
 'layer_visibility' {
 $layer=@((Model).state.layers|Where-Object label -eq $step.name)
 if($layer.Count -ne 1){throw 'Recipe layer did not resolve uniquely'}
 if($layer[0].visible -ne $step.visible){Invoke-Id ('layer-'+$layer[0].id+'-visibility')}
 Wait-Until {@((Model).state.layers|Where-Object id -eq $layer[0].id)[0].visible -eq $step.visible} 'Recipe layer visibility did not commit'
 }
 'effect' {
 Select-Filter $step.effect
 if($step.effect -eq 'curves'){Set-Curves}else{
 Edit 'property-vibrance' ([string]$step.parameters.vibrance.value);[CapyRowPointer]::Key([uint32]$review.Id,0x0d)
 Wait-Until {[Math]::Abs((Property 'vibrance').value.value-$step.parameters.vibrance.value) -lt .001} 'Recipe vibrance did not commit'
 }
 }
 'frame' {
 foreach($command in @('actual_pixels','zoom_in')){
 & (Join-Path $scripts 'open-application-menu.ps1') -Root $root -Name 'View' -Caption (Menu-Label 'view')
 Invoke-Id $command
 Wait-Until {(Control 'drawing-canvas').Current.IsEnabled -and !(Find $command -Visible)} 'Sketch view command did not close'
 }
 $r=(Control 'drawing-canvas').Current.BoundingRectangle;$x=[int]($r.X+$r.Width/2);$y=[int]($r.Y+$r.Height/2)
 [CapyRowPointer]::MiddleDrag($x,$y,[int]($x+(@((Model).state.tabs|Where-Object active)[0].width/2-$step.focus[0])*(Model).state.camera.zoom),[int]($y+(@((Model).state.tabs|Where-Object active)[0].height/2-$step.focus[1])*(Model).state.camera.zoom))
 }
 'wait_histogram' {Wait-Until {$h=(Model).state.histogram;$null -ne $h.data -and $h.status -eq (Catalog-Text $Locale 'resources-histogram-exact' 'resources') -and (Model).windows_scopes -gt 0} 'Native histogram did not publish counts'}
 }
 }
 (Control 'drawing-canvas').SetFocus()
 Wait-Until {(Model).brush_ready -and !(Model).windows_workspace.busy} 'Scene did not settle' 90
 Wait-Until {
 foreach($layer in (Model).state.layers){
 if(!$layer.has_thumbnail){continue}
 $image=Find ('layer-'+$layer.id+'-thumbnail') -Visible
 if($image -and $image.Current.ItemStatus -ne (Catalog-Text $Locale 'native-header-ready' 'common')){return $false}
 };$true
 } 'Visible layer previews did not become ready' 120
 $m=Model
 if($Scene -eq 'paint' -and ($m.state.brush.preset -ne (Recipe-Action 'select_brush').id -or [Math]::Abs($m.state.brush.diameter-(Recipe-Action 'set_brush_size').value) -gt .001 -or [Math]::Abs($m.state.brush.opacity-(Recipe-Action 'set_brush_opacity').value) -gt .00001)){throw 'Capture brush recipe assertion failed'}
 $rgba=(Recipe-Action 'set_color').rgba
 for($i=0;$i -lt 4;$i++){if([Math]::Abs($m.state.brush.color[$i]-$rgba[$i]) -gt .002){throw 'Capture color recipe assertion failed'}}
 if($m.state.theme -ne $Theme){throw 'Capture theme assertion failed'}
 if($m.windows_active_tag -ne $Locale -or $m.state.settings.language.Explicit -ne $Locale){throw 'Capture locale assertion failed'}
 if(!$m.windows_adapter -or $m.windows_adapter -match '\(Cpu\)'){throw 'Native capture requires hardware adapter evidence'}
 if($m.error -or $m.state.host_error -or $m.windows_workspace.notice -or $m.windows_filter_load.error){throw 'Native capture has a notice or error'}
 $companions=@{}
 foreach($name in @('layer_windows.dll','capy-photo-reader.exe')){$path=Join-Path (Split-Path $Executable) $name;if(Test-Path $path){$companions[$name]=(Get-FileHash $path -Algorithm SHA256).Hash.ToLower()}elseif($name -eq 'layer_windows.dll'){throw 'Capture shared Rust DLL is missing'}}
 Capture ($Scene+'-'+$Theme) -Composed -WithModel
 Copy-Item (Join-Path $run ($Scene+'-'+$Theme+'.png')) (Join-Path $base ($Scene+'-'+$Theme+'.png'))
 [IO.File]::WriteAllText((Join-Path $base ($Scene+'-'+$Theme+'-receipt.json')),(@{language=$Locale;scene=$Scene;theme=$Theme;device='desktop';recipe_sha256=(Get-FileHash $Recipe -Algorithm SHA256).Hash.ToLower();executable_sha256=(Get-FileHash $Executable -Algorithm SHA256).Hash.ToLower();companion_sha256=$companions;adapter=$m.windows_adapter;canvas_ready=$m.canvas_ready;brush_ready=$m.brush_ready;native_adaptation=@{sketch_frame='actual_pixels then zoom_in; pan to recipe focus';brush_set='select named Loaded Oil preset from requested oil set';histogram='exact native histogram counts readiness'}}|ConvertTo-Json))
} catch {try{Capture 'failure' -Composed -WithModel}catch{};throw}
finally{
 try{if($review -and !$review.HasExited -and $drawing){& (Join-Path $scripts 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawing.Handle.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved}}
 finally{
 if($review -and !$review.HasExited){$review.Kill();$review.WaitForExit()}
 if($profileDirectory -and (Test-Path $profileDirectory)){Remove-Item -LiteralPath $profileDirectory -Recurse -Force}
 Exit-CapyEnvironment
 }
}
