use actix_admin::prelude::*;
use axum::body::{Body, Bytes};
use axum::extract::{Path, RawQuery};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Extension;
use chrono::Local;
use std::sync::Arc;
use sea_orm::prelude::Decimal;
use sea_orm::{ConnectOptions, DatabaseConnection, EntityTrait, Set};

use super::sample_with_tenant_id;
use super::SampleWithTenantId;
use super::{comment, create_tables, post, Comment, Post};

pub async fn setup_db(create_entities: bool) -> DatabaseConnection {
    let opt = ConnectOptions::new("sqlite::memory:".to_owned());

    let db = sea_orm::Database::connect(opt).await.unwrap();
    let _ = create_tables(&db).await;

    if create_entities {
        for i in 1..1000 {
            let row = post::ActiveModel {
                title: Set(format!("Test {}", i)),
                text: Set("some content".to_string()),
                tea_mandatory: Set(post::Tea::EverydayTea),
                tea_optional: Set(None),
                insert_date: Set(Local::now().date_naive()),
                // Cover every branch of the new nullable renderers:
                //   * some rows are all-NULL (i % 5 == 0)
                //   * other rows have populated values including a
                //     characteristic marker so we can grep the response.
                summary_html: Set(if i % 5 == 0 {
                    None
                } else {
                    Some(format!("<em>row-{}</em>", i))
                }),
                homepage: Set(if i % 5 == 0 {
                    None
                } else {
                    Some(format!("https://example.com/{}", i))
                }),
                contact_email: Set(if i % 5 == 0 {
                    None
                } else {
                    Some(format!("row{}@example.com", i))
                }),
                cover_image: Set(if i % 3 == 0 {
                    Some("placeholder.png".to_string())
                } else {
                    None
                }),
                notes_md: Set(if i % 5 == 0 {
                    None
                } else {
                    Some(format!("# markdown-{}", i))
                }),
                external_id: Set(Some(format!("EXT-{:05}", i))),
                ..Default::default()
            };
            let insert_res = Post::insert(row)
                .exec(&db)
                .await
                .expect("could not insert post");

            let row = comment::ActiveModel {
                comment: Set(format!("Test {}", i)),
                user: Set("me@home.com".to_string()),
                my_decimal: Set(Decimal::new(105, 0)),
                insert_date: Set(Local::now().naive_utc()),
                is_visible: Set(i % 2 == 0),
                post_id: Set(Some(insert_res.last_insert_id as i32)),
                ..Default::default()
            };
            let _res = Comment::insert(row)
                .exec(&db)
                .await
                .expect("could not insert comment");

            let row = sample_with_tenant_id::ActiveModel {
                title: Set(format!("TestTenant{}", i % 2)),
                text: Set("me@home.com".to_string()),
                tenant_id: Set(i % 2),
                ..Default::default()
            };
            let _res = SampleWithTenantId::insert(row)
                .exec(&db)
                .await
                .expect("could not insert sample with tenant id");
        }
    }

    db
}

/// Wrap an admin `Router` with the layers the library expects: `ActixAdmin`
/// and the `DatabaseConnection` as `Extension`s (the axum equivalent of
/// actix-web's `app_data`) plus a session store, which the CSRF and auth
/// paths read from.
pub fn wrap_admin_router(
    router: axum::Router,
    actix_admin: ActixAdmin,
    conn: sea_orm::DatabaseConnection,
) -> axum::Router {
    router
        .layer(Extension(Arc::new(actix_admin)))
        .layer(Extension(conn))
        .layer(tower_sessions::SessionManagerLayer::new(
            tower_sessions::MemoryStore::default(),
        ))
}

#[macro_export]
macro_rules! create_app (
    ($db: expr, $enable_auth: expr, $tenant_ref: expr, $enable_inline_editing: expr) => ({
        let conn = $db.clone();
        let actix_admin_builder = super::create_actix_admin_builder($enable_auth, $tenant_ref, $enable_inline_editing);
        let actix_admin = actix_admin_builder.get_actix_admin();

        $crate::test_setup::helper::wrap_admin_router(
            actix_admin_builder.get_scope(),
            actix_admin,
            conn,
        )
    });
);

#[macro_export]
macro_rules! create_server (
    ($db: expr, $enable_auth: expr, $tenant_ref: expr, $enable_inline_editing: expr) => ({
        let conn = $db.clone();
        let actix_admin_builder = create_actix_admin_builder($enable_auth, $tenant_ref, $enable_inline_editing);
        let actix_admin = actix_admin_builder.get_actix_admin();

        let app = $crate::test_setup::helper::wrap_admin_router(
            actix_admin_builder.get_scope(),
            actix_admin,
            conn,
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:5555").await.unwrap();
        axum::serve(listener, app).await.expect("Failed to run server");
    });
);

pub fn request(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

/// Sets `Content-Type: application/x-www-form-urlencoded`, which axum's `Form`
/// extractor requires on non-GET requests.
pub fn form_request_raw(method: &str, uri: &str, body: String) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap()
}

pub fn form_request(method: &str, uri: &str, form: impl serde::Serialize) -> Request<Body> {
    form_request_raw(method, uri, serde_urlencoded::to_string(form).unwrap())
}

pub async fn read_body(resp: Response) -> Bytes {
    axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
}

pub fn create_actix_admin_builder(
    enable_auth: bool,
    tenant_ref: Option<for<'a> fn(&'a Session) -> Option<i32>>,
    enable_inline_editing: bool,
) -> ActixAdminBuilder {
    let mut post_view_model = ActixAdminViewModel::from(Post);
    post_view_model.inline_edit = enable_inline_editing;
    let comment_view_model = ActixAdminViewModel::from(Comment);
    let sample_with_tenant_id_view_model = ActixAdminViewModel::from(SampleWithTenantId);

    let configuration = ActixAdminConfiguration {
        enable_auth: enable_auth,
        user_tenant_ref: tenant_ref,
        user_is_logged_in: None,
        login_link: None,
        logout_link: None,
        file_upload_directory: "./file_uploads",
        navbar_title: "test",
        base_path: "/admin",
        custom_css_paths: None,
        custom_js_paths: None,
        enable_csrf: false,
    };

    let mut admin_builder = ActixAdminBuilder::new(configuration);
    admin_builder.add_entity::<Post>(&post_view_model);
    admin_builder.add_entity::<Comment>(&comment_view_model);
    admin_builder.add_entity::<SampleWithTenantId>(&sample_with_tenant_id_view_model);

    admin_builder.add_custom_handler_for_entity::<Comment>(
        "Create Comment From Plaintext",
        "/create_post_from_plaintext",
        post(create_post_from_plaintext::<Comment>),
        false,
    );

    admin_builder.add_custom_handler_for_entity::<Post>(
        "Create Post From Plaintext",
        "/create_post_from_plaintext",
        post(create_post_from_plaintext::<Post>),
        false,
    );

    admin_builder.add_custom_handler_for_entity::<SampleWithTenantId>(
        "Create Sample With Tenant Id From Plaintext",
        "/create_post_from_plaintext",
        post(create_post_from_plaintext::<SampleWithTenantId>),
        false,
    );

    admin_builder.add_custom_handler_for_entity::<Post>(
        "Edit Post From Plaintext",
        "/edit_post_from_plaintext/{id}",
        post(edit_post_from_plaintext::<Post>),
        false,
    );

    admin_builder.add_custom_handler_for_entity::<Comment>(
        "Edit Comment From Plaintext",
        "/edit_post_from_plaintext/{id}",
        post(edit_post_from_plaintext::<Comment>),
        false,
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

async fn create_post_from_plaintext<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
    text: String,
) -> Result<Response, ActixAdminError> {
    run_local(async move {
        let model = ActixAdminModel::from(text);
        create_or_edit_post::<E>(
            &session,
            &headers,
            &raw_query.unwrap_or_default(),
            &db,
            Ok(model),
            None::<E::Id>,
            &actix_admin,
        )
        .await
    })
}

async fn edit_post_from_plaintext<E: ActixAdminViewModelTrait>(
    session: Session,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Extension(actix_admin): Extension<Arc<ActixAdmin>>,
    Extension(db): Extension<DatabaseConnection>,
    Path(id): Path<E::Id>,
    text: String,
) -> Result<Response, ActixAdminError> {
    run_local(async move {
        let model = ActixAdminModel::from(text);
        create_or_edit_post::<E>(
            &session,
            &headers,
            &raw_query.unwrap_or_default(),
            &db,
            Ok(model),
            Some(id),
            &actix_admin,
        )
        .await
    })
}

async fn support() -> Response {
    let resp = "<div id=\"support_content\">SupportDiv</div>";
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html")], resp).into_response()
}

async fn card(Path(id): Path<i32>) -> Response {
    let resp = format!("<div class=\"card-content\">Card{}</div>", id);
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html")], resp).into_response()
}
/// Builds an admin router with CSRF **enabled** and a single `Post` entity,
/// wrapped with the session store. Used by the CSRF seam test to assert that a
/// non-multipart write POST is rejected by the CSRF check (403) rather than by
/// the multipart body parser (400).
pub fn create_csrf_admin_router(conn: DatabaseConnection) -> axum::Router {
    let post_view_model = ActixAdminViewModel::from(Post);

    let configuration = ActixAdminConfiguration {
        enable_auth: false,
        user_tenant_ref: None,
        user_is_logged_in: None,
        login_link: None,
        logout_link: None,
        file_upload_directory: "./file_uploads",
        navbar_title: "test",
        base_path: "/admin",
        custom_css_paths: None,
        custom_js_paths: None,
        enable_csrf: true,
    };

    let mut admin_builder = ActixAdminBuilder::new(configuration);
    admin_builder.add_entity::<Post>(&post_view_model);
    let actix_admin = admin_builder.get_actix_admin();

    wrap_admin_router(admin_builder.get_scope(), actix_admin, conn)
}

pub trait BodyTest {
    #[allow(dead_code)]
    fn as_str(&self) -> &str;
}

impl BodyTest for Bytes {
    fn as_str(&self) -> &str {
        std::str::from_utf8(self).unwrap()
    }
}
