extern crate serde_derive;

use actix_admin::prelude::*;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Form};
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use sea_orm::ConnectOptions;
use std::sync::Arc;
use std::time::Duration;
use tera::{Context, Tera};

fn html(body: String) -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html")], body).into_response()
}

async fn support(
    session: Session,
    Extension(tera): Extension<Arc<Tera>>,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
) -> Response {
    let mut ctx = Context::new();
    ctx.extend(get_admin_ctx(&session, &actix_admin).await);
    html(tera.render("support.html", &ctx).unwrap())
}

#[derive(serde::Deserialize)]
struct SupportForm {
    question: String,
    context: String,
}

async fn support_post(
    session: Session,
    Extension(tera): Extension<Arc<Tera>>,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Form(form): Form<SupportForm>, // Add this parameter to extract form data
) -> Response {
    let ollama = Ollama::default();
    let model = "llama3.1".to_string();
    // naive context, better use GenerationContext
    let prompt = format!("Context: {} Question: {}", form.context, form.question);
    println!("{}", prompt);
    let request = GenerationRequest::new(model, prompt);
    let res = ollama.generate(request).await;

    if let Ok(res) = res {
        let mut ctx = Context::new();
        ctx.extend(get_admin_ctx(&session, &actix_admin).await);
        ctx.insert("answer", res.response.as_str());
        html(tera.render("chat_answer.html", &ctx).unwrap())
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed generating answer",
        )
            .into_response()
    }
}

async fn custom_index(
    session: Session,
    Extension(tera): Extension<Arc<Tera>>,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
) -> Response {
    let ctx = get_admin_ctx(&session, &actix_admin).await;
    html(tera.render("custom_index.html", &ctx).unwrap())
}

fn create_actix_admin_builder() -> ActixAdminBuilder {
    let configuration = ActixAdminConfiguration {
        enable_auth: true,
        user_is_logged_in: Some(|_session: &Session| -> bool { true }),
        login_link: None,
        logout_link: None,
        file_upload_directory: "./file_uploads",
        navbar_title: "ActixAdmin Example",
        user_tenant_ref: None,
        base_path: "/absproxy/5000/admin",
        custom_css_paths: None,
        custom_js_paths: None,
        enable_csrf: false,
    };

    let mut admin_builder = ActixAdminBuilder::new(configuration);

    let _support_route = admin_builder.add_support_handler("/support", get(support));
    let _support_route_post = admin_builder.add_support_handler("/support", post(support_post));
    let _custom_index = admin_builder.add_custom_handler_for_index(get(custom_index));

    admin_builder
}

fn get_db_options() -> ConnectOptions {
    let db_url = "sqlite::memory:".to_string();
    let mut opt = ConnectOptions::new(db_url);
    opt.max_connections(100)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(8))
        .sqlx_logging(true);
    opt
}

#[tokio::main]
async fn main() {
    let opt = get_db_options();
    let conn: sea_orm::DatabaseConnection = sea_orm::Database::connect(opt).await.unwrap();

    println!("The admin interface is available at http://localhost:5000/absproxy/5000/admin");

    let actix_admin_builder = create_actix_admin_builder();
    let actix_admin = actix_admin_builder.get_actix_admin();

    // Start from actix-admin's tera (filters + templates) and layer
    // the example's own templates on top.
    let mut tera = actix_admin.tera.clone();
    tera.load_from_glob(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/chat_support/templates/*.html"
    ))
    .unwrap();

    let app = actix_admin_builder
        .get_scope()
        .layer(Extension(Arc::new(tera)))
        .layer(Extension(Arc::new(actix_admin)))
        .layer(Extension(conn))
        .layer(tower_sessions::SessionManagerLayer::new(
            tower_sessions::MemoryStore::default(),
        ))
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:5000")
        .await
        .expect("Can not bind to port 5000");
    axum::serve(listener, app).await.unwrap();
}
