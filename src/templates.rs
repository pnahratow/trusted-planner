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

    pub fn render<S: serde::Serialize>(
        &self,
        name: &str,
        ctx: S,
    ) -> Result<String, minijinja::Error> {
        let mut env = Environment::new();
        // Strict: a name the context does not supply is an error, not a silent
        // empty string. A missing `board_id` once rendered `hx-post="/b//task"`
        // in the week grid — a live 400 that every offline test still passed,
        // because the fragment route did supply the name. Fail loudly instead.
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_loader(path_loader(&self.dir));
        env.get_template(name)?.render(ctx)
    }
}
