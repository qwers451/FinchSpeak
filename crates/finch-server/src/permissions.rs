//! Who may do what. The only place that knows the roles' powers.

use crate::api::auth::AuthUser;
use crate::error::ServerError;

/// Something a role can be allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// See the member list with roles, state and last visit.
    ViewMembers,
    /// Disconnect a member from the server (they may sign in again).
    KickMember,
    /// Send a member out of a voice room or silence them there.
    ModerateVoice,
    /// Delete messages of other people.
    DeleteAnyMessage,
    /// Read the log of administrative actions.
    ReadAudit,
    /// Ban and unban, and change anyone's role.
    ManageMembers,
    /// Create, rename, reorder and delete channels.
    ManageChannels,
    /// Create and revoke invitations.
    ManageInvites,
    /// Server page: registration mode, backups.
    ManageServer,
}

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_MODERATOR: &str = "moderator";
pub const ROLE_MEMBER: &str = "member";

/// Every role the server knows.
pub const ROLES: [&str; 3] = [ROLE_ADMIN, ROLE_MODERATOR, ROLE_MEMBER];

/// May `role` do `permission`?
pub fn allows(role: &str, permission: Permission) -> bool {
    use Permission::*;
    match role {
        ROLE_ADMIN => true,
        ROLE_MODERATOR => matches!(permission, ViewMembers | KickMember | ModerateVoice | DeleteAnyMessage | ReadAudit),
        _ => false,
    }
}

/// Refuse (403) unless the caller may.
pub fn require(auth: &AuthUser, permission: Permission) -> Result<(), ServerError> {
    if allows(&auth.role, permission) {
        Ok(())
    } else {
        Err(ServerError::Forbidden)
    }
}

/// May `actor` use a member-level action on an account with role `target`?
/// Moderators only act on plain members; admins act on everybody (the callers still stop self-harm).
pub fn may_act_on(actor_role: &str, target_role: &str) -> bool {
    match actor_role {
        ROLE_ADMIN => true,
        ROLE_MODERATOR => target_role == ROLE_MEMBER,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Permission::*;

    const ALL: [Permission; 9] = [ViewMembers, KickMember, ModerateVoice, DeleteAnyMessage, ReadAudit, ManageMembers, ManageChannels, ManageInvites, ManageServer];

    #[test]
    fn admins_may_do_everything() {
        assert!(ALL.iter().all(|p| allows(ROLE_ADMIN, *p)));
    }

    #[test]
    fn moderators_may_moderate_and_look_but_not_manage() {
        let allowed: Vec<_> = ALL.iter().copied().filter(|p| allows(ROLE_MODERATOR, *p)).collect();
        assert_eq!(allowed, vec![ViewMembers, KickMember, ModerateVoice, DeleteAnyMessage, ReadAudit]);
    }

    #[test]
    fn members_and_unknown_roles_may_do_nothing() {
        for role in [ROLE_MEMBER, "", "root", "ADMIN"] {
            assert!(ALL.iter().all(|p| !allows(role, *p)), "{role:?}");
        }
    }

    #[test]
    fn moderators_only_act_on_members() {
        assert!(may_act_on(ROLE_MODERATOR, ROLE_MEMBER));
        assert!(!may_act_on(ROLE_MODERATOR, ROLE_MODERATOR));
        assert!(!may_act_on(ROLE_MODERATOR, ROLE_ADMIN));
        assert!(may_act_on(ROLE_ADMIN, ROLE_ADMIN));
        assert!(!may_act_on(ROLE_MEMBER, ROLE_MEMBER));
    }
}
