#![forbid(unsafe_code)]
use board_store::site_content;
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use std::{ffi::OsString, fs::File, process::ExitCode, str::FromStr};

const HELP: &str = "Usage: board-site-content validate <file.json>\n       board-site-content publish <file.json>\n       board-site-content page-revision <key>\n       board-site-content retract-page <key> <expected-revision>\n       board-site-content retract-news <id>\n\nvalidate is offline. Other commands require MIGRATION_DATABASE_URL and the actual\nboard_migrator login. Input is at most 32768 bytes. All content renders as escaped\nplain text. Page keys: about, faq, rules, rules/<board>, contact, feedback, legal,\npress, advertise, support, japanese, blog, announcement. New pages use expected_revision=0.\nPage envelope: {\"kind\":\"page\",\"version\":1,\"key\":\"faq\",\"expected_revision\":0,\"title\":\"FAQ\",\"body\":\"Local help\"}\nNews envelope: {\"kind\":\"news\",\"version\":1,\"published_at\":1,\"title\":\"News\",\"body\":\"Announcement\"}\nPage bodies: 8192 bytes; news bodies: 16384 bytes; titles: 256 bytes.\nNews timestamps increase monotonically; future entries appear at their timestamp.\nPublishing makes content public. Retraction hides content and retains its record.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Validate(OsString),
    Publish(OsString),
    Revision(String),
    RetractPage(String, i64),
    RetractNews(i64),
}
fn integer(value: &std::ffi::OsStr, maximum: i64) -> Option<i64> {
    value
        .to_str()
        .filter(|s| !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()))?
        .parse()
        .ok()
        .filter(|id| (1..=maximum).contains(id))
}
fn key(value: &std::ffi::OsStr) -> Option<String> {
    value
        .to_str()
        .filter(|s| site_content::valid_page_key(s))
        .map(str::to_owned)
}
fn parse(args: Vec<OsString>) -> Result<Command, String> {
    let result = match args.as_slice() {
        [command] if command == "--help" || command == "-h" => Some(Command::Help),
        [command, path] if command == "validate" => Some(Command::Validate(path.clone())),
        [command, path] if command == "publish" => Some(Command::Publish(path.clone())),
        [command, value] if command == "page-revision" => key(value).map(Command::Revision),
        [command, value] if command == "retract-news" => {
            integer(value, 10000).map(Command::RetractNews)
        }
        [command, page, revision] if command == "retract-page" => key(page)
            .zip(integer(revision, 2147483646))
            .map(|(key, revision)| Command::RetractPage(key, revision)),
        _ => None,
    };
    result.ok_or_else(|| format!("Invalid command.\n{HELP}"))
}
async fn database() -> Result<PgConnection, String> {
    let url = std::env::var("MIGRATION_DATABASE_URL")
        .map_err(|_| "MIGRATION_DATABASE_URL must be set.")?;
    let options = PgConnectOptions::from_str(&url)
        .map_err(|_| "Invalid migration connection configuration.")?;
    PgConnection::connect_with(&options)
        .await
        .map_err(|_| "Cannot connect to the migration database.".into())
}
async fn run(args: Vec<OsString>) -> Result<(), String> {
    match parse(args)? {
        Command::Help => println!("{HELP}"),
        Command::Validate(path) => {
            site_content::read_input(File::open(path).map_err(|_| "Cannot open content input.")?)
                .map_err(|e| e.to_string())?;
            println!("Valid local site content. No database accessed.");
        }
        Command::Publish(path) => {
            let input = site_content::read_input(
                File::open(path).map_err(|_| "Cannot open content input.")?,
            )
            .map_err(|e| e.to_string())?;
            let result = site_content::publish(&mut database().await?, &input)
                .await
                .map_err(|e| e.to_string())?;
            println!("Published content. New page revision or news ID: {result}");
        }
        Command::Revision(key) => {
            let revision = site_content::page_revision(&mut database().await?, &key)
                .await
                .map_err(|e| e.to_string())?;
            println!("{revision}");
        }
        Command::RetractPage(key, revision) => {
            site_content::retract_page(&mut database().await?, &key, revision)
                .await
                .map_err(|e| e.to_string())?;
            println!("Page retracted; retained revision is {}.", revision + 1);
        }
        Command::RetractNews(id) => {
            site_content::retract_news(&mut database().await?, id)
                .await
                .map_err(|e| e.to_string())?;
            println!("News entry retracted; history retained.");
        }
    }
    Ok(())
}
#[tokio::main]
async fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_commands_have_no_implicit_publication() {
        assert_eq!(
            parse(vec!["validate".into(), "owned.json".into()]).unwrap(),
            Command::Validate("owned.json".into())
        );
        assert_eq!(
            parse(vec!["retract-page".into(), "rules/g".into(), "2".into()]).unwrap(),
            Command::RetractPage("rules/g".into(), 2)
        );
        for id in ["0", "01", "+1", "-1", "1 ", "10001"] {
            assert!(parse(vec!["retract-news".into(), id.into()]).is_err());
        }
        assert!(parse(vec!["publish".into()]).is_err());
        assert!(parse(vec!["page-revision".into(), "rules/../g".into()]).is_err());
    }
}
