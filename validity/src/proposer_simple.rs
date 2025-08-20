use std::{collections::HashMap, str::FromStr, sync::Arc, time::Duration};

use alloy_primitives::{Address, B256};
use anyhow::Result;
use futures_util::{stream, StreamExt, TryStreamExt};
use op_succinct_client_utils::boot::BootInfoStruct;
use op_succinct_host_utils::{
    fetcher::OPSuccinctDataFetcher, host::OPSuccinctHost,
    DisputeGameFactory::DisputeGameFactoryInstance as DisputeGameFactoryContract,
    OPSuccinctL2OutputOracle::OPSuccinctL2OutputOracleInstance as OPSuccinctL2OOContract,
};
use op_succinct_proof_utils::get_range_elf_embedded;
use op_succinct_signer_utils::Signer;
use sp1_sdk::{
    CudaProver, HashableKey, Prover, SP1Proof, SP1ProofWithPublicValues,
};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::{
    db::{DriverDBClient, OPSuccinctRequest, RequestMode, RequestStatus},
    find_gaps, get_latest_proposed_block_number, CommitmentConfig,
    ContractConfig, OPSuccinctProofRequester, ProgramConfig, RequesterConfig, ValidityGauge,
    create_cuda_prover,
};

/// Configuration for the CUDA-only validity proposer driver.
pub struct DriverConfig {
    pub cuda_prover: Arc<CudaProver>,
    pub fetcher: Arc<OPSuccinctDataFetcher>,
    pub driver_db_client: Arc<DriverDBClient>,
    pub signer: Signer,
    pub loop_interval: u64,
}

/// Type alias for a map of task IDs to their join handles and associated requests
pub type TaskMap = HashMap<i64, (tokio::task::JoinHandle<Result<()>>, OPSuccinctRequest)>;

pub struct Proposer<P, H: OPSuccinctHost>
where
    P: alloy_provider::Provider + Clone + Send + Sync + 'static,
    H: Clone + Send + Sync + 'static,
{
    pub driver_config: DriverConfig,
    pub contract_config: ContractConfig<P>,
    pub proof_requester: Arc<OPSuccinctProofRequester<H>>,
    pub requester_config: RequesterConfig,
    pub task_map: Arc<Mutex<TaskMap>>,
}

impl<P, H: OPSuccinctHost> Proposer<P, H>
where
    P: alloy_provider::Provider + Clone + Send + Sync + 'static,
    H: Clone + Send + Sync + 'static,
{
    pub async fn new(
        provider: P,
        db_client: Arc<DriverDBClient>,
        fetcher: Arc<OPSuccinctDataFetcher>,
        requester_config: RequesterConfig,
        signer: Signer,
        loop_interval: u64,
        host: Arc<H>,
    ) -> Result<Self> {
        // This check prevents users from running multiple proposers for the same chain at the same
        // time.
        let is_locked = db_client
            .is_chain_locked(requester_config.l1_chain_id, requester_config.l2_chain_id)
            .await?;

        if is_locked {
            return Err(anyhow::anyhow!(
                "Chain is locked. Please wait for the existing proposer to finish or remove the lock."
            ));
        }

        // Add the chain lock to the database.
        db_client
            .add_chain_lock(requester_config.l1_chain_id, requester_config.l2_chain_id)
            .await?;

        let cuda_prover = create_cuda_prover()?;

        let (range_pk, range_vk) = cuda_prover.setup(get_range_elf_embedded());
        let (agg_pk, agg_vk) = cuda_prover.setup(op_succinct_elfs::AGGREGATION_ELF);
        let multi_block_vkey_u8 = u32_to_u8(range_vk.vk.hash_u32());
        let range_vkey_commitment = B256::from(multi_block_vkey_u8);
        let agg_vkey_hash = B256::from_str(&agg_vk.bytes32()).unwrap();

        // Initialize fetcher
        let rollup_config_hash = hash_rollup_config(fetcher.rollup_config.as_ref().unwrap());

        let program_config = ProgramConfig {
            range_vk: Arc::new(range_vk),
            range_pk: Arc::new(range_pk),
            agg_vk: Arc::new(agg_vk),
            agg_pk: Arc::new(agg_pk),
            commitments: CommitmentConfig {
                range_vkey_commitment,
                agg_vkey_hash,
                rollup_config_hash,
            },
        };

        // Initialize the proof requester.
        let proof_requester = Arc::new(OPSuccinctProofRequester::new(
            host,
            cuda_prover.clone(),
            fetcher.clone(),
            db_client.clone(),
            program_config.clone(),
            requester_config.mock,
            requester_config.agg_proof_mode,
            requester_config.safe_db_fallback,
        ));

        let l2oo_contract =
            OPSuccinctL2OOContract::new(requester_config.l2oo_address, provider.clone());

        let dgf_contract =
            DisputeGameFactoryContract::new(requester_config.dgf_address, provider.clone());

        let proposer = Proposer {
            driver_config: DriverConfig {
                cuda_prover,
                fetcher,
                driver_db_client: db_client,
                signer,
                loop_interval,
            },
            contract_config: ContractConfig {
                l2oo_address: requester_config.l2oo_address,
                dgf_address: requester_config.dgf_address,
                l2oo_contract,
                dgf_contract,
            },
            proof_requester,
            requester_config,
            task_map: Arc::new(Mutex::new(HashMap::new())),
        };

        Ok(proposer)
    }

    /// CUDA-only simplified run loop - just process requests synchronously
    pub async fn run(&self) -> Result<()> {
        info!("Starting CUDA-only validity proposer loop...");
        
        loop {
            // Process any pending requests
            if let Err(e) = self.process_pending_requests().await {
                warn!("Error processing pending requests: {}", e);
            }

            // Create new range proof requests
            if let Err(e) = self.create_range_proof_requests().await {
                warn!("Error creating range proof requests: {}", e);
            }

            // Submit completed aggregation proofs
            if let Err(e) = self.submit_completed_aggregation_proofs().await {
                warn!("Error submitting aggregation proofs: {}", e);
            }

            // Wait before next iteration
            tokio::time::sleep(Duration::from_secs(self.driver_config.loop_interval)).await;
        }
    }

    /// Process all pending proof requests (CUDA mode - synchronous)
    async fn process_pending_requests(&self) -> Result<()> {
        let pending_requests = self.driver_config.driver_db_client
            .get_requests_by_status(RequestStatus::WitnessGeneration, 50)
            .await?;

        info!("Processing {} pending requests", pending_requests.len());

        for request in pending_requests {
            // In CUDA mode, proof generation is synchronous and happens in make_proof_request
            if let Err(e) = self.proof_requester.make_proof_request(request.clone()).await {
                warn!("Failed to process request {}: {}", request.id, e);
                // Mark as failed for retry
                self.driver_config.driver_db_client
                    .update_request_status(request.id, RequestStatus::Failed)
                    .await?;
            }
        }

        Ok(())
    }

    /// Create new range proof requests based on latest blocks
    async fn create_range_proof_requests(&self) -> Result<()> {
        let finalized_block_number = get_latest_proposed_block_number(
            &self.contract_config.l2oo_contract,
            &self.contract_config.dgf_contract,
        ).await?;

        let latest_request = self.driver_config.driver_db_client
            .get_latest_request_by_type_and_status(
                crate::db::RequestType::Range,
                vec![RequestStatus::Complete],
                self.requester_config.l1_chain_id,
                self.requester_config.l2_chain_id,
                &self.proof_requester.program_config.commitments,
            )
            .await?;

        let latest_proposed_block_number = latest_request
            .map(|r| r.end_block as u64)
            .unwrap_or(finalized_block_number);

        if latest_proposed_block_number >= finalized_block_number {
            debug!("No new blocks to prove");
            return Ok(());
        }

        // Create requests for the gap
        let gap_start = latest_proposed_block_number + 1;
        let gap_end = finalized_block_number;
        
        info!("Creating range proof request for blocks {} to {}", gap_start, gap_end);

        let mode = if self.requester_config.mock {
            RequestMode::Mock
        } else {
            RequestMode::Live
        };

        let range_request = OPSuccinctRequest::create_range_request(
            mode,
            gap_start as i64,
            gap_end as i64,
            self.requester_config.l1_chain_id,
            self.requester_config.l2_chain_id,
            &self.proof_requester.program_config.commitments,
        );

        self.driver_config.driver_db_client
            .add_request(&range_request)
            .await?;

        Ok(())
    }

    /// Submit any completed aggregation proofs
    async fn submit_completed_aggregation_proofs(&self) -> Result<()> {
        let completed_agg_requests = self.driver_config.driver_db_client
            .get_requests_by_status_and_type(
                RequestStatus::Complete,
                crate::db::RequestType::Aggregation,
                10,
                self.requester_config.l1_chain_id,
                self.requester_config.l2_chain_id,
                &self.proof_requester.program_config.commitments,
            )
            .await?;

        for request in completed_agg_requests {
            if let Some(proof_data) = &request.proof {
                // Submit the proof to L1
                info!("Submitting aggregation proof for request {}", request.id);
                // TODO: Implement actual L1 submission logic
                // For now, just mark as submitted
                self.driver_config.driver_db_client
                    .update_request_status(request.id, RequestStatus::Submitted)
                    .await?;
            }
        }

        Ok(())
    }
}

// Helper functions
fn u32_to_u8(input: u32) -> [u8; 32] {
    let mut output = [0u8; 32];
    output[28..].copy_from_slice(&input.to_be_bytes());
    output
}

fn hash_rollup_config(config: &BootInfoStruct) -> B256 {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bincode::serialize(config).unwrap());
    B256::from_slice(&hasher.finalize())
}
