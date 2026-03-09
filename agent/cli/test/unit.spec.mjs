import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

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

async function loadUnitWasm() {
  const wasm = await readFile(new URL("../unit_test_exports.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(wasm, hostStubs());
  return instance.exports;
}

test("json helper units pass", async () => {
  const exports = await loadUnitWasm();
  assert.equal(exports.unit_extract_json_direct(), 1);
  assert.equal(exports.unit_extract_json_fenced(), 1);
  assert.equal(exports.unit_parse_response_normalizes_command_shape(), 1);
  assert.equal(exports.unit_parse_response_fallback_answer(), 1);
});

test("safety helper units pass", async () => {
  const exports = await loadUnitWasm();
  assert.equal(exports.unit_safety_classification(), 1);
  assert.equal(exports.unit_should_exit_detection(), 1);
});

test("truncate helper unit passes", async () => {
  const exports = await loadUnitWasm();
  assert.equal(exports.unit_smart_truncate_marker(), 1);
});

test("prompt helper units pass", async () => {
  const exports = await loadUnitWasm();
  assert.equal(exports.unit_escape_json_string_behaviour(), 1);
  assert.equal(exports.unit_initial_prompt_contains_context(), 1);
  assert.equal(exports.unit_invalid_command_prompt_contains_command(), 1);
});
