mod test_setup;
use test_setup::prelude::*;

#[cfg(test)]
mod csrf_seam {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test(flavor = "multi_thread")]
    async fn csrf_checked_before_multipart_body_on_create() {
        let db = setup_db(false).await;
        let app = create_csrf_admin_router(db);

        let req = form_request("POST", "/admin/post/create", [("x", "1")]);
        let resp = app.oneshot(req).await.unwrap();

        assert_eq!(
            resp.status().as_u16(),
            403,
            "a non-multipart write POST with CSRF enabled must be rejected by the CSRF check (403), not by the multipart body parser (400)"
        );
    }
}
