use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use serde::Serialize;

use super::{TemplateContent, TemplateEngine, TemplateView, serialize_view};
use crate::node::Readable;

/// Load a fixture template file.
fn load_fixture(name: &str) -> String { crate::test_support::load_fixture("templates", name) }

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
#[test]
fn render_basic_template() {
    let engine = engine_with_fixture("basic.j2");
    let view = TestView {
        name: "world".into(),
        items: vec![],
    };
    insta::assert_snapshot!(engine.render("test", &view));
}

/// Tests that trim_blocks and lstrip_blocks engine settings strip whitespace correctly.
#[test]
fn trim_blocks_and_lstrip() {
    let engine = engine_with_fixture("trim_blocks.j2");
    let view = TestView {
        name: String::new(),
        items: vec!["a".into(), "b".into()],
    };
    insta::assert_snapshot!(engine.render("test", &view));
}

/// Tests that the tokens filter formats values below 1k as plain numbers.
#[test]
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
#[test]
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
#[test]
fn template_view_serialize() {
    let engine = engine_with_fixture("basic.j2");

    #[derive(Serialize)]
    struct StaticView {
        name: String,
        items: Vec<String>,
    }

    let view = serialize_view(StaticView {
        name: "static".into(),
        items: vec!["x".into(), "y".into()],
    });
    let result = view.render(&engine, "test").unwrap();
    insta::assert_snapshot!(String::from_utf8(result).unwrap());
}

/// Dynamic view via manual `TemplateView` impl — computes at render time.
#[test]
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
#[test]
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
#[test]
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
        serialize_view(V {
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

use crate::test_support::{StubEvents, StubFs, StubResolver, stub_request_context};

/// Exercise `TemplateContent` through `Readable::read` with a real `RequestContext`.
#[test]
fn template_content_readable_integration() {
    let engine = Arc::new(engine_with_fixture("basic.j2"));

    #[derive(Serialize)]
    struct V {
        name: String,
        items: Vec<String>,
    }

    let content = TemplateContent::new(
        &engine,
        "test",
        serialize_view(V {
            name: "readable".into(),
            items: vec!["one".into(), "two".into()],
        }),
    );

    let path = crate::types::vfs_path::VfsPath::root();
    let real_fs = StubFs;
    let events = StubEvents;
    let resolver = StubResolver;
    let file_generations = crate::dispatch::content_cache::FileGenerations::new();
    let ctx = stub_request_context(&path, &real_fs, &events, &resolver, &file_generations);

    let output = content.read(&ctx).unwrap();
    insta::assert_snapshot!(String::from_utf8(output).unwrap());
}
