use std::path::Path;

use axum::Router;
use axum::http::header::{CACHE_CONTROL, HeaderValue};
use axum::middleware::map_response;
use axum::response::Response;
use tower_http::services::{ServeDir, ServeFile};

pub fn router(assets_dir: &Path) -> Router {
    let index = ServeFile::new(assets_dir.join("index.html"))
        .precompressed_br()
        .precompressed_gzip();
    let pages = ServeDir::new(assets_dir)
        .precompressed_br()
        .precompressed_gzip()
        .fallback(index);
    let hashed = ServeDir::new(assets_dir.join("assets"))
        .precompressed_br()
        .precompressed_gzip();
    Router::new()
        .nest_service("/assets", hashed)
        .layer(map_response(cache_immutable))
        .merge(
            Router::new()
                .fallback_service(pages)
                .layer(map_response(cache_revalidated)),
        )
}

// Build file names under assets/ carry a content hash, so a changed file gets a new name
async fn cache_immutable(mut response: Response) -> Response {
    if response.status().is_success() || response.status().is_redirection() {
        response.headers_mut().insert(
            CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    response
}

async fn cache_revalidated(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}
