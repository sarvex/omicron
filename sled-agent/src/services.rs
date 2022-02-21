// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Support for miscellaneous services managed by the sled.

use crate::illumos::running_zone::{RunningZone, InstalledZone};
use crate::illumos::vnic::VnicAllocator;
use crate::illumos::zone::AddressRequest;
use omicron_common::api::internal::sled_agent::ServiceRequest;
use slog::Logger;
use std::collections::HashSet;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Cannot serialize TOML file")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Cannot deserialize TOML file")]
    TomlDeserialize(#[from] toml::de::Error),

    #[error("Error accessing filesystem: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    RunningZone(#[from] crate::illumos::running_zone::Error),

    #[error("Services already configured for this Sled Agent")]
    ServicesAlreadyConfigured,
}

impl From<Error> for omicron_common::api::external::Error {
    fn from(err: Error) -> Self {
        omicron_common::api::external::Error::InternalError {
            internal_message: err.to_string(),
        }
    }
}

fn services_config_path() -> PathBuf {
    Path::new(crate::OMICRON_CONFIG_PATH).join("services.toml")
}

/// Manages miscellaneous Sled-local services.
pub struct ServiceManager {
    log: Logger,
    zones: Mutex<Vec<RunningZone>>,
    vnic_allocator: VnicAllocator,
}

impl ServiceManager {
    /// Creates a service manager, which returns once all requested services
    /// have been started.
    pub async fn new(
        log: Logger,
    ) -> Result<Self, Error> {
        let mgr = Self {
            log,
            zones: Mutex::new(vec![]),
            vnic_allocator: VnicAllocator::new("Service")
        };

        let config_path = services_config_path();
        if config_path.exists() {
            let requests: Vec<ServiceRequest> = toml::from_str(
                &tokio::fs::read_to_string(&services_config_path()).await?,
            )?;
            let mut existing_zones = mgr.zones.lock().await;
            mgr.initialize_services_locked(&mut existing_zones, &requests).await?;
        }

        Ok(mgr)
    }

    // Populates `existing_zones` according to the requests in `services`.
    async fn initialize_services_locked(
        &self,
        existing_zones: &mut Vec<RunningZone>,
        services: &Vec<ServiceRequest>
    ) -> Result<(), Error> {
        // TODO: As long as we ensure the requests don't overlap, we could
        // parallelize this request.
        for service in services {
            // Before we bother allocating anything for this request, check if
            // this service has already been created.
            if let Some(existing_zone) = existing_zones.iter().find(|z| {
                z.name() == service.name || z.address() == service.address
            }) {
                // The caller is requesting that we instantiate a zone that
                // already exists, with the desired configuration.
                if existing_zone.name() == service.name &&
                    existing_zone.address() == service.address {
                    continue;
                }
                // Otherwise, there is a request which collides with our
                // existing zones.
                return Err(Error::ServicesAlreadyConfigured);
            }

            let installed_zone = InstalledZone::install(
                &self.log,
                &self.vnic_allocator,
                &service.name,
                /* unique_name= */ None,
                /* dataset= */ &[],
                /* devices= */ &[],
                /* vnics= */ vec![],
            ).await?;

            let running_zone = RunningZone::boot(
                installed_zone,
                AddressRequest::new_static(service.address.ip(), None),
                service.address.port(),
            ).await?;
            existing_zones.push(running_zone);
        }
        Ok(())
    }

    /// Ensures that particular services should be initialized.
    ///
    /// These services will be instantiated by this function, will be recorded
    /// to a local file to ensure they start automatically on next boot.
    pub async fn ensure(
        &self,
        services: Vec<ServiceRequest>,
    ) -> Result<(), Error> {
        let mut existing_zones = self.zones.lock().await;
        let config_path = services_config_path();
        if config_path.exists() {
            let known_services: Vec<ServiceRequest> = toml::from_str(
                &tokio::fs::read_to_string(&services_config_path()).await?,
            )?;

            let known_set: HashSet<&ServiceRequest> = HashSet::from_iter(known_services.iter());
            let requested_set = HashSet::from_iter(services.iter());

            if known_set != requested_set {
                // If the caller is requesting we instantiate a
                // zone that exists, but isn't what they're asking for, throw an
                // error.
                //
                // We may want to use a different mechanism for zone removal, in
                // the case of changing configurations, rather than just doing
                // that removal implicitly.
                warn!(
                    self.log,
                    "Cannot request services on this sled, differing configurations: {:?}",
                    known_set.symmetric_difference(&requested_set)
                );
                return Err(Error::ServicesAlreadyConfigured);
            }
        }

        self.initialize_services_locked(&mut existing_zones, &services).await?;

        tokio::fs::write(
            &services_config_path(),
            toml::to_string(&services)?,
        ).await?;

        Ok(())
    }

}
