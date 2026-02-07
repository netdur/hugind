import { print, getArgsJson, alloc } from "./wasm_sdk";

export { alloc };

export function main(): void {
  // 1. Simple print to prove we are alive
  print("Sanity Check: Agent Started!");

  // 2. Test receiving data from Host
  const args = getArgsJson();
  print("Received Args: " + args);
}
