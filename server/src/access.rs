//! What a user may see and drive.
//!
//! Administrators see and drive every installation. A user (role "user") only sees the
//! installations an administrator gave them, each read-only or with control. Installations
//! added later (by hand, from the Airzone account or through another node) are given to nobody.
//! On the installations they control, a user may also change zone names and groups, routines
//! and Airzone schedules, unless an administrator took that right away (see `UserRights`).

use crate::auth::AuthUser;
use crate::db::{self, Db, UserRights};
use crate::error::{ApiError, ApiResult};
use crate::hub::InstallationView;
use crate::routines::Action;
use crate::timers::Target;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Access {
    admin: bool,
    /// installation id → can control
    granted: HashMap<i64, bool>,
    rights: UserRights,
}

impl Access {
    pub async fn of(db: &Db, user: &AuthUser) -> sqlx::Result<Access> {
        Self::for_role(db, user.id, &user.role).await
    }

    pub async fn for_role(db: &Db, user_id: i64, role: &str) -> sqlx::Result<Access> {
        if role == "admin" {
            return Ok(Access { admin: true, granted: HashMap::new(), rights: UserRights::default() });
        }
        let granted = db::user_access(db, user_id).await?.into_iter().map(|(id, p)| (id, p == "control")).collect();
        let rights = db::user_by_id(db, user_id).await?.map_or(UserRights::NONE, |u| u.rights());
        Ok(Access { admin: false, granted, rights })
    }

    pub fn is_admin(&self) -> bool {
        self.admin
    }

    pub fn can_view(&self, installation_id: i64) -> bool {
        self.admin || self.granted.contains_key(&installation_id)
    }

    pub fn can_control(&self, installation_id: i64) -> bool {
        self.admin || self.granted.get(&installation_id).copied().unwrap_or(false)
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
        Self::allowed(self.rights.zones, "vous n'avez pas le droit de renommer ni de regrouper les zones")
    }

    /// Creating, changing or deleting a routine with these actions.
    pub fn edit_routine(&self, actions: &[Action]) -> ApiResult<()> {
        self.control_actions(actions)?;
        Self::allowed(self.rights.routines, "vous n'avez pas le droit de modifier les routines")
    }

    /// Creating, changing or deleting an Airzone schedule (control is checked by the caller).
    pub fn edit_schedules(&self) -> ApiResult<()> {
        Self::allowed(self.rights.schedules, "vous n'avez pas le droit de modifier les programmations")
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

    /// The installation as this user sees it: not controllable when read-only for them.
    pub fn apply(&self, mut view: InstallationView) -> InstallationView {
        view.controllable = view.controllable && self.can_control(view.id);
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
