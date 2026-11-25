use crate::case::Case;
use crate::compare::{CallComparison, compare_calls};
use crate::mock::{MockServer, Scenario};
use crate::normalize::with_sorted_keys;
use crate::webhook::{WEBHOOK_SECRET, Webhook, WebhookSender};
use anyhow::{Context as _, Result, anyhow};
use rand::SeedableRng;
use rand::rngs::StdRng;
use rsa::PrivateKey;
use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

pub const DEFAULT_QUIET_PERIOD: Duration = Duration::from_secs(4);
pub const DEFAULT_WEBHOOK_PATH: &str = "/api/github/webhooks";
const MAXIMUM_CASE_DURATION: Duration = Duration::from_secs(40);
const GRACE_AFTER_EXPECTED_CALLS: Duration = Duration::from_millis(500);
const MOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);
const TEST_SIGNING_KEY_SEED: u64 = 0x6d65_7267_6561_626c;

static TEST_SIGNING_KEY_PEM: LazyLock<Result<String, String>> = LazyLock::new(|| {
    let mut seeded_rng = StdRng::seed_from_u64(TEST_SIGNING_KEY_SEED);
    PrivateKey::new(&mut seeded_rng, 2048)
        .map_err(|error| error.to_string())
        .and_then(|key| {
            key.to_pkcs1_pem(LineEnding::LF)
                .map(|pem| pem.to_string())
                .map_err(|error| error.to_string())
        })
});

pub fn test_signing_key_pem() -> Result<&'static str> {
    TEST_SIGNING_KEY_PEM
        .as_deref()
        .map_err(|error| anyhow!("generating the test signing key: {error}"))
}

pub fn app_environment(mock_port: u16) -> Result<BTreeMap<String, String>> {
    Ok(BTreeMap::from(
        [
            ("APP_ID", "1"),
            ("PRIVATE_KEY", test_signing_key_pem()?),
            ("WEBHOOK_SECRET", WEBHOOK_SECRET),
            ("GHE_HOST", &format!("127.0.0.1:{mock_port}")),
            ("GHE_PROTOCOL", "http"),
            ("LOG_LEVEL", "fatal"),
            ("NODE_ENV", "production"),
        ]
        .map(|(name, value)| (name.to_owned(), value.to_owned())),
    ))
}

pub fn in_process_app_environment(mock_port: u16, case: &Case) -> Result<BTreeMap<String, String>> {
    let mut environment = app_environment(mock_port)?;
    environment.insert("HOST".to_owned(), "127.0.0.1".to_owned());
    environment.insert("PORT".to_owned(), "0".to_owned());
    environment.extend(case.definition.env.clone());
    Ok(environment)
}

pub struct CaseRun {
    pub status: u16,
    pub calls: Vec<Value>,
    pub settled: bool,
}

pub fn webhook_of_case(case: &Case) -> Result<Webhook> {
    Ok(Webhook {
        event: case.definition.event.clone(),
        delivery_id: case.definition.delivery_id(),
        body: serde_json::to_vec(&case.definition.payload)?,
    })
}

#[must_use]
pub fn case_quiet_period(case: &Case, default_quiet_period: Duration) -> Duration {
    case.definition
        .quiet_ms
        .map(Duration::from_millis)
        .map_or(default_quiet_period, |case_period| {
            case_period.max(default_quiet_period)
        })
}

fn expected_call_count(case: &Case) -> Option<usize> {
    if case.definition.quiet_ms.is_some() {
        return None;
    }
    case.definition
        .expected
        .as_ref()
        .map(|expected| expected.calls.len())
}

async fn wait_for_quiet_mock(
    mock: &MockServer,
    webhook_answered_at: Instant,
    quiet_period: Duration,
    expected_calls: Option<usize>,
) -> bool {
    loop {
        tokio::time::sleep(MOCK_POLL_INTERVAL).await;
        let last_activity = mock.github.last_request_at().max(webhook_answered_at);
        let quiet_for = last_activity.elapsed();
        if quiet_for >= quiet_period {
            return true;
        }
        let reached_expected_calls =
            expected_calls.is_some_and(|expected| mock.github.recorded_call_count() >= expected);
        if reached_expected_calls && quiet_for >= GRACE_AFTER_EXPECTED_CALLS {
            return true;
        }
        if webhook_answered_at.elapsed() >= MAXIMUM_CASE_DURATION {
            return false;
        }
    }
}

pub async fn exercise_case(
    mock: &MockServer,
    app_port: u16,
    webhook_path: &str,
    case: &Case,
    quiet_period: Duration,
) -> Result<CaseRun> {
    mock.github
        .load_scenarios(vec![Scenario::from_case(&case.definition)], true);
    let status = WebhookSender::new()?
        .send(app_port, webhook_path, &webhook_of_case(case)?)
        .await
        .with_context(|| format!("sending webhook of case {}", case.definition.name))?;
    let settled = wait_for_quiet_mock(
        mock,
        Instant::now(),
        quiet_period,
        expected_call_count(case),
    )
    .await;
    Ok(CaseRun {
        status,
        calls: mock.github.take_calls(),
        settled,
    })
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    PassOrderDiffers,
    Fail,
}

fn pretty_or_missing(call: Option<&Value>) -> String {
    call.and_then(|call| serde_json::to_string_pretty(call).ok())
        .unwrap_or_else(|| "<no call>".to_owned())
}

#[must_use]
pub fn judge(case: &Case, run: &CaseRun) -> (Outcome, Option<String>) {
    let Some(expected) = &case.definition.expected else {
        return (
            Outcome::Fail,
            Some("no expected section: run replay record first".to_owned()),
        );
    };
    if expected.status != run.status {
        return (
            Outcome::Fail,
            Some(format!(
                "webhook status: expected {}, actual {}",
                expected.status, run.status
            )),
        );
    }
    match compare_calls(&expected.calls, &run.calls) {
        CallComparison::Equal => (Outcome::Pass, None),
        CallComparison::OrderDiffers { .. } if case.definition.unordered => (Outcome::Pass, None),
        CallComparison::OrderDiffers {
            first_differing_position,
        } => (
            Outcome::PassOrderDiffers,
            Some(format!(
                "same calls, order differs from position {first_differing_position}"
            )),
        ),
        CallComparison::Different {
            first_differing_position,
            expected: expected_call,
            actual: actual_call,
        } => (
            Outcome::Fail,
            Some(format!(
                "{} expected calls, {} actual; first difference at call {first_differing_position}\nexpected:\n{}\nactual:\n{}",
                expected.calls.len(),
                run.calls.len(),
                pretty_or_missing(expected_call.as_ref()),
                pretty_or_missing(actual_call.as_ref()),
            )),
        ),
    }
}

pub struct ComparableTexts {
    pub expected: String,
    pub actual: String,
}

fn readable_calls(status: Option<u16>, calls: &[Value]) -> String {
    let document = serde_json::json!({
        "status": status,
        "calls": calls.iter().map(with_sorted_keys).collect::<Vec<Value>>(),
    });
    serde_json::to_string_pretty(&document).unwrap_or_else(|error| error.to_string())
}

#[must_use]
pub fn comparable_texts(case: &Case, run: &CaseRun) -> ComparableTexts {
    let expected = case.definition.expected.as_ref();
    ComparableTexts {
        expected: readable_calls(
            expected.map(|expected| expected.status),
            expected.map_or(&[][..], |expected| expected.calls.as_slice()),
        ),
        actual: readable_calls(Some(run.status), &run.calls),
    }
}
