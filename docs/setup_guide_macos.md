# April 2026 TLDR Setup for Hugind + Gemma 4 26B-A4B on a Mac mini (Apple Silicon)

## Prerequisites
- Mac mini with Apple Silicon (M1/M2/M3/M4/M5)
- At least 16GB unified memory (24GB+ recommended)
- macOS with Homebrew installed

## Step 1: Install Hugind

```bash
brew tap netdur/hugind
brew install hugind
```

Verify:

```bash
hugind --version
```

## Step 2: Download Gemma 4 26B-A4B

Gemma 4 26B-A4B is a Mixture-of-Experts model — 25.2B total parameters but only 3.8B active during inference, giving you 26B-quality at 4B-speed. It supports text and image input with 256K context.

```bash
hugind model add unsloth/gemma-4-26B-A4B-it-GGUF
```

This downloads the GGUF model from Hugging Face to `~/.hugind/models/unsloth/gemma-4-26B-A4B-it-GGUF/`. Downloads are resumable and SHA256-verified.

**Recommended quantization by RAM:**

| RAM | Quantization | Size |
|-----|-------------|------|
| 16GB | UD-IQ4_XS | 13.4 GB |
| 24GB | UD-Q4_K_M | 16.9 GB |
| 32GB+ | UD-Q6_K | 22.9 GB |

UD-Q4_K_M is the sweet spot for most setups — good quality with reasonable memory usage.

List downloaded models:

```bash
hugind model list
```

## Step 3: Create a Config

Use the interactive wizard — it auto-detects your hardware and sets optimal defaults:

```bash
hugind config init gemma4
```

The wizard will:
- Prompt you to select the GGUF file from the downloaded repo
- Auto-detect Apple Silicon and enable Metal GPU acceleration
- Set GPU layers to 99 (fully offloaded)
- Enable unified memory mode
- Configure optimal thread count
- Detect and set the vision projector (mmproj) if present

Your config is saved at `~/.hugind/configs/gemma4.yml`.

To inspect or edit it:

```bash
cat ~/.hugind/configs/gemma4.yml
```

Key defaults for Apple Silicon:

```yaml
server:
  host: "0.0.0.0"
  port: 8080
  system_prompt: "You are a helpful assistant."

model:
  path: "~/.hugind/models/unsloth/gemma-4-26B-A4B-it-GGUF/gemma-4-26B-A4B-it-UD-Q4_K_M.gguf"
  mmproj_path: "~/.hugind/models/unsloth/gemma-4-26B-A4B-it-GGUF/mmproj-F16.gguf"
  gpu_layers: 99

context:
  size: 4096
  batch_size: 2048
  seq_max: 4
  threads: 8
```

## Step 4: Start the Server

```bash
hugind server start gemma4
```

Verify it's running:

```bash
hugind server list
```

Check health and stats:

```bash
curl http://localhost:8080/v1/monitor | jq .
```

## Step 5: Test the Model

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemma4",
    "messages": [{"role": "user", "content": "Hello, what model are you?"}]
  }' | jq .choices[0].message.content
```

## Step 6: Configure Auto-Start on Login

### 6a. Create a Launch Agent

```bash
cat << 'EOF' > ~/Library/LaunchAgents/com.hugind.gemma4.plist
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.hugind.gemma4</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/hugind</string>
        <string>server</string>
        <string>start</string>
        <string>gemma4</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/hugind-gemma4.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/hugind-gemma4.log</string>
</dict>
</plist>
EOF
```

Load the agent:

```bash
launchctl load ~/Library/LaunchAgents/com.hugind.gemma4.plist
```

### 6b. Verify Auto-Start

```bash
launchctl list | grep hugind
hugind server list
```

## Step 7: Multimodal (Vision)

Gemma 4 26B-A4B supports image input natively. If a vision projector file was detected during `config init`, you can send images:

```bash
IMAGE_B64=$(base64 < photo.jpg | tr -d '\n')

curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"gemma4\",
    \"messages\": [{
      \"role\": \"user\",
      \"content\": [
        {\"type\": \"text\", \"text\": \"Describe this image.\"},
        {\"type\": \"image_url\", \"image_url\": {\"url\": \"data:image/jpeg;base64,${IMAGE_B64}\"}}
      ]
    }]
  }" | jq .choices[0].message.content
```

## Step 8: Sessions (Stateful Conversations)

Hugind supports stateful sessions with a 3-tier KV cache (VRAM → RAM → Disk). Pass a session ID to maintain conversation context across requests:

```bash
SESSION="my-session-$(date +%s)"

# Turn 1
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "x-session-id: $SESSION" \
  -d '{"model": "gemma4", "messages": [{"role": "user", "content": "My name is Steve."}]}'

# Turn 2 — model remembers the context
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "x-session-id: $SESSION" \
  -d '{"model": "gemma4", "messages": [{"role": "user", "content": "What is my name?"}]}'
```

## Step 9: Streaming

Enable SSE streaming for real-time token output:

```bash
curl -N http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemma4",
    "messages": [{"role": "user", "content": "Write a haiku about Mac minis."}],
    "stream": true
  }'
```

## API Reference

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/chat/completions` | POST | Chat completion (OpenAI-compatible) |
| `/v1/embeddings` | POST | Generate embeddings |
| `/v1/models` | GET | List loaded models |
| `/v1/monitor` | GET | Server health and stats |
| `/v1/state/save` | POST | Persist session KV cache to disk |
| `/v1/state/idle` | POST | Evict session from memory |
| `/v1/state/:id` | GET | Check session availability |
| `/v1/state/:id` | DELETE | Delete session |

## Useful Commands

| Command | Description |
|---------|-------------|
| `hugind server start gemma4` | Start server with config |
| `hugind server list` | Show running servers |
| `hugind server stop gemma4` | Stop server |
| `hugind model list` | List downloaded models |
| `hugind model add <repo>` | Download model from Hugging Face |
| `hugind model remove <repo>` | Delete model |
| `hugind config list` | List saved configs |
| `hugind config init <name>` | Create config with hardware auto-detect |
| `hugind config info` | Show system hardware info |
| `hugind config validate <path>` | Validate config file |

## Uninstall / Remove Auto-Start

```bash
# Remove the launch agent
launchctl unload ~/Library/LaunchAgents/com.hugind.gemma4.plist
rm ~/Library/LaunchAgents/com.hugind.gemma4.plist

# Uninstall hugind
brew uninstall hugind

# Remove all data (models, configs, sessions)
rm -rf ~/.hugind
```

## Architecture Notes

- **Engine**: llama.cpp via Rust FFI — no Python, no Docker
- **GPU**: Apple Metal on Apple Silicon (fully offloaded by default)
- **Concurrency**: Continuous batching with up to `seq_max` (default 4) concurrent sequences
- **Sessions**: 3-tier KV cache (VRAM → RAM → Disk) — loading from disk is faster than re-processing large conversation histories
- **Process model**: Each model runs in its own OS process with its own config and port — run multiple models simultaneously on different ports
- **Security**: Optional bearer token auth, SSRF protection on image URLs

## Memory

Gemma 4 26B-A4B is a MoE model — only 3.8B parameters are active per token, so it runs efficiently despite the 25.2B total parameter count. At Q4_K_M quantization (~17GB on disk), expect ~10-12GB memory usage when loaded. A 16GB Mac mini can run the UD-IQ4_XS quantization comfortably; 24GB+ is recommended for Q4_K_M or higher.
