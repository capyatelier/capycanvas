param([Parameter(Mandatory)][string]$Executable,[ValidateSet('light','dark')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/cursors/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$CapyWaitSeconds=30
$CapyPopups=$true
$checks=[ordered]@{theme=$Theme}
function Start-App {
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run ('stderr-'+[Guid]::NewGuid().ToString('N')+'.log'))
    $null=$review.Handle
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'Cursor review did not open' 60
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Cursor review did not prepare' 120
    if(!(Model).windows_isolated_settings){throw 'Cursor review requires isolated settings'}
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
}
function Close-App {
    [CapyRowPointer]::Dispose()
    $review.CloseMainWindow()|Out-Null
    Wait-Until {$review.Refresh();$review.HasExited} 'Cursor review did not close' 60 -Closing
}
function Cursor-Row {@((Model).preferences.pages.groups.rows|Where-Object id -eq 'cursor')[0]}
function Preferences {
    Invoke-Id 'settings-button'
    Wait-Until {(Model).preferences} 'Preferences did not open'
    Invoke-Id 'preference-page-input'
    Wait-Until {(Model).preferences.page -eq 'input' -and (Find 'preference-choice-cursor' -Visible)} 'Cursor choices did not appear'
    if(@((Cursor-Row).kind.options).Count -ne 12 -or @((Cursor-Row).kind.icons).Count -ne 12){throw 'Cursor choices must come from shared metadata'}
}
function Choose-Cursor([string]$Label,[string]$Mode) {
    $dropdown=(Control 'preference-choice-cursor').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
    $dropdown.Expand()
    (Control $Label -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    $dropdown.Collapse()
    Wait-Until {(Model).state.settings.cursor -eq $Mode} "Cursor mode did not change to $Mode"
    Wait-Until {(Read-Snapshot (Join-Path $env:CAPY_SETTINGS_DIRECTORY 'settings.json')).cursor -eq $Mode} 'Cursor choice did not persist'
    Capture ('settings-'+$Mode) -WithModel -Composed
    Invoke-Id 'CloseButton'
    Wait-Until {!(Model).preferences -and !(Find 'CloseButton' -Visible)} 'Preferences did not close'
}
function Tool([string]$Id) {
    $command=@((Model).state.commands|Where-Object id -eq $Id)[0]
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    Wait-Until {
        $canvas=Find 'drawing-canvas' -Visible
        if(!$canvas -or !$canvas.Current.IsEnabled -or !$canvas.Current.IsKeyboardFocusable){return $false}
        $canvas.SetFocus()
        [System.Windows.Automation.AutomationElement]::FocusedElement.Current.AutomationId -eq 'drawing-canvas'
    } 'Drawing canvas did not regain native keyboard focus'
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4B)
    Wait-Until {Find 'command-search' -Visible} 'Command search did not open'
    (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($command.label)
    Wait-Until {$row=Find 'command-result-0' -Visible;$row -and $row.Current.Name.StartsWith($command.label)} "Tool search did not find $Id"
    [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    Wait-Until {!(Find 'command-search' -Visible) -and @((Model).state.commands|Where-Object {$_.id -eq $Id -and $_.selected}).Count -eq 1 -and (Model).brush_ready} "Tool did not activate: $Id"
}
function Pixels([string]$Name='') {
    $bitmap=[Drawing.Bitmap]::new(96,96)
    $graphics=[Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($script:x-48,$script:y-48,0,0,$bitmap.Size)
        $lock=$bitmap.LockBits([Drawing.Rectangle]::new(0,0,96,96),[Drawing.Imaging.ImageLockMode]::ReadOnly,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {$bytes=[byte[]]::new($lock.Stride*96);[Runtime.InteropServices.Marshal]::Copy($lock.Scan0,$bytes,0,$bytes.Length)}finally{$bitmap.UnlockBits($lock)}
        if($Name){$bitmap.Save((Join-Path $run ($Name+'.png')))}
        [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
    }finally{$graphics.Dispose();$bitmap.Dispose()}
}
try {
    Enter-CapyEnvironment
    $previousDpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.Directory]::CreateDirectory($env:CAPY_SETTINGS_DIRECTORY)|Out-Null
    [IO.File]::WriteAllText((Join-Path $env:CAPY_SETTINGS_DIRECTORY 'settings.json'),(@{theme=$Theme;language=@{Explicit='en'}}|ConvertTo-Json -Depth 4))
    Start-App
    if((Model).state.theme -ne $Theme){throw 'Cursor review did not apply the requested theme'}
    $revision=(Model).state.document_file.revision
    foreach($mode in @(@('Tool','tool'),@('Tool and brush size','tool_brush_size'))) {
        Preferences
        Choose-Cursor $mode[0] $mode[1]
        foreach($device in @('mouse','pen')) {
            $seen=@()
            foreach($id in @('pen','pencil','brush','eraser','lasso','rectangle_select')) {
                Tool $id
                $canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$camera=(Model).state.camera
                $ratio=$canvas.Width/$camera.viewport[0]
                $script:x=[int]($canvas.X+($camera.work_area[0]+$camera.work_area[2]/2)*$ratio)
                $script:y=[int]($canvas.Y+($camera.work_area[1]+$camera.work_area[3]/2)*$ratio)
                [CapyRowPointer]::Hover($x+160,$y)
                $empty=Wait-StablePixels {Pixels}
                if($device -eq 'pen'){[CapyRowPointer]::PenHover($x,$y)}else{[CapyRowPointer]::Hover($x,$y)}
                Wait-Until {$hash=Pixels;$hash -ne $empty -and $seen -notcontains $hash} "$device $($mode[1]) $id did not present a distinct tool cursor"
                $icon=Wait-StablePixels {Pixels}
                if($icon -eq $empty -or $seen -contains $icon){throw "$device $($mode[1]) $id has no distinct presented tool cursor"}
                $seen+=$icon
                $null=Pixels "$device-$($mode[1])-$id"
                [CapyRowPointer]::PenLeave()
                [CapyRowPointer]::Hover($x+160,$y)
                Wait-Until {(Pixels) -eq $empty} 'Leaving the pointer did not clear the retained cursor'
                if((Wait-StablePixels {Pixels}) -ne $empty){throw 'Leaving the pointer did not clear the retained cursor'}
            }
            $checks["$device-$($mode[1])"]='six distinct presented tool icons; pointer leave clears'
        }
    }
    if((Model).state.document_file.revision -ne $revision){throw 'Cursor hover edited the drawing'}
    Close-App
    Start-App
    if((Model).state.settings.cursor -ne 'tool_brush_size'){throw 'Cursor mode did not survive reopening'}
    $checks.persistence='passed'
    Close-App
    $checks.evidence=$run
    $checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
}finally {
    [CapyRowPointer]::Dispose()
    if($previousDpi){[CapyRowPointer]::SetThreadDpiAwarenessContext($previousDpi)|Out-Null}
    Exit-CapyEnvironment
}
