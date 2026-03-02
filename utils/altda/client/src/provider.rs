use std::{fmt::Display, sync::Arc};

use alloy_primitives::{keccak256, Bytes};
use async_trait::async_trait;
use kona_derive::{PipelineError, PipelineErrorKind};
use kona_preimage::{errors::PreimageOracleError, CommsClient, PreimageKey, PreimageKeyType};
use kona_proof::Hint;
use thiserror::Error;

use crate::hint::AltDAHintType;

pub const ALTDA_DERIVATION_VERSION: u8 = 0x01;

const ALTDA_COMMITMENT_TYPE_KECCAK256: u8 = 0x00;
const ALTDA_COMMITMENT_TYPE_GENERIC: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AltDACommitmentType {
    Keccak256,
    Generic,
}

impl AltDACommitmentType {
    fn from_prefix(prefix: u8) -> Result<Self, AltDAProviderError> {
        match prefix {
            ALTDA_COMMITMENT_TYPE_KECCAK256 => Ok(Self::Keccak256),
            ALTDA_COMMITMENT_TYPE_GENERIC => Ok(Self::Generic),
            _ => Err(AltDAProviderError::UnknownCommitmentType(prefix)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedAltDACommitment {
    pub commitment_type: AltDACommitmentType,
    pub payload: Vec<u8>,
}

pub fn parse_altda_commitment(
    commitment: &[u8],
) -> Result<ParsedAltDACommitment, AltDAProviderError> {
    if commitment.len() < 2 {
        return Err(AltDAProviderError::InvalidCommitment(
            "commitment must include type prefix and payload",
        ));
    }

    let commitment_type = AltDACommitmentType::from_prefix(commitment[0])?;
    let payload = commitment[1..].to_vec();
    if payload.is_empty() {
        return Err(AltDAProviderError::InvalidCommitment("commitment payload is empty"));
    }

    if commitment_type == AltDACommitmentType::Keccak256 && payload.len() != 32 {
        return Err(AltDAProviderError::InvalidCommitmentLength {
            expected: 32,
            actual: payload.len(),
        });
    }

    Ok(ParsedAltDACommitment { commitment_type, payload })
}

#[derive(Debug, Error)]
pub enum AltDAProviderError {
    #[error("preimage oracle error: {0}")]
    Preimage(#[from] PreimageOracleError),
    #[error("invalid AltDA commitment: {0}")]
    InvalidCommitment(&'static str),
    #[error("unknown AltDA commitment type prefix: {0:#x}")]
    UnknownCommitmentType(u8),
    #[error("invalid AltDA commitment length (expected {expected}, got {actual})")]
    InvalidCommitmentLength { expected: usize, actual: usize },
    #[error("AltDA commitment does not match fetched input")]
    CommitmentMismatch,
}

impl From<AltDAProviderError> for PipelineErrorKind {
    fn from(value: AltDAProviderError) -> Self {
        PipelineError::Provider(value.to_string()).temp()
    }
}

#[async_trait]
pub trait AltDAInputProvider {
    type Error: Display + ToString + Into<PipelineErrorKind> + Send + Sync;

    async fn get_input(&mut self, commitment: &[u8]) -> Result<Bytes, Self::Error>;
}

pub trait AltDAInputProviderFactory<O>: AltDAInputProvider + Sized {
    fn from_oracle(oracle: Arc<O>) -> Self;
}

#[derive(Debug, Clone)]
pub struct OracleAltDAInputProvider<T: CommsClient> {
    oracle: Arc<T>,
}

impl<T: CommsClient> OracleAltDAInputProvider<T> {
    pub fn new(oracle: Arc<T>) -> Self {
        Self { oracle }
    }
}

impl<T: CommsClient + Send + Sync> AltDAInputProviderFactory<T> for OracleAltDAInputProvider<T> {
    fn from_oracle(oracle: Arc<T>) -> Self {
        Self::new(oracle)
    }
}

#[async_trait]
impl<T: CommsClient + Send + Sync> AltDAInputProvider for OracleAltDAInputProvider<T> {
    type Error = AltDAProviderError;

    async fn get_input(&mut self, commitment: &[u8]) -> Result<Bytes, Self::Error> {
        let parsed_commitment = parse_altda_commitment(commitment)?;

        let hint = Hint::new(AltDAHintType::AltDACommitment, commitment.to_vec());
        self.oracle.write(&hint.encode()).await?;

        let key = PreimageKey::new(*keccak256(commitment), PreimageKeyType::GlobalGeneric);
        let input = self.oracle.get(key).await?;
        if parsed_commitment.commitment_type == AltDACommitmentType::Keccak256 &&
            keccak256(input.as_slice()).as_slice() != parsed_commitment.payload.as_slice()
        {
            return Err(AltDAProviderError::CommitmentMismatch);
        }
        Ok(input.into())
    }
}
