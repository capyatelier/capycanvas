param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='visible'
$CapyCaptureDelay=250
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/enclose-fill/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$reference=Join-Path $run 'Reference ink.png'
$bitmap=[Drawing.Bitmap]::new(384,256,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
try{
    foreach($left in 40,150,270){for($y=50;$y -lt 170;$y++){for($x=$left;$x -lt $left+90;$x++){
        if($x -lt $left+6 -or $x -ge $left+84 -or $y -lt 56 -or $y -ge 164){$bitmap.SetPixel($x,$y,[Drawing.Color]::Black)}
    }}}
    $bitmap.Save($reference,[Drawing.Imaging.ImageFormat]::Png)
}finally{$bitmap.Dispose()}
function Key([uint16]$Code){[CapyRowPointer]::Key([uint32]$review.Id,$Code)}
function Chord([uint16[]]$Modifiers,[uint16]$Code){[CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Code)}
function Command([string]$Id){
    $label=((Model).state.commands|Where-Object id -eq $Id).label
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    Chord @(0x11) 0x4B
    Wait-Until {$search=Find 'command-search';$search -and !$search.Current.IsOffscreen} 'Command search did not open'
    (Find 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($label)
    Wait-Until {(Find 'command-result-0').Current.Name -eq $label} "Command search did not find $Id"
    Key 0x0D
}
function Screen([double]$X,[double]$Y){
    $camera=(Model).state.camera;$bounds=(Control 'drawing-canvas').Current.BoundingRectangle
    @{x=[int]($bounds.X+($X*$camera.zoom+$camera.translation[0])*$bounds.Width/$camera.viewport[0]);
      y=[int]($bounds.Y+($Y*$camera.zoom+$camera.translation[1])*$bounds.Height/$camera.viewport[1])}
}
function Pixel([double]$X,[double]$Y){
    $at=Screen $X $Y;$sample=[Drawing.Bitmap]::new(1,1);$graphics=[Drawing.Graphics]::FromImage($sample)
    try{$graphics.CopyFromScreen($at.x,$at.y,0,0,[Drawing.Size]::new(1,1));$color=$sample.GetPixel(0,0);@($color.R,$color.G,$color.B)}finally{$graphics.Dispose();$sample.Dispose()}
}
function Red($rgb){$rgb[0] -gt 180 -and $rgb[1] -lt 80 -and $rgb[2] -lt 80}
function Revision{[string](@((Model).state.layers|Where-Object editing)[0].paint_revision)}
function Park{$away=Screen 370 240;[CapyRowPointer]::Hover($away.x,$away.y);Start-Sleep -Milliseconds 300}
function Enclose([switch]$Cancel){
    $points=@(@(20,30),@(315,30),@(315,190),@(20,190),@(20,30))
    $start=Screen $points[0][0] $points[0][1];[CapyRowPointer]::Hover($start.x,$start.y);Start-Sleep -Milliseconds 80
    [CapyRowPointer]::Down('mouse',$start.x,$start.y)
    for($i=1;$i -lt $points.Count;$i++){
        for($step=1;$step -le 8;$step++){
            $x=$points[$i-1][0]+($points[$i][0]-$points[$i-1][0])*$step/8;$y=$points[$i-1][1]+($points[$i][1]-$points[$i-1][1])*$step/8
            $at=Screen $x $y;[CapyRowPointer]::Move($at.x,$at.y);Start-Sleep -Milliseconds 16
        }
    }
    if($Cancel){[CapyRowPointer]::Key(0x1B)}
    [CapyRowPointer]::Up()
    Park
}
function Settled{Wait-Until {$m=Model;$m -and !$m.state.document_file.busy -and $m.brush_ready} 'The drawing did not settle' 30}
function Holes([bool]$Filled){
    Wait-Until {$a=Pixel 85 110;$b=Pixel 195 110;(Red $a) -eq $Filled -and (Red $b) -eq $Filled} $(if($Filled){'The enclosed holes did not fill'}else{'The enclosed holes did not clear'}) 15
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned enclose fill review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Enclose fill review did not start' 90
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Start-Sleep -Milliseconds 600
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)

    Open-Project $reference
    Wait-Until {$tab=@((Model).state.tabs|Where-Object active)[0];$tab -and $tab.width -eq 384 -and $tab.height -eq 256 -and (Model).brush_ready} 'The reference image did not open' 45
    Invoke-Id 'layer-reference'
    Wait-Until {(Model).state.layer_tools.references_selected} 'The reference ink did not become a reference'
    Invoke-Id 'layer-new'
    Wait-Until {@((Model).state.layers).Count -ge 2 -and !@((Model).state.layers|Where-Object editing)[0].reference} 'A new paint layer did not open above the reference'
    Command 'enclose_fill'
    Wait-Until {((Model).state.commands|Where-Object id -eq 'enclose_fill').selected} 'Enclose and Fill did not become the tool'
    $settings=@((Model).state.tool_settings|ForEach-Object id)
    foreach($id in 'tolerance','gap_closing','expansion','smoothing'){
        if($settings -notcontains $id){throw "Enclose and Fill does not share $id with Fill"}
        $null=Control ('tool-setting-'+$id)
    }
    foreach($id in 'gap_closing','expansion','smoothing'){
        $entry=Control ('tool-setting-'+$id);$entry.SetFocus()
        $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('0');Key 0x0D
        Wait-Until {((Model).state.tool_settings|Where-Object id -eq $id).value -eq 0} "Enclose and Fill did not accept $id 0"
    }
    Invoke-Id 'color-edit'
    Wait-Until {Find 'edit-color-apply'} 'Edit Color did not open'
    Invoke 'edit-color-hex';$hex=Control 'edit-color-hex-entry';$hex.SetFocus()
    $hex.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#D91A0D');Key 0x0D
    Invoke 'edit-color-apply';Wait-Until {!(Find 'edit-color-apply')} 'Edit Color did not close'
    Fit-Canvas;Settled;Park
    Holes $false
    $empty=Revision
    Enclose
    Wait-Until {(Revision) -ne $empty} 'Enclose and Fill did not paint'
    Settled;Park;Holes $true
    foreach($point in @(@(25,110),@(140,110),@(290,110),@(85,195))){if(Red (Pixel $point[0] $point[1])){throw "Enclose and Fill painted outside the enclosed holes at $($point -join ',')"}}
    $ink=Pixel 42 110;if(($ink|Measure-Object -Maximum).Maximum -gt 60){throw "The reference ink did not stay visible: $($ink -join ',')"}
    Capture "enclose-fill-$Theme" -WithModel
    Chord @(0x11) 0x5A;Wait-Until {(Revision) -eq $empty} 'One Undo did not remove both holes';Settled;Park;Holes $false
    Chord @(0x11) 0x59;Wait-Until {(Revision) -ne $empty} 'Redo did not restore the fill';Settled;Park;Holes $true
    Chord @(0x11) 0x5A;Wait-Until {(Revision) -eq $empty} 'Undo did not remove the fill again';Settled
    Enclose -Cancel
    Settled;Start-Sleep -Milliseconds 500
    if((Revision) -ne $empty){throw 'Escape did not cancel the enclosure'}
    Park;Holes $false
    Chord @(0x11) 0x59;Wait-Until {(Revision) -ne $empty} 'Cancelling an enclosure lost Redo';Settled;Park;Holes $true
    if((Model).state.host_error){throw "Enclose and Fill reported $((Model).state.host_error)"}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{theme=$Theme;reference='passed';shared_controls='passed';enclosed_holes='passed';exterior_and_crossed='passed';reference_ink='passed';undo_redo='passed';escape='passed';evidence=$run}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    try{[CapyRowPointer]::Dispose()}catch{}
    Exit-CapyEnvironment
}
