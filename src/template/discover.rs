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
    /// `<home>/.config/minuta/templates`; `None` when the home directory is unknown.
    pub user: Option<PathBuf>,
    pub project: PathBuf,
}

impl Dirs {
    /// The real locations: the user's config folder and `./templates`.
    pub fn standard() -> Result<Dirs> {
        let user =
            directories::BaseDirs::new().map(|d| d.home_dir().join(".config/minuta/templates"));
        Ok(Dirs {
            user,
            project: std::env::current_dir()
                .map_err(|e| {
                    Error::Io(io::Error::new(
                        e.kind(),
                        format!("cannot determine the current directory: {e}"),
                    ))
                })?
                .join("templates"),
        })
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
        load_layer(&mut catalog, user, Layer::User)?;
    }
    load_layer(&mut catalog, &dirs.project, Layer::Project)?;
    Ok(catalog)
}

fn load_layer(catalog: &mut Catalog, dir: &Path, layer: Layer) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_err(dir, e)),
    };
    let canonical_dir = fs::canonicalize(dir).map_err(|e| io_err(dir, e))?;
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
            Ok(m) if m.is_file() => load_file(catalog, path, layer, &canonical_dir),
            Ok(_) => {}
            Err(e) => catalog.invalid.push(Invalid {
                error: io_err(&path, e).to_string(),
                path,
                layer,
            }),
        }
    }
    Ok(())
}

/// Read one file. Problems with the file itself go to `catalog.invalid`.
fn load_file(catalog: &mut Catalog, path: PathBuf, layer: Layer, canonical_dir: &Path) {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else {
        let error = "file name is not valid UTF-8".to_string();
        catalog.invalid.push(Invalid { path, layer, error });
        return;
    };
    let result = if valid_stem(&stem) {
        read_template(&stem, &path, layer, canonical_dir)
    } else {
        Err(invalid_name_message(&stem))
    };
    match result {
        Ok(t) => {
            catalog.templates.insert(stem, t);
        }
        Err(error) => {
            // The broken file shadows any template of the same name from a lower layer.
            catalog.templates.remove(&stem);
            catalog.invalid.push(Invalid { path, layer, error });
        }
    }
}

fn read_template(
    stem: &str,
    path: &Path,
    layer: Layer,
    canonical_dir: &Path,
) -> std::result::Result<Template, String> {
    let target = fs::canonicalize(path).map_err(|e| io_err(path, e).to_string())?;
    if !target.starts_with(canonical_dir) {
        return Err(format!(
            "{}: symlink points outside the templates directory",
            path.display()
        ));
    }
    let bytes = fs::read(path).map_err(|e| io_err(path, e).to_string())?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("template {stem}: {} is not valid UTF-8", path.display()))?;
    Template::parse(stem, layer, Some(path.to_path_buf()), &text).map_err(|e| match e {
        Error::Usage(m) => m,
        other => other.to_string(),
    })
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
