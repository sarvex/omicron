// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Executable program to run a simulated sled agent

// TODO see the TODO for nexus.

use clap::Parser;
use dropshot::ConfigDropshot;
use dropshot::ConfigLogging;
use dropshot::ConfigLoggingLevel;
use omicron_common::cmd::fatal;
use omicron_common::cmd::CmdError;
use omicron_sled_agent::sim::RssArgs;
use omicron_sled_agent::sim::{
    run_server, Config, ConfigHardware, ConfigStorage, ConfigUpdates,
    ConfigZpool, SimMode,
};
use std::net::SocketAddr;
use std::net::SocketAddrV6;
use uuid::Uuid;

fn parse_sim_mode(src: &str) -> Result<SimMode, String> {
    match src {
        "auto" => Ok(SimMode::Auto),
        "explicit" => Ok(SimMode::Explicit),
        mode => Err(format!("Invalid sim mode: {}", mode)),
    }
}

#[derive(Debug, Parser)]
#[clap(name = "sled_agent", about = "See README.adoc for more information")]
struct Args {
    #[clap(
        long = "sim-mode",
        value_parser = parse_sim_mode,
        default_value = "auto",
        help = "Automatically simulate transitions",
    )]
    sim_mode: SimMode,

    #[clap(name = "SA_UUID", action)]
    uuid: Uuid,

    #[clap(name = "SA_IP:PORT", action)]
    sled_agent_addr: SocketAddrV6,

    #[clap(name = "NEXUS_IP:PORT", action)]
    nexus_addr: SocketAddr,

    #[clap(long, name = "NEXUS_EXTERNAL_IP:PORT", action)]
    /// If specified, when the simulated sled agent initializes the rack, it
    /// will record the Nexus service running with the specified external IP
    /// address.  When combined with DNS_EXTERNAL_IP:PORT, this will cause
    /// Nexus to publish DNS names to external DNS.
    rss_nexus_external_addr: Option<SocketAddr>,

    #[clap(long, name = "EXTERNAL_DNS_INTERNAL_IP:PORT", action)]
    /// If specified, when the simulated sled agent initializes the rack, it
    /// will record the external DNS service running with the specified internal
    /// IP address.  When combined with NEXUS_EXTERNAL_IP:PORT, this will cause
    /// Nexus to publish DNS names to external DNS.
    rss_external_dns_internal_addr: Option<SocketAddrV6>,
}

#[tokio::main]
async fn main() {
    if let Err(message) = do_run().await {
        fatal(message);
    }
}

async fn do_run() -> Result<(), CmdError> {
    let args = Args::parse();

    let tmp = camino_tempfile::tempdir()
        .map_err(|e| CmdError::Failure(e.to_string()))?;
    let config = Config {
        id: args.uuid,
        sim_mode: args.sim_mode,
        nexus_address: args.nexus_addr,
        dropshot: ConfigDropshot {
            bind_address: args.sled_agent_addr.into(),
            request_body_max_bytes: 1024 * 1024,
            ..Default::default()
        },
        log: ConfigLogging::StderrTerminal { level: ConfigLoggingLevel::Info },
        storage: ConfigStorage {
            // Create 10 "virtual" U.2s, with 1 TB of storage.
            zpools: vec![ConfigZpool { size: 1 << 40 }; 10],
            ip: (*args.sled_agent_addr.ip()).into(),
        },
        updates: ConfigUpdates { zone_artifact_path: tmp.path().to_path_buf() },
        hardware: ConfigHardware {
            hardware_threads: 32,
            physical_ram: 64 * (1 << 30),
        },
    };

    let rss_args = RssArgs {
        nexus_external_addr: args.rss_nexus_external_addr,
        external_dns_internal_addr: args.rss_external_dns_internal_addr,
    };

    run_server(&config, &rss_args).await.map_err(CmdError::Failure)
}
