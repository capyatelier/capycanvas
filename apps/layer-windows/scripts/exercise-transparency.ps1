param([Parameter(Mandatory)][string]$Executable,[ValidateSet('full','hover-dark','hover-light')][string]$Journey='full')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='visible'
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
$null=[CapyCanvasTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/transparency/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$ControlType=[System.Windows.Automation.ControlType]
Add-Type -AssemblyName System.Drawing
Add-Type -Name GlassWindow -Namespace Capy -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,int[] point);'
function Center($element){$b=$element.Current.BoundingRectangle;@{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
function Tap($at,[string]$Device='mouse'){[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up()}
function Switch-Workspace([string]$Name,[string]$Id){
    $found=@{switch=$null;menu=$null}
    Wait-Until {
        $found.switch=Find ('workspace-switch-'+$Id);$found.menu=Find 'header-workspace-menu'
        $found.switch -or $found.menu
    } "No workspace switcher for $Name" 15
    $switch=$found.switch
    if(!$switch){Invoke 'header-workspace-menu';$switch=Control $Name -Name -Type $ControlType::MenuItem}
    Wait-Until {try{$switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle();$true}catch{$false}} "$Name switch stayed unavailable" 20
    Wait-Until {(Model).windows_workspace.id -eq $Id -and !(Model).windows_workspace.busy} "$Name did not open" 20
}
function Preference([int]$Index,[string]$Theme){
    Invoke 'settings-button'
    $dialog=Control 'Preferences' -Name -Type $ControlType::Window
    if($Theme){
        $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
        $choice=Control 'preference-choice-theme' -Within $dialog;$choice.SetFocus()
        Wait-Until {$choice.Current.HasKeyboardFocus} 'Color theme did not take focus'
        [CapyRowPointer]::Key([uint32]$review.Id,[uint16]0x73)
        Wait-Until {Find 'Light' -Name -Type $ControlType::ListItem} 'Theme choices did not open'
        foreach($key in @(0x24)+@(@(0x28)*@('system','light','dark').IndexOf($Theme))+@(0x0D)){[CapyRowPointer]::Key([uint32]$review.Id,[uint16]$key)}
        Wait-Until {(Model).state.settings.theme -eq $Theme} "The $Theme theme did not apply"
    }
    $circle=Control ('preference-transparency-'+$Index)
    if($circle.Current.Name -ne @('Off','Low','Medium','High')[$Index]){throw "Transparency circle $Index is misnamed"}
    Invoke ('preference-transparency-'+$Index)
    Wait-Until {(Model).state.palette.glass.transparency -eq @('off','low','medium','high')[$Index]} "Transparency $Index did not apply"
    Wait-Until {$circle.Current.ItemStatus -eq 'Selected'} "Transparency $Index circle was not checked"
    $close=$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'CloseButton'),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$ControlType::Button)))
    $close.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Find 'Preferences' -Name -Type $ControlType::Window)} 'Preferences did not close'
}
function Sample([string]$Name,$Points){
    Start-Sleep -Milliseconds 900;Capture $Name -Composed
    $origin=[int[]]@(0,0);$null=[Capy.GlassWindow]::ClientToScreen($drawingWindow,$origin)
    $image=[System.Drawing.Bitmap]::new((Join-Path $run ($Name+'.png')))
    try{@($Points|ForEach-Object {$image.GetPixel($_.x-$origin[0],$_.y-$origin[1]).G})}finally{$image.Dispose()}
}
function Tab-Shades([string]$Name){
    $origin=[int[]]@(0,0);$null=[Capy.GlassWindow]::ClientToScreen($drawingWindow,$origin)
    $image=[System.Drawing.Bitmap]::new((Join-Path $run ($Name+'.png')))
    try{
        foreach($group in @((Model).layout.groups|Where-Object {$_.tabs_visible})){
            $tab=(Control ('panel-tab-'+$group.active) -Arranged).Current.BoundingRectangle
            $x=[int]($tab.X+4*$scale-$origin[0])
            $shade,$body=@(($tab.Bottom-3*$scale),($tab.Bottom+3*$scale))|ForEach-Object {$image.GetPixel($x,[int]($_-$origin[1]))}
            $difference=[Math]::Max([Math]::Max([Math]::Abs($shade.R-$body.R),[Math]::Abs($shade.G-$body.G)),[Math]::Abs($shade.B-$body.B))
            $line="$Name $($group.active): tab $($shade.R),$($shade.G),$($shade.B) panel $($body.R),$($body.G),$($body.B)"
            $tabShades.Add($line);if($difference -gt 2){$tabMismatches.Add($line)}
        }
    }finally{$image.Dispose()}
}
function Tab-Feedback([string]$Name){
    $group=@((Model).layout.groups|Where-Object {$_.tabs_visible -and $_.panels.Count -ge 2})[0]
    if(!$group){throw 'No adjacent panel tabs to check'}
    $restore=$group.active
    foreach($index in 0,1){
        $active=$group.panels[$index];$next=$group.panels[1-$index]
        if(@((Model).layout.groups|Where-Object {$_.id -eq $group.id})[0].active -ne $active){Invoke ('panel-tab-'+$active)}
        Wait-Until {@((Model).layout.groups|Where-Object {$_.id -eq $group.id})[0].active -eq $active} 'Tab feedback setup did not select its tab'
        $park=Center (Control 'drawing-canvas');[CapyRowPointer]::Hover($park.x,$park.y)
        $tab=Control ('panel-tab-'+$next) -Arranged;$bounds=$tab.Current.BoundingRectangle
        $origin=[int[]]@(0,0);$null=[Capy.GlassWindow]::ClientToScreen($drawingWindow,$origin)
        $retained=$tab.GetRuntimeId() -join ':'
        $selectedBounds=(Control ('panel-tab-'+$active) -Arranged).Current.BoundingRectangle
        $foot=@(foreach($x in @(($selectedBounds.Left+3*$scale),($selectedBounds.Right-3*$scale))){
            foreach($y in @(($selectedBounds.Bottom-2*$scale),($selectedBounds.Bottom+2*$scale))){@{x=[int]($x-$origin[0]);y=[int]($y-$origin[1])}}
        })
        $interior=@(foreach($fraction in .2,.5,.8){@{x=[int]($bounds.Left+$bounds.Width*$fraction-$origin[0]);y=[int]($bounds.Top+4*$scale-$origin[1])}})
        $expectedFoot=$null;$expectedInterior=$null
        foreach($state in 'rest','hover','press','release'){
            if($state -eq 'hover'){$at=Center $tab;[CapyRowPointer]::Hover($at.x,$at.y)}
            if($state -eq 'press'){
                [CapyRowPointer]::Down('mouse',$at.x,$at.y)
                Wait-Until {@((Model).layout.groups|Where-Object {$_.id -eq $group.id})[0].active -eq $next} 'Primary down did not select the inactive tab before release'
            }
            if($state -eq 'release'){[CapyRowPointer]::Up()}
            Start-Sleep -Milliseconds 200
            $capture="$Name-feedback-$index-$state";Capture $capture -Composed
            $image=[System.Drawing.Bitmap]::new((Join-Path $run ($capture+'.png')))
            try{
                $pixels=(@($foot|ForEach-Object {$image.GetPixel($_.x,$_.y).ToArgb()}) -join ',')
                $fill=(@($interior|ForEach-Object {$image.GetPixel($_.x,$_.y).ToArgb()}) -join ',')
            }finally{$image.Dispose()}
            if($state -eq 'rest'){$expectedFoot=$pixels;$expectedInterior=$fill}
            if($state -eq 'hover'){
                if($pixels -ne $expectedFoot){throw "$capture changed the active tab's foot"}
                if($fill -eq $expectedInterior){throw "$capture has no visible inactive-tab hover"}
            }
            if($state -in 'press','release'){
                if(((Control ('panel-tab-'+$next) -Arranged).GetRuntimeId() -join ':') -ne $retained){throw "$capture replaced the native tab button"}
                if(@((Model).layout.groups|Where-Object {$_.id -eq $group.id})[0].active -ne $next){throw "$capture lost the selected tab"}
                if($null -ne (Model).state.customization.expanded){throw "$capture opened panel configuration"}
                Tab-Shades $capture
            }
        }
    }
    if(@((Model).layout.groups|Where-Object {$_.id -eq $group.id})[0].active -ne $restore){Invoke ('panel-tab-'+$restore)}
    Wait-Until {@((Model).layout.groups|Where-Object {$_.id -eq $group.id})[0].active -eq $restore} 'Tab feedback did not restore selection'
}
function Four-TabGroup {
    $group=@((Model).layout.groups|Where-Object {$_.panels -contains 'brushes'})[0]
    foreach($panel in 'navigator','proof'){
        if($group.panels -contains $panel){continue}
        $at=Center (Control ('group-grip-'+$group.id) -Arranged);[CapyRowPointer]::RightClick($at.x,$at.y)
        (Control 'Add built-in panel' -Name).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
        $title=@((Model).panels|Where-Object id -eq $panel)[0].configuration_title -replace '^Configure ',''
        (Control $title -Name -Type $ControlType::MenuItem).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
        Wait-Until {@((Model).layout.groups|Where-Object {$_.id -eq $group.id})[0].panels -contains $panel} "Four-tab setup did not add $panel"
    }
    @((Model).layout.groups|Where-Object {$_.id -eq $group.id})[0]
}
function Tab-Style($Group,[string]$Name){
    $at=Center (Control ('group-grip-'+$Group) -Arranged);[CapyRowPointer]::RightClick($at.x,$at.y)
    (Control $Name -Name -Type $ControlType::MenuItem).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {!(Find $Name -Name -Type $ControlType::MenuItem)} 'Tab style menu did not close'
}
function Tab-Contains([double]$X,[double]$Y,$Bounds){
    $x=$X-$Bounds.Left;$y=$Y-$Bounds.Top;$w=$Bounds.Width;$h=$Bounds.Height
    if($y -ge $h){return $true}
    if($y -lt 0 -or $x -lt -6 -or $x -gt $w+6){return $false}
    if($x -lt 0 -or $x -gt $w){
        if($y -lt $h-6){return $false}
        $dx=if($x -lt 0){($x+6)/6}else{($x-$w-6)/6}
        return [Math]::Pow($dx,4)+[Math]::Pow(($y-$h+6)/6,4) -ge 1
    }
    $r=[Math]::Min(18,[Math]::Min($w/2,$h-6))
    if($y -lt $r -and ($x -lt $r -or $x -gt $w-$r)){
        $dx=if($x -lt $r){($x-$r)/$r}else{($x-$w+$r)/$r}
        return [Math]::Pow($dx,4)+[Math]::Pow(($y-$r)/$r,4) -le 1
    }
    $true
}
function Contour-Pixels([Drawing.Bitmap]$Image,$Logical,$Group,[double]$Scale,[Drawing.Color]$Panel,[Drawing.Color]$Strip,$Reference=$null){
    $geometry=@($Logical.Left,$Logical.Top,$Logical.Width,$Logical.Height,$Group.x,$Group.y,$Group.width,$Scale)
    if($Reference){
        if($Reference.geometry.Count -ne $geometry.Count -or @(0..($geometry.Count-1)|Where-Object {[Math]::Abs($geometry[$_]-$Reference.geometry[$_]) -gt .001}).Count){throw 'Off/High changed the contour geometry'}
    }
    $channel='R';foreach($candidate in 'G','B'){if([Math]::Abs($Panel.$candidate-$Strip.$candidate) -gt [Math]::Abs($Panel.$channel-$Strip.$channel)){$channel=$candidate}}
    $difference=$Panel.$channel-$Strip.$channel
    if([Math]::Abs($difference) -lt 4){throw 'Contour role endpoints are not distinguishable'}
    $tested=0;$failures=[Collections.Generic.List[object]]::new();$pixels=@{};$mixed=@{upper=0;foot=0};$anchors=[Collections.Generic.List[object]]::new()
    for($py=[int][Math]::Floor($logical.Top*$scale);$py -lt ($logical.Top+$logical.Height)*$scale;$py++){
        for($px=[int][Math]::Floor(($logical.Left-7)*$scale);$px -lt ($logical.Left+$logical.Width+7)*$scale;$px++){
            $x=($px+.5)/$scale;$y=($py+.5)/$scale
            if($x -lt $Group.x+19 -or $x -gt $Group.x+$Group.width-19){continue}
            $inside=Tab-Contains $x $y $logical
            $near=@((Tab-Contains ($x-.8) $y $logical),(Tab-Contains ($x+.8) $y $logical),(Tab-Contains $x ($y-.8) $logical),(Tab-Contains $x ($y+.8) $logical))
            if(@($near|Where-Object {$_ -ne $inside}).Count -eq 0){continue}
            $pixel=$Image.GetPixel($px,$py);$tested++;$key="$px,$py"
            $native=($pixel.$channel-$Strip.$channel)/[double]$difference
            $pixels[$key]=[Math]::Max(0.,[Math]::Min(1.,$native))
            if($native -gt .1 -and $native -lt .9){if($y -lt $Logical.Top+18){$mixed.upper++}elseif($y -gt $Logical.Top+$Logical.Height-6){$mixed.foot++}}
            if($Reference -and !$Reference.pixels.ContainsKey($key)){throw 'High contour has no validated opaque reference pixel'}
            foreach($component in 'R','G','B'){
                $expected=if($Reference){$coverage=$Reference.pixels[$key];$Strip.$component*(1-$coverage)+$Panel.$component*$coverage}else{
                    [Math]::Max([Math]::Min($Panel.$component,$Strip.$component),[Math]::Min([Math]::Max($Panel.$component,$Strip.$component),$pixel.$component))
                }
                if([Math]::Abs($pixel.$component-$expected) -gt 3){
                    $failures.Add(@{kind=$(if($Reference){'opacity_coverage'}else{'opaque_envelope'});x=$px;y=$py;channel=$component;actual=$pixel.$component;expected=$expected;native_coverage=$native});break
                }
            }
        }
    }
    $anchor={param([string]$Kind,[double]$X,[double]$Y,[bool]$Inside)
        if($Kind -ne 'selected_face' -and ($X -lt $Group.x+19 -or $X -gt $Group.x+$Group.width-19)){return}
        if((Tab-Contains $X $Y $Logical) -ne $Inside){throw "Invalid independent $Kind geometry anchor"}
        $px=[int][Math]::Floor($X*$Scale);$py=[int][Math]::Floor($Y*$Scale);$pixel=$Image.GetPixel($px,$py);$color=if($Inside){$Panel}else{$Strip}
        $anchors.Add(@{kind=$Kind;x=$px;y=$py;actual=@($pixel.R,$pixel.G,$pixel.B);expected=@($color.R,$color.G,$color.B)})
        foreach($component in 'R','G','B'){if([Math]::Abs($pixel.$component-$color.$component) -gt 1){$failures.Add(@{kind=$Kind;x=$px;y=$py;channel=$component;actual=$pixel.$component;expected=$color.$component});break}}
    }
    & $anchor 'selected_face' ($Logical.Left+$Logical.Width/2) ($Logical.Top+5) $true
    foreach($side in -1,1){
        $edge=if($side -lt 0){$Logical.Left}else{$Logical.Left+$Logical.Width};$bottom=$Logical.Top+$Logical.Height
        & $anchor 'excluded_top_corner' ($edge-$side) ($Logical.Top+1) $false
        & $anchor 'foot_strip' ($edge+$side*4) ($bottom-4) $false
        & $anchor 'foot_face' ($edge-$side*2) ($bottom-2) $true
        $px=if($side -lt 0){[Math]::Floor($edge*$Scale)-1}else{[Math]::Ceiling($edge*$Scale)}
        $py=[Math]::Ceiling($bottom*$Scale)-1;$x=($px+.5)/$Scale;$y=($py+.5)/$Scale
        $clear=@(0..7|Where-Object {$angle=$_*[Math]::PI/4;!(Tab-Contains ($x+.8*[Math]::Cos($angle)) ($y+.8*[Math]::Sin($angle)) $Logical)}).Count -eq 0
        if($clear){& $anchor 'external_foot' $x $y $true}
    }
    @{geometry=$geometry;panel=@($Panel.R,$Panel.G,$Panel.B);strip=@($Strip.R,$Strip.G,$Strip.B);samples=$tested;pixels=$pixels;mixed=$mixed;anchors=$anchors.ToArray();failures=$failures.ToArray()}
}
function Assert-Contour($Result,[string]$Name,[double]$Scale){
    if($Result.samples -lt 50){throw "$Name did not cover enough absolute contour pixels ($($Result.samples))"}
    if($Result.failures.Count){throw "$Name has $($Result.failures.Count) incorrect absolute panel/strip contour pixels: $($Result.failures[0]|ConvertTo-Json -Compress)"}
    if(!$Result.mixed.upper -or !$Result.mixed.foot){throw "$Name has no genuine antialias coverage at both upper and foot contours"}
    if(@($Result.anchors|Where-Object kind -eq 'external_foot').Count -eq 0 -and $Scale -ge 1.5){throw "$Name has no independent external-foot anchor"}
}
function Tab-Coverage([Drawing.Bitmap]$Image,$Bounds,$Group,[string]$Name){
    $glass=(Model).state.palette.glass;$colors=@{}
    foreach($role in 'panel','strip'){
        $rgba=$glass.$role;$channels=@(0..2|ForEach-Object {[int][Math]::Round(255*$rgba[$_]*$rgba[3]+$contourBackdrop[$_]*(1-$rgba[3]))})
        $colors[$role]=[Drawing.Color]::FromArgb($channels[0],$channels[1],$channels[2])
    }
    $origin=[int[]]@(0,0);$null=[Capy.GlassWindow]::ClientToScreen($drawingWindow,$origin)
    $logical=@{Left=($Bounds.Left-$origin[0])/$scale;Top=($Bounds.Top-$origin[1])/$scale;Width=$Bounds.Width/$scale;Height=$Bounds.Height/$scale}
    $key=$Name -replace '^(opaque|translucent)-','';$reference=if($Name.StartsWith('translucent-')){$script:CapyOpaqueContours[$key]}else{$null}
    if($Name.StartsWith('translucent-') -and !$reference){throw 'High contour needs a validated opaque reference'}
    $layout=Get-Content (Join-Path $run ($Name+'-layout.json')) -Raw|ConvertFrom-Json
    $placement=@($layout.presentation.groups|Where-Object id -eq $Group.id)[0].bounds
    if(!$placement){throw 'Contour capture has no native group placement'}
    $result=Contour-Pixels $Image $logical $placement $scale $colors.panel $colors.strip $reference
    $result|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($Name+'-coverage.json'))
    Assert-Contour $result $Name $Scale
    if(!$reference){$script:CapyOpaqueContours[$key]=$result}
}
function Hover-Contours([string]$Name){
    if($Name -eq 'opaque'){$script:CapyOpaqueContours=@{}}
    $group=Four-TabGroup;$panels=@($group.panels)
    if($panels.Count -ne 4){throw 'Hover contours need exactly four tabs'}
    $origin=[int[]]@(0,0);$null=[Capy.GlassWindow]::ClientToScreen($drawingWindow,$origin)
    $park=Center (Control 'drawing-canvas');$observations=[Collections.Generic.List[object]]::new();$selectedArgb=$null;$inactiveArgb=$null
    $hoverText=[Drawing.ColorTranslator]::FromHtml((Model).state.palette.text)
    $tooltip=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$ControlType::ToolTip),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
    Tab-Style $group.id 'Icons only'
    foreach($active in 0..3){
        Invoke ('panel-tab-'+$panels[$active])
        Wait-Until {@((Model).layout.groups|Where-Object {$_.id -eq $group.id})[0].active -eq $panels[$active]} 'Hover setup did not select its tab'
        $selected=(Control ('panel-tab-'+$panels[$active]) -Arranged).Current.BoundingRectangle
        (Control 'settings-button' -Arranged).SetFocus()
        [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
        [CapyRowPointer]::Hover($park.x,$park.y);Start-Sleep -Milliseconds 200
        $header=[Windows.Rect]::new($selected.Left-7*$scale,$selected.Top,$selected.Width+14*$scale,$selected.Height)
        Wait-Until {!@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$tooltip)|Where-Object {
            $b=$_.Current.BoundingRectangle
            !$_.Current.IsOffscreen -and $b.Width -gt 0 -and $b.Height -gt 0 -and $b.Right+24*$scale -gt $header.Left -and $b.Left-24*$scale -lt $header.Right -and $b.Bottom+24*$scale -gt $header.Top -and $b.Top-24*$scale -lt $header.Bottom
        }).Count} 'Tab tooltip did not leave the contour capture area'
        Wait-Until {
            Capture ("$Name-$active-rest") -Composed
            $sample=[Drawing.Bitmap]::new((Join-Path $run ("$Name-$active-rest.png")))
            try{
                $color=$sample.GetPixel([int](($selected.Left+$selected.Right)/2-$origin[0]),[int]($selected.Bottom-3*$scale-$origin[1])).ToArgb()
                $inactive=@(foreach($index in 0..3|Where-Object {$_ -ne $active}){
                    $b=(Control ('panel-tab-'+$panels[$index]) -Arranged).Current.BoundingRectangle
                    $sample.GetPixel([int](($b.Left+$b.Right)/2-$origin[0]),[int]($b.Bottom-8*$scale-$origin[1])).ToArgb()
                })
            }finally{$sample.Dispose()}
            if($null -eq $selectedArgb){if(@($inactive|Where-Object {$_ -ne $inactive[0]}).Count -ne 0 -or $color -eq $inactive[0]){return $false};$script:CapySelectedTabArgb=$color;$script:CapyInactiveTabArgb=$inactive[0];return $true}
            $color -eq $selectedArgb -and @($inactive|Where-Object {$_ -ne $inactiveArgb}).Count -eq 0
        } 'Selected tab surface was not presented'
        if($null -eq $selectedArgb){$selectedArgb=$script:CapySelectedTabArgb;$inactiveArgb=$script:CapyInactiveTabArgb}
        $rest=[Drawing.Bitmap]::new((Join-Path $run ("$Name-$active-rest.png")))
        try{
            $selected=(Control ('panel-tab-'+$panels[$active]) -Arranged).Current.BoundingRectangle
            Tab-Coverage $rest $selected $group ("$Name-$active-rest")
            foreach($hovered in 0..3){
                $tab=Control ('panel-tab-'+$panels[$hovered]) -Arranged;$bounds=$tab.Current.BoundingRectangle;$at=Center $tab
                [CapyRowPointer]::Hover($at.x,$at.y);Start-Sleep -Milliseconds 200;$captureName="$Name-$active-hover-$hovered"
                Wait-Until {
                    Capture $captureName -Composed
                    if($hovered -eq $active){return $true}
                    $sample=[Drawing.Bitmap]::new((Join-Path $run ($captureName+'.png')))
                    try{
                        $px=[int](($bounds.Left+$bounds.Right)/2-$origin[0]);$py=[int]($bounds.Bottom-8*$scale-$origin[1])
                        $before=$rest.GetPixel($px,$py);$after=$sample.GetPixel($px,$py)
                        $contrast=0;$change=0;$shadeError=0
                        foreach($component in 'R','G','B'){
                            $expected=[int][Math]::Round(($before.$component*235+$hoverText.$component*20)/255.)
                            $contrast=[Math]::Max($contrast,[Math]::Abs($before.$component-$expected))
                            $change=[Math]::Max($change,[Math]::Abs($before.$component-$after.$component))
                            $shadeError=[Math]::Max($shadeError,[Math]::Abs($after.$component-$expected))
                        }
                        $change -ge [Math]::Max(1,$contrast-1) -and $shadeError -le 1
                    }finally{$sample.Dispose()}
                } "$captureName has no visible hover interior"
                @{bounds=@($bounds.Left,$bounds.Top,$bounds.Right,$bounds.Bottom);injected=@($at.x,$at.y);cursor=[CapyRowPointer]::Cursor();origin=$origin;selected=@($selected.Left,$selected.Top,$selected.Right,$selected.Bottom)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($captureName+'-pointer.json'))
                $image=[Drawing.Bitmap]::new((Join-Path $run ($captureName+'.png')))
                try{
                    foreach($x in @(($selected.Left+3*$scale),($selected.Right-3*$scale))){
                        $px=[int]($x-$origin[0]);$py=[int]($selected.Bottom-2*$scale-$origin[1])
                        if($image.GetPixel($px,$py).ToArgb() -ne $rest.GetPixel($px,$py).ToArgb()){throw "$captureName changed the selected foot"}
                    }
                    foreach($x in @(([Math]::Floor($selected.Left)-1),([Math]::Ceiling($selected.Right)))){
                        $px=[int]($x-$origin[0]);$py=[int]([Math]::Floor($selected.Bottom)-1-$origin[1])
                        if($image.GetPixel($px,$py).ToArgb() -ne $rest.GetPixel($px,$py).ToArgb()){throw "$captureName changed the selected outer foot"}
                    }
                    if($hovered -eq $active){continue}
                    foreach($side in 0,1){
                        $neighbor=if($side -eq 0){$hovered-1}else{$hovered+1}
                        $kind=if($neighbor -lt 0 -or $neighbor -gt 3){'square'}elseif($neighbor -eq $active){'inset'}else{'reverse'}
                        $x=if($kind -eq 'reverse'){if($side -eq 0){[Math]::Floor($bounds.Left)-1}else{[Math]::Ceiling($bounds.Right)}}else{if($side -eq 0){[Math]::Ceiling($bounds.Left)}else{[Math]::Floor($bounds.Right)-1}}
                        $px=[int]($x-$origin[0]);$py=[int]([Math]::Floor($bounds.Bottom)-1-$origin[1])
                        $changed=$image.GetPixel($px,$py).ToArgb() -ne $rest.GetPixel($px,$py).ToArgb()
                        $observations.Add(@{active=$active;hovered=$hovered;side=$side;kind=$kind;changed=$changed;x=$px;y=$py})
                        $observations|ConvertTo-Json|Set-Content (Join-Path $run ($Name+'-hover-contours.json'))
                        if($changed -ne ($kind -ne 'inset')){throw "$captureName $side $kind lower contour has incorrect pixels"}
                    }
                }finally{$image.Dispose()}
            }
        }finally{$rest.Dispose()}
    }
    Write-Output 'Four-tab lower contours, selected feet and square ends passed'
}
function Zoom([double]$Minimum){
    (Control 'drawing-canvas').SetFocus()
    Wait-Until {(Control 'drawing-canvas').Current.HasKeyboardFocus} 'Transparency canvas did not own keyboard focus'
    for($i=0;$i -lt 32 -and (Model).state.camera.zoom -le $Minimum;$i++){
        $before=(Model).state.camera.zoom
        [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0xBB)
        Wait-Until {(Model).state.camera.zoom -gt $before} 'Transparency zoom input did not change the camera'
    }
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Transparency review $($review.Id): $run"
    Wait-Until {$review.Refresh();$native=Owned-DrawingWindow $review;if(!$native){return $false};$script:drawingWindow=$native.Handle;$script:root=$native.Root;(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Transparency review did not start' 90
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Start-Sleep -Milliseconds 600
    $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Switch-Workspace 'Paint' 'builtin:workspace:illustrator'
    if((Model).state.palette.glass.transparency -ne 'low'){throw 'Low is not the default transparency'}
    Invoke 'panel-tab-stats'
    Wait-Until {@((Model).layout.groups|Where-Object active -eq 'stats').Count -eq 1} 'Diagnostics did not become the active tab'
    $settings=Control 'settings-button' -Arranged;$settings.SetFocus()
    Wait-Until {$settings.Current.HasKeyboardFocus} 'Settings button did not take keyboard focus before tab sampling'
    Wait-Until {(Model).windows_glass.regions -ge 4} 'Paint did not publish its glass surfaces'
    $group=@((Model).layout.groups|Where-Object {$_.panels -contains 'layers'})[0]
    $scale=[Capy.GlassWindow]::GetDpiForWindow($drawingWindow)/96.;$client=[int[]]@(0,0);$null=[Capy.GlassWindow]::ClientToScreen($drawingWindow,$client)
    $bounds=$group.bounds
    $point=@{x=[int]($client[0]+($bounds.x+$bounds.width-24)*$scale);y=[int]($client[1]+($bounds.y+$bounds.height-72)*$scale)}
    $panel=[Convert]::ToInt32((Model).state.palette.panel.Substring(3,2),16)
    $tabShades=[Collections.Generic.List[string]]::new();$tabMismatches=[Collections.Generic.List[string]]::new()
    if($Journey -ne 'full'){
        Preference 0 ($Journey -replace '^hover-','')
        if($Journey -eq 'hover-dark'){
            Invoke 'color-swap'
            Wait-Until {@((Model).state.colors.foreground.rgba|Where-Object {$_ -lt .999}).Count -eq 0} 'Contour backdrop did not select white'
        }else{
            Invoke 'color-edit'
            Wait-Until {Find 'edit-color-apply'} 'Contour backdrop did not open Edit Color'
            Invoke 'edit-color-hex'
            $entry=Control 'edit-color-hex-entry';$entry.SetFocus()
            $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#000000')
            [CapyRowPointer]::Key([uint32]$review.Id,0x0d)
            Invoke 'edit-color-apply'
            Wait-Until {!(Find 'edit-color-apply') -and @((Model).state.colors.foreground.rgba|Select-Object -First 3|Where-Object {$_ -gt .001}).Count -eq 0} 'Contour backdrop did not select black'
        }
        $contourBackdrop=@((Model).state.colors.foreground.rgba|Select-Object -First 3|ForEach-Object {$_*255})
        $layers=@((Model).state.layers).Count
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Filter'
        (Control 'Fill' -Name -Type $ControlType::MenuItem).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
        Invoke 'Solid Color' -Name
        Wait-Until {@((Model).state.layers).Count -eq $layers+1} 'Contour backdrop did not add the solid fill'
        foreach($paper in @((Model).state.layers|Where-Object {$_.paper -and $_.visible})){
            Invoke ('layer-'+$paper.id+'-visibility')
            Wait-Until {!@((Model).state.layers|Where-Object id -eq $paper.id)[0].visible} 'Contour backdrop did not hide the paper relief'
        }
        Zoom 2
        Hover-Contours 'opaque'
        Preference 3
        Hover-Contours 'translucent'
        [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
        if(!$review.WaitForExit(8000) -or $review.ExitCode -ne 0){throw 'Hover contour review did not close cleanly'}
        return
    }
    Fit-Canvas
    $surround=@{};$overPaper=@{}
    foreach($level in 0,3,2,1){
        Preference $level
        $surround[$level]=(Sample ('fit-'+$level) @($point))[0];Tab-Shades ('fit-'+$level);Tab-Feedback ('fit-'+$level)
        if([Math]::Abs($surround[$level]-$panel) -gt 1){throw "Level $level panel over the surround is $($surround[$level]), expected $panel"}
    }
    Zoom 2
    Wait-Until {$z=(Model).state.camera.zoom;$z -gt 2} 'The canvas did not zoom under the panels'
    foreach($level in 0,1,2,3){
        Preference $level
        $overPaper[$level]=(Sample ('paper-'+$level) @($point))[0];Tab-Shades ('paper-'+$level)
    }
    Write-Output ("Layers panel green over the surround: "+(($surround.GetEnumerator()|Sort-Object Key|ForEach-Object {$_.Value}) -join ',')+"; over paper: "+(0..3|ForEach-Object {$overPaper[$_]}) -join ',')
    if($overPaper[0] -ne $panel){throw "Off panel over paper is $($overPaper[0]), expected $panel"}
    if(!($overPaper[1] -gt $overPaper[0] -and $overPaper[2] -gt $overPaper[1] -and $overPaper[3] -gt $overPaper[2])){throw 'Higher transparency did not show more paper'}
    Preference 0 'light'
    Fit-Canvas
    foreach($level in 0,1,2,3){
        if($level){Preference $level}
        $null=Sample ('light-fit-'+$level) @();Tab-Shades ('light-fit-'+$level);Tab-Feedback ('light-fit-'+$level)
    }
    Zoom 2
    Wait-Until {$z=(Model).state.camera.zoom;$z -gt 2} 'The light canvas did not zoom under the panels'
    foreach($level in 0,1,2,3){
        Preference $level
        $null=Sample ('light-paper-'+$level) @();Tab-Shades ('light-paper-'+$level)
    }
    $tabShades|Write-Output
    if(!$tabShades.Count){throw 'Paint has no tabbed group to compare'}
    if($tabMismatches.Count){throw "Active tabs differ from their panels:`n"+($tabMismatches -join "`n")}
    Preference 3 'dark'
    Switch-Workspace 'Sketch' 'builtin:workspace:painter'
    $color=$null;foreach($zone in @((Model).header.model.zones)){foreach($entry in @($zone)){if($entry.item.control.kind -eq 'color'){$color=$entry.id}}}
    $settled=@{count=-1;since=[Diagnostics.Stopwatch]::StartNew()}
    Wait-Until {$count=(Model).windows_glass.regions;if($count -ne $settled.count){$settled.count=$count;$settled.since.Restart()};$settled.since.ElapsedMilliseconds -gt 1000} 'Sketch glass did not settle' 15
    $closed=$settled.count
    Tap (Center (Control ('header-item-'+$color)))
    Wait-Until {(Find 'tool-drawer') -and (Model).windows_glass.regions -ge $closed+2} 'The Sketch drawer and its connector did not publish glass'
    Capture 'sketch-drawer'
    [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if(!$review.WaitForExit(8000)){throw 'Transparency review did not close'}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Transparency review wrote to stderr'}
    [pscustomobject]@{default_low='passed';circles='passed';invisible_over_surround='passed';paper_by_level='passed';active_tab_matches_panel='passed';tab_feedback_preserves_join='passed';drawer_connector='passed';
        scope='Synthetic UIA and captured pixels; physical display and performance acceptance are separate'}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
    try{Capture 'failure'}catch{}
    try{
        $focused=[System.Windows.Automation.AutomationElement]::FocusedElement
        $tips=[System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$ControlType::ToolTip),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
        @{cursor=[CapyRowPointer]::Cursor();focus=$(if($focused.Current.ProcessId -eq $review.Id){@{id=$focused.Current.AutomationId;name=$focused.Current.Name;bounds=$focused.Current.BoundingRectangle}});
            tooltips=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$tips)|ForEach-Object {@{name=$_.Current.Name;bounds=$_.Current.BoundingRectangle;offscreen=$_.Current.IsOffscreen}})}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'failure-pointer.json')
    }catch{}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyCanvasTouch]::Dispose();[CapyRowPointer]::Dispose()
    if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force -ErrorAction SilentlyContinue}
    Exit-CapyEnvironment
}
