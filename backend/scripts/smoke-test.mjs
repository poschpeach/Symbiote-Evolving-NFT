const base = process.env.BACKEND_URL || "http://127.0.0.1:3000";
const testWallet = process.env.TEST_WALLET_ADDRESS || "";

async function main() {
  await checkHealth();
  if (testWallet) {
    await checkAuthChallenge(testWallet);
  } else {
    console.log("Skipping /auth/challenge check: set TEST_WALLET_ADDRESS to enable.");
  }
  console.log("Smoke test finished.");
}

async function checkHealth() {
  const response = await fetch(`${base}/health`);
  if (!response.ok) {
    throw new Error(`/health failed with status ${response.status}`);
  }
  const body = await response.json();
  if (!body.ok) {
    throw new Error("/health returned unexpected body");
  }
  console.log("Health check passed.");
}

async function checkAuthChallenge(walletAddress) {
  const response = await fetch(`${base}/auth/challenge`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ walletAddress }),
  });
  if (!response.ok) {
    throw new Error(`/auth/challenge failed with status ${response.status}`);
  }
  const body = await response.json();
  if (!body.message || !body.nonce) {
    throw new Error("/auth/challenge missing message or nonce");
  }
  console.log("Auth challenge check passed.");
}

main().catch((error) => {
  console.error(`Smoke test failed: ${error.message}`);
  process.exit(1);
});
