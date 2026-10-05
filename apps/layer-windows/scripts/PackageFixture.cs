using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Text;
public static class CapyPackageFixture {
 public static List<KeyValuePair<string,byte[]>> Read(string path) {
  var members=new List<KeyValuePair<string,byte[]>>();
  using(var archive=ZipFile.OpenRead(path)) {
   foreach(var entry in archive.Entries) {
    using(var stream=entry.Open()) using(var copy=new MemoryStream()) {stream.CopyTo(copy);members.Add(new KeyValuePair<string,byte[]>(entry.FullName,copy.ToArray()));}
   }
  }
  return members;
 }
 public static void Write(string path,IList<KeyValuePair<string,byte[]>> members) {
  var output=new MemoryStream();var directory=new MemoryStream();
  foreach(var member in members) {
   var name=Encoding.ASCII.GetBytes(member.Key);var data=member.Value;var crc=Crc(data);var offset=(uint)output.Position;
   Dword(output,0x04034b50);foreach(var value in new[]{20,0,0,0,0})Word(output,value);
   Dword(output,crc);Dword(output,(uint)data.Length);Dword(output,(uint)data.Length);Word(output,name.Length);Word(output,0);
   output.Write(name,0,name.Length);output.Write(data,0,data.Length);
   Dword(directory,0x02014b50);foreach(var value in new[]{20,20,0,0,0,0})Word(directory,value);
   Dword(directory,crc);Dword(directory,(uint)data.Length);Dword(directory,(uint)data.Length);
   foreach(var value in new[]{name.Length,0,0,0,0})Word(directory,value);
   Dword(directory,0);Dword(directory,offset);directory.Write(name,0,name.Length);
  }
  var start=(uint)output.Position;directory.Position=0;directory.CopyTo(output);
  Dword(output,0x06054b50);foreach(var value in new[]{0,0,members.Count,members.Count})Word(output,value);
  Dword(output,(uint)directory.Length);Dword(output,start);Word(output,0);
  File.WriteAllBytes(path,output.ToArray());
 }
 static uint Crc(byte[] data) {
  uint crc=0xFFFFFFFF;
  foreach(var item in data){crc^=item;for(int bit=0;bit<8;bit++)crc=(crc&1)!=0?(crc>>1)^0xEDB88320u:crc>>1;}
  return ~crc;
 }
 static void Word(Stream stream,int value){stream.WriteByte((byte)value);stream.WriteByte((byte)(value>>8));}
 static void Dword(Stream stream,uint value){Word(stream,(int)(value&0xFFFF));Word(stream,(int)(value>>16));}
}
