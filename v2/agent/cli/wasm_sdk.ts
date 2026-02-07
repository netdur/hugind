// @ts-nocheck

@external("hugind", "print")
declare function host_print(ptr: i32, len: i32): void;

@external("hugind", "input")
declare function host_input(ptr: i32, len: i32): i64;

@external("hugind", "net_fetch")
declare function host_net_fetch(ptr: i32, len: i32): i64;

@external("hugind", "llm_chat")
declare function host_llm_chat(ptr: i32, len: i32): i64;

@external("hugind", "llm_chat_stream")
declare function host_llm_chat_stream(ptr: i32, len: i32): i64;

@external("hugind", "run_command")
declare function host_run_command(ptr: i32, len: i32): i64;

@external("hugind", "get_args")
declare function host_get_args(): i64;

@external("hugind", "set_result")
declare function host_set_result(ptr: i32, len: i32): void;

@external("hugind_fs", "fs_cwd")
declare function host_fs_cwd(): i64;

@external("hugind_fs", "fs_exists")
declare function host_fs_exists(ptr: i32, len: i32): i32;

@external("hugind_fs", "fs_is_file")
declare function host_fs_is_file(ptr: i32, len: i32): i32;

@external("hugind_fs", "fs_is_dir")
declare function host_fs_is_dir(ptr: i32, len: i32): i32;

@external("hugind_fs", "fs_realpath")
declare function host_fs_realpath(ptr: i32, len: i32): i64;

@external("hugind_fs", "fs_read_text")
declare function host_fs_read_text(ptr: i32, len: i32): i64;

@external("hugind_fs", "fs_read_bytes")
declare function host_fs_read_bytes(ptr: i32, len: i32): i64;

@external("hugind_fs", "fs_write_text")
declare function host_fs_write_text(path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32): i32;

@external("hugind_fs", "fs_write_bytes")
declare function host_fs_write_bytes(path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32): i32;

@external("hugind_fs", "fs_append_text")
declare function host_fs_append_text(path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32): i32;

@external("hugind_fs", "fs_list_dir")
declare function host_fs_list_dir(ptr: i32, len: i32): i64;

@external("hugind_fs", "fs_mkdir")
declare function host_fs_mkdir(ptr: i32, len: i32, recursive: i32): i32;

@external("hugind_fs", "fs_remove")
declare function host_fs_remove(ptr: i32, len: i32, recursive: i32): i32;

@external("hugind_fs", "fs_rename")
declare function host_fs_rename(src_ptr: i32, src_len: i32, dst_ptr: i32, dst_len: i32): i32;

@external("hugind_fs", "fs_copy")
declare function host_fs_copy(src_ptr: i32, src_len: i32, dst_ptr: i32, dst_len: i32): i32;

@external("hugind_fs", "fs_stat")
declare function host_fs_stat(ptr: i32, len: i32): i64;

export function alloc(len: i32): i32 {
  const pagesNeeded = (len + 0xffff) >>> 16;
  const currentPages = memory.size();
  if (memory.grow(pagesNeeded) == -1) {
    unreachable(); // Crash if OOM
  }
  return currentPages << 16;
}

function unpackPtrLen(res: i64): i32[] {
  const ptr = <i32>(res >>> 32);
  const len = <i32>(res);
  return [ptr, len];
}

function readStringFromHost(res: i64): string {
  const parts = unpackPtrLen(res);
  return String.UTF8.decodeUnsafe(parts[0], parts[1], false);
}

function readBytesFromHost(res: i64): Uint8Array {
  const parts = unpackPtrLen(res);
  const view = Uint8Array.wrap(changetype<ArrayBuffer>(memory.buffer), parts[0], parts[1]);
  return view.slice();
}

export function print(msg: string): void {
  const buf = String.UTF8.encode(msg, false);
  host_print(changetype<i32>(buf), buf.byteLength);
}

export function input(prompt: string): string {
  const buf = String.UTF8.encode(prompt, false);
  const res = host_input(changetype<i32>(buf), buf.byteLength);
  return readStringFromHost(res);
}

export function netFetch(url: string): string {
  const buf = String.UTF8.encode(url, false);
  const res = host_net_fetch(changetype<i32>(buf), buf.byteLength);
  return readStringFromHost(res);
}

export function llmChat(prompt: string): string {
  const buf = String.UTF8.encode(prompt, false);
  const res = host_llm_chat(changetype<i32>(buf), buf.byteLength);
  return readStringFromHost(res);
}

export function llmChatStream(prompt: string): string {
  const buf = String.UTF8.encode(prompt, false);
  const res = host_llm_chat_stream(changetype<i32>(buf), buf.byteLength);
  return readStringFromHost(res);
}

export function runCommand(cmd: string): string {
  const buf = String.UTF8.encode(cmd, false);
  const res = host_run_command(changetype<i32>(buf), buf.byteLength);
  return readStringFromHost(res);
}

export function getArgsJson(): string {
  return readStringFromHost(host_get_args());
}

export function setResultJson(json: string): void {
  const buf = String.UTF8.encode(json, false);
  host_set_result(changetype<i32>(buf), buf.byteLength);
}

export function fsCwd(): string {
  return readStringFromHost(host_fs_cwd());
}

export function fsExists(path: string): bool {
  const buf = String.UTF8.encode(path, false);
  return host_fs_exists(changetype<i32>(buf), buf.byteLength) != 0;
}

export function fsIsFile(path: string): bool {
  const buf = String.UTF8.encode(path, false);
  return host_fs_is_file(changetype<i32>(buf), buf.byteLength) != 0;
}

export function fsIsDir(path: string): bool {
  const buf = String.UTF8.encode(path, false);
  return host_fs_is_dir(changetype<i32>(buf), buf.byteLength) != 0;
}

export function fsRealpath(path: string): string {
  const buf = String.UTF8.encode(path, false);
  return readStringFromHost(host_fs_realpath(changetype<i32>(buf), buf.byteLength));
}

export function fsReadText(path: string): string {
  const buf = String.UTF8.encode(path, false);
  return readStringFromHost(host_fs_read_text(changetype<i32>(buf), buf.byteLength));
}

export function fsReadBytes(path: string): Uint8Array {
  const buf = String.UTF8.encode(path, false);
  return readBytesFromHost(host_fs_read_bytes(changetype<i32>(buf), buf.byteLength));
}

export function fsWriteText(path: string, data: string): void {
  const pathBuf = String.UTF8.encode(path, false);
  const dataBuf = String.UTF8.encode(data, false);
  host_fs_write_text(
    changetype<i32>(pathBuf),
    pathBuf.byteLength,
    changetype<i32>(dataBuf),
    dataBuf.byteLength,
  );
}

export function fsWriteBytes(path: string, data: Uint8Array): void {
  const pathBuf = String.UTF8.encode(path, false);
  host_fs_write_bytes(
    changetype<i32>(pathBuf),
    pathBuf.byteLength,
    data.dataStart,
    data.length,
  );
}

export function fsAppendText(path: string, data: string): void {
  const pathBuf = String.UTF8.encode(path, false);
  const dataBuf = String.UTF8.encode(data, false);
  host_fs_append_text(
    changetype<i32>(pathBuf),
    pathBuf.byteLength,
    changetype<i32>(dataBuf),
    dataBuf.byteLength,
  );
}

// Returns JSON string (array of entry names)
export function fsListDir(path: string): string {
  const buf = String.UTF8.encode(path, false);
  return readStringFromHost(host_fs_list_dir(changetype<i32>(buf), buf.byteLength));
}

export function fsMkdir(path: string, recursive: bool = false): void {
  const buf = String.UTF8.encode(path, false);
  host_fs_mkdir(changetype<i32>(buf), buf.byteLength, recursive ? 1 : 0);
}

export function fsRemove(path: string, recursive: bool = false): void {
  const buf = String.UTF8.encode(path, false);
  host_fs_remove(changetype<i32>(buf), buf.byteLength, recursive ? 1 : 0);
}

export function fsRename(src: string, dst: string): void {
  const srcBuf = String.UTF8.encode(src, false);
  const dstBuf = String.UTF8.encode(dst, false);
  host_fs_rename(
    changetype<i32>(srcBuf),
    srcBuf.byteLength,
    changetype<i32>(dstBuf),
    dstBuf.byteLength,
  );
}

export function fsCopy(src: string, dst: string): void {
  const srcBuf = String.UTF8.encode(src, false);
  const dstBuf = String.UTF8.encode(dst, false);
  host_fs_copy(
    changetype<i32>(srcBuf),
    srcBuf.byteLength,
    changetype<i32>(dstBuf),
    dstBuf.byteLength,
  );
}

// Returns JSON string (stat object)
export function fsStat(path: string): string {
  const buf = String.UTF8.encode(path, false);
  return readStringFromHost(host_fs_stat(changetype<i32>(buf), buf.byteLength));
}
