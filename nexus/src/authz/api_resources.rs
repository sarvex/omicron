// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authz types for resources in the API hierarchy
//!
//! The general pattern in Nexus for working with an object is to look it up
//! (see [`crate::db::lookup::LookupPath`]) and get back a so-called `authz`
//! type.  This type uniquely identifies the resource regardless of any other
//! changes (e.g., name change or moving it to a different parent collection).
//! The various datastore functions that modify API resources accept these
//! `authz` types.
//!
//! The `authz` types can be passed to
//! [`crate::context::OpContext::authorize()`] to do an authorization check --
//! is the caller allowed to perform some action on the resource?  This is the
//! primary way of doing authz checks in Nexus.
//!
//! `authz` types also retain information about how the resource was looked-up
//! in the first place so that if it turns out the caller is not even allowed to
//! know if the resource exists, we can produce an appropriate 404 error.  For
//! example, if they look up organization "foo", and we get back one with id
//! 123, but they're not allowed to see it, then the user should get back a 404
//! that organization "foo" doesn't exist (and definitely not that organization
//! 123 doesn't exist, since that would tell the user that it _does_ exist!).
//!
//! Most `authz` types are generated by the `authz_resource!` macro.

use super::actor::AnyActor;
use super::context::AuthorizedResource;
use super::oso_generic::Init;
use super::roles::{
    load_roles_for_resource, load_roles_for_resource_tree, RoleSet,
};
use super::Action;
use super::{actor::AuthenticatedActor, Authz};
use crate::authn;
use crate::context::OpContext;
use crate::db::fixed_data::FLEET_ID;
use crate::db::model::UpdateArtifactKind;
use crate::db::DataStore;
use authz_macros::authz_resource;
use futures::future::BoxFuture;
use futures::FutureExt;
use omicron_common::api::external::{Error, LookupType, ResourceType};
use uuid::Uuid;

/// Describes an authz resource that corresponds to an API resource that has a
/// corresponding ResourceType and is stored in the database
pub trait ApiResource: Clone + Send + Sync + 'static {
    /// If roles can be assigned to this resource, return the type and id of the
    /// database record describing this resource
    ///
    /// If roles cannot be assigned to this resource, returns `None`.
    fn db_resource(&self) -> Option<(ResourceType, Uuid)>;

    /// If this resource has a parent in the API hierarchy whose assigned roles
    /// can affect access to this resource, return the parent resource.
    /// Otherwise, returns `None`.
    fn parent(&self) -> Option<&dyn AuthorizedResource>;
}

/// Practically, all objects which implement [`ApiResourceError`]
/// also implement [`ApiResource`]. However, [`ApiResource`] is not object
/// safe because it implements [`std::clone::Clone`].
///
/// This allows callers to use [`ApiResourceError`] as a trait object.
pub trait ApiResourceError {
    /// Returns an error as though this resource were not found, suitable for
    /// use when an actor should not be able to see that this resource exists
    fn not_found(&self) -> Error {
        self.lookup_type().clone().into_not_found(self.resource_type())
    }

    fn resource_type(&self) -> ResourceType;
    fn lookup_type(&self) -> &LookupType;
}

impl<T: ApiResource + ApiResourceError + oso::PolarClass> AuthorizedResource
    for T
{
    fn load_roles<'a, 'b, 'c, 'd, 'e, 'f>(
        &'a self,
        opctx: &'b OpContext,
        datastore: &'c DataStore,
        authn: &'d authn::Context,
        roleset: &'e mut RoleSet,
    ) -> BoxFuture<'f, Result<(), Error>>
    where
        'a: 'f,
        'b: 'f,
        'c: 'f,
        'd: 'f,
        'e: 'f,
    {
        load_roles_for_resource_tree(self, opctx, datastore, authn, roleset)
            .boxed()
    }

    fn on_unauthorized(
        &self,
        authz: &Authz,
        error: Error,
        actor: AnyActor,
        action: Action,
    ) -> Error {
        if action == Action::Read {
            return self.not_found();
        }

        // If the user failed an authz check, and they can't even read this
        // resource, then we should produce a 404 rather than a 401/403.
        match authz.is_allowed(&actor, Action::Read, self) {
            Err(error) => Error::internal_error(&format!(
                "failed to compute read authorization to determine visibility: \
                {:#}",
                error
            )),
            Ok(false) => self.not_found(),
            Ok(true) => error,
        }
    }
}

/// Represents the Oxide fleet for authz purposes
///
/// Fleet-level resources are essentially global.  See RFD 24 for more on
/// Fleets.
///
/// This object is used for authorization checks on a Fleet by passing it as the
/// `resource` argument to [`crate::context::OpContext::authorize()`].  You
/// don't construct a `Fleet` yourself -- use the global [`FLEET`].
#[derive(Clone, Copy, Debug)]
pub struct Fleet;
/// Singleton representing the [`Fleet`] itself for authz purposes
pub const FLEET: Fleet = Fleet;

impl Eq for Fleet {}
impl PartialEq for Fleet {
    fn eq(&self, _: &Self) -> bool {
        // There is only one Fleet.
        true
    }
}

impl oso::PolarClass for Fleet {
    fn get_polar_class_builder() -> oso::ClassBuilder<Self> {
        oso::Class::builder().with_equality_check().add_method(
            "has_role",
            |_: &Fleet, actor: AuthenticatedActor, role: String| {
                actor.has_role_resource(ResourceType::Fleet, *FLEET_ID, &role)
            },
        )
    }
}

impl AuthorizedResource for Fleet {
    fn load_roles<'a, 'b, 'c, 'd, 'e, 'f>(
        &'a self,
        opctx: &'b OpContext,
        datastore: &'c DataStore,
        authn: &'d authn::Context,
        roleset: &'e mut RoleSet,
    ) -> futures::future::BoxFuture<'f, Result<(), Error>>
    where
        'a: 'f,
        'b: 'f,
        'c: 'f,
        'd: 'f,
        'e: 'f,
    {
        load_roles_for_resource(
            opctx,
            datastore,
            authn,
            ResourceType::Fleet,
            *FLEET_ID,
            roleset,
        )
        .boxed()
    }

    fn on_unauthorized(
        &self,
        _: &Authz,
        error: Error,
        _: AnyActor,
        _: Action,
    ) -> Error {
        error
    }
}

/// ConsoleSessionList is a synthetic resource used for modeling who has access
/// to create sessions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsoleSessionList;

pub const CONSOLE_SESSION_LIST: ConsoleSessionList = ConsoleSessionList {};

impl oso::PolarClass for ConsoleSessionList {
    fn get_polar_class_builder() -> oso::ClassBuilder<Self> {
        // Roles are not directly attached to ConsoleSessionList.
        oso::Class::builder()
            .with_equality_check()
            .add_method(
                "has_role",
                |_: &ConsoleSessionList,
                 _actor: AuthenticatedActor,
                 _role: String| false,
            )
            .add_attribute_getter("fleet", |_| FLEET)
    }
}

impl AuthorizedResource for ConsoleSessionList {
    fn load_roles<'a, 'b, 'c, 'd, 'e, 'f>(
        &'a self,
        opctx: &'b OpContext,
        datastore: &'c DataStore,
        authn: &'d authn::Context,
        roleset: &'e mut RoleSet,
    ) -> futures::future::BoxFuture<'f, Result<(), Error>>
    where
        'a: 'f,
        'b: 'f,
        'c: 'f,
        'd: 'f,
        'e: 'f,
    {
        load_roles_for_resource(
            opctx,
            datastore,
            authn,
            ResourceType::Fleet,
            *FLEET_ID,
            roleset,
        )
        .boxed()
    }

    fn on_unauthorized(
        &self,
        _: &Authz,
        error: Error,
        _: AnyActor,
        _: Action,
    ) -> Error {
        error
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GlobalImageList;
/// Singleton representing the [`GlobalImageList`] itself for authz purposes
pub const GLOBAL_IMAGE_LIST: GlobalImageList = GlobalImageList;

impl Eq for GlobalImageList {}
impl PartialEq for GlobalImageList {
    fn eq(&self, _: &Self) -> bool {
        // There is only one GlobalImageList.
        true
    }
}

impl oso::PolarClass for GlobalImageList {
    fn get_polar_class_builder() -> oso::ClassBuilder<Self> {
        oso::Class::builder()
            .with_equality_check()
            .add_attribute_getter("fleet", |_x: &GlobalImageList| FLEET)
    }
}

impl AuthorizedResource for GlobalImageList {
    fn load_roles<'a, 'b, 'c, 'd, 'e, 'f>(
        &'a self,
        opctx: &'b OpContext,
        datastore: &'c DataStore,
        authn: &'d authn::Context,
        roleset: &'e mut RoleSet,
    ) -> futures::future::BoxFuture<'f, Result<(), Error>>
    where
        'a: 'f,
        'b: 'f,
        'c: 'f,
        'd: 'f,
        'e: 'f,
    {
        // there's no roles related to GlobalImageList, just permissions but we
        // still need to load the fleet related roles to find if the actor has
        // the "admin" role on the fleet
        load_roles_for_resource(
            opctx,
            datastore,
            authn,
            ResourceType::Fleet,
            *FLEET_ID,
            roleset,
        )
        .boxed()
    }

    fn on_unauthorized(
        &self,
        _: &Authz,
        error: Error,
        _: AnyActor,
        _: Action,
    ) -> Error {
        error
    }
}

// Main resource hierarchy: Organizations, Projects, and their resources

authz_resource! {
    name = "Organization",
    parent = "Silo",
    primary_key = Uuid,
    roles_allowed = true,
    polar_snippet = Custom,
}

authz_resource! {
    name = "Project",
    parent = "Organization",
    primary_key = Uuid,
    roles_allowed = true,
    polar_snippet = Custom,
}

authz_resource! {
    name = "Disk",
    parent = "Project",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = InProject,
}

authz_resource! {
    name = "Instance",
    parent = "Project",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = InProject,
}

authz_resource! {
    name = "NetworkInterface",
    parent = "Instance",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = InProject,
}

authz_resource! {
    name = "Vpc",
    parent = "Project",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = InProject,
}

authz_resource! {
    name = "VpcRouter",
    parent = "Vpc",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = InProject,
}

authz_resource! {
    name = "RouterRoute",
    parent = "VpcRouter",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = InProject,
}

authz_resource! {
    name = "VpcSubnet",
    parent = "Vpc",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = InProject,
}

// Miscellaneous resources nested directly below "Fleet"

authz_resource! {
    name = "ConsoleSession",
    parent = "Fleet",
    primary_key = String,
    roles_allowed = false,
    polar_snippet = FleetChild,
}

authz_resource! {
    name = "SiloUser",
    parent = "Fleet",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = FleetChild,
}

authz_resource! {
    name = "RoleBuiltin",
    parent = "Fleet",
    primary_key = (String, String),
    roles_allowed = false,
    polar_snippet = FleetChild,
}

authz_resource! {
    name = "UserBuiltin",
    parent = "Fleet",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = FleetChild,
}

authz_resource! {
    name = "Rack",
    parent = "Fleet",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = FleetChild,
}

authz_resource! {
    name = "Silo",
    parent = "Fleet",
    primary_key = Uuid,
    roles_allowed = true,
    polar_snippet = Custom,
}

authz_resource! {
    name = "Sled",
    parent = "Fleet",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = FleetChild,
}

authz_resource! {
    name = "UpdateAvailableArtifact",
    parent = "Fleet",
    primary_key = (String, i64, UpdateArtifactKind),
    roles_allowed = false,
    polar_snippet = FleetChild,
}

authz_resource! {
    name = "GlobalImage",
    parent = "Fleet",
    primary_key = Uuid,
    roles_allowed = false,
    polar_snippet = FleetChild,
}
