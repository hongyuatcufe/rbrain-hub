//! Multi-tenant context carried through every Engine query (M4).
//!
//! Three reserved `user_id` values (plan.md §M4 / decision #6):
//!
//! - `'global'` — pipeline-written content (ingested journal articles).
//!   Every project that subscribes to a matching topic can read these.
//! - `'admin'` — ZeroClaw / platform operator content; surfaced to users
//!   only via `push_inbox`, never directly readable from a user context.
//! - `'default'` — backwards-compatible value used by pre-M4 data, fixtures,
//!   and the CLI when no explicit user is configured.
//!
//! A `TenantContext` carries the *caller's* identity. The engine uses
//! [`TenantContext::readable_user_ids`] to compute which `user_id`
//! partitions a SELECT should consider — typically the caller plus
//! `'global'`. Writes always target the caller's `user_id` exactly.

use serde::{Deserialize, Serialize};

/// Reserved global-content tenant. See module docs.
pub const GLOBAL_USER: &str = "global";

/// Reserved admin / platform-operator tenant. See module docs.
pub const ADMIN_USER: &str = "admin";

/// Backwards-compat tenant for pre-M4 data and tests.
pub const DEFAULT_USER: &str = "default";

/// Conventional `project_id` for the backwards-compat single-tenant case.
pub const DEFAULT_PROJECT: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantContext {
    /// Owning user — either a real user_id, `'global'`, `'admin'`, or `'default'`.
    pub user_id: String,
    /// Optional project namespace within the user. `None` means the user's
    /// personal space (for research-journal page types: daily/meeting/etc.).
    pub project_id: Option<String>,
}

impl TenantContext {
    /// Constructor for global / pipeline writes. Never used as a *read*
    /// context — global content is read through any user's context via
    /// [`Self::readable_user_ids`].
    pub fn global() -> Self {
        Self {
            user_id: GLOBAL_USER.to_string(),
            project_id: None,
        }
    }

    /// Constructor for ZeroClaw / platform-operator writes.
    pub fn admin() -> Self {
        Self {
            user_id: ADMIN_USER.to_string(),
            project_id: None,
        }
    }

    /// Backwards-compat constructor for code paths that haven't been wired
    /// through the M4 tenancy yet (CLI default, fixtures, dev mode).
    pub fn default_tenant() -> Self {
        Self {
            user_id: DEFAULT_USER.to_string(),
            project_id: Some(DEFAULT_PROJECT.to_string()),
        }
    }

    pub fn for_user(user_id: impl Into<String>, project_id: Option<impl Into<String>>) -> Self {
        Self {
            user_id: user_id.into(),
            project_id: project_id.map(Into::into),
        }
    }

    pub fn is_admin(&self) -> bool {
        self.user_id == ADMIN_USER
    }

    pub fn is_global(&self) -> bool {
        self.user_id == GLOBAL_USER
    }

    /// The `user_id` partitions a SELECT issued by this context may consider.
    ///
    /// - Admin context: only its own writes — admin does **not** silently see
    ///   user content. Cross-tenant reads must go through explicit cross-tenant
    ///   APIs (M10).
    /// - Global context: only global itself.
    /// - User context: the user + global.
    /// - Default (backwards-compat): default + global (so tests that load
    ///   global ingestion content can still see it).
    pub fn readable_user_ids(&self) -> Vec<&str> {
        if self.is_admin() {
            vec![ADMIN_USER]
        } else if self.is_global() {
            vec![GLOBAL_USER]
        } else {
            vec![self.user_id.as_str(), GLOBAL_USER]
        }
    }
}

impl Default for TenantContext {
    fn default() -> Self {
        Self::default_tenant()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tenant_uses_reserved_default_id() {
        let t = TenantContext::default_tenant();
        assert_eq!(t.user_id, DEFAULT_USER);
        assert_eq!(t.project_id.as_deref(), Some(DEFAULT_PROJECT));
    }

    #[test]
    fn user_context_can_read_self_and_global() {
        let t = TenantContext::for_user("alice", Some("phd"));
        let ids = t.readable_user_ids();
        assert_eq!(ids, vec!["alice", GLOBAL_USER]);
    }

    #[test]
    fn admin_context_isolated_from_users_and_global() {
        let t = TenantContext::admin();
        assert!(t.is_admin());
        assert_eq!(t.readable_user_ids(), vec![ADMIN_USER]);
    }

    #[test]
    fn global_context_only_sees_global() {
        let t = TenantContext::global();
        assert!(t.is_global());
        assert_eq!(t.readable_user_ids(), vec![GLOBAL_USER]);
    }

    #[test]
    fn personal_space_has_no_project() {
        let t = TenantContext::for_user("alice", None::<String>);
        assert_eq!(t.project_id, None);
    }
}
