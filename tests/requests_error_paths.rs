//! Regression tests for the Phase 0-3 error-handling improvements.
//!
//! These verify that malformed requests produce proper HTTP error codes
//! rather than panicking the handler (which previously returned 500 or,
//! worse, killed the actix worker).

mod test_setup;

use test_setup::prelude::*;

#[cfg(test)]
mod error_paths {
    use super::create_app;
    use super::{form_request, request};
    use actix_admin::prelude::*;
    use tower::ServiceExt;

    /// Unknown sort column used to `panic!("Unknown column")` inside the
    /// generated `list_model`; we now return 400 Bad Request from the route.
    #[tokio::test(flavor = "multi_thread")]
    async fn unknown_sort_column_returns_400() {
        let db = super::setup_db(false).await;
        let app = create_app!(&db, false, None, false);

        let req = request("GET", "/admin/post/list?sort_by=totally_not_a_column");
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 400);
    }

    /// GET /show/{id} for a missing entity should be 404, not 500.
    #[tokio::test(flavor = "multi_thread")]
    async fn show_missing_entity_returns_404() {
        let db = super::setup_db(false).await;
        let app = create_app!(&db, false, None, false);

        let req = request("GET", "/admin/post/show/99999");
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 404);
    }

    /// GET /edit/{id} for a missing entity should be 404, not 500.
    #[tokio::test(flavor = "multi_thread")]
    async fn edit_missing_entity_returns_404() {
        let db = super::setup_db(false).await;
        let app = create_app!(&db, false, None, false);

        let req = request("GET", "/admin/post/edit/99999");
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 404);
    }

    /// DELETE /file/{id}/{column} where `column` is not a FileUpload field must
    /// not panic and not disclose non-file column values.
    #[tokio::test(flavor = "multi_thread")]
    async fn delete_file_rejects_non_file_column() {
        let db = super::setup_db(true).await;
        let app = create_app!(&db, false, None, false);

        let req = request("DELETE", "/admin/post/file/1/title");
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 400);
    }

    /// GET /file/{id}/{column} where `column` is not a FileUpload field must
    /// return 400 (previously silently attempted a file lookup).
    #[tokio::test(flavor = "multi_thread")]
    async fn download_rejects_non_file_column() {
        let db = super::setup_db(true).await;
        let app = create_app!(&db, false, None, false);

        let req = request("GET", "/admin/post/file/1/title");
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 400);
    }

    /// Bulk delete used to `.unwrap()` on `parse::<i32>` — a malformed id in
    /// the form body would panic. Now it is silently skipped.
    #[tokio::test(flavor = "multi_thread")]
    async fn delete_many_ignores_unparseable_ids() {
        let db = super::setup_db(true).await;
        let app = create_app!(&db, false, None, false);

        let req = form_request(
            "DELETE",
            "/admin/post/delete",
            [("ids", "not-a-number"), ("ids", "also-not")],
        );
        let resp = app.oneshot(req).await.unwrap();
        // With zero valid ids and no errors, we get a redirect back to /list.
        assert!(
            resp.status().is_redirection() || resp.status().is_success(),
            "unexpected status: {}",
            resp.status()
        );
    }

    /// Garbage in the querystring must not panic; the route should ignore
    /// unknown/unparseable params and render normally.
    #[tokio::test(flavor = "multi_thread")]
    async fn list_ignores_garbage_query_string() {
        let db = super::setup_db(true).await;
        let app = create_app!(&db, false, None, false);

        let req = request("GET", "/admin/post/list?page=not_a_number&entities_per_page=abc");
        let resp = app.oneshot(req).await.unwrap();
        // Params::from_query is infallible now, so this should render.
        assert!(resp.status().is_success());
    }
}
