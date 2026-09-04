use std::fs;

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::json;
use tempfile::tempdir;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path, query_param},
};

#[test]
fn help_exposes_requested_command_groups() {
    let mut command = Command::cargo_bin("taiga-cli").unwrap();
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
    let directory = tempdir().unwrap();
    taiga(
        &directory.path().join("missing.json"),
        &["project", "delete", "1"],
    )
    .assert()
    .code(2)
    .stderr(contains("delete requires --yes"));
}

#[test]
fn missing_session_reports_login_hint_without_network() {
    let directory = tempdir().unwrap();
    taiga(&directory.path().join("missing.json"), &["project", "list"])
        .assert()
        .code(5)
        .stderr(contains("no valid session, run `taiga-cli auth login`"));
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

fn write_config_value(path: &std::path::Path, value: serde_json::Value) {
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

fn read_config(path: &std::path::Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn taiga(config: &std::path::Path, args: &[&str]) -> Command {
    let mut command = Command::cargo_bin("taiga-cli").unwrap();
    command
        .args(["--output", "json"])
        .args(args)
        .env("TAIGA_CONFIG", config)
        .env_remove("TAIGA_API_URL")
        .env_remove("TAIGA_AUTH_TOKEN")
        .env_remove("TAIGA_REFRESH_TOKEN")
        .env_remove("TAIGA_USERNAME")
        .env_remove("TAIGA_PASSWORD")
        .env_remove("TAIGA_PASSWORD_STORE_FILE");
    command
}

fn status_command(config: &std::path::Path) -> Command {
    taiga(config, &["auth", "status"])
}

/// Unsigned JWT whose payload carries the given `exp`; the CLI only reads
/// the claim, so the signature is irrelevant.
fn jwt(exp: u64) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let payload = format!(r#"{{"token_type":"access","exp":{exp}}}"#);
    let mut encoded = String::new();
    for chunk in payload.as_bytes().chunks(3) {
        let mut buffer = [0u8; 3];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let n = u32::from_be_bytes([0, buffer[0], buffer[1], buffer[2]]);
        for i in 0..chunk.len() + 1 {
            encoded.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    format!("eyJhbGciOiJIUzI1NiJ9.{encoded}.sig")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn mount_identity(server: &MockServer, bearer: &str) {
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("authorization", format!("Bearer {bearer}").as_str()))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":1,"username":"ada"})),
        )
        .expect(1)
        .mount(server)
        .await;
}

async fn mount_login(server: &MockServer, username: &str, password: &str) {
    Mock::given(method("POST"))
        .and(path("/api/v1/auth"))
        .and(body_json(serde_json::json!({
            "type": "normal",
            "username": username,
            "password": password,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 1,
            "username": username,
            "auth_token": "login-access",
            "refresh": "login-refresh",
        })))
        .expect(1)
        .mount(server)
        .await;
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

#[tokio::test]
async fn expired_jwt_access_token_refreshes_without_a_failing_request() {
    let server = MockServer::start().await;
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
    mount_identity(&server, "new-access").await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    write_config(&config, &server.uri(), &jwt(now() - 60), "old-refresh");

    status_command(&config)
        .assert()
        .success()
        .stdout(contains("\"username\": \"ada\""));
    assert_eq!(read_config(&config)["auth_token"], "new-access");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests.len(),
        2,
        "no request should be made with the expired token"
    );
    server.verify().await;
}

#[tokio::test]
async fn expired_tokens_without_credentials_fail_before_any_request() {
    let server = MockServer::start().await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    write_config_value(
        &config,
        serde_json::json!({
            "api_url": server.uri(),
            "auth_token": jwt(now() - 60),
            "refresh_token": jwt(now() - 30),
            "username": "ada",
        }),
    );
    let original = fs::read(&config).unwrap();

    status_command(&config)
        .assert()
        .code(5)
        .stderr(contains("no valid session, run `taiga-cli auth login`"));
    assert_eq!(fs::read(&config).unwrap(), original);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn expired_tokens_relogin_with_stored_password_and_persist() {
    let server = MockServer::start().await;
    mount_login(&server, "ada", "s3cret").await;
    mount_identity(&server, "login-access").await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    let store = directory.path().join("passwords.json");
    let api_url = server.uri();
    fs::write(
        &store,
        serde_json::to_vec(&serde_json::json!({
            format!("taiga-cli:{api_url}\u{1f}ada"): "s3cret",
        }))
        .unwrap(),
    )
    .unwrap();
    write_config_value(
        &config,
        serde_json::json!({
            "api_url": api_url,
            "auth_token": jwt(now() - 60),
            "refresh_token": jwt(now() - 30),
            "username": "ada",
            "remember_password": true,
        }),
    );

    let mut command = status_command(&config);
    command.env("TAIGA_PASSWORD_STORE_FILE", &store);
    command
        .assert()
        .success()
        .stdout(contains("\"password_stored\": true"));
    assert_eq!(
        read_config(&config),
        serde_json::json!({
            "api_url": server.uri(),
            "auth_token": "login-access",
            "refresh_token": "login-refresh",
            "username": "ada",
            "remember_password": true,
        }),
    );
    server.verify().await;
}

#[tokio::test]
async fn rejected_refresh_falls_back_to_environment_password_without_persisting() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/auth/refresh"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"_error_message":"refresh expired"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    mount_login(&server, "ada", "env-secret").await;
    mount_identity(&server, "login-access").await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    write_config_value(
        &config,
        serde_json::json!({
            "api_url": server.uri(),
            "auth_token": jwt(now() - 60),
            "refresh_token": "old-refresh",
            "username": "ada",
        }),
    );
    let original = fs::read(&config).unwrap();

    let mut command = status_command(&config);
    command.env("TAIGA_PASSWORD", "env-secret");
    command.assert().success();
    assert_eq!(fs::read(&config).unwrap(), original);
    server.verify().await;
}

#[tokio::test]
async fn login_remember_saves_password_and_logout_removes_it() {
    let server = MockServer::start().await;
    mount_login(&server, "ada", "s3cret").await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    let store = directory.path().join("passwords.json");
    let api_url = server.uri();

    let mut command = taiga(
        &config,
        &[
            "auth",
            "login",
            "--username",
            "ada",
            "--password-stdin",
            "--remember",
        ],
    );
    command
        .args(["--api-url", &api_url])
        .env("TAIGA_PASSWORD_STORE_FILE", &store)
        .write_stdin("s3cret\n");
    command
        .assert()
        .success()
        .stdout(contains("\"password_stored\": true"));
    assert_eq!(
        read_config(&config),
        serde_json::json!({
            "api_url": api_url,
            "auth_token": "login-access",
            "refresh_token": "login-refresh",
            "username": "ada",
            "remember_password": true,
        }),
    );
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&store).unwrap()).unwrap();
    assert_eq!(saved[format!("taiga-cli:{api_url}\u{1f}ada")], "s3cret");

    let mut command = taiga(&config, &["auth", "logout"]);
    command.env("TAIGA_PASSWORD_STORE_FILE", &store);
    command.assert().success();
    assert_eq!(
        read_config(&config),
        serde_json::json!({"api_url": api_url, "auth_token": null, "refresh_token": null}),
    );
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&store).unwrap()).unwrap();
    assert_eq!(saved, serde_json::json!({}));
}

#[tokio::test]
async fn milestone_list_uses_closed_filter_and_stories_filter_by_milestone() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/milestones"))
        .and(query_param("project", "7"))
        .and(query_param("closed", "false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([{"id":17}])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/userstories"))
        .and(query_param("project", "7"))
        .and(query_param("milestone", "17"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([{"id":99}])))
        .expect(1)
        .mount(&server)
        .await;
    let directory = tempdir().unwrap();
    let config = directory.path().join("config.json");
    write_config(&config, &server.uri(), "access", "refresh");

    taiga(
        &config,
        &["milestone", "list", "--project", "7", "--closed", "false"],
    )
    .assert()
    .success()
    .stdout(contains("\"id\": 17"));
    taiga(
        &config,
        &["userstory", "list", "--project", "7", "--milestone", "17"],
    )
    .assert()
    .success()
    .stdout(contains("\"id\": 99"));
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|r| !r.url.query().unwrap_or("").contains("status__is_closed")),
        "milestones must not receive status__is_closed"
    );
    server.verify().await;
}

/// Fake release archive for the platform this test binary was built for,
/// holding `payload` where the CLI binary would be.
fn release_archive(payload: &[u8]) -> (String, Vec<u8>) {
    let target = env!("TARGET");
    if target.contains("windows") {
        let mut bytes = Vec::new();
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
        writer
            .start_file("taiga-cli.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, payload).unwrap();
        writer.finish().unwrap();
        (format!("taiga-cli-9.9.9-{target}.zip"), bytes)
    } else {
        let mut bytes = Vec::new();
        {
            let encoder = flate2::write::GzEncoder::new(&mut bytes, flate2::Compression::fast());
            let mut builder = tar::Builder::new(encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "taiga-cli", payload)
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        (format!("taiga-cli-9.9.9-{target}.tar.gz"), bytes)
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

async fn mount_release(
    server: &MockServer,
    tag: &str,
    archive_name: &str,
    archive: Vec<u8>,
    sums: String,
) {
    Mock::given(method("GET"))
        .and(path("/repos/andersou/taiga-cli-rs/releases/latest"))
        .and(header("user-agent", format!("taiga-cli/{}", env!("CARGO_PKG_VERSION")).as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tag_name": tag,
            "html_url": format!("https://github.com/andersou/taiga-cli-rs/releases/tag/{tag}"),
            "assets": [
                {"name": archive_name, "browser_download_url": format!("{}/download/{archive_name}", server.uri())},
                {"name": "SHA256SUMS", "browser_download_url": format!("{}/download/SHA256SUMS", server.uri())},
            ]
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/download/{archive_name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/SHA256SUMS"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sums))
        .mount(server)
        .await;
}

/// A private copy of the built CLI, so replacing it leaves the real test
/// binary untouched.
fn binary_copy(directory: &std::path::Path) -> std::path::PathBuf {
    let source = assert_cmd::cargo::cargo_bin("taiga-cli");
    let name = source.file_name().unwrap();
    let copy = directory.join(name);
    fs::copy(&source, &copy).unwrap();
    copy
}

fn update_command(binary: &std::path::Path, server: &MockServer, args: &[&str]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(["--output", "json", "update"])
        .args(args)
        .env("TAIGA_CLI_RELEASES_API", server.uri())
        .env_remove("GITHUB_TOKEN");
    command
}

#[tokio::test]
async fn update_replaces_the_binary_after_verifying_the_checksum() {
    let server = MockServer::start().await;
    let payload = b"#!/bin/sh\necho updated\n".to_vec();
    let (archive_name, archive) = release_archive(&payload);
    let sums = format!("{}  {archive_name}\n", sha256_hex(&archive));
    mount_release(&server, "v9.9.9", &archive_name, archive, sums).await;
    let directory = tempdir().unwrap();
    let binary = binary_copy(directory.path());

    update_command(&binary, &server, &["--check"])
        .assert()
        .success()
        .stdout(contains("\"update_available\": true"))
        .stdout(contains("\"latest\": \"9.9.9\""));
    assert_ne!(
        fs::read(&binary).unwrap(),
        payload,
        "--check must not install"
    );

    update_command(&binary, &server, &[])
        .assert()
        .success()
        .stdout(contains("\"updated\": true"));
    assert_eq!(fs::read(&binary).unwrap(), payload);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(&binary).unwrap().permissions().mode() & 0o111,
            0
        );
    }
}

#[tokio::test]
async fn update_refuses_archives_that_fail_the_checksum() {
    let server = MockServer::start().await;
    let (archive_name, archive) = release_archive(b"tampered");
    let sums = format!("{}  {archive_name}\n", "0".repeat(64));
    mount_release(&server, "v9.9.9", &archive_name, archive, sums).await;
    let directory = tempdir().unwrap();
    let binary = binary_copy(directory.path());
    let original = fs::read(&binary).unwrap();

    update_command(&binary, &server, &[])
        .assert()
        .code(5)
        .stderr(contains("checksum mismatch"));
    assert_eq!(fs::read(&binary).unwrap(), original);
}

#[tokio::test]
async fn update_reports_when_already_current_without_downloading() {
    let server = MockServer::start().await;
    let (archive_name, archive) = release_archive(b"same");
    let sums = format!("{}  {archive_name}\n", sha256_hex(&archive));
    let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    mount_release(&server, &tag, &archive_name, archive, sums).await;
    let directory = tempdir().unwrap();
    let binary = binary_copy(directory.path());
    let original = fs::read(&binary).unwrap();

    update_command(&binary, &server, &[])
        .assert()
        .success()
        .stdout(contains("\"updated\": false"));
    assert_eq!(fs::read(&binary).unwrap(), original);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
