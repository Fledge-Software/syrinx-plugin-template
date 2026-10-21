// SPDX-License-Identifier: MPL-2.0
//! The plugin's own logic, kept apart from the WIT glue so unit tests run without Syrinx.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::bindings::exports::syrinx::plugin::forms::{
    ChoiceOption, Field, FieldError, FieldKind, FieldValue, FormSpec, SubmitResult, TextOptions,
};
use crate::bindings::exports::syrinx::plugin::views::ViewRequest;
use crate::bindings::syrinx::plugin::host::LogLevel;
use crate::bindings::syrinx::plugin::types::{Outcome, PluginError};

/// The command and tab ids, as declared in syrinx-plugin.toml.
pub const GREET_COMMAND: &str = "hello.greet";
pub const GREETINGS_TAB: &str = "hello.greetings";

/// The settings key this plugin uses, if the owner grants `storage`.
const LAST_NAME_KEY: &str = "last-name";

/// Limits the plugin keeps itself to, so its memory stays small.
const MAX_NAME_CHARS: usize = 40;
const MAX_GREETINGS: usize = 100;
const MAX_REMEMBERED_SUBMISSIONS: usize = 32;

/// The greeting words the form offers, as (value, label).
const GREETING_WORDS: &[(&str, &str)] = &[("hello", "Hello"), ("hi", "Hi"), ("welcome", "Welcome")];

/// Everything this plugin asks of Syrinx. The real implementation calls the
/// WIT imports; unit tests pass a fake.
pub trait HostApi {
    fn log(&self, level: LogLevel, message: &str);
    fn settings_get(&self, key: &str) -> Result<Option<Vec<u8>>, PluginError>;
    fn settings_set(&self, key: &str, value: &[u8]) -> Result<(), PluginError>;
    fn open_view(&self, kind: &str, address: &str) -> Result<u64, PluginError>;
    fn set_view_metadata(
        &self,
        view: u64,
        title: &str,
        badge: Option<&str>,
    ) -> Result<(), PluginError>;
    fn post_view_event(&self, view: u64, topic: &str, json: &str) -> Result<(), PluginError>;
    fn now(&self) -> i64;
}

/// One greeting the owner made through the form.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Greeting {
    pub text: String,
    pub at: i64,
}

/// The values of the form once they have been checked.
struct Checked {
    name: String,
    word: &'static str,
    shout: bool,
}

/// The plugin's state. It lives in the module, never in the web view (S3).
#[derive(Debug, Default)]
pub struct Greeter {
    last_name: Option<String>,
    greetings: Vec<Greeting>,
    open_views: BTreeSet<u64>,
    submissions: BTreeMap<String, Outcome>,
}

impl Greeter {
    pub const fn new() -> Self {
        Self {
            last_name: None,
            greetings: Vec::new(),
            open_views: BTreeSet::new(),
            submissions: BTreeMap::new(),
        }
    }

    /// Start up; read the remembered name only if `storage` was granted.
    pub fn activate(&mut self, host: &dyn HostApi, granted: &[String]) -> Result<(), PluginError> {
        *self = Self::new();
        if !granted.iter().any(|kind| kind == "storage") {
            return Ok(());
        }
        let stored = host.settings_get(LAST_NAME_KEY)?;
        self.last_name = stored.and_then(|bytes| String::from_utf8(bytes).ok());
        Ok(())
    }

    /// Running the command without its form opens the Greetings tab.
    pub fn run(&mut self, host: &dyn HostApi, id: &str) -> Result<(), PluginError> {
        if id != GREET_COMMAND {
            return Err(PluginError::NotFound(format!("no command {id}")));
        }
        host.open_view(GREETINGS_TAB, "greetings")?;
        Ok(())
    }

    /// The form the palette draws for the command.
    pub fn form_for(&self, id: &str) -> Result<FormSpec, PluginError> {
        if id != GREET_COMMAND {
            return Err(PluginError::NotFound(format!("no form for {id}")));
        }
        Ok(FormSpec {
            title: "Say hello".into(),
            submit_label: "Greet".into(),
            fields: vec![
                name_field(self.last_name.clone()),
                greeting_word_field(),
                shout_field(),
            ],
        })
    }

    /// The plugin's own checks, beyond the host's required and length checks.
    pub fn validate(&self, id: &str, values: &[FieldValue]) -> Vec<FieldError> {
        if id != GREET_COMMAND {
            return vec![field_error("", "This command has no form.")];
        }
        check_values(values).err().unwrap_or_default()
    }

    /// Act on a submitted form. A repeated submission id changes nothing.
    pub fn submit(
        &mut self,
        host: &dyn HostApi,
        id: &str,
        submission_id: &str,
        values: &[FieldValue],
    ) -> SubmitResult {
        if id != GREET_COMMAND {
            let error = PluginError::NotFound(format!("no form for {id}"));
            return SubmitResult::Finished(Outcome::Failed(error));
        }
        if let Some(earlier) = self.submissions.get(submission_id) {
            return SubmitResult::Finished(earlier.clone());
        }
        let checked = match check_values(values) {
            Ok(checked) => checked,
            Err(errors) => return SubmitResult::Invalid(errors),
        };
        let outcome = self.greet(host, &checked);
        self.remember_submission(submission_id, &outcome);
        SubmitResult::Finished(outcome)
    }

    /// The tab's interface has been loaded into a plugin view.
    pub fn view_opened(
        &mut self,
        host: &dyn HostApi,
        view: u64,
        kind: &str,
    ) -> Result<(), PluginError> {
        if kind != GREETINGS_TAB {
            return Err(PluginError::InvalidInput(format!("no tab type {kind}")));
        }
        self.open_views.insert(view);
        self.refresh_metadata(host, view)
    }

    /// Answer a request from the tab's HTML and JavaScript.
    pub fn view_message(
        &mut self,
        host: &dyn HostApi,
        request: &ViewRequest,
    ) -> Result<String, PluginError> {
        if !self.open_views.contains(&request.view) {
            return Err(PluginError::NotFound(format!(
                "no open view {}",
                request.view
            )));
        }
        match request.method.as_str() {
            "list-greetings" => Ok(to_json(&GreetingList {
                greetings: &self.greetings,
            })),
            "clear-greetings" => self.clear(host),
            other => Err(PluginError::NotFound(format!("no view method {other}"))),
        }
    }

    /// The tab closed; forget the view so no event is sent to it.
    pub fn view_closed(&mut self, view: u64) {
        self.open_views.remove(&view);
    }

    #[cfg(test)]
    pub fn greetings(&self) -> &[Greeting] {
        &self.greetings
    }

    /// Record a greeting, try to remember the name and tell open views.
    fn greet(&mut self, host: &dyn HostApi, checked: &Checked) -> Outcome {
        let greeting = Greeting {
            text: compose(checked),
            at: host.now(),
        };
        if self.greetings.len() == MAX_GREETINGS {
            self.greetings.remove(0);
        }
        self.greetings.push(greeting.clone());
        self.remember_name(host, &checked.name);
        self.announce(host, &greeting);
        Outcome::Done(Some(greeting.text))
    }

    /// Without the `storage` grant the host refuses; the plugin carries on.
    fn remember_name(&mut self, host: &dyn HostApi, name: &str) {
        self.last_name = Some(name.to_owned());
        match host.settings_set(LAST_NAME_KEY, name.as_bytes()) {
            Ok(()) => {}
            Err(PluginError::Denied(_) | PluginError::Revoked(_)) => host.log(
                LogLevel::Info,
                "storage is not granted, so the name is kept only until Syrinx closes",
            ),
            Err(_) => host.log(LogLevel::Warn, "could not remember the name"),
        }
    }

    /// Send the new greeting to every open Greetings tab.
    fn announce(&self, host: &dyn HostApi, greeting: &Greeting) {
        let json = to_json(greeting);
        for &view in &self.open_views {
            let sent = host.post_view_event(view, "greeting-added", &json);
            let refreshed = self.refresh_metadata(host, view);
            if sent.is_err() || refreshed.is_err() {
                host.log(LogLevel::Warn, "an open view did not take the update");
            }
        }
    }

    fn clear(&mut self, host: &dyn HostApi) -> Result<String, PluginError> {
        let cleared = self.greetings.len();
        self.greetings.clear();
        for &view in &self.open_views {
            self.refresh_metadata(host, view)?;
        }
        Ok(format!("{{\"cleared\":{cleared}}}"))
    }

    /// The tab's title and its badge, the number of greetings.
    fn refresh_metadata(&self, host: &dyn HostApi, view: u64) -> Result<(), PluginError> {
        let count = self.greetings.len();
        let badge = (count > 0).then(|| count.to_string());
        host.set_view_metadata(view, "Greetings", badge.as_deref())
    }

    fn remember_submission(&mut self, submission_id: &str, outcome: &Outcome) {
        if self.submissions.len() == MAX_REMEMBERED_SUBMISSIONS {
            self.submissions.pop_first();
        }
        self.submissions
            .insert(submission_id.to_owned(), outcome.clone());
    }
}

/// The JSON the tab receives for `list-greetings`.
#[derive(Serialize)]
struct GreetingList<'a> {
    greetings: &'a [Greeting],
}

fn name_field(initial: Option<String>) -> Field {
    Field {
        name: "name".into(),
        label: "Name".into(),
        kind: FieldKind::Text(TextOptions {
            max_length: MAX_NAME_CHARS as u32,
            placeholder: Some("Ada".into()),
        }),
        required: true,
        initial,
    }
}

fn greeting_word_field() -> Field {
    let options = GREETING_WORDS
        .iter()
        .map(|(value, label)| ChoiceOption {
            value: (*value).into(),
            label: (*label).into(),
        })
        .collect();
    Field {
        name: "greeting".into(),
        label: "Greeting".into(),
        kind: FieldKind::Choice(options),
        required: true,
        initial: Some("hello".into()),
    }
}

fn shout_field() -> Field {
    Field {
        name: "shout".into(),
        label: "Shout it".into(),
        kind: FieldKind::Toggle,
        required: false,
        initial: Some("false".into()),
    }
}

/// Check every field; return all the problems at once, not just the first.
fn check_values(values: &[FieldValue]) -> Result<Checked, Vec<FieldError>> {
    let value_of = |name: &str| {
        values
            .iter()
            .find(|value| value.name == name)
            .map(|value| value.value.as_str())
    };
    let mut errors = Vec::new();
    let name = check_name(value_of("name").unwrap_or_default(), &mut errors);
    let word = check_word(value_of("greeting").unwrap_or("hello"), &mut errors);
    let shout = check_toggle(value_of("shout").unwrap_or("false"), &mut errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Checked { name, word, shout })
}

fn check_name(raw: &str, errors: &mut Vec<FieldError>) -> String {
    let name = raw.trim();
    if name.is_empty() {
        errors.push(field_error("name", "Enter a name."));
    } else if name.chars().count() > MAX_NAME_CHARS {
        errors.push(field_error("name", "Use at most 40 characters."));
    } else if name.chars().any(char::is_control) {
        errors.push(field_error("name", "Use letters, not control characters."));
    }
    name.to_owned()
}

fn check_word(raw: &str, errors: &mut Vec<FieldError>) -> &'static str {
    let known = GREETING_WORDS.iter().find(|(value, _)| *value == raw);
    let Some((_, label)) = known else {
        errors.push(field_error("greeting", "Choose one of the greetings."));
        return "Hello";
    };
    label
}

fn check_toggle(raw: &str, errors: &mut Vec<FieldError>) -> bool {
    match raw {
        "true" => true,
        "false" => false,
        _ => {
            errors.push(field_error("shout", "Expected true or false."));
            false
        }
    }
}

fn field_error(name: &str, message: &str) -> FieldError {
    FieldError {
        name: name.into(),
        message: message.into(),
    }
}

/// "Hello, Ada!" or, shouted, "HELLO, ADA!".
fn compose(checked: &Checked) -> String {
    let text = format!("{}, {}!", checked.word, checked.name);
    if checked.shout {
        return text.to_uppercase();
    }
    text
}

/// Serialising these plain structs cannot fail; fall back to `null` if it did.
fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// A fake host that records calls and grants only what the test says.
    #[derive(Default)]
    struct FakeHost {
        storage_granted: bool,
        settings: RefCell<BTreeMap<String, Vec<u8>>>,
        opened: RefCell<Vec<(String, String)>>,
        events: RefCell<Vec<(u64, String, String)>>,
        badges: RefCell<Vec<(u64, Option<String>)>>,
        logs: RefCell<Vec<String>>,
    }

    impl HostApi for FakeHost {
        fn log(&self, _level: LogLevel, message: &str) {
            self.logs.borrow_mut().push(message.into());
        }
        fn settings_get(&self, key: &str) -> Result<Option<Vec<u8>>, PluginError> {
            self.check_storage()?;
            Ok(self.settings.borrow().get(key).cloned())
        }
        fn settings_set(&self, key: &str, value: &[u8]) -> Result<(), PluginError> {
            self.check_storage()?;
            self.settings
                .borrow_mut()
                .insert(key.into(), value.to_vec());
            Ok(())
        }
        fn open_view(&self, kind: &str, address: &str) -> Result<u64, PluginError> {
            self.opened.borrow_mut().push((kind.into(), address.into()));
            Ok(7)
        }
        fn set_view_metadata(
            &self,
            view: u64,
            _title: &str,
            badge: Option<&str>,
        ) -> Result<(), PluginError> {
            self.badges
                .borrow_mut()
                .push((view, badge.map(str::to_owned)));
            Ok(())
        }
        fn post_view_event(&self, view: u64, topic: &str, json: &str) -> Result<(), PluginError> {
            self.events
                .borrow_mut()
                .push((view, topic.into(), json.into()));
            Ok(())
        }
        fn now(&self) -> i64 {
            1_700_000_000_000
        }
    }

    impl FakeHost {
        fn check_storage(&self) -> Result<(), PluginError> {
            if self.storage_granted {
                return Ok(());
            }
            Err(PluginError::Denied("storage".into()))
        }
    }

    fn values(name: &str, greeting: &str, shout: &str) -> Vec<FieldValue> {
        [("name", name), ("greeting", greeting), ("shout", shout)]
            .into_iter()
            .map(|(field, value)| FieldValue {
                name: field.into(),
                value: value.into(),
            })
            .collect()
    }

    fn done_text(result: SubmitResult) -> String {
        match result {
            SubmitResult::Finished(Outcome::Done(Some(text))) => text,
            other => panic!("expected a finished greeting, got {other:?}"),
        }
    }

    #[test]
    fn the_form_has_three_fields_of_different_kinds() {
        let form = Greeter::new().form_for(GREET_COMMAND).expect("form");
        let names: Vec<&str> = form.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["name", "greeting", "shout"]);
        assert!(matches!(form.fields[0].kind, FieldKind::Text(_)));
        assert!(matches!(form.fields[1].kind, FieldKind::Choice(_)));
        assert!(matches!(form.fields[2].kind, FieldKind::Toggle));
    }

    #[test]
    fn submitting_greets_and_shouting_uppercases() {
        let host = FakeHost::default();
        let mut greeter = Greeter::new();
        let plain = greeter.submit(&host, GREET_COMMAND, "s1", &values("Ada", "hi", "false"));
        assert_eq!(done_text(plain), "Hi, Ada!");
        let loud = greeter.submit(
            &host,
            GREET_COMMAND,
            "s2",
            &values(" Bo ", "welcome", "true"),
        );
        assert_eq!(done_text(loud), "WELCOME, BO!");
        assert_eq!(greeter.greetings().len(), 2);
    }

    #[test]
    fn invalid_values_are_all_reported_and_change_nothing() {
        let host = FakeHost::default();
        let mut greeter = Greeter::new();
        let result = greeter.submit(&host, GREET_COMMAND, "s1", &values("  ", "howdy", "maybe"));
        let SubmitResult::Invalid(errors) = result else {
            panic!("expected invalid");
        };
        let fields: Vec<&str> = errors.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(fields, ["name", "greeting", "shout"]);
        assert!(greeter.greetings().is_empty());
    }

    #[test]
    fn a_long_or_control_character_name_is_refused() {
        let greeter = Greeter::new();
        let long = "x".repeat(41);
        assert_eq!(
            greeter
                .validate(GREET_COMMAND, &values(&long, "hi", "false"))
                .len(),
            1
        );
        assert_eq!(
            greeter
                .validate(GREET_COMMAND, &values("A\u{7}", "hi", "false"))
                .len(),
            1
        );
        assert!(
            greeter
                .validate(GREET_COMMAND, &values("Ada", "hi", "false"))
                .is_empty()
        );
    }

    #[test]
    fn a_repeated_submission_id_does_not_greet_twice() {
        let host = FakeHost::default();
        let mut greeter = Greeter::new();
        let first = greeter.submit(&host, GREET_COMMAND, "same", &values("Ada", "hi", "false"));
        let again = greeter.submit(&host, GREET_COMMAND, "same", &values("Ada", "hi", "false"));
        assert_eq!(done_text(first), done_text(again));
        assert_eq!(greeter.greetings().len(), 1);
    }

    #[test]
    fn without_storage_the_name_is_not_saved_and_the_greeting_still_works() {
        let host = FakeHost::default();
        let mut greeter = Greeter::new();
        greeter.activate(&host, &[]).expect("activate");
        let result = greeter.submit(&host, GREET_COMMAND, "s1", &values("Ada", "hi", "false"));
        assert_eq!(done_text(result), "Hi, Ada!");
        assert!(host.settings.borrow().is_empty());
        assert!(host.logs.borrow()[0].contains("not granted"));
    }

    #[test]
    fn with_storage_the_name_is_saved_and_prefills_the_form() {
        let host = FakeHost {
            storage_granted: true,
            ..FakeHost::default()
        };
        let mut greeter = Greeter::new();
        greeter.submit(&host, GREET_COMMAND, "s1", &values("Ada", "hi", "false"));
        greeter
            .activate(&host, &["storage".into()])
            .expect("activate");
        let form = greeter.form_for(GREET_COMMAND).expect("form");
        assert_eq!(form.fields[0].initial.as_deref(), Some("Ada"));
    }

    #[test]
    fn running_the_command_opens_the_greetings_tab() {
        let host = FakeHost::default();
        Greeter::new().run(&host, GREET_COMMAND).expect("run");
        assert_eq!(host.opened.borrow()[0].0, GREETINGS_TAB);
        assert!(Greeter::new().run(&host, "hello.other").is_err());
    }

    #[test]
    fn an_open_view_hears_of_new_greetings_and_can_list_and_clear_them() {
        let host = FakeHost::default();
        let mut greeter = Greeter::new();
        greeter.view_opened(&host, 7, GREETINGS_TAB).expect("open");
        greeter.submit(&host, GREET_COMMAND, "s1", &values("Ada", "hi", "false"));
        let events = host.events.borrow().clone();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, "greeting-added");
        assert!(events[0].2.contains("Hi, Ada!"));
        assert_eq!(host.badges.borrow().last(), Some(&(7, Some("1".into()))));

        let request = |method: &str| ViewRequest {
            view: 7,
            request_id: 1,
            method: method.into(),
            json: "{}".into(),
        };
        let listed = greeter
            .view_message(&host, &request("list-greetings"))
            .expect("list");
        assert!(listed.starts_with("{\"greetings\":[{\"text\":\"Hi, Ada!\""));
        let cleared = greeter
            .view_message(&host, &request("clear-greetings"))
            .expect("clear");
        assert_eq!(cleared, "{\"cleared\":1}");
        assert!(greeter.view_message(&host, &request("rm-rf")).is_err());

        greeter.view_closed(7);
        assert!(
            greeter
                .view_message(&host, &request("list-greetings"))
                .is_err()
        );
    }

    #[test]
    fn a_view_of_another_kind_is_refused() {
        let host = FakeHost::default();
        assert!(Greeter::new().view_opened(&host, 1, "hello.other").is_err());
    }

    #[test]
    fn only_the_latest_hundred_greetings_are_kept() {
        let host = FakeHost::default();
        let mut greeter = Greeter::new();
        for index in 0..=MAX_GREETINGS {
            let id = format!("s{index}");
            greeter.submit(
                &host,
                GREET_COMMAND,
                &id,
                &values(&format!("N{index}"), "hi", "false"),
            );
        }
        assert_eq!(greeter.greetings().len(), MAX_GREETINGS);
        assert_eq!(greeter.greetings()[0].text, "Hi, N1!");
    }
}
