use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub cpu_model: String,
    pub logical_cores: usize,
    pub physical_cores: usize,
    pub memory_bytes: u64,
    pub disk_total_bytes: u64,
    pub disk_available_bytes: u64,
    pub gpus: Vec<GpuInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GpuInfo {
    pub name: String,
    pub memory: Option<String>,
}

pub struct SystemInspector;

impl SystemInspector {
    pub fn inspect() -> SystemInfo {
        let mut sys = System::new_all();
        sys.refresh_all();

        let os = System::name().unwrap_or_else(|| "Unknown".to_string());
        let os_ver = System::os_version().unwrap_or_else(|| "Unknown".to_string());

        let cpu = sys.cpus().first();
        let cpu_model = cpu
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let disks = Disks::new_with_refreshed_list();
        let (total_disk, avail_disk) = disks.list().iter().fold((0, 0), |(acc_t, acc_a), d| {
            (acc_t + d.total_space(), acc_a + d.available_space())
        });

        let mut gpus = Vec::new();
        if cfg!(target_os = "macos") {
            if std::env::consts::ARCH == "aarch64" {
                gpus.push(GpuInfo {
                    name: "Apple M-Series GPU".to_string(),
                    memory: None,
                });
            }
        } else if let Ok(output) = std::process::Command::new("nvidia-smi")
            .args(&["--query-gpu=name,memory.total", "--format=csv,noheader"])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            for line in s.lines() {
                let parts: Vec<&str> = line.split(',').collect();
                if !parts.is_empty() {
                    gpus.push(GpuInfo {
                        name: parts[0].trim().to_string(),
                        memory: if parts.len() > 1 {
                            Some(parts[1].trim().to_string())
                        } else {
                            None
                        },
                    });
                }
            }
        }

        SystemInfo {
            os: format!("{} {}", os, os_ver),
            arch: std::env::consts::ARCH.to_string(),
            cpu_model,
            logical_cores: sys.cpus().len(),
            physical_cores: sysinfo::System::physical_core_count().unwrap_or(sys.cpus().len()),
            memory_bytes: sys.total_memory(),
            disk_total_bytes: total_disk,
            disk_available_bytes: avail_disk,
            gpus,
        }
    }

    pub fn recommend_preset(info: &SystemInfo) -> &'static str {
        if info
            .gpus
            .iter()
            .any(|g| g.name.to_lowercase().contains("nvidia"))
        {
            return "cuda_dedicated";
        }
        if info.os.to_lowercase().contains("macos") || info.arch == "aarch64" {
            return "metal_unified";
        }
        "cpu_only"
    }
}
