param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
public static class CapyRuntimeFilterCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/runtime-filters/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE','CAPY_FILTERS_DIR','CAPY_FILTERS_MODE')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}

$script:latestSnapshot=$null
function Model {
    # Trace publication can overlap this read. Keep only the last complete,
    # isolated snapshot from this exact process; new-value waits still time out.
    try{
        $s=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json
        if($s.process_id -eq $review.Id -and $s.model.windows_isolated_settings){$script:latestSnapshot=$s}
    }catch{}
    if($script:latestSnapshot.process_id -eq $review.Id){$script:latestSnapshot.model}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=5){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Condition){return};$review.Refresh();if($review.HasExited){throw 'Effects review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $match=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$match=[System.Windows.Automation.AndCondition]::new($match,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$match)
}
function Control([string]$Value,[switch]$Name,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name -Type $Type;$null -ne $hit.item} "Missing $Value"
    $hit.item
}
function Invoke([string]$Value,[switch]$Name){
    $hit=@{item=$null}
    Wait-Until {
        $hit.item=Find $Value -Name:$Name -Type ([System.Windows.Automation.ControlType]::Button)
        if(!$hit.item){$hit.item=Find $Value -Name:$Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
        $null -ne $hit.item
    } "Missing action: $Value"
    $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Edit([string]$Id,[string]$Text){
    $entry=Control $Id;$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
}
function Property([string]$Key){(Model).state.layer_properties.controls|Where-Object {$_.key -eq $Key}}
function Choose([string]$Id,[string]$Option){
    (Control $Id).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Option -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Select-Panel([string]$Id){
    # Repeating an active tab opens panel configuration. Insertion can select
    # Properties itself, so select only when the shared active panel differs.
    if(@((Model).layout.groups|Where-Object {$_.active -eq $Id}).Count -eq 0){Invoke ('panel-tab-'+$Id)}
    Wait-Until {@((Model).layout.groups|Where-Object {$_.active -eq $Id}).Count -gt 0} "Panel $Id did not become active"
}
function Select-Filter([string]$Id,[string]$Label){
    Select-Panel 'adjustments'
    Wait-Until {@((Model).layout.groups|Where-Object {$_.active -eq 'adjustments'}).Count -gt 0} 'Filters tab did not open'
    if($null -ne (Model).state.filter_picker.search){Invoke 'filter-search-toggle'}
    Choose 'filter-category' 'All filters'
    Invoke 'filter-search-toggle'
    Edit 'filter-search' $Label
    Wait-Until {(Model).state.filter_picker.search -eq $Label} 'Search not acknowledged'
    Wait-Until {
        $buttons=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))
        $rows=@($buttons|Where-Object {$_.Current.AutomationId.StartsWith('filter-') -and $_.Current.AutomationId -ne 'filter-search-toggle'})
        $rows.Count -eq (Model).state.adjustments.Count -and $null -ne (Find ('filter-'+$Id))
    } 'Search rows did not reach the native tree'
    Invoke ('filter-'+$Id)
    Wait-Until {(Model).state.layer_properties.description -eq $Label} 'Inserted filter did not become active'
    Select-Panel 'properties'
    Wait-Until {@((Model).layout.groups|Where-Object {$_.active -eq 'properties'}).Count -gt 0} 'Properties did not appear'
}
function Preview-Hash([string]$Id){
    $image=Control ('filter-preview-'+$Id)
    if($image.Current.ItemStatus -ne 'Ready'){return ''}
    $r=$image.Current.BoundingRectangle
    $window=[CapyRuntimeFilterCapture+Rect]::new()
    if(![CapyRuntimeFilterCapture]::GetWindowRect($review.MainWindowHandle,[ref]$window)){throw 'Window bounds unavailable'}
    $bitmap=[Drawing.Bitmap]::new($window.right-$window.left,$window.bottom-$window.top)
    try{
        $graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
        try{if(![CapyRuntimeFilterCapture]::PrintWindow($review.MainWindowHandle,$dc,2)){throw 'App capture failed'}}
        finally{$graphics.ReleaseHdc($dc);$graphics.Dispose()}
        $area=[Drawing.Rectangle]::new([int]$r.X-$window.left+2,[int]$r.Y-$window.top+2,[int]$r.Width-4,[int]$r.Height-4)
        $crop=$bitmap.Clone($area,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $stream=[IO.MemoryStream]::new();$sha=[Security.Cryptography.SHA256]::Create()
        try{$crop.Save($stream,[Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($sha.ComputeHash($stream.ToArray()))}
        finally{$crop.Dispose();$stream.Dispose();$sha.Dispose()}
    }finally{$bitmap.Dispose()}
}
function Capture([string]$Name){
    Start-Sleep -Milliseconds 250 # Allow acknowledged layout/theme changes to reach composition.
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
}

function Load-State {(Model).windows_filter_load}
function Reload([switch]$Failure) {
    $id=(Load-State).request_id
    Invoke 'Test filter reload' -Name
    Wait-Until {(Load-State).request_id -gt $id -and !(Load-State).pending} 'Runtime filter reload did not finish' 60
    if($Failure){
        if(!(Load-State).error){throw 'Invalid package did not report an error'}
    }else{
        if((Load-State).phase -ne 'ready' -or (Load-State).error){throw ((Load-State)|ConvertTo-Json)}
    }
}
try {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    $package=Join-Path $run 'package'
    [IO.Directory]::CreateDirectory($package)|Out-Null
    foreach($name in @('manifest.json','tent.wgsl','prepare.wgsl')){
        Copy-Item -LiteralPath (Join-Path $repo ('examples/filters/tent-blur/'+$name)) -Destination (Join-Path $package $name)
    }
    $manifestPath=Join-Path $package 'manifest.json'
    $shaderPath=Join-Path $package 'tent.wgsl'
    $shader=[IO.File]::ReadAllText($shaderPath)
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_FILTERS_DIR=$package;$env:CAPY_FILTERS_MODE='add'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $beforeBinary=(Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/runtime-filters-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned runtime filter review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and !(Load-State).pending} 'Runtime filter startup did not finish' 60
    if((Load-State).phase -ne 'ready'){throw ((Load-State)|ConvertTo-Json)}
    [CapyRuntimeFilterCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.modified} 'Controlled drawing did not finish'
    Select-Panel 'adjustments'
    Choose 'filter-category' 'All filters'
    Invoke 'filter-search-toggle';Edit 'filter-search' 'Tent Blur'
    Wait-Until {$null -ne (Find 'filter-example:tent_blur')} 'Runtime filter did not appear in native picker'
    Wait-Until {(Preview-Hash 'example:tent_blur') -ne ''} 'Runtime filter preview did not render' 30
    Invoke 'filter-example:tent_blur'
    Wait-Until {(Model).state.layer_properties.description -eq 'Tent Blur'} 'Runtime filter did not insert'
    Select-Panel 'properties'
    $radius=Control 'Radius' -Name -Type ([System.Windows.Automation.ControlType]::Edit)
    $radius.SetFocus();$radius.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('7')
    (Control 'Drawing canvas' -Name).SetFocus()
    Wait-Until {(Property 'radius').value.value -eq 7} 'Runtime parameter did not use shared numeric controls'
    Capture 'tent-controls'
    Select-Panel 'adjustments'
    Wait-Until {(Preview-Hash 'example:tent_blur') -ne ''} 'Preview after insertion did not render' 30
    $initialPreview=Preview-Hash 'example:tent_blur'
    Select-Panel 'properties'

    $manifest=Get-Content -LiteralPath $manifestPath -Raw|ConvertFrom-Json
    $manifest.filters[0].program.label='Tinted Tent'
    [IO.File]::WriteAllText($manifestPath,($manifest|ConvertTo-Json -Depth 60))
    if(!$shader.Contains('return value;')){throw 'Expected example shader return not found'}
    $changed=$shader.Replace('return value;','return vec4<f32>(value.r*0.5,value.g,value.b,value.a);')
    [IO.File]::WriteAllText($shaderPath,$changed)
    $catalog=(Model).state.filter_catalog_revision
    Reload
    Wait-Until {(Model).state.filter_catalog_revision -gt $catalog -and (Model).state.layer_properties.description -eq 'Tinted Tent'} 'Runtime metadata/program replacement did not publish'
    if((Property 'radius').value.value -ne 7){throw 'Replacement reset a compatible parameter value'}
    Capture 'replacement-controls'
    $catalog=(Model).state.filter_catalog_revision
    $revision=(Model).state.document_file.revision
    [IO.File]::WriteAllText($shaderPath,'this is not valid WGSL')
    Reload -Failure
    if((Model).state.filter_catalog_revision -ne $catalog -or (Model).state.document_file.revision -ne $revision -or (Property 'radius').value.value -ne 7){throw 'Invalid WGSL changed the working catalog, document or values'}
    Capture 'rejected-wgsl'

    [IO.File]::WriteAllText($shaderPath,$changed)
    Reload
    $catalog=(Model).state.filter_catalog_revision
    $revision=(Model).state.document_file.revision
    $manifest.filters[0].program.wgsl=@('missing.wgsl')
    [IO.File]::WriteAllText($manifestPath,($manifest|ConvertTo-Json -Depth 60))
    Reload -Failure
    if((Model).state.filter_catalog_revision -ne $catalog -or (Model).state.document_file.revision -ne $revision){throw 'Missing module changed the working catalog or document'}
    $manifest.filters[0].program.wgsl=@('tent.wgsl')
    [IO.File]::WriteAllText($manifestPath,($manifest|ConvertTo-Json -Depth 60))
    Reload
    if((Property 'radius').value.value -ne 7){throw 'Retry lost the current parameter value'}
    Select-Panel 'adjustments'
    Edit 'filter-search' 'Tinted Tent'
    Wait-Until {(Model).state.filter_picker.search -eq 'Tinted Tent' -and $null -ne (Find 'filter-example:tent_blur')} 'New filter metadata did not refresh retained search'
    Wait-Until {(Preview-Hash 'example:tent_blur') -ne ''} 'Replacement preview did not render' 30
    if((Preview-Hash 'example:tent_blur') -eq $initialPreview){throw 'Replacement did not change the rendered preview'}
    Capture 'replacement-picker'
    if((Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash -ne $beforeBinary){throw 'Executable changed during runtime package checks'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{startup_package='passed';native_picker_and_properties='passed';live_wgsl_and_metadata='passed';compatible_values='passed';invalid_wgsl_preserves_work='passed';missing_module_preserves_work='passed';retry='passed';changed_gpu_preview='passed';unchanged_executable='passed';scope='native D3D12/UI Automation and controlled drawing; full-image GPU assertions and physical/performance acceptance remain separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
