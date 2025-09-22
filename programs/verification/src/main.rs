//! A program that verifies multiple aggregation proofs and produces a final verification output.

#![cfg_attr(target_os = "zkvm", no_main)]
#[cfg(target_os = "zkvm")]
sp1_zkvm::entrypoint!(main);

use alloy_primitives::{B256, hex};
use alloy_sol_types::SolValue;
use op_succinct_client_utils::types::AggregationOutputs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Input structure for the verification program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationInputs {
    /// The public values from aggregation proofs to verify
    pub public_values_vec: Vec<AggregationOutputs>,
    /// The raw committed public values bytes (as committed by aggregation program)
    pub raw_public_values_vec: Vec<Vec<u8>>,
    /// The aggregation verification key
    pub agg_vkey: [u32; 8],
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
    let verification_inputs = sp1_zkvm::io::read::<VerificationInputs>();
    
    println!("cycle-tracker-start: verification-setup");
    
    assert!(!verification_inputs.public_values_vec.is_empty(), "No aggregation proofs provided");
    
    println!("cycle-tracker-end: verification-setup");
    
    println!("cycle-tracker-start: proof-verification");
    
    let mut verified_outputs = Vec::new();
    
    for (i, (public_values, raw_public_values)) in verification_inputs.public_values_vec.iter()
        .zip(verification_inputs.raw_public_values_vec.iter()).enumerate() {
        println!("Verifying aggregation proof {}", i + 1);
        
        let pv_digest = Sha256::digest(raw_public_values);
        
        println!("Raw committed data length: {}", raw_public_values.len());
        println!("Computed digest: {:?}", hex::encode(pv_digest));

        sp1_lib::verify::verify_sp1_proof(&verification_inputs.agg_vkey, &pv_digest.into());

        println!("Successfully verified aggregation proof {}", i + 1);
        verified_outputs.push(public_values);
    }
    
    println!("cycle-tracker-end: proof-verification");
    
    println!("cycle-tracker-start: independent-validation");
    
    println!("Verified {} independent rollup proofs", verified_outputs.len());
    
    println!("cycle-tracker-end: independent-validation");
    
    println!("cycle-tracker-start: output-creation");
    
    let agg_vkey_bytes = verification_inputs.agg_vkey.iter()
        .flat_map(|&x| x.to_be_bytes())
        .collect::<Vec<u8>>();
    let aggr_vkey = B256::from_slice(&Sha256::digest(&agg_vkey_bytes));
    
    let proofs_outputs = verified_outputs.iter().map(|&output| output.clone()).collect();
    
    let verification_output = VerificationOutputs {
        aggr_vkey,
        proofs_verified: verified_outputs.len() as u64,
        proofs_outputs,
    };
    
    println!("cycle-tracker-end: output-creation");
    
    sp1_zkvm::io::commit(&verification_output);
    
    println!("Successfully verified {} independent rollup aggregation proofs",
             verification_output.proofs_verified);
    println!("Verified blocks: {:?}", verification_output.proofs_outputs.iter().map(|output| output.l2BlockNumber).collect::<Vec<_>>());
    println!("Aggregation vkey hash: 0x{}", hex::encode(verification_output.aggr_vkey));
}
