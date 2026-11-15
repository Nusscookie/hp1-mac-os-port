//! All packages of an installed game, opened on demand, with cross-package reference resolution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::{Error, ObjRef, Package, Result};

pub const PACKAGE_EXTENSIONS: &[&str] = &["u", "unr", "utx", "uax", "umx", "usx"];
pub const PACKAGE_DIRS: &[&str] = &["System", "Maps", "Textures", "Sounds", "Music"];

/// A resolved object: the package that holds it and its export index there.
#[derive(Clone)]
pub struct ObjectHandle {
    pub package: Arc<Package>,
    pub export: usize,
}

impl ObjectHandle {
    pub fn export(&self) -> &crate::Export {
        &self.package.exports[self.export]
    }
    pub fn name(&self) -> &str {
        self.package.name(self.export().name)
    }
    pub fn class_name(&self) -> &str {
        self.package.class_name(self.export())
    }
    pub fn path(&self) -> String {
        self.package.ref_path(ObjRef::Export(self.export))
    }
}

pub struct Library {
    files: HashMap<String, PathBuf>,
    open: Mutex<HashMap<String, Arc<Package>>>,
}

impl Library {
    /// Indexes every package file in the game folder's standard subfolders. Language variants
    /// such as `AllDialog.ger_uax` are listed under their full file name only, so plain names
    /// always mean the installed language.
    pub fn scan(game: &Path) -> Result<Self> {
        let mut files = HashMap::new();
        for path in package_files(game)? {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if PACKAGE_EXTENSIONS.contains(&ext.as_str()) {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
                files.entry(stem).or_insert(path);
            }
        }
        if files.is_empty() {
            return Err(Error::Format(format!("no Unreal packages found under {}", game.display())));
        }
        Ok(Self { files, open: Mutex::new(HashMap::new()) })
    }

    pub fn package_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.files.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn path_of(&self, name: &str) -> Option<&Path> {
        self.files.get(&name.to_ascii_lowercase()).map(PathBuf::as_path)
    }

    /// True if `e` came from asking for a package that isn't installed.
    pub fn is_missing_package(&self, e: &Error) -> bool {
        matches!(e, Error::MissingPackage(_))
    }

    pub fn package(&self, name: &str) -> Result<Arc<Package>> {
        let key = name.to_ascii_lowercase();
        if let Some(p) = self.open.lock().expect("library lock").get(&key) {
            return Ok(p.clone());
        }
        let path = self.files.get(&key).ok_or_else(|| Error::MissingPackage(name.to_owned()))?;
        let pkg = Arc::new(Package::open(path)?);
        self.open.lock().expect("library lock").entry(key).or_insert(pkg.clone());
        Ok(pkg)
    }

    /// Follows a reference made inside `pkg` to the export it names, opening other packages as
    /// needed. Returns `None` for a null reference, and an error if the target can't be found.
    pub fn resolve(&self, pkg: &Arc<Package>, r: ObjRef) -> Result<Option<ObjectHandle>> {
        match r {
            ObjRef::None => Ok(None),
            ObjRef::Export(i) => Ok(Some(ObjectHandle { package: pkg.clone(), export: i })),
            ObjRef::Import(i) => {
                // Name chain from the outermost (the package) down to the object.
                let mut chain = Vec::new();
                let mut cur = ObjRef::Import(i);
                while let ObjRef::Import(j) = cur {
                    chain.push(pkg.name(pkg.imports[j].name).to_owned());
                    cur = pkg.imports[j].outer;
                    if chain.len() > 32 {
                        return Err(Error::Format("import outer chain too long".into()));
                    }
                }
                chain.reverse();
                let class = pkg.name(pkg.imports[i].class_name).to_owned();
                let (package_name, path) = chain.split_first().expect("chain has the import itself");
                if path.is_empty() {
                    return Err(Error::Format(format!("{package_name} is a package, not an object")));
                }
                let target = self.package(package_name)?;
                let found = find_by_path(&target, path, Some(&class))
                    .or_else(|| find_by_path(&target, path, None))
                    .ok_or_else(|| Error::Format(format!("{} not found", chain.join("."))))?;
                Ok(Some(ObjectHandle { package: target, export: found }))
            }
        }
    }

    /// Looks up `Package.Group.Name` (groups optional).
    pub fn find(&self, path: &str) -> Result<ObjectHandle> {
        let parts: Vec<String> = path.split('.').map(str::to_owned).collect();
        let (package_name, rest) =
            parts.split_first().ok_or_else(|| Error::Format(format!("bad object path {path}")))?;
        let pkg = self.package(package_name)?;
        let found = find_by_path(&pkg, rest, None)
            .or_else(|| rest.last().and_then(|n| pkg.find_export(n, None)))
            .ok_or_else(|| Error::Format(format!("{path} not found")))?;
        Ok(ObjectHandle { package: pkg, export: found })
    }
}

/// Finds an export whose name and chain of outers (groups) match `path`.
fn find_by_path(pkg: &Package, path: &[String], class: Option<&str>) -> Option<usize> {
    let (name, groups) = path.split_last()?;
    pkg.exports.iter().enumerate().find_map(|(i, e)| {
        if !pkg.name(e.name).eq_ignore_ascii_case(name) {
            return None;
        }
        if class.is_some_and(|c| !pkg.class_name(e).eq_ignore_ascii_case(c)) {
            return None;
        }
        let mut outer = e.outer;
        for g in groups.iter().rev() {
            match outer {
                ObjRef::Export(o) if pkg.name(pkg.exports[o].name).eq_ignore_ascii_case(g) => {
                    outer = pkg.exports[o].outer;
                }
                _ => return None,
            }
        }
        outer.is_none().then_some(i)
    })
}

/// Every package file (including language variants like `.ger_uax`) in the game folder.
pub fn package_files(game: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for dir in PACKAGE_DIRS {
        let Ok(entries) = std::fs::read_dir(game.join(dir)) else { continue };
        for entry in entries {
            let path = entry?.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let base_ext = ext.rsplit('_').next().unwrap_or("");
            if path.is_file()
                && (PACKAGE_EXTENSIONS.contains(&ext.as_str()) || PACKAGE_EXTENSIONS.contains(&base_ext))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}
