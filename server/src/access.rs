//! What a user may see, drive and change.
//!
//! Administrators of the server see, drive and change everything. A user (role "user") only sees
//! the installations given to them, each with a permission: "read", "control" (with or without
//! the right to change zone names and groups, routines and Airzone schedules) or "admin" (all of
//! that, plus the installation's settings). Installations added later (by hand, from the Airzone
//! account or through another node) are given to nobody.
//!
//! The devices of the modules (Shelly, Hue, shutters, ventilation…) are given apart: all of them,
//! those of a room, of a type or one device, "read" or "control" — the most a grant gives wins.
//! Configuring them (names, rooms, roles) stays with the administrators.

use crate::auth::AuthUser;
use crate::db::{self, Db, DeviceGrant, Grant, UserRights};
use crate::error::{ApiError, ApiResult};
use crate::hub::{InstallationUserRights, InstallationView};
use crate::routines::{Action, Scope};
use crate::timers::Target;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Access {
    admin: bool,
    granted: HashMap<i64, Grant>,
    devices: Vec<DeviceGrant>,
}

/// A device of a module, as an access to it is judged: its room and its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleDevice {
    pub id: i64,
    pub room_id: Option<i64>,
    /// See `devices::type_of`.
    pub kind: &'static str,
}

impl Access {
    pub async fn of(db: &Db, user: &AuthUser) -> sqlx::Result<Access> {
        Self::for_role(db, user.id, &user.role).await
    }

    pub async fn for_role(db: &Db, user_id: i64, role: &str) -> sqlx::Result<Access> {
        if role == "admin" {
            return Ok(Access { admin: true, granted: HashMap::new(), devices: vec![] });
        }
        let granted = db::user_access(db, user_id).await?.into_iter().map(|g| (g.installation_id, g)).collect();
        let devices = db::user_device_access(db, user_id).await?;
        Ok(Access { admin: false, granted, devices })
    }

    /// 0: nothing, 1: read, 2: control — the most a grant gives.
    fn module_level(&self, d: &ModuleDevice) -> u8 {
        if self.admin {
            return 2;
        }
        self.devices
            .iter()
            .filter(|g| match g.scope.as_str() {
                "all" => true,
                "room" => d.room_id.is_some_and(|r| r.to_string() == g.reference),
                "type" => g.reference == d.kind,
                "device" => d.id.to_string() == g.reference,
                _ => false,
            })
            .map(|g| if g.permission == "control" { 2 } else { 1 })
            .max()
            .unwrap_or(0)
    }

    pub fn can_view_module(&self, d: &ModuleDevice) -> bool {
        self.module_level(d) >= 1
    }

    pub fn can_control_module(&self, d: &ModuleDevice) -> bool {
        self.module_level(d) >= 2
    }

    /// 404 for a device the user cannot see, 403 for one they only see.
    pub fn control_module(&self, d: &ModuleDevice) -> ApiResult<()> {
        match self.module_level(d) {
            0 => Err(ApiError::not_found("device not found")),
            1 => Err(ApiError::forbidden("vous avez accès à cet appareil en lecture seule")),
            _ => Ok(()),
        }
    }

    /// Sees the devices of a type wherever they are (the energy page: "energy" or "solar").
    pub fn sees_type(&self, kind: &str) -> bool {
        self.admin || self.devices.iter().any(|g| g.scope == "all" || (g.scope == "type" && g.reference == kind))
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
            Some(g) if g.permission == "control" => InstallationUserRights { zones: g.rights.zones, routines: g.rights.routines, schedules: g.rights.schedules, admin: false },
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
        UserRights { zones: all.iter().any(|r| r.zones), routines: all.iter().any(|r| r.routines), schedules: all.iter().any(|r| r.schedules) }
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

    /// Driving what a routine drives: the zones of an installation need the right to drive it, a
    /// device of a module the right to drive it (a device no longer there: an administrator).
    pub fn control_scopes(&self, scopes: &[Scope]) -> ApiResult<()> {
        scopes.iter().try_for_each(|s| match s {
            Scope::Installation(i) => self.control(*i),
            Scope::Module(d) => self.control_module(d),
            Scope::Gone if self.admin => Ok(()),
            Scope::Gone => Err(ApiError::forbidden("administrators only")),
        })
    }

    /// Creating, changing or deleting a routine that drives these.
    pub fn edit_routine(&self, scopes: &[Scope]) -> ApiResult<()> {
        self.control_scopes(scopes)?;
        scopes.iter().try_for_each(|s| match s {
            Scope::Installation(i) => Self::allowed(self.rights(*i).routines, "vous n'avez pas le droit de modifier les routines"),
            Scope::Module(_) | Scope::Gone => Ok(()),
        })
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

    /// Sees everything a routine drives.
    pub fn sees_scopes(&self, scopes: &[Scope]) -> bool {
        scopes.iter().all(|s| match s {
            Scope::Installation(i) => self.can_view(*i),
            Scope::Module(d) => self.can_view_module(d),
            Scope::Gone => self.admin,
        })
    }
}
