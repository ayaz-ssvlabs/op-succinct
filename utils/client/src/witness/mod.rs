pub mod executor;
pub mod preimage_store;

use std::{fmt::Debug, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use kzg_rs::{Blob, Bytes48, Bytes32};
use preimage_store::PreimageStore;
use serde::{Deserialize, Serialize};

use crate::BlobStore;

#[async_trait]
pub trait WitnessData: Sized {
    /// Creates a new WitnessData from the given preimage store, blob data, and storage data.
    fn from_parts(
        preimage_store: PreimageStore, 
        blob_data: BlobData,
        inbox_chains: Vec<Bytes32>,
        outbox_chains: Vec<Bytes32>,
        inbox_roots: Vec<Bytes32>,
        outbox_roots: Vec<Bytes32>,
    ) -> Self;

    /// Consumes the WitnessData to extract its core components.
    fn into_parts(self) -> (PreimageStore, BlobData, Vec<Bytes32>, Vec<Bytes32>, Vec<Bytes32>, Vec<Bytes32>);

    /// Gets the oracle and blob provider from the witness data and validates the correctness of the
    /// preimages.
    async fn get_oracle_and_blob_provider(self) -> Result<(Arc<PreimageStore>, BlobStore)> {
        let (owned_preimage_store, owned_blob_data, _inbox_chains, _outbox_chains, _inbox_roots, _outbox_roots) = self.into_parts();

        println!("cycle-tracker-report-start: oracle-verify");
        // Check the preimages in the witness are valid.
        owned_preimage_store.check_preimages().expect("Failed to validate preimages");
        println!("cycle-tracker-report-end: oracle-verify");

        // Create an Arc of the preimage store.
        let oracle = Arc::new(owned_preimage_store);

        // Create a BlobStore from the blobs in the witness and verifies them for correctness.
        println!("cycle-tracker-report-start: blob-verification");
        let beacon = BlobStore::from(owned_blob_data);
        println!("cycle-tracker-report-end: blob-verification");

        Ok((oracle, beacon))
    }

    async fn get_mailbox_inputs(self) -> Result<(Vec<Bytes32>, Vec<Bytes32>, Vec<Bytes32>, Vec<Bytes32>)> {
        let (_, _, inbox_chains, outbox_chains, inbox_roots, outbox_roots) = self.into_parts();

        Ok((inbox_chains, outbox_chains, inbox_roots, outbox_roots))
    }
}

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct DefaultWitnessData {
    pub preimage_store: PreimageStore,
    pub blob_data: BlobData,
    pub inbox_chains: Vec<Bytes32>,
    pub outbox_chains: Vec<Bytes32>,
    pub inbox_roots: Vec<Bytes32>,
    pub outbox_roots: Vec<Bytes32>,
}

#[async_trait]
impl WitnessData for DefaultWitnessData {
    fn from_parts(
        preimage_store: PreimageStore, 
        blob_data: BlobData,
        inbox_chains: Vec<Bytes32>,
        outbox_chains: Vec<Bytes32>,
        inbox_roots: Vec<Bytes32>,
        outbox_roots: Vec<Bytes32>,
    ) -> Self {
        Self { 
            preimage_store, 
            blob_data,
            inbox_chains,
            outbox_chains,
            inbox_roots,
            outbox_roots,
        }
    }

    fn into_parts(self) -> (PreimageStore, BlobData, Vec<Bytes32>, Vec<Bytes32>, Vec<Bytes32>, Vec<Bytes32>) {
        (
            self.preimage_store, 
            self.blob_data,
            self.inbox_chains,
            self.outbox_chains,
            self.inbox_roots,
            self.outbox_roots,
        )
    }
}

#[derive(
    Clone, Debug, Default, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct BlobData {
    pub blobs: Vec<Blob>,
    pub commitments: Vec<Bytes48>,
    pub proofs: Vec<Bytes48>,
}
