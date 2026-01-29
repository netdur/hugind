use rquickjs::{loader::Resolver, Ctx, Error};
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
        // Only allow relative imports
        if !(name.starts_with("./") || name.starts_with("../")) {
            return Err(Error::new_resolving_message(
                base,
                name,
                "only relative imports are allowed",
            ));
        }

        let base_dir = Path::new(base)
            .parent()
            .unwrap_or(&self.root);

        let resolved = base_dir.join(name);
        let resolved = resolved.canonicalize().map_err(|e| {
            Error::new_resolving_message(
                base,
                name,
                format!("cannot resolve import: {e}"),
            )
        })?;

        // Prevent escaping the agent root
        if !resolved.starts_with(&self.root) {
            return Err(Error::new_resolving_message(
                base,
                name,
                "import escapes agent root",
            ));
        }

        // Restrict to .js files only
        if resolved.extension().and_then(|s| s.to_str()) != Some("js") {
            return Err(Error::new_resolving_message(
                base,
                name,
                "only .js modules are allowed",
            ));
        }

        Ok(resolved.to_string_lossy().into_owned())
    }
}
