//! minijinja wiring.
//!
//! The environment is rebuilt per render so editing a template in
//! `PLANNER_TEMPLATE_DIR` takes effect on the next page load with no
//! recompile and no restart (D10). Parsing a handful of small templates is
//! microseconds — irrelevant at household load, and it makes staleness
//! structurally impossible.

use std::path::{Path, PathBuf};

use minijinja::{Environment, UndefinedBehavior, path_loader};

#[derive(Clone)]
pub struct Templates {
    dir: PathBuf,
}

impl Templates {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    /// `locale` is consumed here because the `t` filter holds it for the
    /// length of the render: one lookup table, built once, for every string on
    /// the page.
    pub fn render<S: serde::Serialize>(
        &self,
        name: &str,
        ctx: S,
        locale: crate::i18n::Locale,
    ) -> Result<String, minijinja::Error> {
        let mut env = Environment::new();
        // Strict: a name the context does not supply is an error, not a silent
        // empty string. A missing `board_id` once rendered `hx-post="/b//task"`
        // in the week grid — a live 400 that every offline test still passed,
        // because the fragment route did supply the name. Fail loudly instead.
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_loader(path_loader(&self.dir));
        // `{{ "Today" | t }}` — the English text is the key, so a template
        // reads the same whether or not a translation exists for it.
        env.add_filter("t", move |text: &str| locale.t(text).to_string());
        env.get_template(name)?.render(ctx)
    }
}
