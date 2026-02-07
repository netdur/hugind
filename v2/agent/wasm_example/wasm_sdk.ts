// @ts-nocheck

@external("hugind", "print")
declare function host_print(ptr: i32, len: i32): void;

@external("hugind", "input")
declare function host_input(ptr: i32, len: i32): i64;

@external("hugind", "net_fetch")
declare function host_net_fetch(ptr: i32, len: i32): i64;

@external("hugind", "llm_chat")
declare function host_llm_chat(ptr: i32, len: i32): i64;

@external("hugind", "get_args")
declare function host_get_args(): i64;

@external("hugind", "set_result")
declare function host_set_result(ptr: i32, len: i32): void;

export function alloc(len: i32): i32 {
  const pagesNeeded = (len + 0xffff) >>> 16;
  const currentPages = memory.size();
  if (memory.grow(pagesNeeded) == -1) {
    unreachable(); // Crash if OOM
  }
  return currentPages << 16;
}

export function print(msg: string): void {
  const buf = String.UTF8.encode(msg, true);
  host_print(changetype<i32>(buf), buf.byteLength);
}

export function input(prompt: string): string {
  const buf = String.UTF8.encode(prompt, true);
  const res = host_input(changetype<i32>(buf), buf.byteLength);
  const ptr = <i32>(res >>> 32);
  const len = <i32>(res);
  return String.UTF8.decodeUnsafe(ptr, len, false);
}

export function netFetch(url: string): string {
  const buf = String.UTF8.encode(url, true);
  const res = host_net_fetch(changetype<i32>(buf), buf.byteLength);
  const ptr = <i32>(res >>> 32);
  const len = <i32>(res);
  return String.UTF8.decodeUnsafe(ptr, len, false);
}

export function llmChat(prompt: string): string {
  const buf = String.UTF8.encode(prompt, true);
  const res = host_llm_chat(changetype<i32>(buf), buf.byteLength);
  const ptr = <i32>(res >>> 32);
  const len = <i32>(res);
  return String.UTF8.decodeUnsafe(ptr, len, false);
}

export function getArgsJson(): string {
  const res = host_get_args();
  const ptr = <i32>(res >>> 32);
  const len = <i32>(res);
  return String.UTF8.decodeUnsafe(ptr, len, false);
}

export function setResultJson(json: string): void {
  const buf = String.UTF8.encode(json, true);
  host_set_result(changetype<i32>(buf), buf.byteLength);
}
