Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
if(!('CapyWindowApi' -as [type])){Add-Type -TypeDefinition @"
using System;using System.Runtime.InteropServices;using System.Text;
public static class CapyWindowApi {
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint owner);
 [DllImport("user32.dll")]public static extern bool PostMessage(IntPtr h,uint m,UIntPtr w,IntPtr l);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int n);
 [DllImport("user32.dll")]public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,UIntPtr w,IntPtr l,uint f,uint t,out UIntPtr r);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,UIntPtr w,StringBuilder l,uint f,uint t,out UIntPtr r);
 public static void Path(IntPtr h,string s){UIntPtr r;SendMessageTimeout(h,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out r);SendMessageTimeout(h,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out r);foreach(char c in s)SendMessageTimeout(h,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out r);var actual=new StringBuilder(32768);SendMessageTimeout(h,13,new UIntPtr((uint)actual.Capacity),actual,2,2000,out r);if(actual.ToString()!=s)throw new Exception("Picker path mismatch");}
}
"@}
$CapyScripts=$PSScriptRoot
$CapyWaitSeconds=8
$CapyEach=$null
$CapyFind='first'
$CapyPopups=$false
$CapyTraceDirectory=$null
$CapyStateFile=$null
$CapyCacheModel=$false
$CapyCaptureDelay=0
$CapyTrace=$null
$CapyEnvironment=$null

function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=$script:CapyWaitSeconds,[switch]$Closing){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{
        if($script:CapyEach){& $script:CapyEach}
        try{if(& $Condition){return}}catch [System.Windows.Automation.ElementNotAvailableException]{}
        if($review){$review.Refresh();if($review.HasExited){if($Closing){return};throw "Owned review exited with code $($review.ExitCode): $Message"}}
        Start-Sleep -Milliseconds 50
    }while($watch.Elapsed.TotalSeconds -lt $Seconds*$(if($env:CAPY_WAIT_SCALE){[double]$env:CAPY_WAIT_SCALE}else{1}))
    throw $Message
}
function Update-CapyFixtureContext([scriptblock]$Update){
    if(!$env:CAPY_FIXTURE_CONTEXT){return}
    $context=@{}
    if(Test-Path -LiteralPath $env:CAPY_FIXTURE_CONTEXT){foreach($property in (Get-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json).PSObject.Properties){$context[$property.Name]=$property.Value}}
    & $Update $context
    $temporary=$env:CAPY_FIXTURE_CONTEXT+'.tmp-'+[guid]::NewGuid().ToString('N')
    $stream=[IO.FileStream]::new($temporary,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write)
    try{$bytes=[Text.UTF8Encoding]::new($false).GetBytes(($context|ConvertTo-Json -Depth 12));$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}finally{$stream.Dispose()}
    if([IO.File]::Exists($env:CAPY_FIXTURE_CONTEXT)){[IO.File]::Replace($temporary,$env:CAPY_FIXTURE_CONTEXT,[NullString]::Value)}else{[IO.File]::Move($temporary,$env:CAPY_FIXTURE_CONTEXT)}
}
function Register-CapyFixtureWindow([Diagnostics.Process]$Process,[long]$WindowHandle,[string]$TraceDirectory){
    if(!$env:CAPY_FIXTURE_CONTEXT){return}
    $owner=[uint32]0;[CapyWindowApi]::GetWindowThreadProcessId([IntPtr]$WindowHandle,[ref]$owner)|Out-Null
    if($WindowHandle -eq 0 -or $owner -ne $Process.Id){throw 'Fixture census window is not owned by the review'}
    $executable=$Process.Path
    $context=if(Test-Path -LiteralPath $env:CAPY_FIXTURE_CONTEXT){Get-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json}else{$null}
    $known=@($context.windows|Where-Object {$_.process_id -eq $Process.Id -and $_.hwnd -eq $WindowHandle})
    if($known.Count -gt 1 -or ($known.Count -eq 1 -and $known[0].executable -ne $executable)){throw 'Fixture census owner changed'}
    if(!$TraceDirectory){$TraceDirectory=if($known.Count){$known[0].trace_directory}elseif($script:CapyTraceDirectory){$script:CapyTraceDirectory}else{Split-Path -Parent $executable}}
    $entry=[ordered]@{process_id=$Process.Id;hwnd=$WindowHandle;executable=$executable;trace_directory=$TraceDirectory;observed_utc=[DateTime]::UtcNow.ToString('o')}
    $registryPath=Join-Path $TraceDirectory ("windows-$($Process.Id).json")
    if(Test-Path -LiteralPath $registryPath){
        $registry=Read-Snapshot $registryPath
        $registered=@($registry.windows|Where-Object hwnd -eq $WindowHandle)
        if($registry.process_id -ne $Process.Id -or $registered.Count -ne 1){throw 'Fixture census requires the registered owned drawing HWND'}
        $entry.window_id=[long]$registered[0].id
        $entry.registered_window=$registered[0]
    }
    Update-CapyFixtureContext {
        param($context)
        $windows=@($context.windows|Where-Object {$null -ne $_})
        $old=@($windows|Where-Object {$_.process_id -eq $entry.process_id -and $_.hwnd -eq $entry.hwnd})
        if($old.Count -gt 1 -or ($old.Count -eq 1 -and ($old[0].executable -ne $entry.executable -or $old[0].trace_directory -ne $entry.trace_directory -or ($null -ne $old[0].window_id -and $old[0].window_id -ne $entry.window_id)))){throw 'Fixture census ownership changed'}
        $context.windows=@($windows|Where-Object {!($_.process_id -eq $entry.process_id -and $_.hwnd -eq $entry.hwnd)})+@($entry)
    }
}
function Record-CapyFixtureClose([int]$ProcessId,[long]$WindowHandle,[bool]$WindowOnly,[ValidateSet('exercise-window original five-second authorization','image-rows original close_document exit30')][string]$Contract='exercise-window original five-second authorization'){
    if(!$env:CAPY_FIXTURE_CONTEXT){return}
    $entry=[ordered]@{process_id=$ProcessId;hwnd=$WindowHandle;contract=$Contract;result='pass';window_only=$WindowOnly;completed_utc=[DateTime]::UtcNow.ToString('o')}
    Update-CapyFixtureContext {param($context);$context.closes=@($context.closes)+@($entry)}
}
function Owned-ProcessWindows([Diagnostics.Process]$Process,[long]$WindowHandle){
    $windows=if($WindowHandle){@([System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$WindowHandle))}
        else{[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$Process.Id))}
    foreach($window in $windows){
        if(!$window){continue}
        $current=$window.Current;$handle=[IntPtr]$current.NativeWindowHandle;$owner=[uint32]0
        [CapyWindowApi]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
        if($handle -ne [IntPtr]::Zero -and $owner -eq $Process.Id -and $current.ProcessId -eq $Process.Id){
            [pscustomobject]@{Root=$window;Handle=$handle}
        }
    }
}
function Owned-ProcessSurface([Diagnostics.Process]$Process,[string]$AutomationId){
    $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$AutomationId)
    $found=@(foreach($window in Owned-ProcessWindows $Process){
        foreach($surface in $window.Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){
            if($surface.Current.ProcessId -eq $Process.Id -and $surface.Current.AutomationId -ceq $AutomationId){
                [pscustomobject]@{Root=$window.Root;Handle=$window.Handle;Surface=$surface}
            }
        }
    })
    if($found.Count -gt 1){throw "The owned process has multiple $AutomationId surfaces"}
    if($found.Count){$found[0]}
}
function Owned-DrawingWindow([Diagnostics.Process]$Process,[long]$WindowHandle,[string]$TraceDirectory=$script:CapyTraceDirectory){
    $directory=$TraceDirectory
    if(!$directory){$executable=$Process.Path;if(!$executable){return $null};$directory=Split-Path -Parent $executable}
    $registered=$false
    if($WindowHandle){
        $path=Join-Path $directory "windows-$($Process.Id).json"
        if(Test-Path $path){
            $trace=Read-Snapshot $path
            $registered=$trace.process_id -eq $Process.Id -and $WindowHandle -in $trace.windows.hwnd
        }
    }
    $canvas=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'drawing-canvas')
    $found=@(Owned-ProcessWindows $Process $WindowHandle|Where-Object {
        $registered -or $_.Root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$canvas)
    })
    if($found.Count -gt 1){throw 'The owned process has multiple drawing windows; specify a window handle'}
    if($WindowHandle -and $found.Count -ne 1){throw 'The specified window is not an owned drawing window'}
    if($found.Count){Register-CapyFixtureWindow $Process $found[0].Handle.ToInt64() $TraceDirectory;$found[0]}
}
function Same-Curve($Left,$Right){
    if($null -eq $Left -or $null -eq $Right -or $Left.Count -ne $Right.Count){return $false}
    for($i=0;$i -lt $Left.Count;$i++){
        if($Left[$i].Count -ne 2 -or $Right[$i].Count -ne 2){return $false}
        for($axis=0;$axis -lt 2;$axis++){if([single]$Left[$i][$axis] -ne [single]$Right[$i][$axis]){return $false}}
    }
    $true
}
function Maximize-DrawingWindow{
    [CapyWindowApi]::ShowWindow($drawingWindow,3)|Out-Null
    Wait-Until {$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Maximized} 'The owned drawing did not maximize'
    Control 'drawing-canvas' -Arranged|Out-Null
    Wait-Until {
        $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$camera=(Model).state.camera
        [Math]::Abs($camera.viewport[0]-$bounds.Width) -lt 2 -and [Math]::Abs($camera.viewport[1]-$bounds.Height) -lt 2 -and (Model).state.workspace_ready -and !(Model).windows_workspace.busy
    } 'The maximized native canvas did not receive its viewport and workspace acknowledgement'
}
function Find-ClearCanvasSamplePoint([Windows.Rect]$bounds,[Windows.Rect]$paper,[Windows.Rect]$panel,[int]$Dx=0,[int]$Dy=0){
    foreach($rectangle in @($bounds,$paper,$panel)){
        if($rectangle.IsEmpty-or$rectangle.Width-le0-or$rectangle.Height-le0){throw 'Canvas sample requires nonempty native rectangles'}
        foreach($value in @($rectangle.X,$rectangle.Y,$rectangle.Width,$rectangle.Height,$rectangle.Right,$rectangle.Bottom)){
            if([double]::IsNaN($value)-or[double]::IsInfinity($value)){throw 'Canvas sample requires finite native rectangles'}
        }
    }
    $area=[Windows.Rect]::Intersect($paper,$bounds)
    if($area.IsEmpty){throw 'No clear visible paper sample fits outside the utility'}
    $minDx=[Math]::Min(0.,[double]$Dx);$maxDx=[Math]::Max(0.,[double]$Dx)
    $minDy=[Math]::Min(0.,[double]$Dy);$maxDy=[Math]::Max(0.,[double]$Dy)
    $left=[Math]::Ceiling($area.Left+48-$minDx);$right=[Math]::Floor($area.Right-48-$maxDx)
    $top=[Math]::Ceiling($area.Top+48-$minDy);$bottom=[Math]::Floor($area.Bottom-48-$maxDy)
    $ranges=@(
        @($left,([Math]::Min($right,[Math]::Ceiling($panel.Left-48-$maxDx)-1)),$top,$bottom),
        @(([Math]::Max($left,[Math]::Floor($panel.Right+48-$minDx)+1)),$right,$top,$bottom),
        @($left,$right,$top,([Math]::Min($bottom,[Math]::Ceiling($panel.Top-48-$maxDy)-1))),
        @($left,$right,([Math]::Max($top,[Math]::Floor($panel.Bottom+48-$minDy)+1)),$bottom)
    )
    foreach($range in $ranges){
        if($range[0]-gt$range[1]-or$range[2]-gt$range[3]){continue}
        $x=[int]($range[0]+[Math]::Floor(($range[1]-$range[0])/2));$y=[int]($range[2]+[Math]::Floor(($range[3]-$range[2])/2))
        $sample=[System.Windows.Rect]::new($x-48+[Math]::Min(0,$Dx),$y-48+[Math]::Min(0,$Dy),96+[Math]::Abs([double]$Dx),96+[Math]::Abs([double]$Dy))
        if($bounds.Contains($sample) -and $paper.Contains($sample) -and !$panel.IntersectsWith($sample)){return @($x,$y)}
    }
    throw 'No clear visible paper sample fits outside the utility'
}
function Clear-CanvasPoint([string]$Utility,[int]$Dx=0,[int]$Dy=0){
    $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    $camera=(Model).state.camera;$tab=@((Model).state.tabs|Where-Object active)[0]
    if($camera.rotation -ne 0 -or $camera.flipped[0] -or $camera.flipped[1]){throw 'Canvas pixel sampling requires the initial unrotated camera'}
    $left=[Math]::Max($camera.work_area[0],$camera.translation[0]);$right=[Math]::Min($camera.work_area[0]+$camera.work_area[2],$camera.translation[0]+$tab.width*$camera.zoom)
    $top=[Math]::Max($camera.work_area[1],$camera.translation[1]);$bottom=[Math]::Min($camera.work_area[1]+$camera.work_area[3],$camera.translation[1]+$tab.height*$camera.zoom)
    $sx=$bounds.Width/$camera.viewport[0];$sy=$bounds.Height/$camera.viewport[1]
    $paper=[System.Windows.Rect]::new($bounds.X+$left*$sx,$bounds.Y+$top*$sy,($right-$left)*$sx,($bottom-$top)*$sy)
    $panel=(Control $Utility -Arranged).Current.BoundingRectangle
    Find-ClearCanvasSamplePoint $bounds $paper $panel $Dx $Dy
}
function Assert-RendererHealthy([int]$ProcessId,$Drawing){
    $owner=[uint32]0
    [CapyWindowApi]::GetWindowThreadProcessId($Drawing.Handle,[ref]$owner)|Out-Null
    if($Drawing.Handle -eq [IntPtr]::Zero -or $owner -ne $ProcessId -or $Drawing.Root.Current.ProcessId -ne $ProcessId){throw 'Renderer health requires the owned drawing window'}
    $status=$Drawing.Root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'canvas-status'))
    if($status){
        if($status.Current.ProcessId -ne $ProcessId){throw 'Renderer status belongs to another process'}
        if(!$status.Current.IsOffscreen -and $status.Current.Name -match '^(canvas renderer failed:|Unexpected render worker failure$)'){throw $status.Current.Name}
    }
}
function Screen-Pixels([int]$X,[int]$Y,[string]$Path='',[switch]$Cursor) {
    $bitmap=[Drawing.Bitmap]::new(96,96)
    $graphics=[Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($X-48,$Y-48,0,0,$bitmap.Size)
        if($Cursor){
            $nativeCursor=[CapyRowPointer]::Cursor()
            $canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
            if([CapyRowPointer]::GetForegroundWindow() -ne [IntPtr]$root.Current.NativeWindowHandle -or !$canvas.Contains([double]$nativeCursor.position.x,[double]$nativeCursor.position.y) -or [Math]::Abs($nativeCursor.position.x-$X) -gt 2 -or [Math]::Abs($nativeCursor.position.y-$Y) -gt 2){throw 'Cursor capture does not belong to the owned canvas point'}
            $dc=$graphics.GetHdc()
            try{[CapyRowPointer]::DrawCursor($dc,$nativeCursor,$X-48,$Y-48)}finally{$graphics.ReleaseHdc($dc)}
        }
        $lock=$bitmap.LockBits([Drawing.Rectangle]::new(0,0,96,96),[Drawing.Imaging.ImageLockMode]::ReadOnly,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {$bytes=[byte[]]::new($lock.Stride*96);[Runtime.InteropServices.Marshal]::Copy($lock.Scan0,$bytes,0,$bytes.Length)}finally{$bitmap.UnlockBits($lock)}
        if($Path){$bitmap.Save($Path)}
        $hash=[Security.Cryptography.SHA256]::Create()
        try{[BitConverter]::ToString($hash.ComputeHash($bytes)).Replace('-','')}finally{$hash.Dispose()}
    }finally{$graphics.Dispose();$bitmap.Dispose()}
}
function Wait-StablePixels([scriptblock]$Sample){
    $watch=[Diagnostics.Stopwatch]::StartNew();$last=& $Sample;$stable=0;$comparisons=0
    do{
        Start-Sleep -Milliseconds 100;$next=& $Sample;$comparisons++
        if($next -eq $last){$stable++}else{$stable=0};$last=$next
        if($stable -ge 3){return $last}
    }while($watch.Elapsed.TotalSeconds -lt 5)
    throw "Canvas pixels did not settle for comparison ($comparisons comparisons, $stable consecutive matches, $([Math]::Round($watch.Elapsed.TotalSeconds,3)) seconds)"
}
function Find([string]$Value,[switch]$Name,$Within=$root,$Type,[switch]$Visible,[switch]$NoPopups){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    if($NoPopups){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$root.Current.ProcessId))}
    if(!$Within){$Within=$root}
    $searchScope=$Within
    if(!$Name -and $Within -eq $root -and $Value -match '^layer-(?:row-)?\d+(?:-|$)'){
        $parent=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'layer-list'))
        $bounds=if($parent){$parent.Current.BoundingRectangle}else{$null}
        if(!$parent -or $parent.Current.IsOffscreen -or !$bounds -or $bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){
            if($NoPopups){$parent=Find 'layer-list' -Visible -Within $root -NoPopups}
            else{$parent=Find 'layer-list' -Visible -Within $root}
        }
        if($parent){$searchScope=$parent}
    }
    $found=$null
    if(!$Visible -and 'first' -eq $script:CapyFind){$found=$searchScope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)}
    else{
        $first=$searchScope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
        if($first -and !$first.Current.IsOffscreen){return $first}
        $items=@();if($first){$items=$searchScope.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)}
        foreach($item in $items){if(!$item.Current.IsOffscreen){return $item}}
        if(!$Visible -and 'prefer-visible' -eq $script:CapyFind -and $items.Count){$found=$items[0]}
    }
    if(!$found -and $script:CapyPopups -and !$NoPopups -and $Within -eq $root){
        $owned=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id)
        foreach($window in [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$owned)){
            if($window -eq $root){continue}
            foreach($item in $window.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){
                if(!$item.Current.IsOffscreen){return $item}
            }
        }
    }
    $found
}
function Control([string]$Value,[switch]$Name,$Within=$root,$Type,[int]$Seconds=$script:CapyWaitSeconds,[switch]$Arranged,[switch]$NoPopups){
    $hit=@{item=$null;bounds=$null;stable=0}
    Wait-Until {
        if(!$Arranged -or !$hit.item){
            if($NoPopups){$hit.item=Find $Value -Name:$Name -Within $Within -Type $Type -Visible:$Arranged -NoPopups}
            else{$hit.item=Find $Value -Name:$Name -Within $Within -Type $Type -Visible:$Arranged}
        }
        if(!$hit.item){return $false}
        if(!$Arranged){return $true}
        try{$bounds=$hit.item.Current.BoundingRectangle;$offscreen=$hit.item.Current.IsOffscreen}catch [System.Windows.Automation.ElementNotAvailableException]{$hit.item=$null;$hit.bounds=$null;$hit.stable=0;return $false}
        if($offscreen -or $bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){$hit.item=$null;$hit.bounds=$null;$hit.stable=0;return $false}
        if($bounds -eq $hit.bounds){$hit.stable++}else{$hit.bounds=$bounds;$hit.stable=0}
        $hit.stable -ge 2
    } "Missing control: $Value" $Seconds
    $hit.item
}
function Read-ControlSnapshot([string[]]$Ids){
    $conditions=@(foreach($id in $Ids){[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$id)})
    if(!$conditions.Count){return}
    $idsCondition=if($conditions.Count -eq 1){$conditions[0]}else{[System.Windows.Automation.OrCondition]::new([System.Windows.Automation.Condition[]]$conditions)}
    $condition=[System.Windows.Automation.AndCondition]::new($idsCondition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
    $cache=[System.Windows.Automation.CacheRequest]::new();$cache.TreeScope=[System.Windows.Automation.TreeScope]::Element
    $cache.TreeFilter=[System.Windows.Automation.Automation]::RawViewCondition;$cache.AutomationElementMode=[System.Windows.Automation.AutomationElementMode]::None
    foreach($property in @([System.Windows.Automation.AutomationElement]::AutomationIdProperty,[System.Windows.Automation.AutomationElement]::NameProperty,[System.Windows.Automation.AutomationElement]::IsEnabledProperty,[System.Windows.Automation.AutomationElement]::ItemStatusProperty,[System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.AutomationElement]::IsOffscreenProperty,[System.Windows.Automation.AutomationElement]::ProcessIdProperty)){$cache.Add($property)}
    $active=$cache.Activate()
    try{
        foreach($element in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){
            $info=$element.Cached
            if($info.ProcessId -eq $review.Id){[pscustomobject]@{AutomationId=$info.AutomationId;Name=$info.Name;IsEnabled=$info.IsEnabled;ItemStatus=$info.ItemStatus;ControlType=$info.ControlType;IsOffscreen=$info.IsOffscreen;ProcessId=$info.ProcessId}}
        }
    }finally{$active.Dispose()}
}
function Snapshot-Control($Snapshot,[string]$Id,$Type){
    $first=$null
    foreach($item in $Snapshot){
        if($item.AutomationId -cne $Id -or ($Type -and $item.ControlType -ne $Type)){continue}
        if($script:CapyFind -eq 'first' -or !$item.IsOffscreen){return $item}
        if(!$first){$first=$item}
    }
    if($script:CapyFind -eq 'prefer-visible'){$first}
}
function Scroll-Pattern($Control,[switch]$Focus){
    $provider=$Control;$pattern=$null
    if(!$provider.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$pattern)){
        $condition=[System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'PART_ScrollPresenter'),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::IsScrollPatternAvailableProperty,$true))
        $providers=@($Control.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition))
        if($providers.Count -ne 1){throw 'Native scroll container must have exactly one scroll provider'}
        $provider=$providers[0];$pattern=$provider.GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
    }
    if($Focus){
        $focusTarget=if($Control.Current.IsKeyboardFocusable){$Control}else{$provider}
        if(!$focusTarget.Current.IsKeyboardFocusable){
            $condition=[System.Windows.Automation.AndCondition]::new(
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::IsKeyboardFocusableProperty,$true),
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::IsOffscreenProperty,$false))
            $focusTarget=$provider.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
        }
        if(!$focusTarget){throw 'Native scroll container has no keyboard focus target'}
        $focusTarget.SetFocus();Wait-Until {$focusTarget.Current.HasKeyboardFocus} 'Native scroll container did not receive keyboard focus'
    }
    $pattern
}
function Focus-Keyboard($Element,[switch]$Backward){
    $Element.SetFocus()
    Wait-Until {$Element.Current.HasKeyboardFocus} 'Native control did not receive focus'
    if($Backward){[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x09)}else{[CapyRowPointer]::Key([uint32]$review.Id,0x09)}
    Wait-Until {!$Element.Current.HasKeyboardFocus} 'Keyboard navigation did not leave the native control'
    if($Backward){[CapyRowPointer]::Key([uint32]$review.Id,0x09)}else{[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x09)}
    Wait-Until {$Element.Current.HasKeyboardFocus} 'Keyboard navigation did not return focus to the native control'
}
function Control-Trace($Element){
    $walker=[System.Windows.Automation.TreeWalker]::RawViewWalker
    $items=@();$node=$Element
    for($i=0;$node -and $i -lt 25;$i++){
        $current=$node.Current;$scroll=$null
        $item=[ordered]@{id=$current.AutomationId;name=$current.Name;type=$current.ControlType.ProgrammaticName;process=$current.ProcessId;bounds=$current.BoundingRectangle;offscreen=$current.IsOffscreen;focused=$current.HasKeyboardFocus}
        if($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll)){$item.scroll=@{vertical=$scroll.Current.VerticallyScrollable;percent=$scroll.Current.VerticalScrollPercent;view_size=$scroll.Current.VerticalViewSize}}
        $items+=[pscustomobject]$item
        $node=$walker.GetParent($node)
    }
    $items
}
function Input-Trace([int]$X,[int]$Y){
    $point=[System.Windows.Point]::new($X,$Y)
    @{point=@($X,$Y);foreground=[CapyRowPointer]::GetForegroundWindow().ToInt64();owned_window=[long]$root.Current.NativeWindowHandle;hit=@(Control-Trace ([System.Windows.Automation.AutomationElement]::FromPoint($point)));focus=@(Control-Trace ([System.Windows.Automation.AutomationElement]::FocusedElement))}
}
function Scroll-Position($Control,[ValidateRange(0,100)][double]$Percent){
    $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$scroller=$walker.GetParent($Control);$scroll=$null
    while($scroller -and !$scroller.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll)){$scroller=$walker.GetParent($scroller)}
    if(!$scroll){throw 'The native options have no scroll provider'}
    if(!$scroll.Current.VerticallyScrollable){return}
    $scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,$Percent)
    Wait-Until {[Math]::Abs($scroll.Current.VerticalScrollPercent-$Percent) -lt .01} 'Native options did not acknowledge their scroll position'
}
function Invoke([string]$Value,[switch]$Name,$Within=$root){
    $item=Control $Value -Name:$Name -Within $Within;$pattern=$null
    if($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
    else{$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
}
function Read-Snapshot([string]$Path){
    for($attempt=0;$attempt -lt 20;$attempt++){
        try{
            $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
            $reader=[IO.StreamReader]::new($stream)
            try{$json=$reader.ReadToEnd()}finally{$reader.Dispose()}
        }catch{
            if($attempt -eq 19 -or $_.Exception.GetBaseException() -isnot [IO.IOException]){throw}
            Start-Sleep -Milliseconds 50
            continue
        }
        return ($json|ConvertFrom-Json)
    }
}
function Settings-File{
    $path=Join-Path $env:CAPY_STORAGE_DIR 'config/settings.json'
    [IO.Directory]::CreateDirectory((Split-Path -Parent $path))|Out-Null
    $path
}
function Trace-File([string]$Kind='ui-state',[switch]$Isolated){
    $folder=if($script:CapyTraceDirectory){$script:CapyTraceDirectory}else{$directory}
    foreach($path in [IO.Directory]::EnumerateFiles($folder,"$Kind-$($review.Id)-*.json")){
        if([IO.File]::GetLastWriteTimeUtc($path) -lt $review.StartTime.ToUniversalTime()){continue}
        try{$value=Read-Snapshot $path}catch{continue}
        if($value.process_id -eq $review.Id -and (!$Isolated -or $value.model.windows_isolated_settings)){return $path}
    }
}
function State-File{
    if($script:CapyStateFile){return $script:CapyStateFile}
    if(!$script:CapyTrace -or $script:CapyTrace.process -ne $review.Id){$script:CapyTrace=@{process=$review.Id;path=$null;model=$null}}
    if(!$script:CapyTrace.path){$script:CapyTrace.path=Trace-File -Isolated}
    $script:CapyTrace.path
}
function Search-State{
 $path=State-File;$base=Read-Snapshot $path
 $searchPath=Join-Path (Split-Path -Parent $path) ((Split-Path -Leaf $path) -replace '^ui-state-','search-state-')
 $search=Read-Snapshot $searchPath
 if($base.process_id -ne $review.Id -or $search.process_id -ne $review.Id -or $search.window_id -ne $base.window_id -or 'search' -notin $search.PSObject.Properties.Name){throw 'The search acknowledgement is not from the owned drawing'}
 $search.search
}
function Model{
    $model=$null
    try{
        $path=State-File
        if($path){
            $value=Read-Snapshot $path
            if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){
                $model=$value.model
                $camera=Join-Path (Split-Path -Parent $path) ((Split-Path -Leaf $path) -replace '^ui-state-','camera-state-')
                if($camera -ne $path -and [IO.File]::Exists($camera)){
                    try{
                        $view=Read-Snapshot $camera
                        if($view.process_id -eq $review.Id -and $view.window_id -eq $value.window_id -and $view.camera.revision -ge $model.state.camera.revision){$model.state.camera=$view.camera}
                    }catch{}
                }
            }
        }
    }catch{}
    if(!$script:CapyCacheModel -or !$script:CapyTrace){return $model}
    if($model){$script:CapyTrace.model=$model}
    if($script:CapyTrace.process -eq $review.Id){$script:CapyTrace.model}
}
function Capture([string]$Name,[switch]$WithModel,[switch]$Composed){
    if($script:CapyCaptureDelay){Start-Sleep -Milliseconds $script:CapyCaptureDelay}
    $handle=[IntPtr]$root.Current.NativeWindowHandle;$owner=[uint32]0
    [CapyWindowApi]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
    if($owner -ne $review.Id){throw 'The capture window no longer belongs to the owned process'}
    & (Join-Path $script:CapyScripts 'inspect-window.ps1') -ProcessId $review.Id -WindowHandle $handle.ToInt64() -Output (Join-Path $run ($Name+'.png')) -ClientOnly -Composed:$Composed -CaptureOnly:($Name -ne 'failure') *> (Join-Path $run ($Name+'.json'))
    $workspace=Workspace-Root
    if($workspace -and ('CapyRowPointer' -as [type])){
        $layout=@{presentation=($workspace.Current.ItemStatus|ConvertFrom-Json);dpi=[CapyRowPointer]::GetDpiForWindow($handle)}
        [IO.File]::WriteAllText((Join-Path $run ($Name+'-layout.json')),($layout|ConvertTo-Json -Depth 15))
    }
    if($WithModel){
        $model=Model
        [IO.File]::WriteAllText((Join-Path $run ($Name+'-model.json')),($model|ConvertTo-Json -Depth 80))
    }
}
function Enter-CapyEnvironment([string[]]$Names=@()){
    if($env:CAPY_FIXTURE_CONTEXT -and $run){Update-CapyFixtureContext {param($context);$context.Clear();$context.run=$run;$context.windows=@();$context.closes=@()}}
    $script:CapyEnvironment=@{}
    foreach($name in @('CAPY_STORAGE_DIR','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY')+$Names){
        $script:CapyEnvironment[$name]=[Environment]::GetEnvironmentVariable($name,'Process')
        Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue
    }
}
function Exit-CapyEnvironment{
    if(!$script:CapyEnvironment){return}
    if($env:CAPY_FIXTURE_CONTEXT -and $run){
        Update-CapyFixtureContext {param($context);$context.run=$run;$context.process_id=$review.Id;$context.trace_directory=$(if($script:CapyTraceDirectory){$script:CapyTraceDirectory}else{$directory});$context.profile=$env:CAPY_STORAGE_DIR}
    }
    foreach($entry in $script:CapyEnvironment.GetEnumerator()){
        if($null -eq $entry.Value){Remove-Item -LiteralPath ('Env:'+$entry.Key) -ErrorAction SilentlyContinue}
        else{[Environment]::SetEnvironmentVariable($entry.Key,$entry.Value,'Process')}
    }
}
function Invoke-Id([string]$Id){
    $hit=@{item=$null}
    Wait-Until {$hit.item=Find $Id;$hit.item -and $hit.item.Current.IsEnabled} "Missing enabled control: $Id"
    Invoke $Id
}
function New-DrawingControl([string]$Id,$Within=$root){
    $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id)
    $hit=$Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
    if($hit -or $Within -ne $root){return $hit}
    $owned=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
    [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$owned)
}
function New-DrawingPixels([int]$Width,[int]$Height){
    Wait-Until {(Model).state.new_drawing -and (New-DrawingControl 'document-width') -and (New-DrawingControl 'new-drawing-unit')} 'New drawing did not open' 30
    if((Model).state.new_drawing.setup.unit -ne 'px'){
        $index=[array]::IndexOf(@((Model).state.new_drawing.units.value),'px')
        $unit=New-DrawingControl 'new-drawing-unit'
        $unit.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
        $choice=@{item=$null};Wait-Until {$choice.item=New-DrawingControl "new-drawing-unit-$index" $unit;$null -ne $choice.item} 'New drawing has no pixel unit'
        $choice.item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
        Wait-Until {(Model).state.new_drawing.setup.unit -eq 'px'} 'New drawing did not switch to pixels'
    }
    Wait-Until {
        $view=(Model).state.new_drawing;$entry=New-DrawingControl 'document-width'
        $entry -and [double](($entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value) -replace '[^0-9.]','') -eq $view.values[0]
    } 'New drawing did not show pixel sizes'
    foreach($field in @(@('document-width',$Width),@('document-height',$Height))){
        $entry=New-DrawingControl $field[0];$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue([string]$field[1])
    }
}
function Tool-Tile([string]$Command){
    $target=@{id=$null}
    Wait-Until {foreach($panel in (Model).panels){foreach($tile in $panel.tiles){
        if($tile.control.command -eq $Command -or $tile.resolved_control.command -eq $Command){$target.id="tile-$($panel.id)-$($tile.id)";return $true}
    }};$false} "No $Command tile"
    $target.id
}
function Tool-Choice([string]$Command){
    $target=@{id=$null;list=$null;index=-1}
    Wait-Until {
        $set=(Model).state.tool_set
        foreach($list in @(@('groups','tool-group-'),@('subtools','tool-subtool-'))){
            $items=@($set.($list[0]))
            for($i=0;$i -lt $items.Count;$i++){$action=$items[$i].action
                if($action.command -eq $Command -or $action.variant.command -eq $Command){$target.id=$list[1]+$i;$target.list=$list[0];$target.index=$i;return $true}}
        }
        $false
    } "No $Command tool choice"
    $target
}
function Invoke-PickerButton($Picker,[string]$Id='1'){
    if($Picker.Current.ClassName -ne '#32770' -or $Picker.Current.ProcessId -ne $review.Id){throw 'Picker does not belong to the isolated review'}
    $window=[IntPtr]$Picker.Current.NativeWindowHandle;$hit=@{item=$null}
    Wait-Until {
        $fresh=[System.Windows.Automation.AutomationElement]::FromHandle($window)
        if($fresh.Current.ProcessId -ne $review.Id -or $fresh.Current.ClassName -ne '#32770'){return $false}
        $hit.item=$fresh.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
        $hit.item -and $hit.item.Current.ClassName -eq 'Button' -and $hit.item.Current.IsEnabled -and $hit.item.Current.NativeWindowHandle -ne 0
    } 'Native picker confirmation did not become ready' 15
    $handle=[IntPtr]$hit.item.Current.NativeWindowHandle;$owner=[uint32]0
    [CapyWindowApi]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
    if($owner -ne $review.Id){throw 'Native picker button has an unexpected owner'}
    if(![CapyWindowApi]::PostMessage($handle,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Cannot invoke native picker button'}
}
function Open-Project([string]$Path,[ValidateSet('open_document','import_image')][string]$Command='open_document'){
    & (Join-Path $script:CapyScripts 'open-application-menu.ps1') -Root $root -Name 'File'
    Invoke-Id $Command
    $hit=@{edit=$null}
    Wait-Until {$hit.edit=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit'),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')));$null -ne $hit.edit} 'Missing Open picker' 45
    [CapyWindowApi]::Path([IntPtr]$hit.edit.Current.NativeWindowHandle,$Path)
    Invoke-PickerButton (Find 'Open' -Name)
    Wait-Until {!(Find 'Open' -Name)} 'Open did not finish' 90
}
function Zoom-Item([string]$Id){
    $owned=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
    [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$owned)
}
function Fit-Canvas{
    if(!(Find 'canvas-view-info')){return}
    Invoke-Id 'canvas-view-info'
    $hit=@{item=$null}
    Wait-Until {$hit.item=Zoom-Item 'zoom-fit_canvas';$hit.item -and $hit.item.Current.IsEnabled} 'The zoom menu did not offer Fit'
    $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Zoom-Item 'zoom-fit_canvas')} 'The zoom menu did not close after Fit'
}
function Save-ProjectAs([string]$Path,[scriptblock]$NativeSaved){
    & (Join-Path $script:CapyScripts 'open-application-menu.ps1') -Root $root -Name 'File'
    Invoke-Id 'save_document_as'
    $hit=@{entry=$null;picker=$null}
    Wait-Until {
        $hit.picker=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'#32770'))
        if(!$hit.picker){return $false}
        $hit.entry=$hit.picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit'),
            [System.Windows.Automation.OrCondition]::new(
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148'),
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'))))
        $null -ne $hit.entry
    } 'The Save As filename edit did not appear' 45
    if($hit.picker.Current.ProcessId -ne $review.Id -or $hit.entry.Current.ProcessId -ne $review.Id){throw 'Wrong picker filename owner'}
    [CapyWindowApi]::Path([IntPtr]$hit.entry.Current.NativeWindowHandle,$Path)
    Invoke-PickerButton $hit.picker
    Wait-Until {
        if($NativeSaved){return (Test-Path -LiteralPath $Path) -and $root.Current.Name -eq ([IO.Path]::GetFileName($Path)+' · Capy Canvas') -and (& $NativeSaved $Path)}
        (Test-Path -LiteralPath $Path) -and (Model).state.document_file.location.uri -eq $Path -and !(Model).state.document_file.busy -and !(Model).state.document_file.modified
    } 'The drawing did not save' 90
    Wait-Until {!(Find 'Save As' -Name) -and (Control 'drawing-canvas').Current.IsEnabled} 'The Save As dialog did not close'
}
function Use-PaintWorkspace{
    $id='builtin:workspace:illustrator'
    if((Model).windows_workspace.id -eq $id){return}
    $found=@{switch=$null;menu=$null}
    Wait-Until {$found.switch=Find ('workspace-switch-'+$id) -Visible;$found.menu=Find 'header-workspace-menu' -Visible;$found.switch -or $found.menu} 'No Paint workspace switch' 15
    if(!$found.switch){Invoke-Id 'header-workspace-menu';$found.switch=Control 'Paint' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
    Wait-Until {try{$found.switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle();$true}catch{$false}} 'The Paint switch stayed unavailable' 20
    Wait-Until {(Model).windows_workspace.id -eq $id -and !(Model).windows_workspace.busy} 'Paint did not open' 30
    Wait-Until {Find 'color-edit' -Visible} 'The Paint workspace shows no color control' 20
}
function Workspace-Root{
    $traced=[System.Windows.Automation.NotCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ItemStatusProperty,''))
    $workspaces=@($root)
    foreach($route in @(
        @([System.Windows.Automation.ControlType]::Pane,'Microsoft.UI.Content.DesktopChildSiteBridge'),
        @([System.Windows.Automation.ControlType]::Pane,'InputSiteWindowClass'),
        @([System.Windows.Automation.ControlType]::Group,'NamedContainerAutomationPeer'),
        @([System.Windows.Automation.ControlType]::Group,'NamedContainerAutomationPeer'))){
        $condition=[System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$route[0]),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,$route[1]))
        $workspaces=@(foreach($parent in $workspaces){$parent.FindAll([System.Windows.Automation.TreeScope]::Children,$condition)})
        if(!$workspaces.Count){break}
    }
    foreach($local in @($true,$false)){
        if(!$local){$workspaces=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$traced)}
        foreach($workspace in $workspaces){
            $status=$workspace.Current.ItemStatus
            if(!$status.StartsWith('{')){continue}
            try{$presentation=$status|ConvertFrom-Json}catch{continue}
            if($presentation.PSObject.Properties.Name -contains 'groups' -and $presentation.PSObject.Properties.Name -contains 'elements'){return $workspace}
        }
    }
}
function Compact-Choice([string]$Id,[double]$Scale,[double]$Height=26){
    $box=Control $Id -Arranged
    $bounds=Native-Bounds $Id $Scale
    $selected=$box.GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern).Current.GetSelection()
    if($selected.Count -ne 1){throw "Compact choice $Id has no native selection"}
    $texts=@(foreach($text in $box.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)){
        $current=$text.Current
        if(!$current.IsOffscreen -and ($current.ControlType -eq [System.Windows.Automation.ControlType]::Text -or $current.ClassName -eq 'TextBlock')){
            @{name=$current.Name;bounds=$current.BoundingRectangle}
        }
    })
    $record=@{id=$Id;bounds=$bounds;selected=$selected[0].Current.Name;texts=$texts;dpi=96*$Scale;visual_review='Required when the closed caption has no native text provider'}
    $record|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run ($Id+'-compact-geometry.json'))
    if([Math]::Abs($bounds.Height/$Scale-$Height) -gt 1){throw "Compact choice $Id has the wrong height"}
    $contained=$bounds;$contained.Inflate(1,1)
    foreach($text in $texts){
        if($text.name -ne $record.selected -or !$contained.Contains($text.bounds) -or $text.bounds.Height/$Scale -lt 18){throw "Compact choice $Id clips or mislabels its visible caption"}
    }
    $record
}
function Native-Bounds([string]$Id,[double]$Scale,[switch]$Group){
    $hit=@{record=$null}
    Wait-Until {
        $workspace=Workspace-Root
        if(!$workspace){return $false}
        $presentation=$workspace.Current.ItemStatus|ConvertFrom-Json
        $records=if($Group){$presentation.groups}else{$presentation.elements}
        $hit.record=@($records|Where-Object id -eq $Id)[0]
        $bounds=if($Group){$hit.record.bounds}else{$hit.record.actual_bounds}
        $hit.record -and $bounds.width -gt 0 -and $bounds.height -gt 0
    } "Native arranged bounds did not become available: $Id"
    $bounds=if($Group){$hit.record.bounds}else{$hit.record.actual_bounds}
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen([IntPtr]$root.Current.NativeWindowHandle,[ref]$origin)){throw 'Native client origin is unavailable'}
    [Windows.Rect]::new($origin.x+$bounds.x*$Scale,$origin.y+$bounds.y*$Scale,$bounds.width*$Scale,$bounds.height*$Scale)
}
