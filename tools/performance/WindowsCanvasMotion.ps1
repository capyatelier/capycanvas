Add-Type -AssemblyName WindowsBase
function Navigation-Focus {
 [CapyWindowApi]::SetForegroundWindow($window)|Out-Null
 (Control 'drawing-canvas').SetFocus()
 Wait-Until {(Control 'drawing-canvas').Current.HasKeyboardFocus -and [CapyRowPointer]::GetForegroundWindow() -eq $window} 'Navigation canvas did not acquire owned keyboard focus'
}
function Navigation-Command([string]$Label,[switch]$Enter) {
 Navigation-Focus;[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4b)
 Wait-Until {$search=Find 'command-search' -Visible;$search -and $search.Current.HasKeyboardFocus} 'Command search did not open'
 (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Label)
 Wait-Until {$row=Find 'command-result-0' -Visible;$row -and $row.Current.Name -eq $Label -and $row.Current.IsEnabled} "Command search did not resolve $Label"
 if($Enter){$at=[Diagnostics.Stopwatch]::GetTimestamp();[CapyRowPointer]::Key([uint32]$review.Id,0x0d);return $at}
 Invoke-Id 'command-result-0'
 Wait-Until {!(Find 'command-search' -Visible)} "Command search did not close after $Label"
}
function Motion-Canvas {
 $canvas=Control 'drawing-canvas' -Arranged
 $excluded=[Collections.Generic.HashSet[string]]::new()
 foreach($node in $canvas.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)){$null=$excluded.Add(($node.GetRuntimeId() -join ':'))}
 $node=$canvas;$bridge=$null
 while($node){
  $null=$excluded.Add(($node.GetRuntimeId() -join ':'))
  if(!$bridge -and $node.Current.NativeWindowHandle -and !$node.Current.IsOffscreen){$bridge=$node}
  if(($node.GetRuntimeId() -join ':') -eq ($root.GetRuntimeId() -join ':')){break}
  $node=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($node)
 }
 if(!$bridge -or !$node -or $bridge.Current.ProcessId -ne $review.Id){throw 'The canvas has no owned visible native ancestor'}
 $parent=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($canvas)
 $parentId=$parent.GetRuntimeId() -join ':'
 $workspace=Control 'canvas-view-info' -Arranged
 while($workspace){
  $container=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($workspace)
  if($container -and ($container.GetRuntimeId() -join ':') -eq $parentId){break}
  $workspace=$container
 }
 if(!$workspace -or ($workspace.GetRuntimeId() -join ':') -eq ($canvas.GetRuntimeId() -join ':') -or $workspace.Current.FrameworkId -ne 'XAML' -or $workspace.Current.ControlType -ne [System.Windows.Automation.ControlType]::Group -or $workspace.Current.AutomationId -or $workspace.Current.NativeWindowHandle -or $workspace.Current.Name -ne $parent.Current.Name){throw 'The transparent workspace container could not be identified'}
 $null=$excluded.Add(($workspace.GetRuntimeId() -join ':'))
 $bounds=$canvas.Current.BoundingRectangle
 $obstacles=@(foreach($item in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)){
  if($excluded.Contains(($item.GetRuntimeId() -join ':')) -or $item.Current.IsOffscreen){continue}
  $rect=$item.Current.BoundingRectangle
  if(!$rect.IsEmpty -and $rect.Width -gt 0 -and $rect.Height -gt 0 -and $bounds.IntersectsWith($rect)){
   [pscustomobject]@{id=$item.Current.AutomationId;name=$item.Current.Name;bounds=$rect}
  }
 })
 [pscustomobject]@{bounds=$bounds;bridge_hwnd=$bridge.Current.NativeWindowHandle;window_hwnd=$root.Current.NativeWindowHandle;obstacles=$obstacles}
}
function Motion-Point($Region,[Windows.Point]$Point) {
 if(!$Region.bounds.Contains($Point)){throw "Motion point $Point is outside the arranged canvas"}
 foreach($item in $Region.obstacles){if($item.bounds.Contains($Point)){throw "Motion point $Point is covered by '$($item.id)' ('$($item.name)')"}}
 [WindowsPenMotion]::VerifyPoint([uint32]$review.Id,[IntPtr]$Region.window_hwnd,[int]$Point.X,[int]$Point.Y)
 [System.Windows.Automation.AutomationElement]::FromPoint($Point)
}
function Motion-Path($Region,[int]$Cx,[int]$Cy,[int]$Rx,[int]$Ry,[int]$Seconds){
 if($Seconds -lt 1 -or $Seconds -gt 30){throw 'Motion duration exceeds the producer bounds'}
 $seen=[Collections.Generic.HashSet[string]]::new();$points=[Collections.Generic.List[object]]::new();$hits=@{};$last=$null
 for($step=0;$step -lt $Seconds*240;$step++){
  $angle=2*[Math]::PI*$step/240;$x=$Cx+[int][Math]::Truncate($Rx*[Math]::Cos($angle));$y=$Cy+[int][Math]::Truncate($Ry*[Math]::Sin($angle));$last=@($x,$y)
  if($seen.Add("$x,$y")){
   $hit=Motion-Point $Region ([Windows.Point]::new($x,$y));$points.Add($last)
   $key=$hit.Current.AutomationId;if(!$key){$key='runtime:'+($hit.GetRuntimeId() -join ':')}
   $hits[$key]=$hit.Current.Name
  }
 }
 [pscustomobject]@{process_id=$review.Id;root_id=($root.GetRuntimeId() -join ':');region=$Region;center=@($Cx,$Cy);radii=@($Rx,$Ry);seconds=$Seconds;rate_hz=240;distinct_points=$points.ToArray();final_up_point=$last;hits=$hits}
}
