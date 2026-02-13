import fs from "fs";
import path from "path";
import crypto from "crypto";
import os from "os";
import express from "express";
import dotenv from "dotenv";
import cors from "cors";
import rateLimit from "express-rate-limit";
import nacl from "tweetnacl";
import Database from "better-sqlite3";
import OpenAI from "openai";
import bs58 from "bs58";
import * as anchor from "@coral-xyz/anchor";
import { Program, AnchorProvider, Wallet } from "@coral-xyz/anchor";
import { Connection, PublicKey, Keypair } from "@solana/web3.js";
import {
  getAssociatedTokenAddressSync,
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
} from "@solana/spl-token";

dotenv.config();

const app = express();
app.use(
  cors({
    origin: process.env.CORS_ORIGIN || "*",
  })
);
app.use(express.json({ limit: "1mb" }));
app.use(
  rateLimit({
    windowMs: Number(process.env.RATE_LIMIT_WINDOW_MS || 60_000),
    max: Number(process.env.RATE_LIMIT_MAX || 120),
    standardHeaders: true,
    legacyHeaders: false,
  })
);

const PORT = Number(process.env.PORT || 3000);
const RPC_URL = process.env.SOLANA_RPC_URL;
const WS_URL = process.env.SOLANA_WS_URL || undefined;
const OPENAI_MODEL = process.env.OPENAI_MODEL || "gpt-4.1-mini";
const JUPITER_API_BASE = process.env.JUPITER_API_BASE || "https://quote-api.jup.ag/v6";
const JUPITER_FEE_BPS = Number(process.env.JUPITER_FEE_BPS || 50);
const JUPITER_REFERRAL_FEE_ACCOUNT = process.env.JUPITER_REFERRAL_FEE_ACCOUNT || "";
const PROGRAM_ID = new PublicKey(process.env.SYMBIOTE_PROGRAM_ID || "Fg6PaFpoGXkYsidMpWxTWqkZq5Q8x8M9KXQvS6kR7d5k");
const IDL_PATH = process.env.SYMBIOTE_IDL_PATH || "../symbiote-anchor/target/idl/symbiote_pet.json";
const TOKEN_METADATA_PROGRAM_ID = new PublicKey("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
const JUPITER_PROGRAM_ID = new PublicKey("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");
const SOL_MINT = "So11111111111111111111111111111111111111112";
const USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const CHALLENGE_TTL_MS = Number(process.env.AUTH_CHALLENGE_TTL_MS || 5 * 60 * 1000);
const SESSION_TTL_MS = Number(process.env.AUTH_SESSION_TTL_MS || 24 * 60 * 60 * 1000);
const MIN_CONFIRM_VOLUME_USD = Number(process.env.MIN_CONFIRM_VOLUME_USD || 1);
const DEFAULT_GAME_TICK_SEC = Number(process.env.GAME_TICK_SEC || 300);

if (!RPC_URL) throw new Error("Missing SOLANA_RPC_URL in environment.");
if (!process.env.OPENAI_API_KEY) throw new Error("Missing OPENAI_API_KEY in environment.");
const db = new Database(path.resolve("symbiote.db"));
initDb(db);

const openai = new OpenAI({ apiKey: process.env.OPENAI_API_KEY });
const connection = new Connection(RPC_URL, {
  wsEndpoint: WS_URL,
  commitment: "confirmed",
});

const botKeypair = loadBackendKeypair();
const wallet = new Wallet(botKeypair);
const provider = new AnchorProvider(connection, wallet, { commitment: "confirmed" });
anchor.setProvider(provider);

const idlAbsolutePath = path.resolve(process.cwd(), IDL_PATH);
if (!fs.existsSync(idlAbsolutePath)) {
  throw new Error(`IDL not found at ${idlAbsolutePath}. Build the Anchor program first.`);
}
const idl = JSON.parse(fs.readFileSync(idlAbsolutePath, "utf8"));
idl.address = PROGRAM_ID.toBase58();
const program = new Program(idl, provider);
const activeSubscriptions = new Map();
const activeGameLoops = new Map();

const authLimiter = rateLimit({
  windowMs: 60_000,
  max: 30,
  standardHeaders: true,
  legacyHeaders: false,
});

app.get("/health", (_req, res) => {
  res.json({ ok: true });
});

app.post("/auth/challenge", authLimiter, async (req, res) => {
  try {
    const { walletAddress } = req.body;
    if (!walletAddress) return res.status(400).json({ error: "walletAddress is required" });
    const walletPk = new PublicKey(walletAddress);

    const nonce = crypto.randomBytes(24).toString("base64url");
    const expiresAt = new Date(Date.now() + CHALLENGE_TTL_MS).toISOString();
    const message = buildAuthMessage(walletPk.toBase58(), nonce);

    cleanupExpiredAuthRows(db);
    saveChallenge(db, walletPk.toBase58(), nonce, expiresAt);

    res.json({
      walletAddress: walletPk.toBase58(),
      nonce,
      message,
      expiresAt,
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.post("/auth/verify", authLimiter, async (req, res) => {
  try {
    const { walletAddress, signatureBase64 } = req.body;
    if (!walletAddress || !signatureBase64) {
      return res.status(400).json({ error: "walletAddress and signatureBase64 are required" });
    }
    const walletPk = new PublicKey(walletAddress);
    const challenge = getLatestValidChallenge(db, walletPk.toBase58(), new Date().toISOString());
    if (!challenge) {
      return res.status(400).json({ error: "No active challenge. Request /auth/challenge first." });
    }

    const message = buildAuthMessage(walletPk.toBase58(), challenge.nonce);
    const signatureBytes = Buffer.from(signatureBase64, "base64");
    const messageBytes = new TextEncoder().encode(message);
    const valid = nacl.sign.detached.verify(messageBytes, signatureBytes, walletPk.toBytes());
    if (!valid) {
      return res.status(401).json({ error: "Invalid wallet signature." });
    }

    deleteChallenges(db, walletPk.toBase58());
    cleanupExpiredSessions(db);
    const token = `${crypto.randomUUID()}-${crypto.randomBytes(12).toString("hex")}`;
    const expiresAt = new Date(Date.now() + SESSION_TTL_MS).toISOString();
    saveSession(db, token, walletPk.toBase58(), expiresAt);

    res.json({
      authenticated: true,
      walletAddress: walletPk.toBase58(),
      token,
      expiresAt,
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.get("/symbiote/:mint", requireSession, async (req, res) => {
  try {
    const state = await fetchSymbioteState(req.params.mint);
    if (!state) return res.status(404).json({ error: "Symbiote not found" });
    res.json(state);
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.get("/metadata/:mint/state.json", async (req, res) => {
  try {
    const state = await fetchSymbioteState(req.params.mint);
    if (!state) return res.status(404).json({ error: "Symbiote not found" });

    const host = process.env.METADATA_IMAGE_BASE_URL || "https://api.dicebear.com/9.x/shapes/svg";
    const image = `${host}?seed=${state.mint}-${state.level}-${encodeURIComponent(state.personality)}`;
    res.json({
      name: `Symbiote Pet #${state.mint.slice(0, 6)}`,
      symbol: "SYMB",
      description: "Autonomous financial pet evolving from wallet behavior.",
      image,
      attributes: [
        { trait_type: "Level", value: state.level },
        { trait_type: "XP", value: state.xp },
        { trait_type: "Personality", value: state.personality },
      ],
      properties: {
        category: "image",
        files: [{ uri: image, type: "image/svg+xml" }],
      },
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.post("/connect-wallet", requireSession, async (req, res) => {
  try {
    const { walletAddress, symbioteMint } = req.body;
    if (!walletAddress) return res.status(400).json({ error: "walletAddress is required" });
    requireWalletMatch(req.auth.walletAddress, walletAddress);

    const walletPk = new PublicKey(walletAddress);
    if (symbioteMint) new PublicKey(symbioteMint);

    upsertUser(db, walletPk.toBase58(), symbioteMint || null);
    upsertGameProfile(db, walletPk.toBase58());
    configureAutoPlay(walletPk.toBase58());
    subscribeWallet(walletPk);
    res.json({
      status: "connected",
      walletAddress: walletPk.toBase58(),
      symbioteMint: symbioteMint || null,
      listenerActive: true,
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.post("/mint-symbiote", requireSession, async (req, res) => {
  try {
    const { walletAddress } = req.body;
    if (!walletAddress) return res.status(400).json({ error: "walletAddress is required" });
    requireWalletMatch(req.auth.walletAddress, walletAddress);

    const owner = new PublicKey(walletAddress);
    const mint = Keypair.generate();
    const [statePda] = PublicKey.findProgramAddressSync([Buffer.from("symbiote_state"), mint.publicKey.toBuffer()], PROGRAM_ID);
    const [metadataPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("metadata"), TOKEN_METADATA_PROGRAM_ID.toBuffer(), mint.publicKey.toBuffer()],
      TOKEN_METADATA_PROGRAM_ID
    );
    const [masterEditionPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("metadata"), TOKEN_METADATA_PROGRAM_ID.toBuffer(), mint.publicKey.toBuffer(), Buffer.from("edition")],
      TOKEN_METADATA_PROGRAM_ID
    );
    const ownerAta = getAssociatedTokenAddressSync(mint.publicKey, owner);

    const signature = await program.methods
      .mintSymbiote(owner)
      .accounts({
        payer: botKeypair.publicKey,
        owner,
        mint: mint.publicKey,
        symbioteState: statePda,
        ownerAta,
        metadata: metadataPda,
        masterEdition: masterEditionPda,
        tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([mint])
      .rpc();

    upsertUser(db, walletAddress, mint.publicKey.toBase58());
    saveMemory(db, walletAddress, "system", `Minted symbiote ${mint.publicKey.toBase58()}`);

    res.json({
      minted: true,
      signature,
      walletAddress,
      symbioteMint: mint.publicKey.toBase58(),
      symbioteState: statePda.toBase58(),
      metadata: metadataPda.toBase58(),
      masterEdition: masterEditionPda.toBase58(),
      ownerAta: ownerAta.toBase58(),
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.post("/suggest-trade", requireSession, async (req, res) => {
  try {
    const { walletAddress } = req.body;
    if (!walletAddress) return res.status(400).json({ error: "walletAddress is required" });
    requireWalletMatch(req.auth.walletAddress, walletAddress);

    const history = await fetchTradeHistory(walletAddress, 20);
    const memory = getMemory(db, walletAddress, 15);
    const ai = await inferSymbioteState(history, memory);
    const swapPlan = await buildSwapPlan(walletAddress, ai.recommendation);

    saveMemory(db, walletAddress, "assistant", JSON.stringify(ai));
    saveSuggestion(db, walletAddress, ai, swapPlan);

    res.json({
      walletAddress,
      riskProfile: ai.risk_profile,
      symbioteReaction: ai.reaction,
      personality: ai.personality,
      recommendation: ai.recommendation,
      jupiterQuote: swapPlan.quote,
      readyToSignSwapTransaction: swapPlan.swapTransactionBase64,
      referralFeeAccount: JUPITER_REFERRAL_FEE_ACCOUNT,
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.post("/agent/play-turn", requireSession, async (req, res) => {
  try {
    const { walletAddress } = req.body;
    if (!walletAddress) return res.status(400).json({ error: "walletAddress is required" });
    requireWalletMatch(req.auth.walletAddress, walletAddress);

    const turn = await runGameTurn(walletAddress, true);
    res.json(turn);
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.get("/agent/state/:walletAddress", requireSession, async (req, res) => {
  try {
    const walletAddress = req.params.walletAddress;
    requireWalletMatch(req.auth.walletAddress, walletAddress);

    const profile = getGameProfile(db, walletAddress);
    const recentActions = getRecentGameActions(db, walletAddress, 12);
    const user = getUser(db, walletAddress);
    const symbiote = user?.symbiote_mint ? await fetchSymbioteState(user.symbiote_mint) : null;

    res.json({
      walletAddress,
      profile: profile || null,
      symbiote,
      recentActions,
      autoPlayActive: activeGameLoops.has(walletAddress),
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.post("/agent/auto-play", requireSession, async (req, res) => {
  try {
    const { walletAddress, enabled, intervalSec } = req.body;
    if (!walletAddress || typeof enabled !== "boolean") {
      return res.status(400).json({ error: "walletAddress and enabled are required." });
    }
    requireWalletMatch(req.auth.walletAddress, walletAddress);

    const safeInterval = Math.max(60, Number(intervalSec || DEFAULT_GAME_TICK_SEC));
    upsertGameProfile(db, walletAddress, {
      mode: "Agentic",
      autoPlay: enabled ? 1 : 0,
      tickIntervalSec: safeInterval,
    });
    configureAutoPlay(walletAddress);

    res.json({
      walletAddress,
      enabled,
      intervalSec: safeInterval,
      autoPlayActive: activeGameLoops.has(walletAddress),
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.post("/confirm-trade", requireSession, async (req, res) => {
  try {
    const { walletAddress, signature } = req.body;
    if (!walletAddress || !signature) {
      return res.status(400).json({ error: "walletAddress and signature are required" });
    }
    requireWalletMatch(req.auth.walletAddress, walletAddress);

    if (hasProcessedTrade(db, signature)) {
      return res.status(409).json({ error: "Trade signature already processed." });
    }

    const user = getUser(db, walletAddress);
    if (!user || !user.symbiote_mint) {
      return res.status(400).json({ error: "Wallet is not connected to a symbiote mint." });
    }

    const parsed = await connection.getParsedTransaction(signature, {
      commitment: "confirmed",
      maxSupportedTransactionVersion: 0,
    });
    if (!parsed) return res.status(404).json({ error: "Transaction not found." });
    if (parsed.meta?.err) return res.status(400).json({ error: "Transaction failed on-chain." });

    const signerMatched = parsed.transaction.message.accountKeys.some((a) => {
      const pubkey = "pubkey" in a ? a.pubkey.toBase58() : "";
      return a.signer && pubkey === walletAddress;
    });
    if (!signerMatched) {
      return res.status(400).json({ error: "Swap signer does not match authenticated wallet." });
    }

    const hasJupiterIx = parsed.transaction.message.instructions.some((ix) => {
      if ("programId" in ix) return ix.programId.equals(JUPITER_PROGRAM_ID);
      return false;
    });
    if (!hasJupiterIx) return res.status(400).json({ error: "Not a Jupiter swap transaction." });

    const tradeVolume = estimateTradeVolumeUsd(parsed);
    if (tradeVolume < MIN_CONFIRM_VOLUME_USD) {
      return res.status(400).json({
        error: `Trade volume below minimum threshold (${MIN_CONFIRM_VOLUME_USD}).`,
      });
    }

    const previousState = await fetchSymbioteState(user.symbiote_mint);
    const ai = await inferPostTradePersonality(walletAddress, tradeVolume, previousState?.personality || "Neutral");
    const evolved = await evolveSymbiote(user.symbiote_mint, previousState, ai.personality, tradeVolume);

    saveTrade(db, walletAddress, signature, tradeVolume, ai.personality);
    saveMemory(db, walletAddress, "system", `Evolved symbiote to ${JSON.stringify(evolved)}`);

    res.json({
      confirmed: true,
      signature,
      tradeVolumeUsd: tradeVolume,
      evolvedState: evolved,
    });
  } catch (error) {
    if (String(error.message || "").includes("UNIQUE constraint failed")) {
      return res.status(409).json({ error: "Trade signature already processed." });
    }
    res.status(500).json({ error: error.message });
  }
});

app.get("/sample-jupiter-transaction", (_req, res) => {
  const sample = JSON.parse(fs.readFileSync(path.resolve("sample-jupiter-transaction.json"), "utf8"));
  res.json(sample);
});

app.listen(PORT, () => {
  console.log(`Symbiote backend listening on :${PORT}`);
});

function initDb(database) {
  database.exec(`
    CREATE TABLE IF NOT EXISTS users (
      wallet_address TEXT PRIMARY KEY,
      symbiote_mint TEXT,
      created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS memory (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      wallet_address TEXT NOT NULL,
      role TEXT NOT NULL,
      content TEXT NOT NULL,
      created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS suggestions (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      wallet_address TEXT NOT NULL,
      risk_profile TEXT NOT NULL,
      personality TEXT NOT NULL,
      reaction TEXT NOT NULL,
      recommendation TEXT NOT NULL,
      quote_json TEXT NOT NULL,
      created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS trades (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      wallet_address TEXT NOT NULL,
      signature TEXT NOT NULL UNIQUE,
      volume_usd REAL NOT NULL,
      personality TEXT NOT NULL,
      created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS auth_challenges (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      wallet_address TEXT NOT NULL,
      nonce TEXT NOT NULL,
      expires_at TEXT NOT NULL,
      created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS sessions (
      token TEXT PRIMARY KEY,
      wallet_address TEXT NOT NULL,
      expires_at TEXT NOT NULL,
      created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS game_profiles (
      wallet_address TEXT PRIMARY KEY,
      mode TEXT NOT NULL DEFAULT 'Agentic',
      archetype TEXT NOT NULL DEFAULT 'Explorer',
      streak INTEGER NOT NULL DEFAULT 0,
      energy INTEGER NOT NULL DEFAULT 100,
      auto_play INTEGER NOT NULL DEFAULT 0,
      tick_interval_sec INTEGER NOT NULL DEFAULT 300,
      updated_at TEXT DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS game_actions (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      wallet_address TEXT NOT NULL,
      symbiote_mint TEXT,
      game_name TEXT NOT NULL,
      objective TEXT NOT NULL,
      move_text TEXT NOT NULL,
      outcome_text TEXT NOT NULL,
      tx_base64 TEXT,
      created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );
  `);
}

function requireSession(req, res, next) {
  try {
    const auth = req.headers.authorization || "";
    if (!auth.startsWith("Bearer ")) return res.status(401).json({ error: "Missing bearer token." });
    const token = auth.slice("Bearer ".length).trim();
    const session = getSession(db, token);
    if (!session) return res.status(401).json({ error: "Invalid session." });
    if (new Date(session.expires_at).getTime() <= Date.now()) {
      deleteSession(db, token);
      return res.status(401).json({ error: "Session expired." });
    }
    req.auth = {
      token,
      walletAddress: session.wallet_address,
    };
    next();
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
}

function requireWalletMatch(authWallet, requestWallet) {
  if (authWallet !== requestWallet) throw new Error("Authenticated wallet does not match request wallet.");
}

function buildAuthMessage(walletAddress, nonce) {
  return `Symbiote Authentication\nWallet: ${walletAddress}\nNonce: ${nonce}`;
}

function saveChallenge(database, walletAddress, nonce, expiresAt) {
  database.prepare(`INSERT INTO auth_challenges (wallet_address, nonce, expires_at) VALUES (?, ?, ?)`).run(walletAddress, nonce, expiresAt);
}

function getLatestValidChallenge(database, walletAddress, nowIso) {
  return database
    .prepare(
      `SELECT * FROM auth_challenges
       WHERE wallet_address = ? AND expires_at > ?
       ORDER BY id DESC LIMIT 1`
    )
    .get(walletAddress, nowIso);
}

function deleteChallenges(database, walletAddress) {
  database.prepare(`DELETE FROM auth_challenges WHERE wallet_address = ?`).run(walletAddress);
}

function cleanupExpiredAuthRows(database) {
  database.prepare(`DELETE FROM auth_challenges WHERE expires_at <= ?`).run(new Date().toISOString());
}

function saveSession(database, token, walletAddress, expiresAt) {
  database
    .prepare(`INSERT OR REPLACE INTO sessions (token, wallet_address, expires_at) VALUES (?, ?, ?)`)
    .run(token, walletAddress, expiresAt);
}

function getSession(database, token) {
  return database.prepare(`SELECT * FROM sessions WHERE token = ?`).get(token);
}

function deleteSession(database, token) {
  database.prepare(`DELETE FROM sessions WHERE token = ?`).run(token);
}

function cleanupExpiredSessions(database) {
  database.prepare(`DELETE FROM sessions WHERE expires_at <= ?`).run(new Date().toISOString());
}

function upsertUser(database, walletAddress, symbioteMint) {
  database
    .prepare(
      `INSERT INTO users (wallet_address, symbiote_mint)
       VALUES (?, ?)
       ON CONFLICT(wallet_address)
       DO UPDATE SET symbiote_mint = COALESCE(excluded.symbiote_mint, users.symbiote_mint)`
    )
    .run(walletAddress, symbioteMint);
}

function getUser(database, walletAddress) {
  return database.prepare(`SELECT * FROM users WHERE wallet_address = ?`).get(walletAddress);
}

function hasProcessedTrade(database, signature) {
  const row = database.prepare(`SELECT 1 as found FROM trades WHERE signature = ? LIMIT 1`).get(signature);
  return Boolean(row?.found);
}

function saveMemory(database, walletAddress, role, content) {
  database.prepare(`INSERT INTO memory (wallet_address, role, content) VALUES (?, ?, ?)`).run(walletAddress, role, content);
}

function getMemory(database, walletAddress, limit) {
  return database
    .prepare(`SELECT role, content, created_at FROM memory WHERE wallet_address = ? ORDER BY id DESC LIMIT ?`)
    .all(walletAddress, limit)
    .reverse();
}

function saveSuggestion(database, walletAddress, ai, swapPlan) {
  database
    .prepare(
      `INSERT INTO suggestions (wallet_address, risk_profile, personality, reaction, recommendation, quote_json)
       VALUES (?, ?, ?, ?, ?, ?)`
    )
    .run(
      walletAddress,
      ai.risk_profile,
      ai.personality,
      ai.reaction,
      ai.recommendation.text,
      JSON.stringify(swapPlan.quote)
    );
}

function saveTrade(database, walletAddress, signature, volumeUsd, personality) {
  database
    .prepare(`INSERT INTO trades (wallet_address, signature, volume_usd, personality) VALUES (?, ?, ?, ?)`)
    .run(walletAddress, signature, volumeUsd, personality);
}

function getGameProfile(database, walletAddress) {
  return database.prepare(`SELECT * FROM game_profiles WHERE wallet_address = ?`).get(walletAddress);
}

function upsertGameProfile(database, walletAddress, patch = {}) {
  const current = getGameProfile(database, walletAddress) || {
    mode: "Agentic",
    archetype: "Explorer",
    streak: 0,
    energy: 100,
    auto_play: 0,
    tick_interval_sec: DEFAULT_GAME_TICK_SEC,
  };
  database
    .prepare(
      `INSERT INTO game_profiles (wallet_address, mode, archetype, streak, energy, auto_play, tick_interval_sec, updated_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT(wallet_address) DO UPDATE SET
         mode = excluded.mode,
         archetype = excluded.archetype,
         streak = excluded.streak,
         energy = excluded.energy,
         auto_play = excluded.auto_play,
         tick_interval_sec = excluded.tick_interval_sec,
         updated_at = excluded.updated_at`
    )
    .run(
      walletAddress,
      patch.mode ?? current.mode,
      patch.archetype ?? current.archetype,
      patch.streak ?? current.streak,
      patch.energy ?? current.energy,
      patch.autoPlay ?? current.auto_play,
      patch.tickIntervalSec ?? current.tick_interval_sec,
      new Date().toISOString()
    );
  return getGameProfile(database, walletAddress);
}

function saveGameAction(database, walletAddress, action) {
  database
    .prepare(
      `INSERT INTO game_actions (wallet_address, symbiote_mint, game_name, objective, move_text, outcome_text, tx_base64)
       VALUES (?, ?, ?, ?, ?, ?, ?)`
    )
    .run(
      walletAddress,
      action.symbioteMint || null,
      action.gameName || "Symbiote Arena",
      action.objective || "Survive market volatility",
      action.moveText || "Hold formation.",
      action.outcomeText || "No major state change.",
      action.txBase64 || null
    );
}

function getRecentGameActions(database, walletAddress, limit = 20) {
  return database
    .prepare(`SELECT * FROM game_actions WHERE wallet_address = ? ORDER BY id DESC LIMIT ?`)
    .all(walletAddress, limit);
}

function subscribeWallet(walletPk) {
  const walletAddress = walletPk.toBase58();
  if (activeSubscriptions.has(walletAddress)) return;

  const subId = connection.onLogs(
    walletPk,
    async (event) => {
      if (!event.signature) return;
      try {
        const history = await fetchTradeHistory(walletAddress, 10);
        const memory = getMemory(db, walletAddress, 10);
        const ai = await inferSymbioteState(history, memory);
        saveMemory(db, walletAddress, "assistant", JSON.stringify(ai));
      } catch (error) {
        console.error("listener error", walletAddress, error.message);
      }
    },
    "confirmed"
  );
  activeSubscriptions.set(walletAddress, subId);
}

async function fetchTradeHistory(walletAddress, limit = 20) {
  const pubkey = new PublicKey(walletAddress);
  const signatures = await connection.getSignaturesForAddress(pubkey, { limit });
  const parsed = await connection.getParsedTransactions(
    signatures.map((s) => s.signature),
    { maxSupportedTransactionVersion: 0, commitment: "confirmed" }
  );
  const compact = [];
  for (const tx of parsed) {
    if (!tx) continue;
    compact.push({
      slot: tx.slot,
      blockTime: tx.blockTime,
      feeLamports: tx.meta?.fee || 0,
      err: tx.meta?.err || null,
      programs: tx.transaction.message.instructions.map((ix) => {
        if ("programId" in ix) return ix.programId.toBase58();
        return "unknown";
      }),
    });
  }
  return compact;
}

async function inferSymbioteState(history, memory) {
  const systemPrompt = `
You are Symbiote, an autonomous Solana trading companion.
Return strict JSON with this shape:
{
  "risk_profile": "Conservative|Balanced|Aggressive|Degen",
  "personality": "short unique label",
  "reaction": "one sentence in-character reaction",
  "recommendation": {
    "text": "next trade suggestion",
    "input_mint": "mint address (default SOL)",
    "output_mint": "mint address (default USDC)",
    "amount_lamports_or_units": "integer string amount"
  }
}
Use mints:
SOL ${SOL_MINT}
USDC ${USDC_MINT}
`;

  const response = await openai.responses.create({
    model: OPENAI_MODEL,
    input: [
      { role: "system", content: systemPrompt },
      { role: "user", content: JSON.stringify({ history, memory }) },
    ],
    temperature: 0.3,
  });

  const parsed = safeJsonParse(response.output_text || "{}");
  return {
    risk_profile: parsed.risk_profile || "Balanced",
    personality: parsed.personality || "Adaptive",
    reaction: parsed.reaction || "I am adapting to your current market tempo.",
    recommendation: {
      text: parsed.recommendation?.text || "You are overexposed to risk, rotate into SOL and stables.",
      input_mint: parsed.recommendation?.input_mint || SOL_MINT,
      output_mint: parsed.recommendation?.output_mint || USDC_MINT,
      amount_lamports_or_units: String(parsed.recommendation?.amount_lamports_or_units || "10000000"),
    },
  };
}

async function inferGameTurn(walletAddress, symbiote, history, memory) {
  const systemPrompt = `
You are a game master for an autonomous on-chain companion.
Create one turn for a persistent game where the Symbiote plays for its owner.
Return strict JSON:
{
  "game_name": "string",
  "objective": "string",
  "move_text": "string",
  "outcome_text": "string",
  "archetype": "string",
  "requires_trade": true|false,
  "trade": {
    "text": "string",
    "input_mint": "mint address",
    "output_mint": "mint address",
    "amount_lamports_or_units": "integer string amount"
  }
}
Use SOL mint ${SOL_MINT}
Use USDC mint ${USDC_MINT}
`;

  const response = await openai.responses.create({
    model: OPENAI_MODEL,
    input: [
      { role: "system", content: systemPrompt },
      { role: "user", content: JSON.stringify({ walletAddress, symbiote, history, memory }) },
    ],
    temperature: 0.6,
  });

  const parsed = safeJsonParse(response.output_text || "{}");
  return {
    gameName: parsed.game_name || "Symbiote Arena",
    objective: parsed.objective || "Preserve energy while compounding XP.",
    moveText: parsed.move_text || "The Symbiote scouts liquidity corridors.",
    outcomeText: parsed.outcome_text || "No catastrophic encounter this round.",
    archetype: parsed.archetype || "Explorer",
    requiresTrade: Boolean(parsed.requires_trade),
    trade: {
      text: parsed.trade?.text || "Rotate some risk into SOL.",
      input_mint: parsed.trade?.input_mint || SOL_MINT,
      output_mint: parsed.trade?.output_mint || USDC_MINT,
      amount_lamports_or_units: String(parsed.trade?.amount_lamports_or_units || "10000000"),
    },
  };
}

async function runGameTurn(walletAddress, allowSwapBuild = true) {
  const user = getUser(db, walletAddress);
  if (!user || !user.symbiote_mint) {
    throw new Error("Wallet is not connected to a symbiote mint.");
  }

  const history = await fetchTradeHistory(walletAddress, 20);
  const memory = getMemory(db, walletAddress, 20);
  const symbiote = await fetchSymbioteState(user.symbiote_mint);
  const turn = await inferGameTurn(walletAddress, symbiote, history, memory);

  let swapPlan = null;
  if (allowSwapBuild && turn.requiresTrade) {
    swapPlan = await buildSwapPlan(walletAddress, turn.trade);
  }

  const profileBefore = upsertGameProfile(db, walletAddress);
  const nextEnergy = Math.max(0, Math.min(100, Number(profileBefore.energy) + (turn.requiresTrade ? -8 : 3)));
  const nextStreak = Number(profileBefore.streak) + 1;
  const profileAfter = upsertGameProfile(db, walletAddress, {
    archetype: turn.archetype,
    streak: nextStreak,
    energy: nextEnergy,
  });

  saveMemory(db, walletAddress, "assistant", `GAME_TURN ${JSON.stringify(turn)}`);
  saveGameAction(db, walletAddress, {
    symbioteMint: user.symbiote_mint,
    gameName: turn.gameName,
    objective: turn.objective,
    moveText: turn.moveText,
    outcomeText: turn.outcomeText,
    txBase64: swapPlan?.swapTransactionBase64 || null,
  });

  return {
    walletAddress,
    symbioteMint: user.symbiote_mint,
    turn,
    gameProfile: profileAfter,
    readyToSignSwapTransaction: swapPlan?.swapTransactionBase64 || null,
    jupiterQuote: swapPlan?.quote || null,
    referralFeeAccount: JUPITER_REFERRAL_FEE_ACCOUNT || null,
  };
}

function configureAutoPlay(walletAddress) {
  const current = activeGameLoops.get(walletAddress);
  if (current) {
    clearInterval(current.intervalId);
    activeGameLoops.delete(walletAddress);
  }

  const profile = getGameProfile(db, walletAddress);
  if (!profile || Number(profile.auto_play) !== 1) return;

  const intervalMs = Math.max(60, Number(profile.tick_interval_sec || DEFAULT_GAME_TICK_SEC)) * 1000;
  const intervalId = setInterval(() => {
    runGameTurn(walletAddress, false).catch((error) => {
      console.error("auto play turn failed", walletAddress, error.message);
    });
  }, intervalMs);

  activeGameLoops.set(walletAddress, { intervalId, intervalMs });
}

async function inferPostTradePersonality(walletAddress, tradeVolumeUsd, currentPersonality) {
  const response = await openai.responses.create({
    model: OPENAI_MODEL,
    input: `Wallet: ${walletAddress}
Current personality: ${currentPersonality}
Latest trade volume usd: ${tradeVolumeUsd}
Generate JSON: {"personality":"...","reason":"..."}`,
    temperature: 0.5,
  });
  const parsed = safeJsonParse(response.output_text || "{}");
  return {
    personality: parsed.personality || currentPersonality,
    reason: parsed.reason || "Updated from latest trade behavior.",
  };
}

async function buildSwapPlan(walletAddress, recommendation) {
  const quoteUrl =
    `${JUPITER_API_BASE}/quote?` +
    new URLSearchParams({
      inputMint: recommendation.input_mint,
      outputMint: recommendation.output_mint,
      amount: recommendation.amount_lamports_or_units,
      slippageBps: "50",
      platformFeeBps: String(JUPITER_FEE_BPS),
    }).toString();

  const quoteRes = await fetch(quoteUrl);
  if (!quoteRes.ok) throw new Error(`Quote failed: ${await quoteRes.text()}`);
  const quote = await quoteRes.json();

  const swapRes = await fetch(`${JUPITER_API_BASE}/swap`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      quoteResponse: quote,
      userPublicKey: walletAddress,
      wrapAndUnwrapSol: true,
      feeAccount: JUPITER_REFERRAL_FEE_ACCOUNT,
      dynamicComputeUnitLimit: true,
      prioritizationFeeLamports: "auto",
    }),
  });
  if (!swapRes.ok) throw new Error(`Swap build failed: ${await swapRes.text()}`);
  const swap = await swapRes.json();

  return {
    quote,
    swapTransactionBase64: swap.swapTransaction,
    lastValidBlockHeight: swap.lastValidBlockHeight,
    prioritizationFeeLamports: swap.prioritizationFeeLamports,
  };
}

async function fetchSymbioteState(symbioteMint) {
  const mint = new PublicKey(symbioteMint);
  const [statePda] = PublicKey.findProgramAddressSync([Buffer.from("symbiote_state"), mint.toBuffer()], PROGRAM_ID);
  const [metadataPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("metadata"), TOKEN_METADATA_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    TOKEN_METADATA_PROGRAM_ID
  );
  try {
    const account = await program.account.symbioteState.fetch(statePda);
    return {
      level: account.level,
      xp: Number(account.xp),
      personality: account.personality,
      uri: account.uri,
      owner: account.owner.toBase58(),
      evolutionAuthority: account.evolutionAuthority.toBase58(),
      mint: account.mint.toBase58(),
      statePda: statePda.toBase58(),
      metadataPda: metadataPda.toBase58(),
    };
  } catch {
    return null;
  }
}

async function evolveSymbiote(symbioteMint, previousState, nextPersonality, tradeVolumeUsd) {
  const mint = new PublicKey(symbioteMint);
  const [statePda] = PublicKey.findProgramAddressSync([Buffer.from("symbiote_state"), mint.toBuffer()], PROGRAM_ID);
  const [metadataPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("metadata"), TOKEN_METADATA_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    TOKEN_METADATA_PROGRAM_ID
  );

  const oldXp = previousState?.xp || 0;
  const xpDelta = Math.max(1, Math.round(tradeVolumeUsd));
  const newXp = oldXp + xpDelta;
  const newLevel = Math.floor(newXp / 1000) + 1;

  await program.methods
    .evolveSymbiote(mint, {
      level: newLevel,
      xp: new anchor.BN(newXp),
      personalityString: nextPersonality,
    })
    .accounts({
      authority: botKeypair.publicKey,
      symbioteState: statePda,
      metadata: metadataPda,
      tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
    })
    .rpc();

  const fresh = await fetchSymbioteState(symbioteMint);
  return {
    mint: symbioteMint,
    level: fresh?.level ?? newLevel,
    xp: fresh?.xp ?? newXp,
    personality: fresh?.personality ?? nextPersonality,
    uri: fresh?.uri,
    xpDelta,
  };
}

function estimateTradeVolumeUsd(parsedTx) {
  const pre = parsedTx.meta?.preTokenBalances || [];
  const post = parsedTx.meta?.postTokenBalances || [];
  if (pre.length === 0 || post.length === 0) return 5;
  const preMap = new Map(pre.map((b) => [`${b.accountIndex}-${b.mint}`, Number(b.uiTokenAmount.uiAmount || 0)]));
  let maxDelta = 0;
  for (const b of post) {
    const key = `${b.accountIndex}-${b.mint}`;
    const prev = preMap.get(key) || 0;
    const next = Number(b.uiTokenAmount.uiAmount || 0);
    const delta = Math.abs(next - prev);
    if (delta > maxDelta) maxDelta = delta;
  }
  return maxDelta === 0 ? 5 : maxDelta;
}

function safeJsonParse(input) {
  try {
    return JSON.parse(input);
  } catch {
    const first = input.indexOf("{");
    const last = input.lastIndexOf("}");
    if (first >= 0 && last > first) {
      try {
        return JSON.parse(input.slice(first, last + 1));
      } catch {
        return {};
      }
    }
    return {};
  }
}

function loadBackendKeypair() {
  const base58Secret = process.env.SYMBIOTE_KEYPAIR_BASE58;
  if (base58Secret) return Keypair.fromSecretKey(bs58.decode(base58Secret));

  const keypairFileRaw = process.env.SYMBIOTE_KEYPAIR_FILE || "~/.config/solana/id.json";
  const keypairFile = keypairFileRaw.startsWith("~/")
    ? path.join(os.homedir(), keypairFileRaw.slice(2))
    : keypairFileRaw;

  if (!fs.existsSync(keypairFile)) {
    throw new Error("Missing keypair: set SYMBIOTE_KEYPAIR_BASE58 or SYMBIOTE_KEYPAIR_FILE.");
  }

  const secret = JSON.parse(fs.readFileSync(keypairFile, "utf8"));
  if (!Array.isArray(secret) || secret.length === 0) {
    throw new Error(`Invalid keypair file at ${keypairFile}`);
  }
  return Keypair.fromSecretKey(Uint8Array.from(secret));
}
