//! A program that verifies multiple aggregation proofs and produces a final verification output.

#![cfg_attr(target_os = "zkvm", no_main)]
#[cfg(target_os = "zkvm")]
sp1_zkvm::entrypoint!(main);

use alloy_primitives::B256;
use alloy_sol_types::SolValue;
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
    /// The first L1 head from the sequence
    pub initial_l1_head: B256,
    /// The final L2 state root after all aggregations
    pub final_l2_post_root: B256,
    /// The final L2 block number
    pub final_l2_block_number: u64,
    /// The rollup config hash (should be consistent across all proofs)
    pub rollup_config_hash: B256,
    /// The multi-block verification key
    pub multi_block_vkey: B256,
    /// The prover address
    pub prover_address: alloy_primitives::Address,
    /// Number of proofs verified
    pub proofs_verified: u64,
    /// Total block range covered
    pub total_blocks_covered: u64,
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
        // Verify the SP1 proof against the aggregation vkey
        let public_values_bytes = agg_proof_data.public_values.abi_encode();
        let pv_digest = Sha256::digest(public_values_bytes);
        
        // Verify the proof
        sp1_lib::verify::verify_sp1_proof(&verification_inputs.agg_vkey, &pv_digest.into());
        
        println!("Verified aggregation proof {}", i + 1);
        verified_outputs.push(&agg_proof_data.public_values);
    }
    
    println!("cycle-tracker-end: proof-verification");
    
    // Validate proof sequence continuity
    println!("cycle-tracker-start: sequence-validation");
    
    for i in 1..verified_outputs.len() {
        let prev = &verified_outputs[i - 1];
        let curr = &verified_outputs[i];
        
        // Ensure the L2 post root of previous proof matches L2 pre root of current proof
        assert_eq!(
            prev.l2PostRoot, 
            curr.l2PreRoot,
            "L2 state continuity broken between proof {} and {}", i, i + 1
        );
        
        // Ensure rollup config is consistent
        assert_eq!(
            prev.rollupConfigHash,
            curr.rollupConfigHash,
            "Rollup config mismatch between proof {} and {}", i, i + 1
        );
        
        // Ensure multi-block vkey is consistent
        assert_eq!(
            prev.multiBlockVKey,
            curr.multiBlockVKey,
            "Multi-block vkey mismatch between proof {} and {}", i, i + 1
        );
        
        // Ensure prover address is consistent
        assert_eq!(
            prev.proverAddress,
            curr.proverAddress,
            "Prover address mismatch between proof {} and {}", i, i + 1
        );
    }
    
    println!("cycle-tracker-end: sequence-validation");
    
    // Create final verification output
    println!("cycle-tracker-start: output-creation");
    
    let first_proof = &verified_outputs[0];
    let last_proof = &verified_outputs[verified_outputs.len() - 1];
    
    // Calculate total blocks covered
    let total_blocks = if verified_outputs.len() == 1 {
        // Single proof case - assume block range from block number
        last_proof.l2BlockNumber
    } else {
        // Multiple proofs - calculate range
        last_proof.l2BlockNumber - first_proof.l2BlockNumber + verified_outputs.len() as u64
    };
    
    let verification_output = VerificationOutputs {
        initial_l1_head: first_proof.l1Head,
        final_l2_post_root: last_proof.l2PostRoot,
        final_l2_block_number: last_proof.l2BlockNumber,
        rollup_config_hash: first_proof.rollupConfigHash,
        multi_block_vkey: first_proof.multiBlockVKey,
        prover_address: first_proof.proverAddress,
        proofs_verified: verified_outputs.len() as u64,
        total_blocks_covered: total_blocks,
    };
    
    println!("cycle-tracker-end: output-creation");
    
    // Commit the verification output
    sp1_zkvm::io::commit(&verification_output);
    
    println!("Successfully verified {} aggregation proofs covering {} total blocks", 
             verification_output.proofs_verified, 
             verification_output.total_blocks_covered);
}
