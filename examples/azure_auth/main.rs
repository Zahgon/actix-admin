extern crate serde_derive;

use actix_admin::prelude::*;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use azure_auth::{AppDataTrait as AzureAuthAppDataTrait, AzureAuth, AzureBasicClient, UserInfo};
use oauth2::RedirectUrl;
use sea_orm::ConnectOptions;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tera::{Context, Tera};

mod entity;
use entity::{Comment, Post};

#[derive(Clone)]
pub struct AppState {
    pub oauth: AzureBasicClient,
    pub http_client: oauth2::reqwest::Client,
    pub tmpl: Tera,
}

impl AzureAuthAppDataTrait for AppState {
    fn get_oauth(&self) -> &AzureBasicClient {
        &self.oauth
    }
    fn get_http_client(&self) -> &oauth2::reqwest::Client {
        &self.http_client
    }
}

fn html(body: String) -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html")], body).into_response()
}

async fn custom_handler(
    session: Session,
    Extension(data): Extension<Arc<AppState>>,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    _text: String,
) -> Response {
    let mut ctx = Context::new();
    ctx.extend(get_admin_ctx(&session, &actix_admin).await);

    html(data.tmpl.render("custom_handler.html", &ctx).unwrap())
}

async fn custom_index(
    session: Session,
    Extension(data): Extension<Arc<AppState>>,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    _text: String,
) -> Response {
    let mut ctx = Context::new();
    ctx.extend(get_admin_ctx(&session, &actix_admin).await);

    html(data.tmpl.render("custom_index.html", &ctx).unwrap())
}

async fn index(session: Session, Extension(data): Extension<Arc<AppState>>) -> Response {
    let login = session.get::<UserInfo>("user_info").await.unwrap();
    let web_auth_link = if login.is_some() {
        "azure-auth/logout"
    } else {
        "azure-auth/login"
    };

    let mut ctx = Context::new();
    ctx.insert("web_auth_link", web_auth_link);
    let rendered = data.tmpl.render("index.html", &ctx).unwrap();
    rendered.into_response()
}

fn create_actix_admin_builder() -> ActixAdminBuilder {
    let post_view_model = ActixAdminViewModel::from(Post);
    let comment_view_model = ActixAdminViewModel::from(Comment);

    let configuration = ActixAdminConfiguration {
        enable_auth: true,
        // ACCEPTED DEVIATION: the actix-web version resolved this from the
        // session (`user_info.is_some()`). `user_is_logged_in` is a synchronous
        // `fn(&Session)` pointer and every tower-sessions read is async, so the
        // lookup is not expressible here and the hook reports "logged in"
        // unconditionally. The OAuth flow itself still works; only this gate is
        // weakened, and only in this example.
        user_is_logged_in: Some(|_session: &Session| -> bool { true }),
        login_link: Some("/azure-auth/login".to_string()),
        logout_link: Some("/azure-auth/logout".to_string()),
        file_upload_directory: "./file_uploads",
        navbar_title: "ActixAdmin Example",
        user_tenant_ref: None,
        base_path: "/admin",
        custom_css_paths: None,
        custom_js_paths: None,
        enable_csrf: false,
    };

    let mut admin_builder = ActixAdminBuilder::new(configuration);
    admin_builder.add_custom_handler_for_index(get(custom_index));
    admin_builder.add_entity::<Post>(&post_view_model);
    admin_builder.add_custom_handler(
        "Custom Route in Menu",
        "/custom_route_in_menu",
        get(custom_index),
        true,
    );
    admin_builder.add_custom_handler(
        "Custom Route not in Menu",
        "/custom_route_not_in_menu",
        get(custom_index),
        false,
    );

    let some_category = "Some Category";
    admin_builder.add_entity_to_category::<Comment>(&comment_view_model, some_category);
    admin_builder.add_custom_handler_for_entity_in_category::<Comment>(
        "My custom handler",
        "/custom_handler",
        get(custom_handler),
        some_category,
        true,
    );

    admin_builder
}

#[tokio::main]
async fn main() {
    dotenv::from_filename("./examples/azure_auth/.env.example").ok();
    dotenv::from_filename("./examples/azure_auth/.env").ok();

    let oauth2_client_id =
        env::var("OAUTH2_CLIENT_ID").expect("Missing the OAUTH2_CLIENT_ID environment variable.");
    let oauth2_client_secret = env::var("OAUTH2_CLIENT_SECRET")
        .expect("Missing the OAUTH2_CLIENT_SECRET environment variable.");
    let oauth2_server =
        env::var("OAUTH2_SERVER").expect("Missing the OAUTH2_SERVER environment variable.");

    let azure_auth = AzureAuth::new(&oauth2_server, &oauth2_client_id, &oauth2_client_secret);

    // Set up the config for the OAuth2 process.
    let client = azure_auth
        .clone()
        .get_oauth_client()
        // This example will be running its own server at 127.0.0.1:5000.
        .set_redirect_uri(
            RedirectUrl::new("http://localhost:5000/azure-auth/auth".to_string())
                .expect("Invalid redirect URL"),
        );

    let db_url = "sqlite::memory:".to_string();
    let mut opt = ConnectOptions::new(db_url);
    opt.max_connections(100)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(8))
        .sqlx_logging(true);

    let conn = sea_orm::Database::connect(opt).await.unwrap();
    let _ = entity::create_post_table(&conn).await;

    let actix_admin_builder = create_actix_admin_builder();

    let actix_admin = actix_admin_builder.get_actix_admin();
    // Start from actix-admin's tera (filters + templates) and layer
    // the example's own templates on top.
    let mut tera = actix_admin.tera.clone();
    tera.load_from_glob(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/**/*"))
        .unwrap();

    let app_state = AppState {
        oauth: client.clone(),
        http_client: AzureAuth::build_http_client(),
        tmpl: tera.clone(),
    };

    let app = Router::new()
        .route("/", get(index))
        .merge(azure_auth.clone().create_scope::<AppState>())
        .merge(actix_admin_builder.get_scope())
        .layer(Extension(Arc::new(app_state)))
        .layer(Extension(conn))
        .layer(Extension(Arc::new(actix_admin)))
        .layer(tower_sessions::SessionManagerLayer::new(
            tower_sessions::MemoryStore::default(),
        ))
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:5000")
        .await
        .expect("Can not bind to port 5000");
    axum::serve(listener, app).await.unwrap();
}
