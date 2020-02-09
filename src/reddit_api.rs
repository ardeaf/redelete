use super::config::{delete_user, read_config_account_info, save_token, AccountInfo, ConfigError};
use super::oauth_server::{wait_for_oauth_redirect, OAuthRedirect};
use async_std::sync::Mutex;
use custom_error::custom_error;
use rate_limit::SyncLimiter;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::result;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
use webbrowser;

#[cfg(test)]
use super::test_data;
#[cfg(test)]
use mockito::Mock;
#[cfg(test)]
use mockito::{server_url, Matcher};
#[cfg(test)]
use serial_test::serial;

#[cfg(not(test))]
fn auth_domain() -> String {
    String::from("https://www.reddit.com")
}
#[cfg(not(test))]
fn domain() -> String {
    String::from("https://oauth.reddit.com")
}
#[cfg(test)]
fn auth_domain() -> String {
    String::from(&*server_url())
}
#[cfg(test)]
fn domain() -> String {
    String::from(&*server_url())
}

const DELETE_ENDPOINT: &'static str = "/api/del";
const ACCESS_TOKEN_ENDPOINT: &'static str = "/api/v1/access_token";
const ACCOUNT_INFO_ENDPOINT: &'static str = "/api/v1/me";
const USER_AGENT_STRING: &'static str = "redelete: v0.0.1 (by /u/ardeaf)";

const CLIENT_ID: &str = "8h7fZ5mmBb8uxA";
const RESPONSE_TYPE: &str = "code";
const REDIRECT_URI: &str = "http://localhost:8000";
const DURATION: &str = "permanent";
const SCOPE: &str = "history,edit,identity";

custom_error! {pub RedditApiError
    OAuthValidationError{text: String} = "Unable to authorize using oauth: {text}",
    Reqwest{source: reqwest::Error} = "Reqwest error",
    Serde{source: serde_json::Error} = "Serde parsing error",
    IO{source: std::io::Error} = "IO Error",
    Config{source:ConfigError} = "Config error",
    InvalidHeaderValue{source: reqwest::header::InvalidHeaderValue} = "Error creating headermap.",
    SystemTimeError{source: std::time::SystemTimeError} = "Error reading system time.",
    RefreshTokenError = "Unable to refresh oauth2 token",
    ParseCommentError = "Unable to parse comments from json response."
}

pub type Result<T> = result::Result<T, RedditApiError>;
#[derive(serde::Deserialize, Serialize, PartialEq, Eq, Debug, Clone)]
pub struct OAuthToken {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub scope: String,
    pub refresh_token: Option<String>,
}

#[derive(serde::Deserialize, Serialize, PartialEq, Eq, Debug, Clone)]
pub struct Expiration {
    pub expires_at: u32,
}
#[derive(Serialize, Deserialize, Debug)]
struct RedditParams {
    after: Option<String>,
    before: Option<String>,
    limit: u64,
    show: String, // can be "all"
    t: String,
}
impl RedditParams {
    pub fn as_vec(self: &Self) -> Vec<(&str, String)> {
        // ("after", &self.after.ok_or("")),
        // ("before", &self.before.ok_or(""),
        let mut vec = vec![
            ("limit", self.limit.to_string()),
            ("t", String::from(&self.t)),
        ];
        match &self.after {
            Some(s) => vec.push(("after", String::from(s))),
            None => (),
        };
        match &self.before {
            Some(s) => vec.push(("before", String::from(s))),
            None => (),
        }
        vec
    }
}
#[derive(Debug)]
pub struct DeletionInfo {
    pub saved: bool,
    pub name: String,
    pub created_utc: f64,
    pub subreddit: String,
    pub score: i32,
    pub selftext: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
}

pub trait RedditPost {
    fn deletion_info(&self) -> DeletionInfo;
}
impl RedditPost for Post {
    fn deletion_info(&self) -> DeletionInfo {
        DeletionInfo {
            saved: self.saved,
            name: String::from(&self.name),
            created_utc: self.created_utc,
            subreddit: String::from(&self.subreddit),
            score: self.score,
            selftext: Some(String::from(&self.selftext)),
            url: Some(String::from(&self.url)),
            title: Some(String::from(&self.title)),
            body: None,
        }
    }
}
impl RedditPost for Comment {
    fn deletion_info(&self) -> DeletionInfo {
        DeletionInfo {
            saved: self.saved,
            name: String::from(&self.name),
            created_utc: self.created_utc,
            subreddit: String::from(&self.subreddit),
            score: self.score,
            selftext: None,
            url: None,
            title: None,
            body: Some(String::from(&self.body)),
        }
    }
}
#[derive(Deserialize, Debug)]
pub struct Post {
    pub saved: bool,
    pub name: String,
    pub created_utc: f64,
    pub subreddit: String,
    pub score: i32,
    pub selftext: String,
    pub url: String,
    pub title: String,
}

#[derive(Deserialize, Debug)]
pub struct Comment {
    pub saved: bool,
    pub name: String,
    pub created_utc: f64,
    pub subreddit: String,
    pub score: i32,
    pub body: String,
}

pub struct RedditClient {
    client: Client,
    pub username: String,
    account_info_mutex: Mutex<()>,
    ratelimiter: SyncLimiter,
}
impl RedditClient {
    pub fn new(username: String) -> RedditClient {
        RedditClient {
            client: make_client().expect("Unable to create reqwest client."),
            username,
            account_info_mutex: Mutex::new(()),
            ratelimiter: SyncLimiter::full(55, Duration::from_secs(60)),
        }
    }
    async fn post(&self, endpoint: &str, params: &Vec<(&str, &str)>) -> Result<String> {
        let ai = self.check_account_info().await?;
        self.ratelimiter.take();
        let response = self
            .client
            .post(&format!("{}{}", domain(), endpoint))
            .bearer_auth(ai.token.access_token)
            .form(params)
            .send()
            .await?;
        let response_text = response.text().await?;
        Ok(response_text)
    }
    async fn fetch(self: &Self, endpoint: &str, params: &Vec<(&str, String)>) -> Result<String> {
        let ai = self.check_account_info().await?;
        self.ratelimiter.take();
        let a = self
            .client
            .get(&format!("{}{}", domain(), endpoint))
            .bearer_auth(ai.token.access_token)
            .query(params);
        let resp = a.send().await?;
        let response_text = resp.text().await?;
        Ok(response_text)
    }
    async fn gather_all<T: serde::de::DeserializeOwned>(
        self: &Self,
        endpoint: &str,
    ) -> Result<Vec<T>> {
        let limit: u64 = 100;
        let show = "all";
        let mut after: Option<String> = None;
        let mut total: Vec<T> = Vec::new();
        loop {
            let params = RedditParams {
                limit,
                show: String::from(show),
                after: after.clone(),
                before: None,
                t: String::from("all"),
            };
            let text = self.fetch(&endpoint, &params.as_vec()).await?;
            let mut json: Value = serde_json::from_str(&*text)?;
            let raw_posts: Vec<Value> = json["data"]["children"]
                .take()
                .as_array()
                .ok_or(RedditApiError::ParseCommentError)?
                .to_owned();
            after = match json["data"]["after"].take() {
                Value::String(s) => Some(s),
                Value::Null => None,
                _ => None,
            };
            for mut p in raw_posts.into_iter() {
                let data: Value = p["data"].take();
                let post: T = serde_json::from_value(data)?;
                total.push(post);
            }
            match after {
                Some(_) => (),
                None => break,
            }
        }
        Ok(total)
    }
    pub async fn comments<'de>(self: &Self) -> Result<Vec<DeletionInfo>> {
        let endpoint = format!("/user/{}/comments", self.username);
        let comments = self.gather_all::<Comment>(&endpoint).await?;
        let di = comments.into_iter().map(|c| c.deletion_info()).collect();
        Ok(di)
    }
    pub async fn posts<'de>(self: &Self) -> Result<Vec<DeletionInfo>> {
        let endpoint = format!("/user/{}/submitted", self.username);
        let posts = self.gather_all::<Post>(&endpoint).await?;
        let di = posts.into_iter().map(|c| c.deletion_info()).collect();
        Ok(di)
    }

    pub async fn delete(self: &Self, fullname: &str) -> Result<()> {
        let params = vec![("id", fullname)];
        let _resp = self.post(DELETE_ENDPOINT, &params).await?;
        println!("Deleted!");
        Ok(())
    }

    async fn refresh(self: &Self, refresh_token: &str) -> Result<AccountInfo> {
        println!("Refreshing OAuth2 token.");
        let new_oauth_token = self.update_token(refresh_token).await?;
        Ok(save_token(String::from(&self.username), new_oauth_token)?)
    }
    async fn check_account_info(self: &Self) -> Result<AccountInfo> {
        let _x = self.account_info_mutex.lock().await;
        let ai =
            read_config_account_info(&self.username).expect("Unable to open account config file.");
        if ai.token_expires > SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() {
            Ok(ai)
        } else {
            let refresh = self
                .refresh(&ai.token.refresh_token.expect(&format!(
                    "Unable to read refresh token from account {}, please reauthorize it.",
                    ai.username
                )))
                .await?;
            Ok(refresh)
        }
    }
    async fn update_token(self: &Self, refresh_token: &str) -> Result<OAuthToken> {
        let text = self
            .client
            .post(&format!("{}{}", auth_domain(), ACCESS_TOKEN_ENDPOINT))
            .basic_auth(CLIENT_ID, Some(""))
            .body(format!(
                "grant_type=refresh_token&refresh_token={}",
                &refresh_token
            ))
            .send()
            .await?
            .text()
            .await?;
        let mut new_oauth_token: OAuthToken = serde_json::from_str(&text)?;
        new_oauth_token.refresh_token = Some(String::from(refresh_token));
        Ok(new_oauth_token)
    }
}

fn validate_oauth_redirect(state: String, oauth_redirect: &OAuthRedirect) -> Result<()> {
    if state != oauth_redirect.state {
        Err(RedditApiError::OAuthValidationError {
            text: format!(
                "state token did not match. Expected {}. Received {}",
                state, oauth_redirect.state
            ),
        })
    } else {
        Ok(())
    }
}

fn make_client() -> Result<Client> {
    let builder = Client::builder();
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static(USER_AGENT_STRING),
    );
    Ok(builder.default_headers(headers).build()?)
}

fn get_token<'de>(oauth_redirect: &OAuthRedirect) -> Result<OAuthToken> {
    let client = make_client()?;

    let data = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}",
        String::from(&oauth_redirect.code),
        REDIRECT_URI
    );
    let access_token = Runtime::new().unwrap().block_on(async {
        let t = client
            .post(&format!("{}{}", &auth_domain(), ACCESS_TOKEN_ENDPOINT))
            .basic_auth(CLIENT_ID, Some(""))
            .body(data)
            .send()
            .await
            .expect("Unable to post data")
            .text()
            .await
            .expect("Unable to parse auth response");
        let token: OAuthToken =
            serde_json::from_str(&t).expect("Unable to serialize access token response text");
        token
    });
    Ok(access_token)
}

#[derive(Deserialize, Debug)]
struct User {
    name: String,
}

pub fn username(token: &OAuthToken) -> Result<String> {
    let client = make_client()?;
    let user_info = Runtime::new().unwrap().block_on(async {
        client
            .get(&format!("{}{}", domain(), ACCOUNT_INFO_ENDPOINT))
            .bearer_auth(&token.access_token)
            .send()
            .await
            .expect("Couldn't get user info.")
            .text()
            .await
            .expect("Couldn't parse user information response")
    });
    let user: User = serde_json::from_str(&user_info)?;
    Ok(user.name)
}

pub fn authorize() -> Result<String> {
    // I don't see how to test this without installing a webdriver and using a dummy account. I don't want to do that.
    let state = open_authorization_page()?;
    let oauth_redirect = wait_for_oauth_redirect(8000, 8001).unwrap();
    validate_oauth_redirect(state, &oauth_redirect)?;
    let access_token = get_token(&oauth_redirect)?;
    let username = username(&access_token)?;
    save_token(username.clone(), access_token)?;
    Ok(username)
}
fn open_authorization_page() -> Result<String> {
    println!("Opening browser, please authorize redelete to access your account.");
    let state = nanoid::simple();
    let url = format!(
        "{}/api/v1/authorize?client_id={}&response_type={}&state={}&redirect_uri={}&duration={}&scope={}",
        auth_domain(),
        CLIENT_ID,
        RESPONSE_TYPE,
        state,
        REDIRECT_URI,
        DURATION,
        SCOPE
    );
    webbrowser::open(&url)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::mock;

    const STATE: &'static str = "abcdefg";
    const TEST_USER: &'static str = "TestUser";
    const REFRESHED_ACCESS_TOKEN: &'static str = "REFRESHED_ACCESS_TOKEN";
    const REFRESH_TOKEN_BODY: &'static str = r#"{
    "access_token": "REFRESHED_ACCESS_TOKEN",
    "token_type": "bearer",
    "expires_in": 3600,
    "scope": "history,edit,account"
}"#;
    const ACCESS_TOKEN: &'static str = "ACCESS_TOKEN";
    const TOKEN_BODY: &'static str = r#"{
    "access_token": "ACCESS_TOKEN", 
    "token_type": "bearer", 
    "expires_in": 3600,
    "scope": "history,edit,account",
    "refresh_token": "REFRESH_TOKEN"
}"#;
    const USER_INFO_BODY: &'static str = "{\"is_employee\": false, \"seen_layout_switch\": true, \"has_visited_new_profile\": true, \"pref_no_profanity\": false, \"has_external_account\": false, \"pref_geopopular\": \"GLOBAL\", \"seen_redesign_modal\": true, \"pref_show_trending\": true, \"subreddit\": {\"default_set\": true, \"user_is_contributor\": false, \"banner_img\": \"\", \"restrict_posting\": true, \"user_is_banned\": false, \"free_form_reports\": true, \"community_icon\": \"\", \"show_media\": true, \"icon_color\": \"#A5A4A4\", \"user_is_muted\": false, \"display_name\": \"u_ardeaf\", \"header_img\": null, \"title\": \"\", \"coins\": 0, \"over_18\": false, \"icon_size\": [256, 256], \"primary_color\": \"\", \"icon_img\": \"https://www.redditstatic.com/avatars/avatar_default_01_A5A4A4.png\", \"description\": \"\", \"submit_link_label\": \"\", \"header_size\": null, \"restrict_commenting\": false, \"subscribers\": 0, \"submit_text_label\": \"\", \"is_default_icon\": true, \"link_flair_position\": \"\", \"display_name_prefixed\": \"u/ardeaf\", \"key_color\": \"\", \"name\": \"t5_auurt\", \"is_default_banner\": true, \"url\": \"/user/ardeaf/\", \"banner_size\": null, \"user_is_moderator\": true, \"public_description\": \"\", \"link_flair_enabled\": false, \"disable_contributor_requests\": false, \"subreddit_type\": \"user\", \"user_is_subscriber\": false}, \"is_sponsor\": false, \"gold_expiration\": null, \"has_gold_subscription\": false, \"num_friends\": 0, \"features\": {\"promoted_trend_blanks\": true, \"show_amp_link\": true, \"chat\": true, \"twitter_embed\": true, \"is_email_permission_required\": false, \"mod_awards\": true, \"expensive_coins_package\": true, \"mweb_xpromo_revamp_v2\": {\"owner\": \"growth\", \"variant\": \"treatment_5\", \"experiment_id\": 457}, \"awards_on_streams\": true, \"mweb_xpromo_modal_listing_click_daily_dismissible_ios\": true, \"community_awards\": true, \"modlog_copyright_removal\": true, \"do_not_track\": true, \"chat_user_settings\": true, \"mweb_xpromo_interstitial_comments_ios\": true, \"chat_subreddit\": true, \"mweb_xpromo_modal_listing_click_daily_dismissible_android\": true, \"premium_subscriptions_table\": true, \"mweb_xpromo_interstitial_comments_android\": true, \"mweb_nsfw_xpromo\": {\"owner\": \"growth\", \"variant\": \"control_2\", \"experiment_id\": 361}, \"delete_vod_when_post_is_deleted\": true, \"awarder_names\": true, \"chat_group_rollout\": true, \"custom_feeds\": true, \"spez_modal\": true, \"mweb_sharing_clipboard\": {\"owner\": \"growth\", \"variant\": \"treatment_1\", \"experiment_id\": 315}}, \"has_android_subscription\": false, \"verified\": true, \"pref_autoplay\": true, \"coins\": 0, \"has_paypal_subscription\": false, \"has_subscribed_to_premium\": false, \"id\": \"dp1yw\", \"has_stripe_subscription\": false, \"seen_premium_adblock_modal\": false, \"can_create_subreddit\": true, \"over_18\": true, \"is_gold\": false, \"is_mod\": false, \"suspension_expiration_utc\": null, \"has_verified_email\": true, \"is_suspended\": false, \"pref_video_autoplay\": true, \"in_redesign_beta\": true, \"icon_img\": \"https://www.redditstatic.com/avatars/avatar_default_01_A5A4A4.png\", \"pref_nightmode\": true, \"oauth_client_id\": \"8h7fZ5mmBb8uxA\", \"hide_from_robots\": true, \"link_karma\": 102, \"force_password_reset\": false, \"seen_give_award_tooltip\": false, \"inbox_count\": 0, \"pref_top_karma_subreddits\": true, \"pref_show_snoovatar\": false, \"name\": \"ardeaf\", \"pref_clickgadget\": 5, \"created\": 1383088257.0, \"gold_creddits\": 0, \"created_utc\": 1383059457.0, \"has_ios_subscription\": false, \"pref_show_twitter\": false, \"in_beta\": false, \"comment_karma\": 4144, \"has_subscribed\": true, \"seen_subreddit_chat_ftux\": false}";

    fn oauth_redirect() -> OAuthRedirect {
        OAuthRedirect {
            state: STATE.into(),
            code: String::from("abc"),
        }
    }

    fn token() -> OAuthToken {
        serde_json::from_str(TOKEN_BODY).unwrap()
    }

    #[test]
    fn test_username() -> Result<()> {
        let _m = mock("GET", ACCOUNT_INFO_ENDPOINT)
            .with_body(USER_INFO_BODY)
            .create();
        let username = username(&token())?;
        assert_eq!(username, "ardeaf");
        Ok(())
    }
    #[test]
    fn test_validate_oauth_redirect() {
        assert_eq!(
            (),
            validate_oauth_redirect(String::from(STATE), &oauth_redirect()).unwrap()
        );
        assert_eq!(
            validate_oauth_redirect(String::from(""), &oauth_redirect()).is_err(),
            true
        );
    }
    #[test]
    fn test_fetch_token() {
        let _m = mock("POST", ACCESS_TOKEN_ENDPOINT)
            .with_body(TOKEN_BODY)
            .create();
        let token: OAuthToken = serde_json::from_str(TOKEN_BODY).unwrap();
        assert_eq!(get_token(&oauth_redirect()).unwrap(), token)
    }

    fn expired_token_mocks() -> (mockito::Mock, mockito::Mock, mockito::Mock) {
        let rejected = r#"{"message": "Unauthorized", "error": 401}"#;
        let m1 = mock("GET", ACCOUNT_INFO_ENDPOINT)
            .match_header("authorization", "Bearer ACCESS_TOKEN")
            .with_status(401)
            .with_body(rejected)
            .create();
        let m2 = mock("GET", ACCOUNT_INFO_ENDPOINT)
            .match_header("authorization", "Bearer REFRESHED_ACCESS_TOKEN")
            .with_body(USER_INFO_BODY)
            .create();
        let m3 = mock("POST", ACCESS_TOKEN_ENDPOINT)
            .with_body(REFRESH_TOKEN_BODY)
            .create();
        (m1, m2, m3)
    }

    #[test]
    #[serial]
    fn test_update_token() {
        let (_m1, _m2, _m3) = expired_token_mocks();
        let reddit_client = reddit_client(String::from(TEST_USER));
        save_token(reddit_client.username.clone(), token()).unwrap();
        let new_token = Runtime::new().unwrap().block_on(async {
            reddit_client
                .update_token(&token().refresh_token.unwrap())
                .await
                .unwrap()
        });
        assert_eq!(new_token.refresh_token.unwrap(), "REFRESH_TOKEN");
        assert_eq!(new_token.access_token, REFRESHED_ACCESS_TOKEN);
        delete_user(TEST_USER).unwrap();
    }

    #[test]
    #[serial]
    fn test_expired_token() {
        let username = String::from(TEST_USER);
        let mut token = token();
        token.expires_in = 0;
        save_token(String::from(&username), token).unwrap();
        let client = reddit_client(String::from(&username));
        let (_m1, _m2, _m3) = expired_token_mocks();
        let _resp = Runtime::new()
            .unwrap()
            .block_on(async { client.fetch(ACCOUNT_INFO_ENDPOINT, &vec![]).await.unwrap() });
        let ai = read_config_account_info(&username).unwrap();
        assert_eq!(ai.token.access_token, REFRESHED_ACCESS_TOKEN);
        delete_user(&username).unwrap();
    }
    fn reddit_client(username: String) -> RedditClient {
        RedditClient::new(username)
    }

    #[test]
    #[serial]
    fn test_post() {
        let _m = mock("POST", ACCOUNT_INFO_ENDPOINT)
            .with_body(TOKEN_BODY)
            .with_status(200)
            .create();
        let reddit_client = reddit_client(String::from(TEST_USER));
        save_token(String::from(&reddit_client.username), token()).unwrap();
        let resp = Runtime::new().unwrap().block_on(async {
            reddit_client
                .post(ACCOUNT_INFO_ENDPOINT, &vec![])
                .await
                .unwrap()
        });
        delete_user(&reddit_client.username).unwrap();
        assert_eq!(resp, TOKEN_BODY)
    }

    #[test]
    #[serial]
    fn test_fetch() {
        let _m = mock("GET", ACCOUNT_INFO_ENDPOINT)
            .with_body(TOKEN_BODY)
            .with_status(200)
            .create();
        let reddit_client = reddit_client(String::from(TEST_USER));
        save_token(String::from(&reddit_client.username), token()).unwrap();
        let resp = Runtime::new().unwrap().block_on(async {
            reddit_client
                .fetch(ACCOUNT_INFO_ENDPOINT, &vec![])
                .await
                .unwrap()
        });
        delete_user(&reddit_client.username).unwrap();
        assert_eq!(resp, TOKEN_BODY)
    }
    #[test]
    #[serial]
    fn test_comments() {
        let comments = test_data::comments();
        let end = 40;
        let mocks: Vec<Mock> = (0..=end.clone())
            .map(|i| {
                let after = match i {
                    40 => String::from("null"),
                    x => format!(r#""{}""#, x + 1),
                };
                let body = format!(
                    r#"{{
                        "kind": "Listing",
                        "data": {{
                            "modhash": "masdfkmasdlfmasfmaslfm",
                            "dist": 7,
                            "children": [
                                {}
                            ],
                            "after": {},
                            "before": null
                        }}
                    }}"#,
                    &String::from(comments.join(", ")),
                    &after
                );
                // let endpoint = &format!("/user/{}/comments", TEST_USER);
                if i.clone() > 0 {
                    mock("GET", Matcher::Any)
                        .match_query(Matcher::UrlEncoded("after".into(), i.to_string()))
                        .with_body(body)
                        .with_status(200)
                        .create()
                } else {
                    mock("GET", Matcher::Any)
                        .with_body(body)
                        .with_status(200)
                        .create()
                }
            })
            .collect();
        let reddit_client = reddit_client(String::from(TEST_USER));
        save_token(String::from(&reddit_client.username), token()).unwrap();
        let fetched_comments = Runtime::new()
            .unwrap()
            .block_on(async { reddit_client.comments().await.unwrap() });
        for mock in mocks {
            mock.assert();
        }
        delete_user(TEST_USER).unwrap();
        assert_eq!(fetched_comments.len(), (end + 1) * comments.len())
    }

    #[test]
    #[serial]
    fn test_posts() {
        let posts = test_data::posts();
        let end = 50;
        let mocks: Vec<Mock> = (0..=end.clone())
            .map(|i| {
                let after = match i {
                    50 => String::from("null"),
                    x => format!(r#""{}""#, x + 1),
                };
                let body = format!(
                    r#"{{
                        "kind": "Listing",
                        "data": {{
                            "modhash": "masdfkmasdlfmasfmaslfm",
                            "dist": 7,
                            "children": [
                                {}
                            ],
                            "after": {},
                            "before": null
                        }}
                    }}"#,
                    &String::from(posts.join(", ")),
                    &after
                );
                // let endpoint = &format!("/user/{}/submitted", TEST_USER);
                if i.clone() > 0 {
                    mock("GET", Matcher::Any)
                        .match_query(Matcher::UrlEncoded("after".into(), i.to_string()))
                        .with_body(body)
                        .with_status(200)
                        .create()
                } else {
                    mock("GET", Matcher::Any)
                        .with_body(body)
                        .with_status(200)
                        .create()
                }
            })
            .collect();
        let reddit_client = reddit_client(String::from(TEST_USER));
        save_token(String::from(&reddit_client.username), token()).unwrap();
        let _fetched_comments = Runtime::new()
            .unwrap()
            .block_on(async { reddit_client.posts().await.unwrap() });
        for mock in mocks {
            mock.assert();
        }
        delete_user(TEST_USER).unwrap();
    }

    #[test]
    #[serial]
    fn test_delete() {
        let client = reddit_client(String::from(TEST_USER));
        save_token(String::from(&client.username), token()).unwrap();
        let _m = mock("POST", DELETE_ENDPOINT).with_status(204).create();
        let res = Runtime::new()
            .unwrap()
            .block_on(async { client.delete("t1_a").await.unwrap() });
        assert_eq!((), res)
    }
}
