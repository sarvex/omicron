// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Facilities for looking up API resources from the database

use super::datastore::DataStore;
use super::identity::Resource;
use super::model;
use crate::{
    authz,
    context::OpContext,
    db,
    db::error::{public_error_from_diesel_pool, ErrorHandler},
    db::model::Name,
};
use async_bb8_diesel::AsyncRunQueryDsl;
use diesel::{ExpressionMethods, QueryDsl, SelectableHelper};
use futures::future::BoxFuture;
use futures::FutureExt;
use omicron_common::api::external::{LookupResult, LookupType, ResourceType};
use uuid::Uuid;

pub trait Fetch {
    type FetchType;
    fn fetch(&self) -> BoxFuture<'_, LookupResult<Self::FetchType>>;
}

trait Lookup {
    type LookupType;
    fn lookup(
        &self,
        lookup: &LookupPath,
    ) -> BoxFuture<'_, LookupResult<Self::LookupType>>;
}

trait GetLookupRoot {
    fn lookup_root(&self) -> &LookupPath<'_>;
}

enum Key<'a, P> {
    Name(P, &'a Name),
    Id(LookupPath<'a>, Uuid),
}

impl<'a, T> GetLookupRoot for Key<'a, T>
where
    T: GetLookupRoot,
{
    fn lookup_root(&self) -> &LookupPath<'_> {
        match self {
            Key::Name(parent, _) => parent.lookup_root(),
            Key::Id(lookup, _) => lookup,
        }
    }
}

impl<'a, P> Key<'a, P> {
    fn lookup_type(&self) -> LookupType {
        match self {
            Key::Name(_, name) => LookupType::ByName(name.as_str().to_string()),
            Key::Id(_, id) => LookupType::ById(*id),
        }
    }
}

pub struct LookupPath<'a> {
    opctx: &'a OpContext,
    datastore: &'a DataStore,
}

impl<'a> LookupPath<'a> {
    pub fn new<'b, 'c>(
        opctx: &'b OpContext,
        datastore: &'c DataStore,
    ) -> LookupPath<'a>
    where
        'b: 'a,
        'c: 'a,
    {
        LookupPath { opctx, datastore }
    }

    pub fn organization_name<'b, 'c>(self, name: &'b Name) -> Organization<'c>
    where
        'a: 'c,
        'b: 'c,
    {
        Organization { key: Key::Name(self, name) }
    }

    pub fn organization_id(self, id: Uuid) -> Organization<'a> {
        Organization { key: Key::Id(self, id) }
    }

    pub fn project_id(self, id: Uuid) -> Project<'a> {
        Project { key: Key::Id(self, id) }
    }

    pub fn instance_id(self, id: Uuid) -> Instance<'a> {
        Instance { key: Key::Id(self, id) }
    }
}

impl<'a> GetLookupRoot for LookupPath<'a> {
    fn lookup_root(&self) -> &LookupPath<'_> {
        self
    }
}

pub struct Organization<'a> {
    key: Key<'a, LookupPath<'a>>,
}

impl<'a> Organization<'a> {
    fn project_name<'b, 'c>(self, name: &'b Name) -> Project<'c>
    where
        'a: 'c,
        'b: 'c,
    {
        Project { key: Key::Name(self, name) }
    }
}

impl<'a> GetLookupRoot for Organization<'a> {
    fn lookup_root(&self) -> &LookupPath<'_> {
        self.key.lookup_root()
    }
}

impl Fetch for Organization<'_> {
    type FetchType = (authz::Organization, model::Organization);

    fn fetch(&self) -> BoxFuture<'_, LookupResult<Self::FetchType>> {
        let lookup = self.lookup_root();
        let opctx = &lookup.opctx;
        let datastore = lookup.datastore;
        async {
            use db::schema::organization::dsl;
            let conn = datastore.pool_authorized(opctx).await?;
            // XXX-dap TODO This construction sucks.  What I kind of want is a
            // generic function that takes:
            // - a table (e.g., dsl::organization)
            // - a db model type to return
            // - some information about the LookupType we're trying to do
            // - a closure that can be used to apply filters
            //   (which would be either "name" or "id")
            // and does the whole thing.
            let db_org = match self.key {
                Key::Name(_, name) => dsl::organization
                    .filter(dsl::time_deleted.is_null())
                    .filter(dsl::name.eq(name.clone()))
                    .select(model::Organization::as_select())
                    .get_result_async(conn)
                    .await
                    .map_err(|e| {
                        public_error_from_diesel_pool(
                            e,
                            ErrorHandler::NotFoundByLookup(
                                ResourceType::Organization,
                                self.key.lookup_type(),
                            ),
                        )
                    }),
                Key::Id(_, id) => dsl::organization
                    .filter(dsl::time_deleted.is_null())
                    .filter(dsl::id.eq(id))
                    .select(model::Organization::as_select())
                    .get_result_async(conn)
                    .await
                    .map_err(|e| {
                        public_error_from_diesel_pool(
                            e,
                            ErrorHandler::NotFoundByLookup(
                                ResourceType::Organization,
                                self.key.lookup_type(),
                            ),
                        )
                    }),
            }?;

            let authz_org =
                authz::FLEET.organization(db_org.id(), self.key.lookup_type());
            opctx.authorize(authz::Action::Read, &authz_org).await?;
            Ok((authz_org, db_org))
        }
        .boxed()
    }
}

pub struct Project<'a> {
    key: Key<'a, Organization<'a>>,
}

impl<'a> GetLookupRoot for Project<'a> {
    fn lookup_root(&self) -> &LookupPath<'_> {
        self.key.lookup_root()
    }
}

impl Fetch for Project<'_> {
    type FetchType = (authz::Organization, authz::Project, model::Project);

    fn fetch(&self) -> BoxFuture<'_, LookupResult<Self::FetchType>> {
        todo!()
    }
}

impl<'a> Project<'a> {
    fn instance_name<'b, 'c>(self, name: &'b Name) -> Instance<'c>
    where
        'a: 'c,
        'b: 'c,
    {
        Instance { key: Key::Name(self, name) }
    }
}

impl Fetch for Instance<'_> {
    type FetchType =
        (authz::Organization, authz::Project, authz::Instance, model::Instance);

    fn fetch(&self) -> BoxFuture<'_, LookupResult<Self::FetchType>> {
        todo!()
    }
}

pub struct Instance<'a> {
    key: Key<'a, Project<'a>>,
}

impl<'a> GetLookupRoot for Instance<'a> {
    fn lookup_root(&self) -> &LookupPath<'_> {
        self.key.lookup_root()
    }
}

#[cfg(test)]
mod test {
    use super::Instance;
    use super::Key;
    use super::LookupPath;
    use super::Organization;
    use super::Project;
    use crate::context::OpContext;
    use crate::db::model::Name;
    use nexus_test_utils::db::test_setup_database;
    use omicron_test_utils::dev;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_lookup() {
        let logctx = dev::test_setup_log("test_lookup");
        let mut db = test_setup_database(&logctx.log).await;
        let (_, datastore) =
            crate::db::datastore::datastore_test(&logctx, &db).await;
        let opctx =
            OpContext::for_tests(logctx.log.new(o!()), Arc::clone(&datastore));
        let org_name: Name = Name("my-org".parse().unwrap());
        let project_name: Name = Name("my-project".parse().unwrap());
        let instance_name: Name = Name("my-instance".parse().unwrap());

        let leaf = LookupPath::new(&opctx, &datastore)
            .organization_name(&org_name)
            .project_name(&project_name)
            .instance_name(&instance_name);
        assert!(matches!(&leaf,
            Instance {
                key: Key::Name(Project {
                    key: Key::Name(Organization {
                        key: Key::Name(_, o)
                    }, p)
                }, i)
            }
            if **o == org_name && **p == project_name && **i == instance_name));

        let org_id = "006f29d9-0ff0-e2d2-a022-87e152440122".parse().unwrap();
        let leaf = LookupPath::new(&opctx, &datastore)
            .organization_id(org_id)
            .project_name(&project_name);
        assert!(matches!(&leaf, Project {
            key: Key::Name(Organization {
                key: Key::Id(LookupPath { .. }, o)
            }, p)
        } if *o == org_id && **p == project_name));

        db.cleanup().await.unwrap();
    }
}
