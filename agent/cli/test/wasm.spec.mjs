import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { TextDecoder, TextEncoder } from "node:util";

function hostStubs() {
  const emptyI64 = () => 0n;
  const emptyI32 = () => 0;
  return {
    env: {
      abort() {},
      seed: () => 1,
    },
    hugind: {
      print() {},
      print_raw() {},
      input: emptyI64,
      net_fetch: emptyI64,
      llm_chat: emptyI64,
      llm_chat_stream: emptyI64,
      run_command: emptyI64,
      tools_list: emptyI64,
      tools_call: emptyI64,
      get_args: emptyI64,
      set_result() {},
    },
    hugind_fs: {
      fs_cwd: emptyI64,
      fs_exists: emptyI32,
      fs_is_file: emptyI32,
      fs_is_dir: emptyI32,
      fs_realpath: emptyI64,
      fs_read_text: emptyI64,
      fs_read_bytes: emptyI64,
      fs_write_text: emptyI32,
      fs_write_bytes: emptyI32,
      fs_append_text: emptyI32,
      fs_list_dir: emptyI64,
      fs_mkdir: emptyI32,
      fs_remove: emptyI32,
      fs_rename: emptyI32,
      fs_copy: emptyI32,
      fs_stat: emptyI64,
    },
  };
}

test("main.wasm exports expected entry points", async () => {
  const wasm = await readFile(new URL("../main.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(wasm, hostStubs());

  assert.equal(typeof instance.exports.main, "function");
  assert.equal(typeof instance.exports.llm_on_token, "function");
  assert.equal(typeof instance.exports.llm_on_sse, "function");
  assert.equal(typeof instance.exports.alloc, "function");
});

function packPtrLen(ptr, len) {
  return (BigInt(ptr >>> 0) << 32n) | BigInt(len >>> 0);
}

async function instantiateCli(options = {}) {
  const decoder = new TextDecoder();
  const encoder = new TextEncoder();
  const printed = [];
  const printedRaw = [];
  const inputs = [...(options.inputs ?? ["exit\n"])];
  const streamedFragments = options.streamedFragments ?? [];
  const streamReturn = options.streamReturn ?? '{"kind":"answer","answer":"ok"}';
  const args = options.args ?? [];
  let instance;
  let memory;

  const readGuestString = (ptr, len) =>
    decoder.decode(new Uint8Array(memory.buffer, ptr, len));

  const allocAndWriteGuest = (text) => {
    const bytes = encoder.encode(text);
    const ptr = Number(instance.exports.alloc(bytes.length));
    new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
    return packPtrLen(ptr, bytes.length);
  };

  const imports = {
    env: {
      abort() {},
      seed: () => 1,
    },
    hugind: {
      print(ptr, len) {
        printed.push(readGuestString(ptr, len));
      },
      print_raw(ptr, len) {
        printedRaw.push(readGuestString(ptr, len));
      },
      input() {
        return allocAndWriteGuest(inputs.length > 0 ? inputs.shift() : "exit\n");
      },
      net_fetch: () => allocAndWriteGuest(""),
      llm_chat: () => allocAndWriteGuest(streamReturn),
      llm_chat_stream() {
        for (const frag of streamedFragments) {
          const bytes = encoder.encode(frag);
          const ptr = Number(instance.exports.alloc(bytes.length));
          new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
          instance.exports.llm_on_token(ptr, bytes.length);
        }
        return allocAndWriteGuest(streamReturn);
      },
      run_command: () => allocAndWriteGuest("Darwin\n"),
      tools_list: () => allocAndWriteGuest("[]"),
      tools_call: () => allocAndWriteGuest("{}"),
      get_args: () => allocAndWriteGuest(JSON.stringify({ args })),
      set_result() {},
    },
    hugind_fs: {
      fs_cwd: () => allocAndWriteGuest("/"),
      fs_exists: () => 0,
      fs_is_file: () => 0,
      fs_is_dir: () => 0,
      fs_realpath: () => allocAndWriteGuest(""),
      fs_read_text: () => allocAndWriteGuest(""),
      fs_read_bytes: () => allocAndWriteGuest(""),
      fs_write_text: () => 0,
      fs_write_bytes: () => 0,
      fs_append_text: () => 0,
      fs_list_dir: () => allocAndWriteGuest("[]"),
      fs_mkdir: () => 0,
      fs_remove: () => 0,
      fs_rename: () => 0,
      fs_copy: () => 0,
      fs_stat: () => allocAndWriteGuest("{}"),
    },
  };

  const wasm = await readFile(new URL("../main.wasm", import.meta.url));
  const instantiated = await WebAssembly.instantiate(wasm, imports);
  instance = instantiated.instance;
  memory = instance.exports.memory;
  return { instance, printed, printedRaw };
}

test("streamed fragmented think tags drive spinner without leaking text", async () => {
  const { instance, printedRaw } = await instantiateCli({
    inputs: ["hello\n", "exit\n"],
    streamedFragments: ["<th", "ink>secret", "</th", "ink>", "visible"],
    streamReturn: '{"kind":"answer","answer":"ok"}',
  });

  instance.exports.main();

  const raw = printedRaw.join("");
  assert.match(raw, /\r[|\/\\-]/);
  assert.ok(raw.includes("\r \r"));
  assert.equal(raw.includes("secret"), false);
  assert.equal(raw.includes("visible"), false);
});
