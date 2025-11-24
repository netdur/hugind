# The "Slot" Architecture Explained

In standard LLM inference, the **Context (KV Cache)** is the most expensive resource after the model weights. It stores the mathematical representation of every token in the conversation history.

A **Slot** is a dedicated block of **VRAM (Video RAM)** reserved for one specific conversation history.

## 1. The Memory Hierarchy (Lifecycle)

We have implemented a **Three-Tier Memory System** to manage conversation state efficiently.

### Tier 1: VRAM (Hot Slot) 🔥
*   **Location:** GPU Memory (or CPU RAM if no GPU).
*   **Status:** ✅ **Active.**
*   **Behavior:**
    *   The model can generate text for this user **instantly**.
    *   Switching to this user takes **0ms** (just changing a pointer).
*   **Limit:** Defined by `max_slots` in config (e.g., 4 users).

### Tier 2: System RAM (Warm State) 🧊
*   **Location:** Dart Heap (`Uint8List`).
*   **Status:** ✅ **Active.**
*   **Behavior:**
    *   **Eviction:** When VRAM is full, the system identifies the Least Recently Used (LRU) user.
    *   **Swap:** It calls `saveState()`, moving the context from GPU -> System RAM (~100ms), then frees the VRAM slot.
    *   **Resume:** If the user returns, the system allocates a new VRAM slot and `loadState()` from RAM. This avoids re-processing the prompt (Prefill).

### Tier 3: Disk (Cold Storage) 💾
*   **Location:** Hard Drive (`sessions/*.bin`).
*   **Status:** ✅ **Active.**
*   **Behavior:**
    *   **Archiving:** A background maintenance task runs every minute. If a RAM session is inactive for > 60 minutes, it is written to disk and removed from RAM.
    *   **Restoration:** `loadSession()` reads the binary file directly into VRAM.
    *   **Persistence:** Sessions survive server restarts.

---

## 2. Execution Model: Time-Slicing vs. Continuous Batching

Hugind deliberately uses a **Time-Slicing** model (Sequential Processing) instead of Continuous Batching.

**Design Philosophy:**
*   **Continuous Batching (vLLM):** Maximizes *Throughput* (tokens/sec) for cloud APIs. Requires high complexity and specialized kernels.
*   **Time-Slicing (Hugind):** Maximizes *Simplicity* and *Single-User Latency*. It ensures User A gets 100% of the GPU compute while their turn is active.

**Scenario:** You have **1 Worker** with **4 Slots**.
**Incoming:** 4 Users (A, B, C, D) send a message at the exact same second.

**The Hugind Flow:**
1.  **Worker picks User A:** Switches to Slot 1 (Instant) -> Generates Stream.
2.  **Worker picks User B:** Switches to Slot 2 (Instant) -> Generates Stream.
3.  ... and so on.

---

## 3. Eviction Strategy (Handling User #5)

What happens when **User E** arrives, but you only have 4 Slots?

**The Implemented Logic:**

1.  **Check Capacity:** The engine sees active sessions >= `config.maxSlots`.
2.  **Identify Victim:** It scans metadata to find the user who hasn't spoken in the longest time (e.g., User A).
3.  **Soft Eviction (Tier 1 -> Tier 2):**
    *   **Save:** `llama.saveState()` captures User A's context into a byte array.
    *   **Dispose:** `llama.freeSlot()` releases the VRAM.
4.  **Allocation:** User E is assigned the newly freed VRAM slot.
5.  **User A Returns:**
    *   Engine looks in VRAM? ❌ Miss.
    *   Engine looks in RAM? ✅ Hit.
    *   Allocates new VRAM slot -> `llama.loadState()` -> Resumes generation instantly.

---

## 4. Future Roadmap: Next Steps

Now that the foundation is solid, here are the next optimizations:

### A. Sticky Sessions (Load Balancing)
*   **Current:** `EngineManager` uses Round-Robin distribution.
*   **Problem:** If User A is saved to RAM on **Worker #1**, but their next request goes to **Worker #2**, Worker #2 won't find the session (RAM is local).
*   **Solution:** Implement "Sticky Routing" based on User ID hash, ensuring a user always routes to the same worker unless that worker is dead.

### B. Slot Shifting (Context Defragmentation)
If User A talks extensively, their context fills up.
*   **Current:** We trim history and re-process the prompt.
*   **Future:** Use `llama.rewind()` or KV cache shifting to "delete" the oldest 100 tokens inside VRAM without reloading. This allows "Infinite Context" with 0-cost trimming.

### C. Speculative Decoding
Use a small "Draft Model" to guess the next 5 tokens, and the main model to verify them.
*   **Benefit:** 2x-3x speed increase for single users.

### Summary of Status

| Feature | Description | Status |
| :--- | :--- | :--- |
| **Context Slots** | Dedicated VRAM per user | ✅ **Done** |
| **LRU Eviction** | Auto-handling overflow users | ✅ **Done** |
| **State Swapping** | VRAM <-> RAM (Soft Eviction) | ✅ **Done** |
| **Disk Archiving** | RAM -> Disk (Long-term storage) | ✅ **Done** |
| **Multi-Worker** | Parallel processes (`concurrency`) | ✅ **Done** |
| **Sticky Sessions** | Routing user to same worker | 🚧 **Todo** |
| **Continuous Batching** | Parallel token generation | ⛔️ **Not Planned** |
