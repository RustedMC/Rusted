//! Resource packs sent to players on join. Drop `.mcpack` / `.zip` files, or pack folders,
//! into `resource_packs/` next to the server. Each needs a `manifest.json` (its uuid and
//! version come from there). Players download them while joining, like on any Bedrock server.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::sync::OnceLock;

pub struct Pack {
    pub uuid: uuid::Uuid,
    /// "1.0.0", from the manifest
    pub version: String,
    /// The pack as a zip, as it's sent
    pub bytes: Vec<u8>,
    pub sha256: Vec<u8>,
    pub name: String,
}

impl Pack {
    /// How the game names a pack in download requests: "uuid_version".
    pub fn id(&self) -> String {
        format!("{}_{}", self.uuid, self.version)
    }
}

/// Bytes per download chunk (what vanilla servers use).
pub const CHUNK: usize = 1024 * 1024;

const DIR: &str = "resource_packs";

/// Packs loaded once at start-up.
pub fn all() -> &'static [Pack] {
    static PACKS: OnceLock<Vec<Pack>> = OnceLock::new();
    PACKS.get_or_init(|| {
        let mut out = Vec::new();
        let Ok(dir) = std::fs::read_dir(DIR) else { return out };
        let mut paths: Vec<_> = dir.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let bytes = if path.is_dir() {
                zip_folder(&path)
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mcpack") || e.eq_ignore_ascii_case("zip")) {
                std::fs::read(&path).ok()
            } else {
                continue;
            };
            match bytes.and_then(|b| load(b, &path)) {
                Some(p) => {
                    println!("Resource pack: {} {} ({} KB)", p.name, p.version, p.bytes.len() / 1024);
                    out.push(p);
                }
                None => println!("Skipped {}: not a pack with a manifest.json", path.display()),
            }
        }
        out
    })
}

pub fn find(id: &str) -> Option<&'static Pack> {
    let uuid = id.split('_').next()?;
    all().iter().find(|p| p.uuid.to_string() == uuid)
}

fn load(bytes: Vec<u8>, path: &std::path::Path) -> Option<Pack> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).ok()?;
    // The manifest may sit at the top or in one folder down (zips made of a folder).
    let name = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string())).filter(|n| n.ends_with("manifest.json")).min_by_key(|n| n.len())?;
    let mut text = String::new();
    zip.by_name(&name).ok()?.read_to_string(&mut text).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&strip_comments(&text)).ok()?;
    let header = &manifest["header"];
    let uuid = uuid::Uuid::parse_str(header["uuid"].as_str()?).ok()?;
    let version = header["version"].as_array()?.iter().filter_map(|v| v.as_u64()).map(|v| v.to_string()).collect::<Vec<_>>().join(".");
    let sha256 = Sha256::digest(&bytes).to_vec();
    let name = header["name"].as_str().map(str::to_string).unwrap_or_else(|| path.file_stem().map_or("pack".into(), |s| s.to_string_lossy().into_owned()));
    Some(Pack { uuid, version, bytes, sha256, name })
}

/// Packs often have // comments in their JSON, which serde_json doesn't accept.
fn strip_comments(text: &str) -> String {
    text.lines().map(|l| match l.find("//") {
        Some(i) if !l[..i].contains('"') || l[..i].matches('"').count() % 2 == 0 => &l[..i],
        _ => l,
    }).collect::<Vec<_>>().join("\n")
}

/// Zips a pack folder in memory, with paths relative to it.
fn zip_folder(dir: &std::path::Path) -> Option<Vec<u8>> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).ok()?.filter_map(Result::ok) {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path.strip_prefix(dir).ok()?.to_string_lossy().replace('\\', "/");
            out.start_file(rel, options).ok()?;
            out.write_all(&std::fs::read(&path).ok()?).ok()?;
        }
    }
    Some(out.finish().ok()?.into_inner())
}

#[cfg(test)]
mod tests {
    #[test]
    fn folder_packs_load() {
        let dir = std::env::temp_dir().join(format!("rusted-pack-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("textures")).unwrap();
        std::fs::write(dir.join("manifest.json"), r#"{
            // packs often have comments
            "format_version": 2,
            "header": { "name": "Test", "uuid": "11111111-2222-3333-4444-555555555555", "version": [1, 2, 3] },
            "modules": [{ "type": "resources", "uuid": "11111111-2222-3333-4444-666666666666", "version": [1, 2, 3] }]
        }"#).unwrap();
        std::fs::write(dir.join("textures/a.txt"), b"hi").unwrap();
        let bytes = super::zip_folder(&dir).unwrap();
        let p = super::load(bytes, &dir).expect("loads");
        assert_eq!(p.version, "1.2.3");
        assert_eq!(p.id(), "11111111-2222-3333-4444-555555555555_1.2.3");
        assert_eq!(p.sha256.len(), 32);
        let _ = std::fs::remove_dir_all(dir);
    }
}
