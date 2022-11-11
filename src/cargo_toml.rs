use anyhow::{Context, Result};
use fs_err as fs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// The `[lib]` section of a Cargo.toml
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CargoTomlLib {
    pub(crate) crate_type: Option<Vec<String>>,
    pub(crate) name: Option<String>,
}

/// The `[package]` section of a Cargo.toml
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CargoTomlPackage {
    pub(crate) name: String,
    metadata: Option<CargoTomlMetadata>,
}

/// Extract of the Cargo.toml that can be reused for the python metadata
///
/// See https://doc.rust-lang.org/cargo/reference/manifest.html for a specification
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CargoToml {
    pub(crate) lib: Option<CargoTomlLib>,
    pub(crate) package: CargoTomlPackage,
}

impl CargoToml {
    /// Reads and parses the Cargo.toml at the given location
    pub fn from_path(manifest_file: impl AsRef<Path>) -> Result<Self> {
        let contents = fs::read_to_string(&manifest_file).context(format!(
            "Can't read Cargo.toml at {}",
            manifest_file.as_ref().display(),
        ))?;
        let cargo_toml = toml_edit::easy::from_str(&contents).context(format!(
            "Failed to parse Cargo.toml at {}",
            manifest_file.as_ref().display()
        ))?;
        Ok(cargo_toml)
    }

    /// Returns the value of `[project.metadata.maturin]` or an empty stub
    pub fn remaining_core_metadata(&self) -> RemainingCoreMetadata {
        match &self.package.metadata {
            Some(CargoTomlMetadata {
                maturin: Some(extra_metadata),
            }) => extra_metadata.clone(),
            _ => Default::default(),
        }
    }

    /// Warn about removed python metadata support in `Cargo.toml`
    pub fn warn_removed_python_metadata(&self) -> bool {
        let mut removed = Vec::new();
        if let Some(CargoTomlMetadata {
            maturin: Some(extra_metadata),
        }) = &self.package.metadata
        {
            let removed_keys = [
                "scripts",
                "classifiers",
                "classifier",
                "maintainer",
                "maintainer-email",
                "requires-dist",
                "requires-python",
                "requires-external",
                "project-url",
                "provides-extra",
                "description-content-type",
            ];
            for key in removed_keys {
                if extra_metadata.other.contains_key(key) {
                    removed.push(key);
                }
            }
        }
        if !removed.is_empty() {
            eprintln!(
                "⚠️  Warning: the following metadata fields in `package.metadata.maturin` section \
                of Cargo.toml are removed since maturin 0.14.0: {}, \
                please set them in pyproject.toml as PEP 621 specifies.",
                removed.join(", ")
            );
            true
        } else {
            false
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
struct CargoTomlMetadata {
    maturin: Option<RemainingCoreMetadata>,
}

/// The `[project.metadata.maturin]` with the maturin specific metadata
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct RemainingCoreMetadata {
    pub name: Option<String>,
    /// The directory with python module, contains `<module_name>/__init__.py`
    pub python_source: Option<String>,
    /// The directory containing the wheel data
    pub data: Option<String>,
    #[serde(flatten)]
    pub other: HashMap<String, toml_edit::easy::Value>,
}

#[cfg(test)]
mod test {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_metadata_from_cargo_toml() {
        let cargo_toml = indoc!(
            r#"
            [package]
            authors = ["konstin <konstin@mailbox.org>"]
            name = "info-project"
            version = "0.1.0"
            description = "A test project"
            homepage = "https://example.org"
            keywords = ["ffi", "test"]

            [lib]
            crate-type = ["cdylib"]
            name = "pyo3_pure"

            [package.metadata.maturin.scripts]
            ph = "maturin:print_hello"

            [package.metadata.maturin]
            classifiers = ["Programming Language :: Python"]
            requires-dist = ["flask~=1.1.0", "toml==0.10.0"]
        "#
        );

        let cargo_toml: Result<CargoToml, _> = toml_edit::easy::from_str(cargo_toml);
        assert!(cargo_toml.is_ok());
    }

    #[test]
    fn test_metadata_from_cargo_toml_without_authors() {
        let cargo_toml = indoc!(
            r#"
            [package]
            name = "info-project"
            version = "0.1.0"
            description = "A test project"
            homepage = "https://example.org"
            keywords = ["ffi", "test"]

            [lib]
            crate-type = ["cdylib"]
            name = "pyo3_pure"

            [package.metadata.maturin.scripts]
            ph = "maturin:print_hello"

            [package.metadata.maturin]
            classifiers = ["Programming Language :: Python"]
            requires-dist = ["flask~=1.1.0", "toml==0.10.0"]
        "#
        );

        let cargo_toml: Result<CargoToml, _> = toml_edit::easy::from_str(cargo_toml);
        assert!(cargo_toml.is_ok());
    }
}
