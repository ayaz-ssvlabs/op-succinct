use std::time::Duration;

use alloy_primitives::{hex, keccak256};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kona_host::{
    single::{SingleChainHintHandler, SingleChainHost, SingleChainHostError, SingleChainProviders},
    HintHandler, OfflineHostBackend, OnlineHostBackend, OnlineHostBackendCfg, PreimageServer,
    SharedKeyValueStore,
};
use kona_preimage::{Channel, HintReader, OracleServer, PreimageKey, PreimageKeyType};
use kona_proof::Hint;
use op_succinct_altda_client_utils::{parse_altda_commitment, AltDACommitmentType, AltDAHintType};
use op_succinct_host_utils::host::PreimageServerStarter;
use tokio::task::{self, JoinHandle};
use tracing::{debug, warn};

const DEFAULT_ALTDA_GET_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone)]
pub struct SingleChainHostWithAltDA {
    pub single_host: SingleChainHost,
    pub altda_da_server: Option<String>,
    pub altda_get_timeout_secs: u64,
}

impl SingleChainHostWithAltDA {
    pub fn from_single_host(single_host: SingleChainHost) -> Self {
        let altda_da_server = std::env::var("ALTDA_DA_SERVER").ok();
        let altda_get_timeout_secs = std::env::var("ALTDA_GET_TIMEOUT")
            .ok()
            .and_then(|raw| parse_timeout_seconds(&raw))
            .filter(|secs| *secs > 0)
            .unwrap_or(DEFAULT_ALTDA_GET_TIMEOUT_SECS);

        Self { single_host, altda_da_server, altda_get_timeout_secs }
    }

    pub const fn is_offline(&self) -> bool {
        self.single_host.is_offline()
    }

    pub async fn start_server<C>(
        &self,
        hint: C,
        preimage: C,
    ) -> Result<JoinHandle<Result<(), SingleChainHostError>>, SingleChainHostError>
    where
        C: Channel + Send + Sync + 'static,
    {
        let kv_store = self.single_host.create_key_value_store()?;

        let task_handle = if self.is_offline() {
            task::spawn(async {
                PreimageServer::new(
                    OracleServer::new(preimage),
                    HintReader::new(hint),
                    std::sync::Arc::new(OfflineHostBackend::new(kv_store)),
                )
                .start()
                .await
                .map_err(SingleChainHostError::from)
            })
        } else {
            let providers = self.single_host.create_providers().await?;
            // Avoid proactive prefetch for AltDA until the hint path is fully stabilized.
            // Proactive L2 payload witness prefetch can issue non-canonical header lookups
            // and close the preimage channel when the header is unavailable.
            let backend =
                OnlineHostBackend::new(self.clone(), kv_store.clone(), providers, AltDAHintHandler);

            task::spawn(async {
                PreimageServer::new(
                    OracleServer::new(preimage),
                    HintReader::new(hint),
                    std::sync::Arc::new(backend),
                )
                .start()
                .await
                .map_err(SingleChainHostError::from)
            })
        };

        Ok(task_handle)
    }

    fn resolve_da_server(&self) -> Option<String> {
        self.altda_da_server.clone().or_else(|| std::env::var("ALTDA_DA_SERVER").ok())
    }

    fn resolve_get_timeout_secs(&self) -> u64 {
        std::env::var("ALTDA_GET_TIMEOUT")
            .ok()
            .and_then(|raw| parse_timeout_seconds(&raw))
            .filter(|secs| *secs > 0)
            .unwrap_or(self.altda_get_timeout_secs.max(1))
    }
}

#[async_trait]
impl PreimageServerStarter for SingleChainHostWithAltDA {
    async fn start_server<C>(
        &self,
        hint: C,
        preimage: C,
    ) -> Result<JoinHandle<Result<(), SingleChainHostError>>, SingleChainHostError>
    where
        C: Channel + Send + Sync + 'static,
    {
        self.start_server(hint, preimage).await
    }
}

impl OnlineHostBackendCfg for SingleChainHostWithAltDA {
    type HintType = AltDAHintType;
    type Providers = SingleChainProviders;
}

#[derive(Debug, Clone, Copy)]
pub struct AltDAHintHandler;

#[async_trait]
impl HintHandler for AltDAHintHandler {
    type Cfg = SingleChainHostWithAltDA;

    async fn fetch_hint(
        hint: Hint<<Self::Cfg as OnlineHostBackendCfg>::HintType>,
        cfg: &Self::Cfg,
        providers: &<Self::Cfg as OnlineHostBackendCfg>::Providers,
        kv: SharedKeyValueStore,
    ) -> Result<()> {
        match hint.ty {
            AltDAHintType::Standard(standard_hint) => {
                let inner_hint = Hint { ty: standard_hint, data: hint.data };
                SingleChainHintHandler::fetch_hint(inner_hint, &cfg.single_host, providers, kv)
                    .await?;
            }
            AltDAHintType::AltDACommitment => {
                let commitment = hint.data.to_vec();
                let parsed = parse_altda_commitment(&commitment)
                    .map_err(|e| anyhow!("invalid AltDA commitment hint: {e}"))?;

                let da_server = cfg
                    .resolve_da_server()
                    .ok_or_else(|| anyhow!("ALTDA_DA_SERVER is required for AltDA hints"))?;
                let get_url = format!(
                    "{}/get/0x{}",
                    da_server.trim_end_matches('/'),
                    hex::encode(&commitment)
                );
                let timeout_secs = cfg.resolve_get_timeout_secs();

                debug!(target: "altda_hint_handler", "Fetching AltDA input from {}", get_url);
                let response = reqwest::Client::new()
                    .get(&get_url)
                    .timeout(Duration::from_secs(timeout_secs))
                    .send()
                    .await
                    .map_err(|e| anyhow!("failed to fetch AltDA input: {e}"))?;

                if !response.status().is_success() {
                    return Err(anyhow!(
                        "AltDA server returned status {} for {}",
                        response.status(),
                        get_url
                    ));
                }

                let input = response
                    .bytes()
                    .await
                    .map_err(|e| anyhow!("failed to read AltDA input response body: {e}"))?;

                if parsed.commitment_type == AltDACommitmentType::Keccak256 &&
                    keccak256(input.as_ref()).as_slice() != parsed.payload.as_slice()
                {
                    return Err(anyhow!(
                        "AltDA keccak commitment mismatch for commitment 0x{}",
                        hex::encode(&commitment)
                    ));
                }

                let mut kv_lock = kv.write().await;
                let key = PreimageKey::new(*keccak256(&commitment), PreimageKeyType::GlobalGeneric);
                kv_lock.set(key.into(), input.to_vec().into())?;

                warn!(
                    target: "altda_hint_handler",
                    "Loaded AltDA input for commitment 0x{} ({} bytes)",
                    hex::encode(commitment),
                    input.len()
                );
            }
        }

        Ok(())
    }
}

fn parse_timeout_seconds(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(seconds) = trimmed.strip_suffix('s') {
        return seconds.trim().parse::<u64>().ok();
    }

    trimmed.parse::<u64>().ok()
}
