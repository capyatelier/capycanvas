param([Parameter(Mandatory)][object]$Root,[Parameter(Mandatory)][string]$Name,[switch]$Inspect,[switch]$PassThru)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$id=$Name.ToLowerInvariant().Replace('application-menu-','')
if($id -notin @('file','edit','layer','select','filter','view','window','help')){throw "Unknown application menu: $Name"}
function Visible-Control([string]$Id,$Type,[switch]$ByName){
    $property=if($ByName){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Id)
    foreach($entry in $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){
        if(!$entry.Current.IsOffscreen -and (!$Type -or $entry.Current.ControlType -eq $Type)){return $entry}
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
    if(!$entry){$entry=Visible-Control ([Globalization.CultureInfo]::InvariantCulture.TextInfo.ToTitleCase($id)) ([System.Windows.Automation.ControlType]::MenuItem) -ByName}
    $entry
}
$button=Visible-Control ("application-menu-"+$id) ([System.Windows.Automation.ControlType]::Button)
if($Inspect){
    if(!$button){$button=Menu-Button}
    if(!$button){throw 'The application menu has no visible entry point'}
    return $button
}
if($button){
    $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    if($PassThru){$button}
    return
}
$submenu=Submenu
if(!$submenu){
    $overflow=Menu-Button
    if(!$overflow){throw 'The compact application menu is unavailable'}
    $overflow.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{
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
