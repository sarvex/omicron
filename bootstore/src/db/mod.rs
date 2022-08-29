// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Database layer for the bootstore
mod macros;
mod models;
mod schema;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::SqliteConnection;
use slog::Logger;
use slog::{info, o};

use crate::trust_quorum::SerializableShareDistribution;
use models::EncryptedRootSecret;
use models::KeyShareCommit;
use models::KeySharePrepare;
use models::Share;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to open db connection to {path}: {err}")]
    DbOpen { path: String, err: ConnectionError },

    #[error(transparent)]
    Db(#[from] diesel::result::Error),
}

/// The ID of the database used to store blobs.
///
/// We separate them because they are encrypted and accessed differently.
pub enum DbId {
    /// Used pre-rack unlock: Contains key shares and membership data
    TrustQuorum,

    /// Used post-rack unlock: Contains information necessary for setting
    /// up NTP.
    NetworkConfig,
}

pub struct Db {
    log: Logger,
    conn: SqliteConnection,
}

impl Db {
    pub fn open(log: Logger, path: &str) -> Result<Db, Error> {
        let schema = include_str!("./schema.sql");
        let log = log.new(o!(
            "component" => "BootstoreDb"
        ));
        info!(log, "opening database {:?}", path);
        let mut c = SqliteConnection::establish(path)
            .map_err(|err| Error::DbOpen { path: path.into(), err })?;

        // Enable foreign key processing, which is off by default. Without
        // enabling this, there is no referential integrity check between
        // primary and foreign keys in tables.
        diesel::sql_query("PRAGMA foreign_keys = 'ON'").execute(&mut c)?;

        // Enable the WAL.
        diesel::sql_query("PRAGMA journal_mode = 'WAL'").execute(&mut c)?;

        // Force overwriting with 0s on delete
        diesel::sql_query("PRAGMA secure_delete = 'ON'").execute(&mut c)?;

        // Sync to disk after every commit.
        // DO NOT CHANGE THIS SETTING!
        diesel::sql_query("PRAGMA synchronous = 'FULL'").execute(&mut c)?;

        // Create tables
        c.batch_execute(&schema)?;

        Ok(Db { log, conn: c })
    }

    pub fn prepare_share(
        &mut self,
        epoch: i32,
        share: SerializableShareDistribution,
    ) -> Result<(), Error> {
        use schema::key_share_prepares::dsl;
        let prepare = KeySharePrepare { epoch, share: Share(share) };
        diesel::insert_into(dsl::key_share_prepares)
            .values(&prepare)
            .execute(&mut self.conn)?;
        Ok(())
    }

    pub fn commit_share(
        &mut self,
        epoch: i32,
        digest: sprockets_common::Sha3_256Digest,
    ) -> Result<(), Error> {
        use schema::key_share_commits::dsl;
        let commit = KeyShareCommit { epoch, share_digest: digest.into() };
        diesel::insert_into(dsl::key_share_commits)
            .values(&commit)
            .execute(&mut self.conn)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust_quorum::{RackSecret, ShareDistribution};
    use omicron_test_utils::dev::test_setup_log;
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};

    // TODO: Fill in with actual member certs
    fn new_shares() -> Vec<ShareDistribution> {
        let member_device_id_certs = vec![];
        let rack_secret_threshold = 3;
        let total_shares = 5;
        let secret = RackSecret::new();
        let (shares, verifier) =
            secret.split(rack_secret_threshold, total_shares).unwrap();

        shares
            .into_iter()
            .map(move |share| ShareDistribution {
                threshold: rack_secret_threshold,
                verifier: verifier.clone(),
                share,
                member_device_id_certs: member_device_id_certs.clone(),
            })
            .collect()
    }

    fn rand_db_name() -> String {
        let seed: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        format!("/tmp/testdb-{}.sqlite", seed)
    }

    #[test]
    fn simple_prepare_insert_and_query() {
        use schema::key_share_prepares::dsl;
        let log = test_setup_log("test_db").log.clone();
        let mut db = Db::open(log, &rand_db_name()).unwrap();
        let shares = new_shares();
        let epoch = 0;
        let expected: SerializableShareDistribution = shares[0].clone().into();
        db.prepare_share(epoch, expected.clone()).unwrap();
        let val = dsl::key_share_prepares
            .select(dsl::share)
            .filter(dsl::epoch.eq(epoch))
            .get_result::<Share>(&mut db.conn)
            .unwrap();
        assert_eq!(val.0, expected);
    }

    #[test]
    fn commit_fails_without_corresponding_prepare() {
        let log = test_setup_log("test_db").log.clone();
        let mut db = Db::open(log, &rand_db_name()).unwrap();
        let epoch = 0;

        let digest = sprockets_common::Sha3_256Digest::default();
        let err = db.commit_share(epoch, digest).unwrap_err();
        assert!(matches!(
            err,
            Error::Db(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::ForeignKeyViolation,
                _
            )),
        ));
    }
}
