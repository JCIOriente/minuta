//! Layered template discovery.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{builtins, invalid_name_message, valid_stem, Layer, Template};
use crate::error::{Error, Result};

/// Where user and project templates live.
#[derive(Debug, Clone)]
pub struct Dirs {
    /// `<home>/.minuta/templates`; `None` when the home directory is unknown.
    pub user: Option<PathBuf>,
    pub project: PathBuf,
}

impl Dirs {
    /// The real locations: the user's `~/.minuta/templates` and `./templates`.
    pub fn standard() -> Result<Dirs> {
        let cwd = std::env::current_dir().map_err(|e| {
            Error::Io(io::Error::new(
                e.kind(),
                format!("cannot determine the current directory: {e}"),
            ))
        })?;
        let mut dirs = Dirs::for_home(Path::new(""), &cwd);
        dirs.user =
            directories::BaseDirs::new().and_then(|d| Dirs::for_home(d.home_dir(), &cwd).user);
        Ok(dirs)
    }

    /// The locations for a given home directory and working directory.
    pub fn for_home(home: &Path, cwd: &Path) -> Dirs {
        Dirs {
            user: Some(home.join(".minuta/templates")),
            project: cwd.join("templates"),
        }
    }
}

/// A template file that could not be loaded.
#[derive(Debug, Clone)]
pub struct Invalid {
    pub path: PathBuf,
    pub layer: Layer,
    pub error: String,
}

#[derive(Debug, Default)]
pub struct Catalog {
    pub templates: BTreeMap<String, Template>,
    pub invalid: Vec<Invalid>,
}

fn io_err(path: &Path, e: io::Error) -> Error {
    Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

/// Load built-in, then user, then project templates; a later layer replaces an
/// earlier one by name.
pub fn discover(dirs: &Dirs) -> Result<Catalog> {
    let mut catalog = Catalog::default();
    for t in builtins()? {
        catalog.templates.insert(t.name.clone(), t);
    }
    if let Some(user) = &dirs.user {
        load_layer(&mut catalog, user, Layer::User, false)?;
    }
    // The project layer is untrusted (a cloned repo). If `./templates` is itself a
    // symlink it is not loaded at all: whatever it points at could be a broad
    // directory (`templates -> .`), and no containment rule on its files would
    // then protect anything. Inside a real directory, a file's symlink target must
    // stay inside that directory. User-layer symlinks (dotfiles, nix store) are
    // followed.
    load_layer(&mut catalog, &dirs.project, Layer::Project, true)?;
    Ok(catalog)
}

fn load_layer(catalog: &mut Catalog, dir: &Path, layer: Layer, untrusted: bool) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_err(dir, e)),
    };
    let canonical_dir = fs::canonicalize(dir).map_err(|e| io_err(dir, e))?;
    let dir_outside = untrusted
        && fs::symlink_metadata(dir)
            .map_err(|e| io_err(dir, e))?
            .file_type()
            .is_symlink();
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| io_err(dir, e))?.path();
        if path.extension().map_or(true, |e| e != "md") {
            continue;
        }
        paths.push(path);
    }
    paths.sort();
    for path in paths {
        match fs::metadata(&path) {
            Ok(m) if m.is_file() => {
                let confine = untrusted.then_some(canonical_dir.as_path());
                load_file(catalog, path, layer, confine, dir_outside)
            }
            Ok(_) => {}
            Err(e) => {
                let error = io_err(&path, e).to_string();
                record_invalid(catalog, path, layer, error);
            }
        }
    }
    Ok(())
}

/// Record a broken file. It shadows any template of the same name from a lower layer.
fn record_invalid(catalog: &mut Catalog, path: PathBuf, layer: Layer, error: String) {
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        catalog.templates.remove(stem);
    }
    catalog.invalid.push(Invalid { path, layer, error });
}

/// Read one file. Problems with the file itself go to `catalog.invalid`.
fn load_file(
    catalog: &mut Catalog,
    path: PathBuf,
    layer: Layer,
    confine: Option<&Path>,
    dir_outside: bool,
) {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else {
        let error = "file name is not valid UTF-8".to_string();
        catalog.invalid.push(Invalid { path, layer, error });
        return;
    };
    let result = if dir_outside {
        Err(format!(
            "{}: the project templates directory is a symlink; it is not loaded",
            path.display()
        ))
    } else if valid_stem(&stem) {
        read_template(&stem, &path, layer, confine)
    } else {
        Err(invalid_name_message(&stem))
    };
    match result {
        Ok(t) => {
            catalog.templates.insert(stem, t);
        }
        Err(error) => record_invalid(catalog, path, layer, error),
    }
}

fn read_template(
    stem: &str,
    path: &Path,
    layer: Layer,
    confine: Option<&Path>,
) -> std::result::Result<Template, String> {
    if let Some(dir) = confine {
        let target = fs::canonicalize(path).map_err(|e| io_err(path, e).to_string())?;
        if !target.starts_with(dir) {
            return Err(format!(
                "{}: symlink points outside the templates directory",
                path.display()
            ));
        }
        reject_hard_link(path)?;
    }
    let bytes = fs::read(path).map_err(|e| io_err(path, e).to_string())?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("template {stem}: {} is not valid UTF-8", path.display()))?;
    Template::parse(stem, layer, Some(path.to_path_buf()), &text).map_err(|e| match e {
        Error::Usage(m) => m,
        other => other.to_string(),
    })
}

/// A hard link to a file outside the untrusted directory canonicalizes to its
/// in-directory path, so the symlink check cannot see it. Reject any file with
/// more than one link. `fs::metadata` follows symlinks, so a symlink to a
/// hard-linked file is rejected too.
#[cfg(unix)]
fn reject_hard_link(path: &Path) -> std::result::Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(path).map_err(|e| io_err(path, e).to_string())?;
    if meta.nlink() > 1 {
        return Err(format!(
            "{}: file is hard-linked; hard links are not loaded from the project templates directory",
            path.display()
        ));
    }
    Ok(())
}

/// Not unix: std exposes no stable link count there, so the check is compiled out.
#[cfg(not(unix))]
fn reject_hard_link(_path: &Path) -> std::result::Result<(), String> {
    Ok(())
}

impl Catalog {
    /// Find a template by name. Unknown: usage error listing the names. Invalid:
    /// that file's error.
    pub fn get(&self, name: &str) -> Result<&Template> {
        if let Some(t) = self.templates.get(name) {
            return Ok(t);
        }
        let broken = self
            .invalid
            .iter()
            .rev()
            .find(|i| i.path.file_stem().is_some_and(|s| s == name));
        if let Some(i) = broken {
            return Err(Error::Usage(i.error.clone()));
        }
        let names: Vec<&str> = self.templates.keys().map(String::as_str).collect();
        Err(Error::Usage(format!(
            "unknown template '{name}' (available: {})",
            names.join(", ")
        )))
    }
}
