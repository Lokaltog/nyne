use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use rstest::rstest;
use serde::Serialize;

use super::{TemplateContent, TemplateEngine, TemplateGlobals, TemplateHandle, TemplateView, serialize_view};

/// Load a fixture template file.
fn load_fixture(name: &str) -> String { crate::load_fixture!("templates", name) }

/// Create an engine with a single fixture template loaded as "test".
fn engine_with_fixture(name: &str) -> TemplateEngine {
    let source = load_fixture(name);
    let mut engine = TemplateEngine::new();
    // SAFETY: leaked to satisfy 'static — acceptable in tests.
    engine.add_template("test", String::leak(source));
    engine
}

/// Shared view model for template rendering tests.
#[derive(Serialize)]
struct TestView {
    name: String,
    items: Vec<String>,
}

/// Tests that a basic template renders with a simple view model.
#[rstest]
fn render_basic_template() {
    let engine = engine_with_fixture("basic.j2");
    let view = TestView {
        name: "world".into(),
        items: vec![],
    };
    insta::assert_snapshot!(engine.render("test", &view));
}

/// Tests that `trim_blocks` and `lstrip_blocks` engine settings strip whitespace correctly.
#[rstest]
fn trim_blocks_and_lstrip() {
    let engine = engine_with_fixture("trim_blocks.j2");
    let view = TestView {
        name: String::new(),
        items: vec!["a".into(), "b".into()],
    };
    insta::assert_snapshot!(engine.render("test", &view));
}

/// Tests that the tokens filter formats values below 1k as plain numbers.
#[rstest]
fn tokens_filter_below_1k() {
    let engine = engine_with_fixture("tokens.j2");

    #[derive(Serialize)]
    struct V {
        count: usize,
    }

    // Filter converts bytes → tokens (bytes / 4), so 3400 bytes → 850 tokens.
    insta::assert_snapshot!(engine.render("test", &V { count: 3400 }));
}

/// Tests that the tokens filter formats values above 1k with decimal notation.
#[rstest]
fn tokens_filter_above_1k() {
    let engine = engine_with_fixture("tokens.j2");

    #[derive(Serialize)]
    struct V {
        count: usize,
    }

    // Filter converts bytes → tokens (bytes / 4), so 8400 bytes → 2100 tokens.
    insta::assert_snapshot!(engine.render("test", &V { count: 8400 }));
}

/// Static view via `serialize_view` — the simple path.
#[rstest]
fn template_view_serialize() {
    let engine = engine_with_fixture("basic.j2");

    #[derive(Serialize)]
    struct StaticView {
        name: String,
        items: Vec<String>,
    }

    let data = StaticView {
        name: "static".into(),
        items: vec!["x".into(), "y".into()],
    };
    let view = serialize_view(&data);
    let result = view.render(&engine, "test").unwrap();
    insta::assert_snapshot!(String::from_utf8(result).unwrap());
}

/// Dynamic view via manual `TemplateView` impl — computes at render time.
#[rstest]
fn template_view_dynamic() {
    let engine = engine_with_fixture("basic.j2");

    struct DynamicView {
        base_name: String,
        call_count: std::sync::atomic::AtomicU32,
    }

    impl TemplateView for DynamicView {
        fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
            let n = self.call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let name = format!("{}-{n}", self.base_name);
            let view = minijinja::context! { name, items => Vec::<String>::new() };
            Ok(engine.render(template, &view).into_bytes())
        }
    }

    let view = DynamicView {
        base_name: "dynamic".into(),
        call_count: std::sync::atomic::AtomicU32::new(0),
    };

    let first = String::from_utf8(view.render(&engine, "test").unwrap()).unwrap();
    let second = String::from_utf8(view.render(&engine, "test").unwrap()).unwrap();
    // Each call produces different output — proves render-time computation.
    assert_ne!(first, second);
    insta::assert_snapshot!("dynamic_first", first);
    insta::assert_snapshot!("dynamic_second", second);
}

/// Fallible view — errors propagate through `TemplateView::render`.
#[rstest]
fn template_view_fallible() {
    let engine = engine_with_fixture("basic.j2");

    struct FailingView;

    impl TemplateView for FailingView {
        fn render(&self, _engine: &TemplateEngine, _template: &str) -> Result<Vec<u8>> {
            Err(eyre!("data source unavailable"))
        }
    }

    let result = FailingView.render(&engine, "test");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("data source unavailable"));
}

/// `TemplateContent` accepts both `serialize_view` and manual `TemplateView` impls.
#[rstest]
fn template_content_construction() {
    let engine = Arc::new(engine_with_fixture("basic.j2"));

    #[derive(Serialize)]
    struct V {
        name: String,
        items: Vec<String>,
    }

    // Verify TemplateContent can be constructed with serialize_view.
    let _content = TemplateContent::new(
        &engine,
        "test",
        serialize_view(&V {
            name: "content".into(),
            items: vec!["a".into()],
        }),
    );

    // Verify it also accepts a manual TemplateView impl.
    struct ManualView;
    impl TemplateView for ManualView {
        fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
            let view = minijinja::context! { name => "manual", items => Vec::<String>::new() };
            Ok(engine.render(template, &view).into_bytes())
        }
    }
    let _content = TemplateContent::new(&engine, "test", ManualView);
}
/// `TemplateHandle::lazy_node` creates a closure-backed template node.
#[rstest]
fn lazy_node_renders_via_closure() {
    let engine = Arc::new(engine_with_fixture("basic.j2"));
    let handle = TemplateHandle::new(&engine, "test");

    let node = handle.lazy_node("test.md", |engine: &TemplateEngine, tmpl: &str| {
        let view = minijinja::context! { name => "lazy", items => vec!["one"] };
        Ok(engine.render_bytes(tmpl, &view))
    });
    assert_eq!(node.name(), "test.md");

    let fs = crate::router::MemFs::new();
    let ctx = crate::router::ReadContext {
        path: std::path::Path::new("/test.md"),
        fs: &fs,
    };
    let bytes = node.readable().unwrap().read(&ctx).unwrap();
    let content = String::from_utf8(bytes).unwrap();
    insta::assert_snapshot!("lazy_node", content);
}

/// `TemplateHandle::editable_lazy_node` creates a closure-backed readable+writable node.
#[rstest]
fn editable_lazy_node_renders_and_writes() {
    let engine = Arc::new(engine_with_fixture("basic.j2"));
    let handle = TemplateHandle::new(&engine, "test");

    let written = Arc::new(std::sync::Mutex::new(Vec::new()));
    let w = Arc::clone(&written);

    let node = handle.editable_lazy_node(
        "test.md",
        |engine: &TemplateEngine, tmpl: &str| {
            let view = minijinja::context! { name => "editable", items => Vec::<String>::new() };
            Ok(engine.render_bytes(tmpl, &view))
        },
        move |_ctx: &crate::router::WriteContext<'_>, data: &[u8]| {
            *w.lock().unwrap() = data.to_vec();
            Ok(vec![])
        },
    );

    let fs = crate::router::MemFs::new();
    let read_ctx = crate::router::ReadContext {
        path: std::path::Path::new("/test.md"),
        fs: &fs,
    };
    let bytes = node.readable().unwrap().read(&read_ctx).unwrap();
    assert!(String::from_utf8(bytes).unwrap().contains("editable"));

    let write_ctx = crate::router::WriteContext {
        path: std::path::Path::new("/test.md"),
        fs: &fs,
    };
    node.writable().unwrap().write(&write_ctx, b"new content").unwrap();
    assert_eq!(*written.lock().unwrap(), b"new content");
}
/// `TemplateGlobals` default impl walks nested fields and registers each
/// string leaf as a template global using UPPER_SNAKE keys joined by `_`.
#[rstest]
fn template_globals_derives_nested_keys() {
    #[derive(Serialize)]
    struct Vfs {
        dir: VfsDirs,
        file: VfsFiles,
        flat: String,
    }
    #[derive(Serialize)]
    struct VfsDirs {
        git: String,
        by_kind: String,
    }
    #[derive(Serialize)]
    struct VfsFiles {
        head_diff: String,
    }
    impl TemplateGlobals for Vfs {}

    let vfs = Vfs {
        dir: VfsDirs {
            git: "git".into(),
            by_kind: "by-kind".into(),
        },
        file: VfsFiles {
            head_diff: "HEAD.diff".into(),
        },
        flat: "flat.txt".into(),
    };
    let mut engine = TemplateEngine::new();
    vfs.register_globals(&mut engine);
    engine.add_template("t", "{{ DIR_GIT }}|{{ DIR_BY_KIND }}|{{ FILE_HEAD_DIFF }}|{{ FLAT }}");
    assert_eq!(engine.render("t", &()), "git|by-kind|HEAD.diff|flat.txt");
}
