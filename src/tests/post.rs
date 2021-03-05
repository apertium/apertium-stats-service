use self::common::*;
use super::*;

#[test]
fn package_stats() {
    let module = format!("apertium-{}", TEST_LT_MODULE);
    let endpoint = format!("/{}", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);

        wait_for_ok(&client, &endpoint, |response| {
            let mut body = parse_response(response);
            if body["in_progress"].as_array().expect("valid in_progress").is_empty() {
                let stats = body["stats"].as_array_mut().expect("valid stats");
                stats.sort_by_key(|entry| entry["stat_kind"].as_str().expect("stat_kind is string").to_string());
                let created = stats[0]["created"].as_str().expect("created is string");

                let response = client.post(endpoint.clone()).dispatch();
                assert_eq!(response.status(), Status::Accepted);
                let mut body = parse_response(response);
                let in_progress = body["in_progress"].as_array_mut().expect("valid in_progress");
                assert_eq!(in_progress.len(), TEST_LT_MODULE_FILES_COUNT);

                wait_for_ok(&client, &endpoint, |response| {
                    let mut body = parse_response(response);
                    if body["in_progress"].as_array().expect("valid in_progress").is_empty() {
                        assert_eq!(body["name"], module);
                        let new_stats = body["stats"].as_array_mut().expect("valid stats");
                        assert_eq!(stats.len(), TEST_LT_MODULE_STATS_COUNT);
                        new_stats
                            .sort_by_key(|entry| entry["stat_kind"].as_str().expect("stat_kind is string").to_string());
                        let new_created = new_stats[0]["created"].as_str().expect("created is string");
                        assert!(new_created > created);

                        true
                    } else {
                        false
                    }
                });

                true
            } else {
                false
            }
        });
    });
}

#[test]
fn package_specific_stats() {
    let module = format!("apertium-{}", TEST_LT_MODULE);
    let endpoint = format!("/{}/monodix", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);

        wait_for_ok(&client, &endpoint, |response| {
            let mut body = parse_response(response);
            if body["in_progress"].as_array().expect("valid in_progress").is_empty() {
                let stats = body["stats"].as_array_mut().expect("valid stats");
                stats.sort_by_key(|entry| {
                    format!(
                        "{}_{}",
                        entry["path"].as_str().expect("path is string").to_string(),
                        entry["stat_kind"].as_str().expect("stat_kind is string").to_string()
                    )
                });
                let created = stats[0]["created"].as_str().expect("created is string");

                let response = client.post(endpoint.clone()).dispatch();
                assert_eq!(response.status(), Status::Accepted);
                let mut body = parse_response(response);
                let in_progress = body["in_progress"].as_array_mut().expect("valid in_progress");
                assert_eq!(in_progress.len(), 1);

                wait_for_ok(&client, &endpoint, |response| {
                    let mut body = parse_response(response);
                    if body["in_progress"].as_array().expect("valid in_progress").is_empty() {
                        assert_eq!(body["name"], module);
                        let new_stats = body["stats"].as_array_mut().expect("valid stats");
                        new_stats.sort_by_key(|entry| {
                            format!(
                                "{}_{}",
                                entry["path"].as_str().expect("path is string").to_string(),
                                entry["stat_kind"].as_str().expect("stat_kind is string").to_string()
                            )
                        });
                        let new_created = new_stats[0]["created"].as_str().expect("created is string");
                        assert!(new_created > created);

                        true
                    } else {
                        false
                    }
                });

                true
            } else {
                false
            }
        });
    });
}

#[test]
fn update_package_listing() {
    run_test_with_github_auth!(|client| {
        let response = client.get("/packages").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = parse_response(response);
        let packages_len = body["packages"].as_array().expect("valid packages").len();
        assert!(packages_len > 0);

        let response = client.post("/packages").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let updated_body = parse_response(response);
        let updated_packages_len = updated_body["packages"].as_array().expect("valid packages").len();
        assert!(body["as_of"].as_str().expect("valid as_of") < updated_body["as_of"].as_str().expect("valid as_of"));
        assert_eq!(updated_packages_len, packages_len);

        let response = client.post(format!("/packages/{}", TEST_LT_MODULE)).dispatch();
        assert_eq!(response.status(), Status::Ok);
        let updated_body_2 = parse_response(response);
        let updated_packages_len_2 = updated_body_2["packages"].as_array().expect("valid packages").len();
        assert!(
            updated_body["as_of"].as_str().expect("valid as_of")
                < updated_body_2["as_of"].as_str().expect("valid as_of")
        );
        assert!(updated_packages_len_2 < updated_packages_len, "{}", updated_body_2);
    });
}
