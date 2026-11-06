function Read-BlenderDocumentPixels([string]$Path,[int[]]$Extent){
    $png=[IO.File]::ReadAllBytes($Path)
    if($png.Length -lt 45 -or [BitConverter]::ToString($png[0..7]).Replace('-','') -cne '89504E470D0A1A0A' -or [BitConverter]::ToString($png[($png.Length-8)..($png.Length-1)]).Replace('-','') -cne '49454E44AE426082'){throw 'Blender export is not a complete PNG'}
    $bitmap=[Drawing.Bitmap]::new($Path);$locked=$null
    try{
        if($bitmap.Width -ne $Extent[0] -or $bitmap.Height -ne $Extent[1]){throw 'Blender export dimensions differ from the original drawing'}
        $rowBytes=$bitmap.Width*4;$pixels=[byte[]]::new($rowBytes*$bitmap.Height)
        $locked=$bitmap.LockBits([Drawing.Rectangle]::new(0,0,$bitmap.Width,$bitmap.Height),[Drawing.Imaging.ImageLockMode]::ReadOnly,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        for($y=0;$y -lt $bitmap.Height;$y++){
            [Runtime.InteropServices.Marshal]::Copy([IntPtr]::Add($locked.Scan0,$y*$locked.Stride),$pixels,$y*$rowBytes,$rowBytes)
        }
        [pscustomobject]@{path=$Path;width=$bitmap.Width;height=$bitmap.Height;encoding='straight BGRA8';pixels=[Convert]::ToBase64String($pixels)}
    }finally{if($locked){$bitmap.UnlockBits($locked)};$bitmap.Dispose()}
}
function Assert-BlenderExportRecipe($Recipe,[int[]]$Extent,[int[]]$Output){
    if($Recipe.format -cne 'Png' -or $Recipe.profile.profile.Builtin -cne 'Srgb' -or $Recipe.profile.channels -cne 'Rgb' -or $Recipe.depth -cne 'U8' -or $Recipe.background -cne 'Preserve' -or $Recipe.size -cne 'Original' -or $Recipe.encoding.dither -cne 'None' -or $Recipe.encoding.conversion.intent -cne 'RelativeColorimetric' -or $Recipe.encoding.conversion.black_point_compensation -or $Output.Count -ne 2 -or ($Output -join ',') -cne ($Extent -join ',')){throw 'Blender export recipe is not original-size straight sRGB RGBA8 PNG without matte or dither'}
}
function Test-BlenderExportComplete($Export,[datetime]$Written,$View){
    if($View.windows_document.error){throw ('Blender export failed: '+$View.windows_document.error)}
    !$View.state.document_file.busy -and !$View.windows_document -and $View.state.document_file.export_uri -ceq $Export.path -and (Test-Path -LiteralPath $Export.path) -and (Get-Item -LiteralPath $Export.path).LastWriteTimeUtc -gt $Written
}
function Export-BlenderPixels([string]$Name,$Export,[string]$Project=''){
    $before=Blender-SavedState;$hash=if($Project){(Get-FileHash -LiteralPath $Project).Hash}else{$null}
    $written=if(Test-Path -LiteralPath $Export.path){(Get-Item -LiteralPath $Export.path).LastWriteTimeUtc}else{[datetime]::MinValue}
    if(!$Export.recipe){
        if(Test-Path -LiteralPath $Export.path){throw 'Blender first export must use a fresh private path'}
        & (Join-Path $script:CapyScripts 'open-application-menu.ps1') -Root $root -Name 'File';Invoke 'export_document'
        Wait-Until {(Model).windows_document.stage -eq 'options'} 'Blender export options did not open' 45
        $details=(Model).windows_document.details;$copy=$details.form.copy
        Combo-Select (Control 'export-format') {$_.Current.Name -ceq $copy.format_png}
        $profiles=@($details.form.profiles);$index=-1
        for($i=0;$i -lt $profiles.Count;$i++){if($profiles[$i].profile.Builtin -ceq 'Srgb'){$index=$i;break}}
        if($index -lt 0){throw 'Blender export has no built-in sRGB profile'}
        $profileName=$details.form.profile_names[$index]
        Combo-Select (Control 'export-profile') {$_.Current.Name -ceq $profileName}
        Combo-Select (Control $copy.depth -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)) {$_.Current.Name -ceq $copy.depth_8}
        Combo-Select (Control 'export-background') {$_.Current.Name -ceq $copy.keep_transparency}
        Combo-Select (Control $copy.intent -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)) {$_.Current.Name -ceq $copy.relative}
        Combo-Select (Control $copy.dither -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)) {$_.Current.Name -ceq $copy.dither_none}
        $resize=(Control $copy.fit_bounds -Name -Type ([System.Windows.Automation.ControlType]::CheckBox)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
        if($resize.Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off){$resize.Toggle()}
        Invoke $copy.preview -Name
        Wait-Until {(Model).windows_document.stage -eq 'preview'} 'Blender export preview did not finish' 60
        $preview=(Model).windows_document.details
        Assert-BlenderExportRecipe $preview.recipe $Export.extent $preview.output_extent
        $Export.recipe=$preview.recipe
        Invoke $copy.export -Name
        Submit-SavePath $Export.path
    }else{
        if((Model).state.document_file.export_uri -cne $Export.path){throw 'Blender Export Again lost its acknowledged destination'}
        & (Join-Path $script:CapyScripts 'open-application-menu.ps1') -Root $root -Name 'File';Invoke 'export_again'
    }
    Wait-Until {(Test-BlenderExportComplete $Export $written (Model)) -and (Control 'drawing-canvas').Current.IsEnabled -and !(Find 'Save As' -Name)} 'Blender export did not acknowledge a fresh completed destination write' 45
    if((Blender-SavedState) -cne $before -or ($Project -and (Get-FileHash -LiteralPath $Project).Hash -cne $hash)){throw 'Blender export changed authored state, settings, history, camera or saved project'}
    $path=Join-Path $run ($Name+'.png');Copy-Item -LiteralPath $Export.path -Destination $path
    try{
        $image=Read-BlenderDocumentPixels $path $Export.extent
        Assert-BlenderExportRecipe $Export.recipe $Export.extent @($image.width,$image.height)
    }catch{
        @{error=$_.Exception.Message;export=$Export;state=($before|ConvertFrom-Json);project_sha256=$hash;png_sha256=(Get-FileHash -LiteralPath $path).Hash}|ConvertTo-Json -Depth 35|Set-Content -LiteralPath (Join-Path $run ($Name+'-export-failure.json'))
        throw
    }
    $image|Add-Member -NotePropertyMembers @{recipe=$Export.recipe;state=($before|ConvertFrom-Json);project_sha256=$hash}
    $image
}
function Blender-PixelEvidence($Image){
    $sha=[Security.Cryptography.SHA256]::Create()
    try{[ordered]@{path=$Image.path;width=$Image.width;height=$Image.height;encoding=$Image.encoding;bgra8_sha256=[BitConverter]::ToString($sha.ComputeHash([Convert]::FromBase64String($Image.pixels))).Replace('-','');recipe=$Image.recipe;state=$Image.state;project_sha256=$Image.project_sha256}}finally{$sha.Dispose()}
}
function Assert-BlenderDocumentPixels($Expected,$Actual,[string]$Name,$Export=$null){
    if(!$Expected.pixels -or !$Actual.pixels -or $Expected.encoding -cne 'straight BGRA8' -or $Actual.encoding -cne $Expected.encoding -or $Expected.width -ne $Actual.width -or $Expected.height -ne $Actual.height -or $Expected.pixels -cne $Actual.pixels){
        @{expected=(Blender-PixelEvidence $Expected);actual=(Blender-PixelEvidence $Actual);export=$Export;project_sha256=$(if($Export.project){(Get-FileHash -LiteralPath $Export.project).Hash}else{$null})}|ConvertTo-Json -Depth 35|Set-Content -LiteralPath (Join-Path $run ($Name+'-pixels-failure.json'))
        throw ('Blender document pixels differ: '+$Name)
    }
}
function Assert-BlenderSavedFile([string]$Path,[string]$Expected){
    $actual=(Get-FileHash -LiteralPath $Path).Hash
    if($actual -cne $Expected){
        @{path=$Path;expected_sha256=$Expected;actual_sha256=$actual}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $run ([IO.Path]::GetFileName($Path)+'-file-failure.json'))
        throw 'Native Blender session restart changed its saved file'
    }
}
