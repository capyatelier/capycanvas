$script:CapyNativeInputWait=${function:Wait-Until}
function Get-CapyInputParent($Element){[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($Element)}
function Start-CapyInputQuery([string]$Kind,[double[]]$Point,$Channel){
    if($Channel){
        if(!$review -or $Channel.process -ne $review.Id){throw 'Input query channel belongs to another drawing'}
        $window=$Channel.window;$arguments=$Channel.arguments;$command=$Channel.command
    }else{
        if(!$review -or !$root -or $root.Current.ProcessId -ne $review.Id){throw 'Input readiness requires the owned drawing root'}
        $inputRoot=$root;$window=[long]$inputRoot.Current.NativeWindowHandle
        if(!$window){
            for($ancestor=Get-CapyInputParent $inputRoot;$ancestor -and $ancestor.Current.ProcessId -eq $review.Id;$ancestor=Get-CapyInputParent $ancestor){
                $handle=[long]$ancestor.Current.NativeWindowHandle
                if($handle){$inputRoot=$ancestor;$window=$handle}
            }
        }
        if(!$window){throw 'Input readiness requires an owned native window'}
        $find={param($id) $inputRoot.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$id))}
        $arguments=& $find 'test-input-query';$command=& $find 'test-input-query-invoke'
        $argumentProcess=if($arguments){$arguments.Current.ProcessId};$commandProcess=if($command){$command.Current.ProcessId}
        if(!$arguments -or !$command -or $argumentProcess -ne $review.Id -or $commandProcess -ne $review.Id){
            throw ('Owned native input query is unavailable; endpoint_state='+(@{window=$window;arguments_found=[bool]$arguments;command_found=[bool]$command;arguments_process=$argumentProcess;command_process=$commandProcess}|ConvertTo-Json -Compress))
        }
    }
    $nonce=[guid]::NewGuid().ToString('N');$query=@{request=$nonce;kind=$Kind}
    if($Kind -eq 'pointer'){
        if($Point.Count -ne 2){throw 'Pointer readiness requires its unchanged screen point'}
        $query.screen=$Point
    }
    $arguments.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue(($query|ConvertTo-Json -Compress))
    $command.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    @{request=$nonce;process=$review.Id;window=$window;arguments=$arguments;command=$command;kind=$Kind}
}
function Test-CapyInputInteger($Value){($Value -is [int] -or $Value -is [long] -or $Value -is [uint64]) -and $Value -ge 0}
function Test-CapyInputPoint($Value){
    if($Value -isnot [array] -or $Value.Count -ne 2){return $false}
    foreach($coordinate in $Value){
        if(($coordinate -isnot [int] -and $coordinate -isnot [long] -and $coordinate -isnot [uint64] -and $coordinate -isnot [double] -and $coordinate -isnot [single] -and $coordinate -isnot [decimal]) -or
            [double]::IsNaN($coordinate) -or [double]::IsInfinity($coordinate) -or [Math]::Truncate($coordinate) -ne $coordinate -or $coordinate -lt [int]::MinValue -or $coordinate -gt [int]::MaxValue){return $false}
    }
    $true
}
function Assert-CapyInputIdentity($Identity,[string]$Field){
    if($Identity.path -isnot [array] -or @($Identity.path|Where-Object {$_ -isnot [string]}).Count){throw "Native input receipt has an invalid $Field.path"}
    if($null -ne $Identity.record -and !(Test-CapyInputInteger $Identity.record)){throw "Native input receipt has an invalid $Field.record"}
    if($null -ne $Identity.part -and $Identity.part -isnot [string]){throw "Native input receipt has an invalid $Field.part"}
}
function Assert-CapyInputTarget($Target,[switch]$Keyboard){
    if($Target -is [string]){if(!$Target){throw 'Input site has no intended native target id'};return}
    if($null -ne $Target.id -and $Target.id -isnot [string]){throw 'Input site has an invalid native target id'}
    if($null -ne $Target.within -and $Target.within -isnot [bool]){throw 'Input site has an invalid native target scope'}
    if($null -ne $Target.record -and !(Test-CapyInputInteger $Target.record)){throw 'Input site has an invalid native target record'}
    if($null -ne $Target.part -and ($Target.part -isnot [string] -or !$Target.part)){throw 'Input site has an invalid native target part'}
    if(!$Target.id -and $Keyboard){throw 'Input site has no intended native target id'}
    if(!$Target.id -and ($null -eq $Target.record -or !$Target.part)){throw 'Input site has no intended native target id or complete record/part'}
}
function Read-CapyInputReceipt($Request){
    $json=$Request.arguments.Current.ItemStatus;if(!$json){return}
    $receipt=$json|ConvertFrom-Json
    if($receipt.request -cne $Request.request){return}
    if($receipt.error){throw "Native input query refused the request: $($receipt.error)"}
    foreach($field in 'process_id','window_id','hwnd','foreground','point_hwnd','point_process','down_serial','dropped'){
        if(!(Test-CapyInputInteger $receipt.$field)){throw "Native input receipt has an invalid $field"}
    }
    if($receipt.process_id -ne $Request.process -or $receipt.hwnd -ne $Request.window){throw 'Native input receipt belongs to another drawing'}
    if($receipt.active -isnot [bool] -or $receipt.progress -isnot [pscustomobject]){throw 'Native input receipt lacks its applied state'}
    if($receipt.downs -isnot [array]){throw 'Native input receipt has an invalid downs'}
    foreach($entry in $receipt.downs){
        foreach($field in 'serial','pointer_id','time'){
            if(!(Test-CapyInputInteger $entry.$field)){throw "Native input receipt has an invalid downs.$field"}
        }
        if($entry.device -cnotin @('mouse','pen','touch')){throw 'Native input receipt has an invalid downs.device'}
        if($entry.route -cnotin @('routed','independent','xaml_canvas')){throw 'Native input receipt has an invalid downs.route'}
        if($null -ne $entry.admitted -and $entry.admitted -isnot [bool]){throw 'Native input receipt has an invalid downs.admitted'}
        Assert-CapyInputIdentity $entry 'downs'
        if(!(Test-CapyInputPoint $entry.screen)){throw 'Native input receipt has an invalid downs.screen'}
    }
    foreach($field in 'enabled','loaded','canvas'){if($receipt.pointer.$field -isnot [bool]){throw "Native pointer receipt has an invalid $field"}}
    Assert-CapyInputIdentity $receipt.pointer 'pointer'
    Assert-CapyInputIdentity $receipt.focus 'focus'
    $receipt|Add-Member -NotePropertyName CapyNativeInputChannel -NotePropertyValue $Request -PassThru
}
function Test-CapyInputTargetIdentity($Actual,$Target,[switch]$Keyboard){
    Assert-CapyInputTarget $Target -Keyboard:$Keyboard
    if($Target -is [string]){$Target=@{id=$Target}}
    if(!$Actual){return $false}
    if($Target.id){
        $path=@($Actual.path)
        if($Keyboard -and !$Target.within){$matched=$path.Count -gt 0 -and $path[0] -ceq $Target.id}
        else{$matched=$path -ccontains $Target.id}
        if(!$matched){return $false}
    }
    if($null -ne $Target.record -and $Actual.record -ne $Target.record){return $false}
    if($Target.part -and $Actual.part -cne $Target.part){return $false}
    $true
}
function Test-CapyInputTarget($Actual,$Target,[switch]$Keyboard){
    if(!(Test-CapyInputTargetIdentity $Actual $Target -Keyboard:$Keyboard)){return $false}
    if($Target -is [string]){$Target=@{id=$Target}}
    if($Target.id -ceq 'drawing-canvas' -and !$Keyboard -and !$Actual.canvas -and $Actual.route -notin @('independent','xaml_canvas')){return $false}
    $true
}
function Get-CapyInputIdentityProgress($Identity,[switch]$Pointer){
    $value=[ordered]@{path=@($Identity.path);record=$Identity.record;part=$Identity.part}
    if($Pointer){$value['enabled']=$Identity.enabled;$value['loaded']=$Identity.loaded;$value['canvas']=$Identity.canvas}
    $value
}
function Test-CapyInputProgress($State){
    if(!$State.receipt){return $false}
    $sample=$State.receipt
    $entries=@(foreach($entry in $sample.downs){[ordered]@{serial=$entry.serial;pointer_id=$entry.pointer_id;time=$entry.time;device=$entry.device;screen=@($entry.screen);path=@($entry.path);route=$entry.route;admitted=$entry.admitted;record=$entry.record;part=$entry.part}})
    $key=[ordered]@{process_id=$sample.process_id;window_id=$sample.window_id;hwnd=$sample.hwnd;active=$sample.active;foreground=$sample.foreground;point_hwnd=$sample.point_hwnd;point_process=$sample.point_process;
        pointer=(Get-CapyInputIdentityProgress $sample.pointer -Pointer);focus=(Get-CapyInputIdentityProgress $sample.focus);downs=$entries}|ConvertTo-Json -Depth 20 -Compress
    $before=$State.progress;$State.progress=$key
    $null -ne $before -and $key -cne $before
}
function Get-CapyInputActualIdentity($Node){
    $path=@($Node.path|Select-Object -First 8|ForEach-Object {$text=[string]$_;$text.Substring(0,[Math]::Min(160,$text.Length))})
    $part=[string]$Node.part
    @{path=$path;record=$(if(Test-CapyInputInteger $Node.record){$Node.record}else{$null});part=$part.Substring(0,[Math]::Min(80,$part.Length));
        enabled=$(if($Node.enabled -is [bool]){$Node.enabled}else{$null});loaded=$(if($Node.loaded -is [bool]){$Node.loaded}else{$null});canvas=$(if($Node.canvas -is [bool]){$Node.canvas}else{$null})}
}
function Wait-CapyInputTarget($Target,[string]$Kind,[double[]]$Point){
    Assert-CapyInputTarget $Target -Keyboard:($Kind -eq 'keyboard')
    $inputTarget=@{request=$null;receipt=$null;progress=$null}
    $capyTargetMessage="Native $Kind target did not become input-ready: $($Target|ConvertTo-Json -Compress)"
    try{
    & $script:CapyNativeInputWait {
        if(!$inputTarget.request){$inputTarget.request=Start-CapyInputQuery $Kind $Point}
        $sample=Read-CapyInputReceipt $inputTarget.request
        if(!$sample){return $false}
        $inputTarget.receipt=$sample;$inputTarget.request=$null
        if($Kind -eq 'keyboard' -and (!$sample.active -or $sample.foreground -ne $sample.hwnd)){return $false}
        if($Kind -eq 'pointer'){
            if($sample.point_process -ne $review.Id -or !$sample.pointer.enabled -or !$sample.pointer.loaded){return $false}
            return Test-CapyInputTarget $sample.pointer $Target
        }
        Test-CapyInputTarget $sample.focus $Target -Keyboard
    } $capyTargetMessage -ResetBound {Test-CapyInputProgress $inputTarget}
    }catch{
        if($_.Exception.Message -cne $capyTargetMessage){throw}
        $capyLastActual=if($inputTarget.receipt){
            $capyLastReceipt=$inputTarget.receipt
            @{process_id=$capyLastReceipt.process_id;window_id=$capyLastReceipt.window_id;hwnd=$capyLastReceipt.hwnd;active=$capyLastReceipt.active;foreground=$capyLastReceipt.foreground;
                point_hwnd=$capyLastReceipt.point_hwnd;point_process=$capyLastReceipt.point_process;down_serial=$capyLastReceipt.down_serial;
                pointer=(Get-CapyInputActualIdentity $capyLastReceipt.pointer);focus=(Get-CapyInputActualIdentity $capyLastReceipt.focus)}
        }
        throw ($capyTargetMessage+'; last_actual='+($capyLastActual|ConvertTo-Json -Depth 8 -Compress))
    }
    $inputTarget.receipt
}
function Assert-CapyPointerDown($Before,$Target,[string]$Device,[double[]]$Point){
    if($null -eq $Before.CapyNativeInputChannel){throw 'Input receiver has no admitted query channel'}
    $downWait=@{request=$null;receipt=$null;progress=$null}
    & $script:CapyNativeInputWait {
        if(!$downWait.request){$downWait.request=Start-CapyInputQuery 'receipts' $null $Before.CapyNativeInputChannel}
        $sample=Read-CapyInputReceipt $downWait.request
        if(!$sample){return $false}
        $downWait.request=$null;$downWait.receipt=$sample
        if($sample.window_id -ne $Before.window_id){throw 'Input receiver window identity changed'}
        $entries=@($sample.downs|Where-Object {$_.serial -gt $Before.down_serial -and $_.device -ceq $Device})
        if($Target -ceq 'drawing-canvas' -or $Target.id -ceq 'drawing-canvas'){
            $wrong=@($entries|Where-Object {$_.route -ceq 'routed' -and $_.path -cnotcontains 'drawing-canvas'})
            if($wrong.Count){throw "Native Down reached another receiver: expected=drawing-canvas actual=$($wrong[0]|ConvertTo-Json -Depth 12 -Compress)"}
            $entries=@($entries|Where-Object {$_.route -in @('independent','xaml_canvas')})
        }
        if(!$entries.Count){
            if($sample.dropped -gt $Before.dropped){throw 'Native Down receipt was lost from its bounded journal'}
            return $false
        }
        foreach($entry in $entries){
            if(!(Test-CapyInputTarget $entry $Target) -or $entry.screen.Count -ne 2 -or [Math]::Abs($entry.screen[0]-$Point[0]) -gt 1 -or [Math]::Abs($entry.screen[1]-$Point[1]) -gt 1){
                throw "Native Down reached another receiver: expected=$($Target|ConvertTo-Json -Compress) actual=$($entry|ConvertTo-Json -Depth 12 -Compress)"
            }
            if($Target -ceq 'drawing-canvas' -or $Target.id -ceq 'drawing-canvas'){
                if($null -eq $entry.admitted){continue}
                if(!$entry.admitted){throw "Canvas rejected its native Down: $($entry|ConvertTo-Json -Depth 12 -Compress)"}
            }
            return $true
        }
        return $false
    } "Native Down did not reach its intended target: $($Target|ConvertTo-Json -Compress)" -ResetBound {Test-CapyInputProgress $downWait}
}
function Set-CapyInputContactMatch([int]$Expected,$Candidates,$Assignments,$Visited){
    foreach($group in $Candidates[$Expected]){
        if($Visited.ContainsKey($group)){continue};$Visited[$group]=$true
        if(!$Assignments.ContainsKey($group) -or (Set-CapyInputContactMatch $Assignments[$group] $Candidates $Assignments $Visited)){$Assignments[$group]=$Expected;return $true}
    }
    $false
}
function Assert-CapyPointerContacts($Before,$Contacts){
    if($null -eq $Before.CapyNativeInputChannel){throw 'Input receiver has no admitted query channel'}
    $batchWait=@{request=$null;receipt=$null;progress=$null}
    & $script:CapyNativeInputWait {
        if(!$batchWait.request){$batchWait.request=Start-CapyInputQuery 'receipts' $null $Before.CapyNativeInputChannel}
        $sample=Read-CapyInputReceipt $batchWait.request
        if(!$sample){return $false}
        $batchWait.request=$null;$batchWait.receipt=$sample
        if($sample.window_id -ne $Before.window_id){throw 'Input receiver window identity changed'}
        if($sample.dropped -gt $Before.dropped){throw 'Native Down receipt was lost from its bounded journal'}
        $groups=@($sample.downs|Where-Object {$_.serial -gt $Before.down_serial}|Group-Object {('{0}:{1}:{2}' -f $_.pointer_id,$_.time,$_.device)})
        $candidates=[Collections.Generic.List[object]]::new()
        foreach($contact in $Contacts){$candidates.Add([Collections.Generic.List[int]]::new())}
        for($groupIndex=0;$groupIndex -lt $groups.Count;$groupIndex++){
            $entries=@($groups[$groupIndex].Group);$declared=[Collections.Generic.List[int]]::new()
            for($contactIndex=0;$contactIndex -lt $Contacts.Count;$contactIndex++){
                $contact=$Contacts[$contactIndex]
                if($entries[0].device -ceq $contact.Device -and [Math]::Abs($entries[0].screen[0]-$contact.Point[0]) -le 1 -and [Math]::Abs($entries[0].screen[1]-$contact.Point[1]) -le 1){$declared.Add($contactIndex)}
            }
            if(!$declared.Count){throw "Native Down reached an undeclared batch point or device: $($entries|ConvertTo-Json -Depth 12 -Compress)"}
            foreach($entry in $entries){
                $matched=$false;$atDeclaredPoint=$false
                foreach($contact in $Contacts){
                    if($entry.device -ceq $contact.Device -and [Math]::Abs($entry.screen[0]-$contact.Point[0]) -le 1 -and [Math]::Abs($entry.screen[1]-$contact.Point[1]) -le 1){
                        $atDeclaredPoint=$true;if(Test-CapyInputTargetIdentity $entry $contact.Target){$matched=$true}
                    }
                }
                if(!$atDeclaredPoint){throw "Native Down reached an undeclared batch point or device: $($entry|ConvertTo-Json -Depth 12 -Compress)"}
                if(!$matched){throw "Native Down reached another receiver: expected=$($Contacts|ConvertTo-Json -Depth 12 -Compress) actual=$($entry|ConvertTo-Json -Depth 12 -Compress)"}
                if($entry.route -in @('independent','xaml_canvas') -and $null -ne $entry.admitted -and !$entry.admitted){throw "Canvas rejected its native Down: $($entry|ConvertTo-Json -Depth 12 -Compress)"}
            }
            foreach($contactIndex in $declared){
                $target=$Contacts[$contactIndex].Target
                foreach($entry in $entries){
                    if([Math]::Abs($entry.screen[0]-$Contacts[$contactIndex].Point[0]) -gt 1 -or [Math]::Abs($entry.screen[1]-$Contacts[$contactIndex].Point[1]) -gt 1 -or !(Test-CapyInputTarget $entry $target)){continue}
                    if(($target -ceq 'drawing-canvas' -or $target.id -ceq 'drawing-canvas') -and $null -eq $entry.admitted){continue}
                    $candidates[$contactIndex].Add($groupIndex);break
                }
            }
        }
        if($groups.Count -gt $Contacts.Count){throw 'Native Down batch contained more contacts than declared'}
        $assignments=@{}
        for($contactIndex=0;$contactIndex -lt $Contacts.Count;$contactIndex++){
            if(!(Set-CapyInputContactMatch $contactIndex $candidates $assignments @{})){return $false}
        }
        $true
    } 'Native Down batch did not reach every declared contact' -ResetBound {Test-CapyInputProgress $batchWait}
}
function Invoke-CapyPointerInput([Alias('Target')]$CapyInputTarget,[Alias('Point')][double[]]$CapyInputPoint,[Alias('Device')][string]$CapyInputDevice,[Alias('Action')][scriptblock]$CapyInputAction,[Alias('Contacts')][object[]]$CapyInputContacts){
    if($PSBoundParameters.ContainsKey('CapyInputContacts')){
        if(!$CapyInputContacts.Count){throw 'Input batch has no declared contacts'}
        foreach($capyInputField in 'CapyInputTarget','CapyInputPoint','CapyInputDevice'){if($PSBoundParameters.ContainsKey($capyInputField)){throw 'Input batch cannot mix Contacts with single-contact arguments'}}
        $capyPointerContacts=@(foreach($capyInputContact in $CapyInputContacts){
            Assert-CapyInputTarget $capyInputContact.Target
            if(!(Test-CapyInputPoint $capyInputContact.Point) -or $capyInputContact.Device -cnotin @('mouse','pen','touch')){throw 'Input batch has an invalid declared point or device'}
            $capyContactTarget=if($capyInputContact.Target -is [string]){$capyInputContact.Target}else{@{id=$capyInputContact.Target.id;record=$capyInputContact.Target.record;part=$capyInputContact.Target.part;within=$capyInputContact.Target.within}}
            @{Target=$capyContactTarget;Point=[double[]]@($capyInputContact.Point[0],$capyInputContact.Point[1]);Device=[string]$capyInputContact.Device}
        })
        $capyPointerBaselines=@(foreach($capyInputContact in $capyPointerContacts){Wait-CapyInputTarget $capyInputContact.Target 'pointer' $capyInputContact.Point})
        $capyPointerBefore=$capyPointerBaselines|Sort-Object down_serial -Descending|Select-Object -First 1
        foreach($capyPointerBaseline in $capyPointerBaselines){if($capyPointerBaseline.window_id -ne $capyPointerBefore.window_id){throw 'Input receiver window identity changed'}}
        & $CapyInputAction
        Assert-CapyPointerContacts $capyPointerBefore $capyPointerContacts
        return
    }
    $capyPointerBefore=Wait-CapyInputTarget $CapyInputTarget 'pointer' $CapyInputPoint
    & $CapyInputAction
    Assert-CapyPointerDown $capyPointerBefore $CapyInputTarget $CapyInputDevice $CapyInputPoint
}
function Invoke-CapyKeyInput([Alias('Target')]$CapyInputTarget,[Alias('Action')][scriptblock]$CapyInputAction){
    $null=Wait-CapyInputTarget $CapyInputTarget 'keyboard' $null
    & $CapyInputAction
}
