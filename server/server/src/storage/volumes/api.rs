use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};

use super::service::{self, Ask, Holder};
use crate::{
    governance::context::FileCtx,
    kernel::{
        error::{Error, Result},
        http::Routes,
        state::Shared,
    },
};

fn ask(c: &FileCtx) -> Ask<'_> {
    Ask {
        identity: &c.0.identity,
        workspace: &c.0.workspace.id,
        org: c.0.org(),
        sides: &c.0.sides,
    }
}

fn param(c: &FileCtx, name: &str) -> Result<String> {
    c.0.params
        .get(name)
        .cloned()
        .ok_or_else(|| Error::Internal(format!("the route has no {name}")))
}

/// The same four calls for the two kinds of thing that hold files, a volume and a table. They differ only in the privileges asked
/// for (see `Holder`) and in the word in the address.
fn files(of: Holder) -> Routes {
    let base = format!("/api/v1/workspaces/{{ws}}/{}/{{id}}/files", of.route());
    let one = format!("{base}/{{*path}}");
    Routes::new()
        .get(
            &base,
            move |State(app): State<Shared>, c: FileCtx| async move {
                let items: Vec<Value> = service::list(&app, &ask(&c), of, &param(&c, "id")?)
                    .await?
                    .into_iter()
                    .map(|e| json!({"path": e.path, "size": e.size}))
                    .collect();
                Ok::<_, Error>(Json(json!({"items": items})))
            },
        )
        .put(
            &one,
            move |State(app): State<Shared>, c: FileCtx, body: Body| async move {
                let (id, path) = (param(&c, "id")?, param(&c, "path")?);
                let size = service::put(&app, &ask(&c), of, &id, &path, body).await?;
                Ok::<_, Error>((
                    StatusCode::CREATED,
                    Json(json!({"path": path, "size": size})),
                ))
            },
        )
        // Always sent as a download that the browser may not guess the type of: a file here is data, never a page.
        .get(
            &one,
            move |State(app): State<Shared>, c: FileCtx| async move {
                let (id, path) = (param(&c, "id")?, param(&c, "path")?);
                let (body, size) = service::get(&app, &ask(&c), of, &id, &path).await?;
                let mut res = Response::new(body);
                let h = res.headers_mut();
                h.insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/octet-stream"),
                );
                h.insert(
                    header::CONTENT_DISPOSITION,
                    HeaderValue::from_static("attachment"),
                );
                h.insert(
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static("nosniff"),
                );
                h.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
                Ok::<_, Error>(res)
            },
        )
        .delete(
            &one,
            move |State(app): State<Shared>, c: FileCtx| async move {
                let (id, path) = (param(&c, "id")?, param(&c, "path")?);
                service::delete(&app, &ask(&c), of, &id, &path).await?;
                Ok::<_, Error>(StatusCode::NO_CONTENT)
            },
        )
}

#[must_use]
pub fn routes() -> Routes {
    files(Holder::Volume).merge(files(Holder::Table))
}
