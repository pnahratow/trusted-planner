//! minijinja wiring.
//!
//! The environment is rebuilt per render so editing a template in
//! `PLANNER_TEMPLATE_DIR` takes effect on the next page load with no
//! recompile and no restart (D10). Parsing a handful of small templates is
//! microseconds — irrelevant at household load, and it makes staleness
//! structurally impossible.

use std::path::{Path, PathBuf};

use minijinja::{Environment, path_loader};

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
        env.set_loader(path_loader(&self.dir));
        env.get_template(name)?.render(ctx)
    }
}
