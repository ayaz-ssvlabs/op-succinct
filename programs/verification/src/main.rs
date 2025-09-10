//! A program that verifies multiple aggregation proofs and produces a final verification output.

#![cfg_attr(target_os = "zkvm", no_main)]
#[cfg(target_os = "zkvm")]
sp1_zkvm::entrypoint!(main);

use alloy_primitives::{B256, hex};
use op_succinct_client_utils::types::AggregationOutputs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Input structure for the verification program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationInputs {
    /// The aggregation proofs to verify
    pub agg_proofs: Vec<AggregationProofData>,
    /// The aggregation verification key
    pub agg_vkey: [u32; 8],
}

/// Aggregation proof data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationProofData {
    /// The SP1 proof (compressed)
    pub proof: Vec<u8>,
    /// The public values (AggregationOutputs)
    pub public_values: AggregationOutputs,
}

/// Output structure for the verification program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationOutputs {
    /// The aggregation verification key
    pub aggr_vkey: B256,
    /// Number of proofs verified
    pub proofs_verified: u64,
    /// Internal structure holding all proofs outputs
    pub proofs_outputs: Vec<AggregationOutputs>,
}

pub fn main() {
    // Read the verification inputs
    let verification_inputs = sp1_zkvm::io::read::<VerificationInputs>();
    
    println!("cycle-tracker-start: verification-setup");
    
    // Validate that we have at least one proof
    assert!(!verification_inputs.agg_proofs.is_empty(), "No aggregation proofs provided");
    
    println!("cycle-tracker-end: verification-setup");
    
    // Verify each aggregation proof
    println!("cycle-tracker-start: proof-verification");
    
    let mut verified_outputs = Vec::new();
    
    for (i, agg_proof_data) in verification_inputs.agg_proofs.iter().enumerate() {
        println!("Verifying aggregation proof {}", i + 1);
        
        // Verify the SP1 proof using the aggregation verification key
        let serialized_public_values = bincode::serialize(&agg_proof_data.public_values).unwrap();
        let pv_digest = Sha256::digest(serialized_public_values);
        
        // This is the critical proof verification step that was missing
        sp1_lib::verify::verify_sp1_proof(&verification_inputs.agg_vkey, &pv_digest.into());
        
        println!("Successfully verified aggregation proof {}", i + 1);
        verified_outputs.push(&agg_proof_data.public_values);
    }
    
    println!("cycle-tracker-end: proof-verification");
    
    // Since proofs are from different L2 rollups, we don't validate continuity
    // Each proof is independent and from its own rollup
    println!("cycle-tracker-start: independent-validation");
    
    println!("Verified {} independent rollup proofs", verified_outputs.len());
    
    println!("cycle-tracker-end: independent-validation");
    
    // Create final verification output
    println!("cycle-tracker-start: output-creation");
    
    // Convert agg_vkey to B256 for output
    let agg_vkey_bytes = verification_inputs.agg_vkey.iter()
        .flat_map(|&x| x.to_be_bytes())
        .collect::<Vec<u8>>();
    let aggr_vkey = B256::from_slice(&Sha256::digest(&agg_vkey_bytes));
    
    // Collect all verified proof outputs
    let proofs_outputs = verified_outputs.iter().map(|&output| output.clone()).collect();
    
    let verification_output = VerificationOutputs {
        aggr_vkey,
        proofs_verified: verified_outputs.len() as u64,
        proofs_outputs,
    };
    
    println!("cycle-tracker-end: output-creation");
    
    // Commit the verification output
    sp1_zkvm::io::commit(&verification_output);
    
    println!("Successfully verified {} independent rollup aggregation proofs", 
             verification_output.proofs_verified);
    println!("Verified rollups: {:?}", verification_output.proofs_outputs.iter().map(|output| output.rollupConfigHash).collect::<Vec<_>>());
    println!("Aggregation vkey hash: 0x{}", hex::encode(verification_output.aggr_vkey));
}
