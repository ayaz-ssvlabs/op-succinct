use alloy_primitives::{Address, Bytes};
use async_trait::async_trait;
use kona_derive::{
    BlobProvider, ChainProvider, DataAvailabilityProvider, EthereumDataSource, PipelineError,
    PipelineErrorKind, PipelineResult,
};
use kona_protocol::BlockInfo;
use std::fmt::Debug;
use tracing::warn;

use crate::provider::{parse_altda_commitment, AltDAInputProvider, ALTDA_DERIVATION_VERSION};

#[derive(Debug, Clone)]
pub struct AltDASource<P>
where
    P: AltDAInputProvider + Send + Clone,
{
    pub input_provider: P,
}

impl<P> AltDASource<P>
where
    P: AltDAInputProvider + Send + Clone,
{
    pub const fn new(input_provider: P) -> Self {
        Self { input_provider }
    }

    pub async fn next(&mut self, commitment: &[u8]) -> Result<Bytes, P::Error> {
        self.input_provider.get_input(commitment).await
    }

    pub fn clear(&mut self) {}
}

#[derive(Debug, Clone)]
pub struct AltDADataSource<C, B, P>
where
    C: ChainProvider + Send + Clone,
    B: BlobProvider + Send + Clone,
    P: AltDAInputProvider + Send + Clone,
{
    pub ethereum_source: EthereumDataSource<C, B>,
    pub altda_source: AltDASource<P>,
    pub pending_commitment: Option<Bytes>,
}

impl<C, B, P> AltDADataSource<C, B, P>
where
    C: ChainProvider + Send + Clone + Debug,
    B: BlobProvider + Send + Clone + Debug,
    P: AltDAInputProvider + Send + Clone + Debug,
{
    pub const fn new(
        ethereum_source: EthereumDataSource<C, B>,
        altda_source: AltDASource<P>,
    ) -> Self {
        Self { ethereum_source, altda_source, pending_commitment: None }
    }

    fn parse_commitment_data(data: &Bytes) -> PipelineResult<Bytes> {
        if data.len() <= 2 {
            return Err(PipelineError::NotEnoughData.temp());
        }

        let commitment = &data[1..];
        if let Err(err) = parse_altda_commitment(commitment) {
            warn!("invalid AltDA commitment, skipping batch: {}", err);
            return Err(PipelineError::NotEnoughData.temp());
        }

        Ok(Bytes::copy_from_slice(commitment))
    }
}

#[async_trait]
impl<C, B, P> DataAvailabilityProvider for AltDADataSource<C, B, P>
where
    C: ChainProvider + Send + Sync + Clone + Debug,
    B: BlobProvider + Send + Sync + Clone + Debug,
    P: AltDAInputProvider + Send + Sync + Clone + Debug,
{
    type Item = Bytes;

    async fn next(
        &mut self,
        block_ref: &BlockInfo,
        batcher_addr: Address,
    ) -> PipelineResult<Self::Item> {
        if self.pending_commitment.is_none() {
            let data = match self.ethereum_source.next(block_ref, batcher_addr).await {
                Err(err @ PipelineErrorKind::Temporary(PipelineError::Eof)) => {
                    self.clear();
                    return Err(err);
                }
                other => other?,
            };

            if data.is_empty() {
                return Err(PipelineError::NotEnoughData.temp());
            }

            if data[0] != ALTDA_DERIVATION_VERSION {
                return Ok(data);
            }

            self.pending_commitment = Some(Self::parse_commitment_data(&data)?);
        }

        let commitment = self
            .pending_commitment
            .clone()
            .expect("pending commitment must exist when fetching AltDA input");

        match self.altda_source.next(commitment.as_ref()).await {
            Ok(input) => {
                self.pending_commitment = None;
                Ok(input)
            }
            Err(err) => Err(err.into()),
        }
    }

    fn clear(&mut self) {
        self.pending_commitment = None;
        self.altda_source.clear();
        self.ethereum_source.clear();
    }
}
