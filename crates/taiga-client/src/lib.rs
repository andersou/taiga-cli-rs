use std::{collections::BTreeMap, fmt, time::Duration};

use chrono::{DateTime, NaiveDate, Utc};
use reqwest::{Method, Response, StatusCode, header, multipart};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;
use url::Url;

macro_rules! id_type {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);
        impl fmt::Display for $name { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) } }
        impl From<u64> for $name { fn from(value: u64) -> Self { Self(value) } }
    )+};
}
id_type!(
    ProjectId,
    UserStoryId,
    TaskId,
    IssueId,
    EpicId,
    MilestoneId,
    WikiPageId,
    UserId,
    RoleId,
    StatusId,
    IssueTypeId,
    PriorityId,
    SeverityId,
    PointId,
    AttachmentId,
    NotificationId
);

#[derive(Clone, Debug, Deserialize)]
pub struct TokenPair {
    pub auth_token: SecretString,
    pub refresh: SecretString,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AuthSession {
    #[serde(flatten)]
    pub tokens: TokenPair,
    pub id: Option<UserId>,
    pub username: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug)]
pub struct LoginRequest {
    pub username: String,
    pub password: SecretString,
}
#[derive(Clone, Debug)]
pub struct RefreshRequest {
    pub refresh: SecretString,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaginationMode {
    All,
    Page(u32),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Pagination {
    pub paginated: Option<bool>,
    pub page_size: Option<u64>,
    pub count: Option<u64>,
    pub current: Option<u64>,
    pub next: Option<String>,
    pub previous: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListResponse<T> {
    pub items: Vec<T>,
    pub pagination: Option<Pagination>,
}

#[derive(Debug, Error)]
pub enum TaigaError {
    #[error("invalid Taiga API URL: {0}")]
    InvalidUrl(String),
    #[error("HTTP transport failure: {message}")]
    Transport { message: String },
    #[error("response decoding failure: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("authentication failed: {message}")]
    Unauthorized { message: String },
    #[error("permission denied: {message}")]
    Forbidden { message: String },
    #[error("resource not found: {message}")]
    NotFound { message: String },
    #[error("optimistic concurrency conflict: {message}")]
    Conflict { message: String },
    #[error("Taiga rate limit reached: {message}")]
    RateLimited {
        message: String,
        retry_after: Option<String>,
    },
    #[error("Taiga client error {status}: {message}")]
    ClientResponse { status: StatusCode, message: String },
    #[error("Taiga server error {status}: {message}")]
    ServerResponse { status: StatusCode, message: String },
    #[error("unexpected HTTP status {status}: {message}")]
    UnexpectedStatus { status: StatusCode, message: String },
    #[error("Taiga endpoint is unavailable: {0}")]
    UnsupportedEndpoint(&'static str),
}

#[derive(Clone)]
pub struct TaigaClient {
    http: reqwest::Client,
    base: Url,
    token: Option<SecretString>,
    language: Option<String>,
}

pub struct TaigaClientBuilder {
    base: Url,
    token: Option<SecretString>,
    language: Option<String>,
    timeout: Duration,
}

impl TaigaClientBuilder {
    pub fn new(api_url: &str) -> Result<Self, TaigaError> {
        let mut base = Url::parse(api_url).map_err(|e| TaigaError::InvalidUrl(e.to_string()))?;
        if !matches!(base.scheme(), "http" | "https")
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(TaigaError::InvalidUrl(
                "URL must be absolute HTTP(S) without credentials, query, or fragment".into(),
            ));
        }
        let path = base.path().trim_end_matches('/');
        let api_path = if path.ends_with("/api/v1") {
            path.to_owned()
        } else if path.ends_with("/api") {
            format!("{path}/v1")
        } else {
            format!("{path}/api/v1")
        };
        base.set_path(&(api_path.trim_start_matches('/').to_owned() + "/"));
        Ok(Self {
            base,
            token: None,
            language: None,
            timeout: Duration::from_secs(30),
        })
    }
    pub fn bearer_token(mut self, token: SecretString) -> Self {
        self.token = Some(token);
        self
    }
    pub fn accept_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }
    pub fn build(self) -> Result<TaigaClient, TaigaError> {
        let http = reqwest::Client::builder()
            .timeout(self.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(transport_error)?;
        Ok(TaigaClient {
            http,
            base: self.base,
            token: self.token,
            language: self.language,
        })
    }
}

impl TaigaClient {
    pub fn builder(api_url: &str) -> Result<TaigaClientBuilder, TaigaError> {
        TaigaClientBuilder::new(api_url)
    }
    pub fn api_url(&self) -> &Url {
        &self.base
    }
    pub fn auth(&self) -> AuthService<'_> {
        AuthService(self)
    }
    pub fn users(&self) -> ResourceService<'_> {
        ResourceService::new(self, "users")
    }
    pub fn projects(&self) -> ResourceService<'_> {
        ResourceService::new(self, "projects")
    }
    pub fn user_stories(&self) -> ResourceService<'_> {
        ResourceService::new(self, "userstories")
    }
    pub fn tasks(&self) -> ResourceService<'_> {
        ResourceService::new(self, "tasks")
    }
    pub fn issues(&self) -> ResourceService<'_> {
        ResourceService::new(self, "issues")
    }
    pub fn epics(&self) -> ResourceService<'_> {
        ResourceService::new(self, "epics")
    }
    pub fn milestones(&self) -> ResourceService<'_> {
        ResourceService::new(self, "milestones")
    }
    pub fn wiki(&self) -> ResourceService<'_> {
        ResourceService::new(self, "wiki")
    }
    pub fn notifications(&self) -> NotificationService<'_> {
        NotificationService(self)
    }
    pub fn timeline(&self) -> TimelineService<'_> {
        TimelineService(self)
    }
    pub fn search(&self) -> SearchService<'_> {
        SearchService(self)
    }
    pub async fn get_path<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<T, TaigaError> {
        self.request(Method::GET, path, query, None, true, None)
            .await
            .map(|(v, _)| v)
    }
    pub async fn list_path<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
        pagination: PaginationMode,
    ) -> Result<ListResponse<T>, TaigaError> {
        let (items, pagination) = self
            .request(Method::GET, path, query, None, true, Some(pagination))
            .await?;
        Ok(ListResponse { items, pagination })
    }
    pub async fn post_path<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, TaigaError> {
        self.request(
            Method::POST,
            path,
            &[],
            Some(serde_json::to_value(body)?),
            true,
            None,
        )
        .await
        .map(|(v, _)| v)
    }
    pub async fn patch_path<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, TaigaError> {
        self.request(
            Method::PATCH,
            path,
            &[],
            Some(serde_json::to_value(body)?),
            true,
            None,
        )
        .await
        .map(|(v, _)| v)
    }
    pub async fn delete_path(&self, path: &str) -> Result<(), TaigaError> {
        self.empty(Method::DELETE, path, &[], None).await
    }
    /// Fetches raw bytes from an absolute media URL, as returned in an
    /// attachment's `url`. Media hosts authenticate with the token embedded
    /// in the URL, so the session bearer is deliberately not sent.
    pub async fn download(&self, url: &str) -> Result<Vec<u8>, TaigaError> {
        let target = Url::parse(url).map_err(|e| TaigaError::InvalidUrl(e.to_string()))?;
        if !matches!(target.scheme(), "http" | "https") {
            return Err(TaigaError::InvalidUrl(
                "attachment URL must be HTTP(S)".into(),
            ));
        }
        let response = self
            .http
            .get(target)
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.map_err(transport_error)?;
            return Err(map_error(status, &text, None));
        }
        Ok(response.bytes().await.map_err(transport_error)?.into())
    }
    /// Sends one file with `multipart/form-data`, the encoding Taiga's
    /// attachment endpoints require. `fields` carries the text parts that
    /// accompany the file (`object_id`, `project`, `description`, ...).
    async fn upload<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        fields: &[(String, String)],
        file_field: &str,
        file: AttachmentFile,
    ) -> Result<T, TaigaError> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| TaigaError::Unauthorized {
                message: "no bearer token configured".into(),
            })?;
        let mut form = multipart::Form::new();
        for (key, value) in fields {
            form = form.text(key.clone(), value.clone());
        }
        form = form.part(
            file_field.to_owned(),
            multipart::Part::bytes(file.bytes)
                .file_name(file.name)
                .mime_str("application/octet-stream")
                .map_err(transport_error)?,
        );
        let mut request = self
            .http
            .request(method, self.url(path)?)
            .header(header::ACCEPT, "application/json")
            .bearer_auth(token.expose_secret())
            .multipart(form);
        if let Some(language) = &self.language {
            request = request.header(header::ACCEPT_LANGUAGE, language);
        }
        let response = request.send().await.map_err(transport_error)?;
        decode(response, None).await.map(|(value, _)| value)
    }

    fn url(&self, path: &str) -> Result<Url, TaigaError> {
        self.base
            .join(path.trim_start_matches('/'))
            .map_err(|e| TaigaError::InvalidUrl(e.to_string()))
    }
    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Value>,
        auth: bool,
        pagination: Option<PaginationMode>,
    ) -> Result<(T, Option<Pagination>), TaigaError> {
        let mut request = self
            .http
            .request(method, self.url(path)?)
            .header(header::ACCEPT, "application/json")
            .query(query);
        if let Some(language) = &self.language {
            request = request.header(header::ACCEPT_LANGUAGE, language);
        }
        if auth {
            if let Some(token) = &self.token {
                request = request.bearer_auth(token.expose_secret());
            } else {
                return Err(TaigaError::Unauthorized {
                    message: "no bearer token configured".into(),
                });
            }
        }
        if let Some(mode) = pagination {
            match mode {
                PaginationMode::All => request = request.header("x-disable-pagination", "True"),
                PaginationMode::Page(n) if n > 0 => {
                    request = request.query(&[("page", n.to_string())])
                }
                PaginationMode::Page(_) => {
                    return Err(TaigaError::InvalidUrl("page must be positive".into()));
                }
            }
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(transport_error)?;
        decode(response, pagination).await
    }
    async fn empty(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Value>,
    ) -> Result<(), TaigaError> {
        let mut request = self
            .http
            .request(method, self.url(path)?)
            .header(header::ACCEPT, "application/json");
        if let Some(token) = &self.token {
            request = request.bearer_auth(token.expose_secret());
        } else {
            return Err(TaigaError::Unauthorized {
                message: "no bearer token configured".into(),
            });
        }
        request = request.query(query);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(transport_error)?;
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let text = response.text().await.map_err(transport_error)?;
            Err(map_error(status, &text, None))
        }
    }
}

/// Turns a finished response into a typed body, mapping HTTP failures to
/// `TaigaError` and reading pagination headers when the caller asked for
/// a paginated list.
async fn decode<T: DeserializeOwned>(
    response: Response,
    pagination: Option<PaginationMode>,
) -> Result<(T, Option<Pagination>), TaigaError> {
    let status = response.status();
    let page = pagination.and_then(|_| parse_pagination(response.headers()));
    let retry_after = response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let text = response.text().await.map_err(transport_error)?;
    if !status.is_success() {
        return Err(map_error(status, &text, retry_after));
    }
    Ok((serde_json::from_str(&text)?, page))
}

fn transport_error(error: reqwest::Error) -> TaigaError {
    let category = if error.is_timeout() {
        "request timed out after 30 seconds"
    } else if error.is_connect() {
        "connection, DNS, proxy, or TLS handshake failed"
    } else if error.is_request() {
        "request construction failed"
    } else if error.is_decode() {
        "response body transfer failed"
    } else {
        "HTTP request failed"
    };
    TaigaError::Transport {
        message: format!("{category}: {error:?}"),
    }
}

fn parse_pagination(headers: &header::HeaderMap) -> Option<Pagination> {
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let paginated = get("x-paginated").and_then(|v| v.parse().ok());
    let page_size = get("x-paginated-by").and_then(|v| v.parse().ok());
    let count = get("x-pagination-count").and_then(|v| v.parse().ok());
    let current = get("x-pagination-current").and_then(|v| v.parse().ok());
    let next = get("x-pagination-next");
    let previous = get("x-pagination-prev");
    if paginated.is_none()
        && page_size.is_none()
        && count.is_none()
        && current.is_none()
        && next.is_none()
        && previous.is_none()
    {
        None
    } else {
        Some(Pagination {
            paginated,
            page_size,
            count,
            current,
            next,
            previous,
        })
    }
}
fn map_error(status: StatusCode, text: &str, retry_after: Option<String>) -> TaigaError {
    let message = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| {
            v.get("_error_message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| text.to_owned());
    match status {
        StatusCode::UNAUTHORIZED => TaigaError::Unauthorized { message },
        StatusCode::FORBIDDEN => TaigaError::Forbidden { message },
        StatusCode::NOT_FOUND => TaigaError::NotFound { message },
        StatusCode::CONFLICT => TaigaError::Conflict { message },
        StatusCode::TOO_MANY_REQUESTS => TaigaError::RateLimited {
            message,
            retry_after,
        },
        s if s.is_client_error() => TaigaError::ClientResponse { status: s, message },
        s if s.is_server_error() => TaigaError::ServerResponse { status: s, message },
        s => TaigaError::UnexpectedStatus { status: s, message },
    }
}

pub struct AuthService<'a>(&'a TaigaClient);
impl AuthService<'_> {
    pub async fn login(&self, request: &LoginRequest) -> Result<AuthSession, TaigaError> {
        let body = serde_json::json!({"type":"normal", "username":request.username, "password":request.password.expose_secret()});
        self.0
            .request(Method::POST, "auth", &[], Some(body), false, None)
            .await
            .map(|(v, _)| v)
    }
    pub async fn refresh(&self, request: &RefreshRequest) -> Result<TokenPair, TaigaError> {
        self.0
            .request(
                Method::POST,
                "auth/refresh",
                &[],
                Some(serde_json::json!({"refresh":request.refresh.expose_secret()})),
                false,
                None,
            )
            .await
            .map(|(v, _)| v)
    }
}

/// One file to upload: the name Taiga stores and its bytes.
#[derive(Clone, Debug)]
pub struct AttachmentFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Optional attachment fields shared by create and update. Omitted fields
/// are left untouched by `PATCH` and defaulted by the API on create.
#[derive(Clone, Debug, Default)]
pub struct AttachmentFields {
    pub description: Option<String>,
    pub is_deprecated: Option<bool>,
    pub from_comment: Option<bool>,
    pub order: Option<i64>,
}
impl AttachmentFields {
    pub fn is_empty(&self) -> bool {
        self.description.is_none()
            && self.is_deprecated.is_none()
            && self.from_comment.is_none()
            && self.order.is_none()
    }
    /// Text parts for a multipart request; Django parses these booleans and
    /// integers from their string forms.
    fn text_parts(&self) -> Vec<(String, String)> {
        let mut parts = Vec::new();
        if let Some(description) = &self.description {
            parts.push(("description".to_owned(), description.clone()));
        }
        if let Some(deprecated) = self.is_deprecated {
            parts.push(("is_deprecated".to_owned(), deprecated.to_string()));
        }
        if let Some(from_comment) = self.from_comment {
            parts.push(("from_comment".to_owned(), from_comment.to_string()));
        }
        if let Some(order) = self.order {
            parts.push(("order".to_owned(), order.to_string()));
        }
        parts
    }
    fn json(&self) -> Value {
        let mut body = serde_json::Map::new();
        if let Some(description) = &self.description {
            body.insert("description".into(), Value::from(description.clone()));
        }
        if let Some(deprecated) = self.is_deprecated {
            body.insert("is_deprecated".into(), Value::from(deprecated));
        }
        if let Some(from_comment) = self.from_comment {
            body.insert("from_comment".into(), Value::from(from_comment));
        }
        if let Some(order) = self.order {
            body.insert("order".into(), Value::from(order));
        }
        Value::Object(body)
    }
}

pub struct ResourceService<'a> {
    client: &'a TaigaClient,
    path: &'static str,
}
impl<'a> ResourceService<'a> {
    fn new(client: &'a TaigaClient, path: &'static str) -> Self {
        Self { client, path }
    }
    pub async fn list<T: DeserializeOwned>(
        &self,
        query: &[(String, String)],
        pagination: PaginationMode,
    ) -> Result<ListResponse<T>, TaigaError> {
        let (items, pagination) = self
            .client
            .request(Method::GET, self.path, query, None, true, Some(pagination))
            .await?;
        Ok(ListResponse { items, pagination })
    }
    pub async fn get<T: DeserializeOwned>(&self, id: u64) -> Result<T, TaigaError> {
        self.client
            .request(
                Method::GET,
                &format!("{}/{}", self.path, id),
                &[],
                None,
                true,
                None,
            )
            .await
            .map(|(v, _)| v)
    }
    pub async fn by_ref<T: DeserializeOwned>(
        &self,
        project: ProjectId,
        reference: u64,
    ) -> Result<T, TaigaError> {
        self.client
            .request(
                Method::GET,
                &format!("{}/by_ref", self.path),
                &[
                    ("project".into(), project.0.to_string()),
                    ("ref".into(), reference.to_string()),
                ],
                None,
                true,
                None,
            )
            .await
            .map(|(v, _)| v)
    }
    pub async fn create<T: DeserializeOwned, B: Serialize>(
        &self,
        body: &B,
    ) -> Result<T, TaigaError> {
        self.client
            .request(
                Method::POST,
                self.path,
                &[],
                Some(serde_json::to_value(body)?),
                true,
                None,
            )
            .await
            .map(|(v, _)| v)
    }
    pub async fn patch<T: DeserializeOwned, B: Serialize>(
        &self,
        id: u64,
        body: &B,
    ) -> Result<T, TaigaError> {
        self.client
            .request(
                Method::PATCH,
                &format!("{}/{}", self.path, id),
                &[],
                Some(serde_json::to_value(body)?),
                true,
                None,
            )
            .await
            .map(|(v, _)| v)
    }
    pub async fn delete(&self, id: u64) -> Result<(), TaigaError> {
        self.client
            .empty(Method::DELETE, &format!("{}/{}", self.path, id), &[], None)
            .await
    }
    pub async fn nested_list<T: DeserializeOwned>(
        &self,
        suffix: &str,
        query: &[(String, String)],
        pagination: PaginationMode,
    ) -> Result<ListResponse<T>, TaigaError> {
        let (items, pagination) = self
            .client
            .request(
                Method::GET,
                &format!("{}/{}", self.path, suffix),
                query,
                None,
                true,
                Some(pagination),
            )
            .await?;
        Ok(ListResponse { items, pagination })
    }
    /// Lists the attachments of one object of this resource. Taiga requires
    /// both the object and its project.
    pub async fn attachments<T: DeserializeOwned>(
        &self,
        project: ProjectId,
        object_id: u64,
        pagination: PaginationMode,
    ) -> Result<ListResponse<T>, TaigaError> {
        let (items, pagination) = self
            .client
            .request(
                Method::GET,
                &self.attachments_path(),
                &[
                    ("object_id".into(), object_id.to_string()),
                    ("project".into(), project.0.to_string()),
                ],
                None,
                true,
                Some(pagination),
            )
            .await?;
        Ok(ListResponse { items, pagination })
    }
    pub async fn attachment<T: DeserializeOwned>(&self, id: u64) -> Result<T, TaigaError> {
        self.client
            .request(
                Method::GET,
                &format!("{}/{}", self.attachments_path(), id),
                &[],
                None,
                true,
                None,
            )
            .await
            .map(|(v, _)| v)
    }
    pub async fn create_attachment<T: DeserializeOwned>(
        &self,
        project: ProjectId,
        object_id: u64,
        file: AttachmentFile,
        fields: &AttachmentFields,
    ) -> Result<T, TaigaError> {
        let mut parts = vec![
            ("object_id".to_owned(), object_id.to_string()),
            ("project".to_owned(), project.0.to_string()),
        ];
        parts.extend(fields.text_parts());
        self.client
            .upload(
                Method::POST,
                &self.attachments_path(),
                &parts,
                "attached_file",
                file,
            )
            .await
    }
    /// Partially updates one attachment (`PATCH`). Passing `file` replaces
    /// the stored file, which the API only accepts as multipart.
    pub async fn update_attachment<T: DeserializeOwned>(
        &self,
        id: u64,
        file: Option<AttachmentFile>,
        fields: &AttachmentFields,
    ) -> Result<T, TaigaError> {
        let path = format!("{}/{}", self.attachments_path(), id);
        match file {
            Some(file) => {
                self.client
                    .upload(
                        Method::PATCH,
                        &path,
                        &fields.text_parts(),
                        "attached_file",
                        file,
                    )
                    .await
            }
            None => self.client.patch_path(&path, &fields.json()).await,
        }
    }
    pub async fn delete_attachment(&self, id: u64) -> Result<(), TaigaError> {
        self.client
            .empty(
                Method::DELETE,
                &format!("{}/{}", self.attachments_path(), id),
                &[],
                None,
            )
            .await
    }
    fn attachments_path(&self) -> String {
        format!("{}/attachments", self.path)
    }
}

pub struct NotificationService<'a>(&'a TaigaClient);
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebNotification {
    pub id: NotificationId,
    pub event_type: Value,
    pub user: Value,
    pub data: Value,
    pub created: Option<DateTime<Utc>>,
    pub read: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
impl NotificationService<'_> {
    pub async fn list(
        &self,
        unread: bool,
        pagination: PaginationMode,
    ) -> Result<ListResponse<WebNotification>, TaigaError> {
        let query = if unread {
            vec![("only_unread".into(), "true".into())]
        } else {
            vec![]
        };
        let (value, page) = self
            .0
            .request::<Value>(
                Method::GET,
                "web-notifications",
                &query,
                None,
                true,
                Some(pagination),
            )
            .await?;
        let items = if let Some(objects) = value.get("objects") {
            serde_json::from_value(objects.clone())?
        } else {
            serde_json::from_value(value)?
        };
        Ok(ListResponse {
            items,
            pagination: page,
        })
    }
    pub async fn unread_count(&self) -> Result<u64, TaigaError> {
        let (value, _) = self
            .0
            .request::<Value>(
                Method::GET,
                "web-notifications",
                &[("only_unread".into(), "true".into())],
                None,
                true,
                Some(PaginationMode::Page(1)),
            )
            .await?;
        value.get("total").and_then(Value::as_u64).ok_or_else(|| {
            TaigaError::Decode(serde_json::Error::io(std::io::Error::other(
                "missing notification total",
            )))
        })
    }
    pub async fn read(&self, id: NotificationId) -> Result<(), TaigaError> {
        self.0
            .empty(
                Method::PATCH,
                &format!("web-notifications/{}/set-as-read", id.0),
                &[],
                None,
            )
            .await
    }
    pub async fn read_all(&self) -> Result<(), TaigaError> {
        self.0
            .empty(Method::POST, "web-notifications/set-as-read", &[], None)
            .await
    }
}

pub struct TimelineService<'a>(&'a TaigaClient);
impl TimelineService<'_> {
    pub async fn list<T: DeserializeOwned>(
        &self,
        kind: &str,
        id: u64,
        relevant: bool,
        pagination: PaginationMode,
    ) -> Result<ListResponse<T>, TaigaError> {
        let query = if relevant {
            vec![("only_relevant".into(), "true".into())]
        } else {
            vec![]
        };
        let (items, page) = self
            .0
            .request(
                Method::GET,
                &format!("timeline/{kind}/{id}"),
                &query,
                None,
                true,
                Some(pagination),
            )
            .await?;
        Ok(ListResponse {
            items,
            pagination: page,
        })
    }
}

pub struct SearchService<'a>(&'a TaigaClient);
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchResults {
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub epics: Vec<Value>,
    #[serde(default)]
    pub userstories: Vec<Value>,
    #[serde(default)]
    pub tasks: Vec<Value>,
    #[serde(default)]
    pub issues: Vec<Value>,
    #[serde(default)]
    pub wikipages: Vec<Value>,
}
impl SearchService<'_> {
    pub async fn project(
        &self,
        project: ProjectId,
        text: &str,
    ) -> Result<SearchResults, TaigaError> {
        self.0
            .request(
                Method::GET,
                "search",
                &[
                    ("project".into(), project.0.to_string()),
                    ("text".into(), text.into()),
                ],
                None,
                true,
                None,
            )
            .await
            .map(|(v, _)| v)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PatchField<T> {
    Unset,
    Value(T),
    Null,
}
impl<T: Serialize> PatchField<T> {
    pub fn into_value(self) -> Option<Value> {
        match self {
            Self::Unset => None,
            Self::Value(v) => serde_json::to_value(v).ok(),
            Self::Null => Some(Value::Null),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaigaObject {
    pub id: Option<u64>,
    #[serde(rename = "ref")]
    pub ref_: Option<u64>,
    pub subject: Option<String>,
    pub name: Option<String>,
    pub slug: Option<String>,
    pub project: Option<ProjectId>,
    pub version: Option<u64>,
    pub description: Option<String>,
    pub status: Option<StatusId>,
    pub assigned_to: Option<UserId>,
    pub milestone: Option<MilestoneId>,
    pub is_closed: Option<bool>,
    pub created_date: Option<DateTime<Utc>>,
    pub modified_date: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MilestoneStats {
    pub name: Option<String>,
    pub estimated_start: Option<NaiveDate>,
    pub estimated_finish: Option<NaiveDate>,
    pub completed_tasks: Option<u64>,
    pub total_tasks: Option<u64>,
    pub completed_userstories: Option<u64>,
    pub total_userstories: Option<u64>,
    #[serde(default)]
    pub days: Vec<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn transport_error_includes_connection_category() {
        let error = reqwest::Client::new()
            .get("http://127.0.0.1:1")
            .send()
            .await
            .unwrap_err();
        let message = transport_error(error).to_string();
        assert!(message.contains("connection, DNS, proxy, or TLS handshake failed"));
    }
}
