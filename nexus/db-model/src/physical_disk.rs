// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::{Generation, PhysicalDiskKind};
use crate::collection::DatastoreCollectionConfig;
use crate::schema::{physical_disk, zpool};
use chrono::{DateTime, Utc};
use db_macros::Asset;
use uuid::Uuid;

/// Physical disk attached to sled.
#[derive(Queryable, Insertable, Debug, Clone, Selectable, Asset)]
#[diesel(table_name = physical_disk)]
pub struct PhysicalDisk {
    #[diesel(embed)]
    identity: PhysicalDiskIdentity,
    time_deleted: Option<DateTime<Utc>>,
    rcgen: Generation,

    pub vendor: String,
    pub serial: String,
    pub model: String,

    pub variant: PhysicalDiskKind,
    pub sled_id: Uuid,
    pub total_size: i64,
}

impl PhysicalDisk {
    pub fn new(
        vendor: String,
        serial: String,
        model: String,
        variant: PhysicalDiskKind,
        sled_id: Uuid,
        total_size: i64,
    ) -> Self {
        Self {
            identity: PhysicalDiskIdentity::new(Uuid::new_v4()),
            time_deleted: None,
            rcgen: Generation::new(),
            vendor,
            serial,
            model,
            variant,
            sled_id,
            total_size,
        }
    }
}

impl DatastoreCollectionConfig<super::Zpool> for PhysicalDisk {
    type CollectionId = Uuid;
    type GenerationNumberColumn = physical_disk::dsl::rcgen;
    type CollectionTimeDeletedColumn = physical_disk::dsl::time_deleted;
    type CollectionIdColumn = zpool::dsl::sled_id;
}
