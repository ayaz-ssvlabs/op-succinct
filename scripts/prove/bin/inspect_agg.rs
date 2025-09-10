use alloy_sol_types::SolValue;
use anyhow::Result;
use clap::Parser;
use op_succinct_client_utils::boot::{BootInfoStruct, AGGREGATION_OUTPUTS_SIZE};
use sp1_sdk::{utils, SP1ProofWithPublicValues};
use std::fs;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Aggregation proof file paths (comma-separated)
    #[arg(short, long, num_args = 1.., value_delimiter = ',')]
    proofs: Vec<String>,

    /// Env file path.
    #[arg(default_value = ".env", short, long)]
    env_file: String,
}

/// Load and inspect an aggregation proof
fn inspect_aggregation_proof(proof_path: &str) -> Result<()> {
    println!("\n=== Inspecting: {} ===", proof_path);
    
    if !fs::metadata(proof_path).is_ok() {
        println!("ERROR: Proof file not found: {}", proof_path);
        return Ok(());
    }

    // Load the proof
    let mut proof_with_pv = SP1ProofWithPublicValues::load(proof_path)
        .map_err(|e| anyhow::anyhow!("Failed to load proof: {}", e))?;

    // 1. Print proof in hex format
    println!("Proof (hex):");
    let proof_bytes = proof_with_pv.bytes();
    println!("   Length: {} bytes", proof_bytes.len());
    println!("   Hex: 0x{}", hex::encode(&proof_bytes));

    // 2. Print aggregation output (public values)
    println!("\nPublic Values (Aggregation Output):");
    
    // Aggregation proofs store BootInfoStruct as public values (based on fetch_and_save_proof.rs)
    const BOOT_INFO_SIZE: usize = 5 * 32; // 5 fields, each padded to 32 bytes in ABI = 160 bytes
    let mut raw_boot_info = [0u8; BOOT_INFO_SIZE];
    proof_with_pv.public_values.read_slice(&mut raw_boot_info);
    
    let boot_info = BootInfoStruct::abi_decode(&raw_boot_info)
        .map_err(|e| anyhow::anyhow!("Failed to decode boot info: {}", e))?;
    
    println!("   L1 Head: 0x{}", hex::encode(boot_info.l1Head));
    println!("   L2 Pre Root: 0x{}", hex::encode(boot_info.l2PreRoot));
    println!("   L2 Post Root: 0x{}", hex::encode(boot_info.l2PostRoot));
    println!("   L2 Block Number: {}", boot_info.l2BlockNumber);
    println!("   Rollup Config Hash: 0x{}", hex::encode(boot_info.rollupConfigHash));
    
    println!("\n   Formatted Values:");
    println!("   L1 Head (B256): {}", boot_info.l1Head);
    println!("   L2 Pre Root (B256): {}", boot_info.l2PreRoot);
    println!("   L2 Post Root (B256): {}", boot_info.l2PostRoot);
    println!("   Rollup Config Hash (B256): {}", boot_info.rollupConfigHash);

    // 3. Print Groth16 proof details
    println!("\nGroth16 Proof Details:");
    match &proof_with_pv.proof {
        sp1_sdk::SP1Proof::Groth16(_) => {
            println!("   Type: Groth16 (ready for on-chain verification)");
            let proof_bytes = proof_with_pv.bytes();
            println!("   Proof bytes length: {}", proof_bytes.len());
            println!("   Status: Ready for L1 submission");
        }
        _ => {
            println!("   ERROR: Expected Groth16 proof, but found different type");
            println!("   Hint: Use --prove flag with .groth16() mode in aggregation");
        }
    }

    println!("Successfully inspected proof\n");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    utils::setup_logger();

    let args = Args::parse();

    dotenv::from_filename(args.env_file).ok();

    println!("SP1 Aggregation Proof Inspector");
    println!("=====================================");

    if args.proofs.is_empty() {
        println!("ERROR: No proof files specified. Use --proofs to specify proof files.");
        return Ok(());
    }

    // Inspect each proof file
    for proof_path in &args.proofs {
        // Handle both absolute and relative paths
        let full_path = if proof_path.starts_with("data/") {
            proof_path.clone()
        } else {
            format!("data/fetched_proofs/{}", proof_path)
        };
        
        if let Err(e) = inspect_aggregation_proof(&full_path) {
            println!("ERROR inspecting {}: {}", full_path, e);
        }
    }

    println!("Inspection complete!");
    Ok(())
}
