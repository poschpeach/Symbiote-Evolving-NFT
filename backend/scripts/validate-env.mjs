import fs from "fs";
import path from "path";
import os from "os";

const envPath = path.resolve(process.cwd(), ".env");
if (!fs.existsSync(envPath)) {
  console.error("Missing .env file in backend directory.");
  process.exit(1);
}

const raw = fs.readFileSync(envPath, "utf8");
const required = [
  "SOLANA_RPC_URL",
  "OPENAI_API_KEY",
  "SYMBIOTE_PROGRAM_ID",
  "SYMBIOTE_IDL_PATH",
];

const map = new Map();
for (const line of raw.split("\n")) {
  const t = line.trim();
  if (!t || t.startsWith("#")) continue;
  const idx = t.indexOf("=");
  if (idx < 0) continue;
  map.set(t.slice(0, idx), t.slice(idx + 1));
}

const missing = required.filter((key) => !map.get(key));
if (missing.length > 0) {
  console.error(`Missing required env keys: ${missing.join(", ")}`);
  process.exit(1);
}

const hasBase58 = Boolean(map.get("SYMBIOTE_KEYPAIR_BASE58"));
const hasFile = Boolean(map.get("SYMBIOTE_KEYPAIR_FILE"));
if (!hasBase58 && !hasFile) {
  console.error("Set one of: SYMBIOTE_KEYPAIR_BASE58 or SYMBIOTE_KEYPAIR_FILE");
  process.exit(1);
}

if (hasFile) {
  const rawPath = map.get("SYMBIOTE_KEYPAIR_FILE");
  const keypairPath = rawPath.startsWith("~/")
    ? path.join(os.homedir(), rawPath.slice(2))
    : path.resolve(process.cwd(), rawPath);
  if (!fs.existsSync(keypairPath)) {
    console.error(`SYMBIOTE_KEYPAIR_FILE not found: ${keypairPath}`);
    process.exit(1);
  }
}

const idlPath = path.resolve(process.cwd(), map.get("SYMBIOTE_IDL_PATH"));
if (!fs.existsSync(idlPath)) {
  console.error(`IDL file not found: ${idlPath}`);
  process.exit(1);
}

console.log("Environment validation passed.");
