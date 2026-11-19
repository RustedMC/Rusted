//! World backups: every `backup-interval` minutes the level folder is copied to
//! backups/<level>-<unix time>/, and only the newest `backup-keep` are kept.

use std::path::Path;

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<u64> {
    std::fs::create_dir_all(to)?;
    let mut bytes = 0;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            bytes += copy_dir(&e.path(), &target)?;
        } else if e.path().extension().is_none_or(|x| x != "tmp") {
            bytes += std::fs::copy(e.path(), target)?;
        }
    }
    Ok(bytes)
}

/// Copies `level_dir` into `root` (backups/) and drops the oldest past `keep`. Returns the new folder.
pub fn run(level_dir: &Path, root: &Path, keep: usize) -> std::io::Result<std::path::PathBuf> {
    let name = level_dir.file_name().map_or("world".into(), |n| n.to_string_lossy().into_owned());
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let dest = root.join(format!("{name}-{stamp}"));
    copy_dir(level_dir, &dest)?;
    // Oldest first by the time in the name; remove until only `keep` are left.
    let mut all: Vec<_> = std::fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let t: u64 = n.strip_prefix(&format!("{name}-"))?.parse().ok()?;
            Some((t, e.path()))
        })
        .collect();
    all.sort();
    while all.len() > keep {
        let (_, old) = all.remove(0);
        let _ = std::fs::remove_dir_all(old);
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    #[test]
    fn copies_and_prunes() {
        let tmp = std::env::temp_dir().join(format!("rusted-backup-test-{}", std::process::id()));
        let level = tmp.join("lvl");
        std::fs::create_dir_all(level.join("sub")).unwrap();
        std::fs::write(level.join("a.dat"), b"x").unwrap();
        std::fs::write(level.join("sub/b.dat"), b"y").unwrap();
        let root = tmp.join("backups");
        let first = super::run(&level, &root, 1).unwrap();
        assert!(first.join("sub/b.dat").exists());
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = super::run(&level, &root, 1).unwrap();
        assert!(second.exists() && !first.exists(), "only the newest is kept");
        let _ = std::fs::remove_dir_all(tmp);
    }
}
