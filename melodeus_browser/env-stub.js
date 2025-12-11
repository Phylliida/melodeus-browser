// Minimal placeholder for bare 'env' imports when bundling the wasm output.
// The generated bindings shouldn't rely on this, but webpack will resolve it
// if the wasm import section references an `env` module.
export const memory = new WebAssembly.Memory({ initial: 0, maximum: 0 });
export default { memory };
