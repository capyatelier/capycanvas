param([string]$Helper=(Join-Path $PSScriptRoot '../scripts/open-application-menu.ps1'))
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
function Menu-Root([string]$Mode){
    $result=[pscustomobject]@{reads=0;mode=$Mode}
    $result|Add-Member ScriptMethod FindAll {
        param($scope,$condition)
        if(![object]::ReferenceEquals([System.Windows.Automation.CacheRequest]::Current.TreeFilter,[System.Windows.Automation.Automation]::RawViewCondition)){throw 'Menu snapshot cache did not retain the raw search view'}
        if($condition -ne [System.Windows.Automation.Condition]::TrueCondition){return}
        $this.reads++
        if($this.mode -eq 'changing-route' -and $this.reads -gt 2){return}
        $type=if($this.mode -eq 'inspect' -and $this.reads -ge 3){[System.Windows.Automation.ControlType]::Button}else{[System.Windows.Automation.ControlType]::MenuItem}
        $item=[pscustomobject]@{Current=[pscustomobject]@{AutomationId='application-menu-file';Name='File';IsOffscreen=$false;IsEnabled=$true;ControlType=$type}}
        $item|Add-Member NoteProperty Cached $item.Current
        $item|Add-Member ScriptMethod GetCurrentPattern {
            param($pattern)
            $result=[pscustomobject]@{Current=[pscustomobject]@{ExpandCollapseState=[System.Windows.Automation.ExpandCollapseState]::Expanded}}
            $result|Add-Member ScriptMethod Expand {}
            $result
        }
        $item
    }
    $result
}
$failures=[Collections.Generic.List[string]]::new()
foreach($mode in @('inspect','expanded','changing-route')){
    try{
        $window=Menu-Root $mode
        if($mode -eq 'inspect'){
            $entry=& $Helper -Root $window -Name File -Inspect
            if($entry.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button){throw 'Inspection returned a submenu instead of the later button'}
        }else{
            $entry=& $Helper -Root $window -Name File -PassThru
            if($entry.Current.ControlType -ne [System.Windows.Automation.ControlType]::MenuItem){throw 'The existing submenu route was not retained'}
        }
        $expected=if($mode -eq 'inspect'){3}else{1}
        if($window.reads -ne $expected){throw 'Menu discovery did not retain one tree snapshot per attempt'}
        Write-Output ('Application menu '+$mode+': passed')
    }catch{
        $failures.Add($mode+': '+$_.Exception.Message)
        Write-Output ('Application menu '+$mode+': failed')
    }
}
if($failures.Count){throw ($failures -join [Environment]::NewLine)}

$popup=[pscustomobject]@{full_reads=0;child_reads=0;popup_reads=0;opened=$false;expanded=$false;failure=$null}
$overflow=[pscustomobject]@{Current=[pscustomobject]@{AutomationId='application-menus';Name='Menus';IsOffscreen=$false;IsEnabled=$true;ControlType=[System.Windows.Automation.ControlType]::Button};Root=$popup}
$overflow|Add-Member NoteProperty Cached $overflow.Current
$overflow|Add-Member ScriptMethod GetCurrentPattern {
    param($pattern)
    $invoke=[pscustomobject]@{Root=$this.Root}
    $invoke|Add-Member ScriptMethod Invoke {$this.Root.opened=$true}
    $invoke
}
$submenu=[pscustomobject]@{Current=[pscustomobject]@{AutomationId='menu-file';Name='File';IsOffscreen=$false;IsEnabled=$true;ControlType=[System.Windows.Automation.ControlType]::MenuItem};Root=$popup}
$submenu|Add-Member NoteProperty Cached $submenu.Current
$submenu|Add-Member ScriptMethod GetCurrentPattern {
    param($pattern)
    $expand=[pscustomobject]@{Root=$this.Root;Current=[pscustomobject]@{ExpandCollapseState=[System.Windows.Automation.ExpandCollapseState]::Expanded}}
    $expand|Add-Member ScriptMethod Expand {$this.Root.expanded=$true}
    $expand
}
$popup|Add-Member NoteProperty Overflow $overflow
$popup|Add-Member NoteProperty Submenu $submenu
$popupHost=[pscustomobject]@{Root=$popup;Cached=[pscustomobject]@{Name='PopupHost';ClassName='Microsoft.UI.Content.PopupWindowSiteBridge';ControlType=[System.Windows.Automation.ControlType]::Pane}}
$popupHost|Add-Member ScriptMethod FindAll {
    param($scope,$condition)
    if($scope -ne [System.Windows.Automation.TreeScope]::Descendants -or $condition -ne [System.Windows.Automation.Condition]::TrueCondition){throw 'Unexpected popup search scope'}
    if(![object]::ReferenceEquals([System.Windows.Automation.CacheRequest]::Current.TreeFilter,[System.Windows.Automation.Automation]::RawViewCondition)){throw 'Popup snapshot cache did not retain the raw search view'}
    $this.Root.popup_reads++
    if($this.Root.failure){throw $this.Root.failure}
    if($this.Root.popup_reads -eq 1){throw [System.Windows.Automation.ElementNotAvailableException]::new('Popup host expired')}
    $this.Root.Submenu
}
$popup|Add-Member NoteProperty PopupHost $popupHost
$popup|Add-Member ScriptMethod FindAll {
    param($scope,$condition)
    if(![object]::ReferenceEquals([System.Windows.Automation.CacheRequest]::Current.TreeFilter,[System.Windows.Automation.Automation]::RawViewCondition)){throw 'Popup snapshot cache did not retain the raw search view'}
    if($condition -ne [System.Windows.Automation.Condition]::TrueCondition){throw 'Unexpected menu search condition'}
    if($scope -eq [System.Windows.Automation.TreeScope]::Descendants){
        $this.full_reads++
        if($this.opened){throw 'Compact lookup traversed the expensive background subtree'}
        return $this.Overflow
    }
    if($scope -ne [System.Windows.Automation.TreeScope]::Children -or !$this.opened){throw 'Unexpected compact search scope'}
    $this.child_reads++
    if($this.child_reads -eq 1){throw [System.Windows.Automation.ElementNotAvailableException]::new('Popup is materializing')}
    if($this.child_reads -eq 2){return}
    $this.PopupHost
}
& $Helper -Root $popup -Name File
if($popup.full_reads -ne 1 -or $popup.child_reads -ne 4 -or $popup.popup_reads -ne 2 -or !$popup.expanded){throw 'Transient popup query did not retain its bounded scope and expand the owned File submenu'}
Write-Output 'Application menu popup materialization: passed'
$popup.full_reads=0;$popup.child_reads=0;$popup.popup_reads=0;$popup.opened=$false;$popup.expanded=$false;$popup.failure='fixture-popup-provider-failure'
$failed=$false
try{& $Helper -Root $popup -Name File}catch{if($_.Exception.Message -notmatch 'fixture-popup-provider-failure'){throw};$failed=$true}
if(!$failed -or $popup.full_reads -ne 1 -or $popup.child_reads -ne 3 -or $popup.popup_reads -ne 1 -or $popup.expanded){throw 'Unexpected popup provider failure was suppressed'}
Write-Output 'Application menu popup provider failure: passed'

$tokens=$null;$errors=$null
$capy=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Shared snapshot helper did not parse'}
$read=$capy.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Read-Snapshot'},$true)
.([scriptblock]::Create($read.Extent.Text))
$owned=Join-Path ([IO.Path]::GetTempPath()) ('capy-menu-'+[guid]::NewGuid().ToString('N'))
$oldContext=$env:CAPY_FIXTURE_CONTEXT
try{
    [IO.Directory]::CreateDirectory($owned)|Out-Null;$env:CAPY_FIXTURE_CONTEXT=Join-Path $owned 'context.json'
    $registration=@{process_id=$PID;hwnd=111;window_id=77;trace_directory=$owned}
    @{windows=@($registration)}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT
    $state=[pscustomobject]@{stage=0;events=[Collections.Generic.List[string]]::new();items=@();popup=$null;generic=$null;row=$null;headerRows=@();submenu=$null;delayed=''}
    function Menu-Node([string]$Id,$Type){
        $node=[pscustomobject]@{Current=[pscustomobject]@{AutomationId=$Id;Name=$Id;ControlType=$Type;IsEnabled=$true;IsOffscreen=$false;ProcessId=$PID};State=$state}
        $node|Add-Member NoteProperty Cached $node.Current
        $node|Add-Member ScriptMethod GetCurrentPattern {
            param($pattern)
            if($this.Current.ProcessId -ne $PID){throw 'Invoked a foreign header provider'}
            $result=[pscustomobject]@{Id=$this.Current.AutomationId;State=$this.State;Current=[pscustomobject]@{ExpandCollapseState=[System.Windows.Automation.ExpandCollapseState]::Expanded}}
            $result|Add-Member ScriptMethod Invoke {
                $this.State.events.Add($this.Id)
                switch($this.Id){'header-overflow-2'{$this.State.stage=1};'header-overflow-item-99'{$this.State.stage=2};'application-menu-file'{};default{throw 'Invoked the wrong native header entry'}}
            }
            $result|Add-Member ScriptMethod Expand {$this.State.events.Add('file-expanded')}
            $result
        }
        $node
    }
    $title=Menu-Node 'title-bar' ([System.Windows.Automation.ControlType]::Pane)
    $title.Current|Add-Member NoteProperty ItemStatus (@{geometry=@{hidden=@(@(17),@(),@(99))}}|ConvertTo-Json -Depth 8 -Compress)
    $right=Menu-Node 'header-overflow-2' ([System.Windows.Automation.ControlType]::Button)
    $wrong=Menu-Node 'header-overflow-0' ([System.Windows.Automation.ControlType]::Button)
    $state.items=@($title,$wrong,$right);$state.row=Menu-Node 'header-overflow-item-99' ([System.Windows.Automation.ControlType]::Button)
    $foreign=Menu-Node 'header-overflow-item-99' ([System.Windows.Automation.ControlType]::Button);$foreign.Current.ProcessId=$PID+1
    $other=Menu-Node 'header-overflow-item-17' ([System.Windows.Automation.ControlType]::Button)
    $state.headerRows=@($other,$foreign,$state.row)
    $state.generic=[pscustomobject]@{Cached=[pscustomobject]@{Name='Popup';ClassName='';ControlType=[System.Windows.Automation.ControlType]::Pane}}
    $state.submenu=Menu-Node 'menu-file' ([System.Windows.Automation.ControlType]::MenuItem)
    $state.popup=[pscustomobject]@{Cached=[pscustomobject]@{Name='PopupHost';ClassName='Microsoft.UI.Content.PopupWindowSiteBridge';ControlType=[System.Windows.Automation.ControlType]::Pane};State=$state}
    $state.popup|Add-Member ScriptMethod FindAll {param($scope,$condition);if($this.State.stage -ne 2){throw 'Generic Flyout was treated as a windowed MenuFlyout'};$this.State.submenu}
    $window=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=$PID;NativeWindowHandle=111};State=$state}
    $window|Add-Member ScriptMethod FindAll {
        param($scope,$condition)
        if($scope -eq [System.Windows.Automation.TreeScope]::Children){if($this.State.stage -eq 1){return $this.State.generic};return $this.State.popup}
        if($this.State.stage -eq 1){return $this.State.headerRows}
        if($this.State.stage -eq 2){throw 'Primary MenuFlyout discovery escaped its owned popup'}
        $this.State.items
    }
    $snapshot=@{process_id=$PID;window_id=77;model=@{windows_isolated_settings=$true;header=@{model=@{zones=@(@(@{id=17;item=@{kind='tool'}}),@(),@(@{id=99;item=@{kind='menu'}}))}}}}
    $snapshotPath=Join-Path $owned "ui-state-$PID-77.json"
    function Start-Sleep([int]$Milliseconds){
        if($state.delayed -eq 'registration'){@{windows=@($registration)}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT}
        elseif($state.delayed -eq 'hidden'){$title.Current.ItemStatus=@{geometry=@{hidden=@(@(17),@(),@(99))}}|ConvertTo-Json -Depth 8 -Compress}
        if($state.delayed){$state.events.Add('metadata-published');$state.delayed=''}
    }
    foreach($kind in 'menu','menu_labels'){foreach($metadata in 'ready','registration','hidden'){
        $snapshot.model.header.model.zones[2][0].item.kind=$kind
        $snapshot|ConvertTo-Json -Depth 14|Set-Content -LiteralPath $snapshotPath
        $state.stage=0;$state.events.Clear();$state.delayed=if($metadata -eq 'ready'){''}else{$metadata}
        @{windows=$(if($metadata -eq 'registration'){@()}else{@($registration)})}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT
        $title.Current.ItemStatus=@{geometry=@{hidden=@(@(17),@(),$(if($metadata -eq 'hidden'){@()}else{@(99)}))}}|ConvertTo-Json -Depth 8 -Compress
        $entry=& $Helper -Root $window -Name File -PassThru
        $expected=if($metadata -eq 'ready'){'header-overflow-2,header-overflow-item-99,file-expanded'}else{'metadata-published,header-overflow-2,header-overflow-item-99,file-expanded'}
        if($entry.Current.AutomationId -ne 'menu-file' -or ($state.events -join ',') -ne $expected){throw 'Header overflow did not invoke exactly its published owning zone and menu component'}
        $state.stage=0;$state.events.Clear()
        $entry=& $Helper -Root $window -Name File -Inspect
        if($entry -ne $right -or $state.events.Count -or $state.stage){throw 'Header overflow inspection caused native input or returned the wrong zone'}
    }}
    $inline=Menu-Node 'application-menu-file' ([System.Windows.Automation.ControlType]::Button)
    $state.items=@($inline);$state.stage=0;$state.events.Clear()
    & $Helper -Root $window -Name File
    if(($state.events -join ',') -ne 'application-menu-file'){throw 'Header overflow changed the original inline route'}
    $helperAst=[Management.Automation.Language.Parser]::ParseFile($Helper,[ref]$tokens,[ref]$errors)
    foreach($name in 'Visible-Control','Header-Overflow'){
        $fn=$helperAst.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name}.GetNewClosure(),$true)
        if(!$fn){throw "Missing actual menu helper $name"};.([scriptblock]::Create($fn.Extent.Text))
    }
    $Root=$window;$menuNodes=@($title,$wrong,$right);$Inspect=$false
    foreach($negative in 'wrong-zone','no-owned-trace','foreign-snapshot'){
        $title.Current.ItemStatus=@{geometry=@{hidden=@(@(17),$(if($negative -eq 'wrong-zone'){@(99)}else{@()}),$(if($negative -eq 'wrong-zone'){@()}else{@(99)}))}}|ConvertTo-Json -Depth 8 -Compress
        $registration.hwnd=if($negative -eq 'no-owned-trace'){222}else{111}
        @{windows=@($registration)}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT
        $snapshot.window_id=if($negative -eq 'foreign-snapshot'){88}else{77}
        $snapshot|ConvertTo-Json -Depth 14|Set-Content -LiteralPath $snapshotPath
        $failure=$null;$route=$null
        try{$route=Header-Overflow}catch{$failure=$_}
        if($negative -eq 'foreign-snapshot'){if(!$failure -or $failure.Exception.Message -ne 'Registered header snapshot does not belong to the owned drawing'){throw 'Header menu accepted a foreign snapshot'}}
        elseif($failure -or $route){if($failure){throw $failure};throw "Header menu accepted $negative"}
        if($state.events.Count -ne 1){throw 'Header route validation injected native input'}
    }
}finally{[Environment]::SetEnvironmentVariable('CAPY_FIXTURE_CONTEXT',$oldContext,'Process');if(Test-Path -LiteralPath $owned){Remove-Item -LiteralPath $owned -Recurse -Force}}
Write-Output 'Actual menu opener retains inline aliases, discovers generic Flyout rows by owned ID/PID before windowed MenuFlyout discovery, keeps Inspect passive and rejects wrong-zone or foreign ownership.'
