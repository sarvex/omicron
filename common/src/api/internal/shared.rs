// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Types shared between Nexus and Sled Agent.

use crate::api::external;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

/// The type of network interface
#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Deserialize,
    Serialize,
    JsonSchema,
    Hash,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NetworkInterfaceKind {
    /// A vNIC attached to a guest instance
    Instance { id: Uuid },
    /// A vNIC associated with an internal service
    Service { id: Uuid },
}

/// Information required to construct a virtual network interface
#[derive(
    Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Hash,
)]
pub struct NetworkInterface {
    pub id: Uuid,
    pub kind: NetworkInterfaceKind,
    pub name: external::Name,
    pub ip: IpAddr,
    pub mac: external::MacAddr,
    pub subnet: external::IpNet,
    pub vni: external::Vni,
    pub primary: bool,
    pub slot: u8,
}

/// An IP address and port range used for source NAT, i.e., making
/// outbound network connections from guests or services.
#[derive(
    Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Hash,
)]
pub struct SourceNatConfig {
    /// The external address provided to the instance or service.
    pub ip: IpAddr,
    /// The first port used for source NAT, inclusive.
    pub first_port: u16,
    /// The last port used for source NAT, also inclusive.
    pub last_port: u16,
}
