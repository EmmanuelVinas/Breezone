//! A module built against version 1.0 of the interface (`wit/`, a copy kept as it was, never to be
//! changed): the host must load it and drive it whatever it adds later. One plug, always on.

wit_bindgen::generate!({
    path: "wit",
    world: "module",
});

use breezone::module::types::{Curve, Described, DeviceInfo, Error, HardwareRoom, Paired, Picture, Scene, Schedule};
use exports::breezone::module::driver::{Guest, GuestSource, Source as SourceHandle};

struct Module;

pub struct Source;

fn no<T>(what: &str) -> Result<T, Error> {
    Err(Error::Unsupported(what.to_string()))
}

impl GuestSource for Source {
    fn open(_config: String, _secret: Option<String>) -> Result<SourceHandle, Error> {
        Ok(SourceHandle::new(Source))
    }

    fn describe(&self) -> Result<Described, Error> {
        let plug = DeviceInfo { reference: "plug".into(), kind: "switch.relay".into(), traits: vec!["switch.relay@1".into()], name: "Prise".into() };
        Ok(Described { model: "Contract 1.0".into(), firmware: None, devices: vec![plug], identity: None })
    }

    fn read(&self) -> Result<Vec<(String, String)>, Error> {
        Ok(vec![("plug".into(), r#"{"on":true}"#.into())])
    }

    fn act(&self, _reference: String, _trait_name: String, _action: String, _params: String) -> Result<(), Error> {
        Ok(())
    }

    fn on_message(&self, _subscription: u32, _data: Vec<u8>) -> Result<(), Error> {
        Ok(())
    }

    fn diagnostic(&self) -> Result<String, Error> {
        Ok("{}".into())
    }

    fn rooms(&self) -> Result<Vec<HardwareRoom>, Error> {
        no("rooms")
    }

    fn scenes(&self) -> Result<Vec<Scene>, Error> {
        no("scenes")
    }

    fn activate_scene(&self, _id: String) -> Result<(), Error> {
        no("scenes")
    }

    fn picture(&self, _reference: String) -> Result<Picture, Error> {
        no("picture")
    }

    fn forecast(&self, _reference: String) -> Result<Vec<Curve>, Error> {
        no("forecast")
    }

    fn unlock(&self, _key: String) -> Result<bool, Error> {
        no("unlock")
    }

    fn schedules(&self) -> Result<Vec<Schedule>, Error> {
        no("schedules")
    }

    fn save_schedule(&self, _schedule: Schedule) -> Result<String, Error> {
        no("schedules")
    }

    fn delete_schedule(&self, _id: String) -> Result<(), Error> {
        no("schedules")
    }

    fn close(&self) {}
}

impl Guest for Module {
    type Source = Source;

    fn probe(_host: String) -> Option<String> {
        None
    }

    fn pair(_step: String, _input: String) -> Result<Paired, Error> {
        no("pair")
    }
}

impl exports::breezone::module::service::Guest for Module {
    fn check() -> Result<(), Error> {
        no("services")
    }

    fn query(_contract: String, _query: String, _params: String) -> Result<String, Error> {
        no("services")
    }
}

export!(Module);
