const { VersionedTransaction } = window.solanaWeb3;

const connectBtn = document.getElementById("connectBtn");
const mintBtn = document.getElementById("mintBtn");
const suggestBtn = document.getElementById("suggestBtn");
const executeBtn = document.getElementById("executeBtn");
const backendUrlInput = document.getElementById("backendUrl");
const walletOut = document.getElementById("walletOut");
const symbioteOut = document.getElementById("symbioteOut");
const aiOut = document.getElementById("aiOut");
const execOut = document.getElementById("execOut");

let walletAddress = null;
let authToken = null;
let symbioteMint = null;
let pendingSwapBase64 = null;
let refreshTimer = null;

connectBtn.addEventListener("click", connectWallet);
mintBtn.addEventListener("click", mintSymbiote);
suggestBtn.addEventListener("click", suggestTrade);
executeBtn.addEventListener("click", executeTrade);

async function connectWallet() {
  try {
    const provider = getPhantomProvider();
    const result = await provider.connect();
    walletAddress = result.publicKey.toBase58();

    const challenge = await post("/auth/challenge", { walletAddress }, false);
    const encoded = new TextEncoder().encode(challenge.message);
    const signed = await provider.signMessage(encoded, "utf8");
    const signatureBase64 = bytesToBase64(signed.signature);
    const auth = await post("/auth/verify", { walletAddress, signatureBase64 }, false);
    authToken = auth.token;

    await post("/connect-wallet", { walletAddress, symbioteMint });
    walletOut.textContent = JSON.stringify(
      {
        walletAddress,
        authenticated: true,
        sessionExpiresAt: auth.expiresAt,
      },
      null,
      2
    );

    mintBtn.disabled = false;
    suggestBtn.disabled = false;
  } catch (error) {
    execOut.textContent = `Connect/auth failed: ${error.message}`;
  }
}

async function mintSymbiote() {
  try {
    ensureWallet();
    const response = await post("/mint-symbiote", { walletAddress });
    symbioteMint = response.symbioteMint;
    symbioteOut.textContent = JSON.stringify(response, null, 2);
    startStateRefresh();
  } catch (error) {
    execOut.textContent = `Mint failed: ${error.message}`;
  }
}

async function suggestTrade() {
  try {
    ensureWallet();
    const response = await post("/suggest-trade", { walletAddress });
    aiOut.textContent = JSON.stringify(response, null, 2);
    pendingSwapBase64 = response.readyToSignSwapTransaction || null;
    executeBtn.disabled = !pendingSwapBase64;
  } catch (error) {
    execOut.textContent = `Suggestion failed: ${error.message}`;
  }
}

async function executeTrade() {
  try {
    ensureWallet();
    if (!pendingSwapBase64) throw new Error("No pending Jupiter transaction. Run Suggest Trade first.");

    const provider = getPhantomProvider();
    const tx = VersionedTransaction.deserialize(base64ToBytes(pendingSwapBase64));
    const sent = await provider.signAndSendTransaction(tx);
    const confirm = await post("/confirm-trade", { walletAddress, signature: sent.signature });

    execOut.textContent = JSON.stringify({ signature: sent.signature, evolveResult: confirm }, null, 2);
    pendingSwapBase64 = null;
    executeBtn.disabled = true;
    if (symbioteMint) await refreshSymbioteState();
  } catch (error) {
    execOut.textContent = `Execution failed: ${error.message}`;
  }
}

function getPhantomProvider() {
  const provider = window.phantom?.solana;
  if (!provider?.isPhantom) throw new Error("Phantom wallet not found.");
  return provider;
}

function ensureWallet() {
  if (!walletAddress) throw new Error("Connect wallet first.");
  if (!authToken) throw new Error("Authenticate wallet first.");
}

function startStateRefresh() {
  if (refreshTimer) window.clearInterval(refreshTimer);
  refreshTimer = window.setInterval(() => {
    refreshSymbioteState().catch((err) => {
      execOut.textContent = `State refresh failed: ${err.message}`;
    });
  }, 10000);
}

async function refreshSymbioteState() {
  if (!symbioteMint) return;
  const state = await get(`/symbiote/${symbioteMint}`);
  symbioteOut.textContent = JSON.stringify(state, null, 2);
}

function base64ToBytes(base64) {
  const binary = window.atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function bytesToBase64(bytes) {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 1) binary += String.fromCharCode(bytes[i]);
  return window.btoa(binary);
}

async function post(endpoint, body, requireAuth = true) {
  const backend = backendUrlInput.value.trim().replace(/\/$/, "");
  const headers = { "content-type": "application/json" };
  if (requireAuth) {
    if (!authToken) throw new Error("No auth token. Connect wallet again.");
    headers.authorization = `Bearer ${authToken}`;
  }
  const response = await fetch(`${backend}${endpoint}`, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  const data = await response.json();
  if (!response.ok) throw new Error(data.error || `Request failed (${response.status})`);
  return data;
}

async function get(endpoint) {
  const backend = backendUrlInput.value.trim().replace(/\/$/, "");
  const headers = {};
  if (authToken) headers.authorization = `Bearer ${authToken}`;
  const response = await fetch(`${backend}${endpoint}`, { headers });
  const data = await response.json();
  if (!response.ok) throw new Error(data.error || `Request failed (${response.status})`);
  return data;
}
