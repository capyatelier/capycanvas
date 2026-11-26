function Write-NativeTestLogTail($Reader,[byte[]]$Bytes,[char[]]$Characters,[switch]$Final) {
    while(($count=$Reader.stream.Read($Bytes,0,$Bytes.Length)) -gt 0) {
        $written=$Reader.decoder.GetChars($Bytes,0,$count,$Characters,0,$false)
        $Reader.output.Write($Characters,0,$written)
    }
    if($Final) {
        $written=$Reader.decoder.GetChars($Bytes,0,0,$Characters,0,$true)
        $Reader.output.Write($Characters,0,$written)
    }
    $Reader.output.Flush()
}

function Invoke-NativeTestProcess([hashtable]$Launch) {
    $process=Start-Process @Launch
    $null=$process.Handle
    $readers=@()
    try {
        $encoding=[Text.UTF8Encoding]::new($false)
        $bytes=[byte[]]::new(8192)
        $characters=[char[]]::new($encoding.GetMaxCharCount($bytes.Length))
        foreach($entry in @(
            @{path=$Launch.RedirectStandardOutput;output=[Console]::Out},
            @{path=$Launch.RedirectStandardError;output=[Console]::Error}
        )) {
            $readers+=@{stream=[IO.File]::Open($entry.path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite);decoder=$encoding.GetDecoder();output=$entry.output}
        }
        do {
            $exited=$process.WaitForExit(500)
            foreach($reader in $readers) {Write-NativeTestLogTail $reader $bytes $characters}
        } while(!$exited)
        $process.WaitForExit()
        foreach($reader in $readers) {Write-NativeTestLogTail $reader $bytes $characters -Final}
        return $process.ExitCode
    } finally {
        foreach($reader in $readers) {$reader.stream.Dispose()}
        $process.Dispose()
    }
}
