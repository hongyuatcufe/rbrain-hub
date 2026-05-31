use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Loads prompt files with a two-level fallback:
///   1. `$prompts_dir/{name}.md`  (user override)
///   2. Built-in defaults compiled into the binary
#[derive(Clone)]
pub struct PromptLoader {
    prompts_dir: PathBuf,
    builtins: HashMap<&'static str, &'static str>,
}

impl PromptLoader {
    pub fn new(prompts_dir: PathBuf, builtins: HashMap<&'static str, &'static str>) -> Self {
        Self { prompts_dir, builtins }
    }

    /// Load prompt by name (without `.md` extension).
    /// Returns user file content if present, otherwise the built-in default.
    /// Returns an error if neither exists or the file cannot be read.
    pub fn load(&self, name: &str) -> String {
        match self.try_load(name) {
            Ok(s) => s,
            Err(e) => {
                // Panic in non-server contexts is acceptable for unknown builtins;
                // callers that need graceful handling should use try_load().
                panic!("{}", e)
            }
        }
    }

    /// Fallible version of load — returns Err instead of panicking.
    pub fn try_load(&self, name: &str) -> Result<String, String> {
        let path = self.prompts_dir.join(format!("{name}.md"));
        if path.exists() {
            return std::fs::read_to_string(&path)
                .map_err(|e| format!("failed to read prompt file {path:?}: {e}"));
        }
        self.builtins
            .get(name)
            .map(|s| s.to_string())
            .ok_or_else(|| format!("no prompt found for '{name}' (checked {path:?} and builtins)"))
    }

    /// Render a prompt with `{key}` placeholder substitution.
    pub fn render(template: &str, vars: &HashMap<&str, &str>) -> String {
        let mut out = template.to_string();
        for (k, v) in vars {
            out = out.replace(&format!("{{{k}}}"), v);
        }
        out
    }

    pub fn prompts_dir(&self) -> &Path {
        &self.prompts_dir
    }
}
