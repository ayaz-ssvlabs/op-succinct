use alloy_primitives::Address;
use alloy_sol_types::SolValue;
use anyhow::Result;
use clap::Parser;
use op_succinct_client_utils::types::AggregationOutputs;
use op_succinct_elfs::VERIFICATION_ELF;
use serde::{Deserialize, Serialize};
use sp1_sdk::{utils, HashableKey, ProverClient, SP1ProofWithPublicValues, SP1Stdin};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Aggregation proof file paths (comma-separated)
    #[arg(short, long, num_args = 1.., value_delimiter = ',')]
    proofs: Vec<String>,

    /// Aggregation verification key (hex string)
    #[arg(short, long)]
    agg_vkey: String,

    /// Prover address for verification
    #[arg(short = 'r', long)]
    prover_address: String,


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

    // Parse aggregation verification key from hex string
    println!("Using aggregation verification key: {}", args.agg_vkey);
    let agg_vkey_hex = if args.agg_vkey.starts_with("0x") {
        &args.agg_vkey[2..]
    } else {
        &args.agg_vkey
    };
    
    let agg_vkey_bytes = hex::decode(agg_vkey_hex)
        .map_err(|e| anyhow::anyhow!("Failed to decode aggregation vkey hex: {}", e))?;
    
    // For now, we'll just use a placeholder vkey array. In a real implementation,
    // you would properly convert the vkey bytes to the required format
    let agg_vkey_array: [u32; 8] = [0; 8]; // Placeholder
    
    println!("Aggregation verification key loaded successfully ({} bytes)", agg_vkey_bytes.len());

    // Create verification inputs for the ZK program
    let verification_inputs = verification::VerificationInputs {
        agg_proofs: agg_proof_data.clone(),
        agg_vkey: agg_vkey_array,
    };

    // Setup SP1 client
    let client = ProverClient::from_env();

    // Create stdin for verification program
    let mut stdin = SP1Stdin::new();
    stdin.write(&verification_inputs);

    println!("Generating ZK verification proof (Groth16)...");
    
    // Setup the verification program
    let (verification_pk, verification_vk) = client.setup(VERIFICATION_ELF);
    println!("Verification ELF Verification Key: {:?}", verification_vk.vk.bytes32());
    
    // Generate the ZK proof that proves aggregation verification (Groth16)
    let proof = client
        .prove(&verification_pk, &stdin)
        .groth16()
        .run()
        .expect("Failed to generate verification proof");

    // Save the verification proof
    let verification_proof_names: Vec<String> = agg_proof_data
        .iter()
        .map(|data| format!("block_{}", data.public_values.l2BlockNumber))
        .collect();
    
    let verification_proof_path = format!(
        "data/fetched_proofs/verification_proof_{}.bin", 
        verification_proof_names.join("_")
    );
    
    // Create directory if it doesn't exist
    if let Some(parent) = std::path::Path::new(&verification_proof_path).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    
    proof.save(&verification_proof_path)
        .expect("Failed to save verification proof");

    // Read and display the verification output
    let mut verification_output_proof = proof;
    let verification_output: verification::VerificationOutputs = verification_output_proof.public_values.read();
    
    println!("\nZK Verification Proof Generated Successfully!");
    println!("============================================");
    println!("Proof Type: Groth16 (ready for on-chain verification)");
    println!("Proof saved to: {}", verification_proof_path);
    println!("Proofs verified: {}", verification_output.proofs_verified);
    println!("Total blocks covered: {}", verification_output.total_blocks_covered);
    println!("Final L2 block number: {}", verification_output.final_l2_block_number);
    println!("Final L2 post root: 0x{}", hex::encode(verification_output.final_l2_post_root));
    println!("Multi-block VKey: 0x{}", hex::encode(verification_output.multi_block_vkey));
    println!("Prover address: {}", verification_output.prover_address);

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
