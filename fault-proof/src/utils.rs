use sp1_sdk::{NetworkProver, CudaProver, ProverClient};
use std::{env, sync::Arc};
use anyhow::Result;
use tracing_subscriber::{fmt, EnvFilter};

use crate::config::SP1ProverMode;

/// Enum to hold either NetworkProver or CudaProver
pub enum SP1ProverType {
    Network(Arc<NetworkProver>),
    Cuda(Arc<CudaProver>),
}

impl SP1ProverType {
    /// Get the network prover, panicking if it's not a network prover
    pub fn as_network(&self) -> &Arc<NetworkProver> {
        match self {
            SP1ProverType::Network(prover) => prover,
            SP1ProverType::Cuda(_) => panic!("Expected network prover but got CUDA prover"),
        }
    }

    /// Get the CUDA prover, panicking if it's not a CUDA prover
    pub fn as_cuda(&self) -> &Arc<CudaProver> {
        match self {
            SP1ProverType::Cuda(prover) => prover,
            SP1ProverType::Network(_) => panic!("Expected CUDA prover but got network prover"),
        }
    }
}

pub fn setup_logging() {
    let format = fmt::format()
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .with_ansi(true);

    // Initialize logging using RUST_LOG environment variable, defaulting to INFO level
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| {
            EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into())
        }))
        .event_format(format)
        .init();
}

/// Creates a ProverClient based on the specified SP1ProverMode.
/// 
/// # Arguments
/// 
/// * `sp1_prover_mode` - The mode to use for the prover (Network or Cuda)
/// 
/// # Returns
/// 
/// * `Result<SP1ProverType>` - The configured prover client wrapped in an enum
/// 
/// # Errors
/// 
/// This function will return an error if the NETWORK_PRIVATE_KEY environment variable
/// is not set when using network mode.
pub fn create_prover_client(sp1_prover_mode: &SP1ProverMode) -> Result<SP1ProverType> {
    match sp1_prover_mode {
        SP1ProverMode::Network => {
            // Set a default network private key to avoid an error in mock mode.
            let private_key = env::var("NETWORK_PRIVATE_KEY").unwrap_or_else(|_| {
                tracing::warn!(
                    "Using default NETWORK_PRIVATE_KEY of 0x01. This is only valid in mock mode."
                );
                "0x0000000000000000000000000000000000000000000000000000000000000001".to_string()
            });

            let prover = ProverClient::builder()
                .network()
                .private_key(&private_key)
                .build();
            
            tracing::info!("Created SP1 ProverClient in network mode");
            Ok(SP1ProverType::Network(Arc::new(prover)))
        }
        SP1ProverMode::Cuda => {
            let prover = ProverClient::builder()
                .cuda()
                .build();
            
            tracing::info!("Created SP1 ProverClient in CUDA mode");
            Ok(SP1ProverType::Cuda(Arc::new(prover)))
        }
    }
}
