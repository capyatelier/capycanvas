using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public sealed class PrivateWorkspaceLease : IDisposable {
    [DllImport("winsqlite3.dll", CallingConvention=CallingConvention.Cdecl)]
    static extern int sqlite3_open_v2([MarshalAs(UnmanagedType.LPUTF8Str)] string path, out IntPtr db, int flags, IntPtr vfs);
    [DllImport("winsqlite3.dll", CallingConvention=CallingConvention.Cdecl)]
    static extern int sqlite3_busy_timeout(IntPtr db, int milliseconds);
    [DllImport("winsqlite3.dll", CallingConvention=CallingConvention.Cdecl)]
    static extern int sqlite3_exec(IntPtr db, [MarshalAs(UnmanagedType.LPUTF8Str)] string sql, IntPtr callback, IntPtr context, out IntPtr error);
    [DllImport("winsqlite3.dll", CallingConvention=CallingConvention.Cdecl)]
    static extern int sqlite3_close_v2(IntPtr db);
    [DllImport("winsqlite3.dll", CallingConvention=CallingConvention.Cdecl)]
    static extern void sqlite3_free(IntPtr memory);
    [DllImport("winsqlite3.dll", CallingConvention=CallingConvention.Cdecl)]
    static extern int sqlite3_prepare_v2(IntPtr db, [MarshalAs(UnmanagedType.LPUTF8Str)] string sql, int bytes, out IntPtr statement, IntPtr tail);
    [DllImport("winsqlite3.dll", CallingConvention=CallingConvention.Cdecl)]
    static extern int sqlite3_step(IntPtr statement);
    [DllImport("winsqlite3.dll", CallingConvention=CallingConvention.Cdecl)]
    static extern IntPtr sqlite3_column_text(IntPtr statement, int column);
    [DllImport("winsqlite3.dll", CallingConvention=CallingConvention.Cdecl)]
    static extern int sqlite3_finalize(IntPtr statement);
    public sealed class Claim {
        public string Id, Name, Owner, Epoch, Fence, LeaseUntil;
    }
    readonly object sync = new object();
    readonly Stopwatch elapsed = Stopwatch.StartNew();
    IntPtr connection;
    Timer watchdog;
    public bool Expired { get; private set; }
    public string ReleaseError { get; private set; }
    public string Database { get; private set; }
    public long ElapsedMilliseconds { get { return elapsed.ElapsedMilliseconds; } }
    static string Text(IntPtr value) {
        if(value == IntPtr.Zero) return null;
        int length=0; while(Marshal.ReadByte(value,length)!=0) length++;
        var bytes=new byte[length]; Marshal.Copy(value,bytes,0,length);
        return Encoding.UTF8.GetString(bytes);
    }
    static string Validate(string caseRoot, string profileRoot) {
        string run=Path.GetFullPath(caseRoot).TrimEnd(Path.DirectorySeparatorChar)+Path.DirectorySeparatorChar;
        string profile=Path.GetFullPath(profileRoot).TrimEnd(Path.DirectorySeparatorChar);
        if(!profile.StartsWith(run,StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("Profile must belong to this fixture run");
        string configured=Environment.GetEnvironmentVariable("CAPY_STORAGE_DIR");
        if(String.IsNullOrEmpty(configured)||!String.Equals(Path.GetFullPath(configured).TrimEnd(Path.DirectorySeparatorChar),profile,StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("Profile differs from the owned CAPY_STORAGE_DIR");
        string db=Path.Combine(profile,"data","workspaces","workspaces.sqlite3");
        if(!File.Exists(db)) throw new FileNotFoundException("Owned workspace database is not initialized",db);
        for(string path=db;!String.IsNullOrEmpty(path);path=Path.GetDirectoryName(path)) {
            if((File.GetAttributes(path)&FileAttributes.ReparsePoint)!=0) throw new InvalidOperationException("Reparse points are not accepted in the owned database path");
        }
        return db;
    }
    static IntPtr Open(string path, int flags) {
        IntPtr db; int code=sqlite3_open_v2(path,out db,flags|0x10000,IntPtr.Zero);
        if(code!=0) { if(db!=IntPtr.Zero) sqlite3_close_v2(db); throw new InvalidOperationException("SQLite open failed: "+code); }
        if(sqlite3_busy_timeout(db,250)!=0) { sqlite3_close_v2(db); throw new InvalidOperationException("SQLite fixture busy timeout failed"); }
        return db;
    }
    static void Execute(IntPtr db, string sql) {
        IntPtr error; int code=sqlite3_exec(db,sql,IntPtr.Zero,IntPtr.Zero,out error);
        string detail=Text(error); if(error!=IntPtr.Zero) sqlite3_free(error);
        if(code!=0) throw new InvalidOperationException("SQLite fixture statement failed: "+code+" "+detail);
    }
    public PrivateWorkspaceLease(string caseRoot, string profileRoot) {
        Database=Validate(caseRoot,profileRoot);
        connection=Open(Database,2);
        try {
            Execute(connection,"BEGIN IMMEDIATE");
            elapsed.Restart();
            watchdog=new Timer(delegate { Release(true); },null,4000,Timeout.Infinite);
        } catch { sqlite3_close_v2(connection); connection=IntPtr.Zero; throw; }
    }
    void Release(bool expired) {
        lock(sync) {
            if(connection==IntPtr.Zero) return;
            Expired=expired;
            try { Execute(connection,"ROLLBACK"); }
            catch(Exception error) { ReleaseError=error.Message; }
            finally { sqlite3_close_v2(connection); connection=IntPtr.Zero; }
            if(watchdog!=null) { watchdog.Dispose(); watchdog=null; }
        }
    }
    public void Dispose() {
        Release(false);
        if(Expired) throw new InvalidOperationException("Fixture SQLite acknowledgement gate expired before explicit release");
        if(ReleaseError!=null) throw new InvalidOperationException(ReleaseError);
    }
    public static Claim[] ReadClaims(string caseRoot, string profileRoot) {
        IntPtr db=Open(Validate(caseRoot,profileRoot),1),statement=IntPtr.Zero;
        try {
            int code=sqlite3_prepare_v2(db,"SELECT id,name,owner,epoch,fence,lease_until FROM items WHERE owner IS NOT NULL ORDER BY id",-1,out statement,IntPtr.Zero);
            if(code!=0) throw new InvalidOperationException("Cannot prepare owned claim read: "+code);
            var rows=new List<Claim>();
            while((code=sqlite3_step(statement))==100) rows.Add(new Claim {Id=Text(sqlite3_column_text(statement,0)),Name=Text(sqlite3_column_text(statement,1)),Owner=Text(sqlite3_column_text(statement,2)),Epoch=Text(sqlite3_column_text(statement,3)),Fence=Text(sqlite3_column_text(statement,4)),LeaseUntil=Text(sqlite3_column_text(statement,5))});
            if(code!=101) throw new InvalidOperationException("Cannot finish owned claim read: "+code);
            return rows.ToArray();
        } finally { if(statement!=IntPtr.Zero) sqlite3_finalize(statement); sqlite3_close_v2(db); }
    }
}
