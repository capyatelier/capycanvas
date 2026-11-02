use layer_host::StorageRoots;
use std::path::Path;

fn copy(source: &Path, target: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() { copy(&entry.path(), &destination)?; }
        else { std::fs::copy(entry.path(), destination)?; }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::canonicalize(std::env::args_os().nth(1).ok_or("Expected a data folder")?)?;
    let directory = std::fs::canonicalize(std::env::temp_dir())?
        .join(format!("capy-saved-check-{}", layer_core::PortableId::random()));
    if directory.starts_with(&source) { return Err("Temporary storage must be outside the checked data folder".into()); }
    let result = (|| {
        copy(&source, &directory)?;
        let report = layer_workspace::check_saved_data(&StorageRoots::within(&directory));
        println!("{}", serde_json::to_string(&report)?);
        Ok(())
    })();
    std::fs::remove_dir_all(directory)?;
    result
}
