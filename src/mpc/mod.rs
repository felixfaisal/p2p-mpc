pub mod aux_party;
pub mod keygen_party;
pub mod orchestrator;
pub mod signing_party;

pub use aux_party::AuxParty;
pub use keygen_party::KeygenParty;
pub use orchestrator::{CoordinationMessage, MPCOrchestrator, ProtocolState};
pub use signing_party::SigningParty;

use cggmp24::security_level::SecurityLevel128;
use cggmp24::supported_curves::Secp256k1;
use sha2::Sha256;

pub type SecurityLevelTest = SecurityLevel128;
pub type AuxMsg = cggmp24::key_refresh::Msg<Sha256, SecurityLevelTest>;
pub type ThresholdMessage =
    cggmp24::keygen::msg::threshold::Msg<Secp256k1, SecurityLevelTest, Sha256>;
pub type SigningMessage = cggmp24::signing::msg::Msg<Secp256k1, Sha256>;

pub type PartyId = u16;
