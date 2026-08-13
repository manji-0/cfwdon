use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    embed_ui_bundle(
        &manifest_dir,
        &out_dir,
        UiBundleSpec {
            dist_relative: "../../admin-ui/dist",
            fallback_relative: "admin_ui_fallback/index.html",
            embed_subdir: "admin_ui_embed",
            path_prefix: "/admin/",
            index_web_path: "/admin/",
            output_module: "admin_ui_assets.rs",
            lookup_fn: "lookup_admin_embedded_asset",
            normalize_fn: "normalize_admin_asset_path",
            normalize_from: "/admin",
            normalize_to: "/admin/",
        },
    );

    embed_ui_bundle(
        &manifest_dir,
        &out_dir,
        UiBundleSpec {
            dist_relative: "../../web-ui/dist",
            fallback_relative: "web_ui_fallback/index.html",
            embed_subdir: "web_ui_embed",
            path_prefix: "/app/",
            index_web_path: "/app/",
            output_module: "web_ui_assets.rs",
            lookup_fn: "lookup_web_embedded_asset",
            normalize_fn: "normalize_web_asset_path",
            normalize_from: "/app",
            normalize_to: "/app/",
        },
    );
}

struct UiBundleSpec<'a> {
    dist_relative: &'a str,
    fallback_relative: &'a str,
    embed_subdir: &'a str,
    path_prefix: &'a str,
    index_web_path: &'a str,
    output_module: &'a str,
    lookup_fn: &'a str,
    normalize_fn: &'a str,
    normalize_from: &'a str,
    normalize_to: &'a str,
}

fn embed_ui_bundle(manifest_dir: &Path, out_dir: &Path, spec: UiBundleSpec<'_>) {
    let dist = manifest_dir.join(spec.dist_relative);
    let embed_dir = out_dir.join(spec.embed_subdir);
    let fallback = manifest_dir.join(spec.fallback_relative);

    println!("cargo:rerun-if-changed={}", spec.fallback_relative);
    if dist.exists() {
        println!("cargo:rerun-if-changed={}", spec.dist_relative);
    }

    fs::create_dir_all(&embed_dir).expect("create embed dir");

    let mut entries = Vec::new();
    if dist.exists() {
        collect_dist_files(
            &dist,
            &dist,
            &embed_dir,
            spec.path_prefix,
            spec.index_web_path,
            &mut entries,
        );
    } else {
        let target = embed_dir.join("index.html");
        fs::copy(&fallback, &target).expect("copy fallback index.html");
        entries.push(EmbedEntry {
            web_path: spec.index_web_path.to_owned(),
            file_name: "index.html".to_owned(),
            content_type: "text/html; charset=utf-8".to_owned(),
        });
    }

    write_generated_module(
        out_dir,
        spec.output_module,
        spec.embed_subdir,
        spec.lookup_fn,
        spec.normalize_fn,
        spec.normalize_from,
        spec.normalize_to,
        &entries,
    );
}

struct EmbedEntry {
    web_path: String,
    file_name: String,
    content_type: String,
}

fn collect_dist_files(
    dist_root: &Path,
    current: &Path,
    embed_dir: &Path,
    path_prefix: &str,
    index_web_path: &str,
    entries: &mut Vec<EmbedEntry>,
) {
    for entry in fs::read_dir(current).expect("read dist directory") {
        let entry = entry.expect("read dist entry");
        let path = entry.path();
        if path.is_dir() {
            collect_dist_files(
                dist_root,
                &path,
                embed_dir,
                path_prefix,
                index_web_path,
                entries,
            );
            continue;
        }

        let relative = path.strip_prefix(dist_root).expect("path inside dist root");
        let relative_str = relative.to_string_lossy().replace('\\', "/");
        let web_path = if relative_str == "index.html" {
            index_web_path.to_owned()
        } else {
            format!("{path_prefix}{relative_str}")
        };
        let file_name = format!("file_{}", relative_str.replace(['/', '.', '-'], "_"));
        let target = embed_dir.join(&file_name);
        fs::copy(&path, &target).expect("copy embedded asset");
        entries.push(EmbedEntry {
            web_path,
            file_name,
            content_type: content_type_for_path(&relative_str),
        });
    }
}

fn content_type_for_path(path: &str) -> String {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "json" => "application/json; charset=utf-8",
        "webmanifest" => "application/manifest+json; charset=utf-8",
        "ico" => "image/x-icon",
        "map" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
    .to_owned()
}

fn write_generated_module(
    out_dir: &Path,
    output_module: &str,
    embed_subdir: &str,
    lookup_fn: &str,
    normalize_fn: &str,
    normalize_from: &str,
    normalize_to: &str,
    entries: &[EmbedEntry],
) {
    let mut source = format!(
        "// @generated by build.rs — do not edit\n\
         pub(crate) fn {lookup_fn}(path: &str) -> Option<(&'static [u8], &'static str)> {{\n\
             let normalized = {normalize_fn}(path);\n\
             match normalized.as_str() {{\n",
    );

    for entry in entries {
        source.push_str(&format!(
            "        \"{}\" => Some((include_bytes!(\"{embed_subdir}/{}\"), \"{}\")),\n",
            entry.web_path, entry.file_name, entry.content_type,
        ));
    }

    source.push_str(
        "        _ => None,\n\
             }\n\
         }\n\n",
    );

    source.push_str(&format!(
        "fn {normalize_fn}(path: &str) -> String {{\n\
             if path == \"{normalize_from}\" {{\n\
                 return \"{normalize_to}\".to_owned();\n\
             }}\n\
             path.to_owned()\n\
         }}\n",
    ));

    fs::write(out_dir.join(output_module), source).expect("write generated ui assets module");
}
