//! What a user may see, drive and change.
//!
//! Administrators of the server see, drive and change everything. A user (role "user") only sees
//! the installations given to them, each with a permission: "read", "control" (with or without
//! the right to change zone names and groups, routines and Airzone schedules) or "admin" (all of
//! that, plus the installation's settings). Installations added later (by hand, from the Airzone
//! account or through another node) are given to nobody.

use crate::auth::AuthUser;
use crate::db::{self, Db, Grant, UserRights};
use crate::error::{ApiError, ApiResult};
use crate::hub::{InstallationUserRights, InstallationView};
use crate::routines::Action;
use crate::timers::Target;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Access {
    admin: bool,
    granted: HashMap<i64, Grant>,
}

impl Access {
    pub async fn of(db: &Db, user: &AuthUser) -> sqlx::Result<Access> {
        Self::for_role(db, user.id, &user.role).await
    }

    pub async fn for_role(db: &Db, user_id: i64, role: &str) -> sqlx::Result<Access> {
        if role == "admin" {
            return Ok(Access { admin: true, granted: HashMap::new() });
        }
        let granted = db::user_access(db, user_id).await?.into_iter().map(|g| (g.installation_id, g)).collect();
        Ok(Access { admin: false, granted })
    }

    pub fn is_admin(&self) -> bool {
        self.admin
    }

    pub fn can_view(&self, installation_id: i64) -> bool {
        self.admin || self.granted.contains_key(&installation_id)
    }

    pub fn can_control(&self, installation_id: i64) -> bool {
        self.admin || self.granted.get(&installation_id).is_some_and(|g| g.permission != "read")
    }

    /// Administrator of the server, or of this installation.
    pub fn manages(&self, installation_id: i64) -> bool {
        self.admin || self.granted.get(&installation_id).is_some_and(|g| g.permission == "admin")
    }

    /// Administrator of the server or of at least one installation.
    pub fn manages_any(&self) -> bool {
        self.admin || self.granted.values().any(|g| g.permission == "admin")
    }

    /// What the user may change on this installation, besides driving it.
    pub fn rights(&self, installation_id: i64) -> InstallationUserRights {
        if self.manages(installation_id) {
            return InstallationUserRights { zones: true, routines: true, schedules: true, admin: true };
        }
        match self.granted.get(&installation_id) {
            Some(g) if g.permission == "control" => {
                InstallationUserRights { zones: g.rights.zones, routines: g.rights.routines, schedules: g.rights.schedules, admin: false }
            }
            _ => InstallationUserRights::default(),
        }
    }

    /// What the user may change on at least one installation (for the clients up to 0.7.0, which
    /// read it from `/api/me`).
    pub fn any_rights(&self) -> UserRights {
        if self.admin {
            return UserRights::default();
        }
        let all: Vec<InstallationUserRights> = self.granted.keys().map(|id| self.rights(*id)).collect();
        UserRights {
            zones: all.iter().any(|r| r.zones),
            routines: all.iter().any(|r| r.routines),
            schedules: all.iter().any(|r| r.schedules),
        }
    }

    /// Visible installations (`None` = all).
    pub fn visible(&self) -> Option<Vec<i64>> {
        (!self.admin).then(|| {
            let mut ids: Vec<i64> = self.granted.keys().copied().collect();
            ids.sort_unstable();
            ids
        })
    }

    /// 404 for an installation the user cannot see (its existence is not revealed).
    pub fn view(&self, installation_id: i64) -> ApiResult<()> {
        if self.can_view(installation_id) {
            Ok(())
        } else {
            Err(ApiError::not_found("installation not found"))
        }
    }

    pub fn control(&self, installation_id: i64) -> ApiResult<()> {
        self.view(installation_id)?;
        if self.can_control(installation_id) {
            Ok(())
        } else {
            Err(ApiError::forbidden("vous avez accès à cette installation en lecture seule"))
        }
    }

    pub fn control_actions(&self, actions: &[Action]) -> ApiResult<()> {
        actions.iter().try_for_each(|a| self.control(a.installation_id))
    }

    /// Renaming or regrouping the zones of an installation.
    pub fn edit_zones(&self, installation_id: i64) -> ApiResult<()> {
        self.control(installation_id)?;
        Self::allowed(self.rights(installation_id).zones, "vous n'avez pas le droit de renommer ni de regrouper les zones")
    }

    /// Creating, changing or deleting a routine with these actions.
    pub fn edit_routine(&self, actions: &[Action]) -> ApiResult<()> {
        self.control_actions(actions)?;
        actions
            .iter()
            .try_for_each(|a| Self::allowed(self.rights(a.installation_id).routines, "vous n'avez pas le droit de modifier les routines"))
    }

    /// Creating, changing or deleting an Airzone schedule of an installation (control is checked
    /// by the caller).
    pub fn edit_schedules(&self, installation_id: i64) -> ApiResult<()> {
        Self::allowed(self.rights(installation_id).schedules, "vous n'avez pas le droit de modifier les programmations")
    }

    /// Changing the settings of an installation (address, Airzone link, deletion).
    pub fn manage(&self, installation_id: i64) -> ApiResult<()> {
        self.view(installation_id)?;
        Self::allowed(self.manages(installation_id), "administrator role required")
    }

    fn allowed(right: bool, refusal: &str) -> ApiResult<()> {
        if right {
            Ok(())
        } else {
            Err(ApiError::forbidden(refusal))
        }
    }

    /// "All installations" is reserved to administrators (it would include the others').
    pub fn control_target(&self, target: &Target) -> ApiResult<()> {
        match target.installation_id() {
            Some(id) => self.control(id),
            None if self.admin => Ok(()),
            None => Err(ApiError::forbidden("choisissez une installation : « toutes » est réservé aux administrateurs")),
        }
    }

    pub fn sees_target(&self, target: &Target) -> bool {
        target.installation_id().map_or(self.admin, |id| self.can_view(id))
    }

    /// The installation as this user sees it: not controllable when read-only for them, with
    /// what they may change.
    pub fn apply(&self, mut view: InstallationView) -> InstallationView {
        view.controllable = view.controllable && self.can_control(view.id);
        view.rights = self.rights(view.id);
        view
    }

    /// Installations this user can see, adapted to their permission.
    pub fn filter(&self, views: Vec<InstallationView>) -> Vec<InstallationView> {
        views.into_iter().filter(|i| self.can_view(i.id)).map(|i| self.apply(i)).collect()
    }

    pub fn sees_actions(&self, actions: &[Action]) -> bool {
        actions.iter().all(|a| self.can_view(a.installation_id))
    }
}
