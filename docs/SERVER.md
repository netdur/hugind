# Hugind Native Server

The Hugind Native Server is a high-performance, OpenAI-compatible HTTP inference gateway built on top of `llama_cpp_dart`. It allows you to serve local GGUF models with stateful context management, efficient slot allocation, and concurrent user support.

## Architecture

The server is built with a layered architecture:

1.  **Configuration Layer (`lib/server/config/`)**:
    *   **`ServerConfig`**: Strongly-typed configuration object mapping YAML settings to library parameters (`ModelParams`, `ContextParams`, `SamplerParams`).
    *   **`ConfigLoader`**: Loads and validates YAML configuration files from `~/.hugind/configs/`.

2.  **Engine Layer (`lib/server/engine/`)**:
    *   **`LlamaEngine`**: Wraps a `LlamaParent` isolate. Manages a pool of `LlamaScope` instances (slots) and handles the mapping between User Session IDs and internal slots. Implements LRU eviction for efficient resource usage.
    *   **`EngineManager`**: A singleton orchestrator that manages multiple model deployments and handles load balancing between concurrent engine instances.
    *   **`EmbeddingWorker`**: Dedicated isolate for processing embedding requests efficiently.

3.  **API Layer (`lib/server/api/`)**:
    *   **`HttpServer`**: A `shelf`-based HTTP server.
    *   **`ChatHandler`**: Handles `POST /v1/chat/completions` requests.
    *   **`CompletionsHandler`**: Handles `POST /v1/completions` (Legacy).
    *   **`EmbeddingsHandler`**: Handles `POST /v1/embeddings`.
    *   **`ModelsHandler`**: Handles `GET /v1/models`.

## Configuration

Server configurations are stored in `~/.hugind/configs/*.yml` and follow the standard Hugind configuration format.

### Example Config (`~/.hugind/configs/my-config.yml`)

```yaml
# --- Core Model Settings ---
model:
  path: /path/to/model.gguf
  mmproj_path: /path/to/mmproj-model-f16.gguf # Optional (Vision)
  gpu_layers: 99        # Number of layers to offload to GPU
  use_mlock: false
  use_mmap: false

# --- Context & Batching ---
context:
  size: 4096            # Max context length
  batch_size: 512       # Batch size for prompt processing
  flash_attention: true # Flash Attention

# --- Sampling Strategy ---
sampling:
  temp: 0.7
  top_k: 40
  top_p: 0.95
  min_p: 0.05

# --- Server Network & Security ---
server:
  host: 127.0.0.1
  port: 8080
  api_key: "secret-key"
```

## Usage

Start the server with a specific configuration:

```bash
hugind server start my-config
```

This will load `~/.hugind/configs/my-config.yml` and start the server.

You can also override the port:

```bash
hugind server start my-config --port 9090
```

### List Running Models

To see which models are currently loaded on the server:

```bash
hugind server list
```


### Stop a Server

To check how to stop the server for a specific config:

```bash
hugind server stop my-config
```
(This will check the port configured for `my-config` and show commands to kill the process.)
## API Endpoints

### `POST /v1/chat/completions`

Compatible with OpenAI Chat Completions API.

**Request Body:**

```json
{
  "model": "my-model",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello!"}
  ],
  "stream": true
}
```

### `GET /v1/models`

Returns the list of currently deployed models.

### `POST /v1/completions`

Legacy completion endpoint.

### `POST /v1/embeddings`

Generate embeddings for text (only if server started with embeddings enabled).
