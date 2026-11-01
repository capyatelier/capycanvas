use super::*;
use crate::{snapshots, worker};
use snapshots::{SnapshotInfo, VersionMarker};
use std::{fs, path::PathBuf};

fn io(error: std::io::Error) -> StoreError { StoreError::new(ErrorKind::Unavailable,error.to_string()) }

fn identity(path: &Path) -> Result<Option<(u64, SystemTime)>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some((metadata.len(),metadata.modified().map_err(io)?))),
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io(error)),
    }
}

fn read_marker(path: &Path) -> Result<VersionMarker> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.len() > 1024 * 1024 => Err(StoreError::invalid("Invalid workspace version marker.")),
        Ok(_) => Ok(serde_json::from_slice(&fs::read(path).map_err(io)?).unwrap_or_default()),
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => Ok(VersionMarker::default()),
        Err(error) => Err(io(error)),
    }
}

fn write_marker(path: &Path, marker: &VersionMarker) -> Result<()> {
    worker::publish_file(path,|temporary| fs::write(temporary,serde_json::to_vec(marker)?).map_err(io))
}

pub(super) fn protect(connection: &Connection) -> Result<()> {
    let path = Path::new(connection.path().unwrap_or_default());
    let marker_path = path.with_file_name("last-version");
    let previous = read_marker(&marker_path)?;
    let mut copies = Vec::new();
    let mut copy = |source: &Path, version: &str| -> Result<()> {
        let id = format!("{}.sqlite3",snapshots::name(version));
        let destination = path.with_file_name(&id);
        worker::backup_database(source,&destination)?;
        copies.push(SnapshotInfo {id,bytes:fs::metadata(destination).map_err(io)?.len()});
        Ok(())
    };
    if previous.version!=snapshots::VERSION {
        for table in table_names(connection)? {
            if connection.query_row(&format!("SELECT EXISTS(SELECT 1 FROM \"{}\")",table.replace('"',"\"\"")),[],|row|row.get::<_,bool>(0))? {
                copy(path,&previous.version)?;
                break;
            }
        }
    }
    let legacy = path.with_extension("unreadable.sqlite3");
    let unreadable_identity = identity(&legacy)?.map(|database| {
        identity(&PathBuf::from(format!("{}-wal",legacy.display()))).map(|wal|format!("{database:?}:{:?}",wal.filter(|(size,_)|*size>0)))
    }).transpose()?.or_else(||previous.unreadable_identity.clone());
    if unreadable_identity!=previous.unreadable_identity { copy(&legacy,"unreadable")?; }
    let mut marker = previous.clone();
    marker.unreadable_identity = unreadable_identity;
    let (marker,_) = marker.finish(copies,false);
    if marker!=previous {write_marker(&marker_path,&marker)?;}
    Ok(())
}

fn copies(connection: &Connection) -> Result<Vec<SnapshotInfo>> {
    let path = Path::new(connection.path().unwrap_or_default());
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut copies = Vec::new();
    for entry in fs::read_dir(parent).map_err(io)? {
        let entry = entry.map_err(io)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {continue};
        if !name.strip_suffix(".sqlite3").is_some_and(snapshots::managed_name) {continue;}
        let metadata = entry.metadata().map_err(io)?;
        if metadata.is_file() {copies.push(SnapshotInfo {id:name,bytes:metadata.len()});}
    }
    copies.sort_by(|a,b|a.id.cmp(&b.id));
    Ok(copies)
}

pub(super) fn prune(connection: &Connection) -> Result<()> {
    let path = Path::new(connection.path().unwrap_or_default());
    let marker_path = path.with_file_name("last-version");
    let mut marker = read_marker(&marker_path)?;
    let files = copies(connection)?;
    let mut ordered: Vec<_> = files.iter().filter(|file|!marker.copies.iter().any(|copy|copy.id==file.id)).cloned().collect();
    ordered.extend(marker.copies.iter().filter_map(|copy|files.iter().find(|file|file.id==copy.id).cloned()));
    marker.copies = ordered;
    let (marker,remove) = marker.finish(Vec::new(),true);
    write_marker(&marker_path,&marker)?;
    for id in remove { fs::remove_file(path.with_file_name(id)).map_err(io)?; }
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
        write_marker(&marker,&VersionMarker {version:"1.0.14".into(),..Default::default()}).unwrap();
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
            let marker_path = directory.join("last-version");
            let mut marker = read_marker(&marker_path).unwrap();
            marker.version = version.into();
            write_marker(&marker_path,&marker).unwrap();
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
    fn changed_legacy_size_is_copied_even_with_equal_or_older_mtime() {
        let directory = seed();
        let path = directory.join("workspaces.sqlite3");
        let legacy = path.with_extension("unreadable.sqlite3");
        fs::copy(&path,&legacy).unwrap();
        let date = UNIX_EPOCH + std::time::Duration::from_secs(100);
        let set_date = |date| fs::File::options().write(true).open(&legacy).unwrap().set_times(fs::FileTimes::new().set_modified(date)).unwrap();
        set_date(date);
        let store = SqliteStore::open(&path).unwrap();
        let mut count = copies(&store.connection).unwrap().len();
        drop(store);
        for date in [date, UNIX_EPOCH] {
            let before = fs::metadata(&legacy).unwrap().len();
            let sql = Connection::open(&legacy).unwrap();
            sql.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE IF NOT EXISTS growing(bytes BLOB); INSERT INTO growing VALUES(zeroblob(8192));").unwrap();
            drop(sql);
            assert!(fs::metadata(&legacy).unwrap().len()>before);
            set_date(date);
            let store = SqliteStore::open(&path).unwrap();
            count += 1;
            assert_eq!(copies(&store.connection).unwrap().len(),count);
        }
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn publication_order_keeps_this_start_copy_and_treats_untracked_files_as_oldest() {
        let directory = seed();
        let path = directory.join("workspaces.sqlite3");
        let mut marker = VersionMarker {version:"earlier".into(),..Default::default()};
        for _ in 0..12 {
            let id = format!("{}.sqlite3",snapshots::name("future"));
            let destination = directory.join(&id);
            fs::copy(&path,&destination).unwrap();
            fs::File::options().write(true).open(&destination).unwrap().set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH+std::time::Duration::from_secs(4_000_000_000))).unwrap();
            if marker.copies.len()<11 {marker.copies.push(SnapshotInfo {id,bytes:fs::metadata(destination).unwrap().len()});}
        }
        write_marker(&directory.join("last-version"),&marker).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        let current = read_marker(&directory.join("last-version")).unwrap().copies.last().unwrap().id.clone();
        store.handle(StoreRequest::Adopted).unwrap();
        let retained = copies(&store.connection).unwrap();
        assert_eq!(retained.len(),10);
        assert!(retained.iter().any(|copy|copy.id==current));
        assert!(retained.iter().all(|copy|copy.id==current || marker.copies.iter().any(|known|known.id==copy.id)));
        assert_eq!(read_marker(&directory.join("last-version")).unwrap().copies.last().unwrap().id,current);
        assert!(!directory.join(&marker.copies[0].id).exists());
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn failed_marker_read_still_opens_persistent_storage() {
        let directory = seed();
        let path = directory.join("workspaces.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.pragma_update(None,"journal_mode","DELETE").unwrap();
        drop(connection);
        fs::create_dir(directory.join("last-version")).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        assert!(matches!(store.handle(StoreRequest::List).unwrap(),StoreResponse::List(items) if !items.is_empty()));
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn failed_snapshot_persists_changes_and_retries_on_the_next_start() {
        use std::os::unix::fs::PermissionsExt;
        let directory = seed();
        let path = directory.join("workspaces.sqlite3");
        let store = SqliteStore::open(&path).unwrap();
        let count = copies(&store.connection).unwrap().len();
        drop(store);
        let keeper = Connection::open(&path).unwrap();
        keeper.execute_batch("PRAGMA wal_autocheckpoint=0; UPDATE items SET lease_until='123';").unwrap();
        fs::write(directory.join("last-version"),"older").unwrap();
        fs::set_permissions(&directory,fs::Permissions::from_mode(0o500)).unwrap();
        let (controller,mut host) = crate::controller_tests::start_native(&directory);
        assert!(!controller.manager.in_memory());
        let mut editing = *controller.manager.editing().unwrap();
        editing.region_smooth_edges = true;
        host.session.restore_editing(editing.clone()).unwrap();
        controller.manager.observe_editing(editing.clone());
        pollster::block_on(controller.manager.flush()).unwrap();
        crate::controller_tests::close_native(controller,&mut host);
        fs::set_permissions(&directory,fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(fs::read_to_string(directory.join("last-version")).unwrap(),"older");
        let stored: String = keeper.query_row("SELECT json FROM editing",[],|row|row.get(0)).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&stored).unwrap()["region_smooth_edges"].as_bool().unwrap());
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(copies(&store.connection).unwrap().len(),count+1);
        assert_eq!(read_marker(&directory.join("last-version")).unwrap().version,snapshots::VERSION);
        assert_eq!(store.connection.query_row("SELECT json FROM editing",[],|row|row.get::<_,String>(0)).unwrap(),stored);
        drop(store);
        drop(keeper);
        fs::remove_dir_all(directory).unwrap();
    }
}
