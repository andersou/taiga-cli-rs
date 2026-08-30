use std::fs;

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path},
};

#[test]
fn help_exposes_requested_command_groups() {
    let mut command = Command::cargo_bin("taiga").unwrap();
    command
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("project"))
        .stdout(contains("userstory"))
        .stdout(contains("notification"))
        .stdout(contains("timeline"));
}

#[test]
fn delete_requires_explicit_confirmation_before_network() {
    let mut command = Command::cargo_bin("taiga").unwrap();
    command
        .args(["project", "delete", "1"])
        .assert()
        .code(2)
        .stderr(contains("delete requires --yes"));
}

fn write_config(path: &std::path::Path, api_url: &str, auth_token: &str, refresh_token: &str) {
    fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({
            "api_url": api_url,
            "auth_token": auth_token,
            "refresh_token": refresh_token,
        }))
        .unwrap(),
    )
    .unwrap();
}

fn status_command(config: &std::path::Path) -> Command {
    let mut command = Command::cargo_bin("taiga").unwrap();
    command
        .args(["--output", "json", "auth", "status"])
        .env("TAIGA_CONFIG", config)
        .env_remove("TAIGA_API_URL")
        .env_remove("TAIGA_AUTH_TOKEN")
        .env_remove("TAIGA_REFRESH_TOKEN");
    command
}

#[tokio::test]
async fn expired_session_refreshes_once_and_persists_rotated_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("authorization", "Bearer expired-access"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"_error_message":"expired access"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/auth/refresh"))
        .and(body_json(serde_json::json!({"refresh":"old-refresh"})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"auth_token":"new-access","refresh":"new-refresh"}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("authorization", "Bearer new-access"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":1,"username":"ada"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    write_config(&config, &server.uri(), "expired-access", "old-refresh");

    status_command(&config)
        .assert()
        .success()
        .stdout(contains("\"username\": \"ada\""));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(config).unwrap()).unwrap(),
        serde_json::json!({
            "api_url": server.uri(),
            "auth_token": "new-access",
            "refresh_token": "new-refresh",
        }),
    );
    server.verify().await;
}

#[tokio::test]
async fn failed_refresh_returns_auth_error_without_replay_or_config_changes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("authorization", "Bearer expired-access"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"_error_message":"expired access"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/auth/refresh"))
        .and(body_json(serde_json::json!({"refresh":"old-refresh"})))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"_error_message":"refresh expired"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    write_config(&config, &server.uri(), "expired-access", "old-refresh");
    let original = fs::read(&config).unwrap();

    status_command(&config)
        .assert()
        .code(5)
        .stderr(contains("authentication failed: refresh expired"));
    assert_eq!(fs::read(config).unwrap(), original);
    server.verify().await;
}

#[tokio::test]
async fn environment_refresh_token_overrides_storage_without_persisting_rotation() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("authorization", "Bearer expired-access"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/auth/refresh"))
        .and(body_json(serde_json::json!({"refresh":"env-refresh"})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"auth_token":"new-access","refresh":"new-refresh"}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("authorization", "Bearer new-access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":1})))
        .expect(1)
        .mount(&server)
        .await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    write_config(&config, &server.uri(), "expired-access", "stored-refresh");
    let original = fs::read(&config).unwrap();

    let mut command = status_command(&config);
    command.env("TAIGA_REFRESH_TOKEN", "env-refresh");
    command.assert().success();
    assert_eq!(fs::read(config).unwrap(), original);
    server.verify().await;
}

#[tokio::test]
async fn environment_access_token_does_not_fall_back_to_stored_refresh() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("authorization", "Bearer expired-environment-access"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"_error_message":"expired access"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    write_config(&config, &server.uri(), "stored-access", "stored-refresh");
    let original = fs::read(&config).unwrap();

    let mut command = status_command(&config);
    command.env("TAIGA_AUTH_TOKEN", "expired-environment-access");
    command
        .assert()
        .code(5)
        .stderr(contains("authentication failed: expired access"));
    assert_eq!(fs::read(config).unwrap(), original);
    server.verify().await;
}

#[tokio::test]
async fn api_override_does_not_send_or_rotate_stored_refresh() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("authorization", "Bearer stored-access"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"_error_message":"expired access"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    write_config(
        &config,
        "https://endpoint-a.example/api/v1",
        "stored-access",
        "stored-refresh",
    );
    let original = fs::read(&config).unwrap();

    let mut command = status_command(&config);
    command.args(["--api-url", &server.uri()]);
    command
        .assert()
        .code(5)
        .stderr(contains("authentication failed: expired access"));
    assert_eq!(fs::read(config).unwrap(), original);
    server.verify().await;
}

#[tokio::test]
async fn retry_unauthorized_does_not_refresh_again() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("authorization", "Bearer expired-access"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/auth/refresh"))
        .and(body_json(serde_json::json!({"refresh":"old-refresh"})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"auth_token":"new-access","refresh":"new-refresh"}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("authorization", "Bearer new-access"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"_error_message":"still unauthorized"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    write_config(&config, &server.uri(), "expired-access", "old-refresh");

    status_command(&config)
        .assert()
        .code(5)
        .stderr(contains("authentication failed: still unauthorized"));
    server.verify().await;
}
