param([Parameter(Mandatory)][object]$Root,[Parameter(Mandatory)][string]$Name,[string]$Caption,[switch]$Inspect,[switch]$PassThru)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$id=$Name.ToLowerInvariant().Replace('application-menu-','')
if($id -notin @('file','edit','image','layer','select','filter','view','window')){throw "Unknown application menu: $Name"}
$menuNodes=@()
$menuCache=[System.Windows.Automation.CacheRequest]::new()
$menuCache.TreeScope=[System.Windows.Automation.TreeScope]::Element
$menuCache.TreeFilter=[System.Windows.Automation.Automation]::RawViewCondition
foreach($property in @(
    [System.Windows.Automation.AutomationElement]::AutomationIdProperty,
    [System.Windows.Automation.AutomationElement]::NameProperty,
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.AutomationElement]::ClassNameProperty,
    [System.Windows.Automation.AutomationElement]::IsOffscreenProperty,
    [System.Windows.Automation.AutomationElement]::IsEnabledProperty
)){$menuCache.Add($property)}
function Read-MenuNodes([switch]$Popup) {
    $active=$menuCache.Activate()
    try {
        if(!$Popup){return $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)}
        foreach($popupHost in $Root.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.Condition]::TrueCondition)){
            $current=$popupHost.Cached
            if($current.Name -ceq 'PopupHost' -and $current.ClassName -ceq 'Microsoft.UI.Content.PopupWindowSiteBridge' -and $current.ControlType -eq [System.Windows.Automation.ControlType]::Pane){
                $popupHost.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
            }
        }
    }
    catch {
        if($_.Exception.GetBaseException() -isnot [System.Windows.Automation.ElementNotAvailableException]){throw}
    }
    finally {$active.Dispose()}
}

function Visible-Control([string]$Id,$Type,[switch]$ByName,[switch]$Enabled=(!$Inspect)){
    foreach($entry in $menuNodes){
        try {
            $current=$entry.Cached
            $value=if($ByName){$current.Name}else{$current.AutomationId}
            if($value -ceq $Id -and !$current.IsOffscreen -and (!$Enabled -or $current.IsEnabled) -and (!$Type -or $current.ControlType -eq $Type)){return $entry}
        } catch {
            if($_.Exception.GetBaseException() -isnot [System.Windows.Automation.ElementNotAvailableException]){throw}
        }
    }
}
function Menu-Button {
    foreach($entry in @('application-menus','application-primary-menu','header-recovery-menu')){
        $button=Visible-Control $entry ([System.Windows.Automation.ControlType]::Button)
        if($button){return $button}
    }
}
function Submenu {
    $entry=Visible-Control ('application-menu-'+$id) ([System.Windows.Automation.ControlType]::MenuItem)
    if(!$entry){$entry=Visible-Control ('menu-'+$id) ([System.Windows.Automation.ControlType]::MenuItem)}
    if(!$entry -and $Caption){$entry=Visible-Control $Caption ([System.Windows.Automation.ControlType]::MenuItem) -ByName}
    $entry
}
function Invoke-MenuButton($Button) {
    try{$Button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
    catch [System.Windows.Automation.ElementNotAvailableException]{
        do{
            foreach($entry in @(Read-MenuNodes -Popup)){
                $current=$entry.Cached
                if(!$current.IsOffscreen -and $current.ControlType -eq [System.Windows.Automation.ControlType]::MenuItem){return}
            }
            if($watch.Elapsed.TotalSeconds -ge $menuDeadline){throw}
            Start-Sleep -Milliseconds 50
        }while($true)
    }
}
$menuDeadline=8*$(if($env:CAPY_WAIT_SCALE){[double]$env:CAPY_WAIT_SCALE}else{1})
$watch=[Diagnostics.Stopwatch]::StartNew()
do{
    $menuNodes=@(Read-MenuNodes)
    $button=Visible-Control ("application-menu-"+$id) ([System.Windows.Automation.ControlType]::Button)
    $overflow=if(!$button){Menu-Button}
    $submenu=if(!$button -and !$Inspect){Submenu}
    if($button -or $overflow -or $submenu){break}
    Start-Sleep -Milliseconds 50
}while($watch.Elapsed.TotalSeconds -lt $menuDeadline)
if($Inspect){
    if(!$button){$button=$overflow}
    if(!$button){throw 'The application menu has no visible entry point'}
    return $button
}
if($button){
    Invoke-MenuButton $button
    if($PassThru){$button}
    return
}
if(!$submenu){
    if(!$overflow){throw 'The compact application menu is unavailable'}
    $overflow.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{
        $menuNodes=@(Read-MenuNodes -Popup)
        $submenu=Submenu
        if($submenu){break}
        Start-Sleep -Milliseconds 50
    }while($watch.Elapsed.TotalSeconds -lt 5)
}
if(!$submenu -or !$submenu.Current.IsEnabled){throw "Application menu is unavailable: $Name"}
$expand=$submenu.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
$expand.Expand()
$watch=[Diagnostics.Stopwatch]::StartNew()
while($expand.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::Expanded){
    if($watch.Elapsed.TotalSeconds -ge 5){throw "Application menu did not expand: $Name"}
    Start-Sleep -Milliseconds 50
}
if($PassThru){$submenu}
