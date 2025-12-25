# State Management in Hugind

Hugind implements a sophisticated state management system designed to handle high concurrency on limited hardware. Unlike standard stateless LLM APIs, Hugind can persist conversation context (KV cache) between requests, significantly reducing processing time and enabling long-running sessions.

## 1. User & API Perspective

By default, Hugind supports the standard stateless OpenAI API. However, for optimized performance, clients can opt-in to **Stateful Mode**.

### Stateless Mode (Default)
Behaves exactly like the standard OpenAI API.
- **Request**: You send the full list of messages `[{"role": "user", "content": "..."}]` every time.
- **Processing**: The server re-evaluates the entire history for every request.
- **Pros**: Compatibility with existing clients.
- **Cons**: Slower processing for long chats; higher latency.

### Stateful Mode (Optimized)
Allows the server to "remember" the conversation context.
- **Request**: You send **only the new message** and a Session ID.
- **Processing**: The server loads the previous context from memory/disk and only processes the new tokens.
- **Pros**: Fast responses even with long history.

**Usage:**
Add the `X-Session-ID` header to your request.

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-Session-ID: my-unique-session-id" \
  -d '{
    "model": "my-model",
    "messages": [
      { "role": "user", "content": "Hello, my name is Adel." }
    ]
  }'
```

**Implicit History:**
When `X-Session-ID` is present:
1.  **First Request**: The server processes the message and saves the state associated with `my-unique-session-id`.
2.  **Subsequent Request**: You send *only* the next status usage message. The server appends it to the loaded state.

> **⚠️ Developer Responsibility**
>
> It is strictly the **developer's responsibility** to generate and manage unique `X-Session-ID`s.
> *   Do not reuse the same ID for different end-users or different conversation threads, as this will bleed context between them (mixing chats).
> *   We recommend using UUIDs (e.g., `550e8400-e29b-41d4-a716-446655440000`) for each distinct chat session.

---

## 2. Internal Architecture

Internally, Hugind uses a **3-Tier Hierarchical Storage System** to manage session states (`LlamaScope`). This allows it to support more active users than would fit in GPU VRAM.

### The 3 Tiers

| Tier | Name | Storage | Speed | Description |
| :--- | :--- | :--- | :--- | :--- |
| **1** | **Hot** | **VRAM** | ⚡️ Instant | Active sessions loaded on the GPU/CPU. Ready for immediate generation. |
| **2** | **Warm** | **System RAM** | 🚀 Fast | Serialized KV cache stored in main memory. Used when VRAM slots are full. |
| **3** | **Cold** | **Disk** | 🐢 Slow | Binary files stored in `~/.hugind/sessions/`. Long-term storage for specific inactive users. |

### Lifecycle of a Request

1.  **Resolution**: When a request arrives with a Session ID:
    *   **Tier 1 Hit**: If the session is already in VRAM, it is used immediately.
    *   **Tier 2 Hit**: If found in RAM, a VRAM slot is allocated, and memory is restored.
    *   **Tier 3 Hit**: If found on disk, the file is read and loaded into VRAM.
    *   **New**: A fresh session is created.

2.  **Eviction (LRU)**:
    *   If all VRAM slots are full (defined by `max_slots` in config), the **Least Recently Used (LRU)** session is evicted.
    *   **Soft Eviction**: The victim session is serialized to System RAM (Tier 2).
    *   **Hard Eviction**: If System RAM is full (not currently strictly enforced) or during maintenance, it moves to Disk (Tier 3).

3.  **Maintenance**:
    *   A background timer runs every minute.
    *   Sessions in Tier 2 (RAM) that have been inactive for >60 minutes are automatically archived to Tier 3 (Disk) to free up system memory.

### Persistence Configuration

Session files are stored in `~/.hugind/sessions` by default. This path can be configured in your global settings:

**File: `~/.hugind/settings.yml`**
```yaml
sessions_path: "/path/to/custom/storage"
```
