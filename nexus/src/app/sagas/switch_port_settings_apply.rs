// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::{NexusActionContext, NEXUS_DPD_TAG};
use crate::app::sagas::{
    declare_saga_actions, ActionRegistry, NexusSaga, SagaInitError,
};
use crate::db::datastore::UpdatePrecondition;
use crate::{authn, db};
use anyhow::Error;
use db::datastore::SwitchPortSettingsCombinedResult;
use dpd_client::types::{LinkId, PortId, PortSettings};
use ipnetwork::IpNetwork;
use omicron_common::api::external::{self, NameOrId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use steno::ActionError;
use uuid::Uuid;

// switch port settings apply saga: input parameters

#[derive(Debug, Deserialize, Serialize)]
pub struct Params {
    pub serialized_authn: authn::saga::Serialized,
    pub switch_port_id: Uuid,
    pub switch_port_settings_id: Uuid,
    pub switch_port_name: String,
}

// switch port settings apply: actions

declare_saga_actions! {
    switch_port_settings_apply;
    ASSOCIATE_SWITCH_PORT -> "original_switch_port_settings_id" {
        + spa_associate_switch_port
        - spa_disassociate_switch_port
    }
    GET_SWITCH_PORT_SETTINGS -> "switch_port_settings" {
        + spa_get_switch_port_settings
    }
    ENSURE_SWITCH_PORT_SETTINGS -> "ensure_switch_port_settings" {
        + spa_ensure_switch_port_settings
        - spa_undo_ensure_switch_port_settings
    }
}

// switch port settings apply saga: definition

#[derive(Debug)]
pub struct SagaSwitchPortSettingsApply;

impl NexusSaga for SagaSwitchPortSettingsApply {
    const NAME: &'static str = "switch-port-settings-apply";
    type Params = Params;

    fn register_actions(registry: &mut ActionRegistry) {
        switch_port_settings_apply_register_actions(registry);
    }

    fn make_saga_dag(
        _params: &Self::Params,
        mut builder: steno::DagBuilder,
    ) -> Result<steno::Dag, SagaInitError> {
        builder.append(associate_switch_port_action());
        builder.append(get_switch_port_settings_action());
        builder.append(ensure_switch_port_settings_action());
        Ok(builder.build()?)
    }
}

async fn spa_associate_switch_port(
    sagactx: NexusActionContext,
) -> Result<Option<Uuid>, ActionError> {
    let osagactx = sagactx.user_data();
    let params = sagactx.saga_params::<Params>()?;
    let nexus = osagactx.nexus();

    let opctx = crate::context::op_context_for_saga_action(
        &sagactx,
        &params.serialized_authn,
    );

    // first get the current association so we fall back to this on failure
    let port = nexus
        .get_switch_port(&opctx, params.switch_port_id)
        .await
        .map_err(ActionError::action_failed)?;

    // update the switch port settings association
    nexus
        .set_switch_port_settings_id(
            &opctx,
            params.switch_port_id,
            Some(params.switch_port_settings_id),
            UpdatePrecondition::DontCare,
        )
        .await
        .map_err(ActionError::action_failed)?;

    Ok(port.port_settings_id)
}

async fn spa_get_switch_port_settings(
    sagactx: NexusActionContext,
) -> Result<SwitchPortSettingsCombinedResult, ActionError> {
    let osagactx = sagactx.user_data();
    let params = sagactx.saga_params::<Params>()?;
    let nexus = osagactx.nexus();
    let opctx = crate::context::op_context_for_saga_action(
        &sagactx,
        &params.serialized_authn,
    );

    let port_settings = nexus
        .switch_port_settings_get(
            &opctx,
            &NameOrId::Id(params.switch_port_settings_id),
        )
        .await
        .map_err(ActionError::action_failed)?;

    Ok(port_settings)
}

pub(crate) fn api_to_dpd_port_settings(
    port_id: &PortId,
    settings: &SwitchPortSettingsCombinedResult,
) -> PortSettings {
    let mut dpd_port_settings = PortSettings {
        addrs: HashMap::new(),
        routes: HashMap::new(),
        tag: NEXUS_DPD_TAG.into(),
    };

    //TODO breakouts
    let link_id = LinkId(0);

    let addrs: Vec<IpAddr> =
        settings.addresses.iter().map(|a| a.address.ip()).collect();
    dpd_port_settings.addrs.insert(link_id.to_string(), addrs);

    let mut routes: Vec<dpd_client::types::Route> = Vec::new();
    for r in &settings.routes {
        match &r.dst {
            IpNetwork::V4(n) => {
                routes.push(dpd_client::types::Route {
                    cidr: dpd_client::Ipv4Cidr {
                        prefix: n.ip(),
                        prefix_len: n.prefix(),
                    }
                    .into(),
                    link: link_id.clone(),
                    switch_port: port_id.clone(),
                    nexthop: Some(r.gw.ip()),
                });
            }
            IpNetwork::V6(n) => {
                routes.push(dpd_client::types::Route {
                    cidr: dpd_client::Ipv6Cidr {
                        prefix: n.ip(),
                        prefix_len: n.prefix(),
                    }
                    .into(),
                    link: link_id.clone(),
                    switch_port: port_id.clone(),
                    nexthop: Some(r.gw.ip()),
                });
            }
        }
    }

    dpd_port_settings.routes.insert(link_id.to_string(), routes);
    dpd_port_settings
}

async fn spa_ensure_switch_port_settings(
    sagactx: NexusActionContext,
) -> Result<(), ActionError> {
    let osagactx = sagactx.user_data();
    let params = sagactx.saga_params::<Params>()?;

    let settings = sagactx
        .lookup::<SwitchPortSettingsCombinedResult>("switch_port_settings")?;

    let port_id: PortId = PortId::from_str(&params.switch_port_name)
        .map_err(|e| ActionError::action_failed(e.to_string()))?;

    let dpd_client: Arc<dpd_client::Client> =
        Arc::clone(&osagactx.nexus().dpd_client);

    let dpd_port_settings = api_to_dpd_port_settings(&port_id, &settings);

    dpd_client
        .port_settings_apply(&port_id, &dpd_port_settings)
        .await
        .map_err(|e| ActionError::action_failed(e.to_string()))?;

    Ok(())
}

async fn spa_undo_ensure_switch_port_settings(
    sagactx: NexusActionContext,
) -> Result<(), Error> {
    let osagactx = sagactx.user_data();
    let params = sagactx.saga_params::<Params>()?;
    let nexus = osagactx.nexus();
    let opctx = crate::context::op_context_for_saga_action(
        &sagactx,
        &params.serialized_authn,
    );

    let port_id: PortId = PortId::from_str(&params.switch_port_name)
        .map_err(|e| external::Error::internal_error(e))?;

    let orig_port_settings_id = sagactx
        .lookup::<Option<Uuid>>("original_switch_port_settings_id")
        .map_err(|e| external::Error::internal_error(&e.to_string()))?;

    let dpd_client: Arc<dpd_client::Client> =
        Arc::clone(&osagactx.nexus().dpd_client);

    let id = match orig_port_settings_id {
        Some(id) => id,
        None => {
            dpd_client
                .port_settings_clear(&port_id)
                .await
                .map_err(|e| external::Error::internal_error(&e.to_string()))?;

            return Ok(());
        }
    };

    let settings = nexus
        .switch_port_settings_get(&opctx, &NameOrId::Id(id))
        .await
        .map_err(ActionError::action_failed)?;

    let dpd_port_settings = api_to_dpd_port_settings(&port_id, &settings);

    dpd_client
        .port_settings_apply(&port_id, &dpd_port_settings)
        .await
        .map_err(|e| external::Error::internal_error(&e.to_string()))?;

    Ok(())
}

// a common route representation for dendrite and port settings
#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub(crate) struct Route {
    pub dst: IpAddr,
    pub masklen: u8,
    pub nexthop: Option<IpAddr>,
}

async fn spa_disassociate_switch_port(
    sagactx: NexusActionContext,
) -> Result<(), Error> {
    let osagactx = sagactx.user_data();
    let params = sagactx.saga_params::<Params>()?;
    let nexus = osagactx.nexus();

    let opctx = crate::context::op_context_for_saga_action(
        &sagactx,
        &params.serialized_authn,
    );

    // set the port settings id back to what it was before the saga started
    let orig_port_settings_id =
        sagactx.lookup::<Option<Uuid>>("original_switch_port_settings_id")?;

    nexus
        .set_switch_port_settings_id(
            &opctx,
            params.switch_port_id,
            orig_port_settings_id,
            UpdatePrecondition::Value(params.switch_port_settings_id),
        )
        .await
        .map_err(ActionError::action_failed)?;

    Ok(())
}
