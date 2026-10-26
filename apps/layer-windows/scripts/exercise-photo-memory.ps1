param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateRange(32,16000)][int]$Width=12000,[ValidateRange(32,16000)][int]$Height=8000)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.IO.Compression;
public static class CapyPhotoMemoryPng {
    static void Big(Stream s,uint n){s.WriteByte((byte)(n>>24));s.WriteByte((byte)(n>>16));s.WriteByte((byte)(n>>8));s.WriteByte((byte)n);}
    static void Chunk(Stream s,string name,byte[] data,int count){
        Big(s,(uint)count);uint crc=0xffffffff;
        foreach(byte b in System.Text.Encoding.ASCII.GetBytes(name)){s.WriteByte(b);crc=Update(crc,b);}
        s.Write(data,0,count);for(int i=0;i<count;i++)crc=Update(crc,data[i]);Big(s,crc^0xffffffff);
    }
    static uint Update(uint crc,byte b){crc^=b;for(int k=0;k<8;k++)crc=(crc&1)!=0?(crc>>1)^0xedb88320u:crc>>1;return crc;}
    public static void Write(string path,int width,int height){
        string spool=path+".deflate";uint a=1,b=0;byte[] row=new byte[1+width*3];
        byte[][] colors={new byte[]{220,40,30},new byte[]{30,60,220},new byte[]{30,180,60},new byte[]{220,180,30}};
        try{
            using(var output=File.Create(spool)){
                output.WriteByte(0x78);output.WriteByte(0x9c);
                using(var deflate=new DeflateStream(output,CompressionLevel.Optimal,true)){
                    for(int y=0;y<height;y++){
                        for(int x=0;x<width;x++){var color=colors[(y<height/2?0:2)+(x<width/2?0:1)];Buffer.BlockCopy(color,0,row,1+x*3,3);}
                        foreach(byte value in row){a=(a+value)%65521;b=(b+a)%65521;}
                        deflate.Write(row,0,row.Length);
                    }
                }
                Big(output,(b<<16)|a);
                if(output.Length>16*1024*1024)throw new IOException("Fixture PNG exceeds 16 MiB");
            }
            using(var output=File.Create(path))using(var input=File.OpenRead(spool)){
                output.Write(new byte[]{137,80,78,71,13,10,26,10},0,8);
                using(var header=new MemoryStream()){Big(header,(uint)width);Big(header,(uint)height);header.Write(new byte[]{8,2,0,0,0},0,5);Chunk(output,"IHDR",header.ToArray(),13);}
                Chunk(output,"sRGB",new byte[]{0},1);
                byte[] chunk=new byte[65536];int count;while((count=input.Read(chunk,0,chunk.Length))>0)Chunk(output,"IDAT",chunk,count);
                Chunk(output,"IEND",new byte[0],0);
            }
        }finally{if(File.Exists(spool))File.Delete(spool);}
    }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path $Executable
$helper=Join-Path $directory 'capy-photo-reader.exe'
if(!(Test-Path -LiteralPath $helper)){throw 'Staged photo reader is missing'}
$run=Join-Path $repo ('artifacts/windows/photo-memory/'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$photo=Join-Path $run 'Large quadrants.png'
$monitor=$null;$review=$null;$control=[hashtable]::Synchronized(@{stop=$false})
try{
    [CapyPhotoMemoryPng]::Write($photo,$Width,$Height)
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    [IO.File]::WriteAllText((Settings-File),(@{theme=$Theme;language=@{Explicit='en'}}|ConvertTo-Json -Depth 4))
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Wait-Until {$script:drawing=Owned-DrawingWindow $review;$drawing -and (Model).canvas_ready -and (Model).brush_ready} 'Photo review did not start' 90
    $root=$drawing.Root
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $review.Refresh();$baseline=$review.PrivateMemorySize64
    $monitor=[powershell]::Create();$monitor.Runspace=[runspacefactory]::CreateRunspace();$monitor.Runspace.Open()
    $null=$monitor.AddScript({param($Owner,$Control,$Output)
        $ErrorActionPreference='Stop';$children=@{};$app=Get-Process -Id $Owner;$combined=0L;$appPrivate=0L
        try{
            $Control.ready=$true
            while(!$Control.stop){
                foreach($child in @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$Owner AND Name='capy-photo-reader.exe'" -OperationTimeoutSec 2)){
                    if(!$children.ContainsKey([int]$child.ProcessId)){
                        try{$p=Get-Process -Id $child.ProcessId;$null=$p.Handle;$children[[int]$child.ProcessId]=@{process=$p;path=$child.ExecutablePath;arguments=$child.CommandLine;parent=$child.ParentProcessId;sampled_private=0L;peak_working_set=0L;peak_pagefile=0L}}catch{}
                    }
                }
                $app.Refresh();$current=$app.PrivateMemorySize64;$appPrivate=[Math]::Max($appPrivate,$current)
                foreach($entry in $children.Values){
                    $p=$entry.process;$p.Refresh()
                    if(!$p.HasExited){try{$private=$p.PrivateMemorySize64;$entry.sampled_private=[Math]::Max($entry.sampled_private,$private);$entry.peak_working_set=[Math]::Max($entry.peak_working_set,$p.PeakWorkingSet64);$entry.peak_pagefile=[Math]::Max($entry.peak_pagefile,$p.PeakPagedMemorySize64);$current+=$private}catch{if(!$p.HasExited){throw}}}
                }
                $combined=[Math]::Max($combined,$current);Start-Sleep -Milliseconds 50
            }
        }finally{
            $records=@(foreach($entry in $children.Values){$p=$entry.process;$p.Refresh();@{pid=$p.Id;parent=$entry.parent;path=$entry.path;arguments=$entry.arguments;exited=$p.HasExited;exit_code=$(if($p.HasExited){$p.ExitCode}else{$null});sampled_private=$entry.sampled_private;observed_peak_working_set=$entry.peak_working_set;observed_peak_pagefile=$entry.peak_pagefile};if(!$p.HasExited){$p.Kill();$p.WaitForExit()};$p.Dispose()})
            $app.Refresh();@{children=$records;app_sampled_private=$appPrivate;app_peak_working_set=$app.PeakWorkingSet64;app_peak_pagefile=$app.PeakPagedMemorySize64;sampled_combined_private=$combined}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $Output
            $app.Dispose()
        }
    }).AddArgument($review.Id).AddArgument($control).AddArgument((Join-Path $run 'memory.json'))
    $pending=$monitor.BeginInvoke()
    Wait-Until {$control.ready} 'Photo memory monitor did not start' 10
    Open-Project $photo
    Wait-Until {$m=Model;$tab=@($m.state.tabs|Where-Object active)[0];$tab.width -eq $Width -and $tab.height -eq $Height -and $m.canvas_ready -and $m.brush_ready -and !$m.state.document_file.busy -and !@($m.state.requests).Count} 'Large photo did not become ready' 120
    Wait-Until {
        $camera=(Model).state.camera;$work=$camera.work_area
        $camera.translation[0] -ge $work[0] -and $camera.translation[1] -ge $work[1] -and
        $camera.translation[0]+$Width*$camera.zoom -le $work[0]+$work[2]+1 -and
        $camera.translation[1]+$Height*$camera.zoom -le $work[1]+$work[3]+1
    } 'The opened large photo did not fit its native work area'
    $samples=@();$expected=@(@(220,40,30),@(30,60,220),@(30,180,60),@(220,180,30))
    foreach($index in 0..3){
        $x=$Width*$(if($index%2){.75}else{.25});$y=$Height*$(if($index -ge 2){.75}else{.25})
        $sample=@{rgb=$null}
        Wait-Until {
            $m=Model;$camera=$m.state.camera;$bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
            if($camera.rotation -ne 0 -or $camera.flipped[0] -or $camera.flipped[1]){throw 'Photo samples require an unrotated camera'}
            $sx=$bounds.Width/$camera.viewport[0];$sy=$bounds.Height/$camera.viewport[1]
            $px=[int]($bounds.X+($camera.translation[0]+$x*$camera.zoom)*$sx);$py=[int]($bounds.Y+($camera.translation[1]+$y*$camera.zoom)*$sy)
            if(!$bounds.Contains([double]$px,[double]$py)){throw 'Photo quadrant lies outside the canvas'}
            $bitmap=[Drawing.Bitmap]::new(1,1);$graphics=[Drawing.Graphics]::FromImage($bitmap)
            try{$graphics.CopyFromScreen($px,$py,0,0,$bitmap.Size);$color=$bitmap.GetPixel(0,0);$sample.rgb=@([int]$color.R,[int]$color.G,[int]$color.B)}finally{$graphics.Dispose();$bitmap.Dispose()}
            @((0..2)|Where-Object {[Math]::Abs($sample.rgb[$_]-$expected[$index][$_]) -gt 35}).Count -eq 0
        } "Large photo quadrant $index has incorrect pixels"
        $samples+=@{quadrant=$index;rgb=$sample.rgb;expected=$expected[$index]}
    }
    Assert-RendererHealthy $review.Id $drawing
    $control.stop=$true;$null=$monitor.EndInvoke($pending)
    $memory=Read-Snapshot (Join-Path $run 'memory.json')
    if(!@($memory.children).Count){throw 'No owned photo decoder was observed'}
    foreach($child in $memory.children){
        if($child.path -ine $helper -or !$child.exited -or $child.exit_code -ne 0){throw 'Owned photo decoder path or exit status differs'}
        if($child.arguments -notmatch '\s(\d+)\s+(\d+)\s+(\d+)\s*$'){throw 'Owned decoder budgets were not observed'}
        $source=[long]$Matches[1];$codec=[long]$Matches[2]
        if($source -le 0 -or $codec -le 0){throw 'Owned decoder has no memory admission'}
        $child|Add-Member -NotePropertyName source_budget_bytes -NotePropertyValue $source
        $child|Add-Member -NotePropertyName codec_budget_bytes -NotePropertyValue $codec
        $child|Add-Member -NotePropertyName sampled_private_over_source_and_codec -NotePropertyValue ($child.sampled_private/[double]($source+$codec))
    }
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawing.Handle.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
    $review.Refresh();if(!$review.HasExited -or $review.ExitCode -ne 0){throw 'Photo review did not close with exit zero'}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Photo review stderr requires inspection'}
    @{theme=$Theme;extent=@($Width,$Height);encoded_bytes=(Get-Item $photo).Length;app_baseline_private=$baseline;quadrants=$samples;memory=$memory;scope='Observed child OS peaks can miss exit-time growth; combined private usage is sampled. Photo budgets bound codec admission and compressed source ownership, not total editor memory.';run=$run}|ConvertTo-Json -Depth 8|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{($_|Out-String)+$_.ScriptStackTrace|Set-Content -LiteralPath (Join-Path $run 'failure.txt');throw
}finally{
    $control.stop=$true
    if($monitor){if($pending -and !$pending.IsCompleted){$null=$monitor.EndInvoke($pending)};$monitor.Runspace.Dispose();$monitor.Dispose()}
    if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force;$review.WaitForExit()}
    [CapyRowPointer]::Dispose();Exit-CapyEnvironment
    Remove-Item -LiteralPath $photo -Force -ErrorAction SilentlyContinue
}
