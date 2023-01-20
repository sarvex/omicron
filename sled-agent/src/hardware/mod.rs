// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::path::PathBuf;

cfg_if::cfg_if! {
    if #[cfg(target_os = "illumos")] {
        mod illumos;
        pub(crate) use illumos::*;
    } else {
        mod non_illumos;
        pub(crate) use non_illumos::*;
    }
}

/// Provides information from the underlying hardware about updates
/// which may require action on behalf of the Sled Agent.
///
/// These updates should generally be "non-opinionated" - the higher
/// layers of the sled agent can make the call to ignore these updates
/// or not.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum HardwareUpdate {
    TofinoDeviceChange,
    TofinoLoaded,
    TofinoUnloaded,
    DiskAdded(Disk),
    DiskRemoved(Disk),
}

#[derive(Debug, thiserror::Error)]
pub enum DiskError {
    #[error("Cannot open {path} due to {error}")]
    IoError { path: PathBuf, error: std::io::Error },
    #[error("Failed to open partition at {path} due to {error}")]
    Gpt { path: PathBuf, error: anyhow::Error },
    #[error("Unexpected partition layout at {path}")]
    BadPartitionLayout { path: PathBuf },
    #[error("Requested partition {partition:?} not found on device {path}")]
    NotFound { path: PathBuf, partition: Partition },
    #[error(transparent)]
    ZpoolCreate(#[from] crate::illumos::zpool::CreateError),
    #[error("Cannot format {path}: missing a '/dev/ path")]
    CannotFormatMissingDevPath { path: PathBuf },
}

/// A partition (or 'slice') of a disk.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum Partition {
    /// The partition may be used to boot an OS image.
    BootImage,
    /// Reserved for future use.
    Reserved,
    /// The partition may be used as a dump device.
    DumpDevice,
    /// The partition may contain a ZFS pool.
    ZfsPool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiskPaths {
    // Full path to the disk under "/devices".
    // Should NOT end with a ":partition_letter".
    devfs_path: PathBuf,
    // Optional path to the disk under "/dev/dsk".
    dev_path: Option<PathBuf>,
}

impl DiskPaths {
    // Returns the "illumos letter-indexed path" for a device.
    fn partition_path(&self, index: usize) -> Option<PathBuf> {
        let index = u8::try_from(index).ok()?;

        let path = self.devfs_path.display();
        let character = match index {
            0..=5 => (b'a' + index) as char,
            _ => return None,
        };
        Some(PathBuf::from(format!("{path}:{character}")))
    }

    /// Returns the path to the whole disk
    #[allow(dead_code)]
    pub(crate) fn whole_disk(&self, raw: bool) -> PathBuf {
        PathBuf::from(format!(
            "{path}:wd{raw}",
            path = self.devfs_path.display(),
            raw = if raw { ",raw" } else { "" },
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Disk {
    paths: DiskPaths,
    slot: i64,
    variant: DiskVariant,
    partitions: Vec<Partition>,
    // TODO: Device ID?
}

impl Disk {
    #[allow(dead_code)]
    pub fn new(
        devfs_path: PathBuf,
        dev_path: Option<PathBuf>,
        slot: i64,
        variant: DiskVariant,
    ) -> Result<Self, DiskError> {
        let paths = DiskPaths { devfs_path, dev_path };
        let partitions = ensure_partition_layout(&paths, variant)?;
        Ok(Self { paths, slot, variant, partitions })
    }

    // Finds the first 'variant' partition, and returns the path to it.
    fn partition_device_path(
        &self,
        expected_partition: Partition,
    ) -> Result<PathBuf, DiskError> {
        for (index, partition) in self.partitions.iter().enumerate() {
            if &expected_partition == partition {
                let path =
                    self.paths.partition_path(index).ok_or_else(|| {
                        DiskError::NotFound {
                            path: self.paths.devfs_path.clone(),
                            partition: expected_partition,
                        }
                    })?;
                return Ok(path);
            }
        }
        return Err(DiskError::NotFound {
            path: self.paths.devfs_path.clone(),
            partition: expected_partition,
        });
    }

    pub async fn zpool_path(&self) -> Result<PathBuf, DiskError> {
        self.partition_device_path(Partition::ZfsPool)
    }

    #[allow(dead_code)]
    pub async fn boot_path(&self) -> Result<PathBuf, DiskError> {
        self.partition_device_path(Partition::BootImage)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum DiskVariant {
    U2,
    M2,
}
