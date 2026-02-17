use rquickjs::{class::Trace, AsyncContext, Class, Ctx, Result, Value};

use crate::core::config::agent::{AgentConfig, RuntimeFsMode};
use crate::core::fs::FsAccess;
use crate::shared::logging::RunLogger;

#[derive(rquickjs::JsLifetime)]
#[rquickjs::class]
pub struct Fs {
    access: FsAccess,
    fs_mode: RuntimeFsMode,
    logger: Option<RunLogger>,
}

impl<'js> Trace<'js> for Fs {
    fn trace<'a>(&self, _tracer: rquickjs::class::Tracer<'a, 'js>) {}
}

#[rquickjs::methods]
impl Fs {
    pub fn cwd(&self) -> Result<String> {
        self.ensure_host_fs_enabled()?;
        self.log("host.fs.cwd");
        Ok(self.access.cwd().to_string_lossy().into_owned())
    }

    pub fn exists(&self, path: String) -> Result<bool> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.exists path={}", path);
        self.log(&msg);
        Ok(self.access.exists(&path).map_err(fs_err)?)
    }

    pub fn is_file(&self, path: String) -> Result<bool> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.is_file path={}", path);
        self.log(&msg);
        Ok(self.access.is_file(&path).map_err(fs_err)?)
    }

    pub fn is_dir(&self, path: String) -> Result<bool> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.is_dir path={}", path);
        self.log(&msg);
        Ok(self.access.is_dir(&path).map_err(fs_err)?)
    }

    pub fn realpath(&self, path: String) -> Result<String> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.realpath path={}", path);
        self.log(&msg);
        Ok(self
            .access
            .realpath(&path)
            .map_err(fs_err)?
            .to_string_lossy()
            .into_owned())
    }

    pub fn read_text(&self, path: String) -> Result<String> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.read_text path={}", path);
        self.log(&msg);
        Ok(self.access.read_text(&path).map_err(fs_err)?)
    }

    pub fn read_bytes<'js>(&self, ctx: Ctx<'js>, path: String) -> Result<Value<'js>> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.read_bytes path={}", path);
        self.log(&msg);
        let bytes = self.access.read_bytes(&path).map_err(fs_err)?;
        let arr = rquickjs::Array::new(ctx)?;
        for (idx, b) in bytes.iter().enumerate() {
            arr.set(idx, *b as u32)?;
        }
        Ok(arr.into_value())
    }

    pub fn write_text(&self, path: String, data: String) -> Result<()> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.write_text path={} bytes={}", path, data.len());
        self.log(&msg);
        self.access
            .write_text(&path, &data, false)
            .map_err(fs_err)?;
        Ok(())
    }

    pub fn write_bytes(&self, path: String, data: Value<'_>) -> Result<()> {
        self.ensure_host_fs_enabled()?;
        let bytes = value_to_bytes(data)?;
        let msg = format!("host.fs.write_bytes path={} bytes={}", path, bytes.len());
        self.log(&msg);
        self.access
            .write_bytes(&path, &bytes, false)
            .map_err(fs_err)?;
        Ok(())
    }

    pub fn append_text(&self, path: String, data: String) -> Result<()> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.append_text path={} bytes={}", path, data.len());
        self.log(&msg);
        self.access.write_text(&path, &data, true).map_err(fs_err)?;
        Ok(())
    }

    pub fn list_dir(&self, path: String) -> Result<String> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.list_dir path={}", path);
        self.log(&msg);
        let entries = self.access.list_dir(&path).map_err(fs_err)?;
        let json = serde_json::to_string(&entries).map_err(|e| {
            rquickjs::Error::new_loading_message("Filesystem Error", e.to_string())
        })?;
        Ok(json)
    }

    pub fn mkdir(&self, path: String, recursive: bool) -> Result<()> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.mkdir path={} recursive={}", path, recursive);
        self.log(&msg);
        self.access.mkdir(&path, recursive).map_err(fs_err)?;
        Ok(())
    }

    pub fn remove(&self, path: String, recursive: bool) -> Result<()> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.remove path={} recursive={}", path, recursive);
        self.log(&msg);
        self.access.remove(&path, recursive).map_err(fs_err)?;
        Ok(())
    }

    pub fn rename(&self, src: String, dst: String) -> Result<()> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.rename src={} dst={}", src, dst);
        self.log(&msg);
        self.access.rename(&src, &dst).map_err(fs_err)?;
        Ok(())
    }

    pub fn copy(&self, src: String, dst: String) -> Result<()> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.copy src={} dst={}", src, dst);
        self.log(&msg);
        self.access.copy(&src, &dst).map_err(fs_err)?;
        Ok(())
    }

    pub fn stat(&self, path: String) -> Result<String> {
        self.ensure_host_fs_enabled()?;
        let msg = format!("host.fs.stat path={}", path);
        self.log(&msg);
        let stat = self.access.stat(&path).map_err(fs_err)?;
        let json = serde_json::to_string(&stat).map_err(|e| {
            rquickjs::Error::new_loading_message("Filesystem Error", e.to_string())
        })?;
        Ok(json)
    }

    fn ensure_host_fs_enabled(&self) -> Result<()> {
        match self.fs_mode {
            RuntimeFsMode::WasiMounts => Err(rquickjs::Error::new_loading_message(
                "Filesystem Error",
                "host filesystem access is disabled (runtime_fs_mode = wasi_mounts)",
            )),
            RuntimeFsMode::HostFilesystem | RuntimeFsMode::Both => Ok(()),
        }
    }

}

impl Fs {
    fn log(&self, msg: &str) {
        if let Some(logger) = &self.logger {
            logger.log_line(msg);
        }
    }
}

fn value_to_bytes(value: Value<'_>) -> Result<Vec<u8>> {
    if value.is_string() {
        let s: rquickjs::String = value.into_string().unwrap();
        return Ok(s.to_string()?.into_bytes());
    }

    if value.is_array() {
        let arr = value.into_array().unwrap();
        let mut out = Vec::with_capacity(arr.len());
        for i in 0..arr.len() {
            let v: Value = arr.get(i)?;
            if v.is_number() {
                let n = v.as_number().unwrap_or(0.0);
                let clamped = n.round().clamp(0.0, 255.0) as u8;
                out.push(clamped);
            } else {
                return Err(rquickjs::Error::new_loading_message(
                    "Filesystem Error",
                    "write_bytes array must contain numbers",
                ));
            }
        }
        return Ok(out);
    }

    Err(rquickjs::Error::new_loading_message(
        "Filesystem Error",
        "write_bytes expects a string or array of numbers",
    ))
}

fn fs_err(err: anyhow::Error) -> rquickjs::Error {
    rquickjs::Error::new_loading_message("Filesystem Error", err.to_string())
}

pub async fn install(
    ctx: &AsyncContext,
    config: &AgentConfig,
    fs_root: &std::path::Path,
    logger: Option<RunLogger>,
) -> Result<()> {
    let fs_mode = config
        .wasm
        .as_ref()
        .map(|w| w.runtime_fs_mode.clone())
        .unwrap_or_default();
    let fs_access = FsAccess::new(
        fs_root.to_path_buf(),
        config
            .permissions
            .as_ref()
            .and_then(|p| p.filesystem.clone()),
    );

    let fs = Fs {
        access: fs_access,
        fs_mode,
        logger,
    };

    ctx.async_with(|ctx| Box::pin(async move {
        let cls = Class::instance(ctx.clone(), fs)?;
        ctx.globals().set("fs", cls)?;
        Ok(())
    }))
    .await
}
