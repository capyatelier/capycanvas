param([string]$Fixture=(Join-Path $PSScriptRoot '../scripts/exercise-tool-bubbles.ps1'))
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
. (Join-Path $PSScriptRoot '../scripts/BlenderDocumentPixels.ps1')
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($Fixture,[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$journey=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Invoke-BlenderCellChecks'},$true)
$restart=@($journey.Body.EndBlock.Statements|Where-Object {$_.Extent.Text.Contains("throw 'Native Blender session restart changed authored pixels") -or ($_.Extent.Text.StartsWith('Assert-BlenderDocumentPixels') -and $_.Extent.Text.Contains("'-session-restart'"))})
if($restart.Count -ne 1){throw 'Missing actual Blender restart pixel assertion'}
$compare=[scriptblock]::Create($restart[0].Extent.Text)
$directory=Join-Path $PSScriptRoot ('../../../artifacts/windows/blender-document-pixels/'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($directory)|Out-Null
$run=$directory;$Tag='regression';$pixelExport=$null;$checks=0
function Reject([string]$Name,[scriptblock]$Action,[string]$Message){
    $observed=$null
    try{& $Action}catch{$observed=$_.Exception.Message}
    if(!$observed -or $observed -notlike $Message){throw ('Blender negative '+$Name+' was not rejected at its assertion: '+$observed)}
    $script:checks++
}
function Export-BlenderPixels([string]$Name,$Export,[string]$Project=''){$script:exportObserved}
function Pixel-Key($Image){
    $bytes=[byte[]]::new($Image.Width*$Image.Height*4);$i=0
    for($y=0;$y -lt $Image.Height;$y++){for($x=0;$x -lt $Image.Width;$x++){
        $color=$Image.GetPixel($x,$y);$bytes[$i++]=$color.R;$bytes[$i++]=$color.G;$bytes[$i++]=$color.B;$bytes[$i++]=$color.A
    }}
    [Convert]::ToBase64String($bytes)
}
function Projected-Key($Image,[single]$Translation){
    $screen=[Drawing.Bitmap]::new(200,160,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics=[Drawing.Graphics]::FromImage($screen)
    try{
        $graphics.Clear([Drawing.Color]::White)
        $graphics.InterpolationMode=[Drawing.Drawing2D.InterpolationMode]::HighQualityBilinear
        $graphics.PixelOffsetMode=[Drawing.Drawing2D.PixelOffsetMode]::Half
        $graphics.DrawImage($Image,[Drawing.RectangleF]::new($Translation,0,$Image.Width,$Image.Height),[Drawing.RectangleF]::new(0,0,$Image.Width,$Image.Height),[Drawing.GraphicsUnit]::Pixel)
        $center=[int][Math]::Round(80+$Translation)
        $patch=$screen.Clone([Drawing.Rectangle]::new(($center-48),16,96,96),[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try{Pixel-Key $patch}finally{$patch.Dispose()}
    }finally{$graphics.Dispose();$screen.Dispose()}
}
function Wait-StablePixels([scriptblock]$Read){& $Read}
function Screen-Pixels([int]$X,[int]$Y){$script:restoredScreen}
$bitmap=[Drawing.Bitmap]::new(160,128,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
try{
    for($y=0;$y -lt $bitmap.Height;$y++){for($x=0;$x -lt $bitmap.Width;$x++){
        $bitmap.SetPixel($x,$y,[Drawing.Color]::FromArgb((64+($x+$y)%192),(($x*37+$y*3)%256),(($x*13+$y*41)%256),(($x*7+$y*17)%256)))
    }}
    $expectedPath=Join-Path $directory 'painted.png';$actualPath=Join-Path $directory 'restored.png'
    $bitmap.Save($expectedPath,[Drawing.Imaging.ImageFormat]::Png);$bitmap.Save($actualPath,[Drawing.Imaging.ImageFormat]::Png)
    $expected=[Drawing.Bitmap]::new($expectedPath);$actual=[Drawing.Bitmap]::new($actualPath)
    try{
        if($expected.Width -ne $actual.Width -or $expected.Height -ne $actual.Height -or (Pixel-Key $expected) -cne (Pixel-Key $actual)){throw 'Lossless document reference setup changed pixels or alpha'}
        $painted=Projected-Key $expected 0
        $script:restoredScreen=Projected-Key $actual -23.75
        if($painted -ceq $restoredScreen){throw 'Camera phase setup did not change the composed samples'}
        $file=Join-Path $directory 'drawing.capy';[IO.File]::WriteAllBytes($file,[byte[]](1,2,3,4));$savedFile=(Get-FileHash -LiteralPath $file).Hash
        $sample=@{x=56;y=64}
        $paintedDocument=Read-BlenderDocumentPixels $expectedPath @(160,128)
        $script:exportObserved=Read-BlenderDocumentPixels $actualPath @(160,128)
        & $compare;$checks++
        $afterSave=@($journey.Body.EndBlock.Statements|Where-Object {$_.Extent.Text.StartsWith('Assert-BlenderDocumentPixels') -and $_.Extent.Text.Contains("'-after-save'")})
        if($afterSave.Count -ne 1){throw 'Missing actual after-save pixel assertion'}
        $saveCompare=[scriptblock]::Create($afterSave[0].Extent.Text)
        & $saveCompare;$checks++
        $original=$script:exportObserved
        $color=$actual.GetPixel(140,115)
        foreach($case in @(@{name='one-output-LSB';color=[Drawing.Color]::FromArgb($color.A,($color.R -bxor 1),$color.G,$color.B)},@{name='alpha';color=[Drawing.Color]::FromArgb(($color.A -bxor 1),$color.R,$color.G,$color.B)})){
            $actual.SetPixel(140,115,$case.color);$changed=Join-Path $directory ($case.name+'.png');$actual.Save($changed,[Drawing.Imaging.ImageFormat]::Png)
            $script:exportObserved=Read-BlenderDocumentPixels $changed @(160,128)
            Reject $case.name {& $compare} 'Blender document pixels differ: regression-session-restart'
            Reject ('after-save '+$case.name) {& $saveCompare} 'Blender document pixels differ: regression-after-save'
            $actual.SetPixel(140,115,$color)
        }
        $script:exportObserved=$original.PSObject.Copy();$script:exportObserved.width=159
        Reject 'comparison dimensions' {& $compare} 'Blender document pixels differ: regression-session-restart'
        $script:exportObserved=$original
        Reject 'decoded dimensions' {Read-BlenderDocumentPixels $actualPath @(159,128)} 'Blender export dimensions differ from the original drawing'
        $partial=Join-Path $directory 'partial.png';$bytes=[IO.File]::ReadAllBytes($actualPath);[IO.File]::WriteAllBytes($partial,[byte[]]$bytes[0..($bytes.Length-6)])
        Reject 'partial PNG' {Read-BlenderDocumentPixels $partial @(160,128)} 'Blender export is not a complete PNG'
        $savedAssertion=@($journey.Body.EndBlock.Statements|Where-Object {$_.Extent.Text.StartsWith('Assert-BlenderSavedFile ')})
        if($savedAssertion.Count -ne 1){throw 'Missing independent actual saved-file assertion'}
        $savedCheck=[scriptblock]::Create($savedAssertion[0].Extent.Text)
        & $savedCheck;$checks++
        [IO.File]::WriteAllBytes($file,[byte[]](1,2,3,5))
        Reject 'saved file SHA' {& $savedCheck} 'Native Blender session restart changed its saved file'
        [IO.File]::WriteAllBytes($file,[byte[]](1,2,3,4))
        $seedDocument=$paintedDocument.PSObject.Copy();$seedBytes=[Convert]::FromBase64String($seedDocument.pixels);$seedBytes[0]=$seedBytes[0] -bxor 1;$seedDocument.pixels=[Convert]::ToBase64String($seedBytes)
        $blankDocument=$seedDocument.PSObject.Copy();$seedBytes[1]=$seedBytes[1] -bxor 1;$blankDocument.pixels=[Convert]::ToBase64String($seedBytes)
        $seedCheckpoint='seed';$paintedCheckpoint='painted';$blankCheckpoint='blank'
        $loop=@($journey.Body.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.ForEachStatementAst] -and $_.Variable.VariablePath.UserPath -eq 'history'})
        if($loop.Count -ne 1){throw 'Missing actual four-state history loop'}
        $states=@(& ([scriptblock]::Create($loop[0].Condition.Extent.Text)))
        if($states.Count -ne 4 -or ($states.action -join ',') -cne 'Undo,Redo,Undo,Undo' -or ($states.checkpoint -join ',') -cne 'seed,painted,seed,blank'){throw 'Blender history lost its four original actions/checkpoints'}
        $historyAssertion=@($loop[0].Body.Statements|Where-Object {$_.Extent.Text.StartsWith('Assert-BlenderDocumentPixels ')})
        $checkpointAssertion=@($loop[0].Body.Statements|Where-Object {$_.Extent.Text.Contains("throw 'Blender Undo or Redo did not restore the exact authored checkpoint and layer order'")})
        if($historyAssertion.Count -ne 1 -or $checkpointAssertion.Count -ne 1){throw 'Missing actual exact history pixel/checkpoint assertions'}
        $historyCheck=[scriptblock]::Create($historyAssertion[0].Extent.Text);$checkpointCheck=[scriptblock]::Create($checkpointAssertion[0].Extent.Text)
        function Drawing-Checkpoint{$script:checkpointObserved}
        $wanted=@($seedDocument,$paintedDocument,$seedDocument,$blankDocument)
        for($historyIndex=0;$historyIndex -lt 4;$historyIndex++){
            $history=$states[$historyIndex];$script:exportObserved=$wanted[$historyIndex];$script:checkpointObserved=$history.checkpoint
            & $historyCheck;& $checkpointCheck;$checks++
            $script:exportObserved=if($historyIndex -eq 1){$seedDocument}else{$paintedDocument}
            Reject ('swapped history '+$historyIndex) {& $historyCheck} ('Blender document pixels differ: regression-history-'+$historyIndex)
            $script:checkpointObserved='wrong'
            Reject ('history checkpoint '+$historyIndex) {& $checkpointCheck} 'Blender Undo or Redo did not restore the exact authored checkpoint and layer order'
        }
    }finally{$expected.Dispose();$actual.Dispose()}

    $known=Join-Path $directory 'known-rgba.png';[IO.File]::WriteAllBytes($known,[Convert]::FromBase64String('iVBORw0KGgoAAAANSUhEUgAAAAQAAAACCAYAAAB/qH1jAAAAK0lEQVR4nGNQbfzLwMjEzPi/gZFRUPFnPcM/+/p6hv/C/08Gsv+LdXjPAADAMQzNEdy8ggAAAABJRU5ErkJggg=='))
    $decoded=Read-BlenderDocumentPixels $known @(4,2)
    if($decoded.encoding -cne 'straight BGRA8' -or $decoded.pixels -cne '/YElAAMCAQEBgP8B+SERf38//n8T/wD/B1HJ/u9AXQA='){throw 'Native decoder lost known straight BGRA8, alpha 0/1/127 or hidden RGB'};$checks++
    $recipe=[pscustomobject]@{format='Png';profile=@{profile=@{Builtin='Srgb'};channels='Rgb'};depth='U8';background='Preserve';size='Original';encoding=@{dither='None';conversion=@{intent='RelativeColorimetric';black_point_compensation=$false}}}
    Assert-BlenderExportRecipe $recipe @(4,2) @(4,2);$checks++
    foreach($entry in @(@{field='format';value='Jpeg'},@{field='depth';value='U16'},@{field='background';value='White'},@{field='size';value='Fit'})){
        $bad=$recipe.PSObject.Copy();$bad.($entry.field)=$entry.value
        Reject ('recipe '+$entry.field) {Assert-BlenderExportRecipe $bad @(4,2) @(4,2)} 'Blender export recipe is not original-size*'
    }
    $bad=$recipe|ConvertTo-Json -Depth 10|ConvertFrom-Json;$bad.encoding.dither='Stochastic8'
    Reject 'recipe dither' {Assert-BlenderExportRecipe $bad @(4,2) @(4,2)} 'Blender export recipe is not original-size*'
    $bad=$recipe|ConvertTo-Json -Depth 10|ConvertFrom-Json;$bad.profile.profile.Builtin='DisplayP3'
    Reject 'recipe profile' {Assert-BlenderExportRecipe $bad @(4,2) @(4,2)} 'Blender export recipe is not original-size*'
    $bad=$recipe|ConvertTo-Json -Depth 10|ConvertFrom-Json;$bad.encoding.conversion.intent='Perceptual'
    Reject 'recipe intent' {Assert-BlenderExportRecipe $bad @(4,2) @(4,2)} 'Blender export recipe is not original-size*'
    $bad=$recipe|ConvertTo-Json -Depth 10|ConvertFrom-Json;$bad.encoding.conversion.black_point_compensation=$true
    Reject 'recipe compensation' {Assert-BlenderExportRecipe $bad @(4,2) @(4,2)} 'Blender export recipe is not original-size*'
    Reject 'recipe output extent' {Assert-BlenderExportRecipe $recipe @(4,2) @(8,2)} 'Blender export recipe is not original-size*'
    $export=[pscustomobject]@{path=$known};$written=(Get-Item -LiteralPath $known).LastWriteTimeUtc
    $view=[pscustomobject]@{state=@{document_file=@{busy=$false;export_uri=$known}};windows_document=$null}
    if(Test-BlenderExportComplete $export $written $view){throw 'Export receipt accepted a stale destination'};$checks++
    if(!(Test-BlenderExportComplete $export $written.AddTicks(-1) $view)){throw 'Export receipt rejected a fresh completed destination'};$checks++
    $view.state.document_file.busy=$true
    if(Test-BlenderExportComplete $export $written.AddTicks(-1) $view){throw 'Export receipt accepted a busy document'};$checks++
    $view.state.document_file.busy=$false;$view.state.document_file.export_uri='wrong'
    if(Test-BlenderExportComplete $export $written.AddTicks(-1) $view){throw 'Export receipt accepted a wrong destination'};$checks++
    $view.state.document_file.export_uri=$known;$view.windows_document=@{stage='repeat_write'}
    if(Test-BlenderExportComplete $export $written.AddTicks(-1) $view){throw 'Export receipt accepted a pending workflow'};$checks++
    $view.windows_document=@{error='failed output'}
    Reject 'failed export' {Test-BlenderExportComplete $export $written.AddTicks(-1) $view} 'Blender export failed: failed output'
}finally{$bitmap.Dispose();Remove-Item -LiteralPath $directory -Recurse -Force}
Write-Output "Blender document pixels passed $checks cases: actual restart/history assertions, straight BGRA8 and export receipts"
