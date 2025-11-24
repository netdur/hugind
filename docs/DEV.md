# Hugind Developer Documentation

**Hugind** is a high-performance inference server written in Dart. It acts as an Orchestrator and HTTP Gateway for `llama.cpp`.

## 1. Architecture Overview

Hugind follows a **Supervisor/Worker** architecture to ensure the HTTP Event Loop remains non-blocking while CPU-intensive inference runs in background Isolates.

### High-Level Diagram

```mermaid
[HTTP Client]  <-- SSE Stream -->  [Shelf Server (Main Isolate)]
                                         |
                                   [EngineManager]
                                         |
                                   [LlamaEngine] (Tiered Memory Manager)
                                         |
                                   [LlamaParent] (Isolate Controller)
                                         |
                                   (Isolate Boundary ⚡️)
                                         |
                                   [LlamaChild] (Background Isolate)
                                         |
                                   [Llama (FFI)] -> [libllama.so]
```

### Key Components

1.  **Supervisor (Main Isolate):**
    *   Parses CLI arguments.
    *   Runs the `shelf` HTTP server.
    *   Manages Session IDs and Load Balancing.
    *   Translates OpenAI JSON -> Prompt Strings.

2.  **Workers (Background Isolates):**
    *   Managed by `llama_cpp_dart` package (`LlamaParent`/`LlamaChild`).
    *   Holds the actual C++ Pointers (`llama_context`).
    *   Executes token generation loop.
    *   Manages VRAM Slots and State Serialization.

---

## 2. Project Structure

```text
lib/
├── commands/              # CLI Command Implementations
│   ├── config_command.dart  # 'hugind config' (Wizard & Templates)
│   ├── model_command.dart   # 'hugind model' (HF Downloader)
│   └── server_command.dart  # 'hugind server' (Entry point)
│
├── server/                # The Runtime Engine
│   ├── api/                 # HTTP Layer
│   │   ├── chat_handler.dart    # POST /v1/chat/completions
│   │   └── models_handler.dart  # GET /v1/models
│   │
│   ├── config/              # Configuration Logic
│   │   ├── config_loader.dart   # YAML -> Typed Dart Object
│   │   └── server_config.dart   # The ServerConfig data class
│   │
│   ├── engine/              # Logic Layer
│   │   ├── engine_manager.dart  # Load Balancer & Deployer
│   │   ├── llama_engine.dart    # Tiered Memory Logic & Maintenance Loop
│   │   ├── isolate_parent.dart  # Controller for Child Isolate
│   │   ├── isolate_child.dart   # Runner inside Isolate
│   │   └── isolate_types.dart   # Command/Response Data Classes
│   │
│   └── bootstrap.dart       # Server startup & Port checking
│
├── repo_manager.dart      # File System & Hugging Face API logic
└── global_settings.dart   # Global defaults (~/.hugind/settings.yml)
```

---

## 3. Core Logic & Data Flow

### A. Configuration Loading
**File:** `lib/server/config/config_loader.dart`

The server does not accept raw CLI flags for model parameters. Instead:
1.  User generates a YAML file (`config init`).
2.  `ConfigLoader` reads the YAML.
3.  It validates paths (Model, Library).
4.  It maps string enums (e.g., `"q8_0"`) to FFI Enums (`LlamaKvCacheType.q8_0`).
5.  It returns a `ServerConfig` object.

### B. The "Stateful" Engine (Tiered Memory)
**File:** `lib/server/engine/llama_engine.dart`

This is the critical business logic layer. Unlike standard API wrappers, `LlamaEngine` manages a **Three-Tier Memory Architecture** to support more users than physical VRAM allows.

**Memory Tiers:**
1.  **Tier 1 (VRAM):** `Map<String, LlamaScope> _activeSessions`. Live C++ context pointers.
2.  **Tier 2 (RAM):** `Map<String, Uint8List> _ramSessions`. Serialized state data.
3.  **Tier 3 (Disk):** `sessions/*.bin` files. Long-term storage.

**The Request Lifecycle:**
When a request arrives for user `alice`:
1.  **Check Tier 1:** Is `_activeSessions['alice']` present?
    *   **Hit:** Use existing scope. **Cost: 0ms.**
2.  **Check Tier 2:** Is `_ramSessions['alice']` present?
    *   **Hit:** Evict an idle user from Tier 1 -> Allocate VRAM -> `loadState(bytes)`. **Cost: ~100ms.**
3.  **Check Tier 3:** Does `sessions/alice.bin` exist?
    *   **Hit:** Evict an idle user from Tier 1 -> Allocate VRAM -> `loadSession(path)`. **Cost: Disk I/O.**
4.  **Miss:** Create new session -> Format Prompt.

**Maintenance Loop:**
A background timer runs every 60 seconds.
*   Scans `_ramSessions` metadata.
*   If `lastUsed > 60 minutes`: Writes bytes to Disk and removes from RAM.

### C. The Isolate Protocol
**Files:** `isolate_types.dart`, `isolate_parent.dart`, `isolate_child.dart`

Communication crosses the Isolate boundary using `SendPort` and `ReceivePort`. We define specific commands to control the C++ layer.

**Commands:**
*   `LlamaPrompt`: Generate text.
*   `LlamaSaveState`: Serialize `llama_context` -> `Uint8List` (Returns to Parent).
*   `LlamaLoadState`: Deserialize `Uint8List` -> `llama_context`.
*   `LlamaLoadSession`: Load directly from Disk file -> `llama_context`.
*   `LlamaFreeSlot`: Destroy `llama_context` to free VRAM.

---

## 4. Adding New Features

### Scenario 1: Adding a New Configuration Option
*Example: You want to add a `temperature_inc` parameter.*

1.  **Update `ServerConfig`:** Add `final double tempInc;` to `lib/server/config/server_config.dart`.
2.  **Update `ConfigLoader`:** Read `yaml['sampling']['temp_inc']` in `lib/server/config/config_loader.dart`.
3.  **Update `SamplerParams`:** (Inside `llama_cpp_dart` package) Add the field there.
4.  **Update YAML Template:** Add the field to `bin/config/config.yml` so users see it.

### Scenario 2: Adding a New API Endpoint
*Example: You want to add `POST /v1/embeddings`.*

1.  **Create Handler:** Create `lib/server/api/embeddings_handler.dart`.
2.  **Implement Logic:**
    *   Parse JSON.
    *   Get `EngineManager.instance.getEngineForUser(...)`.
    *   Call `engine.getEmbeddings()`.
3.  **Register Route:** Update `lib/server/bootstrap.dart`:
    ```dart
    app.post('/v1/embeddings', EmbeddingsHandler());
    ```

---

## 5. Debugging & Testing

### Logging
*   **HTTP Logs:** Handled by `shelf` middleware.
*   **Logic Logs:** Look for `print` statements in `LlamaEngine`.
    *   `🔥 [Tier 1]`: VRAM Hit.
    *   `🧊 [Tier 2]`: RAM Hit (Restoring...).
    *   `💾 [Tier 3]`: Disk Hit.
    *   `📦 Archiving`: Maintenance loop moving RAM -> Disk.

### Common Issues
1.  **`dlsym` Error:** `libllama.dylib` not found.
    *   *Fix:* Check `GlobalSettings.getLibraryPath()` or `config.yml`.
2.  **Infinite Generation:** Wrong Chat Format.
    *   *Fix:* Check `config.yml` -> `chat: format`. Ensure it matches the model.
3.  **Context Full:**
    *   *Fix:* Run `config init` again and select a smaller context size.

## 6. Build Process

Hugind depends on `config` templates being available at runtime.

**Development Run:**
```bash
dart run bin/hugind.dart server start smol
```
*Logic looks in `./bin/config` relative to script.*

**Production Build:**
```bash
bash build.sh
```
*Compiles to `bin/hugind` executable.*