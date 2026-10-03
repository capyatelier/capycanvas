param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -AssemblyName System.Drawing,System.Windows.Forms
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class CapyIme {
 [DllImport("user32.dll")] static extern IntPtr GetKeyboardLayout(uint thread);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint owner);
 [DllImport("imm32.dll")] static extern IntPtr ImmGetDefaultIMEWnd(IntPtr window);
 [DllImport("user32.dll",SetLastError=true)] static extern IntPtr SendMessageTimeout(IntPtr window,uint message,IntPtr w,IntPtr l,uint flags,uint timeout,out IntPtr result);
 public static int Language(IntPtr window,uint process){uint owner;var thread=GetWindowThreadProcessId(window,out owner);if(owner!=process)throw new Exception("IME target has an unexpected owner.");return (int)((long)GetKeyboardLayout(thread)&0xffff);}
 public static void Open(IntPtr window,uint process){uint owner;var ime=ImmGetDefaultIMEWnd(window);GetWindowThreadProcessId(ime,out owner);if(ime==IntPtr.Zero||owner!=process)throw new Exception("The owned window has no IME provider.");IntPtr result;if(SendMessageTimeout(ime,0x283,new IntPtr(6),new IntPtr(1),2,2000,out result)==IntPtr.Zero)throw new Exception("The owned IME did not acknowledge activation.");}
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/ime/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $run 'profile'))|Out-Null
$script:CapyTraceDirectory=$run
$completed=$false
$script:imeSequence=0
function Value($Control){$Control.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value}
function Key([uint16]$Code){[CapyRowPointer]::Key([uint32]$review.Id,$Code);Start-Sleep -Milliseconds 100}
function Focus($Control){$Control.SetFocus();Wait-Until {$Control.Current.HasKeyboardFocus} 'The owned editor did not receive focus'}
function Select-All{[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17),65)}
function Capture-Desktop([string]$Name){
 $bounds=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds
 $bitmap=[Drawing.Bitmap]::new($bounds.Width,$bounds.Height);$graphics=[Drawing.Graphics]::FromImage($bitmap)
 try{$graphics.CopyFromScreen($bounds.Location,[Drawing.Point]::Empty,$bounds.Size);$bitmap.Save((Join-Path $run ($Name+'.png')))}finally{$graphics.Dispose();$bitmap.Dispose()}
 (Model)|ConvertTo-Json -Depth 80|Set-Content (Join-Path $run ($Name+'-model.json'))
}
function Compose($Control,[string]$Romaji,[string]$Preedit){
 $script:imeSequence++
 Focus $Control
 Start-Sleep -Milliseconds 250
 [CapyIme]::Open($review.MainWindowHandle,[uint32]$review.Id)
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17),20)
 Select-All
 foreach($letter in $Romaji.ToUpperInvariant().ToCharArray()){Key ([uint16]$letter)}
 try{Wait-Until {(Value $Control) -eq $Preedit} 'The installed Japanese IME did not produce genuine Hiragana preedit' 15}catch{
  @{romaji=$Romaji;expected=$Preedit;actual=(Value $Control);text=($Control.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern).DocumentRange.GetText(-1));control=$Control.Current.AutomationId}|ConvertTo-Json|Set-Content (Join-Path $run 'preedit-failure.json')
  Capture-Desktop 'preedit-failure';throw
 }
 Capture-Desktop "$script:imeSequence-hiragana-preedit"
 Key 32
 Wait-Until {(Value $Control) -match '[\u3400-\u9fff]'} 'The Japanese IME did not offer a converted candidate' 15
 Capture-Desktop "$script:imeSequence-converted-candidate"
}
function Begin-Rename{
 $name=Control "layer-$layer-name";Focus $name;Key 113
 Wait-Until {(Model).state.layer_tools.rename_layer -eq $layer} 'Layer rename did not open'
 Control "layer-$layer-rename"
}
function Assert-Renaming([string]$Name){
 $view=Model
 if($view.state.layer_tools.rename_layer -ne $layer -or $view.state.layer_tools.editing_layer.label -ne $Name){throw 'An IME candidate key committed or canceled the layer rename'}
}
try{
 Enter-CapyEnvironment
 if(!(Get-WinUserLanguageList | Where-Object {$_.InputMethodTips -match '^0411:'})){throw 'Enable the installed Japanese IME in the private Windows user before running this fixture'}
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Join-Path $env:CAPY_SETTINGS_DIRECTORY 'settings.json'),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log');$null=$review.Handle
 Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'IME review did not prepare' 120
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 Wait-Until {[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null;[CapyRowPointer]::GetForegroundWindow() -eq $review.MainWindowHandle} 'IME review did not own foreground input'
 $initial=Model;$layer=$initial.state.layer_tools.editing_layer.id;$original=$initial.state.layer_tools.editing_layer.label;$gpu=$initial.windows_gpu_generation
 $entry=Begin-Rename
 if([CapyIme]::Language($review.MainWindowHandle,[uint32]$review.Id) -ne 0x411){[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(91),32)}
 Wait-Until {[CapyIme]::Language($review.MainWindowHandle,[uint32]$review.Id) -eq 0x411} 'The owned editor did not activate the Japanese input profile'
 [CapyIme]::Open($review.MainWindowHandle,[uint32]$review.Id)
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17),20)
 Compose $entry 'nihonn' 'にほん';Assert-Renaming $original
 $candidate=Value $entry;Key 13
 Wait-Until {(Value $entry) -eq $candidate} 'Candidate Enter changed the converted text'
 Assert-Renaming $original;Key 13
 Wait-Until {$view=Model;$null -eq $view.state.layer_tools.rename_layer -and $view.state.layer_tools.editing_layer.label -eq $candidate} 'The ordinary Enter after candidate confirmation did not commit rename'
 $entry=Begin-Rename;Compose $entry 'kanji' 'かんじ';Key 27;Assert-Renaming $candidate
 Wait-Until {(Value $entry) -eq 'かんじ'} 'Candidate Escape did not restore Hiragana preedit'
 Key 27;Assert-Renaming $candidate;Key 27
 Wait-Until {$null -eq (Model).state.layer_tools.rename_layer} 'The ordinary Escape after composition did not cancel rename'
 $opacity=Control 'layer-opacity';$before=Model;$value=$before.state.layer_tools.editing_layer.opacity
 Compose $opacity 'nihonn' 'にほん';Key 13
 if($opacity.Current.HelpText){throw 'Candidate Enter ran numeric validation before committing IME text'}
 Key 13
 Wait-Until {$opacity.Current.HelpText -and (Value $opacity) -match '[\u3400-\u9fff]'} 'The ordinary Enter did not retain a refused numeric draft'
 $after=Model
 if($after.state.layer_tools.editing_layer.opacity -ne $value -or $after.state.document_file.revision -ne $before.state.document_file.revision -or $after.windows_gpu_generation -ne $gpu){throw 'Refused IME numeric input changed the document or GPU owner'}
 Key 27;Wait-Until {!$opacity.Current.HelpText} 'Cancel did not clear the numeric refusal'
 $entry=Begin-Rename;Compose $entry 'kanji' 'かんじ';$text=Value $entry
 $bounds=$entry.Current.BoundingRectangle;[CapyRowPointer]::Initialize([uint32]$review.Id)
 [CapyRowPointer]::Down('mouse',[int]($bounds.X+$bounds.Width/2),[int]($bounds.Y+$bounds.Height/2));[CapyRowPointer]::Up()
 Key 13
 Wait-Until {$view=Model;$null -eq $view.state.layer_tools.rename_layer -and $view.state.layer_tools.editing_layer.label -eq $text} 'Pointer confirmation consumed the next ordinary Enter'
 $entry=Begin-Rename;Compose $entry 'nihonn' 'にほん';$text=Value $entry
 Focus (Control 'layer-opacity')
 Wait-Until {$view=Model;$null -eq $view.state.layer_tools.rename_layer -and $view.state.layer_tools.editing_layer.label -eq $text} 'Focus change did not retire the composition and commit the literal name'
 Capture-Desktop 'completed'
 @{theme=$Theme;engine='Microsoft Japanese IME';genuine_hiragana_preedit=$true;converted_candidate=$true;candidate_enter_then_ordinary_enter='passed';candidate_escape_then_ordinary_escape='passed';selected_layer_name='passed';numeric_refusal='passed';pointer_confirmation='passed';focus_change='passed';gpu_owner='retained';evidence=$run}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
 $completed=$true
}finally{
 [CapyRowPointer]::Dispose()
 if($completed -and $review -and !$review.HasExited){& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run}
 Exit-CapyEnvironment
}
