use std::{fs, path::PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};
use starknet_types_core::felt::Felt;
use thiserror::Error;

use crate::errors::{Error, InitializationError};

#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Args {
    #[arg(long)]
    config: PathBuf,
}

impl Args {
    pub fn read_config(&self) -> Result<Config, Error> {
        let file_content =
            fs::read_to_string(&self.config).map_err(InitializationError::CannotReadConfig)?;

        let config = toml::from_str(&file_content).map_err(InitializationError::Toml)?;

        Ok(config)
    }
}

/// The chain where the represented assets live
#[derive(Debug, Clone, Copy)]
pub enum ChainId {
    /// Starknet mainnet
    Mainnet,
    /// Starknet sepolia testnet
    Sepolia,
    /// A custom network
    ///
    /// The inner value should be a valid cairo short string, otherwise IO will panic
    Custom(Felt),
}

impl ChainId {
    pub fn new_custom(s: &str) -> Result<Self, cashu_starknet::CairoShortStringToFeltError> {
        let short_string = cashu_starknet::felt_from_short_string(s)?;

        Ok(Self::Custom(short_string))
    }
}

impl std::fmt::Display for ChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainId::Mainnet => std::fmt::Display::fmt("mainnet", f),
            ChainId::Sepolia => std::fmt::Display::fmt("sepolia", f),
            ChainId::Custom(felt) => {
                let as_short_string =
                    cashu_starknet::felt_to_short_string(*felt).map_err(|_| std::fmt::Error)?;
                std::fmt::Display::fmt(&as_short_string, f)
            }
        }
    }
}

impl Serialize for ChainId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let as_string = self.to_string();

        serializer.serialize_str(&as_string)
    }
}

impl<'de> Deserialize<'de> for ChainId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let short_string = <String>::deserialize(deserializer)?;
        match short_string.as_str() {
            "mainnet" => Ok(ChainId::Mainnet),
            "sepolia" => Ok(ChainId::Sepolia),
            s => ChainId::new_custom(s).map_err(|_| {
                serde::de::Error::invalid_value(
                    serde::de::Unexpected::Str(s),
                    &"a valid cairo short string",
                )
            }),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// The chain we are using as backend
    pub chain_id: ChainId,
    /// The address of the STRK token address
    ///
    /// Optional if chain_id is "mainnet" or "sepolia"
    strk_address: Option<Felt>,
    /// The url of the signer service
    pub signer_url: String,
    /// The address of the on-chain account managing deposited assets
    pub recipient_address: Felt,
    pub grpc_server_port: String,
}

const MAINNET_STRK_TOKEN_CONTRACT: Felt =
    Felt::from_hex_unchecked("0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d");
const SEPOLIA_STRK_TOKEN_CONTRACT: Felt =
    Felt::from_hex_unchecked("0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d");

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Cannot specify custom STRK contract Address for chain {0}")]
    CannotSpecifyCustomContractAddressForChainId(ChainId),
    #[error("Must specify custom STRK contract Address for custom chains")]
    MustSpecifyCustomContractAddressForCustom,
}

impl Config {
    pub fn strk_token_contract_address(&self) -> Result<Felt, ConfigError> {
        match (self.chain_id, self.strk_address) {
            (ChainId::Mainnet, None) => Ok(MAINNET_STRK_TOKEN_CONTRACT),
            (ChainId::Sepolia, None) => Ok(SEPOLIA_STRK_TOKEN_CONTRACT),
            (ChainId::Custom(_), Some(f)) => Ok(f),
            (ChainId::Custom(_), None) => {
                Err(ConfigError::MustSpecifyCustomContractAddressForCustom)
            }
            (chain_id, Some(_)) => Err(ConfigError::CannotSpecifyCustomContractAddressForChainId(
                chain_id,
            )),
        }
    }
}

pub fn read_env_variables() -> Result<EnvVariables, InitializationError> {
    // Only if we are in debug mode, we allow loading env variable from a .env file
    #[cfg(debug_assertions)]
    dotenvy::from_filename("node.env").map_err(InitializationError::Dotenvy)?;

    let apibara_token = std::env::var("APIBARA_TOKEN").map_err(InitializationError::Env)?;
    let pg_url = std::env::var("PG_URL").map_err(InitializationError::Env)?;

    Ok(EnvVariables {
        apibara_token,
        pg_url,
    })
}

#[derive(Debug)]
pub struct EnvVariables {
    pub apibara_token: String,
    pub pg_url: String,
}
