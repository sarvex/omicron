use omicron_common::api::internal::nexus::DiskRuntimeState;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

///Used to request a Disk state change
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase", tag = "state", content = "instance")]
pub enum DiskStateRequested {
    Detached,
    Attached(Uuid),
    Destroyed,
    Faulted,
}

impl DiskStateRequested {
    /// Returns whether the requested state is attached to an Instance or not.
    pub fn is_attached(&self) -> bool {
        match self {
            DiskStateRequested::Detached => false,
            DiskStateRequested::Destroyed => false,
            DiskStateRequested::Faulted => false,

            DiskStateRequested::Attached(_) => true,
        }
    }
}

/// Sent from to a sled agent to establish the runtime state of a Disk
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DiskEnsureBody {
    /// Last runtime state of the Disk known to Nexus (used if the agent has
    /// never seen this Disk before).
    pub initial_runtime: DiskRuntimeState,
    /// requested runtime state of the Disk
    pub target: DiskStateRequested,
}
