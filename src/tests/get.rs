use self::common::*;
use super::*;

#[test]
fn nonexistent_package_stats() {
    run_test!(|client| {
        let response = client.get("/apertium-xxx").dispatch();
        assert_eq!(response.status(), Status::BadRequest);
        let body = parse_response(response);
        assert_eq!(body["name"], "apertium-xxx");
        let error = body["error"].as_str().expect("error is string");
        assert!(error.starts_with("Package not found"), "{}", error.to_string());
    });
}

#[test]
fn invalid_package_stats() {
    run_test!(|client| {
        let response = client.get("/abcd").dispatch();
        assert_eq!(response.status(), Status::BadRequest);
        let body = parse_response(response);
        assert_eq!(
            body,
            json!({
                "error": "Invalid package name: abcd",
                "name": "abcd"
            })
        );
    });
}

#[test]
fn nonexistent_kind_package_stats() {
    run_test!(|client| {
        let endpoint = format!("/{}/monodix", TEST_HFST_MODULE);
        let response = client.get(endpoint).dispatch();
        assert_eq!(response.status(), Status::NotFound);
        let body = parse_response(response);
        assert_eq!(
            body,
            json!({
                "error": "No recognized files",
                "name": "apertium-kaz"
            })
        );
    });
}

#[test]
fn invalid_kind_package_stats() {
    run_test!(|client| {
        let endpoint = format!("/{}/dix", TEST_HFST_MODULE);
        let response = client.get(endpoint).dispatch();
        assert_eq!(response.status(), Status::BadRequest);
        let body = parse_response(response);
        assert_eq!(
            body,
            json!({
                "error": "Invalid file kind: dix",
                "name": "apertium-kaz"
            })
        );
    });
}

#[test]
fn module_stats() {
    let module = format!("apertium-{}", TEST_HFST_MODULE);
    let endpoint = format!("/{}", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);
        let mut body = parse_response(response);
        assert_eq!(body["name"], module);

        let in_progress = body["in_progress"].as_array_mut().expect("valid in_progress");
        assert_eq!(in_progress.len(), TEST_HFST_MODULE_FILES_COUNT);
        in_progress.sort_by_key(|entry| entry["file"]["path"].as_str().expect("path is string").to_string());
        assert_eq!(in_progress[0]["kind"], "Twol");
        let file = &in_progress[0]["file"];
        assert_eq!(file["path"], format!("apertium-{0}.err.twol", TEST_HFST_MODULE));
        let size = file["size"].as_i64().expect("size is i64");
        assert!(size > 500, "{}", size);
        let revision = file["revision"].as_i64().expect("revision is i64");
        assert!(revision > 500, "{}", revision);
        let sha = file["sha"].as_str().expect("sha is str");
        assert_eq!(sha.len(), 40);

        wait_for_ok(&client, &endpoint, |response| {
            let mut body = parse_response(response);
            if body["in_progress"].as_array().expect("valid in_progress").is_empty() {
                assert_eq!(body["name"], module);
                let stats = body["stats"].as_array_mut().expect("valid stats");
                assert_eq!(stats.len(), TEST_HFST_MODULE_STATS_COUNT);
                stats.sort_by_key(|entry| entry["path"].as_str().expect("path is string").to_string());
                assert_eq!(stats[0]["file_kind"], "Twol");
                assert_eq!(stats[0]["stat_kind"], "Rules");
                assert_eq!(stats[0]["path"], format!("apertium-{0}.err.twol", TEST_HFST_MODULE));
                let revision = stats[0]["revision"].as_i64().expect("revision is i64");
                assert!(revision > 500, "{}", revision);
                let sha = stats[0]["sha"].as_str().expect("sha is str");
                assert_eq!(sha.len(), 40);
                let value = stats[0]["value"].as_i64().expect("value is i64");
                assert!(value > 15, "{}", value);

                let response = client.get(endpoint.clone()).dispatch();
                assert_eq!(response.status(), Status::Ok);
                let body = parse_response(response);
                assert_eq!(body["name"], module);
                assert!(
                    body["in_progress"].as_array().expect("valid in_progress").is_empty(),
                    "{}",
                    body["in_progress"].to_string()
                );
                assert_eq!(
                    body["stats"].as_array().expect("valid stats").len(),
                    TEST_HFST_MODULE_STATS_COUNT
                );

                true
            } else {
                false
            }
        });
    });
}

#[test]
fn hfst_pair_stats() {
    let module = format!("apertium-{}", TEST_HFST_PAIR);
    let endpoint = format!("/{}", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);
        let mut body = parse_response(response);
        let in_progress = body["in_progress"].as_array_mut().expect("valid in_progress");
        assert_eq!(in_progress.len(), TEST_HFST_PAIR_FILES_COUNT);

        wait_for_ok(&client, &endpoint, |response| {
            let body = parse_response(response);
            if body["in_progress"].as_array().expect("valid in_progress").is_empty() {
                assert_eq!(body["name"], module);
                let stats = body["stats"].as_array().expect("valid stats");
                assert_eq!(stats.len(), TEST_HFST_PAIR_STATS_COUNT);
                assert!(
                    stats
                        .iter()
                        .map(|entry| (
                            entry["stat_kind"].as_str().expect("kind is string"),
                            entry["value"].as_i64().expect("value is i64")
                        ))
                        .all(|(kind, value)| kind == "Macros" || value > 0),
                    "{}",
                    body["stats"].to_string(),
                );

                true
            } else {
                false
            }
        });
    });
}

#[test]
fn lt_pair_stats() {
    let module = format!("apertium-{}", TEST_LT_PAIR);
    let endpoint = format!("/{}?async=false", module);

    run_test!(|client| {
        let response = client.get(endpoint).dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = parse_response(response);
        let in_progress = body["in_progress"].as_array().expect("valid in_progress");
        assert_eq!(in_progress.len(), 0);

        assert_eq!(body["name"], module);
        let stats = body["stats"].as_array().expect("valid stats");
        assert_eq!(stats.len(), TEST_LT_PAIR_STATS_COUNT);
        assert!(
            stats
                .iter()
                .map(|entry| (
                    entry["stat_kind"].as_str().expect("kind is string"),
                    entry["value"].as_i64().expect("value is i64")
                ))
                .all(|(kind, value)| kind == "Macros" || value > 0),
            "{}",
            body["stats"].to_string(),
        );

        let mut files = stats
            .iter()
            .map(|entry| entry["path"].as_str().expect("path is string"))
            .collect::<Vec<_>>();
        files.sort_unstable();
        files.dedup();
        assert_eq!(files.len(), TEST_LT_PAIR_FILES_COUNT);
    });
}

#[test]
fn pair_specific_stats() {
    let kinds = [("transfer", 12), ("bidix", 1)];

    for (kind, stat_count) in &kinds {
        let module = format!("apertium-{}", TEST_LT_PAIR);
        let endpoint = format!("/{}/{}?async=false", module, kind);

        run_test!(|client| {
            let response = client.get(endpoint.clone()).dispatch();
            assert_eq!(response.status(), Status::Ok);
            let body = parse_response(response);
            let in_progress = body["in_progress"].as_array().expect("valid in_progress");
            assert_eq!(in_progress.len(), 0);

            assert_eq!(body["name"], module);
            let stats = body["stats"].as_array().expect("valid stats");
            assert_eq!(stats.len(), *stat_count);
        });
    }
}

#[test]
fn lexd_module_stats() {
    let module = "apertium-swa";

    run_test!(|client| {
        let response = client.get(format!("/{}/lexd?async=false", module)).dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = parse_response(response);
        let in_progress = body["in_progress"].as_array().expect("valid in_progress");
        assert_eq!(in_progress.len(), 0);

        assert_eq!(body["name"], module);
        let stats = body["stats"].as_array().expect("valid stats");
        assert_eq!(stats.len(), 4);

        assert!(
            stats
                .iter()
                .map(|entry| (
                    entry["stat_kind"].as_str().expect("kind is string"),
                    entry["value"].as_i64().expect("value is i64")
                ))
                .all(|(_, value)| value > 0),
            "{}",
            body["stats"].to_string(),
        );
    });
}

#[test]
fn module_specific_stats() {
    let kinds = [("monodix", 2), ("rlx", 1), ("postdix", 1)];

    for (kind, stat_count) in &kinds {
        let module = format!("apertium-{}", TEST_LT_MODULE);
        let endpoint = format!("/{}/{}?async=false", module, kind);

        run_test!(|client| {
            let response = client.get(endpoint.clone()).dispatch();
            assert_eq!(response.status(), Status::Ok);
            let body = parse_response(response);
            let in_progress = body["in_progress"].as_array().expect("valid in_progress");
            assert_eq!(in_progress.len(), 0);

            assert_eq!(body["name"], module);
            let stats = body["stats"].as_array().expect("valid stats");
            assert_eq!(stats.len(), *stat_count);
        });
    }
}

#[test]
fn recursive_package_stats() {
    let module = format!("apertium-{}", TEST_HFST_MODULE);
    let endpoint = format!("/{}/twol?recursive=true", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);
        let mut body = parse_response(response);
        assert_eq!(body["name"], module);

        let in_progress = body["in_progress"].as_array_mut().expect("valid in_progress");
        assert_eq!(in_progress.len(), 3);

        wait_for_ok(&client, &endpoint, |response| {
            let body = parse_response(response);
            if body["in_progress"].as_array().expect("valid in_progress").is_empty() {
                assert_eq!(body["name"], module);
                let stats = body["stats"].as_array().expect("valid stats");
                assert_eq!(stats.len(), 3);
                assert!(
                    stats
                        .iter()
                        .any(|entry| entry["path"].as_str().expect("path is string").contains('/')),
                    "{}",
                    body["stats"].to_string()
                );

                true
            } else {
                false
            }
        });
    });
}

#[test]
fn sync_package_stats() {
    let module = format!("apertium-{}", TEST_LT_MODULE);
    let endpoint = format!("/{}/monodix?async=false", module);

    run_test!(|client| {
        let response = client.get(endpoint).dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = parse_response(response);
        let in_progress = body["in_progress"].as_array().expect("valid in_progress");
        assert_eq!(in_progress.len(), 0);

        assert_eq!(body["name"], module);
        let stats = body["stats"].as_array().expect("valid stats");
        assert_eq!(stats.len(), 2);
    });
}

#[test]
fn module_code_conversion() {
    run_test_with_github_auth!(|client| {
        let response = client.get("/apertium-en/bidix").dispatch();
        let body = parse_response(response);
        let name = body["name"].as_str().expect("valid name");
        assert_eq!(name, "apertium-eng");
    });
}

#[test]
fn pair_code_conversion() {
    run_test_with_github_auth!(|client| {
        let response = client.get("/apertium-en-es/monodix").dispatch();
        let body = parse_response(response);
        let name = body["name"].as_str().expect("valid name");
        assert_eq!(name, "apertium-eng-spa");

        let response = client.get("/apertium-eng-glg/monodix").dispatch();
        let body = parse_response(response);
        let name = body["name"].as_str().expect("valid name");
        assert_eq!(name, "apertium-en-gl");

        let response = client.get("/apertium-zho_CN-zho_TW/monodix").dispatch();
        let body = parse_response(response);
        let name = body["name"].as_str().expect("valid name");
        assert_eq!(name, "apertium-zh_CN-zh_TW");
    });
}

#[test]
fn package_listing() {
    run_test_with_github_auth!(|client| {
        let response = client.get("/packages").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = parse_response(response);
        let all_packages_len = body["packages"].as_array().expect("valid packages").len();
        assert!(all_packages_len > 400, "{}", all_packages_len);
        assert!(
            body["as_of"].as_str().expect("valid as_of") < body["next_update"].as_str().expect("valid next_update"),
            "{:#?}",
            body
        );

        let response = client.get(format!("/packages/{}", TEST_LT_MODULE)).dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = parse_response(response);
        let specific_packages_len = body["packages"].as_array().expect("valid packages").len();
        assert!(
            specific_packages_len < all_packages_len && specific_packages_len > 10,
            "{:#?}",
            body
        );
    });
}
