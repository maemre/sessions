use std::error::Error;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use axum_login::{
    login_required, AuthManagerLayerBuilder, AuthUser, AuthnBackend,
};
use minijinja::{context, path_loader, Environment};
use password_auth::verify_password;
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, SqlitePool};
use tokio::task;
use tower_sessions::cookie::time::Duration;
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

// Data model

#[derive(Clone)]
struct Model {
    pool: SqlitePool,
}

impl Model {
    async fn get_counter(&self, user: &str) -> Result<Option<u64>> {
        Ok(
            sqlx::query_scalar::<_, u64>("select value from counter where user = ?")
                .bind(user)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    // Increment the counter, initialize it to 0 if it doesn't exist
    //
    // Returns the new counter value
    async fn increment_counter(&self, user: &str) -> Result<u64> {
        let mut trans = self.pool.begin().await?;
        let counter = sqlx::query_scalar::<_, i64>("select value from counter where user = ?")
            .bind(user)
            .fetch_optional(trans.as_mut())
            .await?;

        let new_value = if let Some(counter) = counter {
            sqlx::query("update counter set value = ? where user = ?")
                .bind(counter + 1)
                .bind(user)
                .execute(trans.as_mut())
                .await?;

            counter + 1
        } else {
            sqlx::query("insert into counter (user, value) values (?, 1)")
                .bind(user)
                .execute(trans.as_mut())
                .await?;

            1
        };

        trans.commit().await?;
        Ok(new_value as u64)
    }
}

// Authentication

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
struct User {
    name: String,
    pw_hash: String,
}

impl AuthUser for User {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.name.clone()
    }

    fn session_auth_hash(&self) -> &[u8] {
        // use pasword hash for session auth hash
        self.pw_hash.as_bytes()
    }
}

#[derive(Serialize, Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    TaskJoin(#[from] task::JoinError),
}

#[derive(Clone)]
struct AuthBackend {
    pool: SqlitePool,
}

impl AuthnBackend for AuthBackend {
    // the user type
    type User = User;

    // credentials used for authentication
    type Credentials = Credentials;

    type Error = AuthError;

    async fn authenticate(
        &self,
        credentials: Self::Credentials,
    ) -> std::result::Result<Option<Self::User>, Self::Error> {
        // fetch the user
        let user: Option<Self::User> = sqlx::query_as("select * from user where name = ? ")
            .bind(credentials.username)
            .fetch_optional(&self.pool)
            .await?;

        // compare the hash against the password
        // using `spawn_blocking` because hashing is slow
        task::spawn_blocking(|| {
            Ok(user.filter(|user| verify_password(credentials.password, &user.pw_hash).is_ok()))
        })
        .await?
    }

    async fn get_user(
        &self,
        user_id: &axum_login::UserId<Self>,
    ) -> std::result::Result<Option<Self::User>, Self::Error> {
        Ok(sqlx::query_as("select * from user where name = ? ")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?)
    }
}

type AuthSession = axum_login::AuthSession<AuthBackend>;

// App state

/// Everything handlers need: the data store **and** the template engine.
///
/// We wrap the Environment in an Arc so it can be shared cheaply across
/// tasks.  It's immutable after setup, so no Mutex needed.
#[derive(Clone)]
struct AppState {
    model: Model,
    templates: Arc<Environment<'static>>,
}

// Create a database error response
fn database_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Html("database error".to_string()),
    )
        .into_response()
}

// Templating

/// Builds the MiniJinja environment with all our templates.
fn build_templates() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_loader(path_loader("templates"));
    env
}

/// Renders a template or returns a 500 error page.
///
/// Centralises the boilerplate of "get template → render → wrap in Html".
fn render(env: &Environment, name: &str, ctx: minijinja::Value) -> Response {
    match env.get_template(name).and_then(|t| t.render(ctx)) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            eprintln!("template error: {e:#}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("template error".to_string()),
            )
                .into_response()
        }
    }
}

// Handlers

/// GET /
async fn get_counter(session: AuthSession, State(state): State<AppState>) -> impl IntoResponse {
    let Some(user) = session.user else {
        return Redirect::to("/login").into_response();
    };

    let Ok(counter) = state.model.get_counter(&user.name).await else {
        return database_error();
    };

    render(&state.templates, "counter.html", context! { counter })
}

/// GET /increment
async fn increment_counter(session: AuthSession, State(state): State<AppState>) -> impl IntoResponse {
    let Some(user) = session.user else {
        return Redirect::to("/login").into_response();
    };

    let Ok(counter) = state.model.increment_counter(&user.name).await else {
        return database_error();
    };

    render(&state.templates, "counter.html", context! { counter })
}

async fn build_router(state: AppState) -> Router {
    // session layer
    let session_store = SqliteStore::new(state.model.pool.clone());
    session_store
        .migrate()
        .await
        .expect("could not migrate the session DB");
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false) // to allow transmitting the cookie over HTTP
        .with_expiry(Expiry::OnInactivity(Duration::minutes(1)));

    // auth service
    let backend = AuthBackend {
        pool: state.model.pool.clone(),
    };
    let auth_layer = AuthManagerLayerBuilder::new(backend, session_layer).build();

    Router::new()
        .route("/", get(get_counter))
        .route("/increment", get(increment_counter))
        .route_layer(login_required!(AuthBackend, login_url = "/login"))
        .with_state(state)
        .layer(auth_layer)
}

// Main

#[tokio::main]
async fn main() {
    let pool = SqlitePool::connect("sqlite:bookmarks.db?mode=rwc")
        .await
        .expect("Cannot connect to the database");
    sqlx::raw_sql(include_str!("../schema.sql"))
        .execute(&pool)
        .await
        .expect("Cannot create the schema");

    let state = AppState {
        model: Model { pool },
        templates: Arc::new(build_templates()),
    };

    let app = build_router(state).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("failed to bind port 8080");

    println!("Open http://127.0.0.1:8080/ in your browser");
    axum::serve(listener, app).await.expect("server error");
}
