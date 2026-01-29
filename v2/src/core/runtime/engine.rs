use rquickjs::{
    loader::{Resolver, ScriptLoader},
    Context, Ctx, Error, Function, Module, Runtime,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
struct LocalOnlyResolver {
    root: PathBuf,
}

impl Resolver for LocalOnlyResolver {
    fn resolve<'js>(&mut self, _ctx: &Ctx<'js>, base: &str, name: &str) -> rquickjs::Result<String> {
        // Only allow relative imports
        if !(name.starts_with("./") || name.starts_with("../")) {
            return Err(Error::new_resolving_message(base, name, "only relative imports are allowed"));
        }

        let base_dir = Path::new(base).parent().unwrap_or(&self.root);

        let resolved = base_dir.join(name);
        let resolved = resolved.canonicalize().map_err(|e| {
            Error::new_resolving_message(base, name, format!("cannot resolve import: {e}"))
        })?;

        if !resolved.starts_with(&self.root) {
            return Err(Error::new_resolving_message(base, name, "import escapes agent root"));
        }

        if resolved.extension().and_then(|s| s.to_str()) != Some("js") {
            return Err(Error::new_resolving_message(base, name, "only .js modules are allowed"));
        }

        Ok(resolved.to_string_lossy().to_string())
    }
}

pub fn run_script(entry_arg: String) -> rquickjs::Result<()> {
    let entry_path = PathBuf::from(&entry_arg)
        .canonicalize()
        .map_err(|e| Error::new_loading_message(entry_arg.clone(), format!("entry not found: {e}")))?;

    let root = entry_path
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| Error::new_loading_message(entry_path.to_string_lossy(), format!("bad root: {e}")))?;

    let entry_source = fs::read_to_string(&entry_path)
        .map_err(|e| Error::new_loading_message(entry_path.to_string_lossy(), format!("cannot read entry: {e}")))?;

    let rt = Runtime::new()?;
    rt.set_loader(LocalOnlyResolver { root }, ScriptLoader::default());

    let ctx = Context::full(&rt)?;
    ctx.with(|ctx| {
        let print_func = Function::new(ctx.clone(), |msg: String| {
            println!("{}", msg);
        })?;
        ctx.globals().set("print", print_func)?;

        let entry_name: String = entry_path.to_string_lossy().into_owned();

        let p = Module::evaluate(ctx, entry_name, entry_source)?;
        p.finish::<()>()?;

        Ok(())
    })
}
