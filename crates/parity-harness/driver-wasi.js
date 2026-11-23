const fs = require('fs');
const path = require('path');
const dir = __dirname;
const wasmPath = path.join(dir, '..', 'placeholder');
const bytes = fs.readFileSync(process.argv[2] || path.join(dir, 'parity_harness.wasm'));
const expected = fs.readFileSync(path.join(dir, 'expected.json'), 'utf8').trim();
let mem = null;
const view = () => new DataView(mem.buffer);
const imports = {
  wasi_snapshot_preview1: {
    random_get: (ptr, len) => {
      crypto.getRandomValues(new Uint8Array(mem.buffer, ptr, len));
      return 0;
    },
    environ_get: () => 0,
    environ_sizes_get: (countPtr, sizePtr) => {
      view().setUint32(countPtr, 0, true);
      view().setUint32(sizePtr, 0, true);
      return 0;
    },
    fd_write: (fd, iovsPtr, iovsLen, nwrittenPtr) => {
      view().setUint32(nwrittenPtr, 0, true);
      return 0;
    },
    proc_exit: (code) => { throw new Error('proc_exit(' + code + ')'); },
  },
};
const mod = new WebAssembly.Module(bytes);
const instance = new WebAssembly.Instance(mod, imports);
mem = instance.exports.memory;
const bufPtr = instance.exports.parity_buf();
const len = instance.exports.parity_run();
const got = Buffer.from(new Uint8Array(mem.buffer, bufPtr, len)).toString('utf8');
console.log('wasm output:', got);
console.log('expected   :', expected);
console.log(got === expected ? 'PARITY OK' : 'MISMATCH');
if (got !== expected) process.exitCode = 1;
