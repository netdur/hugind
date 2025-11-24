# Hugind Configuration Guide

The `hugind config` command is your toolkit for managing hardware profiles and server settings. It generates optimized configuration files used by the `hugind server` command.

## Commands

### 1. `hugind config info`
**"What does Hugind see?"**

Runs a system probe to detect your hardware (CPU, RAM, GPUs). Use this to verify if Hugind correctly identifies your hardware capabilities.

**Example Output:**
```text
System Information
------------------
OS: macos Version 14.4
CPU: Apple M3 Max (16c/16t)
Memory: 64.0 GB
GPUs: Apple M3 Max (Unified)
Recommendation: metal_unified
```

### 2. `hugind config init <name>`
**"Create a new configuration."**

Runs an interactive wizard to generate a `config.yml` file optimized for your specific machine and model.

**The Wizard Steps:**
1.  **Hardware Probe**: Detects system resources.
2.  **Preset Selection**: Applies a hardware profile:
    *   `metal_unified`: Apple Silicon (M1/M2/M3). Enables Flash Attention and mmap.
    *   `cuda_dedicated`: NVIDIA GPUs. Enables layer offloading and tensor splitting.
    *   `cpu_only`: Fallback. Enables aggressive quantization (`q8_0` KV cache) to save RAM.
3.  **Model Selection**: Selects a `.gguf` file from your library (managed via `hugind model`).
    *   *Auto-Detection*: Detects and links vision projectors (`mmproj`) and draft models automatically.
4.  **Context Calculation**:
    *   Formula: `System RAM - Model Size - 2GB (OS Buffer)`.
    *   Suggests the maximum safe context size (e.g., 8192, 32768) to prevent crashes.

**Example:**
```bash
hugind config init my_chat_bot
```

### 3. `hugind config list`
Lists all saved configurations found in `~/.hugind/configs/`.

### 4. `hugind config remove <name>`
Deletes a configuration file.

---

## Configuration File Structure

Hugind generates clean, structured YAML files. You can edit these manually after generation.

**Location:** `~/.hugind/configs/<name>.yml`

### Structure Overview
```yaml
# 1. Server Settings (Port, API Keys, Concurrency)
server:
  host: "0.0.0.0"
  port: 8080
  concurrency: 1      # Number of model instances
  max_slots: 4        # Max concurrent users per instance

# 2. Model Settings (Paths, GPU Offload)
model:
  path: "models/llama-3-8b.gguf"
  gpu_layers: 99
  split_mode: layer   # layer, row, or none

# 3. Context & Performance (Memory usage)
context:
  size: 8192
  batch_size: 2048
  flash_attention: enabled
  cache_type_k: q8_0  # Quantized Cache (Saves VRAM)
  cache_type_v: q8_0

# 4. Sampling Defaults (Creativity)
sampling:
  temp: 0.7
  top_k: 40
```

---

## Advanced: Customizing Templates

Hugind generates configs based on template files located in your system's share directory. You can edit these templates to change the defaults for all future `init` commands.

**Template Locations:**
*   **macOS (Homebrew):** `/opt/homebrew/share/hugind/config/`
*   **Linux:** `/usr/local/share/hugind/config/` or `~/.local/share/hugind/config/`
*   **Developer Mode:** `./bin/config/` (relative to executable)

**Available Templates:**
*   `config.yml`: The base structure.
*   `metal_unified.yml`: Overrides for Apple Silicon.
*   `cuda_dedicated.yml`: Overrides for NVIDIA.
*   `cpu_only.yml`: Overrides for CPU inference.