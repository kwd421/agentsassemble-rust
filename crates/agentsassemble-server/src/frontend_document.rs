use std::{io, path::Path, sync::Arc};

use lol_html::{RewriteStrSettings, element, rewrite_str};

pub(crate) fn render(root: &Path, build_id: &str) -> io::Result<Arc<str>> {
    let source = std::fs::read_to_string(root.join("index.html"))?;
    let base = url::Url::from_directory_path(root)
        .map_err(|()| io::Error::other("frontend release path is invalid"))?;
    let mut bound_document = false;
    let mut asset_count = 0;
    let html = rewrite_str(
        &source,
        RewriteStrSettings::new()
            .append_element_content_handler(element!("html", |element| {
                element.set_attribute("data-agentsassemble-build", build_id)?;
                element.set_attribute(
                    "data-agentsassemble-protocol",
                    &agentsassemble_protocol::PROTOCOL_VERSION.to_string(),
                )?;
                bound_document = true;
                Ok(())
            }))
            .append_element_content_handler(element!(
                "script[src], link[rel=stylesheet][href], link[rel=modulepreload][href]",
                |element| {
                    let attribute = if element.tag_name() == "script" {
                        "src"
                    } else {
                        "href"
                    };
                    let value = element
                        .get_attribute(attribute)
                        .ok_or_else(|| io::Error::other("frontend asset reference is missing"))?;
                    let relative = if let Some(value) = value.strip_prefix("./") {
                        value
                    } else if let Some(value) = value.strip_prefix("/app/") {
                        value
                    } else {
                        value.strip_prefix('/').unwrap_or(&value)
                    };
                    if !relative.starts_with("assets/") {
                        return Err(
                            io::Error::other("frontend asset is outside the release").into()
                        );
                    }
                    let asset = base.join(relative)?;
                    let path = asset
                        .to_file_path()
                        .map_err(|()| io::Error::other("frontend asset path is invalid"))?;
                    if !path.starts_with(root.join("assets")) || !path.is_file() {
                        return Err(
                            io::Error::other("frontend build references a missing asset").into(),
                        );
                    }
                    let suffix = asset[url::Position::BeforePath..]
                        .strip_prefix(base.path())
                        .ok_or_else(|| io::Error::other("frontend asset escaped the release"))?;
                    element.set_attribute(
                        attribute,
                        &format!("/frontend-builds/{build_id}/{suffix}"),
                    )?;
                    asset_count += 1;
                    Ok(())
                }
            )),
    )
    .map_err(|_| io::Error::other("frontend document cannot be bound to its release"))?;
    if !bound_document || asset_count == 0 {
        return Err(io::Error::other("frontend document has no build entry"));
    }
    Ok(Arc::from(html))
}
