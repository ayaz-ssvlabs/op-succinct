use lazy_static::lazy_static;
use std::path::PathBuf;

fn get_workspace_root() -> PathBuf {
    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = start.as_path();

    loop {
        let has_cargo_toml = dir.join("Cargo.toml").is_file();
        let has_contracts_dir = dir.join("contracts").is_dir();
        if has_cargo_toml && has_contracts_dir {
            return dir.to_path_buf();
        }

        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }

    start
}

lazy_static! {
    pub static ref OP_SUCCINCT_L2_OUTPUT_ORACLE_CONFIG_PATH: PathBuf = {
        std::env::var("OP_SUCCINCT_L2_OUTPUT_ORACLE_CONFIG_PATH")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                get_workspace_root().join("contracts").join("opsuccinctl2ooconfig.json")
            })
    };
    pub static ref OP_SUCCINCT_FAULT_DISPUTE_GAME_CONFIG_PATH: PathBuf = {
        std::env::var("OP_SUCCINCT_FAULT_DISPUTE_GAME_CONFIG_PATH")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                get_workspace_root().join("contracts").join("opsuccinctfdgconfig.json")
            })
    };
}
