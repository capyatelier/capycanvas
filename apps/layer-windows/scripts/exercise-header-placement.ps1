param([Parameter(Mandatory)][string]$Executable,[ValidateSet('light','dark')][string]$Theme='light')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class HeaderPlacementNative {
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left,top,right,bottom; }
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window,out Rect rect);
}
'@
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path;$directory=Split-Path $Executable
$run=Join-Path $repo ('artifacts/windows/header-placement/'+$Theme+'-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$script:HeaderPlacementScripts=$PSScriptRoot
$tokens=$null;$errors=$null
$helpers=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'exercise-header.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Header helpers failed parsing'}
foreach($name in @('Header','Presentation','Gesture','At','WindowCommand','Check-Geometry','Edit-Header','Entries')){
    $definition=$helpers.Find({param($node)$node-is[Management.Automation.Language.FunctionDefinitionAst]-and$node.Name-eq$name},$true)
    if(!$definition){throw "Missing existing header helper $name"};Invoke-Expression ($definition.Extent.Text.Replace('$PSScriptRoot','$script:HeaderPlacementScripts'))
}
$review=$null;$drawingWindow=[IntPtr]::Zero;$checks=[Collections.Generic.List[object]]::new();$samples=[Collections.Generic.List[object]]::new();$gaps=[Collections.Generic.List[string]]::new()
function Check($condition,[string]$name,$data){$checks.Add(@{name=$name;pass=[bool]$condition;data=$data});if(!$condition){throw $name}}
function Region-Key($regions){(@($regions)|ForEach-Object {'{0},{1},{2},{3}'-f$_.x,$_.y,$_.width,$_.height})-join';'}
function Header-Key($model){@($model.header.model,$model.titlebar_insets,$model.windows_tabs,$model.chrome_hidden,$model.keep_zen_button)|ConvertTo-Json -Depth 50 -Compress}
function Native-Box([string]$id,$origin,[double]$scale){
    $item=Find $id -Visible
    if(!$item){return @{visible=$false;bounds=@{x=0;y=0;width=0;height=0}}}
    $box=$item.Current.BoundingRectangle
    @{visible=(!$box.IsEmpty-and$box.Width-gt0);bounds=@{x=[single](($box.Left-$origin.Left)/$scale);y=[single](($box.Top-$origin.Top)/$scale);width=[single]($box.Width/$scale);height=[single]($box.Height/$scale)}}
}
function Caption-Sample([string]$phase){
    $bar=Control 'title-bar';$before=$bar.Current.ItemStatus;$model=Model;$modelKey=Header-Key $model
    $p=$before|ConvertFrom-Json
    if(!$p.PSObject.Properties['caption_regions']){throw 'Required caption_regions trace is absent'}
    $uiaOrigin=$bar.Current.BoundingRectangle;$point=[CapyRowPointer+Point]::new();$client=[HeaderPlacementNative+Rect]::new()
    if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$point)-or![HeaderPlacementNative]::GetClientRect($drawingWindow,[ref]$client)){throw 'Owned header client bounds unavailable'}
    $origin=[Windows.Rect]::new($point.x,$point.y,$client.right-$client.left,$client.bottom-$client.top)
    $scale=[single]([CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.)
    $actual=@($p.actual_items|ForEach-Object {@{id=$_.id;bounds=$_.bounds;visible=$true}})
    $overflow=@(0..2|ForEach-Object {Native-Box ('header-overflow-'+$_) $origin $scale})
    $recovery=Native-Box 'header-recovery-menu' $origin $scale
    $zen=@{visible=([bool]$model.keep_zen_button-or(!$model.chrome_hidden-and!$p.editing-and$null-ne$p.geometry.capy_item));bounds=$p.geometry.zen_button}
    $after=$bar.Current.ItemStatus;$current=Model
    if($before-cne$after-or$modelKey-cne(Header-Key $current)){return $null}
    $width=[int][math]::Round($origin.Width,[MidpointRounding]::AwayFromZero)
    $expected=@(Get-HeaderCaptionOracle -ActualItems $actual -Entries @($model.header.model.zones|ForEach-Object {$_}) -Overflow $overflow -Recovery $recovery -Zen $zen -SingleDocument (@($model.windows_tabs.tabs).Count-lt2) -Editing ([bool]$p.editing) -LeftInset $model.titlebar_insets[0] -RightInset $model.titlebar_insets[1] -Height $p.height -Scale $scale -Width $width)
    $sample=@{phase=$phase;expected=$expected;actual=$p.caption_regions;header=$p;model_key=$modelKey;overflow=$overflow;recovery=$recovery;zen=$zen;scale=$scale;origin=$origin;uia_origin=$uiaOrigin;width=$width;insets=$model.titlebar_insets;pass=((Region-Key $expected)-ceq(Region-Key $p.caption_regions))}
    $samples.Add($sample);$sample
}
function Check-Caption([string]$phase,[switch]$SkipGeometry){
    if(!$SkipGeometry){Check-Geometry};$holder=@{value=$null}
    Wait-Until {$holder.value=Caption-Sample $phase;$null-ne$holder.value} 'Caption sample did not remain stable across native reads'
    Check $holder.value.pass ($phase+' exact physical caption intervals') $holder.value
}
function Resize-Header([int]$width){
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    [CapyWindowExercise]::MoveWindow($drawingWindow,40,40,[int]($width*$scale),[int](700*$scale),$true)|Out-Null
    Wait-Until {[math]::Abs($root.Current.BoundingRectangle.Width-$width*$scale)-le2} 'Owned window width did not settle'
}
function Tap-Control([string]$id,[string]$device='mouse'){$at=At $id;[CapyRowPointer]::Down($device,$at.x,$at.y);[CapyRowPointer]::Up()}
function Capy-Check([string]$phase){
    $model=Model;$p=Presentation;$button=Control 'zen-capy' -Arranged;$spec=$model.header.sizes|Where-Object id -eq $p.geometry.size|Select-Object -First 1
    $image=$button.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Image))
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.;$bounds=$button.Current.BoundingRectangle
    $glyphWidth=[math]::Min($spec.capy_icon,$spec.tile-12)*$scale;$glyphHeight=[math]::Min($spec.capy_icon,$spec.tile)*$scale
    Check ($button.Current.Name-eq$model.header.capy.label-and$button.Current.IsEnabled-eq[bool]$model.header.capy.enabled-and[math]::Abs($bounds.Width-$spec.tile*$scale)-le1-and$image-and[math]::Abs($image.Current.BoundingRectangle.Width-$glyphWidth)-le1-and[math]::Abs($image.Current.BoundingRectangle.Height-$glyphHeight)-le1) ($phase+' Capy command, name, button and icon size') @{button=$bounds;image=if($image){$image.Current.BoundingRectangle}else{$null};spec=$spec;name=$button.Current.Name;enabled=$button.Current.IsEnabled;expected_glyph=@($glyphWidth,$glyphHeight)}
}
function Get-HeaderCaptionOracle {
    param($ActualItems,$Entries,$Overflow,$Recovery,$Zen,[bool]$SingleDocument,[bool]$Editing,[double]$LeftInset,[double]$RightInset,[double]$Height,[double]$Scale,[int]$Width)
    if($Editing){return @()}
    $controls=[Collections.Generic.List[object]]::new()
    foreach($item in $ActualItems){
        $entry=@($Entries|Where-Object id -eq $item.id)[0]
        if(!$entry){throw "Actual header item has no model identity: $($item.id)"}
        $kind=$entry.item.kind
        if($kind-in@('clock','battery','space')-or($kind-eq'document_title'-and$SingleDocument)){continue}
        if($item.visible-and$item.bounds.width-gt0){$controls.Add(@{left=[float]$item.bounds.x;right=[single]([single]$item.bounds.x+[single]$item.bounds.width)})}
    }
    foreach($item in @($Overflow)+@($Recovery)){
        if($item-and$item.visible-and$item.bounds.width-gt0){$controls.Add(@{left=[float]$item.bounds.x;right=[single]([single]$item.bounds.x+[single]$item.bounds.width)})}
    }
    if($Zen-and$Zen.visible-and$Zen.bounds.width-gt0){$controls.Add(@{left=[float]$Zen.bounds.x;right=[float]($Zen.bounds.x+$Zen.bounds.width)})}
    $next=[single]$LeftInset;$limit=[single]([single]([single]$Width/[single]$Scale)-[single]$RightInset)
    $result=[Collections.Generic.List[object]]::new()
    foreach($control in @($controls|Sort-Object @{Expression={[double]$_.left}},@{Expression={[double]$_.right}})){
        if($control.left-gt$next){
            $x=[int][math]::Ceiling([single]([single]$next*[single]$Scale));$end=[int][math]::Floor([single]([single][math]::Min($control.left,$limit)*[single]$Scale))
            if($end-gt$x){$result.Add(@{x=$x;y=0;width=$end-$x;height=[int][math]::Round([single]([single]$Height*[single]$Scale),[MidpointRounding]::AwayFromZero)})}
        }
        $next=[math]::Max($next,$control.right)
    }
    if($next-lt$limit){
        $x=[int][math]::Ceiling([single]([single]$next*[single]$Scale));$end=[int][math]::Floor([single]([single]$limit*[single]$Scale))
        if($end-gt$x){$result.Add(@{x=$x;y=0;width=$end-$x;height=[int][math]::Round([single]([single]$Height*[single]$Scale),[MidpointRounding]::AwayFromZero)})}
    }
    @($result.ToArray())
}
try{
    Enter-CapyEnvironment;$env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json))
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log');$null=$review.Handle
    $owned=@{window=$null};Wait-Until {$owned.window=Owned-DrawingWindow $review;$owned.window-and(Model).brush_ready-and(Model).windows_workspace.ready-and!(Model).windows_workspace.busy} 'Owned header placement fixture did not start' 90
    $root=$owned.window.Root;$drawingWindow=$owned.window.Handle
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width 1800 -Height 1050
    [CapyWindowApi]::ShowWindow($drawingWindow,9)|Out-Null;[CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id);Use-PaintWorkspace;Resize-Header 1200
    Check-Caption 'wide';Capy-Check 'wide'
    $retained=@{};foreach($id in @('zen-capy','workspace-switch-builtin:workspace:illustrator','settings-button')){$retained[$id]=(Control $id).GetRuntimeId()-join':'}
    $workspace=Control 'workspace-switch-builtin:workspace:illustrator' -Arranged;$workspace.SetFocus()
    Resize-Header 800;Check-Caption 'compact'
    $workspaceCompact=[bool](Find 'header-workspace-menu' -Visible)
    if($workspaceCompact){Check ((Control 'header-workspace-menu').Current.HasKeyboardFocus) 'Workspace compact transition preserves keyboard focus' $null}
    else{$gaps.Add('Fixture did not produce compact workspace fallback')}
    $fadeWatch=[Diagnostics.Stopwatch]::StartNew();$fadeSamples=0;$outgoingSamples=0
    Resize-Header 1200
    while($fadeWatch.ElapsedMilliseconds-lt700){
        $sample=Caption-Sample 'outgoing-overflow'
        if($sample){
            Check $sample.pass 'Outgoing transition exact caption sample' $sample;$fadeSamples++
            foreach($zone in 0..2){if($sample.overflow[$zone].visible-and!$sample.header.geometry.overflow[$zone]){$outgoingSamples++}}
        }
        Start-Sleep -Milliseconds 5
    }
    if(!$fadeSamples){$gaps.Add('No race-free outgoing fade sample was acquired')}
    if(!$outgoingSamples){$gaps.Add('No live outgoing overflow was observed before collapse; fade-only route remains unverified')}
    Check-Caption 'wide-restored'
    if($workspaceCompact){Check ((Control 'workspace-switch-builtin:workspace:illustrator').Current.HasKeyboardFocus) 'Workspace expansion restores selected keyboard focus' $null}
    foreach($id in $retained.Keys){Check (((Control $id).GetRuntimeId()-join':')-ceq$retained[$id]) ('Width changes retain '+$id) $null}
    if(@(Entries|Where-Object {$_.item.kind-eq'menu_labels'}).Count){
        $menuId='application-menu-'+@((Model).application_menus)[0].id
        (Control $menuId -Arranged).SetFocus();Resize-Header 800;Check-Caption 'menus-compact'
        $menuCompact=[bool](Find 'application-menus' -Visible)
        if($menuCompact){Check ((Control 'application-menus').Current.HasKeyboardFocus) 'Menu compact transition preserves keyboard focus' $null}
        else{$gaps.Add('Fixture did not produce the compact menu-label fallback')}
        Resize-Header 1200;Check-Caption 'menus-expanded'
        if($menuCompact){Check ((Control $menuId).Current.HasKeyboardFocus) 'Menu expansion restores keyboard focus' $null}
    }
    else{$gaps.Add('Default Paint has no menu-label item; compact menu-label focus remains unverified')}
    $originalSize=(Header).size
    foreach($size in @('small','medium','large')){
        Edit-Header;Check-Caption ('editing-'+$size)
        Invoke ('header-size-'+$size);Wait-Until {(Header).size-eq$size} 'Header size did not adopt'
        Check-Caption ('editing-sized-'+$size)
        Invoke 'header-edit-done';Wait-Until {!(Model).header.editing} 'Header editor did not finish'
        Check-Caption ('size-'+$size);Capy-Check ('size-'+$size)
    }
    Edit-Header;Invoke ('header-size-'+$originalSize);Invoke 'header-edit-done';Wait-Until {!(Model).header.editing} 'Original size did not restore'
    Edit-Header;Invoke 'header-edit-cancel';Wait-Until {!(Model).header.editing} 'Header editor did not cancel';Check-Caption 'editing-canceled';Capy-Check 'editing-canceled'
    foreach($device in @('mouse','pen','touch')){
        Tap-Control 'zen-capy' $device;Wait-Until {(Model).chrome_hidden} 'Physical Capy click did not enter Zen'
        Capy-Check ('zen-'+$device);Check-Caption ('zen-'+$device) -SkipGeometry
        Tap-Control 'zen-capy' $device;Wait-Until {!(Model).chrome_hidden} 'Physical kept Capy click did not restore chrome'
        Check-Caption ('restored-'+$device)
        $p=Presentation;$region=@($p.caption_regions|Where-Object width -gt 50|Sort-Object width -Descending)[0]
        if(!$region){throw 'No safe native caption interval for physical drag'}
        $origin=(Control 'title-bar').Current.BoundingRectangle;$from=@{x=[int]($origin.Left+$region.x+$region.width/2);y=[int]($origin.Top+$region.height/2)}
        $before=$root.Current.BoundingRectangle
        [CapyRowPointer]::Down($device,$from.x,$from.y)
        foreach($step in 1..8){[CapyRowPointer]::Move($from.x+4*$step,$from.y+2*$step);Start-Sleep -Milliseconds 12}
        [CapyRowPointer]::Up()
        Wait-Until {[math]::Abs($root.Current.BoundingRectangle.Left-$before.Left)-gt8} 'Physical caption contact did not move the owned window'
        Check $true ($device+' native caption drag') $null;Resize-Header 1200;Check-Caption ('moved-'+$device)
    }
    Capture 'header-final' -Composed -WithModel
    $status=Find 'canvas-status';$model=Model
    Check (!$model.windows_workspace.error-and(!$status-or!$status.Current.Name)) 'Final native health' @{workspace=$model.windows_workspace.error;status=if($status){$status.Current.Name}else{''}}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close
    Check ($review.WaitForExit(8000)-and$review.ExitCode-eq0) 'Normal owned process exit' $null
    Check ((Get-Item (Join-Path $run 'stderr.log')).Length-eq0) 'Empty native stderr' $null
    @{passed=$true;theme=$Theme;checks=$checks.Count;unverified=@($gaps);exe_sha256=(Get-FileHash $Executable).Hash;dll_sha256=(Get-FileHash (Join-Path $directory 'layer_windows.dll')).Hash;exit_code=$review.ExitCode}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'result.json')
    Write-Output "Header placement PASS $Theme $run"
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace)
    if($review-and!$review.HasExited){try{Capture 'failure' -Composed -WithModel}catch{}}
    throw
}finally{
    $checks|ConvertTo-Json -Depth 45|Set-Content (Join-Path $run 'checks.json');$samples|ConvertTo-Json -Depth 60|Set-Content (Join-Path $run 'caption-samples.json')
    [CapyRowPointer]::Dispose()
    if($review-and!$review.HasExited){[CapyWindowApi]::PostMessage($drawingWindow,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$review.WaitForExit(8000)){Stop-Process -Id $review.Id}}
    Exit-CapyEnvironment
}
