Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Security.Cryptography;
public static class CapyPaperFixture {
    static void Big(Stream s,uint v){s.WriteByte((byte)(v>>24));s.WriteByte((byte)(v>>16));s.WriteByte((byte)(v>>8));s.WriteByte((byte)v);}
    static uint Crc(byte[] bytes){
        uint crc=0xffffffff;
        foreach(var b in bytes){crc^=b;for(int k=0;k<8;k++)crc=(crc&1)!=0?(crc>>1)^0xEDB88320u:crc>>1;}
        return crc^0xffffffff;
    }
    static void Chunk(Stream s,string type,byte[] data){
        var body=new byte[4+data.Length];System.Text.Encoding.ASCII.GetBytes(type).CopyTo(body,0);data.CopyTo(body,4);
        Big(s,(uint)data.Length);s.Write(body,0,body.Length);Big(s,Crc(body));
    }
    public static string Write(string directory){
        const int side=64;var rows=new byte[side*side];
        for(int y=0;y<side;y++)for(int x=0;x<side;x++)rows[y*side+x]=(byte)(128+90*Math.Sin(x*Math.PI/8)*Math.Cos(y*Math.PI/16));
        var head=new byte[24];BitConverter.GetBytes((uint)side).CopyTo(head,0);BitConverter.GetBytes((uint)side).CopyTo(head,4);
        BitConverter.GetBytes(300.0).CopyTo(head,8);BitConverter.GetBytes(300.0).CopyTo(head,16);
        string id;using(var sha=SHA256.Create()){sha.TransformBlock(head,0,head.Length,null,0);sha.TransformFinalBlock(rows,0,rows.Length);id=BitConverter.ToString(sha.Hash).Replace("-","").ToLowerInvariant();}
        var raw=new MemoryStream();for(int y=0;y<side;y++){raw.WriteByte(0);raw.Write(rows,y*side,side);}
        var filtered=raw.ToArray();uint a=1,b=0;foreach(var v in filtered){a=(a+v)%65521;b=(b+a)%65521;}
        var zlib=new MemoryStream();zlib.WriteByte(0x78);zlib.WriteByte(0x01);zlib.WriteByte(1);
        zlib.WriteByte((byte)filtered.Length);zlib.WriteByte((byte)(filtered.Length>>8));zlib.WriteByte((byte)~filtered.Length);zlib.WriteByte((byte)(~filtered.Length>>8));
        zlib.Write(filtered,0,filtered.Length);Big(zlib,(b<<16)|a);
        using(var png=File.Create(Path.Combine(directory,id+".png"))){
            png.Write(new byte[]{137,80,78,71,13,10,26,10},0,8);
            var header=new MemoryStream();Big(header,side);Big(header,side);header.Write(new byte[]{8,0,0,0,0},0,5);
            var density=new MemoryStream();Big(density,11811);Big(density,11811);density.WriteByte(1);
            Chunk(png,"IHDR",header.ToArray());Chunk(png,"pHYs",density.ToArray());Chunk(png,"IDAT",zlib.ToArray());Chunk(png,"IEND",new byte[0]);
        }
        var preview=new byte[96*96];for(int i=0;i<preview.Length;i++)preview[i]=rows[(i/96)%side*side+(i%96)%side];
        File.WriteAllBytes(Path.Combine(directory,id+".preview"),preview);
        return id;
    }
}
'@
function Seed-PaperLibrary{
    $papers=Join-Path $env:CAPY_STORAGE_DIR 'data/papers';[IO.Directory]::CreateDirectory($papers)|Out-Null
    $script:TextureLibraryId=[CapyPaperFixture]::Write($papers)
    [IO.File]::WriteAllText((Join-Path $papers 'index.json'),(@{papers=@(@{id=$TextureLibraryId;name='Fixture scan'})}|ConvertTo-Json -Depth 4 -Compress))
}
function Texture-Visible([int]$Index,$Edit,[double]$Scale){
    $tile=(Control "texture-tile-$Index" -Arranged).Current;$cancel=(Control 'texture-cancel' -Arranged).Current.BoundingRectangle;$r=$tile.BoundingRectangle
    !$tile.IsOffscreen -and $r.Top -ge $Edit.top-1 -and $r.Bottom -le $cancel.Top-17*$Scale+1
}
function Texture-Paper{@((Model).state.layers|Where-Object {$_.paper})[-1]}
function Texture-Control{@((Model).state.layer_properties.controls|Where-Object {$_.paper})[0]}
function Texture-Tiles{$control=Texture-Control;if($control){,@($control.paper.tiles)}else{,@()}}
function Texture-Open([string]$Id){
    Invoke $Id
    Wait-Until {$apply=Find 'texture-apply';$apply -and !$apply.Current.IsOffscreen -and (Find 'texture-tile-0')} "Choose Texture did not open from $Id"
}
function Texture-Click([string]$Id){
    $b=(Control $Id -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::Down('mouse',[int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2));Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()
    Wait-Until {$apply=Find 'texture-apply';$apply -and !$apply.Current.IsOffscreen -and (Find 'texture-tile-0')} "Choose Texture did not open from a click on $Id"
}
function Texture-Closed([string]$Message){Wait-Until {!(Find 'texture-apply')} $Message}
function Texture-Pending{
    $grid=Find 'texture-grid' -Visible
    if(!$grid){return -1}
    $selected=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ItemStatusProperty,'Selected'))
    foreach($tile in $grid.FindAll([System.Windows.Automation.TreeScope]::Descendants,$selected)){
        if($tile.Current.AutomationId -match '^texture-tile-([0-9]+)$'){return [int]$Matches[1]}
    }
    -1
}
function Texture-Ready{
    Wait-Until {$tiles=Texture-Tiles;$tiles.Count -and !@(0..($tiles.Count-1)|Where-Object {$tiles[$_].id -and (Find "texture-preview-$_").Current.ItemStatus -ne 'Ready'}).Count} 'Texture previews did not load' 30
}
function Texture-Span([string]$Prefix,$Expected){
    $frame=Control "$Prefix-dialog-frame" -Arranged
    $null=Control "$Prefix-title" -Within $frame
    $bounds=$frame.Current.BoundingRectangle;$window=$root.Current.BoundingRectangle
    $apply=(Control "$Prefix-apply" -Within $frame -Arranged).Current.BoundingRectangle
    if($frame.Current.ClassName -eq 'Popup' -or !$bounds.Contains($apply) -or $bounds.Width -ge $window.Width -or $bounds.Height -ge $window.Height){throw 'Picker geometry requires the dialog frame inside its owned window'}
    $span=[ordered]@{top=$bounds.Top;bottom=$bounds.Bottom;right=$bounds.Right;width=$bounds.Width;height=$bounds.Height;button=$apply.Height;class=$frame.Current.ClassName}
    if($Expected){
        if([Math]::Abs($span.width-$Expected.width) -gt 1 -or [Math]::Abs($span.right-$Expected.right) -gt 1){throw 'Choose Texture does not retain Edit Color width and right edge'}
        $scroll=$frame.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'ScrollViewer'))
        if(!$scroll){throw 'Choose Texture has no native scroll viewport'}
        $viewport=$scroll.Current.BoundingRectangle
        $tiles=@(for($i=0;$i -lt (Texture-Tiles).Count;$i++){$tile=Find "texture-tile-$i" -Within $frame;if($tile -and $tile.Current.BoundingRectangle.Height -gt 0){[pscustomobject]@{id=$i;bounds=$tile.Current.BoundingRectangle}}})
        $rows=@($tiles|Group-Object {$_.bounds.Top}|Sort-Object {[double]$_.Name})
        $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
        $rowEvidence=@(for($row=0;$row -lt $rows.Count;$row++){
            $members=$rows[$row].Group
            $naturalHeight=if($row+1 -lt $rows.Count){[double]$rows[$row+1].Name-[double]$rows[$row].Name-14*$scale}else{($rows[$row-1].Group|ForEach-Object {$_.bounds.Height}|Measure-Object -Maximum).Maximum}
            $fullHeight=($members|ForEach-Object {$_.bounds.Height}|Measure-Object -Maximum).Maximum
            [pscustomobject]@{top=[double]$rows[$row].Name;natural_height=$naturalHeight;observed_height=$fullHeight;tiles=@($members);full=([Math]::Abs($fullHeight-$naturalHeight) -le 1 -and !@($members|Where-Object {!$viewport.Contains($_.bounds)}).Count)}
        })
        @{viewport=$viewport;rows=$rowEvidence}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run "texture-row-geometry-$((Model).state.theme).json")
        $complete=@($rowEvidence|Where-Object full)
        if($complete.Count -lt 3){throw "Choose Texture does not show three full rows including captions: $($complete.Count)"}
        $span.full_rows=$complete.Count;$span.viewport=$viewport
    }
    $span
}
function Texture-ScreenCapture([string]$Name,$Rects){
    $left=[int](($Rects|ForEach-Object Left|Measure-Object -Minimum).Minimum-24);$top=[int](($Rects|ForEach-Object Top|Measure-Object -Minimum).Minimum-24)
    $right=[int](($Rects|ForEach-Object Right|Measure-Object -Maximum).Maximum+24);$bottom=[int](($Rects|ForEach-Object Bottom|Measure-Object -Maximum).Maximum+24)
    $bitmap=[Drawing.Bitmap]::new($right-$left,$bottom-$top);$graphics=[Drawing.Graphics]::FromImage($bitmap)
    try{$graphics.CopyFromScreen($left,$top,0,0,$bitmap.Size);$bitmap.Save((Join-Path $run "$Name.png"),[Drawing.Imaging.ImageFormat]::Png)}finally{$graphics.Dispose();$bitmap.Dispose()}
    (Model)|ConvertTo-Json -Depth 80|Set-Content (Join-Path $run "$Name-model.json")
}
function Texture-Menu([int]$Index,[string]$Op){
    $target=Control "texture-tile-$Index";$scroll=$null
    if($target.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}else{$target.SetFocus()}
    $tile=(Control "texture-tile-$Index" -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::RightClick([int]($tile.X+$tile.Width/2),[int]($tile.Y+$tile.Width/3))
    Invoke "paper-$Op-$Index"
}
function Test-PaperTexture{
    Relationship-Layers
    $theme=(Model).state.theme;$paper=(Texture-Paper).id;$row=Relationship-Layer $paper
    if(!$row.paper -or !$row.paper.editable){throw 'The Paper row did not publish an editable texture'}
    if($row.paper.id -eq $TextureLibraryId){
        Relationship-Reveal "layer-$paper-name";Texture-Open "layer-texture-$paper"
        Invoke 'texture-tile-0';Wait-Until {(Texture-Pending) -eq 0} 'Clicking a tile did not move the pending texture'
        Invoke 'texture-apply';Texture-Closed 'Use Texture did not close Choose Texture'
        Wait-Until {$shown=(Relationship-Layer $paper).paper.id;$shown -and $shown -ne $TextureLibraryId} 'Use Texture did not restore a built-in paper'
        $row=Relationship-Layer $paper
    }
    $original=$row.paper.id
    Relationship-Reveal "layer-$paper-name"
    $texture=Control "layer-texture-$paper" -Arranged
    if($texture.Current.Name -ne 'Choose Texture' -or $texture.Current.HelpText -ne $row.paper.label){throw 'The texture thumbnail name or description differs from shared state'}
    Wait-Until {(Find "layer-texture-$paper-preview").Current.ItemStatus -eq 'Ready'} 'The row texture preview did not load' 30
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $t=$texture.Current.BoundingRectangle;$c=(Control "layer-$paper-content" -Arranged).Current.BoundingRectangle;$n=(Control "layer-$paper-name" -Arranged).Current.BoundingRectangle
    if([Math]::Abs($t.Width/$scale-30) -gt .5 -or $t.Left -lt $c.Right -or $n.Left -lt $t.Right){throw 'The texture thumbnail is not a 30-DIP tile between the content thumbnail and the name'}
    Capture "texture-row-$theme" -WithModel
    Invoke "layer-$paper-content"
    Wait-Until {$apply=Find 'edit-color-apply';$apply -and !$apply.Current.IsOffscreen} 'Edit Color did not open from the Paper'
    $edit=Texture-Span 'edit-color'
    Invoke 'edit-color-cancel';Wait-Until {!(Find 'edit-color-apply')} 'Edit Color did not close'

    Texture-Click "layer-texture-$paper"
    Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $paper -and (Texture-Control)} 'The texture thumbnail did not select the Paper'
    Texture-Ready;$revision=(Model).state.document_file.revision
    $tiles=Texture-Tiles;$current=[int](Texture-Control).value.value
    if((Texture-Pending) -ne $current){throw 'Choose Texture did not start on the current texture'}
    $span=Texture-Span 'texture' $edit
    $preview=(Control "texture-preview-$current" -Arranged).Current.BoundingRectangle
    [ordered]@{edit_color=$edit;texture=$span;preview=$preview;scale=$scale}|ConvertTo-Json|Set-Content (Join-Path $run "texture-dialog-geometry-$theme.json")
    if([Math]::Abs($span.button/$scale-34) -gt .5 -or [Math]::Abs($preview.Width/$scale-96) -gt .5){throw 'Footer buttons or texture previews are not at their native size'}
    Capture "texture-dialog-$theme" -WithModel
    Wait-Until {(Control "texture-tile-$current").Current.HasKeyboardFocus} 'The pending tile did not take keyboard focus'
    [CapyLayersCapture]::SetForegroundWindow($drawingWindow)|Out-Null
    [CapyRowPointer]::Key([uint32]$review.Id,$(if($current -eq 0){0x27}else{0x25}))
    $focused=$(if($current -eq 0){1}else{$current-1})
    Wait-Until {(Control "texture-tile-$focused").Current.HasKeyboardFocus} 'Arrow keys did not move between tiles'
    [CapyRowPointer]::Key([uint32]$review.Id,0x20)
    Wait-Until {(Texture-Pending) -eq $focused} 'Space did not choose the focused tile'
    if((Model).state.document_file.revision -ne $revision){throw 'Choosing a tile changed the drawing'}
    [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Texture-Closed 'Escape did not close Choose Texture'
    if((Model).state.document_file.revision -ne $revision -or (Relationship-Layer $paper).paper.id -ne $original){throw 'Escape changed the texture'}

    if(!@((Model).layout.groups|Where-Object {$_.active -eq 'properties'}).Count){Invoke 'Properties' -Name}
    $button=Control 'property-texture' -Arranged
    Wait-Until {(Find 'property-texture-preview').Current.ItemStatus -eq 'Ready'} 'The Texture button preview did not load' 30
    if($button.Current.HelpText -ne $tiles[$current].label){throw 'The Texture button does not show the current texture'}
    Capture "texture-properties-$theme" -WithModel
    $pick=$(if($current -eq 0){1}else{0})
    Texture-Open 'property-texture'
    if((Texture-Pending) -ne $current){throw 'Choose Texture did not start on the current texture from Properties'}
    Invoke "texture-tile-$pick";Wait-Until {(Texture-Pending) -eq $pick} 'Clicking a tile did not move the pending texture'
    Invoke 'texture-cancel';Texture-Closed 'Cancel did not close Choose Texture'
    if((Model).state.document_file.revision -ne $revision -or (Relationship-Layer $paper).paper.id -ne $original){throw 'Cancel changed the texture'}
    Texture-Open 'property-texture'
    if((Texture-Pending) -ne $current){throw 'Reopening Choose Texture did not reset the pending texture'}
    Invoke "texture-tile-$pick";Wait-Until {(Texture-Pending) -eq $pick} 'Clicking a tile did not move the pending texture'
    Texture-Ready;Capture "texture-dialog-chosen-$theme" -WithModel
    Invoke 'texture-apply';Texture-Closed 'Use Texture did not close Choose Texture'
    Wait-Until {(Relationship-Layer $paper).paper.id -eq $tiles[$pick].id -and [int](Texture-Control).value.value -eq $pick} 'Use Texture did not apply the chosen texture'
    Wait-Until {(Control 'property-texture').Current.HelpText -eq $tiles[$pick].label} 'The Texture button did not follow the chosen texture'
    Capture "texture-applied-$theme" -WithModel
    Relationship-Undo
    Wait-Until {(Relationship-Layer $paper).paper.id -eq $original -and (Control 'property-texture').Current.HelpText -eq $tiles[$current].label} 'One Undo did not restore the texture and its Properties description'

    Relationship-Layers
    Texture-Open "layer-texture-$paper"
    Invoke "texture-tile-$pick";Wait-Until {(Texture-Pending) -eq $pick} 'Clicking a tile did not move the pending texture'
    Invoke 'texture-apply';Texture-Closed 'Use Texture did not close Choose Texture from the row'
    Wait-Until {(Relationship-Layer $paper).paper.id -eq $tiles[$pick].id -and (Find "layer-texture-$paper").Current.HelpText -eq $tiles[$pick].label} 'Use Texture from the row did not apply the texture'
    Relationship-Undo
    Wait-Until {(Relationship-Layer $paper).paper.id -eq $original} 'One Undo did not restore the texture from the row'
    Texture-Open "layer-texture-$paper"
    Invoke "texture-tile-$pick";Wait-Until {(Texture-Pending) -eq $pick} 'Clicking a tile did not move the pending texture'
    Toggle-Flag 'layer-lock';Wait-Until {(Relationship-Layer $paper).locked} 'Locking under Choose Texture did not publish'
    Invoke 'texture-apply';Texture-Closed 'Use Texture did not close after the Paper was locked'
    Wait-Until {(Model).state.notice.text -eq 'Could not complete this action'} 'A refused Use Texture did not report the shared failure'
    if((Relationship-Layer $paper).paper.id -ne $original){throw 'A refused Use Texture changed the texture'}
    Capture "texture-refused-$theme" -WithModel
    Toggle-Flag 'layer-lock';Wait-Until {!(Relationship-Layer $paper).locked} 'Unlocking did not publish'

    $library=-1;$tiles=Texture-Tiles;for($i=0;$i -lt $tiles.Count;$i++){if($tiles[$i].id -eq $TextureLibraryId){$library=$i}}
    if($library -ge 0){
        Texture-Open "layer-texture-$paper"
        Invoke "texture-tile-$library";Wait-Until {(Texture-Pending) -eq $library} 'Clicking the library tile did not move the pending texture'
        Texture-Menu $library 'rename_paper'
        Wait-Until {!(Find 'texture-apply') -and (Find 'paper-rename-name')} 'Rename did not replace Choose Texture with the rename prompt'
        Capture "texture-rename-$theme"
        $name="Renamed $theme scan"
        (Control 'paper-rename-name').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($name)
        Invoke 'PrimaryButton'
        Wait-Until {$apply=Find 'texture-apply';$apply -and !$apply.Current.IsOffscreen -and (Find "texture-tile-$library").Current.Name -eq $name -and (Texture-Pending) -eq $library} 'Choose Texture did not return with the renamed pending paper'
        Capture "texture-renamed-$theme" -WithModel
        Invoke 'texture-apply';Texture-Closed 'Use Texture did not close Choose Texture for the library paper'
        Wait-Until {(Relationship-Layer $paper).paper.id -eq $TextureLibraryId -and (Relationship-Layer $paper).paper.label -eq $name} 'Use Texture did not apply the library paper'
        Texture-Open "layer-texture-$paper"
        if((Texture-Pending) -ne $library){throw 'Choose Texture did not start on the library paper'}
        Wait-Until {Texture-Visible $library $edit $scale} 'Choose Texture did not scroll the pending library paper into view'
        Texture-Ready;$null=Texture-Span 'texture' $edit;Capture "texture-dialog-library-$theme" -WithModel
        Invoke 'texture-cancel';Texture-Closed 'Cancel did not close Choose Texture on the library paper'
        Relationship-Undo
        Wait-Until {(Relationship-Layer $paper).paper.id -eq $original} 'One Undo did not restore the texture after the library paper'
        Texture-Open "layer-texture-$paper"
        Invoke "texture-tile-$library";Wait-Until {(Texture-Pending) -eq $library} 'Clicking the library tile did not move the pending texture'
        if($theme -eq 'light'){
            Texture-Menu $library 'delete_paper'
            Wait-Until {!(Find "texture-tile-$library") -and (Texture-Pending) -eq $current} 'Deleting the pending paper did not fall back to the current texture'
        }
        Invoke 'texture-cancel';Texture-Closed 'Cancel did not close Choose Texture after the library edits'
        Relationship-Layers
        if((Relationship-Layer $paper).paper.id -ne $original){throw 'Library edits changed the texture'}
    }elseif($theme -ne 'light'){throw 'The seeded library paper is missing'}

    Invoke "layer-$paper-name";Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $paper} 'Paper selection did not publish'
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Select';Invoke 'select_all'
    Wait-Until {((Model).state.commands|Where-Object id -eq 'deselect').enabled} 'Select All did not create a selection'
    $maskCommand=(Model).state.commands|Where-Object id -eq 'mask_selection'
    if($maskCommand.enabled -or $maskCommand.disabled_reason -ne "Paper can't have a mask"){throw 'Mask selection did not refuse the Paper'}
    if((Control 'layer-add-mask' -Arranged).Current.IsEnabled){throw 'Add mask stayed enabled for the Paper'}
    Focus "layer-$paper-name";[CapyRowPointer]::Key([uint32]$review.Id,0x5D)
    (Control 'Mask' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $shown=@((Control "layer-row-$paper" -Arranged).Current.BoundingRectangle,(Control 'Mask' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).Current.BoundingRectangle)
    foreach($label in @('Add mask','Mask: reveal selection','Mask: hide selection','Paste mask')){
        $item=Control $label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged
        if($item.Current.IsEnabled){throw "$label stayed enabled for the Paper"}
        $shown+=$item.Current.BoundingRectangle
    }
    Texture-ScreenCapture "texture-row-mask-menu-$theme" $shown
    [CapyRowPointer]::Key([uint32]$review.Id,0x1B);[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Find 'Add mask' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'The Paper row menu did not close'
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Select';Invoke 'deselect'
    Wait-Until {!((Model).state.commands|Where-Object id -eq 'deselect').enabled} 'Deselect did not clear the selection'
    if((Relationship-Layer $paper).has_mask){throw 'The Paper gained a mask'}
    Toggle-Flag 'layer-lock';Wait-Until {(Relationship-Layer $paper).locked -and !(Relationship-Layer $paper).paper.editable} 'Locking did not publish a read-only texture'
    Invoke "layer-texture-$paper";Start-Sleep -Milliseconds 800
    if(Find 'texture-apply'){throw 'A locked Paper opened Choose Texture'}
    Toggle-Flag 'layer-lock';Wait-Until {(Relationship-Layer $paper).paper.editable} 'Unlocking did not restore the texture'
    if((Relationship-Layer $paper).paper.id -ne $original){throw 'The texture journey left the texture changed'}
}
function Paper-Scale{[double]@((Model).state.layer_properties.controls|Where-Object key -eq 'scale')[0].value.value}
function Relief-Comparison([int[]]$Before,[int[]]$After){
    if(!$Before.Count -or $Before.Count -ne $After.Count){throw 'Paper relief sample geometry changed'}
    $changed=0;$minimum=[int]::MaxValue;$maximum=[int]::MinValue
    for($i=0;$i -lt $Before.Count;$i++){
        if($Before[$i] -ne $After[$i]){$changed++}
        $a=[Drawing.Color]::FromArgb($Before[$i]);$b=[Drawing.Color]::FromArgb($After[$i])
        $delta=[int]$b.R+$b.G+$b.B-$a.R-$a.G-$a.B
        $minimum=[Math]::Min($minimum,$delta);$maximum=[Math]::Max($maximum,$delta)
    }
    @{changed=$changed;samples=$Before.Count;textured=($changed*3 -gt $Before.Count -and $minimum -lt $maximum)}
}
function Relief-Samples([string]$Name){
    $model=Model;$camera=$model.state.camera;$tab=@($model.state.tabs|Where-Object active)[0]
    $c=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$k=$c.Width/$camera.viewport[0];$w=[int]($tab.width*$camera.zoom*$k*.3);$h=[int]($tab.height*$camera.zoom*$k*.3)
    $bitmap=[Drawing.Bitmap]::new($w,$h);$graphics=[Drawing.Graphics]::FromImage($bitmap)
    try{
        $graphics.CopyFromScreen([int]($c.X+($tab.width*.35*$camera.zoom+$camera.translation[0])*$k),[int]($c.Y+($tab.height*.35*$camera.zoom+$camera.translation[1])*$k),0,0,$bitmap.Size);$bitmap.Save((Join-Path $run "$Name.png"),[Drawing.Imaging.ImageFormat]::Png)
        $lock=$bitmap.LockBits([Drawing.Rectangle]::new(0,0,$w,$h),[Drawing.Imaging.ImageLockMode]::ReadOnly,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try{$stride=$lock.Stride;$bytes=[byte[]]::new($stride*$h);[Runtime.InteropServices.Marshal]::Copy($lock.Scan0,$bytes,0,$bytes.Length)}finally{$bitmap.UnlockBits($lock)}
    }finally{$graphics.Dispose();$bitmap.Dispose()}
    $samples=[Collections.Generic.List[int]]::new()
    for($y=0;$y -lt $h;$y+=3){for($i=$y*$stride;$i -lt $y*$stride+$w*4;$i+=12){$samples.Add([BitConverter]::ToInt32($bytes,$i))}}
    ,$samples.ToArray()
}
function Paper-ScaleRelief{
    $theme=(Model).state.theme;$paper=(Texture-Paper).id
    Relationship-Layers;Relationship-Reveal "layer-$paper-name";Invoke "layer-$paper-name"
    Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $paper} 'Paper selection did not publish before Scale'
    if(!@((Model).layout.groups|Where-Object {$_.active -eq 'properties'}).Count){Invoke 'Properties' -Name}
    $originalRelief=Lighting-Value 'relief'
    if($originalRelief -ne 0){Lighting-Relief '0' {(Lighting-Value 'relief') -eq 0}}
    $track=(Control 'property-scale-slider' -Arranged).Current.BoundingRectangle;$y=[int]($track.Y+$track.Height/2)
    $relief=[ordered]@{original=(Paper-Scale);held=0.;scale=0.;flat=$null;dragging=$null;released=$null};$baseline=@{pixels=@()}
    try{
        [CapyLayersCapture]::SetForegroundWindow($drawingWindow)|Out-Null;[CapyRowPointer]::Hover([int]($track.X+$track.Width*.5),$y)
        Wait-Until {$pixels=Relief-Samples "scale-relief-flat-$theme";$settled=$baseline.pixels.Count -eq $pixels.Count -and (Relief-Comparison $baseline.pixels $pixels).changed -eq 0;$baseline.pixels=$pixels;$relief.flat=@{samples=$pixels.Count};$settled} 'The canvas did not settle before the Scale drag'
        [CapyRowPointer]::Down('mouse',[int]($track.X+$track.Width*.5),$y)
        for($i=1;$i -le 12;$i++){[CapyRowPointer]::Move([int]($track.X+$track.Width*(.5+.03*$i)),$y);Start-Sleep -Milliseconds 30}
        Wait-Until {$relief.held=Paper-Scale;$relief.dragging=Relief-Comparison $baseline.pixels (Relief-Samples "scale-relief-dragging-$theme");$relief.dragging.textured} 'Holding the Scale slider did not shade the paper relief'
        [CapyRowPointer]::Up();$clock=[Diagnostics.Stopwatch]::StartNew()
        Wait-Until {$relief.scale=Paper-Scale;$relief.scale -gt 1.5*$relief.original} 'Releasing the Scale slider did not grow the paper'
        Wait-Until {$relief.released=Relief-Comparison $baseline.pixels (Relief-Samples "scale-relief-released-$theme");$relief.released.changed -eq 0} 'The paper relief did not fade after the Scale drag' (3*$script:CapyWaitSeconds)
        $relief.fade=$clock.Elapsed.TotalSeconds
    }finally{$relief|ConvertTo-Json|Set-Content (Join-Path $run "scale-relief-$theme.json")}
    Relationship-Undo
    Wait-Until {(Paper-Scale) -eq $relief.original -and (Lighting-Value 'relief') -eq 0} 'One Undo did not restore the Paper Scale'
    if($originalRelief -ne 0){
        Relationship-Undo
        Wait-Until {(Paper-Scale) -eq $relief.original -and (Lighting-Value 'relief') -eq $originalRelief} 'One Undo did not restore Texture visibility after the Paper Scale check'
    }
}
function Lighting-Property([string]$Key){@((Model).state.layer_properties.controls|Where-Object key -eq $Key)[0]}
function Lighting-Value([string]$Key){(Lighting-Property $Key).value.value}
function Lighting-Reveal{
    $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$node=$walker.GetParent((Control 'property-light-dial'));$scroll=$null
    while($node){if($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll) -and $scroll.Current.VerticallyScrollable){break};$scroll=$null;$node=$walker.GetParent($node)}
    if($scroll){$scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,100)}
    foreach($id in @('property-relief-slider','property-light-dial','property-through_paint-slider')){$null=Control $id -Arranged}
}
function Lighting-Point([double]$Angle,[double]$Radii){
    $b=(Control 'property-light-dial' -Arranged).Current.BoundingRectangle;$radius=$b.Width/2;$a=$Angle*[Math]::PI/180
    @([int][Math]::Round($b.X+$radius+[Math]::Cos($a)*$radius*$Radii),[int][Math]::Round($b.Y+$b.Height/2+[Math]::Sin($a)*$radius*$Radii))
}
function Lighting-Dot([string]$Name,[double]$Angle){
    $b=(Control 'property-light-dial' -Arranged).Current.BoundingRectangle
    $origin=[CapyRowPointer+Point]::new();if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'The Light dial capture origin is unavailable'}
    $bitmap=[Drawing.Bitmap]::new((Join-Path $run "$Name.png"))
    try{
        $r=$b.Width/2;$x=$b.X+$r-$origin.x;$y=$b.Y+$r-$origin.y;$a=$Angle*[Math]::PI/180
        $dot=$bitmap.GetPixel([int]($x+[Math]::Cos($a)*$r*.55),[int]($y+[Math]::Sin($a)*$r*.55));$face=$bitmap.GetPixel([int]($x-[Math]::Cos($a)*$r*.55),[int]($y-[Math]::Sin($a)*$r*.55))
        @{angle=$Angle;dot=$dot.ToArgb();face=$face.ToArgb();bounds=$b}|ConvertTo-Json|Set-Content (Join-Path $run "$Name-dot.json")
        if([Math]::Abs($dot.GetBrightness()-$face.GetBrightness()) -lt .15){throw "The Light dial dot does not point toward $Angle degrees: dot=$dot face=$face"}
    }finally{$bitmap.Dispose()}
}
function Lighting-Relief([string]$Text,[scriptblock]$Published){
    Edit 'property-relief' $Text;[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    Wait-Until $Published "Texture visibility $Text did not publish"
}
function Test-PaperLighting{
    $theme=(Model).state.theme;$paper=(Texture-Paper).id
    Relationship-Layers;Relationship-Reveal "layer-$paper-name";Invoke "layer-$paper-name"
    Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $paper} 'Paper selection did not publish'
    if(!@((Model).layout.groups|Where-Object {$_.active -eq 'properties'}).Count){Invoke 'Properties' -Name}
    Wait-Until {(Lighting-Property 'light').kind.numeric.kind -eq 'dial'} 'Paper Properties did not publish the Light dial'
    Lighting-Reveal
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    if((Lighting-Value 'relief') -ne 0){Lighting-Relief '0' {(Lighting-Value 'relief') -eq 0}}
    if((Lighting-Value 'relief') -ne 0 -or !(Lighting-Property 'light').disabled -or !(Lighting-Property 'through_paint').disabled){throw 'Light and Shows through paint were not disabled while Texture visibility is 0'}
    $dial=Control 'property-light-dial' -Arranged
    if($dial.Current.IsEnabled -or (Control 'property-through_paint-slider').Current.IsEnabled -or (Control 'property-light').Current.IsEnabled){throw 'Disabled lighting controls still accept input'}
    $revision=(Model).state.document_file.revision;$light=Lighting-Value 'light'
    $at=Lighting-Point 180 .84;[CapyRowPointer]::Down('mouse',$at[0],$at[1]);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 400
    if((Model).state.document_file.revision -ne $revision -or (Lighting-Value 'light') -ne $light){throw 'A press on the disabled Light dial changed the drawing'}
    Capture "paper-lighting-disabled-$theme" -Composed -WithModel
    Lighting-Relief '100' {(Lighting-Value 'relief') -eq 100 -and !(Lighting-Property 'light').disabled -and !(Lighting-Property 'through_paint').disabled -and (Control 'property-light-dial').Current.IsEnabled}
    (Control 'drawing-canvas').SetFocus();Lighting-Reveal
    $dial=Control 'property-light-dial' -Arranged;$d=$dial.Current.BoundingRectangle
    $track=(Control 'property-relief-slider' -Arranged).Current.BoundingRectangle
    $row=(Control 'number-root-property-light' -Arranged).Current.BoundingRectangle;$slider=(Control 'number-root-property-relief' -Arranged).Current.BoundingRectangle
    $value=(Control 'property-light' -Arranged).Current.BoundingRectangle
    $arrangedDial=Native-Bounds 'property-light-dial' $scale
    $arrangedRow=Native-Bounds 'number-root-property-light' $scale;$arrangedSlider=Native-Bounds 'number-root-property-relief' $scale
    [ordered]@{dial=$d;track=$track;row=$row;slider_row=$slider;value=$value;arranged_dial=$arrangedDial;arranged_row=$arrangedRow;arranged_slider_row=$arrangedSlider;scale=$scale}|ConvertTo-Json|Set-Content (Join-Path $run "paper-lighting-geometry-$theme.json")
    if([Math]::Abs($arrangedDial.Width/$scale-28) -gt .5 -or [Math]::Abs($arrangedDial.Height/$scale-28) -gt .5){throw "The Light dial is not 28 DIP: $arrangedDial"}
    if([Math]::Abs($d.Right-$track.Right) -gt 1){throw "The Light dial does not end where the slider tracks end: $($d.Right) vs $($track.Right)"}
    if([Math]::Abs($arrangedRow.Height-$arrangedSlider.Height) -gt 1 -or [Math]::Abs($arrangedRow.Height/$scale-36) -gt .5){throw "The Light row is not a slider row's height: $($arrangedRow.Height) vs $($arrangedSlider.Height)"}
    if([Math]::Abs($d.Top+$d.Height/2-($row.Top+$row.Height/2)) -gt 1){throw 'The Light dial is not centered in its row'}
    if([Math]::Abs($value.Right-$row.Right) -gt 1 -or [Math]::Abs($value.Bottom-$row.Bottom) -gt 1 -or $value.Left -lt $d.Right){throw 'The Light value is not at the bottom right beside the dial'}
    $range=$dial.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current
    $text=$dial.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
    if($range.Minimum -ne 0 -or $range.Maximum -ne 360 -or $range.Value -ne (Lighting-Value 'light') -or $text -ne (Control 'property-light').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value){throw "The Light dial range or value text differs: $($range.Value) $text"}
    Capture "paper-lighting-$theme" -Composed -WithModel;Lighting-Dot "paper-lighting-$theme" (Lighting-Value 'light')
    foreach($device in @('mouse','pen','touch')){
        $before=Lighting-Value 'light'
        $at=Lighting-Point 180 .84;[CapyRowPointer]::Down($device,$at[0],$at[1])
        Wait-Until {(Lighting-Value 'light') -eq 180} "A $device press on the dial's left edge did not light from the left"
        foreach($step in 1..4){$at=Lighting-Point (180+22.5*$step) .84;[CapyRowPointer]::Move($at[0],$at[1]);Start-Sleep -Milliseconds 30}
        Wait-Until {(Lighting-Value 'light') -eq 270} "A $device drag to the dial's top did not light from above"
        foreach($step in 1..4){$at=Lighting-Point (270+27.5*$step) (.84+.54*$step);[CapyRowPointer]::Move($at[0],$at[1]);Start-Sleep -Milliseconds 30}
        Wait-Until {[Math]::Abs((Lighting-Value 'light')-20) -le 1} "A $device drag far outside the dial did not turn freely: $(Lighting-Value 'light')"
        if($device -eq 'mouse'){Capture "paper-lighting-drag-$theme" -Composed;Lighting-Dot "paper-lighting-drag-$theme" (Lighting-Value 'light')}
        [CapyRowPointer]::Up()
        Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled -and [Math]::Abs((Lighting-Value 'light')-20) -le 1} "The $device dial release changed the light"
        Relationship-Undo
        Wait-Until {(Lighting-Value 'light') -eq $before} "A $device dial drag needed more than one Undo"
    }
    $at=Lighting-Point 180 .84;[CapyRowPointer]::Down('mouse',$at[0],$at[1])
    Wait-Until {(Lighting-Value 'light') -eq 180} 'The held dial press did not preview'
    [CapyRowPointer]::Key(0x1B)
    Wait-Until {(Lighting-Value 'light') -eq $before} 'Escape did not cancel the held dial drag'
    [CapyRowPointer]::Up();Start-Sleep -Milliseconds 250
    if((Lighting-Value 'light') -ne $before){throw 'Releasing after Escape changed the light'}
    $dial=Control 'property-light-dial';$dial.SetFocus();Wait-Until {$dial.Current.HasKeyboardFocus} 'The Light dial did not take focus'
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x09);[CapyRowPointer]::Key([uint32]$review.Id,0x09)
    Wait-Until {(Control 'property-light-dial').Current.HasKeyboardFocus} 'Tab did not return to the Light dial'
    Capture "paper-lighting-focus-$theme" -Composed
    [CapyRowPointer]::Key([uint32]$review.Id,0x27)
    Wait-Until {(Lighting-Value 'light') -eq $before+1} 'Right did not step the focused dial'
    Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled} 'The dial key step did not finish'
    Relationship-Undo
    Wait-Until {(Lighting-Value 'light') -eq $before} 'A dial key step needed more than one Undo'
    $at=Lighting-Point 270 .84;[CapyRowPointer]::Down('mouse',$at[0],$at[1]);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()
    Wait-Until {(Lighting-Value 'light') -eq 270} 'A dial tap did not light from above'
    $at=Lighting-Point 180 .84;$null=[CapyRowPointer]::DoubleClick($at[0],$at[1])
    Wait-Until {(Lighting-Value 'light') -eq (Lighting-Property 'light').default.value} 'Double-clicking the dial did not restore the default light'
    Lighting-Relief '0' {(Lighting-Value 'relief') -eq 0 -and (Lighting-Property 'light').disabled -and !(Control 'property-light-dial').Current.IsEnabled}
}
function Paper-Geometry {
    $list=Control 'layer-list'
    $viewport=$list.Current.BoundingRectangle
    if($list.Current.IsOffscreen -or $viewport.IsEmpty -or $viewport.Width -le 0 -or $viewport.Height -le 0){$list=Find 'layer-list' -Visible}
    if(!$list){throw 'Paper geometry requires the current visible layer list'}
    $button=Control 'layer-add-paper' -Within $list -Arranged
    $last=@((Model).state.layers)[-1]
    $row=Control "layer-row-$($last.id)" -Within $list -Arranged
    $b=$button.Current.BoundingRectangle;$r=$row.Current.BoundingRectangle
    @{button=$b;last_row=$r;theme=(Model).state.theme}|ConvertTo-Json|Set-Content (Join-Path $run "paper-geometry-$((Model).state.theme).json")
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    if([Math]::Abs($b.Top-$r.Bottom) -gt 1 -or [Math]::Abs($b.Height/$scale-40) -gt 1 -or [Math]::Abs($b.Height-$r.Height) -gt 1){throw 'Add paper is not a 40-DIP row immediately after the last layer'}
    if($button.Current.Name -ne (Model).state.layer_tools.add_paper.label){throw 'Add paper caption differs from shared copy'}
}
function Paper-Activate([switch]$Keyboard,[string]$Device='mouse'){
    $count=(Model).state.layers.Count
    if($Keyboard){Focus 'layer-add-paper' -Keyboard;[CapyRowPointer]::Key([uint32]$review.Id,0x20)}
    else{
        $b=(Control 'layer-add-paper' -Arranged).Current.BoundingRectangle
        @{device=$Device;bounds=$b;count=$count}|ConvertTo-Json|Set-Content (Join-Path $run "paper-activate-$Device-$((Model).state.theme).json")
        if($Device -eq 'mouse'){
            [CapyRowPointer]::Hover([int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2))
            Capture "paper-hover-$((Model).state.theme)" -Composed -WithModel
        }
        [CapyRowPointer]::Down($Device,[int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2));Start-Sleep -Milliseconds 35
        [CapyRowPointer]::Up()
    }
    Wait-Until {(Model).state.layers.Count -eq $count+1 -and $null -eq (Model).state.layer_tools.add_paper} 'Add paper did not insert the shared paper layer'
    Relationship-Layers
    Wait-Until {!(Find 'layer-add-paper' -Visible)} 'Add paper stayed visible with paper present'
    $bottom=@((Model).state.layers)[-1]
    if((Model).state.layer_tools.editing_layer.id -ne $bottom.id){throw 'Inserted paper did not become the selected bottom layer'}
}
function Test-LayerPaper([switch]$Light) {
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    try{
    if($Light -and (Model).state.theme -ne 'light'){Relationship-Theme}
    foreach($theme in $(if($Light){@(2)}else{1..2})){
        Test-PaperTexture
        Paper-ScaleRelief
        Test-PaperLighting
        Relationship-Layers
        if((Model).state.layer_tools.add_paper -or (Find 'layer-add-paper' -Visible)){throw 'Add paper appears while initial paper exists'}
        $paper=@((Model).state.layers)[-1].id
        Relationship-Reveal "layer-$paper-name";Invoke "layer-$paper-name"
        Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $paper} 'Paper row selection did not publish'
        $count=(Model).state.layers.Count;Invoke 'layer-delete'
        Wait-Until {(Model).state.layers.Count -eq $count-1 -and $null -ne (Model).state.layer_tools.add_paper} 'Deleting paper did not expose Add paper'
        Paper-Geometry
        if(!$Light){
        foreach($device in @('mouse','pen','touch')){
            $before=(Model).state.document_file.revision
            $b=(Control 'layer-add-paper' -Arranged).Current.BoundingRectangle
            [CapyRowPointer]::Down($device,[int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2));Start-Sleep -Milliseconds 35;[CapyRowPointer]::Move([int]($b.X-20),[int]($b.Y-20));Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()
            Capture "paper-cancel-$device-$((Model).state.theme)" -WithModel
            if((Model).state.document_file.revision -ne $before){throw "Canceled $device Add paper press changed the drawing"}
            Paper-Activate -Device $device
            Relationship-Undo
            Wait-Until {$null -ne (Model).state.layer_tools.add_paper} 'Undo did not restore Add paper'
            Relationship-Layers
            Paper-Geometry
        }
        }
        $inserted=@(foreach($i in 1..64){Relationship-New})
        $rows=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
        $realized=@($rows|Where-Object {$_.Current.AutomationId -match '^layer-row-[0-9]+$'}).Count
        if($realized -ge 40){throw 'Add paper list wrapper disabled layer virtualization'}
        $realized|Set-Content (Join-Path $run "paper-realized-$((Model).state.theme).txt")
        $scroll=Scroll-Pattern (Control 'layer-list')
        Wait-Until {$scroll.Current.VerticallyScrollable} 'Additional layers did not make the list scrollable'
        $scroll.SetScrollPercent(-1,0)
        Wait-Until {!(Find 'layer-add-paper' -Visible)} 'Add paper did not scroll away with the layers'
        $scroll.SetScrollPercent(-1,100)
        Wait-Until {$null -ne (Find 'layer-add-paper' -Visible)} 'Scrolling to the bottom did not reveal Add paper'
        Paper-Geometry
        Paper-Activate -Keyboard
        Relationship-Undo
        Invoke 'Redo' -Name
        Wait-Until {$null -eq (Model).state.layer_tools.add_paper} 'Redo did not restore paper'
        Relationship-Layers
        Capture "paper-$((Model).state.theme)" -Composed -WithModel
        Relationship-Undo
        $first=$inserted[-1];$last=$inserted[0]
        Relationship-Layers
        $list=Control 'layer-list' -Arranged
        $scroll=Scroll-Pattern $list
        $scroll.SetScrollPercent(-1,0)
        $null=Control "layer-$first-name" -Within $list -Arranged;Invoke "layer-$first-name"
        Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $first} 'Cleanup range anchor did not select'
        $list=Control 'layer-list' -Arranged
        $scroll=Scroll-Pattern $list
        $scroll.SetScrollPercent(-1,100)
        $endpoint=Control "layer-$last-name" -Within $list -Arranged
        $viewport=$list.Current.BoundingRectangle
        if($endpoint.Current.BoundingRectangle.Top -lt $viewport.Top -or $endpoint.Current.BoundingRectangle.Bottom -gt $viewport.Bottom){throw 'Cleanup range endpoint is outside the list viewport'}
        $bounds=$endpoint.Current.BoundingRectangle
        [CapyRowPointer]::Hover([int]($bounds.X+$bounds.Width/2),[int]($bounds.Y+$bounds.Height/2))
        [CapyRowPointer]::Hold(0x10,$true)
        try{
            Invoke "layer-$last-name"
            Wait-Until {@((Model).state.layers|Where-Object selected).Count -eq $inserted.Count} 'Cleanup did not select the inserted range'
        }finally{[CapyRowPointer]::Hold(0x10,$false)}
        Invoke 'layer-delete'
        Wait-Until {(Model).state.layers.Count -eq $count-1 -and !@((Model).state.layers|Where-Object {$_.id -in $inserted}).Count} 'Cleanup did not remove exactly the inserted rows'
        Paper-Activate -Keyboard
        if($theme -eq 1){Relationship-Theme}
    }
    while((Model).state.layers.Count){
        $count=(Model).state.layers.Count
        $id=@((Model).state.layers)[0].id
        Relationship-Reveal "layer-$id-name";Invoke "layer-$id-name"
        Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $id} 'Layer row selection did not publish'
        Invoke 'layer-delete'
        Wait-Until {(Model).state.layers.Count -eq $count-1} 'Deleting the remaining row did not publish'
    }
    Wait-Until {$null -ne (Find 'layer-add-paper' -Visible)} 'Empty Layers omitted Add paper'
    Capture 'paper-empty' -Composed -WithModel
    Paper-Activate
    }finally{[CapyRowPointer]::Dispose()}
}
