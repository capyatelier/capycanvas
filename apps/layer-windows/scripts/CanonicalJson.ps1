function Canonical($Value){
    if($Value -is [System.Collections.IDictionary] -or ($null -ne $Value -and $Value.GetType() -eq [System.Management.Automation.PSCustomObject])){
        $record=[System.Collections.Specialized.OrderedDictionary]::new([StringComparer]::Ordinal)
        [string[]]$keys=@(if($Value -is [System.Collections.IDictionary]){$Value.Keys}else{foreach($property in $Value.PSObject.Properties){$property.Name}})
        [Array]::Sort($keys,[StringComparer]::Ordinal)
        foreach($key in $keys){
            $item=if($Value -is [System.Collections.IDictionary]){$Value[$key]}else{$Value.PSObject.Properties[$key].Value}
            $record.Add($key,(Canonical $item))
        }
        return ,$record
    }
    if($Value -is [array]){
        $items=[Collections.Generic.List[object]]::new()
        foreach($item in $Value){$items.Add((Canonical $item))}
        return ,$items.ToArray()
    }
    $Value
}
