import { spawn } from "node:child_process";

// Retained filename from the private plugin. This is a transparent MCP
// launcher, not a tool filter; the skill defines the read/write policy.
const binary = process.env.LIGHTING_BINARY;
const cwd = process.env.LIGHTING_CWD;
if (!binary || !cwd) {
  process.stderr.write("LIGHTING_BINARY and LIGHTING_CWD must be configured\n");
  process.exit(1);
}

const child = spawn(binary, ["mcp"], {
  cwd,
  env: { ...process.env },
  stdio: "inherit",
  windowsHide: true,
});

child.on("error", (error) => {
  process.stderr.write(`Could not start Lantern MCP: ${error.message}\n`);
  process.exitCode = 1;
});
child.on("close", (code, signal) => {
  process.exitCode = code ?? (signal ? 1 : 0);
});
