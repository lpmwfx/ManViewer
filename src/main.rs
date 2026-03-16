//! man-viewer — Slint GUI for browsing man/ documentation.
//!
//! Usage:
//!   man-viewer [PATH]
//!
//! PATH can be:
//!   - a project root (looks for man/MANIFEST.json)
//!   - a man/ directory directly
//!
//! Navigation sidebar: top-level folders are collapsible group headers.
//! Click a header to expand/collapse its files. Click a file to load it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{env, fs};

use slint::{ComponentHandle, Model};
use slint_ui_templates::{docs, dsl::BgStyle, platform, DocBlock, DocsApp, NavItem};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = env::args().nth(1).map(PathBuf::from);
    let man_dir = resolve_man_dir(arg.as_deref())?;

    let pages = collect_pages(&man_dir);

    if pages.is_empty() {
        eprintln!(
            "man-viewer: no man/*.md files found in {}",
            man_dir.display()
        );
        eprintln!("  Run `rulestools-documenter gen <project-root>` first.");
        std::process::exit(1);
    }

    let project_name = resolve_project_name(&man_dir);

    let ui = DocsApp::new()?;
    let window_title = format!("{project_name} — ManViewer");
    ui.set_doc_title(window_title.clone().into());

    let (nav_items, group_map) = build_nav(&pages);

    let nav_model = std::rc::Rc::new(slint::VecModel::from(nav_items));
    ui.set_nav_items(slint::ModelRc::new(nav_model.clone()));

    if let Some((first_id, _, _)) = pages.first() {
        push_page(&ui, first_id, &pages);
        ui.set_active_view(first_id.clone().into());
    }

    let pages_nav = pages.clone();
    let weak = ui.as_weak();
    ui.on_navigate(move |id| {
        if let Some(h) = weak.upgrade() {
            push_page(&h, id.as_str(), &pages_nav);
        }
    });

    let nav_model2 = nav_model.clone();
    ui.on_toggle_group(move |group_id| {
        let Some(indices) = group_map.get(group_id.as_str()) else { return };
        let currently_hidden = indices
            .first()
            .and_then(|&i| nav_model2.row_data(i))
            .map(|item| item.hidden)
            .unwrap_or(false);
        for &i in indices {
            if let Some(mut item) = nav_model2.row_data(i) {
                item.hidden = !currently_hidden;
                nav_model2.set_row_data(i, item);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_request_bg_style(move |style| {
        if let Some(h) = weak.upgrade() {
            let bg = match style.as_str() {
                "mica"    => BgStyle::Mica,
                "acrylic" => BgStyle::Acrylic,
                _         => BgStyle::Solid,
            };
            platform::apply_backdrop(h.window(), bg);
        }
    });

    ui.show()?;
    platform::apply_backdrop(ui.window(), BgStyle::Solid);
    ui.run()?;
    Ok(())
}

/// (id, sidebar-label, md-path) sorted by path.
type PageList = Vec<(String, String, PathBuf)>;

/// Build nav items with collapsible group headers per top-level folder.
fn build_nav(pages: &PageList) -> (Vec<NavItem>, HashMap<String, Vec<usize>>) {
    let mut items: Vec<NavItem> = Vec::new();
    let mut group_map: HashMap<String, Vec<usize>> = HashMap::new();
    let mut current_group: Option<String> = None;

    for (id, _label, _path) in pages {
        let top_folder = id.split('/').next().unwrap_or("").to_string();

        if current_group.as_deref() != Some(&top_folder) && !top_folder.is_empty() {
            current_group = Some(top_folder.clone());
            items.push(NavItem {
                id:        top_folder.clone().into(),
                label:     top_folder.clone().into(),
                icon:      "".into(),
                is_header: true,
                hidden:    false,
            });
            group_map.insert(top_folder.clone(), Vec::new());
        }

        let child_idx = items.len();
        items.push(NavItem {
            id:        id.clone().into(),
            label:     file_label(id).into(),
            icon:      "".into(),
            is_header: false,
            hidden:    false,
        });

        if let Some(ref grp) = current_group {
            group_map.entry(grp.clone()).or_default().push(child_idx);
        }
    }

    (items, group_map)
}

/// Leaf label from path segments.
fn file_label(id: &str) -> String {
    let parts: Vec<&str> = id.split('/').collect();
    if parts.len() <= 2 {
        return parts.last().copied().unwrap_or(id).to_string();
    }
    let depth = parts.len().saturating_sub(2);
    let indent = "  ".repeat(depth);
    let display = parts[parts.len().saturating_sub(2)..].join("/");
    format!("{indent}{display}")
}

/// Collect all .md pages from man/ directory.
fn collect_pages(man_dir: &Path) -> PageList {
    let mut pages: PageList = Vec::new();
    collect_md(man_dir, man_dir, &mut pages);
    pages.sort_by(|a, b| a.0.cmp(&b.0));
    pages
}

/// Recursively collect .md files.
fn collect_md(base: &Path, dir: &Path, out: &mut PageList) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_md(base, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "MANIFEST.md" { continue; }
            let rel = path.strip_prefix(base).unwrap_or(&path).with_extension("");
            let id = rel.to_string_lossy().replace('\\', "/");
            let label = file_label(&id);
            out.push((id, label, path));
        }
    }
}

/// Push a page's markdown into the UI.
fn push_page(ui: &DocsApp, id: &str, pages: &PageList) {
    let blocks: Vec<DocBlock> = if let Some((_, _, md_path)) = pages.iter().find(|(i, ..)| i == id) {
        match fs::read_to_string(md_path) {
            Ok(md) => docs::parse(&md),
            Err(e) => docs::parse(&format!("# Error\n\nCould not read `{}`:\n\n```\n{e}\n```\n", md_path.display())),
        }
    } else {
        docs::parse(&format!("# {id}\n\nNo documentation found.\n"))
    };

    ui.set_doc_blocks(slint::ModelRc::new(slint::VecModel::from(blocks)));
    ui.set_doc_title(id.into());
    ui.set_status_text(format!("man/{id}.md").into());
}

/// Resolve the man/ directory from CLI argument.
fn resolve_man_dir(arg: Option<&Path>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base = arg
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| env::current_dir().expect("no cwd"));

    if base.join("MANIFEST.md").exists() || base.file_name().map_or(false, |n| n == "man") {
        return Ok(base);
    }

    let man = base.join("man");
    if man.is_dir() {
        return Ok(man);
    }

    Err(format!("Cannot find man/ directory in {}\nRun `rulestools-documenter gen .` first.", base.display()).into())
}

/// Extract project name from parent of man/ dir.
fn resolve_project_name(man_dir: &Path) -> String {
    man_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string()
}
