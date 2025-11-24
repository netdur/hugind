# Hugind: Native AI Inference Server

**Hugind** is a high-performance, stateful inference server for LLMs (Large Language Models). It turns local GGUF models into an OpenAI-compatible API endpoint.

Built on `llama.cpp`, it features:
*   **⚡️ Native Performance:** Optimized for Apple Silicon (Metal), CUDA, and CPU.
*   **🧠 Stateful Memory:** Remembers conversation context per-user without re-processing history.
*   **🧊 Smart Hibernation:** Automatically moves inactive users to RAM/Disk to free up GPU resources.
*   **👥 Multi-Tenancy:** Handles 100+ user sessions efficiently using a tiered memory architecture.

---

## 1. Installation

### From Source (Dart)
```bash
# Clone the repository
git clone https://github.com/your-username/hugind.git
cd hugind

# Build the binary
bash build.sh

# Add to path (optional)
export PATH="$PATH:$(pwd)/bin"
```

### Global Setup (One-time)
Hugind needs to know where your `libllama` shared library is located.

```bash
# Set path to your compiled llama.cpp library
hugind config defaults --lib /path/to/libllama.dylib  # macOS
# OR
hugind config defaults --lib /path/to/libllama.so     # Linux
```

*(Optional)* If you want to download gated models (like Llama-3 or Gemma-2), set your Hugging Face token:
```bash
hugind config defaults --hf-token hf_YourTokenHere
```

---

## 2. Workflow Overview

Hugind separates **Weights** (Files) from **Runtime Configs** (Settings).

1.  **Download Model:** Fetch `.gguf` files from Hugging Face.
2.  **Create Config:** Generate a hardware-optimized YAML profile.
3.  **Start Server:** Run the inference engine using that profile.

---

## 3. Managing Models

Download and organize GGUF models directly from Hugging Face.

### Download a Model
Use the interactive downloader to browse files in a repository.

```bash
hugind model add google/gemma-2-9b-it-GGUF
```
*Follow the prompts to select specific quantization levels (e.g., `Q4_K_M`, `Q8_0`).*

### List Downloaded Models
See what models you have stored locally.
```bash
hugind model list
```

### Remove a Model
Delete files to free up disk space.
```bash
hugind model remove google/gemma-2-9b-it-GGUF
```

---

## 4. Creating Configurations

Instead of passing 50 command-line flags, Hugind uses "Smart Configs".

### Run the Wizard
The `init` command probes your hardware (RAM/GPU) and calculates the optimal context size to prevent crashes.

```bash
hugind config init my-chat-bot
```

**Steps:**
1.  **Preset:** Choose `metal_unified` (Mac), `cuda_dedicated` (NVIDIA), or `cpu_only`.
2.  **Model:** Select one of your downloaded models.
3.  **Format:** Select the chat template (e.g., `gemma`, `chatml`, `alpaca`).
4.  **Context:** Hugind calculates max RAM and suggests safe limits (e.g., 16k tokens).

### Edit Manually
Configs are saved in `~/.hugind/configs/`. You can edit them to tweak sampling or concurrency.

```yaml
server:
  port: 8080
  concurrency: 1      # Number of worker instances
  max_slots: 4        # GPU VRAM Slots per worker

sampling:
  temp: 0.7
  dry_multiplier: 0.8 # Enable DRY (anti-repetition)
```

---

## 5. Running the Server

### Start a Server
Pass the name of the config you created.

```bash
hugind server start my-chat-bot
```

**Output:**
```text
🚀 Initializing Hugind Server...
   → Model: gemma-2-9b-it-Q4_K_M.gguf
   → Architecture: 1 Workers / 4 Slots per worker
   ✓ Instance #1 ready
   ✓ Session Monitor active (RAM -> Disk)

✅ Server listening at http://0.0.0.0:8080
```

### Check Status
List all known configs and check if their ports are active.

```bash
hugind server list
```

---

## 6. API Usage (OpenAI Compatible)

Hugind provides an OpenAI-compatible API. You can use standard libraries (Python `openai`, JS `langchain`) by changing the `base_url`.

### Chat Completions
**Endpoint:** `POST /v1/chat/completions`

#### 1. Standard Request
```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "my-chat-bot",
    "messages": [
      {"role": "system", "content": "You are a helpful assistant."},
      {"role": "user", "content": "Explain quantum physics briefly."}
    ],
    "stream": true
  }'
```

#### 2. The "Stateful" Magic (Context Swapping)
Hugind supports **Stateful Context**. If you provide a consistent `user` ID, the server remembers the conversation. You **do not** need to re-send the history.

**Turn 1:**
```json
{
  "user": "alice",
  "messages": [{"role": "user", "content": "My name is Alice."}]
}
```

**Turn 2 (Immediately after):**
*Notice we DO NOT send the previous message history.*
```json
{
  "user": "alice",
  "messages": [{"role": "user", "content": "What is my name?"}]
}
```
**Response:** "Your name is Alice."

> **Why?** Hugind cached Alice's session. Even if 10 other users talked in between, Alice's session was "Hibernated" to RAM and restored instantly for Turn 2.

### List Models
**Endpoint:** `GET /v1/models`
Returns the list of currently active models (config names).

---

## 7. Advanced Concepts

### The Three-Tier Memory System
Hugind uses a sophisticated memory hierarchy to handle more users than your GPU can physically fit.

1.  **Tier 1: Hot Slots (VRAM) 🔥**
    *   **Capacity:** Defined by `max_slots` (e.g., 4 users).
    *   **Speed:** Instant generation.
    *   **Behavior:** The most active users live here.

2.  **Tier 2: Warm State (System RAM) 🧊**
    *   **Capacity:** Limited only by your CPU RAM (e.g., 32GB can hold ~500 users).
    *   **Behavior:** If a 5th user arrives, the "Least Recently Used" user is evicted from VRAM. Instead of deleting them, Hugind **Hibernate** them to System RAM.
    *   **Restoration:** When they return, they are swapped back to VRAM in milliseconds.

3.  **Tier 3: Cold Storage (Disk) 💾**
    *   **Capacity:** Unlimited.
    *   **Behavior:** Sessions idle for more than **60 minutes** are automatically archived to disk (`sessions/*.bin`).
    *   **Persistence:** These sessions survive server restarts.

### Load Balancing
If you have multiple GPUs or massive CPU RAM, you can set `concurrency: 2` in your config. Hugind will spawn **two** independent copies of the model.
*   Doubles your throughput.
*   Doubles VRAM usage.
*   The server automatically routes requests to the least busy worker.
