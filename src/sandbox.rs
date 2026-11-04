use std::path::{Component, Path, PathBuf};

/// Allowlist of canonical root directories. Every read goes through `check`.
pub struct Sandbox {
    roots: Vec<PathBuf>,
}

impl Sandbox {
    /// Roots that do not exist are skipped (nothing under them can then be read).
    pub fn new(roots: &[PathBuf]) -> Sandbox {
        Sandbox {
            roots: roots.iter().filter_map(|r| r.canonicalize().ok()).collect(),
        }
    }

    /// Resolve `p` (following symlinks) and require it to be an existing regular
    /// file inside one of the roots. Any `..` component is rejected up front.
    pub fn check_file(&self, p: &Path) -> Result<PathBuf, String> {
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err("path rejected: contains '..'".into());
        }
        let canon = p
            .canonicalize()
            .map_err(|_| "path rejected: not found".to_string())?;
        if !self.roots.iter().any(|r| canon.starts_with(r)) {
            return Err("path rejected: outside the allowlisted roots".into());
        }
        if !canon.is_file() {
            return Err("path rejected: not a regular file".into());
        }
        Ok(canon)
    }

    pub fn check_dir(&self, p: &Path) -> Result<PathBuf, String> {
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err("path rejected: contains '..'".into());
        }
        let canon = p
            .canonicalize()
            .map_err(|_| "path rejected: not found".to_string())?;
        if !self.roots.iter().any(|r| canon.starts_with(r)) || !canon.is_dir() {
            return Err("path rejected: outside the allowlisted roots".into());
        }
        Ok(canon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn allows_inside_rejects_outside_and_dotdot() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.md"), "x").unwrap();
        fs::write(other.path().join("b.md"), "x").unwrap();
        fs::create_dir(root.path().join("sub")).unwrap();
        let sb = Sandbox::new(&[root.path().to_path_buf()]);

        assert!(sb.check_file(&root.path().join("a.md")).is_ok());
        assert!(sb.check_file(&other.path().join("b.md")).is_err());
        assert!(sb.check_file(&root.path().join("missing.md")).is_err());
        assert!(sb.check_file(&root.path().join("sub")).is_err());

        let sneaky = root.path().join("sub").join("..").join("a.md");
        let err = sb.check_file(&sneaky).unwrap_err();
        assert!(err.contains(".."));
        let escape = root
            .path()
            .join("..")
            .join(other.path().file_name().unwrap());
        assert!(sb.check_file(&escape).is_err());
    }

    #[test]
    fn missing_root_allows_nothing() {
        let sb = Sandbox::new(&[PathBuf::from("definitely/not/here")]);
        assert!(sb.check_file(Path::new("Cargo.toml")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_rejected() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        fs::write(other.path().join("secret.md"), "x").unwrap();
        std::os::unix::fs::symlink(other.path().join("secret.md"), root.path().join("link.md"))
            .unwrap();
        std::os::unix::fs::symlink(other.path(), root.path().join("dirlink")).unwrap();
        let sb = Sandbox::new(&[root.path().to_path_buf()]);
        assert!(sb.check_file(&root.path().join("link.md")).is_err());
        assert!(sb
            .check_file(&root.path().join("dirlink").join("secret.md"))
            .is_err());
    }
}
