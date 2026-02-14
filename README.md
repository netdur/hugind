# Hugind 🐦‍⬛

## Scope / positioning

* Local-only system (not intended to run in the cloud)
* Uses `llama.cpp` as the inference engine
* Aims to keep agents under control

## Implementation stack

* Rust (with a custom `llama.cpp` Rust binding)

Two execution runtimes:

* Javascript runtime via `rquickjs`
* WASM runtime via Wasmtime (run WASM modules built from your favorite language)

## Server

* `llama.cpp` engine (including `llama.cpp` features)
* Continuous batching
* Large contexts are processed by splitting work into a fixed set of request chunks
* Context shifting
* Stateful sessions
* Designed for limited GPU memory: avoid recomputing full history and free GPU resources quickly

3-tier memory / cache:

* VRAM → RAM → disk
* Uses the fastest available tier; e.g. loading cache from disk can be faster than re-processing a large FAQ/history

Session branching:

* OpenAI-compatible HTTP API
* Authentication + streaming
* Embeddings supported
* Multimodal

Process model:

* Each model runs with its own config, OS process, and preconfigured port
* Application clients can probe ports or use predefined ports
* End users should not access the server directly (server is meant to sit behind an app, like a database)

## Agent

* Process isolation
* Each agent runs in its own OS process

Runtimes:

* JavaScript runtime (`rquickjs`)
* WASM runtime (Wasmtime) for running modules compiled from any language

Network access control (allowlist-based):

* Allowlist by IPs and domains
* Supports wildcards (`*`) and DNS-based rules

Filesystem access control (fine-grained):

* Scoped access with explicit read/write permissions
* WASM: filesystem mounts supported

Shell / OS access control (allowlist + sandboxing):

* Allowlist-based shell access with OS sandboxing
* WASM: RAM and CPU usage limitations
* Environment variables supported

Other:

* MCP support
  * MCP servers are declared per-agent in `agent.yaml` under `dependencies.mcp` (include `command` and `transport`)
* Agent install CLI (`hugind agent install <path>`)
* Informs the user of requested permissions and supports granting permissions explicitly

## Model

* Download models from Hugging Face
* Management for installed models

## Config

* Hardware probing for auto-configuration

## Chat (CLI)

* “ChatGPT-style” CLI for quick testing and deployment validation
* Basic administration features

## quick start

### install
```bash
brew install hugind
hugind --version
```

### set Hugging Face token (if needed)
```bash
hugind config defaults --hf-token <your_token>
```

### download a model
```bash
hugind model add google/gemma-3-4b-it-qat-q4_0-gguf
```

### init config
```bash
hugind config init gemmea-b4
```
Choose:
- your hardware preset
- `google/gemma-3-4b-it-qat-q4_0-gguf`
- `gemma-3-4b-it-q4_0.gguf`
- a context size

The config is written to `~/.hugind/configs/gemmea-b4.yml`.

## server usage

### start server
```bash
hugind server start gemmea-b4
Loading model from "/Users/adel/.hugind/google/gemma-3-4b-it-qat-q4_0-gguf/gemma-3-4b-it-q4_0.gguf"
Starting Server on 0.0.0.0:8080
Engine thread started
Server listening on 0.0.0.0:8080
Engine initialized, entering loop
```

### check health
```bash
curl http://0.0.0.0:8080/v1/monitor
```
Output:
```json
{"server_state":"running","requests_processing":0,"requests_waiting":0,"tokens_per_sec_total":0.0,"tokens_per_sec_per_active":0.0,"slots_usage":{"active":0,"total":4},"memory":{"ram_usage_bytes":0,"vram_usage_bytes":null},"cache_stats":{"vram_sessions":0,"ram_sessions":0}}
```

### chat (non-streaming)
```bash
curl http://0.0.0.0:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemmea-b4",
    "stream": false,
    "messages": [
      { "role": "user", "content": "who are you?" }
    ]
  }'
```
Output:
```json
{"id":"c8b50247-ecbb-4351-b8f2-d3c346572a19","object":"chat.completion","created":1770660601,"model":"gemmea-b4","choices":[{"index":0,"message":{"role":"assistant","content":"I'm Gemma, a large language model created by the Gemma team at Google DeepMind. I’m an open-weights model, which means I’m publicly available for anyone to use and experiment with! \n\nI can take text and images as input and generate text as output. \n\nIt’s nice to meet you!"},"finish_reason":"Eos"}],"usage":null}
```

### chat (streaming)
```bash
curl http://0.0.0.0:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemmea-b4",
    "stream": true,
    "messages": [
      { "role": "user", "content": "who are you?" }
    ]
  }'
```
Output:
```text
data: {"id":"7ae2bccb-35ac-4131-8aa6-2875e1cdb6d9","object":"chat.completion.chunk","created":1770660651,"model":"gemmea-b4","choices":[{"index":0,"delta":{"role":null,"content":"I"},"finish_reason":null}]}
...
data: {"id":"7ae2bccb-35ac-4131-8aa6-2875e1cdb6d9","object":"chat.completion.chunk","created":1770660651,"model":"gemmea-b4","choices":[{"index":0,"delta":{"role":null,"content":null},"finish_reason":"Eos"}]}
data: [DONE]
```

### multimodal (image + text)
`image_url` must be a data URL or an http(s) URL.
```bash
curl http://0.0.0.0:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"gemmea-b4\",
    \"stream\": false,
    \"messages\": [
      {
        \"role\": \"user\",
        \"content\": [
          { \"type\": \"text\", \"text\": \"What is the hair color in this image?\" },
          { \"type\": \"image_url\", \"image_url\": { \"url\": \"data:image/jpeg;base64,$(base64 -i /Users/adel/Downloads/W2o284UT.jpg | tr -d '\n')\" } }
        ]
      }
    ]
  }"
```
Output:
```json
{"id":"821c9b8b-ba9d-4a2f-ac15-d01aa00faa6d","object":"chat.completion","created":1770660845,"model":"gemmea-b4","choices":[{"index":0,"message":{"role":"assistant","content":"The hair color in the image is dark brown, possibly with some subtle reddish highlights."},"finish_reason":"Eos"}],"usage":null}
```

### stateful session
```bash
curl http://0.0.0.0:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "x-session-id: my-session-1" \
  -d '{
    "model": "gemmea-b4",
    "stream": false,
    "messages": [
      { "role": "user", "content": "hello, my name is bob" }
    ]
  }'
```
Output:
```json
{"id":"ebb04b55-26a8-4b7a-86fa-20f63053dd13","object":"chat.completion","created":1770666840,"model":"gemmea-b4","choices":[{"index":0,"message":{"role":"assistant","content":"Hi Bob! It's nice to meet you. \n\nHow are you doing today? Is there anything you'd like to chat about, or were you just saying hello?"},"finish_reason":"Eos"}],"usage":null}
```

```bash
curl http://0.0.0.0:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "x-session-id: my-session-1" \
  -d '{
    "model": "gemmea-b4",
    "stream": false,
    "messages": [
      { "role": "user", "content": "What is my name?" }
    ]
  }'
```
Output:
```json
{"id":"061cc7ca-a28d-49a5-a874-c6929c5cc058","object":"chat.completion","created":1770666850,"model":"gemmea-b4","choices":[{"index":0,"message":{"role":"assistant","content":"Your name is Bob! You told me that at the beginning. \n\nIs there anything else you’d like to know, or were you just curious?"},"finish_reason":"Eos"}],"usage":null}
```

### manage session state
```bash
curl -X POST http://0.0.0.0:8080/v1/state/save \
  -H "Content-Type: application/json" \
  -d '{
    "session_id": "my-session-1",
    "template_id": "my-template-1"
  }'
```
Output:
```text
State save requested
```

```bash
ls ~/.hugind/sessions
```
Output:
```text
my-template-1.bin
```

```bash
curl -X DELETE http://0.0.0.0:8080/v1/state/my-session-1
```
Output:
```text
State deletion requested
```

```bash
ls ~/.hugind/sessions
```
Output:
```text
```

```bash
curl http://0.0.0.0:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "x-session-id: my-session-1" \
  -d '{
    "model": "gemmea-b4",
    "stream": false,
    "messages": [
      { "role": "user", "content": "What is my name?" }
    ]
  }'
```
Output:
```json
{"id":"0fdcde06-6129-4d07-a19c-efa9b3f300ef","object":"chat.completion","created":1770667060,"model":"gemmea-b4","choices":[{"index":0,"message":{"role":"assistant","content":"As an AI, I don't know your name! You haven't told me. \n\nYou can tell me if you'd like."},"finish_reason":"Eos"}],"usage":null}
```

## Agent demo

```bash
hugind agent install https://github.com/netdur/hugind/tree/main/agent/audit                               

Requested permissions:
- Network access: No
- File access: Yes (actions: read; can access outside agent folder)
- Run system commands: No
> Grant these permissions and install this agent? Yes
✅ Installed agent 'audit' to /Users/adel/.hugind/agents/audit

hugind agent run audit agent/ocr                                                                          
Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
{
  "Alignment": "PASS - The code reads image URLs, extracts text, and returns structured JSON as described in the agent.yaml.",
  "Security": "PASS - The agent adheres to the specified permissions, avoiding network access and shell commands while allowing filesystem access within the permitted paths.",
  "Notes": "The code reads files from the allowed path `/Users/adel/Downloads`. It uses the `fs.read_bytes` function which should be restricted by the agent's permissions.",
  "Confidence": "high"
}

hugind agent run agent/ocr --image /Users/adel/Downloads/18zjgwovgbhg1.jpeg --prompt "only read the title"
Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
{"blocks": [{"block_type": "text", "text": "32F looking for Moroccan match", "bbox_2d": [171, 133, 783, 223]}]}

more /Users/adel/.hugind/logs/agents/ocr/20260210_122933.526.txt                                                           
[2026-02-10T12:29:33.526Z] agent.run.start name=ocr entry=/Users/adel/Workspace/hugind/agent/ocr/main.js args_len=87 args=["--image","/Users/adel/Downloads/18zjgwovgbhg1.jpeg","--prompt","only read the title"]
[2026-02-10T12:29:33.530Z] host.fs.read_bytes path=/Users/adel/Downloads/18zjgwovgbhg1.jpeg
[2026-02-10T12:29:35.014Z] host.llm.chat_stream input=object messages=Some(1) model=
[2026-02-10T12:29:41.198Z] host.llm.chat_stream response_len=111
[2026-02-10T12:29:41.198Z] agent.run.complete status=ok
```

## License

MIT
