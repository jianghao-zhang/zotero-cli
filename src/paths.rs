use std::{env, path::PathBuf};

pub fn package_root() -> PathBuf {
    env::var_os("ZCLI_PACKAGE_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}
