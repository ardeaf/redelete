#[macro_use]
extern crate clap;

use futures::try_join;
use std::result;
use tokio;
mod config;
mod oauth_server;
mod reddit_api;
use clap::{App, Arg};
use custom_error::custom_error;
use std::time;

#[cfg(test)]
mod test_data;

const MIN_SCORE: &'static str = "min_score";
const MAX_HOURS: &'static str = "max_hours";
const ADD_EXCLUDED_SUBREDDITS: &'static str = "add_excluded";
const REMOVE_EXCLUDED_SUBREDDITS: &'static str = "remove_excluded";
const USERNAME: &'static str = "username";
const AUTHORIZE: &'static str = "authorize";
const VIEW: &'static str = "view";
const RUN: &'static str = "run";
const DRYRUN: &'static str = "dry_run";

custom_error! {pub RedeleteError
    RedditApiError{ source: reddit_api::RedditApiError } = "Reddit API Error",
    ConfigError{ source: config::ConfigError } = "Config Error",
    RunError = "Unable to run"
}

pub type Result<T> = result::Result<T, RedeleteError>;

async fn run(username: String, dry: bool) -> Result<()> {
    let client = reddit_api::RedditClient::new(username);
    let mut i: i32 = 0;
    let (mut comments, mut posts) = try_join!(client.comments(), client.posts())?;
    let mut all = Vec::new();
    all.append(&mut comments);
    all.append(&mut posts);

    let (_, ai) = config::get_config_and_account_info(&client.username)?;
    let mut printed = false;
    for p in all {
        if check_should_delete(&ai, &p) {
            if !printed {
                printed = true;
                println!("Deleting comments/submissions:")
            }
            match p.body {
                Some(s) => {
                    let max = s.len();
                    println!("comment @ /r/{}:", &p.subreddit);
                    println!("{}", &s[..max])
                }
                None => {
                    match p.title {
                        Some(s) => {
                            let max = s.len();
                            println!("submission @ /r/{}:", &p.subreddit);
                            println!("{}", &s[..max])
                        }
                        None => (),
                    }
                    match p.selftext {
                        Some(s) => {
                            let max = s.len();
                            println!("{}", &s[..max])
                        }
                        None => (),
                    }
                    match p.url {
                        Some(s) => {
                            let max = s.len();
                            println!("{}", &s[..max])
                        }
                        None => (),
                    }
                }
            }
            println!("");
            if !dry {
                client.delete(&p.name).await?;
            }
        }
    }
    if !printed {
        println!("No comments or submissions to delete.")
    }
    Ok(())
}

fn check_should_delete(ai: &config::AccountInfo, info: &reddit_api::DeletionInfo) -> bool {
    let age: u64 = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH + time::Duration::from_secs_f64(info.created_utc.clone()))
        .unwrap()
        .as_secs()
        / 3600;
    if ai.max_hours.is_none() && ai.minimum_score.is_none() && ai.excluded_subreddits.is_none() {
        return true;
    }
    if ai.max_hours.is_some() && ai.max_hours.unwrap() > age {
        return false;
    }
    if ai.minimum_score.is_some() && ai.minimum_score.unwrap() < info.score {
        return false;
    }
    if ai.excluded_subreddits.is_some()
        && ai
            .excluded_subreddits
            .clone()
            .unwrap()
            .contains(&String::from(&*info.subreddit))
    {
        return false;
    }
    return true;
}

#[tokio::main]
async fn main() {
    let exclude_arg = Arg::with_name(ADD_EXCLUDED_SUBREDDITS)
        .short("a")
        .long("add-excluded")
        .help("Adds subreddits to an exclusion list. Comments and submissions in the subreddits in this list will not be deleted.")
        .takes_value(true)
        .multiple(true);
    let include_arg = Arg::with_name(REMOVE_EXCLUDED_SUBREDDITS)
        .short("r")
        .long("remove-excluded")
        .help("Removes subreddits from the exclusion list.")
        .takes_value(true)
        .multiple(true);
    let score_arg = Arg::with_name(MIN_SCORE)
        .short("s")
        .long("min-score")
        .help("Minimum score required to prevent deletion of comment/submission. Set to 0 to remove filter.")
        .takes_value(true);
    let max_hours_arg = Arg::with_name(MAX_HOURS)
        .short("t")
        .long("max-hours")
        .help("Will not delete comments/submissions made within this many hours. Set to 0 to remove filter.")
        .takes_value(true);
    let username_arg = Arg::with_name(USERNAME)
        .help("Username to config/run the app for.")
        .index(1)
        .required(true)
        .takes_value(true);
    let matches = App::new("Redelete")
        .version("0.1")
        .author("Justin E. <ardeaf@gmail.com>")
        .about("Deletes your reddit comments and submissions")
        .subcommand(
            App::new("config")
                .about("Set default configuration options for the app.")
                .arg(&username_arg)
                .arg(&exclude_arg)
                .arg(&include_arg)
                .arg(&score_arg)
                .arg(&max_hours_arg),
        )
        .subcommand(
            App::new("run")
                .about("Run the deletion part of the app.")
                .arg(Arg::with_name(DRYRUN).short("d").long("dry-run").help(
                    "Fetches comments and submissions to be deleted, then prompts to delete it.",
                ))
                .arg(&username_arg)
                .arg(&exclude_arg)
                .arg(&include_arg)
                .arg(&score_arg)
                .arg(&max_hours_arg),
        )
        .subcommand(
            App::new(VIEW)
                .about("View saved configs for given <username>")
                .arg(&username_arg),
        )
        .subcommand(
            App::new(AUTHORIZE).about("Authorize this application with your reddit account."),
        )
        .get_matches();
    if let Some(matches) = matches.subcommand_matches("config") {
        let username = matches.value_of(USERNAME).unwrap();
        if matches.is_present(MIN_SCORE) {
            let score = value_t!(matches, MIN_SCORE, i32)
                .expect("Minimum score requires an integer value.");
            match config::set_minimum_score(username.into(), score.clone()) {
                Ok(()) => {
                    if score > 0 {
                        println!("Set minimum score to {}", score)
                    } else {
                        println!("Removed minimum score filter.")
                    }
                }
                Err(e) => println!("Unable to set minimum score: {}", e),
            }
        }
        if matches.is_present(MAX_HOURS) {
            let hours = value_t!(matches, MAX_HOURS, u64)
                .expect("Maximum hours requires an integer value.");
            match config::set_max_hours(username.into(), hours.clone()) {
                Ok(()) => {
                    if hours > 0 {
                        println!("Max hours set to {}", hours)
                    } else {
                        println!("Removed max hours filter.")
                    }
                }
                Err(e) => println!("Unable to set max hours: {}", e),
            }
        }
        if let Some(inputs) = matches.values_of(ADD_EXCLUDED_SUBREDDITS) {
            let mut to_add = Vec::new();
            for input in inputs {
                to_add.push(input);
            }
            match config::add_excluded_subreddits(username.into(), to_add) {
                Ok(v) => (),
                Err(e) => println!("Unable to set subreddit exclusion: {}", e),
            }
        }
        if let Some(inputs) = matches.values_of(REMOVE_EXCLUDED_SUBREDDITS) {
            let mut to_add = Vec::new();
            for input in inputs {
                to_add.push(input);
            }
            match config::remove_excluded_subreddits(username.into(), to_add) {
                Ok(v) => (),
                Err(e) => println!("Unable to set subreddit exclusion: {}", e),
            }
        }
    } else if matches.subcommand_matches(AUTHORIZE).is_some() {
        match reddit_api::authorize() {
            Ok(s) => println!("Authorized account {}", s),
            Err(e) => println!("Unable to authorize account. {}", e),
        }
    } else if let Some(matches) = matches.subcommand_matches(VIEW) {
        match config::read_config_account_info(matches.value_of(USERNAME).unwrap()) {
            Some(ai) => {
                println!("Settings for: {}", &ai.username);
                if ai.excluded_subreddits.is_some() {
                    println!(
                        "Excluded subreddits: {}",
                        &ai.excluded_subreddits.unwrap().join(", ")
                    );
                } else {
                    println!("Not excluding any subreddits.")
                }
                if ai.max_hours.is_some() {
                    let max_hours = ai.max_hours.unwrap();
                    println!(
                        "Not deleting any posts made within {} hour{}.",
                        max_hours.clone(),
                        if max_hours.clone() == 1 { "" } else { "s" }
                    )
                } else {
                    println!("No time minimum before deleting posts.")
                }
                if ai.minimum_score.is_some() {
                    println!(
                        "Only deleting posts with a score less than {}.",
                        ai.minimum_score.unwrap()
                    )
                } else {
                    println!("No score limit set.")
                }
            }
            None => println!(
                "Unable to find username. Did you authorize this app with that reddit account yet?"
            ),
        }
    } else if let Some(matches) = matches.subcommand_matches(RUN) {
        let dry = matches.is_present(DRYRUN);
        let username = matches.value_of(USERNAME).unwrap();
        match config::read_config_account_info(&username) {
            Some(_) => match run(username.into(), dry).await {
                Ok(_) => println!("Done."),
                Err(e) => println!("{}", e),
            },
            None => println!(
                "{} is not a saved username in your config. Try authorizing that username first.",
                &username
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    /// Need to test check_should_delete with more mixed configs to make sure.
    use super::*;
    use config::tests::{account_info, fresh_account_info};
    use reddit_api::RedditPost;
    const SUBREDDIT: &'static str = "subreddit";
    fn hours_ago_to_epoch(hours: f64) -> f64 {
        time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
            - (3600.0 * hours)
    }
    fn test_post(hours_ago: f64, score: i32) -> reddit_api::Post {
        reddit_api::Post {
            saved: true,
            name: "name".into(),
            created_utc: hours_ago_to_epoch(hours_ago),
            subreddit: String::from(SUBREDDIT),
            score,
            selftext: "".into(),
            url: "".into(),
            title: "".into(),
        }
    }

    #[test]
    fn test_no_config_delete() {
        assert_eq!(
            check_should_delete(&fresh_account_info(), &test_post(0.0, 0).deletion_info()),
            true
        )
    }
    #[test]
    fn test_max_hours_keep() {
        let mut account = account_info();
        account.minimum_score = None;
        account.excluded_subreddits = None;
        let keep = test_post(23.0, 0).deletion_info();
        assert_eq!(check_should_delete(&account, &keep), false);
    }
    #[test]
    fn test_max_hours_delete() {
        let mut account = account_info();
        account.minimum_score = None;
        account.excluded_subreddits = None;
        let delete = test_post(25.0, 0).deletion_info();
        assert_eq!(check_should_delete(&account, &delete), true);
    }
    #[test]
    fn test_minimum_score_keep() {
        let mut account = fresh_account_info();
        account.minimum_score = Some(1000);
        let keep = test_post(0.0, 1001).deletion_info();
        assert_eq!(check_should_delete(&account, &keep), false);
    }
    #[test]
    fn test_minimum_score_delete() {
        let mut account = fresh_account_info();
        account.minimum_score = Some(1000);
        let delete = test_post(25.0, 0).deletion_info();
        assert_eq!(check_should_delete(&account, &delete), true);
    }
    #[test]
    fn test_excluded_subreddits_keep() {
        let mut account = fresh_account_info();
        account.excluded_subreddits = Some(vec![SUBREDDIT.into()]);
        assert_eq!(
            check_should_delete(&account, &test_post(0.0, 0).deletion_info()),
            false
        );
    }
    #[test]
    fn test_excluded_subreddits_delete() {
        let mut account = fresh_account_info();
        account.excluded_subreddits = Some(vec!["a".into()]);
        assert_eq!(
            check_should_delete(&account, &test_post(0.0, 0).deletion_info()),
            true
        );
    }
}
