#[macro_use]
extern crate clap;

use reqwest;
use std::result;
use std::sync::Mutex;
use tokio;
mod config;
mod oauth_server;
mod reddit_api;
use clap::{App, Arg};
use custom_error::custom_error;

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
    RunError = "Unable to run"
}

pub type Result<T> = result::Result<T, RedeleteError>;

async fn run(username: String, dry: bool) -> Result<()> {
    let client = reddit_api::RedditClient {
        client: reddit_api::make_client()?,
        username: username,
        account_info_mutex: Mutex::new(()),
    };
    let comments = client.comments().await?;
    dbg!(comments);
    Ok(())
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
        .help("Minimum score required to prevent deletion of comment/submission.")
        .takes_value(true);
    let max_hours_arg = Arg::with_name(MAX_HOURS)
        .short("t")
        .long("max-hours")
        .help("Will not delete comments/submissions made within this many hours.")
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
                .arg(&max_hours_arg)
        )
        .subcommand(
            App::new("run")
                .about("Run the deletion part of the app.")
                .arg(
                    Arg::with_name(DRYRUN)
                        .short("d")
                        .long("dry-run")
                        .requires("run")
                        .help("Fetches comments and submissions to be deleted, then prompts to delete it."),
                )
                .arg(&username_arg)
                .arg(&exclude_arg)
                .arg(&include_arg)
                .arg(&score_arg)
                .arg(&max_hours_arg)
        )
        .subcommand(
            App::new(VIEW)
                .about("View saved configs for given <username>")
                .arg(&username_arg)
        )
        .subcommand(
            App::new(AUTHORIZE)
                .about("Authorize this application with your reddit account.")
        ).get_matches();
    if let Some(matches) = matches.subcommand_matches("config") {
        let username = matches.value_of(USERNAME).unwrap();
        if matches.is_present(MIN_SCORE) {
            let score = value_t!(matches, MIN_SCORE, u32)
                .expect("Minimum score requires an integer value.");
            match config::set_minimum_score(username.into(), score.clone()) {
                Ok(()) => println!("Set minimum score to {}", score),
                Err(e) => println!("Unable to set minimum score: {}", e),
            }
        }
        if matches.is_present(MAX_HOURS) {
            let hours = value_t!(matches, MAX_HOURS, u32)
                .expect("Maximum hours requires an integer value.");
            match config::set_max_hours(username.into(), hours.clone()) {
                Ok(()) => println!("Max hours set to {}", hours),
                Err(e) => println!("Unable to set max hours: {}", e),
            }
        }
        if let Some(inputs) = matches.values_of(ADD_EXCLUDED_SUBREDDITS) {
            let mut to_add = Vec::new();
            for input in inputs {
                to_add.push(input)
            }
            match config::add_excluded_subreddits(username.into(), to_add) {
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
        let dry = matches.value_of(DRYRUN).is_some();
        match run(matches.value_of(USERNAME).unwrap().to_string(), dry).await {
            Ok(_) => println!("Deletion run done!"),
            Err(e) => println!("{}", e),
        }
    }
}
