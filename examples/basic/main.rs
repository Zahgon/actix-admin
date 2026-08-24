extern crate serde_derive;

use actix_admin::prelude::*;
use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Extension;
use sea_orm::ConnectOptions;
use std::sync::Arc;
use std::time::Duration;
use tera::{Context, Tera};
mod entity;
use entity::{Comment, Post, User};

fn html(body: String) -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html")], body).into_response()
}

async fn profile(
    session: Session,
    Extension(tera): Extension<Arc<Tera>>,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
) -> Response {
    let mut ctx = Context::new();
    ctx.extend(get_admin_ctx(&session, &actix_admin).await);
    html(tera.render("profile.html", &ctx).unwrap())
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

async fn card(
    session: Session,
    Extension(tera): Extension<Arc<Tera>>,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Path(id): Path<i32>,
) -> Response {
    let mut ctx = Context::new();
    ctx.extend(get_admin_ctx(&session, &actix_admin).await);
    ctx.insert("id", &id);
    html(tera.render("card.html", &ctx).unwrap())
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
        base_path: "/admin",
        custom_css_paths: None,
        custom_js_paths: None,
        enable_csrf: true,
    };

    let mut admin_builder = ActixAdminBuilder::new(configuration);

    let mut post_view_model = ActixAdminViewModel::from(Post);
    post_view_model.inline_edit = true;
    // Per-view access control demo. These hooks are synchronous `fn(&Session)`
    // pointers, but tower-sessions reads are async, so the session claims the
    // actix-web version consulted (`edit_posts` / `delete_posts`) cannot be
    // read here. Neither claim is ever set anywhere in this example, so the
    // original `unwrap_or(true)` always yielded `true` and the demo behaves
    // identically.
    post_view_model.user_can_edit = Some(|_s: &Session| true);
    post_view_model.user_can_delete = Some(|_s: &Session| true);
    admin_builder.add_entity::<Post>(&post_view_model);

    // Register a custom bulk action on Post. The dispatcher is implemented
    // via `impl ActixAdminBulkActionDispatch for Post` in entity/post.rs.
    admin_builder.add_bulk_action_for_entity::<Post>(ActixAdminBulkAction {
        name: "mark_reviewed".to_string(),
        label: "Mark selected as reviewed".to_string(),
        icon: Some("fa-solid fa-check".to_string()),
        confirm: Some("Mark the selected posts as reviewed?".to_string()),
    });

    let some_category = "Group";
    let comment_view_model = ActixAdminViewModel::from(Comment);
    admin_builder.add_entity_to_category::<Comment>(&comment_view_model, some_category);
    let user_view_model = ActixAdminViewModel::from(User);
    admin_builder.add_entity_to_category::<User>(&user_view_model, some_category);

    let navbar_end_category = "navbar-end";
    admin_builder.add_custom_handler_to_category(
        "Profile",
        "/profile",
        get(profile),
        true,
        navbar_end_category,
    );

    let _support_route = admin_builder.add_support_handler("/support", get(support));
    let _card_route = admin_builder.add_custom_handler("card", "/card/{id}", get(card), false);

    let card_grid: Vec<Vec<String>> = vec![
        vec!["card/1".to_string(), "card/2".to_string()],
        vec!["card/3".to_string()],
    ];
    admin_builder.add_card_grid("Card Grid", "/my_card_grid", card_grid, true);

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
    let _ = entity::create_post_table(&conn).await;

    println!("The admin interface is available at http://localhost:5000/admin/");

    let actix_admin_builder = create_actix_admin_builder();
    let actix_admin = actix_admin_builder.get_actix_admin();

    // Start from actix-admin's tera (filters + templates) and layer
    // the example's own templates on top.
    let mut tera = actix_admin.tera.clone();
    tera.load_from_glob(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/basic/templates/*.html"
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
