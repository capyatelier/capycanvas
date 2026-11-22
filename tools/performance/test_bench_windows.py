import base64
import contextlib
import hashlib
import io
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import bench_windows as bench

ROOT = 'C:\\Users\\hogba\\AppData\\Local\\CapyCanvasAgent\\bench'


def decoded(command):
    return base64.b64decode(command[-1]).decode('utf-16-le')


@unittest.skipUnless(sys.platform == 'win32', 'Requires Windows PowerShell and WindowsBase')
class NativeRunnerTest(unittest.TestCase):
    def run_functions(self, names, script):
        runner = bench.REPO / 'tools/performance/bench-chain-windows.ps1'
        prefix = '$runner=' + bench.quote(runner) + '\n$functions=@(' + ','.join(map(bench.quote, names)) + ')\n'
        source = prefix + r'''
$ErrorActionPreference='Stop'
Add-Type -AssemblyName WindowsBase,UIAutomationClient,UIAutomationTypes
$tokens=$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($runner,[ref]$tokens,[ref]$errors)
if($errors){throw ($errors -join "`n")}
foreach($name in $functions){
 $function=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$false)
 if(!$function){throw "Missing runner function: $name"}
 . ([scriptblock]::Create($function.Extent.Text))
}
''' + script
        command = ['powershell', '-NoProfile', '-NonInteractive', '-EncodedCommand',
                   base64.b64encode(source.encode('utf-16-le')).decode()]
        result = subprocess.run(command, capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_native_obstacles_leave_a_clear_work_area(self):
        result = self.run_functions(('Rect-List', 'Clear-Work-Area'), r'''
$canvas=[Windows.Rect]::new(0,0,100,100)
$docks=@([Windows.Rect]::new(0,0,20,100),[Windows.Rect]::new(80,0,20,100))
$float=[Windows.Rect]::new(40,25,20,50)
$nested=[Windows.Rect]::new(45,30,10,20)
$results=@{
 empty=@(Rect-List (Clear-Work-Area $canvas @()))
 docks=@(Rect-List (Clear-Work-Area $canvas $docks))
 float=@(Rect-List (Clear-Work-Area $canvas ($docks+@($float))))
 nested=@(Rect-List (Clear-Work-Area $canvas ($docks+@($float,$nested,$float))))
 blocked=$false
}
try {Clear-Work-Area $canvas @($canvas)|Out-Null} catch {$results.blocked=$_.Exception.Message -like 'Owned controls leave no usable canvas work area*'}
$results|ConvertTo-Json -Compress
''')
        self.assertEqual(result, {
            'empty': [0, 0, 100, 100], 'docks': [20, 0, 60, 100],
            'float': [20, 0, 60, 25], 'nested': [20, 0, 60, 25], 'blocked': True,
        })

    def test_observe_waits_for_arranged_canvas_and_resets_on_resize(self):
        result = self.run_functions(('Screen-Hash', 'Observe'), r'''
Add-Type -TypeDefinition @'
public static class CapyBenchScreen {
 public static System.Collections.Generic.List<int> X = new System.Collections.Generic.List<int>();
 public static ulong Hash(int x, int y, int width, int height) { X.Add(x); return 1; }
}
'@
$review=[Diagnostics.Process]::GetCurrentProcess()
$script:attempt=0;$script:time=0
function Now {$script:time+=100000000;$script:time}
function Find([string]$Name,[switch]$Visible){
 if($Name -ne 'drawing-canvas' -or !$Visible){throw 'Expected the visible canvas peer'}
 $script:time+=20000000
 $rect=switch($script:attempt){
  1 {throw 'Observed the canvas while readiness was false'}
  2 {[Windows.Rect]::Empty}
  3 {[Windows.Rect]::new([double]::PositiveInfinity,0,30,30)}
  4 {[Windows.Rect]::new(0,0,30,30)}
  default {[Windows.Rect]::new(2,0,30,30)}
 }
 [pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=$rect}}
}
$settled=Observe {$script:attempt++;$script:attempt -gt 1} 'Observation did not settle' 2 -Stable -Changed 0
$null=Screen-Hash ([Windows.Rect]::new(10,0,30,30))
@{settled=$settled;captures=@([CapyBenchScreen]::X);current_x=$script:canvasRect.X}|ConvertTo-Json -Compress
''')
        self.assertEqual(result, {'settled': 580000000, 'captures': [0, 2, 2, 2, 10], 'current_x': 2})

    def test_layers_list_uses_visible_native_route(self):
        result = self.run_functions(('Layers-List',), r'''
function Find([string]$Name,[switch]$Visible){
 if(!$Visible){throw "Expected visible peer: $Name"}
 if($Name -in $script:visible){[pscustomobject]@{Current=[pscustomobject]@{AutomationId=$Name}}}
}
function Invoke-Id([string]$Id){
 $script:activated+=@($Id)
 if($Id -in $script:visible){$script:visible+=@('layer-list')}
}
function Control([string]$Name,[switch]$Arranged){
 if(!$Arranged -or $Name -ne 'layer-list' -or $Name -notin $script:visible){throw "List was not arranged: $Name"}
 $Name
}
$results=@{}
foreach($scenario in 'visible','tab','column','unavailable'){
 $script:activated=@()
 $script:visible=@(switch($scenario){
  'visible' {'layer-list';'panel-tab-layers'}
  'tab' {'panel-tab-layers';'column-icon-layers'}
  'column' {'column-icon-layers'}
 })
 try {$list=Layers-List} catch {
  if($scenario -ne 'unavailable' -or $_.Exception.Message -ne 'The Layers panel has no visible tab or column control'){throw}
  $list=$null
 }
 $results[$scenario]=@{list=$list;activated=$script:activated}
}
$results|ConvertTo-Json -Compress
''')
        self.assertEqual(result, {
            'visible': {'list': 'layer-list', 'activated': []},
            'tab': {'list': 'layer-list', 'activated': ['panel-tab-layers']},
            'column': {'list': 'layer-list', 'activated': ['column-icon-layers']},
            'unavailable': {'list': None, 'activated': []},
        })


class PalindromeTest(unittest.TestCase):
    def test_two_passes_run_a_b_b_a_with_each_first_pass_cold(self):
        self.assertEqual(bench.palindrome(['A', 'B'], 2, False), [('A', 1, True), ('B', 1, True), ('B', 2, False), ('A', 2, False)])

    def test_odd_pass_counts_keep_alternating_direction(self):
        self.assertEqual([version for version, _, _ in bench.palindrome(['A', 'B'], 3, False)], ['A', 'B', 'B', 'A', 'A', 'B'])

    def test_quick_runs_one_warm_pass_per_version(self):
        self.assertEqual(bench.palindrome(['A', 'B', 'C'], 4, True), [('A', 1, False), ('B', 1, False), ('C', 1, False)])


class CommandTest(unittest.TestCase):
    fixtures = {'photo': ROOT + '\\fixtures\\a\\photo.jpg', 'heavy': ROOT + '\\fixtures\\b\\heavy.capy', 'vector': ROOT + '\\fixtures\\c\\vector.capy'}

    def test_remote_paths_stay_under_the_private_agent_root(self):
        self.assertEqual(bench.remote('runs', 'r1', 'src/tools/performance/x.ps1'), ROOT + '\\runs\\r1\\src\\tools\\performance\\x.ps1')
        self.assertEqual(bench.transfer('versions', 'v1.0.15', 'pass-2'), 'AppData/Local/CapyCanvasAgent/bench/versions/v1.0.15/pass-2')

    def test_powershell_command_carries_the_whole_script_encoded(self):
        command = bench.powershell_command('surfacelaptop', "Write-Output 'it''s'")
        self.assertEqual(command[:3], ['ssh', 'surfacelaptop', 'powershell'])
        self.assertEqual(command[-2], '-EncodedCommand')
        self.assertTrue(decoded(command).startswith("$ErrorActionPreference='Stop'"))
        self.assertTrue(decoded(command).endswith("Write-Output 'it''s'"))

    def test_executables_need_unique_labels_and_exe_paths(self):
        self.assertEqual(bench.parse_executables(['v1.0.15=C:\\a\\CapyCanvas.exe']), {'v1.0.15': 'C:\\a\\CapyCanvas.exe'})
        for values in (['C:\\a\\CapyCanvas.exe'], ['v=C:\\a\\notes.txt'], ['v=C:\\a.exe', 'v=C:\\b.exe']):
            with self.subTest(values=values), self.assertRaises(ValueError):
                bench.parse_executables(values)

    def test_invocation_passes_cold_and_steps_only_when_asked(self):
        cold = bench.invocation('r1', 'surfacelaptop', 'v1.0.15', "C:\\o'neil\\CapyCanvas.exe", 1, True, self.fixtures, ['draw.pencil', 'undo'])
        self.assertIn("-Executable 'C:\\o''neil\\CapyCanvas.exe'", cold)
        self.assertIn(f"-Chain '{ROOT}\\runs\\r1\\chain.json' -Pass 1 -Cold -Device 'surfacelaptop' -Version 'v1.0.15'", cold)
        self.assertIn(f"-OutputDirectory '{ROOT}\\versions\\v1.0.15' -Steps 'draw.pencil,undo'", cold)
        self.assertIn(f"& '{ROOT}\\runs\\r1\\src\\tools\\performance\\bench-chain-windows.ps1'", cold)
        self.assertIn(f"$log='{ROOT}\\runs\\r1\\v1.0.15-pass-1'", cold)
        warm = bench.invocation('r1', 'surfacelaptop', 'v1.0.15', 'C:\\b\\CapyCanvas.exe', 2, False, self.fixtures, [])
        self.assertNotIn('-Cold', warm)
        self.assertNotIn('-Steps', warm)

    def test_task_runs_in_the_interactive_session_limited_unless_elevated(self):
        limited, elevated = bench.task_script('CapyBench-r1', 'C:\\x\\a.ps1', False), bench.task_script('CapyBench-r1', 'C:\\x\\a.ps1', True)
        self.assertIn('-LogonType Interactive -RunLevel Limited', limited)
        self.assertIn('-RunLevel Highest', elevated)
        self.assertIn('''-Execute conhost.exe -Argument '"powershell.exe" -NoProfile -Sta -ExecutionPolicy Bypass -File "C:\\x\\a.ps1"\'''', limited)
        self.assertIn('LogonUI', limited)

    def test_cleanup_stops_only_the_launched_executable(self):
        script = bench.cleanup_script('CapyBench-r1', 'C:\\b\\CapyCanvas.exe', 'C:\\r\\pass-1')
        self.assertIn("Unregister-ScheduledTask -TaskName 'CapyBench-r1'", script)
        self.assertIn("'C:\\r\\pass-1\\process-id.txt'", script)
        self.assertIn("$app.Path -eq 'C:\\b\\CapyCanvas.exe'", script)


class RunTest(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory(prefix='capy-bench-windows-test-')
        self.addCleanup(folder.cleanup)
        self.folder = Path(folder.name)
        self.inputs = {name: self.folder / name for name in ('chain.json', 'photo.jpg', 'heavy.capy', 'vector.capy')}
        for name, path in self.inputs.items():
            path.write_text(name)
        self.calls, self.tasks = [], []

    def remote_shell(self, command, timeout=600):
        self.calls.append(command)
        if command[0] == 'scp':
            source, target = command[-2], Path(command[-1])
            if source.startswith('surfacelaptop:AppData/Local/CapyCanvasAgent/bench/versions/'):
                (target / source.rsplit('/', 1)[1]).mkdir(parents=True)
                (target / source.rsplit('/', 1)[1] / 'pass.json').write_text('{}')
            elif source.startswith('surfacelaptop:'):
                target.write_text('log')
            return ''
        script = decoded(command)
        if 'Get-FileHash' in script:
            name = re.search(r"Test-Path -LiteralPath '([^']*)'", script)[1].rsplit('\\', 1)[1]
            return hashlib.sha256(self.inputs[name].read_bytes()).hexdigest().upper()
        if 'Register-ScheduledTask' in script:
            self.tasks.append(script.split('-File "')[1].split('"')[0])
        if 'ConvertTo-Json -Compress' in script:
            return json.dumps({'state': 'Ready', 'code': '0'})
        if script.endswith("\\pass-1'") or script.endswith("\\pass-2'"):
            return 'True'
        return ''

    def test_passes_run_in_order_and_land_in_the_run_layout(self):
        with patch.object(Path, 'home', return_value=self.folder), patch.object(bench, 'run', self.remote_shell), patch.object(bench.time, 'sleep'), \
                contextlib.redirect_stdout(io.StringIO()):
            code = bench.main(['--exe', 'v1.0.15=C:\\a\\CapyCanvas.exe', '--exe', 'g0123456789=C:\\b\\CapyCanvas.exe', '--devices', 'surfacelaptop',
                               '--photo', str(self.inputs['photo.jpg']), '--heavy', str(self.inputs['heavy.capy']),
                               '--vector', str(self.inputs['vector.capy']), '--chain', str(self.inputs['chain.json']), '--run-id', 'r1'])
        self.assertEqual(code, 0)
        self.assertEqual([path.rsplit('\\', 1)[1] for path in self.tasks],
                         ['v1.0.15-pass-1.ps1', 'g0123456789-pass-1.ps1', 'g0123456789-pass-2.ps1', 'v1.0.15-pass-2.ps1'])
        self.assertFalse([call for call in self.calls if call[0] == 'scp' and call[-2].endswith(('.jpg', '.capy'))])
        out = self.folder / 'capy-bench/runs/r1'
        record = json.loads((out / 'run.json').read_text())
        self.assertEqual(record['versions'], ['v1.0.15', 'g0123456789'])
        passes = record['devices']['surfacelaptop']['passes']
        self.assertEqual([(p['version'], p['pass'], p['cold'], p['exit_code']) for p in passes],
                         [('v1.0.15', 1, True, 0), ('g0123456789', 1, True, 0), ('g0123456789', 2, False, 0), ('v1.0.15', 2, False, 0)])
        for p in passes:
            self.assertTrue((out / p['directory'] / 'pass.json').is_file())
            self.assertTrue((out / p['directory'] / 'runner.log').is_file())

    def test_unknown_devices_and_missing_files_stop_before_any_command(self):
        common = ['--exe', 'v=C:\\a\\CapyCanvas.exe', '--photo', str(self.inputs['photo.jpg']), '--heavy', str(self.inputs['heavy.capy']),
                  '--vector', str(self.inputs['vector.capy'])]
        for extra in (['--devices', 'desktop', '--chain', str(self.inputs['chain.json'])], ['--devices', 'surfacepro', '--chain', str(self.folder / 'none.json')]):
            with self.subTest(extra=extra), patch.object(bench, 'run', self.remote_shell), patch('sys.stderr'), self.assertRaises(SystemExit):
                bench.main(common + extra)
        self.assertEqual(self.calls, [])


if __name__ == '__main__':
    unittest.main()
