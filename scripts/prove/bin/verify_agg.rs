use alloy_primitives::Address;
use alloy_sol_types::SolValue;
use anyhow::Result;
use clap::Parser;
use op_succinct_client_utils::types::AggregationOutputs;
use op_succinct_elfs::VERIFICATION_ELF;
use serde::{Deserialize, Serialize};
use sp1_sdk::{utils, HashableKey, ProverClient, SP1ProofWithPublicValues, SP1Stdin, SP1Proof};
use hex;

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
) -> Result<(Vec<SP1Proof>, Vec<AggregationOutputs>, Vec<Vec<u8>>)> {
    let mut proofs = Vec::new();
    let mut public_values_vec = Vec::new();
    let mut raw_public_values_vec = Vec::new();

    for proof_path in proof_paths {
        // Load the aggregation proof
        let mut proof_with_pv = SP1ProofWithPublicValues::load(&proof_path)
            .map_err(|e| anyhow::anyhow!("Failed to load proof from {}: {}", proof_path, e))?;

        // Extract SP1Proof for runtime writing
        proofs.push(proof_with_pv.proof.clone());

        // Extract and decode public values
        const AGG_OUTPUTS_SIZE: usize = 7 * 32;
        let mut raw_agg_outputs = [0u8; AGG_OUTPUTS_SIZE];
        proof_with_pv.public_values.read_slice(&mut raw_agg_outputs);

        let agg_outputs = AggregationOutputs::abi_decode(&raw_agg_outputs)
            .map_err(|e| anyhow::anyhow!("Failed to decode aggregation outputs: {}", e))?;

        println!("Loaded proof: {} (Block: {})", proof_path, agg_outputs.l2BlockNumber);
        println!("Raw public values length: {}", raw_agg_outputs.len());
        println!("Raw public values: {}", hex::encode(&raw_agg_outputs));
        
        // Test: re-encode and see if it matches
        let re_encoded = agg_outputs.abi_encode();
        println!("Re-encoded length: {}", re_encoded.len());
        println!("Re-encoded matches raw: {}", re_encoded == raw_agg_outputs);

        public_values_vec.push(agg_outputs);
        raw_public_values_vec.push(raw_agg_outputs.to_vec());
    }

    Ok((proofs, public_values_vec, raw_public_values_vec))
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
    let (agg_proofs, public_values_vec, raw_public_values_vec) = load_verification_proof_data(args.proofs)?;




    let agg_vkey_hex = if args.agg_vkey.starts_with("0x") {
        &args.agg_vkey[2..]
    } else {
        &args.agg_vkey
    };

    let agg_vkey_bytes = hex::decode(agg_vkey_hex)
        .map_err(|e| anyhow::anyhow!("Failed to decode aggregation vkey hex: {}", e))?;

    if agg_vkey_bytes.len() != 32 {
        anyhow::bail!("Aggregation vkey must be 32 bytes long, but was {} bytes", agg_vkey_bytes.len());
    }

    // Setup SP1 client
    let client = ProverClient::from_env();

    // Setup the aggregation ELF to get the proper verifying key
    let (_, agg_vk) = client.setup(op_succinct_elfs::AGGREGATION_ELF);
    println!("Current Aggregation ELF vKey: {:?}", agg_vk.vk.bytes32());
    println!("Current Aggregation ELF vKey Hash (u32): {:?}", agg_vk.vk.hash_u32());

    // Verify that the provided agg_vkey matches the current ELF
    let mut agg_vkey_array = [0u32; 8];
    for (i, chunk) in agg_vkey_bytes.chunks_exact(4).enumerate() {
        agg_vkey_array[i] = u32::from_be_bytes(chunk.try_into().unwrap());
    }
    
    println!("Provided agg_vkey: {:?}", agg_vkey_array);
    println!("Current ELF vkey hash: {:?}", agg_vk.vk.hash_u32());
    
    // Verify that the provided key matches the current ELF (as a safety check)
    if agg_vkey_array != agg_vk.vk.hash_u32() {
        println!("WARNING: Provided agg_vkey doesn't match current ELF vkey hash. Using ELF vkey hash for verification.");
    }

    let verification_inputs = verification::VerificationInputs {
        public_values_vec: public_values_vec.clone(),
        raw_public_values_vec: raw_public_values_vec.clone(),
        agg_vkey: agg_vk.vk.hash_u32(),
    };

    // Create stdin for verification program
    let mut stdin = SP1Stdin::new();
    
    // Write each aggregation proof to the runtime first
    // Note: These are aggregation proofs, so we use the aggregation vkey
    for proof in &agg_proofs {
        let SP1Proof::Compressed(compressed_proof) = proof else {
            return Err(anyhow::anyhow!("Expected compressed proof, but got different proof type!"));
        };
        stdin.write_proof(*compressed_proof.clone(), agg_vk.vk.clone());
    }
    
    // Write the verification inputs (public values)
    stdin.write(&verification_inputs);

    println!("Start generating proof");
    
    // Setup the verification program
    let (verification_pk, verification_vk) = client.setup(VERIFICATION_ELF);
    println!("Verification ELF vKey: {:?}", verification_vk.vk.bytes32());
    
    // Generate the ZK proof that proves aggregation verification (Groth16)
    let proof = client
        .prove(&verification_pk, &stdin)
        .groth16()
        .run()
        .expect("Failed to generate proof");

    // Save the verification proof
    let verification_proof_names: Vec<String> = public_values_vec
        .iter()
        .map(|data| format!("block_{}", data.l2BlockNumber))
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
    
    println!("Proof Type: Groth16");
    println!("Proof saved to: {}", verification_proof_path);
    println!("Proofs verified: {}", verification_output.proofs_verified);
    println!("Aggregation VKey: 0x{}", hex::encode(verification_output.aggr_vkey));
    println!("Verified proofs outputs:");
    for (i, proof_output) in verification_output.proofs_outputs.iter().enumerate() {
        println!("  Proof {}: Block {} (Root: 0x{})", 
                 i + 1, 
                 proof_output.l2BlockNumber, 
                 hex::encode(proof_output.l2PostRoot));
    }

    Ok(())
}

// Include the verification program types
mod verification {
    use super::*;
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VerificationInputs {
        pub public_values_vec: Vec<AggregationOutputs>,
        pub raw_public_values_vec: Vec<Vec<u8>>,
        pub agg_vkey: [u32; 8],
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VerificationOutputs {
        pub aggr_vkey: alloy_primitives::B256,
        pub proofs_verified: u64,
        pub proofs_outputs: Vec<AggregationOutputs>,
    }
}
