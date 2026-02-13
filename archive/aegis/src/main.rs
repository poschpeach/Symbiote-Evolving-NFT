use std::collections::{HashMap, VecDeque};
use std::env;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client as HttpClient;
use serde_json::{Value, json};

const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const DEFAULT_PYTH_SOL_FEED_ID: &str =
    "ef0d8b6fda2ceba41ff3ec77f091ab2cfec0015c51ffbcaad2f8478f42da64b";

#[derive(Debug, Clone)]
struct RiskConfig {
    danger_health_factor: f64,
    target_health_factor: f64,
    emergency_health_factor: f64,
    liquidation_threshold: f64,
    max_unwind_pct_per_action: f64,
    cooldown_slots: u64,
    max_slippage_bps: u64,
    fee_floor_microlamports: u64,
    fee_ceiling_microlamports: u64,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            danger_health_factor: 1.08,
            target_health_factor: 1.25,
            emergency_health_factor: 1.02,
            liquidation_threshold: 0.9,
            max_unwind_pct_per_action: 0.35,
            cooldown_slots: 2,
            max_slippage_bps: 60,
            fee_floor_microlamports: 10_000,
            fee_ceiling_microlamports: 200_000,
        }
    }
}

#[derive(Debug, Clone)]
struct Position {
    wallet: String,
    collateral_sol: f64,
    stable_balance_usdc: f64,
    debt_usdc: f64,
}

impl Position {
    fn health_factor(&self, price: f64, liquidation_threshold: f64) -> f64 {
        if self.debt_usdc <= 0.0 {
            return 999.0;
        }

        let collateral_value = self.collateral_sol * price + self.stable_balance_usdc;
        (collateral_value * liquidation_threshold) / self.debt_usdc
    }
}

#[derive(Debug, Clone)]
struct MarketState {
    slot: u64,
    sol_price_usdc: f64,
    observed_priority_fee: u64,
    ts_ms: u64,
    source: String,
}

trait MarketSource {
    fn next(&mut self) -> Result<Option<MarketState>, AegisError>;
}

#[derive(Debug)]
struct ScriptedPythSource {
    idx: usize,
    script: Vec<(f64, u64)>,
}

impl ScriptedPythSource {
    fn from_env() -> Self {
        let raw = env::var("AEGIS_PRICE_SCRIPT").unwrap_or_else(|_| {
            "210,25000;208,25000;204,30000;198,55000;194,90000;191,150000;189,140000;193,75000;197,40000;200,22000".to_string()
        });

        let mut script = Vec::new();
        for pair in raw.split(';') {
            let parts: Vec<&str> = pair.split(',').collect();
            if parts.len() != 2 {
                continue;
            }
            let price = parts[0].trim().parse::<f64>().unwrap_or(200.0);
            let fee = parts[1].trim().parse::<u64>().unwrap_or(25_000);
            script.push((price, fee));
        }

        if script.is_empty() {
            script.push((200.0, 25_000));
        }

        Self { idx: 0, script }
    }
}

impl MarketSource for ScriptedPythSource {
    fn next(&mut self) -> Result<Option<MarketState>, AegisError> {
        if self.idx >= self.script.len() {
            return Ok(None);
        }

        let slot = self.idx as u64 + 1;
        let (sol_price_usdc, observed_priority_fee) = self.script[self.idx];
        self.idx += 1;

        Ok(Some(MarketState {
            slot,
            sol_price_usdc,
            observed_priority_fee,
            ts_ms: now_ms(),
            source: "scripted".to_string(),
        }))
    }
}

#[derive(Debug)]
struct LiveHeliusPythSource {
    client: HttpClient,
    helius_rpc_url: String,
    pyth_hermes_url: String,
    pyth_sol_feed_id: String,
    jupiter_price_url: String,
    jupiter_api_key: Option<String>,
    max_cycles: usize,
    emitted: usize,
}

impl LiveHeliusPythSource {
    fn new(cfg: &RuntimeConfig) -> Result<Self, AegisError> {
        let helius_rpc_url = cfg.helius_rpc_url.clone().ok_or_else(|| {
            AegisError::Config(
                "AEGIS_HELIUS_RPC_URL is required in live mode. Set it to your Helius HTTPS RPC URL"
                    .to_string(),
            )
        })?;

        let client = HttpClient::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|e| AegisError::Io(format!("failed to build HTTP client: {e}")))?;

        Ok(Self {
            client,
            helius_rpc_url,
            pyth_hermes_url: cfg.pyth_hermes_url.clone(),
            pyth_sol_feed_id: cfg.pyth_sol_feed_id.clone(),
            jupiter_price_url: cfg.jupiter_price_url.clone(),
            jupiter_api_key: cfg.jupiter_api_key.clone(),
            max_cycles: cfg.max_cycles,
            emitted: 0,
        })
    }

    fn rpc_post(&self, method: &str, params: Value) -> Result<Value, AegisError> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let res = self
            .client
            .post(&self.helius_rpc_url)
            .json(&payload)
            .send()
            .map_err(|e| AegisError::Data(format!("helius rpc {method} request failed: {e}")))?;

        let status = res.status();
        let body: Value = res
            .json()
            .map_err(|e| AegisError::Data(format!("helius rpc {method} decode failed: {e}")))?;

        if !status.is_success() {
            return Err(AegisError::Data(format!(
                "helius rpc {method} http {}: {}",
                status,
                body
            )));
        }

        if let Some(err) = body.get("error") {
            return Err(AegisError::Data(format!(
                "helius rpc {method} returned error: {err}"
            )));
        }

        body.get("result")
            .cloned()
            .ok_or_else(|| AegisError::Data(format!("helius rpc {method} missing result")))
    }

    fn fetch_slot(&self) -> Result<u64, AegisError> {
        let result = self.rpc_post("getSlot", json!([{"commitment": "processed"}]))?;
        value_to_u64(&result, "getSlot result")
    }

    fn fetch_priority_fee(&self) -> Result<u64, AegisError> {
        let result = self.rpc_post("getRecentPrioritizationFees", json!([[]]))?;
        let rows = result
            .as_array()
            .ok_or_else(|| AegisError::Data("getRecentPrioritizationFees result is not array".to_string()))?;

        if rows.is_empty() {
            return Ok(25_000);
        }

        let mut fees = rows
            .iter()
            .filter_map(|row| row.get("prioritizationFee"))
            .filter_map(|v| value_to_u64(v, "prioritizationFee").ok())
            .collect::<Vec<_>>();

        if fees.is_empty() {
            return Ok(25_000);
        }

        fees.sort_unstable();
        let idx = (fees.len() as f64 * 0.75).floor() as usize;
        Ok(fees[idx.min(fees.len() - 1)])
    }

    fn fetch_pyth_price(&self) -> Result<f64, AegisError> {
        let url = format!(
            "{}/v2/updates/price/latest?ids%5B%5D={}",
            self.pyth_hermes_url.trim_end_matches('/'),
            self.pyth_sol_feed_id
        );

        let res = self
            .client
            .get(url)
            .send()
            .map_err(|e| AegisError::Data(format!("pyth request failed: {e}")))?;

        let status = res.status();
        let body: Value = res
            .json()
            .map_err(|e| AegisError::Data(format!("pyth decode failed: {e}")))?;

        if !status.is_success() {
            return Err(AegisError::Data(format!(
                "pyth http {} body {}",
                status,
                body
            )));
        }

        let parsed = body
            .get("parsed")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .ok_or_else(|| AegisError::Data("pyth parsed[0] missing".to_string()))?;

        let price_obj = parsed
            .get("price")
            .ok_or_else(|| AegisError::Data("pyth parsed[0].price missing".to_string()))?;

        let price_raw = value_to_f64(
            price_obj
                .get("price")
                .ok_or_else(|| AegisError::Data("pyth price.price missing".to_string()))?,
            "pyth price",
        )?;
        let expo = value_to_i32(
            price_obj
                .get("expo")
                .ok_or_else(|| AegisError::Data("pyth price.expo missing".to_string()))?,
            "pyth expo",
        )?;

        Ok(price_raw * 10_f64.powi(expo))
    }

    fn fetch_jupiter_price(&self) -> Result<f64, AegisError> {
        let url = format!(
            "{}?ids={}",
            self.jupiter_price_url.trim_end_matches('/'),
            SOL_MINT
        );

        let mut req = self.client.get(url);
        if let Some(api_key) = &self.jupiter_api_key {
            req = req.header("x-api-key", api_key);
        }

        let res = req
            .send()
            .map_err(|e| AegisError::Data(format!("jupiter price request failed: {e}")))?;
        let status = res.status();
        let body: Value = res
            .json()
            .map_err(|e| AegisError::Data(format!("jupiter price decode failed: {e}")))?;

        if !status.is_success() {
            return Err(AegisError::Data(format!(
                "jupiter price http {} body {}",
                status,
                body
            )));
        }

        let node = body
            .get("data")
            .and_then(|v| v.get(SOL_MINT))
            .ok_or_else(|| AegisError::Data("jupiter data[SOL_MINT] missing".to_string()))?;

        if let Some(price) = node.get("usdPrice") {
            return value_to_f64(price, "jupiter usdPrice");
        }

        if let Some(price) = node.get("price") {
            return value_to_f64(price, "jupiter price");
        }

        Err(AegisError::Data(
            "jupiter price payload missing usdPrice/price".to_string(),
        ))
    }
}

impl MarketSource for LiveHeliusPythSource {
    fn next(&mut self) -> Result<Option<MarketState>, AegisError> {
        if self.emitted >= self.max_cycles {
            return Ok(None);
        }

        let slot = self.fetch_slot()?;
        let observed_priority_fee = self.fetch_priority_fee()?;
        let sol_price_usdc = match self.fetch_pyth_price() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("pyth fetch failed, fallback to jupiter price: {e}");
                self.fetch_jupiter_price()?
            }
        };

        self.emitted += 1;

        Ok(Some(MarketState {
            slot,
            sol_price_usdc,
            observed_priority_fee,
            ts_ms: now_ms(),
            source: "live-helius-pyth".to_string(),
        }))
    }
}

#[derive(Debug, Clone)]
enum Action {
    Hold,
    PartialUnwind {
        sell_sol: f64,
        target_repay_usdc: f64,
        priority_fee: u64,
    },
}

#[derive(Debug, Clone)]
struct Decision {
    slot: u64,
    health_factor: f64,
    price: f64,
    reason: String,
    action: Action,
    proof_of_thought: String,
}

struct RiskEngine {
    cfg: RiskConfig,
    last_action_slot: Option<u64>,
}

impl RiskEngine {
    fn new(cfg: RiskConfig) -> Self {
        Self {
            cfg,
            last_action_slot: None,
        }
    }

    fn decide(&mut self, position: &Position, market: &MarketState) -> Decision {
        let hf = position.health_factor(market.sol_price_usdc, self.cfg.liquidation_threshold);

        if hf >= self.cfg.danger_health_factor {
            return self.hold_decision(
                market.slot,
                hf,
                market.sol_price_usdc,
                "health factor above danger threshold",
            );
        }

        if self.in_cooldown(market.slot) {
            return self.hold_decision(
                market.slot,
                hf,
                market.sol_price_usdc,
                "cooldown active; previous unwind recently executed",
            );
        }

        if position.collateral_sol <= 0.0 || position.debt_usdc <= 0.0 {
            return self.hold_decision(
                market.slot,
                hf,
                market.sol_price_usdc,
                "no collateral or debt to unwind",
            );
        }

        let urgency = if hf < self.cfg.emergency_health_factor {
            "emergency"
        } else {
            "warning"
        };

        let slippage = self.cfg.max_slippage_bps as f64 / 10_000.0;
        let target_repay = self.required_repay(position, market.sol_price_usdc);
        let price_after_slippage = market.sol_price_usdc * (1.0 - slippage);

        let raw_sell = if price_after_slippage <= 0.0 {
            0.0
        } else {
            target_repay / price_after_slippage
        };

        let max_sell = position.collateral_sol * self.cfg.max_unwind_pct_per_action;
        let sell_sol = raw_sell.max(0.0).min(max_sell).min(position.collateral_sol);

        if sell_sol <= 0.000001 {
            return self.hold_decision(
                market.slot,
                hf,
                market.sol_price_usdc,
                "insufficient delta to improve health factor",
            );
        }

        let repay = sell_sol * price_after_slippage;
        let priority_fee = dynamic_priority_fee(
            market.observed_priority_fee,
            urgency,
            self.cfg.fee_floor_microlamports,
            self.cfg.fee_ceiling_microlamports,
        );

        let reason = format!(
            "hf {:.4} < {:.2}; urgency={}; sell {:.4} SOL to repay {:.2} USDC",
            hf, self.cfg.danger_health_factor, urgency, sell_sol, repay
        );

        let action = Action::PartialUnwind {
            sell_sol,
            target_repay_usdc: repay,
            priority_fee,
        };

        let proof = proof_of_thought(market.slot, hf, market.sol_price_usdc, &action, &self.cfg);

        Decision {
            slot: market.slot,
            health_factor: hf,
            price: market.sol_price_usdc,
            reason,
            action,
            proof_of_thought: proof,
        }
    }

    fn mark_action_executed(&mut self, slot: u64) {
        self.last_action_slot = Some(slot);
    }

    fn in_cooldown(&self, slot: u64) -> bool {
        self.last_action_slot
            .map(|last| slot.saturating_sub(last) <= self.cfg.cooldown_slots)
            .unwrap_or(false)
    }

    fn required_repay(&self, position: &Position, price: f64) -> f64 {
        let collateral_value = position.collateral_sol * price + position.stable_balance_usdc;
        let safety_value = collateral_value * self.cfg.liquidation_threshold;
        let desired_debt = safety_value / self.cfg.target_health_factor;

        if position.debt_usdc <= desired_debt {
            return 0.0;
        }

        position.debt_usdc - desired_debt
    }

    fn hold_decision(&self, slot: u64, hf: f64, price: f64, reason: &str) -> Decision {
        let action = Action::Hold;
        let proof = proof_of_thought(slot, hf, price, &action, &self.cfg);

        Decision {
            slot,
            health_factor: hf,
            price,
            reason: reason.to_string(),
            action,
            proof_of_thought: proof,
        }
    }
}

trait Executor {
    fn execute(
        &mut self,
        position: &mut Position,
        market: &MarketState,
        decision: &Decision,
        max_slippage_bps: u64,
    ) -> Result<ExecutionReceipt, AegisError>;
}

#[derive(Debug, Clone)]
struct ExecutionReceipt {
    slot: u64,
    action: String,
    tx_id: String,
    repaid_usdc: f64,
    sold_sol: f64,
    health_factor_after: f64,
    quote_source: String,
}

#[derive(Debug)]
struct SimulatedJupiterExecutor {
    quote_base_url: String,
    jupiter_api_key: Option<String>,
    use_live_quote: bool,
    client: HttpClient,
}

impl SimulatedJupiterExecutor {
    fn new(cfg: &RuntimeConfig) -> Result<Self, AegisError> {
        let client = HttpClient::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|e| AegisError::Io(format!("failed to build HTTP client: {e}")))?;

        Ok(Self {
            quote_base_url: cfg.jupiter_quote_url.clone(),
            jupiter_api_key: cfg.jupiter_api_key.clone(),
            use_live_quote: cfg.live_quote_execution,
            client,
        })
    }

    fn quote_swap_out_usdc(&self, sell_sol: f64, slippage_bps: u64) -> Result<f64, AegisError> {
        let lamports = (sell_sol * 1_000_000_000.0).max(0.0) as u64;
        if lamports == 0 {
            return Ok(0.0);
        }

        let url = format!(
            "{}/ultra/v1/order?inputMint={}&outputMint={}&amount={}&slippageBps={}",
            self.quote_base_url.trim_end_matches('/'),
            SOL_MINT,
            USDC_MINT,
            lamports,
            slippage_bps
        );

        let mut req = self.client.get(url);
        if let Some(api_key) = &self.jupiter_api_key {
            req = req.header("x-api-key", api_key);
        }

        let res = req
            .send()
            .map_err(|e| AegisError::Data(format!("jupiter quote request failed: {e}")))?;

        let status = res.status();
        let body: Value = res
            .json()
            .map_err(|e| AegisError::Data(format!("jupiter quote decode failed: {e}")))?;

        if !status.is_success() {
            return Err(AegisError::Data(format!(
                "jupiter quote http {} body {}",
                status,
                body
            )));
        }

        let out = extract_out_amount_micro_usdc(&body)
            .ok_or_else(|| AegisError::Data(format!("jupiter quote missing out amount: {body}")))?;

        Ok(out as f64 / 1_000_000.0)
    }
}

impl Executor for SimulatedJupiterExecutor {
    fn execute(
        &mut self,
        position: &mut Position,
        market: &MarketState,
        decision: &Decision,
        max_slippage_bps: u64,
    ) -> Result<ExecutionReceipt, AegisError> {
        match &decision.action {
            Action::Hold => Ok(ExecutionReceipt {
                slot: decision.slot,
                action: "hold".to_string(),
                tx_id: "none".to_string(),
                repaid_usdc: 0.0,
                sold_sol: 0.0,
                health_factor_after: position.health_factor(market.sol_price_usdc, 0.9),
                quote_source: "none".to_string(),
            }),
            Action::PartialUnwind {
                sell_sol,
                target_repay_usdc,
                priority_fee,
            } => {
                if *sell_sol <= 0.0 {
                    return Err(AegisError::Execution(
                        "invalid unwind amount; sell_sol <= 0".to_string(),
                    ));
                }

                if *sell_sol > position.collateral_sol {
                    return Err(AegisError::Execution(
                        "cannot unwind more SOL than collateral".to_string(),
                    ));
                }

                let (proceeds, quote_source) = if self.use_live_quote {
                    match self.quote_swap_out_usdc(*sell_sol, max_slippage_bps) {
                        Ok(out) => (out, "jupiter-ultra".to_string()),
                        Err(err) => {
                            eprintln!(
                                "jupiter quote failed, fallback to mark-price model: {}",
                                err
                            );
                            let slippage = max_slippage_bps as f64 / 10_000.0;
                            (
                                *sell_sol * market.sol_price_usdc * (1.0 - slippage),
                                "fallback-mark".to_string(),
                            )
                        }
                    }
                } else {
                    let slippage = max_slippage_bps as f64 / 10_000.0;
                    (
                        *sell_sol * market.sol_price_usdc * (1.0 - slippage),
                        "mark-price".to_string(),
                    )
                };

                let repay = proceeds.min(position.debt_usdc).min(*target_repay_usdc);

                position.collateral_sol -= sell_sol;
                position.stable_balance_usdc += proceeds - repay;
                position.debt_usdc -= repay;

                let hf_after = position.health_factor(market.sol_price_usdc, 0.9);
                let tx_id = synthetic_tx_id(decision.slot, *sell_sol, repay, *priority_fee);

                Ok(ExecutionReceipt {
                    slot: decision.slot,
                    action: "partial_unwind".to_string(),
                    tx_id,
                    repaid_usdc: repay,
                    sold_sol: *sell_sol,
                    health_factor_after: hf_after,
                    quote_source,
                })
            }
        }
    }
}

#[derive(Debug)]
enum AegisError {
    Config(String),
    Execution(String),
    Io(String),
    Data(String),
}

impl fmt::Display for AegisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AegisError::Config(msg) => write!(f, "config error: {msg}"),
            AegisError::Execution(msg) => write!(f, "execution error: {msg}"),
            AegisError::Io(msg) => write!(f, "io error: {msg}"),
            AegisError::Data(msg) => write!(f, "data error: {msg}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Scripted,
    Live,
}

#[derive(Debug, Clone)]
struct RuntimeConfig {
    mode: RunMode,
    poll_ms: u64,
    max_cycles: usize,
    dashboard_port: u16,
    run_dashboard: bool,
    history_len: usize,
    risk: RiskConfig,
    initial_position: Position,
    helius_rpc_url: Option<String>,
    pyth_hermes_url: String,
    pyth_sol_feed_id: String,
    jupiter_price_url: String,
    jupiter_quote_url: String,
    jupiter_api_key: Option<String>,
    live_quote_execution: bool,
    audit_log_path: String,
}

impl RuntimeConfig {
    fn from_env() -> Result<Self, AegisError> {
        let mode = match env::var("AEGIS_MODE")
            .unwrap_or_else(|_| "scripted".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "scripted" => RunMode::Scripted,
            "live" => RunMode::Live,
            other => {
                return Err(AegisError::Config(format!(
                    "AEGIS_MODE invalid '{other}'. expected scripted|live"
                )));
            }
        };

        let risk = RiskConfig {
            danger_health_factor: parse_env_f64("AEGIS_DANGER_HF", 1.08)?,
            target_health_factor: parse_env_f64("AEGIS_TARGET_HF", 1.25)?,
            emergency_health_factor: parse_env_f64("AEGIS_EMERGENCY_HF", 1.02)?,
            liquidation_threshold: parse_env_f64("AEGIS_LIQ_THRESHOLD", 0.9)?,
            max_unwind_pct_per_action: parse_env_f64("AEGIS_MAX_UNWIND_PCT", 0.35)?,
            cooldown_slots: parse_env_u64("AEGIS_COOLDOWN_SLOTS", 2)?,
            max_slippage_bps: parse_env_u64("AEGIS_MAX_SLIPPAGE_BPS", 60)?,
            fee_floor_microlamports: parse_env_u64("AEGIS_FEE_FLOOR", 10_000)?,
            fee_ceiling_microlamports: parse_env_u64("AEGIS_FEE_CEIL", 200_000)?,
        };

        let initial_position = Position {
            wallet: env::var("AEGIS_WALLET").unwrap_or_else(|_| "demo-wallet".to_string()),
            collateral_sol: parse_env_f64("AEGIS_COLLATERAL_SOL", 18.0)?,
            stable_balance_usdc: parse_env_f64("AEGIS_STABLE_BALANCE", 300.0)?,
            debt_usdc: parse_env_f64("AEGIS_DEBT_USDC", 3300.0)?,
        };

        let helius_rpc_url = env::var("AEGIS_HELIUS_RPC_URL").ok();

        if mode == RunMode::Live && helius_rpc_url.is_none() {
            return Err(AegisError::Config(
                "AEGIS_HELIUS_RPC_URL is required for AEGIS_MODE=live".to_string(),
            ));
        }

        Ok(Self {
            mode,
            poll_ms: parse_env_u64("AEGIS_POLL_MS", 700)?,
            max_cycles: parse_env_usize("AEGIS_MAX_CYCLES", 20)?,
            dashboard_port: parse_env_u16("AEGIS_DASHBOARD_PORT", 8080)?,
            run_dashboard: parse_env_bool("AEGIS_DASHBOARD", true),
            history_len: parse_env_usize("AEGIS_HISTORY", 25)?,
            risk,
            initial_position,
            helius_rpc_url,
            pyth_hermes_url: env::var("AEGIS_PYTH_HERMES_URL")
                .unwrap_or_else(|_| "https://hermes.pyth.network".to_string()),
            pyth_sol_feed_id: env::var("AEGIS_PYTH_SOL_FEED_ID")
                .unwrap_or_else(|_| DEFAULT_PYTH_SOL_FEED_ID.to_string()),
            jupiter_price_url: env::var("AEGIS_JUPITER_PRICE_URL")
                .unwrap_or_else(|_| "https://lite-api.jup.ag/price/v3".to_string()),
            jupiter_quote_url: env::var("AEGIS_JUPITER_QUOTE_URL")
                .unwrap_or_else(|_| "https://lite-api.jup.ag".to_string()),
            jupiter_api_key: env::var("AEGIS_JUPITER_API_KEY").ok(),
            live_quote_execution: parse_env_bool("AEGIS_LIVE_QUOTE_EXEC", true),
            audit_log_path: env::var("AEGIS_AUDIT_LOG")
                .unwrap_or_else(|_| "aegis_actions.csv".to_string()),
        })
    }
}

#[derive(Debug, Clone)]
struct DashboardState {
    wallet: String,
    last_slot: u64,
    last_price: f64,
    health_factor: f64,
    collateral_sol: f64,
    stable_balance_usdc: f64,
    debt_usdc: f64,
    last_action: String,
    last_reason: String,
    last_proof: String,
    last_source: String,
    decisions: VecDeque<String>,
    receipts: VecDeque<String>,
}

impl DashboardState {
    fn new(position: &Position, history_len: usize) -> Self {
        let mut decisions = VecDeque::new();
        decisions.push_back("aegis booted".to_string());

        Self {
            wallet: position.wallet.clone(),
            last_slot: 0,
            last_price: 0.0,
            health_factor: 0.0,
            collateral_sol: position.collateral_sol,
            stable_balance_usdc: position.stable_balance_usdc,
            debt_usdc: position.debt_usdc,
            last_action: "boot".to_string(),
            last_reason: "startup".to_string(),
            last_proof: "none".to_string(),
            last_source: "none".to_string(),
            decisions,
            receipts: VecDeque::with_capacity(history_len),
        }
    }

    fn push_decision(&mut self, line: String, history_len: usize) {
        self.decisions.push_back(line);
        while self.decisions.len() > history_len {
            self.decisions.pop_front();
        }
    }

    fn push_receipt(&mut self, line: String, history_len: usize) {
        self.receipts.push_back(line);
        while self.receipts.len() > history_len {
            self.receipts.pop_front();
        }
    }
}

fn run_dashboard_server(state: Arc<Mutex<DashboardState>>, port: u16) -> Result<(), AegisError> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| AegisError::Io(format!("failed to bind dashboard on port {port}: {e}")))?;

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                if let Err(err) = handle_dashboard_request(&mut s, &state) {
                    eprintln!("dashboard request error: {err}");
                }
            }
            Err(err) => eprintln!("dashboard connection error: {err}"),
        }
    }

    Ok(())
}

fn handle_dashboard_request(
    stream: &mut TcpStream,
    state: &Arc<Mutex<DashboardState>>,
) -> Result<(), AegisError> {
    let mut buf = [0_u8; 1024];
    let _ = stream
        .read(&mut buf)
        .map_err(|e| AegisError::Io(format!("failed to read dashboard request bytes: {e}")))?;

    let body = {
        let snapshot = state
            .lock()
            .map_err(|_| AegisError::Io("dashboard state lock poisoned".to_string()))?;
        dashboard_json(&snapshot)
    };

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    stream
        .write_all(response.as_bytes())
        .map_err(|e| AegisError::Io(format!("failed to write dashboard response: {e}")))?;

    Ok(())
}

fn dashboard_json(state: &DashboardState) -> String {
    let decisions = state
        .decisions
        .iter()
        .map(|d| format!("\"{}\"", escape_json(d)))
        .collect::<Vec<_>>()
        .join(",");

    let receipts = state
        .receipts
        .iter()
        .map(|r| format!("\"{}\"", escape_json(r)))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "{{\"wallet\":\"{}\",\"slot\":{},\"price\":{:.4},\"health_factor\":{:.6},\"collateral_sol\":{:.6},\"stable_usdc\":{:.4},\"debt_usdc\":{:.4},\"last_action\":\"{}\",\"last_reason\":\"{}\",\"last_proof\":\"{}\",\"last_source\":\"{}\",\"decision_log\":[{}],\"receipt_log\":[{}]}}",
        escape_json(&state.wallet),
        state.last_slot,
        state.last_price,
        state.health_factor,
        state.collateral_sol,
        state.stable_balance_usdc,
        state.debt_usdc,
        escape_json(&state.last_action),
        escape_json(&state.last_reason),
        escape_json(&state.last_proof),
        escape_json(&state.last_source),
        decisions,
        receipts
    )
}

fn dynamic_priority_fee(observed: u64, urgency: &str, floor: u64, ceiling: u64) -> u64 {
    let scaled = match urgency {
        "emergency" => observed.saturating_mul(2),
        _ => observed.saturating_mul(6) / 5,
    };

    scaled.max(floor).min(ceiling)
}

fn proof_of_thought(
    slot: u64,
    health_factor: f64,
    price: f64,
    action: &Action,
    cfg: &RiskConfig,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    slot.hash(&mut hasher);
    health_factor.to_bits().hash(&mut hasher);
    price.to_bits().hash(&mut hasher);
    cfg.danger_health_factor.to_bits().hash(&mut hasher);
    cfg.target_health_factor.to_bits().hash(&mut hasher);
    cfg.emergency_health_factor.to_bits().hash(&mut hasher);
    cfg.max_unwind_pct_per_action.to_bits().hash(&mut hasher);

    match action {
        Action::Hold => {
            0_u8.hash(&mut hasher);
        }
        Action::PartialUnwind {
            sell_sol,
            target_repay_usdc,
            priority_fee,
        } => {
            1_u8.hash(&mut hasher);
            sell_sol.to_bits().hash(&mut hasher);
            target_repay_usdc.to_bits().hash(&mut hasher);
            priority_fee.hash(&mut hasher);
        }
    }

    format!("proof-{:#016x}", hasher.finish())
}

fn synthetic_tx_id(slot: u64, sell_sol: f64, repay: f64, priority_fee: u64) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    slot.hash(&mut hasher);
    sell_sol.to_bits().hash(&mut hasher);
    repay.to_bits().hash(&mut hasher);
    priority_fee.hash(&mut hasher);
    format!("tx-{:#016x}", hasher.finish())
}

fn append_audit_log(
    path: &str,
    market: &MarketState,
    decision: &Decision,
    receipt: &ExecutionReceipt,
) -> Result<(), AegisError> {
    let file_exists = std::path::Path::new(path).exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| AegisError::Io(format!("failed to open {}: {e}", path)))?;

    if !file_exists {
        writeln!(
            file,
            "ts_ms,slot,source,price,hf,action,sold_sol,repaid_usdc,quote_source,tx_id,proof,reason"
        )
        .map_err(|e| AegisError::Io(format!("failed to write log header: {e}")))?;
    }

    writeln!(
        file,
        "{},{},{},{:.4},{:.6},{},{:.6},{:.4},{},{},{},{}",
        market.ts_ms,
        decision.slot,
        market.source,
        decision.price,
        decision.health_factor,
        receipt.action,
        receipt.sold_sol,
        receipt.repaid_usdc,
        receipt.quote_source,
        receipt.tx_id,
        decision.proof_of_thought,
        sanitize_csv_field(&decision.reason)
    )
    .map_err(|e| AegisError::Io(format!("failed to append log row: {e}")))?;

    Ok(())
}

fn parse_env_f64(name: &str, default: f64) -> Result<f64, AegisError> {
    match env::var(name) {
        Ok(v) => v
            .parse::<f64>()
            .map_err(|_| AegisError::Config(format!("{name} must be a number"))),
        Err(_) => Ok(default),
    }
}

fn parse_env_u64(name: &str, default: u64) -> Result<u64, AegisError> {
    match env::var(name) {
        Ok(v) => v
            .parse::<u64>()
            .map_err(|_| AegisError::Config(format!("{name} must be an integer"))),
        Err(_) => Ok(default),
    }
}

fn parse_env_u16(name: &str, default: u16) -> Result<u16, AegisError> {
    match env::var(name) {
        Ok(v) => v
            .parse::<u16>()
            .map_err(|_| AegisError::Config(format!("{name} must be an integer"))),
        Err(_) => Ok(default),
    }
}

fn parse_env_usize(name: &str, default: usize) -> Result<usize, AegisError> {
    match env::var(name) {
        Ok(v) => v
            .parse::<usize>()
            .map_err(|_| AegisError::Config(format!("{name} must be an integer"))),
        Err(_) => Ok(default),
    }
}

fn parse_env_bool(name: &str, default: bool) -> bool {
    env::var(name)
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(default)
}

fn sanitize_csv_field(s: &str) -> String {
    s.replace(',', " ")
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis() as u64
}

fn value_to_u64(v: &Value, field: &str) -> Result<u64, AegisError> {
    if let Some(n) = v.as_u64() {
        return Ok(n);
    }

    if let Some(s) = v.as_str() {
        return s
            .parse::<u64>()
            .map_err(|_| AegisError::Data(format!("{field} is not a u64")));
    }

    Err(AegisError::Data(format!("{field} is not u64/string")))
}

fn value_to_i32(v: &Value, field: &str) -> Result<i32, AegisError> {
    if let Some(n) = v.as_i64() {
        return i32::try_from(n)
            .map_err(|_| AegisError::Data(format!("{field} out of i32 range")));
    }

    if let Some(s) = v.as_str() {
        return s
            .parse::<i32>()
            .map_err(|_| AegisError::Data(format!("{field} is not an i32")));
    }

    Err(AegisError::Data(format!("{field} is not i32/string")))
}

fn value_to_f64(v: &Value, field: &str) -> Result<f64, AegisError> {
    if let Some(n) = v.as_f64() {
        return Ok(n);
    }

    if let Some(n) = v.as_i64() {
        return Ok(n as f64);
    }

    if let Some(s) = v.as_str() {
        return s
            .parse::<f64>()
            .map_err(|_| AegisError::Data(format!("{field} is not a float")));
    }

    Err(AegisError::Data(format!("{field} is not number/string")))
}

fn extract_out_amount_micro_usdc(body: &Value) -> Option<u64> {
    for key in [
        "outAmount",
        "outputAmount",
        "outAmountWithSlippage",
        "otherAmountThreshold",
    ] {
        if let Some(v) = body.get(key) {
            if let Ok(n) = value_to_u64(v, key) {
                return Some(n);
            }
        }
    }

    if let Some(route_plan) = body.get("routePlan").and_then(Value::as_array) {
        for step in route_plan {
            if let Some(swap_info) = step.get("swapInfo") {
                for key in ["outAmount", "outputAmount"] {
                    if let Some(v) = swap_info.get(key)
                        && let Ok(n) = value_to_u64(v, key)
                    {
                        return Some(n);
                    }
                }
            }
        }
    }

    None
}

fn print_boot(cfg: &RuntimeConfig) {
    println!("Aegis Protocol boot");
    println!(
        "mode={:?} wallet={} collateral_sol={:.3} stable={:.2} debt={:.2}",
        cfg.mode,
        cfg.initial_position.wallet,
        cfg.initial_position.collateral_sol,
        cfg.initial_position.stable_balance_usdc,
        cfg.initial_position.debt_usdc
    );
    println!(
        "risk: danger_hf={:.2} target_hf={:.2} emergency_hf={:.2} max_unwind={}%%",
        cfg.risk.danger_health_factor,
        cfg.risk.target_health_factor,
        cfg.risk.emergency_health_factor,
        (cfg.risk.max_unwind_pct_per_action * 100.0) as u64
    );
    println!(
        "runtime: poll_ms={} max_cycles={} live_quote_exec={}",
        cfg.poll_ms, cfg.max_cycles, cfg.live_quote_execution
    );

    if cfg.run_dashboard {
        println!(
            "dashboard: http://127.0.0.1:{} (JSON endpoint)",
            cfg.dashboard_port
        );
    }
}

fn main() {
    let cfg = match RuntimeConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    print_boot(&cfg);

    let dashboard_state = Arc::new(Mutex::new(DashboardState::new(
        &cfg.initial_position,
        cfg.history_len,
    )));

    if cfg.run_dashboard {
        let state_clone = Arc::clone(&dashboard_state);
        let port = cfg.dashboard_port;
        thread::spawn(move || {
            if let Err(e) = run_dashboard_server(state_clone, port) {
                eprintln!("dashboard stopped: {e}");
            }
        });
    }

    let mut source: Box<dyn MarketSource> = match cfg.mode {
        RunMode::Scripted => Box::new(ScriptedPythSource::from_env()),
        RunMode::Live => match LiveHeliusPythSource::new(&cfg) {
            Ok(src) => Box::new(src),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(2);
            }
        },
    };

    let mut position = cfg.initial_position.clone();
    let mut risk_engine = RiskEngine::new(cfg.risk.clone());
    let mut executor = match SimulatedJupiterExecutor::new(&cfg) {
        Ok(ex) => ex,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let mut counters: HashMap<&'static str, u64> = HashMap::new();
    let mut last_price: Option<f64> = None;

    loop {
        let market = match source.next() {
            Ok(Some(s)) => s,
            Ok(None) => break,
            Err(e) => {
                eprintln!("source error: {e}");
                thread::sleep(Duration::from_millis(cfg.poll_ms));
                continue;
            }
        };

        last_price = Some(market.sol_price_usdc);

        let decision = risk_engine.decide(&position, &market);
        let receipt = match executor.execute(&mut position, &market, &decision, cfg.risk.max_slippage_bps)
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("execution failed @slot {}: {}", market.slot, e);
                thread::sleep(Duration::from_millis(cfg.poll_ms));
                continue;
            }
        };

        if matches!(decision.action, Action::PartialUnwind { .. }) {
            risk_engine.mark_action_executed(market.slot);
            *counters.entry("unwind").or_insert(0) += 1;
        } else {
            *counters.entry("hold").or_insert(0) += 1;
        }

        if let Err(e) = append_audit_log(&cfg.audit_log_path, &market, &decision, &receipt) {
            eprintln!("failed to append audit log: {e}");
        }

        let hf_after = position.health_factor(market.sol_price_usdc, cfg.risk.liquidation_threshold);

        {
            let mut state = match dashboard_state.lock() {
                Ok(s) => s,
                Err(_) => {
                    eprintln!("dashboard state lock poisoned");
                    thread::sleep(Duration::from_millis(cfg.poll_ms));
                    continue;
                }
            };

            state.last_slot = market.slot;
            state.last_price = market.sol_price_usdc;
            state.health_factor = hf_after;
            state.collateral_sol = position.collateral_sol;
            state.stable_balance_usdc = position.stable_balance_usdc;
            state.debt_usdc = position.debt_usdc;
            state.last_action = receipt.action.clone();
            state.last_reason = decision.reason.clone();
            state.last_proof = decision.proof_of_thought.clone();
            state.last_source = market.source.clone();
            state.push_decision(
                format!(
                    "slot {} src={} hf={:.4} action={} reason={}",
                    market.slot, market.source, decision.health_factor, receipt.action, decision.reason
                ),
                cfg.history_len,
            );
            state.push_receipt(
                format!(
                    "slot {} tx={} sold={:.4} repaid={:.2} hf_after={:.4} quote={}",
                    receipt.slot,
                    receipt.tx_id,
                    receipt.sold_sol,
                    receipt.repaid_usdc,
                    receipt.health_factor_after,
                    receipt.quote_source
                ),
                cfg.history_len,
            );
        }

        println!(
            "[slot {} {}] price={:.2} fee={} hf_before={:.4} action={} sold={:.4} repaid={:.2} hf_after={:.4} quote={} proof={}",
            market.slot,
            market.source,
            market.sol_price_usdc,
            market.observed_priority_fee,
            decision.health_factor,
            receipt.action,
            receipt.sold_sol,
            receipt.repaid_usdc,
            hf_after,
            receipt.quote_source,
            decision.proof_of_thought
        );

        thread::sleep(Duration::from_millis(cfg.poll_ms));
    }

    println!("\n=== Aegis run summary ===");
    println!("holds: {}", counters.get("hold").copied().unwrap_or(0));
    println!("unwinds: {}", counters.get("unwind").copied().unwrap_or(0));
    println!("final collateral_sol: {:.4}", position.collateral_sol);
    println!("final stable_balance_usdc: {:.2}", position.stable_balance_usdc);
    println!("final debt_usdc: {:.2}", position.debt_usdc);
    println!(
        "final health factor @last price: {:.4}",
        position.health_factor(last_price.unwrap_or(200.0), cfg.risk.liquidation_threshold)
    );
    println!("audit log: {}", cfg.audit_log_path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_unwind_when_health_factor_drops() {
        let mut engine = RiskEngine::new(RiskConfig::default());
        let position = Position {
            wallet: "w1".to_string(),
            collateral_sol: 10.0,
            stable_balance_usdc: 100.0,
            debt_usdc: 1800.0,
        };

        let market = MarketState {
            slot: 1,
            sol_price_usdc: 170.0,
            observed_priority_fee: 50_000,
            ts_ms: 1,
            source: "test".to_string(),
        };

        let decision = engine.decide(&position, &market);
        assert!(matches!(decision.action, Action::PartialUnwind { .. }));
    }

    #[test]
    fn holds_when_health_factor_is_safe() {
        let mut engine = RiskEngine::new(RiskConfig::default());
        let position = Position {
            wallet: "w2".to_string(),
            collateral_sol: 20.0,
            stable_balance_usdc: 500.0,
            debt_usdc: 1500.0,
        };

        let market = MarketState {
            slot: 1,
            sol_price_usdc: 220.0,
            observed_priority_fee: 20_000,
            ts_ms: 1,
            source: "test".to_string(),
        };

        let decision = engine.decide(&position, &market);
        assert!(matches!(decision.action, Action::Hold));
    }

    #[test]
    fn proof_is_stable_for_same_inputs() {
        let cfg = RiskConfig::default();
        let action = Action::PartialUnwind {
            sell_sol: 1.25,
            target_repay_usdc: 240.0,
            priority_fee: 42_000,
        };

        let p1 = proof_of_thought(7, 1.01, 193.0, &action, &cfg);
        let p2 = proof_of_thought(7, 1.01, 193.0, &action, &cfg);
        assert_eq!(p1, p2);
    }

    #[test]
    fn dynamic_fee_scales_with_urgency() {
        let warning = dynamic_priority_fee(30_000, "warning", 10_000, 200_000);
        let emergency = dynamic_priority_fee(30_000, "emergency", 10_000, 200_000);
        assert!(emergency > warning);
    }

    #[test]
    fn extract_out_amount_parses_string_and_number_fields() {
        let as_string = json!({"outAmount": "42000000"});
        let as_number = json!({"outputAmount": 1100000});
        assert_eq!(extract_out_amount_micro_usdc(&as_string), Some(42_000_000));
        assert_eq!(extract_out_amount_micro_usdc(&as_number), Some(1_100_000));
    }
}
