use rquickjs::{Ctx, Error, loader::Resolver};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct LocalOnlyResolver {
    pub root: PathBuf,
}

impl Resolver for LocalOnlyResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
    ) -> rquickjs::Result<String> {
        resolve_local_module(&self.root, base, name)
            .map_err(|msg| Error::new_resolving_message(base, name, msg))
    }
}

fn resolve_local_module(
    root: &Path,
    base: &str,
    name: &str,
) -> std::result::Result<String, String> {
    if !(name.starts_with("./") || name.starts_with("../")) {
        return Err("only relative imports are allowed".to_string());
    }

    let base_dir = Path::new(base).parent().unwrap_or(root);

    let resolved = base_dir.join(name);
    let resolved = resolved
        .canonicalize()
        .map_err(|e| format!("cannot resolve import: {e}"))?;

    if !resolved.starts_with(root) {
        return Err("import escapes agent root".to_string());
    }

    if resolved.extension().and_then(|s| s.to_str()) != Some("js") {
        return Err("only .js modules are allowed".to_string());
    }

    Ok(resolved.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::resolve_local_module;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolves_relative_js_import_within_root() {
        let dir = tempdir().expect("tempdir");
        let root = fs::canonicalize(dir.path()).expect("canonicalize root");
        let app = root.join("main.js");
        let lib = root.join("lib");
        fs::create_dir_all(&lib).expect("mkdir lib");
        let util = lib.join("util.js");
        fs::write(&app, "// app").expect("write app");
        fs::write(&util, "// util").expect("write util");

        let resolved = resolve_local_module(&root, app.to_string_lossy().as_ref(), "./lib/util.js")
            .expect("should resolve");
        assert_eq!(resolved, util.to_string_lossy());
    }

    #[test]
    fn rejects_non_relative_imports() {
        let dir = tempdir().expect("tempdir");
        let root = fs::canonicalize(dir.path()).expect("canonicalize root");
        let app = root.join("main.js");
        fs::write(&app, "// app").expect("write app");

        let err = resolve_local_module(&root, app.to_string_lossy().as_ref(), "fs")
            .expect_err("should fail");
        assert!(err.contains("only relative imports are allowed"));
    }

    #[test]
    fn rejects_import_escape_outside_root() {
        let root_dir = tempdir().expect("root tempdir");
        let outside_dir = tempdir().expect("outside tempdir");
        let root = fs::canonicalize(root_dir.path()).expect("canonicalize root");
        let outside = fs::canonicalize(outside_dir.path()).expect("canonicalize outside");

        let app_dir = root.join("app");
        fs::create_dir_all(&app_dir).expect("mkdir app");
        let app = app_dir.join("main.js");
        fs::write(&app, "// app").expect("write app");

        let secret = outside.join("secret.js");
        fs::write(&secret, "// secret").expect("write secret");

        let rel = format!(
            "../{}/secret.js",
            outside.file_name().expect("name").to_string_lossy()
        );
        let err = resolve_local_module(&root, app.to_string_lossy().as_ref(), &rel)
            .expect_err("should fail");
        assert!(err.contains("cannot resolve import") || err.contains("import escapes agent root"));
    }

    #[test]
    fn rejects_non_js_extension() {
        let dir = tempdir().expect("tempdir");
        let root = fs::canonicalize(dir.path()).expect("canonicalize root");
        let app = root.join("main.js");
        let ts_mod = root.join("mod.ts");
        fs::write(&app, "// app").expect("write app");
        fs::write(&ts_mod, "// ts").expect("write ts");

        let err = resolve_local_module(&root, app.to_string_lossy().as_ref(), "./mod.ts")
            .expect_err("should fail");
        assert!(err.contains("only .js modules are allowed"));
    }
}
