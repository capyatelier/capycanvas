param([Parameter(Mandatory)][string]$Executable,[ValidateSet('light','dark')][string]$Theme='light')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/button-feedback/'+$Theme+'-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$owned=$null;$window=[IntPtr]::Zero;$previousTheme=$env:CAPY_FEEDBACK_THEME
$records=[Collections.Generic.List[object]]::new();$clicks=0
function Element([string]$Id){
    $windows=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$owned.Id))
    foreach($candidate in $windows){$found=$candidate.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id));if($found){return $found}}
}
function Invoke-Button([string]$Id){(Element $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke();Start-Sleep -Milliseconds 250}
function Check-State($Control,[string]$Stage,[bool]$Contact,[int]$Alpha,[int]$Clicks){
    Start-Sleep -Milliseconds 100
    $text=$Control.Current.ItemStatus;$state=@{};foreach($part in $text.Split(';')){$pair=$part.Split('=');$state[$pair[0]]=$pair[1]}
    $records.Add(@{stage=$Stage;status=$text})
    if($state.contact-ne$Contact.ToString().ToLowerInvariant()-or[int]$state.alpha-ne$Alpha-or[int]$state.clicks-ne$Clicks){throw "$Stage expected contact=$Contact alpha=$Alpha clicks=$Clicks; actual $text"}
}
try{
    $env:CAPY_FEEDBACK_THEME=$Theme;$Executable=(Resolve-Path -LiteralPath $Executable).Path
    $owned=Start-Process -FilePath $Executable -WorkingDirectory (Split-Path $Executable) -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log');$null=$owned.Handle
    Wait-Until {foreach($handle in [CapyWindowApi]::Windows([uint32]$owned.Id)){$candidate=[System.Windows.Automation.AutomationElement]::FromHandle($handle);$probe=$candidate.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'feedback-root'));if($probe){$script:window=$handle;return $true}};$false} 'Feedback fixture did not start' 30
    [CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyWindowApi]::SetForegroundWindow($window)|Out-Null;[CapyRowPointer]::Initialize([uint32]$owned.Id)
    foreach($context in @('root','dialog','flyout')){
        if($context-eq'dialog'){Invoke-Button 'feedback-show-dialog'};if($context-eq'flyout'){Invoke-Button 'feedback-show-flyout'}
        $control=Element ('feedback-'+$context);if(!$control){throw "Missing $context button"}
        foreach($device in @('mouse','pen','touch')){
            $bounds=$control.Current.BoundingRectangle;$x=[int]($bounds.Left+$bounds.Width/2);$y=[int]($bounds.Top+$bounds.Height/2)
            $outsideX=[int]($bounds.Left-15);$outsideY=[int]($bounds.Top+10);$prefix=$context+'-'+$device
            [CapyRowPointer]::Hover($outsideX,$outsideY)
            [CapyRowPointer]::Down($device,$x,$y)
            try{Check-State $control ($prefix+'-held') $true 41 $clicks}finally{[CapyRowPointer]::Up()}
            $clicks++;$alpha=if($device-eq'mouse'){20}else{0}
            Check-State $control ($prefix+'-released') $false $alpha $clicks
            [CapyRowPointer]::Hover($outsideX,$outsideY);Check-State $control ($prefix+'-outside') $false 0 $clicks
            [CapyRowPointer]::Down($device,$x,$y)
            try{Start-Sleep -Milliseconds 100;[CapyRowPointer]::Move($outsideX,$outsideY)}finally{[CapyRowPointer]::Up()}
            Check-State $control ($prefix+'-outside-release') $false 0 $clicks
            if($device-ne'mouse'){
                [CapyRowPointer]::Down($device,$x,$y);Start-Sleep -Milliseconds 100;[CapyRowPointer]::Cancel()
                Check-State $control ($prefix+'-canceled') $false 0 $clicks
            }
        }
        if($context-eq'dialog'){Invoke-Button 'feedback-close-dialog'};if($context-eq'flyout'){Invoke-Button 'feedback-hide-flyout'}
    }
    [CapyWindowApi]::PostMessage($window,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
    if(!$owned.WaitForExit(8000)){throw 'Feedback fixture failed to close normally'}
    if($owned.ExitCode-ne0){throw "Feedback fixture exited $($owned.ExitCode)"}
    if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'Feedback fixture wrote stderr'}
    @{passed=$records.Count;clicks=$clicks;exit_code=$owned.ExitCode;executable=$Executable;sha256=(Get-FileHash -LiteralPath $Executable).Hash}|ConvertTo-Json|Set-Content (Join-Path $run 'result.json')
    Write-Output "Button feedback passed $($records.Count) state assertions ($Theme); artifacts: $run"
}finally{
    $records|ConvertTo-Json -Depth 4|Set-Content (Join-Path $run 'states.json')
    [CapyRowPointer]::Dispose();$env:CAPY_FEEDBACK_THEME=$previousTheme
    if($owned-and!$owned.HasExited){[CapyWindowApi]::PostMessage($window,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$owned.WaitForExit(8000)){'Forced cleanup after failed feedback check'|Set-Content (Join-Path $run 'cleanup-failure.txt');Stop-Process -Id $owned.Id}}
}
