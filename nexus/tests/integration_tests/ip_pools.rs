// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Integration tests for operating on IP Pools

use dropshot::test_util::ClientTestContext;
use dropshot::HttpErrorResponseBody;
use http::method::Method;
use http::StatusCode;
use nexus_test_utils::http_testing::AuthnMode;
use nexus_test_utils::http_testing::NexusRequest;
use nexus_test_utils::http_testing::RequestBuilder;
use nexus_test_utils::resource_helpers::objects_list_page_authz;
use nexus_test_utils::ControlPlaneTestContext;
use nexus_test_utils_macros::nexus_test;
use omicron_common::api::external::IdentityMetadataCreateParams;
use omicron_common::api::external::IdentityMetadataUpdateParams;
use omicron_nexus::external_api::params::IpPoolCreate;
use omicron_nexus::external_api::params::IpPoolUpdate;
use omicron_nexus::external_api::shared::IpRange;
use omicron_nexus::external_api::shared::Ipv4Range;
use omicron_nexus::external_api::shared::Ipv6Range;
use omicron_nexus::external_api::views::IpPool;
use omicron_nexus::external_api::views::IpPoolRange;

// Basic test verifying CRUD behavior on the IP Pool itself.
#[nexus_test]
async fn test_ip_pool_basic_crud(cptestctx: &ControlPlaneTestContext) {
    let client = &cptestctx.external_client;
    let ip_pools_url = "/ip-pools";
    let pool_name = "p0";
    let description = "an ip pool";
    let ip_pool_url = format!("{}/{}", ip_pools_url, pool_name);
    let ip_pool_ranges_url = format!("{}/ranges", ip_pool_url);
    let ip_pool_add_range_url = format!("{}/add", ip_pool_ranges_url);

    // Verify the list of IP pools is empty
    assert_eq!(
        NexusRequest::iter_collection_authn::<IpPool>(
            client,
            ip_pools_url,
            "",
            None
        )
        .await
        .expect("Failed to list IP Pools")
        .all_items
        .len(),
        0,
        "Expected a list of zero IP pools"
    );

    // Verify 404 if the pool doesn't exist yet, both for creating or deleting
    let error: HttpErrorResponseBody = NexusRequest::expect_failure(
        client,
        StatusCode::NOT_FOUND,
        Method::GET,
        &ip_pool_url,
    )
    .authn_as(AuthnMode::PrivilegedUser)
    .execute()
    .await
    .unwrap()
    .parsed_body()
    .unwrap();
    assert_eq!(
        error.message,
        format!("not found: ip-pool with name \"{}\"", pool_name),
    );
    let error: HttpErrorResponseBody = NexusRequest::expect_failure(
        client,
        StatusCode::NOT_FOUND,
        Method::DELETE,
        &ip_pool_url,
    )
    .authn_as(AuthnMode::PrivilegedUser)
    .execute()
    .await
    .unwrap()
    .parsed_body()
    .unwrap();
    assert_eq!(
        error.message,
        format!("not found: ip-pool with name \"{}\"", pool_name),
    );

    // Create the pool, verify we can get it back by either listing or fetching
    // directly
    let params = IpPoolCreate {
        identity: IdentityMetadataCreateParams {
            name: String::from(pool_name).parse().unwrap(),
            description: String::from(description),
        },
    };
    let created_pool: IpPool =
        NexusRequest::objects_post(client, ip_pools_url, &params)
            .authn_as(AuthnMode::PrivilegedUser)
            .execute()
            .await
            .unwrap()
            .parsed_body()
            .unwrap();
    assert_eq!(created_pool.identity.name, pool_name);
    assert_eq!(created_pool.identity.description, description);

    let list = NexusRequest::iter_collection_authn::<IpPool>(
        client,
        ip_pools_url,
        "",
        None,
    )
    .await
    .expect("Failed to list IP Pools")
    .all_items;
    assert_eq!(list.len(), 1, "Expected exactly one IP pool");
    assert_pools_eq(&created_pool, &list[0]);

    let fetched_pool: IpPool = NexusRequest::object_get(client, &ip_pool_url)
        .authn_as(AuthnMode::PrivilegedUser)
        .execute()
        .await
        .unwrap()
        .parsed_body()
        .unwrap();
    assert_pools_eq(&created_pool, &fetched_pool);

    // Verify we get a conflict error if we insert it again
    let error: HttpErrorResponseBody = NexusRequest::new(
        RequestBuilder::new(client, Method::POST, ip_pools_url)
            .body(Some(&params))
            .expect_status(Some(StatusCode::BAD_REQUEST)),
    )
    .authn_as(AuthnMode::PrivilegedUser)
    .execute()
    .await
    .unwrap()
    .parsed_body()
    .unwrap();
    assert_eq!(
        error.message,
        format!("already exists: ip-pool \"{}\"", pool_name)
    );

    // Add a range, verify that we can't delete the Pool
    let range = IpRange::V4(
        Ipv4Range::new(
            std::net::Ipv4Addr::new(10, 0, 0, 1),
            std::net::Ipv4Addr::new(10, 0, 0, 5),
        )
        .unwrap(),
    );
    let created_range: IpPoolRange =
        NexusRequest::objects_post(client, &ip_pool_add_range_url, &range)
            .authn_as(AuthnMode::PrivilegedUser)
            .execute()
            .await
            .unwrap()
            .parsed_body()
            .unwrap();
    assert_eq!(range.first_address(), created_range.range.first_address());
    assert_eq!(range.last_address(), created_range.range.last_address());
    let error: HttpErrorResponseBody = NexusRequest::expect_failure(
        client,
        StatusCode::BAD_REQUEST,
        Method::DELETE,
        &ip_pool_url,
    )
    .authn_as(AuthnMode::PrivilegedUser)
    .execute()
    .await
    .unwrap()
    .parsed_body()
    .unwrap();
    assert_eq!(
        error.message,
        "IP Pool cannot be deleted while it contains IP ranges",
    );

    // Rename the pool.
    //
    // Ensure we can fetch the pool under the new name, and not the old, and
    // that the modification time has changed.
    let new_pool_name = "p1";
    let new_ip_pool_url = format!("{}/{}", ip_pools_url, new_pool_name);
    let new_ip_pool_del_range_url =
        format!("{}/ranges/delete", new_ip_pool_url);
    let updates = IpPoolUpdate {
        identity: IdentityMetadataUpdateParams {
            name: Some(String::from(new_pool_name).parse().unwrap()),
            description: None,
        },
    };
    let modified_pool: IpPool =
        NexusRequest::object_put(client, &ip_pool_url, Some(&updates))
            .authn_as(AuthnMode::PrivilegedUser)
            .execute()
            .await
            .unwrap()
            .parsed_body()
            .unwrap();
    assert_eq!(modified_pool.identity.name, new_pool_name);
    assert_eq!(modified_pool.identity.id, created_pool.identity.id);
    assert_eq!(
        modified_pool.identity.description,
        created_pool.identity.description
    );
    assert_eq!(
        modified_pool.identity.time_created,
        created_pool.identity.time_created
    );
    assert!(
        modified_pool.identity.time_modified
            > created_pool.identity.time_modified
    );

    let fetched_modified_pool: IpPool =
        NexusRequest::object_get(client, &new_ip_pool_url)
            .authn_as(AuthnMode::PrivilegedUser)
            .execute()
            .await
            .unwrap()
            .parsed_body()
            .unwrap();
    assert_pools_eq(&modified_pool, &fetched_modified_pool);

    let error: HttpErrorResponseBody = NexusRequest::expect_failure(
        client,
        StatusCode::NOT_FOUND,
        Method::GET,
        &ip_pool_url,
    )
    .authn_as(AuthnMode::PrivilegedUser)
    .execute()
    .await
    .unwrap()
    .parsed_body()
    .unwrap();
    assert_eq!(
        error.message,
        format!("not found: ip-pool with name \"{}\"", pool_name),
    );

    // Delete the range, then verify we can delete the pool and everything looks
    // gravy.
    NexusRequest::new(
        RequestBuilder::new(client, Method::POST, &new_ip_pool_del_range_url)
            .body(Some(&range))
            .expect_status(Some(StatusCode::NO_CONTENT)),
    )
    .authn_as(AuthnMode::PrivilegedUser)
    .execute()
    .await
    .expect("Failed to delete IP range from a pool");
    NexusRequest::object_delete(client, &new_ip_pool_url)
        .authn_as(AuthnMode::PrivilegedUser)
        .execute()
        .await
        .expect("Expected to be able to delete an empty IP Pool");
}

// Data for testing overlapping IP ranges
struct TestRange {
    // A starting IP range that should be inserted correctly
    base_range: IpRange,
    // Ranges that should fail for various reasons of overlap
    bad_ranges: Vec<IpRange>,
}

// Integration test verifying the uniqueness of IP ranges when inserted /
// deleted across multiple pools
#[nexus_test]
async fn test_ip_pool_range_overlapping_ranges_fails(
    cptestctx: &ControlPlaneTestContext,
) {
    let client = &cptestctx.external_client;
    let ip_pools_url = "/ip-pools";
    let pool_name = "p0";
    let description = "an ip pool";
    let ip_pool_url = format!("{}/{}", ip_pools_url, pool_name);
    let ip_pool_ranges_url = format!("{}/ranges", ip_pool_url);
    let ip_pool_add_range_url = format!("{}/add", ip_pool_ranges_url);

    // Create the pool, verify basic properties
    let params = IpPoolCreate {
        identity: IdentityMetadataCreateParams {
            name: String::from(pool_name).parse().unwrap(),
            description: String::from(description),
        },
    };
    let created_pool: IpPool =
        NexusRequest::objects_post(client, ip_pools_url, &params)
            .authn_as(AuthnMode::PrivilegedUser)
            .execute()
            .await
            .unwrap()
            .parsed_body()
            .unwrap();
    assert_eq!(created_pool.identity.name, pool_name);
    assert_eq!(created_pool.identity.description, description);

    // Test data for IPv4 ranges that should fail due to overlap
    let ipv4_range = TestRange {
        base_range: IpRange::V4(
            Ipv4Range::new(
                std::net::Ipv4Addr::new(10, 0, 0, 2),
                std::net::Ipv4Addr::new(10, 0, 0, 5),
            )
            .unwrap(),
        ),
        bad_ranges: vec![
            // The exact same range
            IpRange::V4(
                Ipv4Range::new(
                    std::net::Ipv4Addr::new(10, 0, 0, 2),
                    std::net::Ipv4Addr::new(10, 0, 0, 5),
                )
                .unwrap(),
            ),
            // Overlaps below
            IpRange::V4(
                Ipv4Range::new(
                    std::net::Ipv4Addr::new(10, 0, 0, 1),
                    std::net::Ipv4Addr::new(10, 0, 0, 2),
                )
                .unwrap(),
            ),
            // Overlaps above
            IpRange::V4(
                Ipv4Range::new(
                    std::net::Ipv4Addr::new(10, 0, 0, 5),
                    std::net::Ipv4Addr::new(10, 0, 0, 6),
                )
                .unwrap(),
            ),
            // Contains the base range
            IpRange::V4(
                Ipv4Range::new(
                    std::net::Ipv4Addr::new(10, 0, 0, 1),
                    std::net::Ipv4Addr::new(10, 0, 0, 6),
                )
                .unwrap(),
            ),
            // Contained by the base range
            IpRange::V4(
                Ipv4Range::new(
                    std::net::Ipv4Addr::new(10, 0, 0, 3),
                    std::net::Ipv4Addr::new(10, 0, 0, 4),
                )
                .unwrap(),
            ),
        ],
    };
    test_bad_ip_ranges(client, &ip_pool_add_range_url, &ipv4_range).await;

    // Test data for IPv6 ranges that should fail due to overlap
    let ipv6_range = TestRange {
        base_range: IpRange::V6(
            Ipv6Range::new(
                std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 10),
                std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 20),
            )
            .unwrap(),
        ),
        bad_ranges: vec![
            // The exact same range
            IpRange::V6(
                Ipv6Range::new(
                    std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 10),
                    std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 20),
                )
                .unwrap(),
            ),
            // Overlaps below
            IpRange::V6(
                Ipv6Range::new(
                    std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 5),
                    std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 15),
                )
                .unwrap(),
            ),
            // Overlaps above
            IpRange::V6(
                Ipv6Range::new(
                    std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 15),
                    std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 25),
                )
                .unwrap(),
            ),
            // Contains the base range
            IpRange::V6(
                Ipv6Range::new(
                    std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 0),
                    std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 100),
                )
                .unwrap(),
            ),
            // Contained by the base range
            IpRange::V6(
                Ipv6Range::new(
                    std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 12),
                    std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 13),
                )
                .unwrap(),
            ),
        ],
    };
    test_bad_ip_ranges(client, &ip_pool_add_range_url, &ipv6_range).await;
}

async fn test_bad_ip_ranges(
    client: &ClientTestContext,
    url: &str,
    ranges: &TestRange,
) {
    let created_range: IpPoolRange =
        NexusRequest::objects_post(client, url, &ranges.base_range)
            .authn_as(AuthnMode::PrivilegedUser)
            .execute()
            .await
            .unwrap()
            .parsed_body()
            .unwrap();
    assert_eq!(
        ranges.base_range.first_address(),
        created_range.range.first_address()
    );
    assert_eq!(
        ranges.base_range.last_address(),
        created_range.range.last_address()
    );

    // Everything else should fail
    for bad_range in ranges.bad_ranges.iter() {
        let error: HttpErrorResponseBody = NexusRequest::new(
            RequestBuilder::new(client, Method::POST, url)
                .body(Some(bad_range))
                .expect_status(Some(StatusCode::BAD_REQUEST)),
        )
        .authn_as(AuthnMode::PrivilegedUser)
        .execute()
        .await
        .unwrap()
        .parsed_body()
        .unwrap();
        let expected_message = format!(
            "The provided IP range {}-{} overlaps with an existing range",
            bad_range.first_address(),
            bad_range.last_address(),
        );
        assert_eq!(error.message, expected_message);
    }
}

#[nexus_test]
async fn test_ip_pool_range_pagination(cptestctx: &ControlPlaneTestContext) {
    let client = &cptestctx.external_client;
    let ip_pools_url = "/ip-pools";
    let pool_name = "p0";
    let description = "an ip pool";
    let ip_pool_url = format!("{}/{}", ip_pools_url, pool_name);
    let ip_pool_ranges_url = format!("{}/ranges", ip_pool_url);
    let ip_pool_add_range_url = format!("{}/add", ip_pool_ranges_url);

    // Create the pool, verify basic properties
    let params = IpPoolCreate {
        identity: IdentityMetadataCreateParams {
            name: String::from(pool_name).parse().unwrap(),
            description: String::from(description),
        },
    };
    let created_pool: IpPool =
        NexusRequest::objects_post(client, ip_pools_url, &params)
            .authn_as(AuthnMode::PrivilegedUser)
            .execute()
            .await
            .unwrap()
            .parsed_body()
            .unwrap();
    assert_eq!(created_pool.identity.name, pool_name);
    assert_eq!(created_pool.identity.description, description);

    // Add some ranges, out of order. These will be paginated by their first
    // address, which sorts all IPv4 before IPv6, then within protocol versions
    // by their first address.
    let ranges = [
        IpRange::V6(
            Ipv6Range::new(
                std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 11),
                std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 20),
            )
            .unwrap(),
        ),
        IpRange::V6(
            Ipv6Range::new(
                std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 0),
                std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 10),
            )
            .unwrap(),
        ),
        IpRange::V4(
            Ipv4Range::new(
                std::net::Ipv4Addr::new(10, 0, 0, 1),
                std::net::Ipv4Addr::new(10, 0, 0, 2),
            )
            .unwrap(),
        ),
    ];

    let mut expected_ranges = Vec::with_capacity(ranges.len());
    for range in ranges.iter() {
        let created_range: IpPoolRange =
            NexusRequest::objects_post(client, &ip_pool_add_range_url, &range)
                .authn_as(AuthnMode::PrivilegedUser)
                .execute()
                .await
                .unwrap()
                .parsed_body()
                .unwrap();
        assert_eq!(range.first_address(), created_range.range.first_address());
        assert_eq!(range.last_address(), created_range.range.last_address());
        expected_ranges.push(created_range);
    }
    expected_ranges
        .sort_by(|a, b| a.range.first_address().cmp(&b.range.first_address()));

    // List the first 2 results, then the last. These should appear sorted by
    // their first address.
    let first_page_url = format!("{}?limit=2", ip_pool_ranges_url);
    let first_page =
        objects_list_page_authz::<IpPoolRange>(client, &first_page_url).await;
    assert_eq!(first_page.items.len(), 2);

    let second_page_url = format!(
        "{}&page_token={}",
        first_page_url,
        first_page.next_page.unwrap()
    );
    let second_page =
        objects_list_page_authz::<IpPoolRange>(client, &second_page_url).await;
    assert_eq!(second_page.items.len(), 1);

    let actual_ranges = first_page.items.iter().chain(second_page.items.iter());
    for (expected_range, actual_range) in
        expected_ranges.iter().zip(actual_ranges)
    {
        assert_ranges_eq(expected_range, actual_range);
    }
}

fn assert_pools_eq(first: &IpPool, second: &IpPool) {
    assert_eq!(first.identity, second.identity);
}

fn assert_ranges_eq(first: &IpPoolRange, second: &IpPoolRange) {
    assert_eq!(first.id, second.id);
    assert_eq!(first.time_created, second.time_created);
    assert_eq!(first.range.first_address(), second.range.first_address());
    assert_eq!(first.range.last_address(), second.range.last_address());
}