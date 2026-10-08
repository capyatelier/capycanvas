param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='visible';$CapyPopups=$true;$CapyWaitSeconds=30
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'PackageFixture.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/image-objects/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Package([string]$Path){
    $members=[CapyPackageFixture]::Read($Path)
    [Text.Encoding]::UTF8.GetString(@($members|Where-Object Key -eq 'manifest.json')[0].Value)|ConvertFrom-Json -Depth 100
}
function Canonical($Value){
    if($null -ne $Value -and $Value.GetType() -eq [System.Management.Automation.PSCustomObject]){
        $record=[ordered]@{}
        foreach($property in $Value.PSObject.Properties|Sort-Object Name){$record[$property.Name]=Canonical $property.Value}
        return $record
    }
    if($Value -is [array]){return ,@($Value|ForEach-Object {Canonical $_})}
    $Value
}
function Identity($Manifest){
    $objects=@($Manifest.objects|Where-Object type -in @('capy.image-object/1','capy.object-layer/1','capy.image/1','capy.paint-source/2','capy.occurrence/3')|Sort-Object id)
    foreach($source in $objects|Where-Object type -eq 'capy.paint-source/2'){
        if(!$source.data.PSObject.Properties['color_mode']){$source.data|Add-Member color_mode 'full_color'}
    }
    ConvertTo-Json -InputObject (Canonical $objects) -Depth 100 -Compress
}
function Objects-Row{@((Model).state.layers|Where-Object label -eq 'Images')[0]}
function Invoke-History([bool]$Redo=$false){
    $id=if($Redo){'redo'}else{'undo'}
    Wait-Until {((Model).state.commands|Where-Object id -eq $id).enabled} "$id did not become available"
    (Control 'drawing-canvas').SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),$(if($Redo){0x59}else{0x5A}))
}
function Samples{
    $camera=(Model).state.camera;$bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    $tab=@((Model).state.tabs|Where-Object active)[0]
    $bitmap=[Drawing.Bitmap]::new(1,1);$graphics=[Drawing.Graphics]::FromImage($bitmap)
    try{
        foreach($point in @(@(.1,.45),@(.1,.6),@(.1,.75),@(.25,.18),@(.25,.2),@(.25,.22),@(.82,.4),@(.82,.5),@(.82,.6))){
            $x=$point[0];$y=$point[1]
            $sx=[int]($bounds.X+($tab.width*$x*$camera.zoom+$camera.translation[0])*$bounds.Width/$camera.viewport[0])
            $sy=[int]($bounds.Y+($tab.height*$y*$camera.zoom+$camera.translation[1])*$bounds.Height/$camera.viewport[1])
            $graphics.CopyFromScreen($sx,$sy,0,0,[Drawing.Size]::new(1,1));$c=$bitmap.GetPixel(0,0)
            @([int]$c.R,[int]$c.G,[int]$c.B) -join ','
        }
    }finally{$graphics.Dispose();$bitmap.Dispose()}
}
function Colored($Samples){
    @($Samples|Where-Object {$rgb=$_ -split ','|ForEach-Object {[int]$_};($rgb|Measure-Object -Maximum).Maximum-($rgb|Measure-Object -Minimum).Minimum -gt 8}).Count
}
function Park{
    $b=(Control 'settings-button' -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::Hover([int]$b.X,[int]$b.Y);Start-Sleep -Milliseconds 400
}
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned image object review $($review.Id): $run"
    $native=@{window=$null}
    Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'The object review did not start' 90
    $root=$native.window.Root;$drawingWindow=$native.window.Handle
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    foreach($name in 'shared-image-f64-builtin','shared-image-f64-icc','shared-image-f64-nearest'){
        $source=Join-Path $repo ('apps/layer-web/fixtures/'+$name+'.capy')
        $before=Package $source;$identity=Identity $before
        $objects=@($before.objects|Where-Object type -eq 'capy.image-object/1')
        if($objects.Count -ne 3 -or @($before.objects|Where-Object type -eq 'capy.image/1').Count -ne 1 -or $objects[0].data.affine[4] -ne 16777217.125){throw 'The fixed shared image fixture changed'}
        Open-Project $source
        Wait-Until {(Model).state.document_file.location.uri -eq $source -and !(Model).state.document_file.busy -and (Model).brush_ready -and (Objects-Row)} 'The object package did not open' 120
        Fit-Canvas;Park
        Wait-Until {(Colored (Samples)) -gt 2} 'Cold image-object rendering produced no colored samples' 120
        $visible=Wait-StablePixels {(Samples) -join ';'}
        Capture "$name-$Theme" -WithModel -Composed
        $owner=(Objects-Row).id
        Invoke ('layer-'+$owner+'-visibility')
        Wait-Until {!(Objects-Row).visible} 'Hiding the object layer was not acknowledged'
        Wait-Until {(Colored (Samples)) -eq 0} 'Hiding the object layer left its rendered pixels'
        $hidden=Samples;$painted=$visible -split ';'
        foreach($start in 0,3,6){
            if(@($start..($start+2)|Where-Object {$painted[$_] -ne $hidden[$_]}).Count -eq 0){throw 'One of the three image instances did not contribute pixels in its exclusive region'}
        }
        Invoke-History;Wait-Until {(Objects-Row).visible -and ((Samples) -join ';') -eq $visible} 'Undo did not restore the object pixels'
        Invoke-History $true;Wait-Until {!(Objects-Row).visible -and (Colored (Samples)) -eq 0} 'Redo did not hide the object pixels'
        Invoke-History;Wait-Until {(Objects-Row).visible -and ((Samples) -join ';') -eq $visible} 'Second Undo did not restore the object pixels'
        $saved=Join-Path $run ($name+' 日本語.capy');Save-ProjectAs $saved
        if((Identity (Package $saved)) -ne $identity){throw 'Save changed image identity, shared resources or F64 object placement'}
        $epoch=(Model).state.document_file.epoch
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'close_document'
        Wait-Until {(Model).state.document_file.epoch -ne $epoch} 'The saved object drawing did not close'
        Open-Project $saved
        Wait-Until {(Model).state.document_file.location.uri -eq $saved -and !(Model).state.document_file.busy -and (Model).brush_ready} 'The saved object drawing did not reopen' 120
        Fit-Canvas;Park
        Wait-Until {((Samples) -join ';') -eq $visible} 'Reopen changed the object pixels' 120
        $copy=Join-Path $run ($name+' reopened.capy');Save-ProjectAs $copy
        if((Identity (Package $copy)) -ne $identity){throw 'Save after reopen changed the shared image or object records'}
        if((Model).state.host_error){throw "The object journey reported $((Model).state.host_error)"}
        Write-Output "PASS: $name open, D3D12 pixels, visibility history, F64/shared identity, save/reopen"
    }
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{theme=$Theme;built_in_profile='passed';icc_profile='passed';nearest_f64='passed';visible_history='passed';shared_image_identity='passed';save_reopen='passed';evidence=$run}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel -Composed}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    try{[CapyRowPointer]::Dispose()}catch{}
    Exit-CapyEnvironment
}
