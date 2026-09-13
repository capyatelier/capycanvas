param([Parameter(Mandatory)][string]$Path)
$ErrorActionPreference='Stop'
# MakeAppx uses wall-clock ZIP timestamps even when payload times are fixed.
# Change only those header fields. Preserve compressed blocks, block-map sizes,
# CRCs, file data and all other metadata. Signed packages must never be edited.
$stream=[IO.File]::Open((Resolve-Path -LiteralPath $Path).Path,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$reader=[IO.BinaryReader]::new($stream,[Text.Encoding]::UTF8,$true)
$writer=[IO.BinaryWriter]::new($stream,[Text.Encoding]::UTF8,$true)
try{
    if($stream.Length -lt 22){throw 'Incomplete MSIX ZIP directory.'}
    $stream.Position=$stream.Length-22
    if($reader.ReadUInt32() -ne 0x06054b50){throw 'Expected a MakeAppx archive without a ZIP comment.'}
    $disk=$reader.ReadUInt16();$directoryDisk=$reader.ReadUInt16()
    $onDisk=$reader.ReadUInt16();$count=$reader.ReadUInt16()
    $directorySize=$reader.ReadUInt32();$directory=$reader.ReadUInt32();$comment=$reader.ReadUInt16()
    $directoryEnd=$stream.Length-22
    if($comment -ne 0){throw 'ZIP comments are not supported.'}
    if($count -eq 65535 -or $directory -eq [uint32]::MaxValue -or $directorySize -eq [uint32]::MaxValue){
        $stream.Position=$stream.Length-42
        if($reader.ReadUInt32() -ne 0x07064b50 -or $reader.ReadUInt32() -ne 0){throw 'Invalid ZIP64 locator.'}
        $directoryEnd=[long]$reader.ReadUInt64()
        if($reader.ReadUInt32() -ne 1 -or $directoryEnd -lt 0 -or $directoryEnd+56 -gt $stream.Length-42){throw 'Invalid ZIP64 directory bounds.'}
        $stream.Position=$directoryEnd
        if($reader.ReadUInt32() -ne 0x06064b50){throw 'Invalid ZIP64 directory.'}
        $recordSize=[long]$reader.ReadUInt64()
        if($recordSize -lt 44 -or $directoryEnd+12+$recordSize -ne $stream.Length-42){throw 'Invalid ZIP64 record size.'}
        $stream.Position=$directoryEnd+16
        $disk=$reader.ReadUInt32();$directoryDisk=$reader.ReadUInt32()
        $onDisk=[long]$reader.ReadUInt64();$count=[long]$reader.ReadUInt64()
        $directorySize=[long]$reader.ReadUInt64();$directory=[long]$reader.ReadUInt64()
    }
    $end=[long]$directory+$directorySize
    if($disk -ne 0 -or $directoryDisk -ne 0 -or $count -ne $onDisk -or $count -gt $directorySize/46 -or $end -ne $directoryEnd){throw 'Invalid or split MSIX directory.'}
    $positions=[Collections.Generic.List[long]]::new()
    $cursor=[long]$directory
    for($i=0;$i -lt $count;$i++){
        $stream.Position=$cursor
        if($reader.ReadUInt32() -ne 0x02014b50){throw 'Invalid MSIX central directory.'}
        $stream.Position=$cursor+20
        $compressed=$reader.ReadUInt32();$uncompressed=$reader.ReadUInt32()
        $nameLength=$reader.ReadUInt16();$extraLength=$reader.ReadUInt16();$commentLength=$reader.ReadUInt16()
        $stream.Position=$cursor+42;$local=[long]$reader.ReadUInt32()
        $name=[Text.Encoding]::UTF8.GetString($reader.ReadBytes($nameLength))
        if($name -ieq 'AppxSignature.p7x'){throw 'Refusing to alter a signed MSIX.'}
        $extra=$reader.ReadBytes($extraLength)
        if($local -eq [uint32]::MaxValue){
            $found=$false
            for($offset=0;$offset+4 -le $extra.Length;){
                $tag=[BitConverter]::ToUInt16($extra,$offset);$length=[BitConverter]::ToUInt16($extra,$offset+2)
                $limit=$offset+4+$length
                if($limit -gt $extra.Length){throw 'Invalid ZIP64 entry metadata.'}
                if($tag -eq 1){
                    $value=$offset+4
                    if($uncompressed -eq [uint32]::MaxValue){$value+=8}
                    if($compressed -eq [uint32]::MaxValue){$value+=8}
                    if($value+8 -gt $limit){throw 'Missing ZIP64 local header offset.'}
                    $local=[long][BitConverter]::ToUInt64($extra,$value);$found=$true;break
                }
                $offset=$limit
            }
            if(!$found){throw 'Missing ZIP64 entry metadata.'}
        }
        $next=$cursor+46+$nameLength+$extraLength+$commentLength
        if($next -gt $end -or $local -lt 0 -or $local -ge $directory){throw 'Invalid MSIX entry bounds.'}
        $stream.Position=$local
        if($reader.ReadUInt32() -ne 0x04034b50){throw 'Invalid MSIX local header.'}
        $stream.Position=[long]$local+26
        $localNameLength=$reader.ReadUInt16();$localExtraLength=$reader.ReadUInt16()
        if($localNameLength -ne $nameLength -or [Text.Encoding]::UTF8.GetString($reader.ReadBytes($localNameLength)) -cne $name){throw 'MSIX local and central names differ.'}
        if([long]$local+30+$localNameLength+$localExtraLength -gt $directory){throw 'Invalid MSIX local entry bounds.'}
        $positions.Add($cursor+12);$positions.Add([long]$local+10)
        $cursor=$next
    }
    if($cursor -ne $end){throw 'Incomplete MSIX directory traversal.'}
    # All headers checked before the first mutation: 2000-01-01 00:00 DOS time.
    foreach($position in $positions){$stream.Position=$position;$writer.Write([uint32]0x28210000)}
    $writer.Flush()
}finally{$writer.Dispose();$reader.Dispose();$stream.Dispose()}
