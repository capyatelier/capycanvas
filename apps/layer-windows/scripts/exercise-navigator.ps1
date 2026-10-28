param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$CapyEach={[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP);[CapyRowPointer]::Verify()}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyNavigatorCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left,top,right,bottom; }
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/navigator/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
function Navigator-Pose($Camera){
    @($Camera.zoom,$Camera.rotation,$Camera.flipped,$Camera.translation)|ConvertTo-Json -Depth 4 -Compress
}
function Navigator-Document{
    $file=(Model).state.document_file
    @($file.epoch,$file.revision,$file.modified)|ConvertTo-Json -Compress
}
function Navigator {
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    $menuCondition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.OrCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Navigator panel'),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Navigator')),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
    Wait-Until {$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$menuCondition)} 'Navigator menu item is missing'
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$menuCondition).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
}
function Capture([string]$Name){
    $rect=[CapyNavigatorCapture+Rect]::new()
    if(![CapyNavigatorCapture]::GetWindowRect($drawingWindow,[ref]$rect)){throw 'Window bounds unavailable'}
    $bitmap=[Drawing.Bitmap]::new($rect.right-$rect.left,$rect.bottom-$rect.top)
    $graphics=[Drawing.Graphics]::FromImage($bitmap)
    try {$graphics.CopyFromScreen($rect.left,$rect.top,0,0,$bitmap.Size)}
    finally {$graphics.Dispose()}
    try {$bitmap.Save((Join-Path $run ($Name+'.png')),[Drawing.Imaging.ImageFormat]::Png)}catch{$bitmap.Dispose();throw}
    return $bitmap
}
function Image-Rect {
    $overview=(Control 'navigator-overview').Current.BoundingRectangle
    $window=[CapyNavigatorCapture+Rect]::new()
    if(![CapyNavigatorCapture]::GetWindowRect($drawingWindow,[ref]$window)){throw 'Window bounds unavailable'}
    $scale=[CapyNavigatorCapture]::GetDpiForWindow($drawingWindow)/96.
    $document=(Model).state.tabs[0]
    $fit=[Math]::Min(($overview.Width-8*$scale)/$document.width,($overview.Height-8*$scale)/$document.height)
    $width=$document.width*$fit;$height=$document.height*$fit
    # Stay inside the transparent cutout, away from antialiased boundary pixels.
    [Drawing.Rectangle]::FromLTRB(
        [int][Math]::Ceiling($overview.Left-$window.left+($overview.Width-$width)/2+2),
        [int][Math]::Ceiling($overview.Top-$window.top+($overview.Height-$height)/2+2),
        [int][Math]::Floor($overview.Left-$window.left+($overview.Width+$width)/2-2),
        [int][Math]::Floor($overview.Top-$window.top+($overview.Height+$height)/2-2))
}
function Surround-Matches($Capture) {
    $overview=(Control 'navigator-overview').Current.BoundingRectangle
    $window=[CapyNavigatorCapture+Rect]::new()
    if(![CapyNavigatorCapture]::GetWindowRect($drawingWindow,[ref]$window)){throw 'Window bounds unavailable'}
    $pixel=$Capture.GetPixel([int]($overview.Left-$window.left+2),[int]($overview.Top-$window.top+2))
    $expected=[Drawing.ColorTranslator]::FromHtml((Model).state.palette.bg)
    $pixel.R -eq $expected.R -and $pixel.G -eq $expected.G -and $pixel.B -eq $expected.B
}
function Different($Before,$After,$Rect) {
    $count=0
    for($y=$Rect.Top;$y -lt $Rect.Bottom;$y++){for($x=$Rect.Left;$x -lt $Rect.Right;$x++){
        $a=$Before.GetPixel($x,$y);$b=$After.GetPixel($x,$y)
        if([Math]::Abs([int]$a.R-$b.R)+[Math]::Abs([int]$a.G-$b.G)+[Math]::Abs([int]$a.B-$b.B) -gt 30){$count++}
    }}
    $count
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Compress))
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/navigator-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned Navigator review $($review.Id)"
    $native=@{window=$null}
    Wait-Until {$native.window=Owned-DrawingWindow $review;$null -ne $native.window -and (Model).brush_ready} 'Review did not start' 45
    if(!(Model).windows_isolated_settings){throw 'Navigator fixture requires an isolated profile'}
    [CapyNavigatorCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $root=$native.window.Root;$drawingWindow=$native.window.Handle
    try{Wait-Until {Find 'navigator-overview' -Visible} 'Navigator is hidden by default' 3}catch{Navigator}
    Expand-PanelColumn 'navigator'
    $overview=Control 'navigator-overview' -Arranged
    $identity=$overview.GetRuntimeId() -join ':'
    $scale=[CapyNavigatorCapture]::GetDpiForWindow($drawingWindow)/96.
    if($overview.Current.BoundingRectangle.Height -gt 220*$scale+1){throw 'Overview exceeds the shared 220-unit height cap'}
    foreach($id in @('zoom_out','zoom_in','rotate_left','rotate_right','flip_horizontal','flip_vertical')){
        $button=Control ('navigator-'+$id)
        $command=(Model).state.commands|Where-Object {$_.id -eq $id}
        if($button.Current.Name -ne $command.label -or $button.Current.IsEnabled -ne $command.enabled){throw "Navigator command projection differs: $id"}
    }
    $camera=(Control 'canvas-camera').Current.Name
    Invoke 'navigator-zoom_in'
    Wait-Until {(Control 'canvas-camera').Current.Name -ne $camera} 'Zoom did not update the native camera readout'
    Invoke 'navigator-zoom_out'
    Wait-Until {(Control 'canvas-camera').Current.Name -eq $camera} 'Inverse zoom did not restore the view'
    Invoke 'navigator-rotate_left'
    Wait-Until {(Control 'canvas-camera').Current.Name -ne $camera} 'Rotate did not update the view'
    Invoke 'navigator-rotate_right'
    Wait-Until {(Control 'canvas-camera').Current.Name -eq $camera} 'Inverse rotation did not restore the view'
    foreach($id in @('flip_horizontal','flip_vertical')){
        Invoke ('navigator-'+$id)
        Wait-Until {(Control ('navigator-'+$id)).Current.ItemStatus -eq 'Selected'} 'Flip did not become selected'
        Invoke ('navigator-'+$id)
        Wait-Until {(Control ('navigator-'+$id)).Current.ItemStatus -eq ''} 'Flip did not clear'
    }
    if((Model).state.document_file.modified){throw 'Camera controls modified the document'}
    if(((Control 'navigator-overview').GetRuntimeId() -join ':') -ne $identity){throw 'Camera updates replaced the native overview'}
    $canvas=Control 'drawing-canvas' -Arranged
    $canvas.SetFocus()
    [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $navDocument=Navigator-Document
    $navBaseline=(Model).state.camera
    $navigatorChecks=[Collections.Generic.List[object]]::new()
    for($i=0;$i -lt 3;$i++){
        $beforeZoom=(Model).state.camera.zoom
        Invoke 'navigator-zoom_in'
        Wait-Until {(Model).state.camera.zoom -gt $beforeZoom} 'Navigator gesture zoom did not advance'
    }
    foreach($device in @('mouse','pen','touch')){
        foreach($ending in $(if($device -eq 'mouse'){@('up')}else{@('up','cancel')})){
            foreach($fresh in @($false,$true)){
                $box=(Control 'navigator-overview' -Arranged).Current.BoundingRectangle
                $document=@((Model).state.tabs|Where-Object active)[0]
                $fit=[Math]::Min(($box.Width-8*$scale)/$document.width,($box.Height-8*$scale)/$document.height)
                $width=$document.width*$fit;$height=$document.height*$fit
                if($width -le 16*$scale -or $height -le 16*$scale){throw 'Navigator image is too small for the native contact checks'}
                $left=$box.Left+($box.Width-$width)/2
                $x0=[int]($left+$width*.35);$x1=[int]($left+$width*.65)
                if($fresh){$swap=$x0;$x0=$x1;$x1=$swap}
                $y=[int]($box.Top+$box.Height/2)
                $finish=if($fresh){'up'}else{$ending}
                $stimulus=if($finish -eq 'up'){'normal native Up'}elseif($device -eq 'pen'){'synthetic pen device removal'}else{'synthetic touch canceled Up'}
                $row=@{device=$device;ending_stimulus=$stimulus;fresh=$fresh;start_px=@($x0,$y);end_px=@($x1,$y);bounds=$box;hit=(Input-Trace $x0 $y);before=(Model).state.camera;document_before=(Navigator-Document);move_samples=6;move_sleep_ms=25;uia_during_contact=$false}
                $timer=[Diagnostics.Stopwatch]::StartNew()
                try{
                    try{
                        [CapyRowPointer]::Down($device,$x0,$y)
                        Wait-Until {(Model).state.camera.revision -gt $row.before.revision} "$device Navigator Down was not acknowledged"
                        $row.down=(Model).state.camera
                        for($step=1;$step -le 6;$step++){
                            [CapyRowPointer]::Move([int]($x0+($x1-$x0)*$step/6),$y)
                            Start-Sleep -Milliseconds 25
                        }
                        $settled=@{pose=$null;count=0;camera=$null}
                        Wait-Until {
                            $c=(Model).state.camera;$pose=Navigator-Pose $c
                            if($pose -ceq $settled.pose){$settled.count++}else{$settled.count=0}
                            $settled.pose=$pose;$settled.camera=$c
                            $settled.count -ge 2 -and
                                ([Math]::Abs($c.translation[0]-$row.down.translation[0]) -gt .1 -or
                                 [Math]::Abs($c.translation[1]-$row.down.translation[1]) -gt .1)
                        } "$device Navigator motion did not pan"
                        $row.live=$settled.camera
                        $row.document_live=Navigator-Document
                    }finally{
                        if([CapyRowPointer]::Active){
                            if($finish -eq 'cancel'){[CapyRowPointer]::Cancel()}else{[CapyRowPointer]::Up()}
                        }
                        $timer.Stop();$row.active_elapsed_ms=$timer.Elapsed.TotalMilliseconds
                        $duration=[CapyRowPointer]::ContactDurationMilliseconds
                        $row.contact_ms=if([double]::IsNaN($duration)){$null}else{$duration}
                    }
                    [CapyRowPointer]::Verify()
                    Start-Sleep -Milliseconds 200
                    $row.after=(Model).state.camera
                    $row.document_after=Navigator-Document
                    $row.passed=(Navigator-Pose $row.after) -ceq (Navigator-Pose $row.live) -and $row.document_live -ceq $navDocument -and $row.document_after -ceq $navDocument
                }catch{$row.error=$_.ToString();throw}finally{
                    $navigatorChecks.Add($row)
                    ConvertTo-Json -InputObject @($navigatorChecks.ToArray()) -Depth 15|Set-Content (Join-Path $run 'navigator-contacts.json')
                }
                if(!$row.passed){throw "$device Navigator $finish/fresh=$fresh changed the settled pose or document"}
            }
        }
    }
    if($navigatorChecks.Count -ne 10){throw 'Navigator did not complete all ten native contacts'}
    $secondary=@{device='mouse';button='right';ending_stimulus='normal native right-button Up';start_px=@($x0,$y);end_px=@($x1,$y);hit=(Input-Trace $x0 $y);before=(Model).state.camera;document_before=(Navigator-Document);uia_during_contact=$false}
    $secondary|ConvertTo-Json -Depth 15|Set-Content (Join-Path $run 'navigator-secondary.json')
    $timer=[Diagnostics.Stopwatch]::StartNew()
    [CapyRowPointer]::RightDrag($x0,$y,$x1,$y)
    $timer.Stop();$secondary.active_elapsed_ms=$timer.Elapsed.TotalMilliseconds;$secondary.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds
    Start-Sleep -Milliseconds 350
    $secondary.after=(Model).state.camera;$secondary.document_after=Navigator-Document
    $secondary.passed=(Navigator-Pose $secondary.after) -ceq (Navigator-Pose $secondary.before) -and $secondary.document_after -ceq $navDocument
    $secondary|ConvertTo-Json -Depth 15|Set-Content (Join-Path $run 'navigator-secondary.json')
    if(!$secondary.passed){throw 'Secondary mouse input moved the Navigator camera or changed the document'}
    for($i=0;$i -lt 3;$i++){Invoke 'navigator-zoom_out'}
    Wait-Until {[Math]::Abs((Model).state.camera.zoom-$navBaseline.zoom) -lt .001} 'Navigator gesture zoom did not restore'
    if((Navigator-Document) -cne $navDocument){throw 'Navigator contacts modified the document'}
    @{baseline=$navBaseline;after_inverse_zoom=(Model).state.camera;document_before=$navDocument;document_after=(Navigator-Document)}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'navigator-pose.json')
    $bounds=$canvas.Current.BoundingRectangle
    [CapyRowPointer]::Hover([int]($bounds.Left+$bounds.Width/2),[int]($bounds.Top+$bounds.Height/2))
    $tooltip=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ToolTip),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
    Wait-Until {!@([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$tooltip)|Where-Object {!$_.Current.IsOffscreen}).Count} 'Navigator tooltip did not leave the capture area'
    $area=Image-Rect
    $blank=Capture 'blank'
    try {
        if(!(Surround-Matches $blank)){throw 'Navigator surround differs from the shared palette'}
        Invoke 'Test stroke' -Name
        Wait-Until {(Model).state.document_file.modified} 'Controlled stroke did not reach the shared document'
        Wait-Until {$paint=Capture 'paint';try {(Different $blank $paint $area) -ge 20}finally{$paint.Dispose()}} 'Live GPU overview did not show the stroke'
        Invoke 'Undo' -Name
        Wait-Until {!(Model).state.document_file.modified} 'Undo did not restore the checkpoint'
        Wait-Until {$undo=Capture 'undo';try {(Different $blank $undo $area) -eq 0}finally{$undo.Dispose()}} 'Undo did not restore the GPU overview pixels'
    }finally{$blank.Dispose()}
    Wait-Until {((Model).state.commands|Where-Object id -eq 'new_document').enabled} 'New drawing stayed unavailable'
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File'
    Wait-Until {$item=Find 'new_document';$item -and $item.Current.IsEnabled} 'New drawing menu item stayed disabled'
    Invoke 'new_document'
    New-DrawingPixels 128 64
    Invoke 'Create' -Name
    Wait-Until {(Model).state.tabs[0].width -eq 128 -and (Model).state.tabs[0].height -eq 64 -and !(Model).state.document_file.busy} 'Different-aspect document was not adopted' 45
    Wait-Until {(Control 'drawing-canvas').Current.IsEnabled} 'Document dialog input gate did not clear'
    if(((Control 'navigator-overview').GetRuntimeId() -join ':') -ne $identity){throw 'Document replacement rebuilt the Navigator'}
    Wait-Until {
        $replacement=Capture 'replacement'
        try {
            $area=Image-Rect
            $inside=$replacement.GetPixel($area.Left+8,$area.Top+8)
            if($inside.R -lt 250 -or $inside.G -lt 250 -or $inside.B -lt 250){return $false}
            Surround-Matches $replacement
        }finally{$replacement.Dispose()}
    } 'New document did not fill the expected overview image'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Resize -Width 1550 -Height 1040
    Wait-Until {(Model).state.camera.viewport[0] -gt 1450} 'Native resize did not reach the renderer'
    if(((Control 'navigator-overview').GetRuntimeId() -join ':') -ne $identity){throw 'Resize replaced the native Navigator'}
    Navigator
    Wait-Until {!(Find 'navigator-overview')} 'Hiding Navigator left its native controls loaded'
    Navigator
    $null=Control 'navigator-overview'
    $theme=(Model).state.theme
    Invoke 'settings-button'
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $choice=if($theme -eq 'dark'){'Light'}else{'Dark'}
    (Control $choice -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -ne $theme} 'Theme change not acknowledged'
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'CloseButton'),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
    $null=Control 'navigator-overview'
    Wait-Until {
        $alternate=Capture 'alternate-theme'
        try{Surround-Matches $alternate}finally{$alternate.Dispose()}
    } 'Navigator surround did not present the alternate theme after Preferences closed'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native runtime stderr requires inspection'}
    [PSCustomObject]@{
        theme=$Theme;native_overview_contacts=$navigatorChecks.Count;secondary_mouse_admission='passed'
        shared_commands_and_camera='passed';retained_controls_and_resize='passed'
        live_stroke_and_exact_undo='passed';changed_preview_pixels=$changed
        document_aspect_replacement='passed';hide_reopen_and_theme='passed';zero_exit='passed'
        scope='isolated native controls and app-only GPU pixels; physical pointer gestures, full-editor parity and presentation acceptance remain separate'
    }|ConvertTo-Json
}catch{[IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
