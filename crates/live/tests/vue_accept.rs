use impeccable_common::Io;
use impeccable_live::{event_validation, live_accept};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const SOURCE: &str = include_str!("../../../tests/fixtures/live-vue-accept/StatusCard.vue");
const MARKUP: &str = "  <section class=\"card\" @click=\"increment\">\n    <p>{{ label }}: {{ count }}</p>\n  </section>";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new(source: &str, variant: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "impeccable-vue-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(dir.join("node_modules/.impeccable-live/ab12cd34")).unwrap();
        fs::write(dir.join("package.json"), "{\"type\":\"module\"}").unwrap();
        if let Ok(modules) = std::env::var("IMPECCABLE_TEST_VUE_MODULES") {
            let scope = dir.join("node_modules/@vue");
            #[cfg(unix)]
            std::os::unix::fs::symlink(PathBuf::from(&modules).join("@vue"), scope).unwrap();
        }
        fs::write(dir.join("StatusCard.vue"), source).unwrap();
        let session = dir.join("node_modules/.impeccable-live/ab12cd34");
        let start = source
            .lines()
            .position(|line| line.contains("<section"))
            .unwrap()
            + 1;
        fs::write(session.join("manifest.json"), json!({
            "id":"ab12cd34", "previewMode":"vue-component", "count":1,
            "sourceFile":"StatusCard.vue", "componentDir":"node_modules/.impeccable-live/ab12cd34",
            "sourceStartLine": start, "sourceEndLine":start+2, "originalMarkup": MARKUP,
        }).to_string()).unwrap();
        fs::write(session.join("v1.vue"), variant).unwrap();
        fs::write(
            session.join("params.json"),
            json!({"1":[{"id":"space","kind":"range","default":8}]}).to_string(),
        )
        .unwrap();
        Self(dir)
    }
    fn accept(&self, variant: &str) -> (i32, Value) {
        let (mut io, cap) = Io::captured("", self.0.clone(), HashMap::new());
        let code = live_accept::run(
            &[
                "--id",
                "ab12cd34",
                "--variant",
                variant,
                "--param-values",
                "{\"space\":23}",
            ]
            .map(str::to_owned),
            &mut io,
        );
        let text = String::from_utf8(cap.stdout.borrow().clone()).unwrap();
        (
            code,
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"stdout":text})),
        )
    }
    fn source(&self) -> String {
        fs::read_to_string(self.0.join("StatusCard.vue")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn variant(style: &str) -> String {
    format!("<template>\n{MARKUP}\n</template>\n<style {style}>\n.card {{ padding: var(--p-space, 8px); color: red; }}\n</style>")
}

#[test]
fn vue_accept_preserves_tuned_values_scripts_and_bindings() {
    let fixture = Fixture::new(SOURCE, &variant("scoped"));
    let (_, result) = fixture.accept("1");
    assert_eq!(result["handled"], true, "{result}");
    assert_ne!(result["mode"], "error", "{result}");
    let source = fixture.source();
    assert!(source.contains("padding: 23px"), "{source}");
    assert!(!source.contains("var(--p-space"));
    assert_eq!(
        source.split("<template>").next(),
        SOURCE.split("<template>").next()
    );
    for binding in ["@click=\"increment\"", "{{ label }}", "{{ count }}"] {
        assert!(source.contains(binding));
    }
    assert_eq!(source.matches(".card {").count(), 1, "{source}");
    assert!(!fixture
        .0
        .join("node_modules/.impeccable-live/ab12cd34")
        .exists());
    let (_, repeated) = fixture.accept("1");
    assert_eq!(repeated["alreadyApplied"], true, "{repeated}");
}

#[test]
fn vue_accept_keeps_global_styles_and_writes_to_a_scoped_block() {
    for source in [
        SOURCE.replace("<style scoped>", "<style>"),
        format!("{SOURCE}\n<style>body {{ margin: 0; }}</style>\n"),
    ] {
        let original_global = source.rsplit("<style>").next().unwrap().to_string();
        let fixture = Fixture::new(&source, &variant("scoped"));
        let (_, result) = fixture.accept("1");
        assert_ne!(result["mode"], "error", "{result}");
        let accepted = fixture.source();
        assert!(accepted.contains("<style scoped>"), "{accepted}");
        assert!(accepted.contains("padding: 23px"), "{accepted}");
        assert!(
            accepted.contains(&format!("<style>{original_global}")),
            "global CSS changed: {accepted}"
        );
    }
}

#[test]
fn vue_accept_refuses_unscoped_variant_css_without_writing() {
    let fixture = Fixture::new(SOURCE, &variant(""));
    let (_, result) = fixture.accept("1");
    assert_eq!(result["mode"], "error", "{result}");
    assert!(
        result["error"].as_str().unwrap_or("").contains("scoped"),
        "{result}"
    );
    assert_eq!(fixture.source(), SOURCE);
}

#[test]
fn vue_accept_refuses_style_preprocessors_without_writing() {
    for lang in ["sass", "scss", "less", "stylus"] {
        let fixture = Fixture::new(SOURCE, &variant(&format!("scoped lang=\"{lang}\"")));
        let (_, result) = fixture.accept("1");
        assert_eq!(result["mode"], "error", "{result}");
        assert!(
            result["error"].as_str().unwrap_or("").contains(lang),
            "{result}"
        );
        assert_eq!(fixture.source(), SOURCE);
    }
}

#[test]
fn vue_accept_rejects_stale_source_without_writing() {
    let fixture = Fixture::new(SOURCE, &variant("scoped"));
    let changed = SOURCE.replace("{{ count }}", "{{ count + 1 }}");
    fs::write(fixture.0.join("StatusCard.vue"), &changed).unwrap();
    let (_, result) = fixture.accept("1");
    assert_eq!(result["mode"], "error", "{result}");
    assert!(
        result["error"].as_str().unwrap_or("").contains("changed"),
        "{result}"
    );
    assert_eq!(fixture.source(), changed);
}

#[test]
fn vue_accept_rejects_variant_zero() {
    let fixture = Fixture::new(SOURCE, &variant("scoped"));
    assert_eq!(fixture.accept("0").0, 1);
    assert!(event_validation::validate_event(
        &json!({"type":"accept","id":"ab12cd34","variantId":"0"})
    )
    .is_some());
    assert_eq!(fixture.source(), SOURCE);
}
