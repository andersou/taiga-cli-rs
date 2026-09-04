mod session;

use std::{
    collections::BTreeMap,
    fs,
    io::{self, IsTerminal, Read},
    path::PathBuf,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use directories::ProjectDirs;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use taiga_client::{
    LoginRequest, NotificationId, PaginationMode, ProjectId, RefreshRequest, TaigaClient,
    TaigaError, TokenPair,
};
use thiserror::Error;

use crate::session::PasswordStore;

#[derive(Parser)]
#[command(name = "taiga-cli", version, about = "Taiga REST API CLI")]
struct Cli {
    #[arg(long, global = true, value_enum, default_value_t = Output::Human)]
    output: Output,
    #[arg(long, global = true, env = "TAIGA_API_URL")]
    api_url: Option<String>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Clone, Copy, ValueEnum)]
enum Output {
    Human,
    Json,
}
#[derive(Subcommand)]
enum Command {
    Auth(AuthCommand),
    Project(ResourceCommand),
    Userstory(ResourceCommand),
    Task(TaskCommand),
    Issue(ResourceCommand),
    Epic(EpicCommand),
    Milestone(ResourceCommand),
    Wiki(ResourceCommand),
    Search(SearchCommand),
    Notification(NotificationCommand),
    Timeline(TimelineCommand),
}
#[derive(Subcommand)]
enum AuthAction {
    Login(LoginArgs),
    Status,
    Logout,
    /// Remove the password saved by `login --remember`, keeping the session
    Forget,
}
#[derive(Args)]
struct AuthCommand {
    #[command(subcommand)]
    action: AuthAction,
}
#[derive(Args)]
struct LoginArgs {
    #[arg(short, long, env = "TAIGA_USERNAME")]
    username: Option<String>,
    #[arg(long)]
    password_stdin: bool,
    /// Save the password in the system keychain for silent re-login
    #[arg(long)]
    remember: bool,
}
#[derive(Args)]
struct ResourceCommand {
    #[command(subcommand)]
    action: ResourceAction,
}
#[derive(Subcommand)]
enum ResourceAction {
    List(ListArgs),
    Get(IdArgs),
    GetByRef(RefArgs),
    Create(BodyArgs),
    Edit(EditArgs),
    Delete(DeleteArgs),
    History(HistoryArgs),
    Comments(HistoryArgs),
    Attachments(IdArgs),
    Stats(IdArgs),
    Statuses(ProjectArgs),
    Metadata(ProjectArgs),
    Roles(ProjectArgs),
    Memberships(ProjectArgs),
}
#[derive(Args)]
struct ListArgs {
    #[arg(long)]
    project: Option<u64>,
    #[arg(long)]
    page: Option<u32>,
    #[arg(long)]
    all_visible: bool,
    /// Filter by closed state (milestones use the API `closed` flag,
    /// everything else uses `status__is_closed`)
    #[arg(long)]
    closed: Option<bool>,
    #[arg(long)]
    status: Option<u64>,
    #[arg(long)]
    assigned_to: Option<u64>,
    /// Restrict stories, tasks, or issues to one sprint (milestone ID)
    #[arg(long)]
    milestone: Option<u64>,
    #[arg(long)]
    tag: Vec<String>,
}
#[derive(Args)]
struct IdArgs {
    id: u64,
}
#[derive(Args)]
struct RefArgs {
    reference: u64,
    #[arg(long)]
    project: u64,
}
#[derive(Args)]
struct ProjectArgs {
    #[arg(long)]
    project: u64,
    #[arg(long)]
    page: Option<u32>,
}
#[derive(Args)]
struct BodyArgs {
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    project: Option<u64>,
    #[arg(long)]
    description: Option<String>,
    #[arg(long)]
    status: Option<u64>,
    #[arg(long)]
    data: Option<String>,
}
#[derive(Args)]
struct EditArgs {
    id: u64,
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    description: Option<String>,
    #[arg(long)]
    status: Option<u64>,
    #[arg(long)]
    data: Option<String>,
}
#[derive(Args)]
struct DeleteArgs {
    id: u64,
    #[arg(long)]
    yes: bool,
}
#[derive(Args)]
struct HistoryArgs {
    id: u64,
    #[arg(long, value_enum, default_value_t = HistoryKind::All)]
    kind: HistoryKind,
    #[arg(long)]
    page: Option<u32>,
}
#[derive(Clone, Copy, ValueEnum)]
enum HistoryKind {
    All,
    Activity,
    Comment,
}
#[derive(Args)]
struct TaskCommand {
    #[command(subcommand)]
    action: TaskAction,
}
#[derive(Subcommand)]
enum TaskAction {
    #[command(flatten)]
    Resource(ResourceAction),
    Status(TaskStatusArgs),
}
#[derive(Args)]
struct TaskStatusArgs {
    id: u64,
    status: u64,
}
#[derive(Args)]
struct EpicCommand {
    #[command(subcommand)]
    action: EpicAction,
}
#[derive(Subcommand)]
enum EpicAction {
    #[command(flatten)]
    Resource(ResourceAction),
    Stories(EpicStories),
}
#[derive(Args)]
struct EpicStories {
    #[command(subcommand)]
    action: EpicStoriesAction,
}
#[derive(Subcommand)]
enum EpicStoriesAction {
    List(IdArgs),
    Add(EpicStoryArgs),
    Reorder(EpicStoryOrder),
    Remove(EpicStoryRemove),
}
#[derive(Args)]
struct EpicStoryArgs {
    epic_id: u64,
    story_id: u64,
}
#[derive(Args)]
struct EpicStoryOrder {
    epic_id: u64,
    story_id: u64,
    #[arg(long)]
    order: u64,
}
#[derive(Args)]
struct EpicStoryRemove {
    epic_id: u64,
    story_id: u64,
    #[arg(long)]
    yes: bool,
}
#[derive(Args)]
struct SearchCommand {
    text: String,
    #[arg(long, conflicts_with_all = ["project_slug", "all_projects"])]
    project: Option<u64>,
    #[arg(long, conflicts_with_all = ["project", "all_projects"])]
    project_slug: Option<String>,
    #[arg(long, conflicts_with_all = ["project", "project_slug"])]
    all_projects: bool,
}
#[derive(Args)]
struct NotificationCommand {
    #[command(subcommand)]
    action: NotificationAction,
}
#[derive(Subcommand)]
enum NotificationAction {
    List {
        #[arg(long)]
        unread: bool,
        #[arg(long)]
        page: Option<u32>,
    },
    Count,
    Read(IdArgs),
    ReadAll,
}
#[derive(Args)]
struct TimelineCommand {
    #[command(subcommand)]
    action: TimelineAction,
}
#[derive(Subcommand)]
enum TimelineAction {
    User(TimelineArgs),
    Profile(TimelineArgs),
    Project(TimelineArgs),
}
#[derive(Args)]
struct TimelineArgs {
    id: u64,
    #[arg(long)]
    relevant: bool,
    #[arg(long)]
    page: Option<u32>,
}

#[derive(Debug, Error)]
enum AppError {
    #[error("{0}")]
    Client(#[from] TaigaError),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Usage(String),
}
#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    api_url: Option<String>,
    auth_token: Option<String>,
    refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    remember_password: bool,
}

fn config_path() -> Result<PathBuf, AppError> {
    if let Some(path) = std::env::var_os("TAIGA_CONFIG") {
        return Ok(path.into());
    }
    ProjectDirs::from("", "", "taiga-cli")
        .map(|d| d.config_dir().join("config.json"))
        .ok_or_else(|| AppError::Config("unable to determine configuration directory".into()))
}
fn load_config(path: &PathBuf) -> Result<Config, AppError> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.into()),
    }
}
fn save_config(path: &PathBuf, config: &Config) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
    }
    let data = serde_json::to_vec_pretty(config)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}
fn pagination(page: Option<u32>) -> Result<PaginationMode, AppError> {
    match page {
        Some(0) => Err(AppError::Usage("--page must be positive".into())),
        Some(n) => Ok(PaginationMode::Page(n)),
        None => Ok(PaginationMode::Page(1)),
    }
}
fn api_url(cli_url: &Option<String>, config: &Config) -> String {
    cli_url
        .clone()
        .or_else(|| std::env::var("TAIGA_API_URL").ok())
        .or_else(|| config.api_url.clone())
        .unwrap_or_else(|| "https://api.taiga.io/api/v1".into())
}
fn client(url: &str, access_token: Option<&str>) -> Result<TaigaClient, AppError> {
    let builder = TaigaClient::builder(url)?;
    Ok(match access_token {
        Some(token) => builder.bearer_token(SecretString::from(token)).build()?,
        None => builder.build()?,
    })
}
fn query(path: &str, args: &ListArgs) -> Vec<(String, String)> {
    let mut q = Vec::new();
    for (key, value) in [
        ("project", args.project),
        ("status", args.status),
        ("assigned_to", args.assigned_to),
        ("milestone", args.milestone),
    ] {
        if let Some(v) = value {
            q.push((key.into(), v.to_string()));
        }
    }
    if let Some(closed) = args.closed {
        let key = if path == "milestones" {
            "closed"
        } else {
            "status__is_closed"
        };
        q.push((key.into(), closed.to_string()));
    }
    if !args.tag.is_empty() {
        q.push(("tags".into(), args.tag.join(",")));
    }
    q
}
fn body(args: &BodyArgs) -> Result<Value, AppError> {
    let mut value = match &args.data {
        Some(data) => serde_json::from_str(data)?,
        None => json!({}),
    };
    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::Usage("--data must be a JSON object".into()))?;
    if let Some(project) = args.project {
        object.insert("project".into(), json!(project));
    }
    if let Some(subject) = &args.subject {
        object.insert("subject".into(), json!(subject));
    }
    if let Some(description) = &args.description {
        object.insert("description".into(), json!(description));
    }
    if let Some(status) = args.status {
        object.insert("status".into(), json!(status));
    }
    Ok(value)
}
fn edit_body(args: &EditArgs, version: u64) -> Result<Value, AppError> {
    let mut value = match &args.data {
        Some(data) => serde_json::from_str(data)?,
        None => json!({}),
    };
    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::Usage("--data must be a JSON object".into()))?;
    if let Some(subject) = &args.subject {
        object.insert("subject".into(), json!(subject));
    }
    if let Some(description) = &args.description {
        object.insert("description".into(), json!(description));
    }
    if let Some(status) = args.status {
        object.insert("status".into(), json!(status));
    }
    if object.is_empty() {
        return Err(AppError::Usage("edit needs at least one field".into()));
    }
    object.insert("version".into(), json!(version));
    Ok(value)
}

fn emit(output: Output, value: &impl Serialize) -> Result<(), AppError> {
    match output {
        Output::Json => println!("{}", serde_json::to_string_pretty(value)?),
        Output::Human => println!("{}", serde_json::to_string_pretty(value)?),
    }
    Ok(())
}

async fn resource(
    client: &TaigaClient,
    path: &str,
    action: &ResourceAction,
    output: Output,
) -> Result<(), AppError> {
    let service = match path {
        "projects" => client.projects(),
        "userstories" => client.user_stories(),
        "tasks" => client.tasks(),
        "issues" => client.issues(),
        "epics" => client.epics(),
        "milestones" => client.milestones(),
        "wiki" => client.wiki(),
        _ => unreachable!(),
    };
    match action {
        ResourceAction::List(args) => {
            let mut list_query = query(path, args);
            if path == "projects" && !args.all_visible {
                let me: Value = client.get_path("users/me", &[]).await?;
                let member = me
                    .get("id")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| AppError::Usage("current-user response lacks id".into()))?;
                list_query.push(("member".into(), member.to_string()));
            }
            emit(
                output,
                &service
                    .list::<Value>(&list_query, pagination(args.page)?)
                    .await?,
            )
        }
        ResourceAction::Get(args) => emit(output, &service.get::<Value>(args.id).await?),
        ResourceAction::GetByRef(args) => emit(
            output,
            &service
                .by_ref::<Value>(ProjectId(args.project), args.reference)
                .await?,
        ),
        ResourceAction::Create(args) => {
            emit(output, &service.create::<Value, _>(&body(args)?).await?)
        }
        ResourceAction::Edit(args) => {
            let current = service.get::<Value>(args.id).await?;
            let version = current
                .get("version")
                .and_then(Value::as_u64)
                .ok_or_else(|| AppError::Usage("resource response lacks version".into()))?;
            emit(
                output,
                &service
                    .patch::<Value, _>(args.id, &edit_body(args, version)?)
                    .await?,
            )
        }
        ResourceAction::Delete(args) => {
            if !args.yes {
                return Err(AppError::Usage("delete requires --yes".into()));
            }
            service.delete(args.id).await?;
            emit(output, &json!({"deleted":true,"id":args.id}))
        }
        ResourceAction::History(args) | ResourceAction::Comments(args) => {
            let kind = match args.kind {
                HistoryKind::All => None,
                HistoryKind::Activity => Some("activity"),
                HistoryKind::Comment => Some("comment"),
            };
            let resource = match path {
                "userstories" => "userstory",
                "tasks" => "task",
                "issues" => "issue",
                "wiki" => "wiki",
                "epics" => "epic",
                _ => {
                    return Err(AppError::Usage(
                        "history unavailable for this resource".into(),
                    ));
                }
            };
            let mut q = Vec::new();
            if let Some(kind) = kind {
                q.push(("type".into(), kind.into()));
            }
            emit(
                output,
                &client
                    .list_path::<Value>(
                        &format!("history/{resource}/{}", args.id),
                        &q,
                        pagination(args.page)?,
                    )
                    .await?,
            )
        }
        ResourceAction::Attachments(args) => {
            let current = service.get::<Value>(args.id).await?;
            let project = current
                .get("project")
                .and_then(Value::as_u64)
                .ok_or_else(|| AppError::Usage("resource lacks project".into()))?;
            emit(
                output,
                &client
                    .list_path::<Value>(
                        &format!("{path}/attachments"),
                        &[
                            ("object_id".into(), args.id.to_string()),
                            ("project".into(), project.to_string()),
                        ],
                        PaginationMode::All,
                    )
                    .await?,
            )
        }
        ResourceAction::Stats(args) => emit(
            output,
            &client
                .get_path::<Value>(&format!("{path}/{}/stats", args.id), &[])
                .await?,
        ),
        ResourceAction::Statuses(args) => {
            let kind = if path == "userstories" {
                "userstory"
            } else if path == "tasks" {
                "task"
            } else if path == "epics" {
                "epic"
            } else {
                "issue"
            };
            emit(
                output,
                &client
                    .list_path::<Value>(
                        &format!("{kind}-statuses"),
                        &[("project".into(), args.project.to_string())],
                        pagination(args.page)?,
                    )
                    .await?,
            )
        }
        ResourceAction::Metadata(args) => {
            let mut result = BTreeMap::new();
            for name in ["issue-statuses", "issue-types", "priorities", "severities"] {
                result.insert(
                    name,
                    client
                        .list_path::<Value>(
                            name,
                            &[("project".into(), args.project.to_string())],
                            pagination(args.page)?,
                        )
                        .await?,
                );
            }
            emit(output, &result)
        }
        ResourceAction::Roles(args) => emit(
            output,
            &client
                .list_path::<Value>(
                    "roles",
                    &[("project".into(), args.project.to_string())],
                    pagination(args.page)?,
                )
                .await?,
        ),
        ResourceAction::Memberships(args) => emit(
            output,
            &client
                .list_path::<Value>(
                    "memberships",
                    &[("project".into(), args.project.to_string())],
                    pagination(args.page)?,
                )
                .await?,
        ),
    }
}

fn set_tokens(config: &mut Config, tokens: &TokenPair) {
    config.auth_token = Some(tokens.auth_token.expose_secret().to_owned());
    config.refresh_token = Some(tokens.refresh.expose_secret().to_owned());
}

async fn execute_authenticated(
    client: &TaigaClient,
    config: &Config,
    command: &Command,
    output: Output,
) -> Result<(), AppError> {
    match command {
        Command::Auth(AuthCommand {
            action: AuthAction::Status,
        }) => {
            let identity: Value = client.get_path("users/me", &[]).await?;
            emit(
                output,
                &json!({
                    "api_url": config.api_url,
                    "identity": identity,
                    "password_stored": config.remember_password,
                }),
            )
        }
        Command::Project(c) => resource(client, "projects", &c.action, output).await,
        Command::Userstory(c) => resource(client, "userstories", &c.action, output).await,
        Command::Issue(c) => resource(client, "issues", &c.action, output).await,
        Command::Milestone(c) => resource(client, "milestones", &c.action, output).await,
        Command::Wiki(c) => resource(client, "wiki", &c.action, output).await,
        Command::Task(c) => match &c.action {
            TaskAction::Resource(action) => resource(client, "tasks", action, output).await,
            TaskAction::Status(args) => {
                let current = client.tasks().get::<Value>(args.id).await?;
                let version = current
                    .get("version")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| AppError::Usage("task response lacks version".into()))?;
                emit(
                    output,
                    &client
                        .tasks()
                        .patch::<Value, _>(
                            args.id,
                            &json!({"status":args.status,"version":version}),
                        )
                        .await?,
                )
            }
        },
        Command::Epic(c) => match &c.action {
            EpicAction::Resource(action) => resource(client, "epics", action, output).await,
            EpicAction::Stories(stories) => match &stories.action {
                EpicStoriesAction::List(args) => emit(
                    output,
                    &client
                        .list_path::<Value>(
                            &format!("epics/{}/related_userstories", args.id),
                            &[],
                            PaginationMode::All,
                        )
                        .await?,
                ),
                EpicStoriesAction::Add(args) => emit(
                    output,
                    &client
                        .post_path::<Value, _>(
                            &format!("epics/{}/related_userstories", args.epic_id),
                            &json!({"epic":args.epic_id,"user_story":args.story_id}),
                        )
                        .await?,
                ),
                EpicStoriesAction::Reorder(args) => emit(
                    output,
                    &client
                        .patch_path::<Value, _>(
                            &format!(
                                "epics/{}/related_userstories/{}",
                                args.epic_id, args.story_id
                            ),
                            &json!({"order":args.order}),
                        )
                        .await?,
                ),
                EpicStoriesAction::Remove(args) => {
                    if !args.yes {
                        return Err(AppError::Usage("remove requires --yes".into()));
                    }
                    client
                        .delete_path(&format!(
                            "epics/{}/related_userstories/{}",
                            args.epic_id, args.story_id
                        ))
                        .await?;
                    emit(output, &json!({"deleted":true,"story_id":args.story_id}))
                }
            },
        },
        Command::Search(search) => {
            if search.text.trim().is_empty() {
                return Err(AppError::Usage("search text cannot be blank".into()));
            }
            if search.all_projects {
                let projects = client
                    .projects()
                    .list::<Value>(&[], PaginationMode::All)
                    .await?;
                let mut rows = Vec::new();
                for project in projects.items {
                    let id = project
                        .get("id")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| AppError::Usage("project list entry lacks id".into()))?;
                    rows.push(json!({"project":project,"results":client.search().project(ProjectId(id), &search.text).await?}));
                }
                return emit(output, &json!({"projects":rows}));
            }
            let project = if let Some(id) = search.project {
                ProjectId(id)
            } else if let Some(slug) = &search.project_slug {
                let project: Value = client
                    .get_path("projects/by_slug", &[("slug".into(), slug.clone())])
                    .await?;
                ProjectId(
                    project
                        .get("id")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| AppError::Usage("project lookup lacks id".into()))?,
                )
            } else {
                return Err(AppError::Usage(
                    "select --project, --project-slug, or --all-projects".into(),
                ));
            };
            emit(
                output,
                &client.search().project(project, &search.text).await?,
            )
        }
        Command::Notification(notification) => match &notification.action {
            NotificationAction::List { unread, page } => emit(
                output,
                &client
                    .notifications()
                    .list(*unread, pagination(*page)?)
                    .await?,
            ),
            NotificationAction::Count => emit(
                output,
                &json!({"count":client.notifications().unread_count().await?}),
            ),
            NotificationAction::Read(args) => {
                client.notifications().read(NotificationId(args.id)).await?;
                emit(output, &json!({"read":true,"id":args.id}))
            }
            NotificationAction::ReadAll => {
                client.notifications().read_all().await?;
                emit(output, &json!({"read_all":true}))
            }
        },
        Command::Timeline(timeline) => {
            let (kind, args) = match &timeline.action {
                TimelineAction::User(args) => ("user", args),
                TimelineAction::Profile(args) => ("profile", args),
                TimelineAction::Project(args) => ("project", args),
            };
            emit(
                output,
                &client
                    .timeline()
                    .list::<Value>(kind, args.id, args.relevant, pagination(args.page)?)
                    .await?,
            )
        }
        Command::Auth(_) => unreachable!(),
    }
}

/// Credentials usable for a silent re-login once both tokens are gone.
struct ReloginCredentials {
    username: String,
    password: SecretString,
    /// Whether the resulting tokens belong in the config file.
    persist: bool,
}

fn session_expired() -> AppError {
    AppError::Client(TaigaError::Unauthorized {
        message: "no valid session, run `taiga-cli auth login`".into(),
    })
}

async fn password_from_store(
    store: &PasswordStore,
    url: &str,
    username: &str,
) -> Option<SecretString> {
    let store = store.clone();
    let (url, username) = (url.to_owned(), username.to_owned());
    let lookup = tokio::task::spawn_blocking(move || store.get(&url, &username)).await;
    match lookup {
        Ok(Ok(password)) => password,
        Ok(Err(error)) => {
            eprintln!("warning: {error}");
            None
        }
        Err(error) => {
            eprintln!("warning: password lookup failed: {error}");
            None
        }
    }
}

/// Resolves credentials for re-login in priority order: `TAIGA_PASSWORD`
/// from the environment, the password saved with `login --remember`, and
/// finally an interactive prompt when stdin is a terminal.
async fn relogin_credentials(
    config: &Config,
    url: &str,
    access_from_environment: bool,
    allow_stored: bool,
) -> Option<ReloginCredentials> {
    let username = std::env::var("TAIGA_USERNAME")
        .ok()
        .or_else(|| config.username.clone())?;
    if let Ok(password) = std::env::var("TAIGA_PASSWORD") {
        return Some(ReloginCredentials {
            username,
            password: SecretString::from(password),
            persist: false,
        });
    }
    if access_from_environment || !allow_stored {
        return None;
    }
    if config.remember_password
        && let Some(password) =
            password_from_store(&PasswordStore::from_environment(), url, &username).await
    {
        return Some(ReloginCredentials {
            username,
            password,
            persist: true,
        });
    }
    if io::stdin().is_terminal() {
        let prompt = format!("Session expired. Taiga password for {username}: ");
        let password = rpassword::prompt_password(prompt).ok()?;
        return Some(ReloginCredentials {
            username,
            password: SecretString::from(password),
            persist: true,
        });
    }
    None
}

async fn run_authenticated(
    path: &PathBuf,
    url: &str,
    config: &mut Config,
    allow_stored: bool,
    command: &Command,
    output: Output,
) -> Result<(), AppError> {
    let environment_access = std::env::var("TAIGA_AUTH_TOKEN").ok();
    let access_from_environment = environment_access.is_some();
    let access_token = environment_access.or_else(|| config.auth_token.clone());
    let refresh_source = if let Ok(refresh) = std::env::var("TAIGA_REFRESH_TOKEN") {
        Some((refresh, false))
    } else if access_from_environment || !allow_stored {
        None
    } else {
        config.refresh_token.clone().map(|refresh| (refresh, true))
    };
    let mut last_error = None;

    // Stage 1: the current access token, unless its JWT `exp` already passed.
    // With no token at all the command still runs so argument validation
    // fails before any network access, as it always did.
    let access_usable = access_token
        .as_deref()
        .is_none_or(|token| !session::is_expired(token));
    if access_usable {
        let initial = client(url, access_token.as_deref())?;
        match execute_authenticated(&initial, config, command, output).await {
            Ok(()) => return Ok(()),
            Err(error @ AppError::Client(TaigaError::Unauthorized { .. })) => {
                if access_token.is_some() {
                    last_error = Some(error);
                }
            }
            Err(error) => return Err(error),
        }
    }

    // Stage 2: rotate through the refresh token, unless it is also expired.
    if let Some((refresh, persist)) =
        refresh_source.filter(|(refresh, _)| !session::is_expired(refresh))
    {
        let anonymous = client(url, None)?;
        match anonymous
            .auth()
            .refresh(&RefreshRequest {
                refresh: SecretString::from(refresh),
            })
            .await
        {
            Ok(tokens) => {
                if persist {
                    set_tokens(config, &tokens);
                    save_config(path, config)?;
                }
                let retry = client(url, Some(tokens.auth_token.expose_secret()))?;
                return execute_authenticated(&retry, config, command, output).await;
            }
            // Taiga answers a stale or revoked refresh token with a 4xx;
            // anything else (network, 5xx) is not worth a re-login attempt.
            Err(
                error @ (TaigaError::Unauthorized { .. }
                | TaigaError::Forbidden { .. }
                | TaigaError::ClientResponse { .. }),
            ) => last_error = Some(error.into()),
            Err(error) => return Err(error.into()),
        }
    }

    // Stage 3: a fresh login with stored or prompted credentials.
    let Some(credentials) =
        relogin_credentials(config, url, access_from_environment, allow_stored).await
    else {
        return Err(last_error.unwrap_or_else(session_expired));
    };
    let anonymous = client(url, None)?;
    let session = anonymous
        .auth()
        .login(&LoginRequest {
            username: credentials.username.clone(),
            password: credentials.password,
        })
        .await?;
    if credentials.persist {
        config.username = Some(credentials.username);
        set_tokens(config, &session.tokens);
        save_config(path, config)?;
    }
    let retry = client(url, Some(session.tokens.auth_token.expose_secret()))?;
    execute_authenticated(&retry, config, command, output).await
}

fn store_password(
    store: &PasswordStore,
    url: &str,
    username: &str,
    password: &SecretString,
) -> Result<(), AppError> {
    store
        .set(url, username, password)
        .map_err(|error| AppError::Config(format!("unable to save password: {error}")))
}

fn forget_password(store: &PasswordStore, config: &Config) -> Result<(), AppError> {
    let (Some(url), Some(username)) = (&config.api_url, &config.username) else {
        return Ok(());
    };
    store
        .delete(url, username)
        .map_err(|error| AppError::Config(format!("unable to remove saved password: {error}")))
}

async fn run(cli: Cli) -> Result<(), AppError> {
    let path = config_path()?;
    let mut config = load_config(&path)?;
    let api_overridden = cli.api_url.is_some() || std::env::var_os("TAIGA_API_URL").is_some();
    let url = api_url(&cli.api_url, &config);
    match &cli.command {
        Command::Auth(AuthCommand {
            action: AuthAction::Login(args),
        }) => {
            let username = args.username.clone().ok_or_else(|| {
                AppError::Config("username required via --username or TAIGA_USERNAME".into())
            })?;
            let password = if args.password_stdin {
                let mut text = String::new();
                io::stdin().read_to_string(&mut text)?;
                text.trim_end().to_owned()
            } else if let Ok(password) = std::env::var("TAIGA_PASSWORD") {
                password
            } else {
                rpassword::prompt_password("Taiga password: ")?
            };
            let password = SecretString::from(password);
            let anonymous = client(&url, None)?;
            let session = anonymous
                .auth()
                .login(&LoginRequest {
                    username: username.clone(),
                    password: password.clone(),
                })
                .await?;
            let store = PasswordStore::from_environment();
            let previous_username = config.username.replace(username.clone());
            if config.remember_password
                && let Some(previous) = previous_username
                && (previous != username || config.api_url.as_deref() != Some(url.as_str()))
            {
                // The saved entry belongs to another account or server.
                store
                    .delete(config.api_url.as_deref().unwrap_or(&url), &previous)
                    .ok();
                config.remember_password = false;
            }
            config.api_url = Some(url.clone());
            set_tokens(&mut config, &session.tokens);
            let remember = args.remember || config.remember_password;
            let stored = if remember {
                store_password(&store, &url, &username, &password)
            } else {
                Ok(())
            };
            config.remember_password = remember && stored.is_ok();
            save_config(&path, &config)?;
            stored?;
            emit(
                cli.output,
                &json!({
                    "id": session.id,
                    "username": session.username,
                    "api_url": config.api_url,
                    "password_stored": config.remember_password,
                }),
            )
        }
        Command::Auth(AuthCommand {
            action: AuthAction::Logout,
        }) => {
            let forgotten = if config.remember_password {
                forget_password(&PasswordStore::from_environment(), &config)
            } else {
                Ok(())
            };
            config.auth_token = None;
            config.refresh_token = None;
            config.username = None;
            config.remember_password = false;
            save_config(&path, &config)?;
            forgotten?;
            emit(cli.output, &json!({"logged_out":true}))
        }
        Command::Auth(AuthCommand {
            action: AuthAction::Forget,
        }) => {
            forget_password(&PasswordStore::from_environment(), &config)?;
            config.remember_password = false;
            save_config(&path, &config)?;
            emit(cli.output, &json!({"password_forgotten":true}))
        }
        _ => {
            run_authenticated(
                &path,
                &url,
                &mut config,
                !api_overridden,
                &cli.command,
                cli.output,
            )
            .await
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let code = match tokio::runtime::Runtime::new().unwrap().block_on(run(cli)) {
        Ok(()) => 0,
        Err(AppError::Usage(e)) => {
            eprintln!("{e}");
            2
        }
        Err(AppError::Config(e)) => {
            eprintln!("{e}");
            3
        }
        Err(AppError::Client(TaigaError::Conflict { .. })) => {
            eprintln!("optimistic concurrency conflict");
            6
        }
        Err(AppError::Client(TaigaError::RateLimited { .. })) => {
            eprintln!("Taiga rate limit reached");
            7
        }
        Err(e) => {
            eprintln!("{e}");
            5
        }
    };
    std::process::exit(code);
}
