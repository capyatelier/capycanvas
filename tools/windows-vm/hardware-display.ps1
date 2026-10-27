param([Parameter(Mandatory)][int]$Width,[Parameter(Mandatory)][int]$Height,
 [Parameter(Mandatory)][string]$Receipt)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
function New-HardwareEdid([int]$Width,[int]$Height) {
 if($Width -lt 1920 -or $Height -lt 1080 -or $Width -gt 4095 -or $Height -gt 4095){throw 'Hardware desktop dimensions must be 1920x1080 through 4095x4095'}
 $edid=[Convert]::FromBase64String('AP///////wAOGXgAAQAAAAEkAQSlNiJ4Cu6Vo1RMmSYPUFQAAAABAQEBAQEBAQEBAQEBAQEBvdEAoKBALWAwIDYAHFQhAAAe3mgAoKBALWAwIDYAHFQhAAAeAAAA/ABDQVBZIEdQVSAxMjAKAAAA/QAyeB7cPAAKICAgICAgAIQ=')
 foreach($item in @(@(54,120),@(72,60))){
  $offset=$item[0];$clock=[int][Math]::Round(($Width+160)*($Height+45)*$item[1]/10000.0)
  if($clock -gt 65535){throw 'Hardware desktop timing exceeds EDID pixel clock range'}
  $edid[$offset]=[byte]($clock -band 255);$edid[$offset+1]=[byte]($clock -shr 8)
  $edid[$offset+2]=[byte]($Width -band 255);$edid[$offset+4]=[byte](($Width -shr 8) -shl 4)
  $edid[$offset+5]=[byte]($Height -band 255);$edid[$offset+7]=[byte](($Height -shr 8) -shl 4)
 }
 $edid[116]=[byte][Math]::Max($edid[116],[Math]::Ceiling(($Height+45)*120/1000.0))
 $edid[117]=[byte][Math]::Max($edid[117],[Math]::Ceiling(($Width+160)*($Height+45)*120/10000000.0))
 $sum=0;for($i=0;$i -lt 127;$i++){$sum+=$edid[$i]};$edid[127]=[byte]((256-($sum%256))%256)
 return ,$edid
}
function Write-DisplayReceipt($Result) {
 $pending=$Receipt+'.pending'
 $Result|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $pending
 Move-Item -LiteralPath $pending -Destination $Receipt -Force
}
try {
 $drivers=@(Get-CimInstance Win32_PnPSignedDriver | Where-Object {$_.DeviceClass -eq 'DISPLAY' -and $_.DriverProviderName -match 'NVIDIA' -and $_.IsSigned})
 $healthy=@(Get-PnpDevice -PresentOnly -Class Display | Where-Object {$_.Status -eq 'OK' -and $_.InstanceId -in $drivers.DeviceID})
 if($healthy.Count -ne 1){throw 'Expected one healthy signed assigned NVIDIA display adapter'}
 Add-Type -Path (Join-Path $PSScriptRoot 'hardware-display.cs')
 $ownedFile=Join-Path $PSScriptRoot 'owned-edid.json'
 $owned=if(Test-Path -LiteralPath $ownedFile){Get-Content -LiteralPath $ownedFile -Raw|ConvertFrom-Json}else{$null}
 $edid=New-HardwareEdid $Width $Height
 $previous=if($owned){[Convert]::FromBase64String($owned.edid)}else{$null}
 $connector=if($owned){[uint32]$owned.output}else{[uint32]0}
 $attached=[CapyHardwareEdid]::Run($edid,$previous,$connector)
 @{edid=[Convert]::ToBase64String($edid);output=$attached.output_mask}|ConvertTo-Json|Set-Content -LiteralPath $ownedFile
 $deadline=(Get-Date).AddSeconds(60)
 while($true){
  try{$display=[CapyHardwareDisplay]::Set($Width,$Height);break}catch{if((Get-Date) -ge $deadline){throw};Start-Sleep -Seconds 2}
 }
 Write-DisplayReceipt @{success=$true;boot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o');time=[DateTime]::UtcNow.ToString('o');display=$display;edid=$attached;driver=$drivers[0].DriverVersion}
} catch {
 Write-DisplayReceipt @{success=$false;time=[DateTime]::UtcNow.ToString('o');error=($_|Out-String)}
 throw
}
