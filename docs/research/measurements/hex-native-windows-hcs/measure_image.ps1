$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\image-stream-size-v2'
if(Test-Path -LiteralPath $root){throw 'owned stream-measurement path already exists'}
[void][IO.Directory]::CreateDirectory($root)
[void][IO.Directory]::CreateDirectory(($root+'\temp'))
$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.IO.Compression;
using System.Net;
using System.Security.Cryptography;
using System.Diagnostics;
public static class NativeImageMeter {
 public class Metered : Stream {
  Stream stream; SHA256 hash=new SHA256Cng(); public long count=0; long bound;
  public Metered(Stream source,long maximum){stream=source;bound=maximum;}
  public override int Read(byte[] bytes,int offset,int amount){int got=stream.Read(bytes,offset,amount);count=checked(count+got);if(count>bound)throw new IOException("compressed input exceeded exact manifest extent");if(got>0)hash.TransformBlock(bytes,offset,got,null,0);return got;}
  public string Finish(){hash.TransformFinalBlock(new byte[0],0,0);return BitConverter.ToString(hash.Hash).Replace("-","").ToLowerInvariant();}
  protected override void Dispose(bool disposing){if(disposing){hash.Dispose();stream.Dispose();}base.Dispose(disposing);}
  public override bool CanRead{get{return true;}} public override bool CanSeek{get{return false;}} public override bool CanWrite{get{return false;}}
  public override long Length{get{throw new NotSupportedException();}}public override long Position{get{return count;}set{throw new NotSupportedException();}}
  public override void Flush(){}public override long Seek(long offset,SeekOrigin origin){throw new NotSupportedException();}public override void SetLength(long value){throw new NotSupportedException();}public override void Write(byte[] bytes,int offset,int amount){throw new NotSupportedException();}
 }
 public class Result {public long compressedBytes,tarBytes;public string compressedSha256,tarSha256;public long elapsedMilliseconds;}
 public static Result Measure(string uri,long expectedCompressed,string expectedHash,string expectedTarHash,long maxTarBytes,int budgetMs) {
  Stopwatch time=Stopwatch.StartNew();HttpWebRequest request=(HttpWebRequest)WebRequest.Create(uri);request.Timeout=30000;request.ReadWriteTimeout=30000;
  using(HttpWebResponse response=(HttpWebResponse)request.GetResponse()) {
   if(response.ContentLength!=expectedCompressed)throw new IOException("compressed content-length differs from pinned manifest");
   using(Metered raw=new Metered(response.GetResponseStream(),expectedCompressed))using(SHA256 tarHash=new SHA256Cng()) {
    long total=0;long nextProgress=expectedCompressed/4;long progressStep=Math.Max(1,expectedCompressed/4);byte[] buffer=new byte[1024*1024];
    using(GZipStream gzip=new GZipStream(raw,CompressionMode.Decompress,true)) {
     int count;
     while((count=gzip.Read(buffer,0,buffer.Length))>0){total=checked(total+count);if(total>maxTarBytes)throw new IOException("explicit inflated-byte read ceiling exceeded");if(time.ElapsedMilliseconds>budgetMs)throw new IOException("whole stream caller deadline reached compressed="+raw.count+" tar="+total+" elapsed_ms="+time.ElapsedMilliseconds);tarHash.TransformBlock(buffer,0,count,null,0);if(raw.count>=nextProgress){Console.WriteLine("IMAGE_STREAM_PROGRESS compressed="+raw.count+" tar="+total+" elapsed_ms="+time.ElapsedMilliseconds);nextProgress=checked(nextProgress+progressStep);}}
    }
    while(raw.Read(buffer,0,buffer.Length)>0){if(time.ElapsedMilliseconds>budgetMs)throw new IOException("compressed tail caller deadline reached");}
    tarHash.TransformFinalBlock(new byte[0],0,0);Result result=new Result();result.tarBytes=total;result.compressedBytes=raw.count;result.compressedSha256=raw.Finish();result.tarSha256=BitConverter.ToString(tarHash.Hash).Replace("-","").ToLowerInvariant();result.elapsedMilliseconds=time.ElapsedMilliseconds;
    if(result.compressedBytes!=expectedCompressed || result.compressedSha256!=expectedHash || result.tarSha256!=expectedTarHash)throw new IOException("source-pinned compressed blob or diff_id mismatch");
    return result;
   }
  }
 }
}
'@
$manifest=Get-Content -LiteralPath 'C:\Users\corph\.cache\hex-native-windows-hcs\image-metadata-v2\manifest.json' -Raw|ConvertFrom-Json
$config=Get-Content -LiteralPath 'C:\Users\corph\.cache\hex-native-windows-hcs\image-metadata-v2\config.json' -Raw|ConvertFrom-Json
$result=[ordered]@{kind='read-only-stream-inflated-size';root=$root;reference='mcr.microsoft.com/windows/server@sha256:a62109569303cfe0ab49b1618383615f0477cdc285529b98d9b7085902d0e745';purpose='measure exact hashed tar byte counts before persistent cache transfer/import';needs='cpus=1,mem=4g,disk=12g';max_tar_bytes_per_layer=34359738368;budget_ms_per_layer=390000;layers=@();blob_files_created=0;native_gpu_dispatches=0;native_hcs_invocations=0}
for($i=0;$i -lt $manifest.layers.Count;$i++){
 $layer=$manifest.layers[$i]
 $measured=[NativeImageMeter]::Measure(('https://mcr.microsoft.com/v2/windows/server/blobs/'+$layer.digest),[long]$layer.size,$layer.digest.Substring(7),$config.rootfs.diff_ids[$i].Substring(7),34359738368,390000)
 $result.layers+=@{digest=$layer.digest;diff_id=$config.rootfs.diff_ids[$i];measurement=$measured}
 [IO.File]::WriteAllText(($root+'\result.json'),($result|ConvertTo-Json -Depth 15))
 [Console]::WriteLine(('LAYER_MEASURED '+($result.layers[-1]|ConvertTo-Json -Depth 10 -Compress)))
}
$result.total_compressed_bytes=0L;$result.total_tar_bytes=0L
foreach($row in $result.layers){$result.total_compressed_bytes+=$row.measurement.compressedBytes;$result.total_tar_bytes+=$row.measurement.tarBytes}
$result.disk_semantics='hashed uncompressed tar extents; actual NTFS snapshot allocation not yet measured; persistent preparation must reserve separate bounded overhead/scratch and stop at its admitted cap'
[IO.File]::WriteAllText(($root+'\result.json'),($result|ConvertTo-Json -Depth 15))
[Console]::WriteLine(($result|ConvertTo-Json -Depth 15 -Compress))
