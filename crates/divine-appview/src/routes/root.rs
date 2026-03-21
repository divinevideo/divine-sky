use axum::response::Html;

const ROOT_HTML: &str = include_str!("../root_page.html");

pub async fn root_info() -> Html<&'static str> {
    Html(ROOT_HTML)
}
