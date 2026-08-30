use secrecy::SecretString;
use taiga_client::{LoginRequest, PaginationMode, TaigaClient, TaigaError, TaigaObject};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path, query_param},
};

#[tokio::test]
async fn normalizes_url_and_authenticates_resource_requests() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/projects"))
        .and(header("authorization", "Bearer token"))
        .and(header("x-disable-pagination", "True"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;
    let client = TaigaClient::builder(&format!("{}/api", server.uri()))
        .unwrap()
        .bearer_token(SecretString::from("token"))
        .build()
        .unwrap();
    let response = client
        .projects()
        .list::<TaigaObject>(&[], PaginationMode::All)
        .await
        .unwrap();
    assert!(response.items.is_empty());
    assert_eq!(client.api_url().path(), "/api/v1/");
}

#[tokio::test]
async fn login_omits_bearer_and_returns_rotated_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/auth"))
        .and(body_json(
            serde_json::json!({"type":"normal","username":"ada","password":"secret"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"auth_token":"access","refresh":"refresh","id":1,"username":"ada"}),
        ))
        .mount(&server)
        .await;
    let client = TaigaClient::builder(&server.uri())
        .unwrap()
        .build()
        .unwrap();
    let session = client
        .auth()
        .login(&LoginRequest {
            username: "ada".into(),
            password: SecretString::from("secret"),
        })
        .await
        .unwrap();
    assert_eq!(session.id.unwrap().0, 1);
}

#[tokio::test]
async fn parses_pagination_and_maps_conflict() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/projects"))
        .and(query_param("page", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-paginated", "true")
                .insert_header("x-pagination-count", "3")
                .set_body_json(serde_json::json!([])),
        )
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/api/v1/tasks/8"))
        .respond_with(
            ResponseTemplate::new(409)
                .set_body_json(serde_json::json!({"_error_message":"stale version"})),
        )
        .mount(&server)
        .await;
    let client = TaigaClient::builder(&server.uri())
        .unwrap()
        .bearer_token(SecretString::from("token"))
        .build()
        .unwrap();
    let page = client
        .projects()
        .list::<TaigaObject>(&[], PaginationMode::Page(2))
        .await
        .unwrap();
    assert_eq!(page.pagination.unwrap().count, Some(3));
    assert!(matches!(
        client
            .tasks()
            .patch::<serde_json::Value, _>(8, &serde_json::json!({"status":2,"version":1}))
            .await,
        Err(TaigaError::Conflict { .. })
    ));
}
