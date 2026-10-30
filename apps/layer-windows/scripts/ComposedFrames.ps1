Add-Type -AssemblyName System.Drawing
if(!('TabFrames' -as [type])){
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
public static class TabFrames {
 [DllImport("dwmapi.dll")] public static extern int DwmFlush();
 static int Delta(Color p,Color q){return Math.Max(Math.Abs(p.R-q.R),Math.Max(Math.Abs(p.G-q.G),Math.Abs(p.B-q.B)));}
 public static Bitmap Grab(int x,int y,int width,int height){
  var image=new Bitmap(width,height);
  using(var graphics=Graphics.FromImage(image))graphics.CopyFromScreen(x,y,0,0,image.Size);
  return image;
 }
 public sealed class Envelope {
  readonly int width,height;
  readonly int[] lo,hi;
  public Envelope(Bitmap preview,Bitmap final){
   width=preview.Width;height=preview.Height;int size=width*height*3;
   var minimum=new int[size];var maximum=new int[size];lo=new int[size];hi=new int[size];
   for(int y=0;y<height;y++)for(int x=0;x<width;x++){
    int i=(y*width+x)*3;var p=preview.GetPixel(x,y);var q=final.GetPixel(x,y);
    minimum[i]=Math.Min(p.R,q.R);minimum[i+1]=Math.Min(p.G,q.G);minimum[i+2]=Math.Min(p.B,q.B);
    maximum[i]=Math.Max(p.R,q.R);maximum[i+1]=Math.Max(p.G,q.G);maximum[i+2]=Math.Max(p.B,q.B);
   }
   for(int y=0;y<height;y++)for(int x=0;x<width;x++){
    int i=(y*width+x)*3;lo[i]=lo[i+1]=lo[i+2]=255;
    for(int yy=Math.Max(0,y-1);yy<=Math.Min(height-1,y+1);yy++)
     for(int xx=Math.Max(0,x-1);xx<=Math.Min(width-1,x+1);xx++){
      int j=(yy*width+xx)*3;
      for(int c=0;c<3;c++){lo[i+c]=Math.Min(lo[i+c],minimum[j+c]);hi[i+c]=Math.Max(hi[i+c],maximum[j+c]);}
     }
    for(int c=0;c<3;c++){lo[i+c]-=6;hi[i+c]+=6;}
   }
  }
  public int Difference(Bitmap image){return Difference(image,Rectangle.Empty);}
  public int Difference(Bitmap image,Rectangle excluded){
   int count=0;
   for(int y=0;y<height;y++)for(int x=0;x<width;x++){
    if(excluded.Contains(x,y))continue;
    int i=(y*width+x)*3;var v=image.GetPixel(x,y);
    if(v.R<lo[i]||v.R>hi[i]||v.G<lo[i+1]||v.G>hi[i+1]||v.B<lo[i+2]||v.B>hi[i+2])count++;
   }
   return count;
  }
 }
 public static int[] Remaining(Bitmap image,Bitmap final,Rectangle overview){
  int native=0,paper=0;
  for(int y=0;y<image.Height;y++)for(int x=0;x<image.Width;x++){
   if(Delta(image.GetPixel(x,y),final.GetPixel(x,y))<=6)continue;
   if(overview.Contains(x,y))paper++;else native++;
  }
  return new[]{native,paper};
 }
 public static int FootDifference(Bitmap a,RectangleF from,Bitmap b,RectangleF to,double scale){
  int count=0,pad=(int)(8*scale),y=(int)(to.Bottom-5*scale),oldY=(int)(from.Bottom-5*scale);
  for(int x=pad;x<to.Width-pad;x++){
   int oldX=(int)(from.X+pad+(x-pad)*(from.Width-2*pad)/(to.Width-2*pad));
   if(Delta(a.GetPixel(oldX,oldY),b.GetPixel((int)to.X+x,y))>16)count++;
  }
  return count;
 }
}
'@
}

function Sample-Frames($Area,[scriptblock]$Action){
 $frames=[Collections.Generic.List[Drawing.Bitmap]]::new();$times=@()
 try{
  for($i=0;$i -lt 90;$i++){
   $null=[TabFrames]::DwmFlush();$actionAt=[Diagnostics.Stopwatch]::GetTimestamp();& $Action $i|Out-Null
   $begin=[Diagnostics.Stopwatch]::GetTimestamp()
   $frames.Add([TabFrames]::Grab([int]$Area.Left,[int]$Area.Top,[int]$Area.Width,[int]$Area.Height))
   $end=[Diagnostics.Stopwatch]::GetTimestamp()
   $times+=@{frame=$i;action=$actionAt;begin=$begin;end=$end}
  }
  @{frames=$frames;times=$times}
 }catch{foreach($image in $frames){$image.Dispose()};throw}
}
