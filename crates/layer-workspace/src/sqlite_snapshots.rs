use super::*;
use crate::{snapshots, worker};
use std::{fs, path::PathBuf};

fn io(error: std::io::Error) -> StoreError { StoreError::new(ErrorKind::Unavailable,error.to_string()) }

fn modified(path: &Path) -> Result<Option<SystemTime>> {
    match fs::metadata(path) {
        Ok(metadata) => metadata.modified().map(Some).map_err(io),
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io(error)),
    }
}

pub(super) fn protect(connection: &Connection) -> Result<()> {
    let path = Path::new(connection.path().unwrap_or_default());
    let marker = path.with_file_name("last-version");
    let marker_time = modified(&marker)?;
    let previous = if marker_time.is_some() {
        if fs::metadata(&marker).map_err(io)?.len()>128 { return Err(StoreError::invalid("Invalid workspace version marker.")); }
        fs::read_to_string(&marker).map_err(io)?
    } else {String::new()};
    if previous.trim()!=snapshots::VERSION {
        let mut populated = false;
        for table in table_names(connection)? {
            populated |= connection.query_row(&format!("SELECT EXISTS(SELECT 1 FROM \"{}\")",table.replace('"',"\"\"")),[],|row|row.get::<_,bool>(0))?;
            if populated { break; }
        }
        if populated {
            let destination = path.with_file_name(format!("{}.sqlite3",snapshots::name(previous.trim())));
            worker::backup_database(path,&destination)?;
        }
    }
    let legacy = path.with_extension("unreadable.sqlite3");
    let legacy_time = modified(&legacy)?.into_iter().chain(modified(&PathBuf::from(format!("{}-wal",legacy.display())))?).max();
    if legacy_time.is_some() && (marker_time.is_none() || legacy_time>marker_time) {
        worker::backup_database(&legacy,&path.with_file_name(format!("{}.sqlite3",snapshots::name("unreadable"))))?;
    }
    if previous.trim()!=snapshots::VERSION || legacy_time.is_some() && legacy_time>marker_time {
        worker::publish_file(&marker,|temporary| fs::write(temporary,snapshots::VERSION).map_err(io))?;
    }
    Ok(())
}

fn copies(connection: &Connection) -> Result<Vec<crate::snapshots::SnapshotInfo>> {
    let path = Path::new(connection.path().unwrap_or_default());
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut copies = Vec::new();
    for entry in fs::read_dir(parent).map_err(io)? {
        let entry = entry.map_err(io)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {continue};
        if !name.strip_suffix(".sqlite3").is_some_and(snapshots::managed_name) {continue;}
        let metadata = entry.metadata().map_err(io)?;
        copies.push(snapshots::SnapshotInfo {id:name,bytes:metadata.len(),created_ms:metadata.modified().map_err(io)?.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64});
    }
    Ok(copies)
}

pub(super) fn prune(connection: &Connection) -> Result<()> {
    let path = Path::new(connection.path().unwrap_or_default());
    let parent = path.parent().unwrap_or(Path::new("."));
    for id in snapshots::prune(copies(connection)?) { fs::remove_file(parent.join(id)).map_err(io)?; }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seed() -> PathBuf {
        let directory = crate::test_support::temp_dir("version-snapshots");
        fs::create_dir_all(&directory).unwrap();
        fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat/1.0.15/workspaces.sqlite3"),directory.join("workspaces.sqlite3")).unwrap();
        directory
    }
    #[test]
    fn version_change_copies_before_the_first_write_and_keeps_legacy_unreadable() {
        let directory = seed();
        let path = directory.join("workspaces.sqlite3");
        let sql = Connection::open(&path).unwrap();
        sql.execute("UPDATE items SET owner='prior',epoch='prior',lease_until='0' WHERE kind='workspace'",[]).unwrap();
        drop(sql);
        let marker = directory.join("last-version");
        fs::write(&marker,"1.0.14").unwrap();
        fs::File::options().write(true).open(&marker).unwrap().set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)).unwrap();
        let legacy = path.with_extension("unreadable.sqlite3");
        fs::copy(&path,&legacy).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        let copied = copies(&store.connection).unwrap();
        assert_eq!(copied.len(),2);
        store.handle(StoreRequest::List).unwrap();
        for copy in copied {
            let old = Connection::open(directory.join(copy.id)).unwrap();
            let owners: u32 = old.query_row("SELECT count(*) FROM items WHERE owner='prior'",[],|row|row.get(0)).unwrap();
            assert!(owners>0);
        }
        assert_eq!(store.connection.query_row("SELECT count(*) FROM items WHERE owner='prior'",[],|row|row.get::<_,u32>(0)).unwrap(),0);
        drop(store);
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(copies(&store.connection).unwrap().len(),2);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn upgrade_downgrade_upgrade_keeps_distinct_snapshots_and_prunes_after_adoption() {
        let directory = seed();
        let path = directory.join("workspaces.sqlite3");
        for version in ["1.0.14","9.0.0","1.0.14"] {
            fs::write(directory.join("last-version"),version).unwrap();
            drop(SqliteStore::open(&path).unwrap());
        }
        let mut store = SqliteStore::open(&path).unwrap();
        let copied = copies(&store.connection).unwrap();
        assert_eq!(copied.len(),3);
        assert_eq!(copied.iter().filter(|copy|copy.id.starts_with("workspaces.1.0.14.")).count(),2);
        for _ in 0..9 {
            fs::copy(&path,directory.join(format!("{}.sqlite3",snapshots::name("old")))).unwrap();
        }
        assert_eq!(copies(&store.connection).unwrap().len(),12);
        store.handle(StoreRequest::Adopted).unwrap();
        assert_eq!(copies(&store.connection).unwrap().len(),10);
        let retained = directory.join(&copies(&store.connection).unwrap()[0].id);
        let expected: String = Connection::open(&retained).unwrap().query_row("SELECT json FROM editing",[],|row|row.get(0)).unwrap();
        store.connection.execute("UPDATE editing SET json='{}'",[]).unwrap();
        drop(store);
        let recovery = directory.join("recovery");
        fs::create_dir(&recovery).unwrap();
        for name in ["workspaces.sqlite3","workspaces.sqlite3-wal","workspaces.sqlite3-shm"] {
            if directory.join(name).exists() { fs::rename(directory.join(name),recovery.join(name)).unwrap(); }
        }
        fs::copy(&retained,&path).unwrap();
        fs::remove_file(directory.join("last-version")).unwrap();
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.connection.query_row("SELECT json FROM editing",[],|row|row.get::<_,String>(0)).unwrap(),expected);
        assert!(retained.exists());
        assert!(copies(&store.connection).unwrap().iter().any(|copy|copy.id.starts_with("workspaces.earlier.")));
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn failed_marker_read_keeps_rollback_journal_database_bytes() {
        let directory = seed();
        let path = directory.join("workspaces.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.pragma_update(None,"journal_mode","DELETE").unwrap();
        drop(connection);
        fs::create_dir(directory.join("last-version")).unwrap();
        let original = fs::read(&path).unwrap();
        assert!(SqliteStore::open(&path).is_err());
        assert!(fs::read(&path).unwrap()==original,"Failed marker read changed the database bytes");
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn failed_snapshot_keeps_database_and_wal_bytes_and_starts_in_memory() {
        use std::os::unix::fs::PermissionsExt;
        let directory = seed();
        let path = directory.join("workspaces.sqlite3");
        let keeper = Connection::open(&path).unwrap();
        keeper.execute_batch("PRAGMA wal_autocheckpoint=0; UPDATE items SET lease_until='123';").unwrap();
        keeper.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,true).unwrap();
        drop(keeper);
        let wal = PathBuf::from(format!("{}-wal",path.display()));
        let original = (fs::read(&path).unwrap(),fs::read(&wal).unwrap());
        fs::write(directory.join("last-version"),"older").unwrap();
        fs::set_permissions(&directory,fs::Permissions::from_mode(0o500)).unwrap();
        let (controller,mut host) = crate::controller_tests::start_native(&directory);
        let memory = controller.manager.in_memory();
        crate::controller_tests::close_native(controller,&mut host);
        fs::set_permissions(&directory,fs::Permissions::from_mode(0o700)).unwrap();
        assert!(memory);
        assert!(original==(fs::read(&path).unwrap(),fs::read(&wal).unwrap()),"Failed copy changed the database or WAL bytes");
        assert_eq!(fs::read_to_string(directory.join("last-version")).unwrap(),"older");
        fs::remove_dir_all(directory).unwrap();
    }
}
