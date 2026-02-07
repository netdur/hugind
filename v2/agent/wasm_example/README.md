# WASM example agent (AssemblyScript)

This example shows how to call Hugind's WASM host APIs from AssemblyScript.
The runtime expects `entry_point: "main.wasm"` and a `main()` export.

Host APIs (module name: `hugind`)
- `print(ptr, len)`
- `input(ptr, len) -> i64`
- `net_fetch(ptr, len) -> i64`
- `llm_chat(ptr, len) -> i64`
- `get_args() -> i64`
- `set_result(ptr, len)`

`wasm_sdk.ts` wraps these so you can call `print`, `input`, `netFetch`, `llmChat`,
`getArgsJson`, and `setResultJson` from AssemblyScript.

Notes
- Strings are UTF-8, passed as `(ptr, len)`.
- Functions that return strings pack `(ptr, len)` into a single `i64`.
- This repo does not include the AssemblyScript toolchain. Build with your own setup.

Typical build output is `main.wasm` in this folder.



npm run build

