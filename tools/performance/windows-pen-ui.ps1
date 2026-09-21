param([int]$ProcessId)
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @"
using System;using System.Runtime.InteropServices;using System.Text;
public static class PenBenchUi {
 [DllImport("user32.dll")]public static extern bool PostMessage(IntPtr h,uint m,UIntPtr w,IntPtr l);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int n);
 [DllImport("user32.dll")]public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,UIntPtr w,IntPtr l,uint f,uint t,out UIntPtr r);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,UIntPtr w,StringBuilder l,uint f,uint t,out UIntPtr r);
 public static void Path(IntPtr h,string s){UIntPtr r;SendMessageTimeout(h,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out r);SendMessageTimeout(h,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out r);foreach(char c in s)SendMessageTimeout(h,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out r);var actual=new StringBuilder(32768);SendMessageTimeout(h,13,new UIntPtr((uint)actual.Capacity),actual,2,2000,out r);if(actual.ToString()!=s)throw new Exception("Picker path mismatch");}
}
"@
[PenBenchUi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$windowProcess=Get-Process -Id $ProcessId
$root=[System.Windows.Automation.AutomationElement]::FromHandle($windowProcess.MainWindowHandle)
function Find([string]$Value,[switch]$Name){$p=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty};$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new($p,$Value))}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=45){$watch=[Diagnostics.Stopwatch]::StartNew();do{if(& $Condition){return};Start-Sleep -Milliseconds 100}while($watch.Elapsed.TotalSeconds -lt $Seconds);throw $Message}
function Invoke-Id([string]$Id){$hit=@{item=$null};Wait-Until {$hit.item=Find $Id;$hit.item -and $hit.item.Current.IsEnabled} "Missing enabled control: $Id";$hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Open-Project([string]$Path){
 & (Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/open-application-menu.ps1') -Root $root -Name 'File'
 Invoke-Id 'open_document'
 $hit=@{edit=$null;button=$null}
 Wait-Until {$hit.edit=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit'),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')));$null -ne $hit.edit} 'Missing Open picker'
 [PenBenchUi]::Path([IntPtr]$hit.edit.Current.NativeWindowHandle,$Path)
 $picker=Find 'Open' -Name
 $hit.button=$picker.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1'))
 [PenBenchUi]::PostMessage([IntPtr]$hit.button.Current.NativeWindowHandle,245,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
 Wait-Until {!(Find 'Open' -Name)} 'Open did not finish' 90
}
