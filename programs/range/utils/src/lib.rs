use std::io::Bytes;
use std::sync::{Arc, Once};

use alloy_primitives::{address, keccak256, Address, Sealable, B256}; // for seal_ref_slow
use kzg_rs::Bytes32;

use kona_executor::{TrieDB, TrieDBProvider};
use kona_proof::{l1::OracleL1ChainProvider, l2::OracleL2ChainProvider, BootInfo};
use kona_protocol::BatchValidationProvider; // enables block_by_number on the provider
use op_succinct_client_utils::{
    boot::{hash_rollup_config, BootInfoStruct},
    witness::{
        executor::{get_inputs_for_pipeline, WitnessExecutor},
        preimage_store::PreimageStore,
        WitnessData,
        MailboxStore,
    },
    BlobStore,
};
use tracing::{debug, error, info, warn};

macro_rules! log_info {
    ($($arg:tt)*) => {{
        info!($($arg)*);
        #[cfg(target_os = "zkvm")]
        println!($($arg)*);
    }};
}

macro_rules! log_debug {
    ($($arg:tt)*) => {{
        debug!($($arg)*);
        #[cfg(target_os = "zkvm")]
        println!($($arg)*);
    }};
}

macro_rules! log_warn {
    ($($arg:tt)*) => {{
        warn!($($arg)*);
        #[cfg(target_os = "zkvm")]
        println!($($arg)*);
    }};
}

macro_rules! log_error {
    ($($arg:tt)*) => {{
        error!($($arg)*);
        #[cfg(target_os = "zkvm")]
        println!($($arg)*);
    }};
}

/// Sets up tracing for the range program
pub fn setup_tracing() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        #[cfg(feature = "tracing-subscriber")]
        {
            use anyhow::anyhow;
            use tracing::Level;

            let subscriber = tracing_subscriber::fmt().with_max_level(Level::INFO).finish();
            tracing::subscriber::set_global_default(subscriber).map_err(|e| anyhow!(e)).unwrap();
        }

        #[cfg(not(feature = "tracing-subscriber"))]
        {
            // no op
        }
    });
}

pub async fn run_range_program<E, W>(executor: E, witness_data: W)
where
    E: WitnessExecutor<
            O = PreimageStore,
            B = BlobStore,
            L1 = OracleL1ChainProvider<PreimageStore>,
            L2 = OracleL2ChainProvider<PreimageStore>,
        > + Send
        + Sync,
    W: WitnessData + Send + Sync,
{
    ////////////////////////////////////////////////////////////////
    //                          PROLOGUE                          //
    ////////////////////////////////////////////////////////////////

    log_info!("Starting blocks verification...");

    let (oracle, beacon, mailbox_store) = witness_data.get_oracle_and_blob_provider().await.unwrap();


    let (boot_info, input) = get_inputs_for_pipeline(oracle.clone()).await.unwrap();
    let mut l2_provider_for_mailbox: Option<OracleL2ChainProvider<PreimageStore>> = None;
    let boot_info = match input {
        // Some((_, _, l2_provider)) => {
        Some((cursor, l1_provider, l2_provider)) => {
        let rollup_config = Arc::new(boot_info.rollup_config.clone());

        let pipeline = executor
            .create_pipeline(
                rollup_config,
                cursor.clone(),
                oracle,
                beacon,
                l1_provider,
                l2_provider.clone(),
            )
            .await
            .unwrap();
        // Save for mailbox computation (stubbed for now)
        l2_provider_for_mailbox = Some(l2_provider.clone());
        executor.run(boot_info, pipeline, cursor, l2_provider).await.unwrap()
        // boot_info
        }
        None => boot_info,
    };

    log_info!("Finished blocks verification. Now computing mailbox root...");

    // Compute mailbox root from L2 provider
    // let mailbox_root = compute_mailbox_root(&boot_info, l2_provider_for_mailbox.as_mut()).await;
    // let mailbox_root = compute_mailbox_root_hash(mailbox_store);

    // Commit BootInfoStruct including the mailbox root.
    let boot_info_struct = BootInfoStruct {
        l1Head: boot_info.l1_head,
        l2PreRoot: boot_info.agreed_l2_output_root,
        l2PostRoot: boot_info.claimed_l2_output_root,
        l2BlockNumber: boot_info.claimed_l2_block_number,
        rollupConfigHash: hash_rollup_config(&boot_info.rollup_config),
    };

    sp1_zkvm::io::commit(&boot_info_struct);
}

/// Computes the mailbox root for the final L2 state referenced by `boot_info`.
/// Inputs:
/// - `boot_info`: Includes the claimed L2 block number
/// - `l2_provider`: A provider that can retrieve L2 headers
// async fn compute_mailbox_root(
//     _boot_info: &BootInfo,
//     l2_provider: Option<&mut OracleL2ChainProvider<PreimageStore>>,
// ) -> B256 {
//     log_info!("Inside compute mailbox root...");
//
//     // Assert we have a provider
//     let Some(provider) = l2_provider else {
//         log_warn!("No L2 provider available; skipping mailbox state reads");
//         return B256::ZERO;
//     };
//
//     // Hardcoded Mailbox address
//     // TODO: let it be an input or enforce common address across chains
//     let mailbox_addr: Address = address!("0xF67D90d846731f65313EA43c89d377Cd22602e0d");
//     log_debug!("Computed mailbox address");
//
//     // Attempt getting a block
//     // Try min(claimed_number, safe_number) to avoid going past safe head
//     // though ultimately we need to ensure that we can read the claimed_number block
//     // Probably we'll need to advance the l2_provider to it first
//     let claimed_number = _boot_info.claimed_l2_block_number;
//     let safe_head = provider.l2_safe_head().await.unwrap();
//     let safe_header = provider.header_by_hash(safe_head).unwrap();
//     let safe_number = safe_header.number;
//     log_info!("Safe head block number: {safe_number}. Claimed number: {claimed_number}");
//     let block = provider.block_by_number(claimed_number.min(safe_number)).await;
//     let block = match block {
//         Ok(b) => {
//             log_info!("Mailbox block loaded: {}", claimed_number.min(safe_number));
//             b
//         }
//         Err(e) => {
//             log_error!(
//                 "Failed to load L2 block at number {}; skipping mailbox state reads. Error: {:?}",
//                 claimed_number.min(safe_number),
//                 e
//             );
//             return B256::ZERO;
//         }
//     };
//
//     // Seal block
//     let sealed_header = block.header.seal_slow();
//     log_debug!("Sealed header for mailbox computation");
//
//     // Construct trie DB
//     // let fetcher = provider.clone();
//     // let hinter = provider.clone();
//     // let mut db = TrieDB::new(sealed_header, fetcher, hinter);
//     // log_debug!("Created trie DB for mailbox reads");
//
//     // Get mailbox trie account
//     // let trie_account = db.get_trie_account(&mailbox_addr, claimed_number.min(safe_number));
//     // match trie_account {
//     //     Ok(_account) => match _account {
//     //         Some(_accountv) => {
//     //             log_debug!("Mailbox account exists in state");
//     //         }
//     //         None => {
//     //             log_warn!("Mailbox account not present in state");
//     //         }
//     //     },
//     //     Err(_e) => {
//     //         log_error!("Mailbox trie account error");
//     //     }
//     // }
//
//     // Get list of (chainID, inbox root, outbox root)
//     // let _mailbox_roots = get_mailbox_root();
//
//     // compute_mailbox_root_hash(&_mailbox_roots)
// }

/// Returns a list of (chainID, inbox root, outbox root).
/// Merges inbox and outbox roots by chainID, fills missing roots with B256::ZERO, and sorts by
/// chainID.
// pub fn get_mailbox_root() -> Vec<(u64, B256, B256)> {
//     let inbox_roots = get_inbox_roots();
//     let outbox_roots = get_outbox_roots();
//
//     // Collect all unique chain IDs from both lists
//     let mut chain_ids: Vec<u64> = inbox_roots.iter().map(|(id, _)| *id).collect();
//     chain_ids.extend(outbox_roots.iter().map(|(id, _)| *id));
//     chain_ids.sort_unstable();
//     chain_ids.dedup();
//
//     // For each chain ID, find inbox and outbox roots, or use B256::ZERO
//     let mut result = Vec::with_capacity(chain_ids.len());
//     for chain_id in chain_ids {
//         let inbox = inbox_roots
//             .iter()
//             .find(|(id, _)| *id == chain_id)
//             .map(|(_, root)| *root)
//             .unwrap_or(B256::ZERO);
//         let outbox = outbox_roots
//             .iter()
//             .find(|(id, _)| *id == chain_id)
//             .map(|(_, root)| *root)
//             .unwrap_or(B256::ZERO);
//         result.push((chain_id, inbox, outbox));
//     }
//     result
// }

// Returns a list of (chainID, inbox root).
// TODO
// pub fn get_inbox_roots() -> Vec<(u64, B256)> {
//     Vec::new()
// }

// Returns a list of (chainID, outbox root).
// TODO
// pub fn get_outbox_roots() -> Vec<(u64, B256)> {
//     Vec::new()
// }

// pub fn compute_mailbox_root_hash(mailbox_store: MailboxStore) -> B256 {
//     let mut bytes = Vec::new();
//
//     let mut chain_ids: Vec<u64> = mailbox_store.decode_inbox_chains();
//     chain_ids.extend(mailbox_store.decode_outbox_chains());
//     chain_ids.sort_unstable();
//     chain_ids.dedup();
//
//     // Prefix
//     bytes.extend_from_slice(b"MAILBOX");
//
//     // Number of chainIDs (N) as u64, big-endian
//     bytes.extend_from_slice(&(chain_ids.len() as u64).to_be_bytes());
//
//     let inbox_roots: Vec<Bytes32> = Vec::new();
//
//     for (chain_id) in chain_ids {
//         let selected_index = -1;
//
//         for (idx, inbox_chain_id) in mailbox_store.decode_inbox_chains().iter().enumerate() {
//             if chain_id == *inbox_chain_id {
//                 // selected_index = idx;
//                 break
//             }
//         }
//
//         if selected_index != -1 {
//             let inbox_root = mailbox_store.decode_inbox_roots()[selected_index];
//             // inbox_roots.push(inbox_root)
//         }
//     }
//
//     // for (chain_id, inbox_root, outbox_root) in  {
//         // bytes.extend_from_slice(&chain_id.to_be_bytes());
//         // bytes.extend_from_slice(inbox_root.as_slice());
//         // bytes.extend_from_slice(outbox_root.as_slice());
//     // }
//
//     B256::from(keccak256(&bytes))
// }
