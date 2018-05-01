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
        assert!(error.starts_with("Package not found"), error.to_string());
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
        let response = client.get("/kaz/monodix").dispatch();
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
        let response = client.get("/kaz/dix").dispatch();
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

        let in_progress = body["in_progress"]
            .as_array_mut()
            .expect("valid in_progress");
        assert_eq!(in_progress.len(), TEST_HFST_MODULE_FILES_COUNT);
        in_progress
            .sort_by_key(|entry| entry["path"].as_str().expect("path is string").to_string());
        assert_eq!(in_progress[0]["kind"], "Twol");
        assert_eq!(
            in_progress[0]["path"],
            format!("apertium-{0}.err.twol", TEST_HFST_MODULE)
        );
        let revision = in_progress[0]["revision"]
            .as_i64()
            .expect("revision is i64");
        assert!(revision > 500, revision);

        wait_for_ok(&client, &endpoint, |response| {
            let mut body = parse_response(response);
            if body["in_progress"]
                .as_array()
                .expect("valid in_progress")
                .is_empty()
            {
                assert_eq!(body["name"], module);
                let stats = body["stats"].as_array_mut().expect("valid stats");
                assert_eq!(stats.len(), TEST_HFST_MODULE_STATS_COUNT);
                stats.sort_by_key(|entry| {
                    entry["path"].as_str().expect("path is string").to_string()
                });
                assert_eq!(stats[0]["file_kind"], "Twol");
                assert_eq!(stats[0]["stat_kind"], "Rules");
                assert_eq!(
                    stats[0]["path"],
                    format!("apertium-{0}.err.twol", TEST_HFST_MODULE)
                );
                let revision = stats[0]["revision"].as_i64().expect("revision is i64");
                assert!(revision > 500, revision);
                let value = stats[0]["value"].as_i64().expect("value is i64");
                assert!(value > 15, value);

                let response = client.get(endpoint.clone()).dispatch();
                assert_eq!(response.status(), Status::Ok);
                let body = parse_response(response);
                assert_eq!(body["name"], module);
                assert!(
                    body["in_progress"]
                        .as_array()
                        .expect("valid in_progress")
                        .is_empty(),
                    body["in_progress"].to_string()
                );
                assert_eq!(body["stats"].as_array().expect("valid stats").len(), 5);

                return true;
            }

            return false;
        });
    });
}

#[test]
fn pair_stats() {
    let module = format!("apertium-{}", TEST_PAIR);
    let endpoint = format!("/{}", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);
        let mut body = parse_response(response);
        let in_progress = body["in_progress"]
            .as_array_mut()
            .expect("valid in_progress");
        assert_eq!(in_progress.len(), TEST_PAIR_FILES_COUNT);

        wait_for_ok(&client, &endpoint, |response| {
            let body = parse_response(response);
            if body["in_progress"]
                .as_array()
                .expect("valid in_progress")
                .is_empty()
            {
                assert_eq!(body["name"], module);
                let stats = body["stats"].as_array().expect("valid stats");
                assert_eq!(stats.len(), TEST_PAIR_STATS_COUNT);
                assert!(
                    stats
                        .iter()
                        .map(|entry| (
                            entry["stat_kind"].as_str().expect("kind is string"),
                            entry["value"].as_i64().expect("value is i64")
                        ))
                        .all(|(kind, value)| kind == "Macros" || value > 0),
                    body["stats"].to_string(),
                );

                return true;
            }

            return false;
        });
    });
}

#[test]
fn module_specific_stats() {
    let module = format!("apertium-{}", TEST_LT_MODULE);
    let endpoint = format!("/{}/monodix", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Accepted);
        let mut body = parse_response(response);
        let in_progress = body["in_progress"]
            .as_array_mut()
            .expect("valid in_progress");
        assert_eq!(in_progress.len(), 1);

        wait_for_ok(&client, &endpoint, |response| {
            let body = parse_response(response);
            if body["in_progress"]
                .as_array()
                .expect("valid in_progress")
                .is_empty()
            {
                assert_eq!(body["name"], module);
                let stats = body["stats"].as_array().expect("valid stats");
                assert_eq!(stats.len(), 2);
                assert_eq!(
                    stats[0]["path"],
                    format!("apertium-{0}.{0}.dix", TEST_LT_MODULE)
                );
                assert_eq!(
                    stats[0]["revision"].as_i64().expect("revision1 is i64"),
                    stats[0]["revision"].as_i64().expect("revision2 is i64")
                );
                let value1 = stats[0]["value"].as_i64().expect("value is i64");
                assert!(value1 > 500, value1);
                let value2 = stats[1]["value"].as_i64().expect("value is i64");
                assert!(value2 > 500, value2);

                let response = client.get(endpoint.clone()).dispatch();
                assert_eq!(response.status(), Status::Ok);
                let body = parse_response(response);
                assert_eq!(body["name"], module);
                assert!(
                    body["in_progress"]
                        .as_array()
                        .expect("valid in_progress")
                        .is_empty(),
                    body["in_progress"].to_string()
                );
                assert_eq!(body["stats"].as_array().expect("valid stats").len(), 2);

                return true;
            }

            return false;
        });
    });
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

        let in_progress = body["in_progress"]
            .as_array_mut()
            .expect("valid in_progress");
        assert_eq!(in_progress.len(), 3);

        wait_for_ok(&client, &endpoint, |response| {
            let body = parse_response(response);
            if body["in_progress"]
                .as_array()
                .expect("valid in_progress")
                .is_empty()
            {
                assert_eq!(body["name"], module);
                let stats = body["stats"].as_array().expect("valid stats");
                assert_eq!(stats.len(), 3);
                assert!(
                    stats.iter().any(|entry| entry["path"]
                        .as_str()
                        .expect("path is string")
                        .contains("/")),
                    body["stats"].to_string()
                );

                return true;
            }

            return false;
        });
    });
}

#[test]
fn sync_package_stats() {
    let module = format!("apertium-{}", TEST_LT_MODULE);
    let endpoint = format!("/{}/monodix?async=false", module);

    run_test!(|client| {
        let response = client.get(endpoint.clone()).dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = parse_response(response);
        let in_progress = body["in_progress"].as_array().expect("valid in_progress");
        assert_eq!(in_progress.len(), 0);

        assert_eq!(body["name"], module);
        let stats = body["stats"].as_array().expect("valid stats");
        assert_eq!(stats.len(), 2);
    });
}
