extern crate core;
extern crate iron;
extern crate mount;
extern crate oven;
extern crate params;
extern crate regex;
extern crate id3;
extern crate rustc_serialize;
extern crate staticfile;
extern crate toml;
extern crate url;

#[cfg(windows)]
extern crate uuid;
#[cfg(windows)]
extern crate winapi;
#[cfg(windows)]
extern crate kernel32;
#[cfg(windows)]
extern crate shell32;
#[cfg(windows)]
extern crate user32;

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use iron::prelude::*;
use mount::Mount;
use staticfile::Static;

mod api;
mod collection;
mod error;
mod ui;
mod vfs;

fn main() {

    println!("Spawning server thread");
    std::thread::spawn(move || {
        let mut api_chain;
        {
            let api_handler;
            {
                let mut collection = collection::Collection::new();
                collection.load_config(Path::new("Polaris.toml")).unwrap();
                let collection = Arc::new(Mutex::new(collection));
                api_handler = api::get_api_handler(collection);
            }
            api_chain = Chain::new(api_handler);

            let auth_secret = std::env::var("POLARIS_SECRET")
                .expect("Environment variable POLARIS_SECRET must be set");
            let cookie_middleware = oven::new(auth_secret.into_bytes());
            api_chain.link(cookie_middleware);
        }

        let mut mount = Mount::new();
        mount.mount("/api/", api_chain);
        mount.mount("/", Static::new(Path::new("web")));
        Iron::new(mount).http("localhost:3000").unwrap();
    });

    ui::run();
}
