# Hugind Server Tutorial

This guide shows you how to set up Hugind to serve a Large Language Model (LLM) for multiple concurrent users (e.g., a small team or office).

## Scenario: Serving 10 Concurrent Users

We want to host a server that can handle **10 users** chatting at the same time.

### Prerequisites

- A machine with a decent GPU (Apple Silicon Mac or NVIDIA GPU recommended).
- `hugind` installed (`brew install hugind` or built from source).

---

## Step 1: Download a Model

Use the built-in model manager to fetch GGUF files directly from Hugging Face.
For this tutorial, we'll use a standard model (Gemma 3 4B) which strikes a good balance between performance and size.

```bash
# Verify no models are currently installed
hugind model list

# Download Gemma 3 4B (It will present an interactive selection)
hugind model add google/gemma-3-4b-it-qat-q4_0-gguf
```

It will fetch the file list. Use **Space** to select the model file (e.g., `gemma-3-4b-it-q4_0.gguf`) and **Enter** to confirm.
> **Tip:** If the model requires a projector (like multimodals), select `mmproj-...` files too.

---

## Step 2: Initialize Configuration

Use the hardware probe to generate a base configuration optimized for your hardware (Metal/CUDA/CPU).

```bash
hugind config init team_server
```

1.  **Select Model**: Choose the `gemma-3-4b-it` model you just downloaded.
2.  **Select Preset**: Choose `metal_unified` (macOS) or `cuda` (Linux/Windows) depending on your hardware.

---

## Step 3: Optimization for Multiple Users

The default config is optimized for a single user. We need to edit it to support 10 concurrent slots.

Open `<config_home>/configs/team_server.yml` in your text editor. (See `docs/cli.md` for how `<config_home>` is resolved.)

### Key Changes Needed:

1.  **Increase `max_slots`**: Set this to **10**.
2.  **Set `context.size`**: This is the per-session context window.
    *   If each user needs 4k tokens, set `size: 4096`.
    *   Total KV cache memory scales with `size * max_slots`.
3.  **Concurrency**: Keep `concurrency: 1`. This loads the model **once** into VRAM and shares it across the 10 slots (efficient).

**Example `team_server.yml`:**

```yaml
server:
  host: 0.0.0.0
  port: 8080
  concurrency: 1    # One model instance
  max_slots: 10     # 10 Concurrent Users
  
model:
  # Path is auto-set by 'config init'
  path: /Users/username/.hugind/google/gemma-3-4b-it-qat-q4_0-gguf/gemma-3-4b-it-q4_0.gguf

context:
  size: 4096        # Context per session
  batch_size: 512

sampler:
  temperature: 0.7
```

> **Warning:** Increasing `context.size` increases VRAM usage significantly. If the server fails to start (OOM), try reducing `size` (e.g., to 2048) or `max_slots`.

---

## Step 4: Start the Server

```bash
hugind server start team_server
```

Wait for `✨ LlamaService Engine Ready`. The server is now listening on port 8080 and ready to handle 10 concurrent chat sessions.

---

## Step 5: Connectivity

**Verify with built-in chat:**
```bash
# In a new terminal
hugind chat start team_server
```

**Verify with CURL:**
```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "team_server",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```
