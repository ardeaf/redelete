use super::reddit_api::OAuthToken;
use custom_error::custom_error;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::prelude::*;
use std::path::PathBuf;
use std::result;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use serial_test::serial;

fn config_dir() -> PathBuf {
    let dirs = ProjectDirs::from("", "", "redelete").expect("Cannot create config folder.");
    let path: PathBuf = dirs.config_dir().into();
    std::fs::create_dir_all(&path).expect("Unable to create config directory.");
    path
}

fn config_file_path() -> PathBuf {
    let mut config_dir = config_dir();
    config_dir.push("redelete.conf");
    config_dir
}
#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    accounts: Vec<AccountInfo>,
}
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct AccountInfo {
    pub token: OAuthToken,
    pub username: String,
    pub token_expires: u64,
    pub excluded_subreddits: Option<Vec<String>>,
    pub minimum_score: Option<i32>,
    pub max_hours: Option<u64>,
}

#[cfg_attr(tarpaulin, skip)]
custom_error! {pub ConfigError
    Serde{source: serde_json::Error} = "Serde parsing error",
    IO{source: std::io::Error} = "IO Error",
    TOML{source: toml::ser::Error} = "Toml parsing error",
    NotFound {what: String} = "{what} not found"
}
pub type Result<T> = result::Result<T, ConfigError>;

fn save_config(config: Config) -> Result<()> {
    let file_path = config_file_path();
    let mut file = File::create(file_path)?;
    let json = serde_json::to_string(&config)
        .expect(&format!("Unable to parse config to save {:?}", &config));
    file.write_all(&json.as_bytes())?;
    Ok(())
}

pub fn get_config_and_account_info(username: &str) -> Result<(Config, AccountInfo)> {
    let mut config = get_config()?;
    let accounts = config.accounts.clone();
    let account_info = accounts
        .iter()
        .find(|&account| account.username == username)
        .ok_or(ConfigError::NotFound {
            what: format!("Username {}", &username),
        })?;
    config
        .accounts
        .retain(|account| account.username != username);
    Ok((config, account_info.to_owned()))
}

pub fn remove_excluded_subreddits(username: String, subreddits: Vec<&str>) -> Result<()> {
    let (mut c, mut ai) = get_config_and_account_info(&username)?;
    let es: Vec<String> = ai
        .excluded_subreddits
        .unwrap_or(Vec::new())
        .into_iter()
        .filter(|sr| !subreddits.contains(&sr.as_str()))
        .collect();
    set_excluded_subreddits(username, es)?;
    Ok(())
}

pub fn add_excluded_subreddits(username: String, subreddits: Vec<&str>) -> Result<()> {
    let (mut c, mut ai) = get_config_and_account_info(&username)?;
    let mut es = ai.excluded_subreddits.unwrap_or(Vec::new()).clone();
    for sr in subreddits {
        let s = String::from(sr);
        if !es.contains(&s) {
            es.push(s)
        }
    }
    set_excluded_subreddits(username, es)?;
    Ok(())
}

pub fn set_excluded_subreddits(username: String, excluded_subreddits: Vec<String>) -> Result<()> {
    let (mut c, mut ai) = get_config_and_account_info(&username)?;
    ai.excluded_subreddits = if excluded_subreddits.len() > 0 {
        Some(excluded_subreddits)
    } else {
        None
    };
    c.accounts.push(ai.clone());
    save_config(c)?;
    match ai.excluded_subreddits {
        Some(x) => println!("Excluded subreddits set to {}.", &x.join(", ")),
        None => println!("Excluded subreddits updated -- no subreddits are excluded."),
    }
    Ok(())
}

pub fn set_max_hours(username: String, max_hours: u64) -> Result<()> {
    let (mut c, mut ai) = get_config_and_account_info(&username)?;
    if max_hours > 0 {
        ai.max_hours = Some(max_hours);
    } else {
        ai.max_hours = None;
    }
    c.accounts.push(ai.clone());
    Ok(save_config(c)?)
}

pub fn set_minimum_score(username: String, score: i32) -> Result<()> {
    let (mut c, mut ai) = get_config_and_account_info(&username)?;
    if score > 0 {
        ai.minimum_score = Some(score);
    } else {
        ai.minimum_score = None;
    }
    c.accounts.push(ai.clone());
    Ok(save_config(c)?)
}

pub fn save_token(username: String, token: OAuthToken) -> Result<AccountInfo> {
    let token_expires = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Couldn't get systemtime")
        .as_secs()
        + &token.expires_in;
    let (mut config, account_info) = match get_config_and_account_info(&username) {
        Ok((mut c, mut ai)) => {
            ai.token = token;
            ai.token_expires = token_expires;
            (c, ai)
        }
        Err(_) => {
            let c = get_config()?;
            let ai = AccountInfo {
                username: String::from(&username),
                token,
                token_expires,
                minimum_score: None,
                excluded_subreddits: None,
                max_hours: None,
            };
            (c, ai)
        }
    };
    let cloned = account_info.clone();
    config.accounts.push(cloned);
    save_config(config)?;
    println!(
        "Saved user <{}> information into {}",
        username,
        config_file_path().display()
    );
    Ok(account_info)
}

pub fn update_token(username: String, token: OAuthToken) -> Result<()> {
    let account_config =
        read_config_account_info(&username).expect(&format!("Unable to find user {}", username));
    let mut token_ = token.clone();
    token_.refresh_token = account_config.token.refresh_token;
    save_token(username, token_)?;
    Ok(())
}

fn get_config() -> Result<Config> {
    if !config_file_path().exists() {
        let mut dirs = config_dir();
        std::fs::create_dir_all(dirs)?;
        let f = File::create(config_file_path())?;
        Ok(Config {
            accounts: Vec::new(),
        })
    } else {
        let file_path = config_file_path();
        let mut file = std::fs::File::open(&file_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        if &contents == "" {
            Ok(Config {
                accounts: Vec::new(),
            })
        } else {
            let config: Config = serde_json::from_str(&contents).unwrap();
            Ok(config)
        }
    }
}

pub fn delete_user(username: &str) -> Result<bool> {
    let config = get_config().unwrap();
    let mut accounts: Vec<AccountInfo> = Vec::new();
    for account in &config.accounts {
        if account.username != String::from(username) {
            let acct = account.clone();
            accounts.push(acct);
        } else {
            continue;
        }
    }
    if accounts.len() < config.accounts.len() {
        save_config(Config { accounts }).expect("Failed to delete user from config.");
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn read_config_account_info(username: &str) -> Option<AccountInfo> {
    let config = get_config().unwrap();
    for account in config.accounts {
        if account.username == String::from(username) {
            return Some(account);
        }
    }
    None
}

#[cfg(test)]
pub mod tests {
    use super::*;
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_config_dir() {
        assert_eq!(
            config_dir(),
            PathBuf::from(&*format!(
                "{}/.config/redelete",
                dirs::home_dir().unwrap().to_str().unwrap()
            ))
        )
    }
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_token_file_path() {
        assert_eq!(
            config_file_path(),
            PathBuf::from(&format!(
                "{}/.config/redelete/redelete.conf",
                dirs::home_dir().unwrap().to_str().unwrap()
            ))
        )
    }

    pub fn token() -> OAuthToken {
        OAuthToken {
            access_token: "ACCESS_TOKEN".into(),
            token_type: "bearer".into(),
            expires_in: 3600,
            scope: "history,edit,account".into(),
            refresh_token: Some("REFRESH_TOKEN".into()),
        }
    }
    pub fn test_username() -> String {
        "TestUser".into()
    }
    pub fn fresh_account_info() -> AccountInfo {
        AccountInfo {
            username: test_username(),
            token: token(),
            token_expires: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + token().expires_in,
            excluded_subreddits: None,
            max_hours: None,
            minimum_score: None,
        }
    }

    pub fn account_info() -> AccountInfo {
        AccountInfo {
            username: test_username(),
            token: token(),
            token_expires: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + token().expires_in,
            excluded_subreddits: Some(vec!["a".into(), "b".into(), "c".into()]),
            max_hours: Some(24),
            minimum_score: Some(1000),
        }
    }

    #[test]
    #[serial]
    fn test_save_get_delete_user() {
        save_token(test_username(), token()).unwrap();
        assert_eq!(
            read_config_account_info(&test_username()).unwrap(),
            fresh_account_info()
        );
        assert_eq!(delete_user(&test_username()).unwrap(), true);
    }

    #[test]
    #[serial]
    fn test_no_duplicates() {
        let before_config = get_config().unwrap();
        let token1 = token();
        let token2 = token();
        save_token(test_username(), token1).unwrap();
        save_token(test_username(), token2).unwrap();
        let config = get_config().unwrap();
        assert_eq!(config.accounts.len(), before_config.accounts.len() + 1);
        delete_user(&test_username()).unwrap();
    }
    #[test]
    #[serial]
    fn test_refresh_user_token() {
        const NEW_ACCESS_TOKEN: &'static str = "NEW_ACCESS_TOKEN";
        save_token(test_username(), token()).unwrap();
        let mut new_token = token();
        new_token.refresh_token = None;
        new_token.access_token = NEW_ACCESS_TOKEN.into();
        update_token(test_username(), new_token).unwrap();
        let account_info = read_config_account_info(&test_username()).unwrap();
        assert_eq!(account_info.token.access_token, NEW_ACCESS_TOKEN);
        assert_eq!(
            account_info.token.refresh_token,
            Some("REFRESH_TOKEN".into())
        );
        delete_user(&test_username()).unwrap();
    }
    #[test]
    #[serial]
    fn test_set_max_hours() {
        save_token(test_username(), token()).unwrap();
        assert_eq!(set_max_hours(test_username(), 1).unwrap(), ());
        delete_user(&test_username()).unwrap();
    }
    #[test]
    #[serial]
    fn test_set_minimum_score() {
        save_token(test_username(), token()).unwrap();
        assert_eq!(set_minimum_score(test_username(), 1000).unwrap(), ());
        delete_user(&test_username()).unwrap();
    }
    #[test]
    #[serial]
    fn test_set_excluded_subreddits() {
        save_token(test_username(), token()).unwrap();
        assert_eq!(
            set_excluded_subreddits(test_username(), vec!["a".into(), "b".into(), "c".into()])
                .unwrap(),
            ()
        );
        let account_info = read_config_account_info(&test_username()).unwrap();
        assert_eq!(
            account_info.excluded_subreddits,
            Some(vec!["a".into(), "b".into(), "c".into()])
        );
        assert_eq!(
            set_excluded_subreddits(test_username(), vec![]).unwrap(),
            ()
        );
        let account_info = read_config_account_info(&test_username()).unwrap();
        assert_eq!(account_info.excluded_subreddits, None);
        assert_eq!(
            add_excluded_subreddits(test_username(), vec!["a"]).unwrap(),
            ()
        );
        let account_info = read_config_account_info(&test_username()).unwrap();
        assert_eq!(account_info.excluded_subreddits, Some(vec!["a".into()]));
        assert_eq!(
            remove_excluded_subreddits(test_username(), vec!["a"]).unwrap(),
            ()
        );
        let account_info = read_config_account_info(&test_username()).unwrap();
        assert_eq!(account_info.excluded_subreddits, None);
        delete_user(&test_username()).unwrap();
    }
}
