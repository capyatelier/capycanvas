param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='visible';$CapyPopups=$true;$CapyWaitSeconds=20
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/illustration-filters/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$source=Join-Path $run 'Illustration source.png'
$project=Join-Path $run 'Illustration 日本語.capy'
$bitmap=[Drawing.Bitmap]::new(320,192,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
try{
    $graphics=[Drawing.Graphics]::FromImage($bitmap)
    try{
        foreach($patch in @(@(0,255,220,40,30),@(80,255,255,255,255),@(160,96,0,0,0))){
            $brush=[Drawing.SolidBrush]::new([Drawing.Color]::FromArgb($patch[1],$patch[2],$patch[3],$patch[4]))
            try{$graphics.FillRectangle($brush,$patch[0],0,80,192)}finally{$brush.Dispose()}
        }
    }finally{$graphics.Dispose()}
    $bitmap.Save($source,[Drawing.Imaging.ImageFormat]::Png)
}finally{$bitmap.Dispose()}
function Property([string]$Key){(Model).state.layer_properties.controls|Where-Object key -eq $Key}
function Key([uint16]$Code){[CapyRowPointer]::Key([uint32]$review.Id,$Code)}
function Invoke-History([bool]$Redo=$false){
    $id=if($Redo){'redo'}else{'undo'}
    Wait-Until {((Model).state.commands|Where-Object id -eq $id).enabled} "$id did not become available"
    (Control 'drawing-canvas').SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),$(if($Redo){0x59}else{0x5A}))
}
function Panel([string]$Id){
    if(@((Model).layout.groups|Where-Object active -eq $Id).Count -eq 0){Invoke $(if(Find ('drawer-tab-'+$Id)){'drawer-tab-'+$Id}else{'panel-tab-'+$Id})}
    Wait-Until {@((Model).layout.groups|Where-Object active -eq $Id).Count -gt 0} "Panel $Id did not open"
}
function Choose([string]$Id,[string]$Label){
    (Control $Id).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Label -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Choice([string]$Key,[int]$Index){
    Choose ('property-'+$Key) (Property $Key).kind.options[$Index]
    Wait-Until {(Property $Key).value.value -eq $Index} "$Key did not select $Index"
}
function Number([string]$Key,[string]$Value){
    $entry=Control ('property-'+$Key);$entry.SetFocus()
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value);Key 0x0D
    Wait-Until {[Math]::Abs((Property $Key).value.value-[double]$Value) -lt .00001} "$Key did not commit $Value"
}
function Select-Filter([string]$Id,[string]$Label){
    Panel 'adjustments'
    if($null -ne (Model).state.filter_picker.search){Invoke 'filter-search-toggle'}
    Choose 'filter-category' 'All filters';Invoke 'filter-search-toggle'
    (Control 'filter-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Label)
    Wait-Until {(Model).state.filter_picker.search -eq $Label -and (Find ('filter-'+$Id))} "$Label did not appear in Filters"
    Wait-Until {(Find ('filter-preview-'+$Id)).Current.ItemStatus -eq 'Ready'} "$Label preview did not load" 90
    Invoke ('filter-'+$Id)
    Wait-Until {(Model).state.layer_properties.description -eq $Label} "$Label did not become active"
    Panel 'properties';Fit-Canvas
}
function Pixel([double]$X){
    $camera=(Model).state.camera;$bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    $x=[int]($bounds.X+($X*$camera.zoom+$camera.translation[0])*$bounds.Width/$camera.viewport[0])
    $y=[int]($bounds.Y+(96*$camera.zoom+$camera.translation[1])*$bounds.Height/$camera.viewport[1])
    $sample=[Drawing.Bitmap]::new(1,1);$graphics=[Drawing.Graphics]::FromImage($sample)
    try{$graphics.CopyFromScreen($x,$y,0,0,[Drawing.Size]::new(1,1));$c=$sample.GetPixel(0,0);@([int]$c.R,[int]$c.G,[int]$c.B)}finally{$graphics.Dispose();$sample.Dispose()}
}
function Near($A,$B,[int]$Tolerance=4){
    if($A.Count -ne $B.Count){return $false}
    for($i=0;$i -lt $A.Count;$i++){if([Math]::Abs($A[$i]-$B[$i]) -gt $Tolerance){return $false}}
    $true
}
function Pair([double]$X){Pixel $X;Pixel ($X+8)}
function Transparent([int]$X){Near (Pair $X) $background[$X]}
function Park{
    $bounds=(Control 'settings-button' -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::Hover([int]$bounds.X,[int]$bounds.Y)
    Start-Sleep -Milliseconds 400
}
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned illustration filter review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'The filter review did not start' 90
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Open-Project $source
    Wait-Until {$tab=@((Model).state.tabs|Where-Object active)[0];$tab.width -eq 320 -and $tab.height -eq 192 -and (Model).brush_ready} 'The source image did not open' 90
    Fit-Canvas;Park
    $original=Pixel 40;$partial=Pixel 200
    if($original[0]-$original[1] -lt 100){throw 'The source red patch was not visible'}
    $sourceLayer=@((Model).state.layers|Where-Object editing)[0].id
    Invoke ('layer-'+$sourceLayer+'-visibility')
    Wait-Until {!@((Model).state.layers|Where-Object id -eq $sourceLayer)[0].visible} 'Source visibility did not change'
    Park
    Wait-Until {$p=Pixel 40;[Math]::Abs($p[0]-$p[1]) -lt 4 -and $p[0] -gt 180} 'The hidden source did not reveal the checker'
    $background=@{}
    foreach($x in 40,120,200){
        $background[$x]=Pair $x
        if(Near $background[$x][0..2] $background[$x][3..5]){throw 'The transparency probes must span contrasting checker cells'}
    }
    Invoke-History;Wait-Until {Near (Pixel 40) $original} 'Undo did not restore the source after sampling transparency'
    Select-Filter 'brightness_to_opacity' 'Brightness to Opacity';Park
    if(@((Model).state.layer_properties.controls).Count){throw 'Brightness to Opacity offered controls'}
    Wait-Until {$c=Pixel 40;[Math]::Abs($c[0]-$c[1]) -lt 4 -and !(Near $c $original)} 'Brightness to Opacity did not produce neutral ink'
    Wait-Until {Transparent 120} 'Brightness to Opacity did not make white transparent'
    if(!(Near (Pixel 200) $partial)){throw 'Brightness to Opacity changed black source coverage'}
    Capture "brightness-to-opacity-$Theme" -WithModel -Composed
    Invoke-History;Wait-Until {Near (Pixel 40) $original} 'One Undo did not restore the imported source'
    Invoke-History $true;Wait-Until {Transparent 120} 'Redo did not restore Brightness to Opacity'
    Invoke-History;Wait-Until {Near (Pixel 40) $original} 'Undo did not restore the source before Threshold'
    Select-Filter 'threshold' 'Threshold';Park
    foreach($id in 'threshold','colors','transparency'){$null=Control ('property-'+$id)}
    if(Find 'property-alpha_threshold'){throw 'Keep transparency exposed its hidden alpha threshold'}
    Wait-Until {Near (Pixel 40) @(0,0,0)} 'Threshold did not turn dark color into black'
    Wait-Until {Near (Pixel 120) @(255,255,255)} 'Threshold did not keep white'
    Number 'threshold' '0.378';Invoke-History
    Wait-Until {(Property 'threshold').value.value -eq .5} 'Threshold value was not one Undo'
    Invoke-History $true;Wait-Until {[Math]::Abs((Property 'threshold').value.value-.378) -lt .000001} 'Threshold value did not redo'
    Choice 'colors' 1;Park
    Wait-Until {Transparent 120} 'Black output did not make the light patch transparent'
    Choice 'transparency' 1;Number 'alpha_threshold' '60';Park
    Wait-Until {Transparent 200} 'Alpha threshold did not remove partial coverage'
    Number 'alpha_threshold' '37';Park
    Wait-Until {Near (Pixel 200) @(0,0,0)} 'Alpha threshold did not make retained coverage opaque'
    Choice 'transparency' 0
    Wait-Until {!(Find 'property-alpha_threshold') -and !(Property 'alpha_threshold')} 'Keep did not hide Alpha threshold'
    Park;Wait-Until {Near (Pixel 200) $partial} 'Keep did not restore the original partial coverage'
    Invoke-History;Wait-Until {(Property 'alpha_threshold').value.value -eq 37} 'Undo did not retain the hidden alpha threshold'
    Invoke-History $true;Wait-Until {!(Property 'alpha_threshold')} 'Redo did not return to Keep'
    Choice 'colors' 2;Park
    Wait-Until {Transparent 40} 'White output did not make the dark patch transparent'
    Capture "threshold-white-$Theme" -WithModel -Composed
    Save-ProjectAs $project
    $epoch=(Model).state.document_file.epoch
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'close_document'
    Wait-Until {(Model).state.document_file.epoch -ne $epoch} 'The saved drawing did not close'
    Open-Project $project
    Wait-Until {(Model).state.document_file.location.uri -eq $project -and !(Model).state.document_file.busy -and (Model).brush_ready} 'The saved drawing did not reopen' 90
    $filter=@((Model).state.layers|Where-Object label -eq 'Threshold')[0]
    Invoke ('layer-'+$filter.id+'-name');Panel 'properties';Fit-Canvas;Park
    Wait-Until {(Property 'colors').value.value -eq 2 -and (Property 'transparency').value.value -eq 0 -and [Math]::Abs((Property 'threshold').value.value-.378) -lt .000001} 'Reopen lost Threshold choices'
    Wait-Until {Transparent 40} 'Reopen changed the filtered artwork'
    Choice 'transparency' 1
    Wait-Until {(Property 'alpha_threshold').value.value -eq 37} 'Save/reopen lost the hidden alpha threshold'
    Capture "threshold-reopened-$Theme" -WithModel -Composed
    if((Model).state.host_error){throw "The filter journey reported $((Model).state.host_error)"}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{theme=$Theme;brightness_to_opacity='passed';threshold_outputs='passed';alpha_and_hidden_values='passed';undo_redo='passed';save_reopen='passed';evidence=$run}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel -Composed}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    try{[CapyRowPointer]::Dispose()}catch{}
    Exit-CapyEnvironment
}
