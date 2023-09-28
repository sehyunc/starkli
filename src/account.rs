use std::fmt::Display;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use starknet::{
    core::{
        serde::unsigned_field_element::UfeHex, types::FieldElement, utils::get_contract_address,
    },
    macros::{felt, selector},
};

const BRAAVOS_SIGNER_TYPE_STARK: FieldElement = FieldElement::ONE;

pub const KNOWN_ACCOUNT_CLASSES: [KnownAccountClass; 4] = [
    KnownAccountClass {
        class_hash: felt!("0x048dd59fabc729a5db3afdf649ecaf388e931647ab2f53ca3c6183fa480aa292"),
        variant: AccountVariantType::OpenZeppelin,
        description: "OpenZeppelin account contract v0.6.1 compiled with cairo-lang v0.11.0.2",
    },
    KnownAccountClass {
        class_hash: felt!("0x04d07e40e93398ed3c76981e72dd1fd22557a78ce36c0515f679e27f0bb5bc5f"),
        variant: AccountVariantType::OpenZeppelin,
        description: "OpenZeppelin account contract v0.5.0 compiled with cairo-lang v0.10.1",
    },
    KnownAccountClass {
        class_hash: felt!("0x03131fa018d520a037686ce3efddeab8f28895662f019ca3ca18a626650f7d1e"),
        variant: AccountVariantType::Braavos,
        description: "Braavos official proxy account",
    },
    KnownAccountClass {
        class_hash: felt!("0x01a736d6ed154502257f02b1ccdf4d9d1089f80811cd6acad48e6b6a9d1f2003"),
        variant: AccountVariantType::Argent,
        description: "Argent X official account",
    },
];

#[derive(Serialize, Deserialize)]
pub struct AccountConfig {
    pub version: u64,
    pub variant: AccountVariant,
    pub deployment: DeploymentStatus,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AccountVariant {
    OpenZeppelin(OzAccountConfig),
    Argent(ArgentAccountConfig),
    Braavos(BraavosAccountConfig),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DeploymentStatus {
    Undeployed(UndeployedStatus),
    Deployed(DeployedStatus),
}

pub struct KnownAccountClass {
    pub class_hash: FieldElement,
    pub variant: AccountVariantType,
    pub description: &'static str,
}

pub enum AccountVariantType {
    OpenZeppelin,
    Argent,
    Braavos,
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct OzAccountConfig {
    pub version: u64,
    #[serde_as(as = "UfeHex")]
    pub public_key: FieldElement,
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct ArgentAccountConfig {
    pub version: u64,
    #[serde_as(as = "UfeHex")]
    pub implementation: FieldElement,
    #[serde_as(as = "UfeHex")]
    pub signer: FieldElement,
    #[serde_as(as = "UfeHex")]
    pub guardian: FieldElement,
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct BraavosAccountConfig {
    pub version: u64,
    #[serde_as(as = "UfeHex")]
    pub implementation: FieldElement,
    pub multisig: BraavosMultisigConfig,
    pub signers: Vec<BraavosSigner>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BraavosMultisigConfig {
    On { num_signers: usize },
    Off,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BraavosSigner {
    Stark(BraavosStarkSigner),
    // TODO: add secp256r1
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct BraavosStarkSigner {
    #[serde_as(as = "UfeHex")]
    pub public_key: FieldElement,
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct UndeployedStatus {
    #[serde_as(as = "UfeHex")]
    pub class_hash: FieldElement,
    #[serde_as(as = "UfeHex")]
    pub salt: FieldElement,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<DeploymentContext>,
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct DeployedStatus {
    #[serde_as(as = "UfeHex")]
    pub class_hash: FieldElement,
    #[serde_as(as = "UfeHex")]
    pub address: FieldElement,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "variant", rename_all = "snake_case")]
pub enum DeploymentContext {
    Braavos(BraavosDeploymentContext),
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct BraavosDeploymentContext {
    #[serde_as(as = "UfeHex")]
    pub mock_implementation: FieldElement,
}

impl AccountConfig {
    pub fn deploy_account_address(&self) -> Result<FieldElement> {
        let undeployed_status = match &self.deployment {
            DeploymentStatus::Undeployed(value) => value,
            DeploymentStatus::Deployed(_) => {
                anyhow::bail!("account already deployed");
            }
        };

        match &self.variant {
            AccountVariant::OpenZeppelin(oz) => Ok(get_contract_address(
                undeployed_status.salt,
                undeployed_status.class_hash,
                &[oz.public_key],
                FieldElement::ZERO,
            )),
            AccountVariant::Argent(argent) => Ok(get_contract_address(
                undeployed_status.salt,
                undeployed_status.class_hash,
                &[
                    argent.implementation,   // implementation
                    selector!("initialize"), // selector
                    FieldElement::TWO,       // calldata_len
                    argent.signer,           // calldata[0]: signer
                    argent.guardian,         // calldata[1]: guardian
                ],
                FieldElement::ZERO,
            )),
            AccountVariant::Braavos(braavos) => {
                if !matches!(braavos.multisig, BraavosMultisigConfig::Off) {
                    anyhow::bail!("Braavos accounts cannot be deployed with multisig on");
                }
                if braavos.signers.len() != 1 {
                    anyhow::bail!("Braavos accounts can only be deployed with one seed signer");
                }

                match &undeployed_status.context {
                    Some(DeploymentContext::Braavos(context)) => {
                        // Safe to unwrap as we already checked for length
                        match braavos.signers.get(0).unwrap() {
                            BraavosSigner::Stark(stark_signer) => {
                                Ok(get_contract_address(
                                    undeployed_status.salt,
                                    undeployed_status.class_hash,
                                    &[
                                        context.mock_implementation, // implementation_address
                                        selector!("initializer"),    // initializer_selector
                                        FieldElement::ONE,           // calldata_len
                                        stark_signer.public_key,     // calldata[0]: public_key
                                    ],
                                    FieldElement::ZERO,
                                ))
                            } // Reject other variants as we add more types
                        }
                    }
                    _ => Err(anyhow::anyhow!("missing Braavos deployment context")),
                }
            }
        }
    }
}

impl BraavosSigner {
    pub fn decode(raw_signer_model: &[FieldElement]) -> Result<Self> {
        let raw_signer_type = raw_signer_model
            .get(4)
            .ok_or_else(|| anyhow::anyhow!("unable to read `type` field"))?;

        if raw_signer_type == &BRAAVOS_SIGNER_TYPE_STARK {
            // Index access is safe as we already checked getting the element after
            let public_key = raw_signer_model[0];

            Ok(Self::Stark(BraavosStarkSigner { public_key }))
        } else {
            Err(anyhow::anyhow!("unknown signer type: {}", raw_signer_type))
        }
    }
}

impl Display for AccountVariantType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountVariantType::OpenZeppelin => write!(f, "OpenZeppelin"),
            AccountVariantType::Argent => write!(f, "Argent X"),
            AccountVariantType::Braavos => write!(f, "Braavos"),
        }
    }
}
