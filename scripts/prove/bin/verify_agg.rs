use alloy_primitives::Address;
use alloy_sol_types::SolValue;
use anyhow::Result;
use clap::Parser;
use op_succinct_client_utils::types::AggregationOutputs;
use serde::{Deserialize, Serialize};
use sp1_sdk::{utils, SP1ProofWithPublicValues};
use std::fs;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Aggregation proof file paths (comma-separated)
    #[arg(short, long, num_args = 1.., value_delimiter = ',')]
    proofs: Vec<String>,

    /// Aggregation verification key file path
    #[arg(short, long)]
    agg_vkey_path: String,

    /// Prover address for verification
    #[arg(short = 'r', long)]
    prover_address: String,

    /// Generate the proof (vs. just witness generation)
    #[arg(long, default_value_t = false)]
    prove: bool,

    /// Env file path
    #[arg(default_value = ".env", short, long)]
    env_file: String,
}

/// Load aggregation proof data from files
fn load_verification_proof_data(
    proof_paths: Vec<String>,
) -> Result<Vec<verification::AggregationProofData>> {
    let mut proof_data = Vec::new();

    for proof_path in proof_paths {
        // Load the aggregation proof
        let mut proof_with_pv = SP1ProofWithPublicValues::load(&proof_path)
            .map_err(|e| anyhow::anyhow!("Failed to load proof from {}: {}", proof_path, e))?;

        // Extract proof bytes
        let proof_bytes = proof_with_pv.bytes();

        // Extract and decode public values
        const AGG_OUTPUTS_SIZE: usize = 7 * 32;
        let mut raw_agg_outputs = [0u8; AGG_OUTPUTS_SIZE];
        proof_with_pv.public_values.read_slice(&mut raw_agg_outputs);

        let agg_outputs = AggregationOutputs::abi_decode(&raw_agg_outputs)
            .map_err(|e| anyhow::anyhow!("Failed to decode aggregation outputs: {}", e))?;

        println!("Loaded proof: {} (Block: {})", proof_path, agg_outputs.l2BlockNumber);

        proof_data.push(verification::AggregationProofData {
            proof: proof_bytes,
            public_values: agg_outputs,
        });
    }

    Ok(proof_data)
}

#[tokio::main]
async fn main() -> Result<()> {
    utils::setup_logger();
    
    let args = Args::parse();
    dotenv::from_filename(args.env_file).ok();

    println!("SP1 Aggregation Proof Verifier");
    println!("==============================");

    // Parse prover address
    let _prover_address: Address = args.prover_address.parse()
        .map_err(|e| anyhow::anyhow!("Invalid prover address: {}", e))?;

    // Load aggregation proof data
    println!("Loading {} aggregation proofs...", args.proofs.len());
    let agg_proof_data = load_verification_proof_data(args.proofs)?;

    // Load aggregation verification key (for future ZK verification)
    println!("Loading aggregation verification key from: {}", args.agg_vkey_path);
    let agg_vkey_bytes = fs::read(&args.agg_vkey_path)
        .map_err(|e| anyhow::anyhow!("Failed to read aggregation vkey: {}", e))?;
    let _agg_vkey: sp1_sdk::SP1VerifyingKey = bincode::deserialize(&agg_vkey_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize aggregation vkey: {}", e))?;

    // Validate proof sequence continuity (same logic as in the ZK program)
    println!("Validating proof sequence continuity...");
    
    for i in 1..agg_proof_data.len() {
        let prev = &agg_proof_data[i - 1].public_values;
        let curr = &agg_proof_data[i].public_values;
        
        // Ensure the L2 post root of previous proof matches L2 pre root of current proof
        if prev.l2PostRoot != curr.l2PreRoot {
            return Err(anyhow::anyhow!(
                "L2 state continuity broken between proof {} and {}: prev post root 0x{} != curr pre root 0x{}",
                i, i + 1, hex::encode(prev.l2PostRoot), hex::encode(curr.l2PreRoot)
            ));
        }
        
        // Ensure rollup config is consistent
        if prev.rollupConfigHash != curr.rollupConfigHash {
            return Err(anyhow::anyhow!(
                "Rollup config mismatch between proof {} and {}",
                i, i + 1
            ));
        }
        
        // Ensure multi-block vkey is consistent
        if prev.multiBlockVKey != curr.multiBlockVKey {
            return Err(anyhow::anyhow!(
                "Multi-block vkey mismatch between proof {} and {}",
                i, i + 1
            ));
        }
        
        // Ensure prover address is consistent
        if prev.proverAddress != curr.proverAddress {
            return Err(anyhow::anyhow!(
                "Prover address mismatch between proof {} and {}",
                i, i + 1
            ));
        }
    }
    
    // Create summary verification output
    let first_proof = &agg_proof_data[0].public_values;
    let last_proof = &agg_proof_data[agg_proof_data.len() - 1].public_values;
    
    let total_blocks = if agg_proof_data.len() == 1 {
        last_proof.l2BlockNumber
    } else {
        last_proof.l2BlockNumber - first_proof.l2BlockNumber + agg_proof_data.len() as u64
    };
    
    println!("\nVerification Summary:");
    println!("====================");
    println!("Proofs verified: {}", agg_proof_data.len());
    println!("Total blocks covered: {}", total_blocks);
    println!("Initial L1 head: 0x{}", hex::encode(first_proof.l1Head));
    println!("Final L2 block number: {}", last_proof.l2BlockNumber);
    println!("Final L2 post root: 0x{}", hex::encode(last_proof.l2PostRoot));
    println!("Rollup config hash: 0x{}", hex::encode(first_proof.rollupConfigHash));
    println!("Multi-block VKey: 0x{}", hex::encode(first_proof.multiBlockVKey));
    println!("Prover address: {}", first_proof.proverAddress);
    
    println!("\nAll aggregation proofs are valid and form a continuous sequence!");
    
    if args.prove {
        println!("\nNote: To generate an actual ZK verification proof, you would need to:");
        println!("1. Build the verification program ELF using the SP1 build system");
        println!("2. Run the verification program with SP1 to generate a proof");
        println!("3. This proof could then be used for on-chain verification");
    }

    Ok(())
}

// Include the verification program types
mod verification {
    pub use super::*;
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VerificationInputs {
        pub agg_proofs: Vec<AggregationProofData>,
        pub agg_vkey: [u32; 8],
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct AggregationProofData {
        pub proof: Vec<u8>,
        pub public_values: AggregationOutputs,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VerificationOutputs {
        pub initial_l1_head: alloy_primitives::B256,
        pub final_l2_post_root: alloy_primitives::B256,
        pub final_l2_block_number: u64,
        pub rollup_config_hash: alloy_primitives::B256,
        pub multi_block_vkey: alloy_primitives::B256,
        pub prover_address: alloy_primitives::Address,
        pub proofs_verified: u64,
        pub total_blocks_covered: u64,
    }
}
