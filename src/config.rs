use clap::{Arg, App};

pub struct Config {
    pub server: String,
    pub username: String,
    pub namespace: String
}

impl Config {
    pub fn from_args() -> Result<Config, ()> {
        let matches = App::new("Matrix tabsbot")
                        .version("0.1")
                        .author("Victor B. <victor.berger@m4x.org>")
                        .about("A simple matrix bot for keeping tabs in a room.")
                        .arg(Arg::with_name("SERVER")
                                .help("URL of the homeserver to connect to")
                                .required(true))
                        .arg(Arg::with_name("USERNAME")
                                .help("Username of the bot")
                                .required(true))
                        .arg(Arg::with_name("NAMESPACE")
                                .help("Namespace used by the bot to store its state")
                                .required(true))
                        .get_matches();
        let cfg = Config {
            server: matches.value_of("SERVER").unwrap().into(),
            username: matches.value_of("USERNAME").unwrap().into(),
            namespace: matches.value_of("NAMESPACE").unwrap().into()
        };

        // sanity check
        if cfg.namespace.len() == 0 {
            println!("ERROR: The namespace cannot be the empty string.");
            return Err(());
        }

        Ok(cfg)
    }
}