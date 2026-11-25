use super::mock_context::mock_context;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

fn iso_time_ago(elapsed: SignedDuration) -> Value {
    json!(format!("{:.3}", Timestamp::now() - elapsed))
}

#[tokio::test]
async fn checks_that_days_is_validated_properly() {
    let settings = json!({"do": "age", "created_at": {"days": 1}});

    let created_an_hour_ago =
        mock_context(&json!({"createdAt": iso_time_ago(SignedDuration::from_hours(1))})).await;
    let validation = created_an_hour_ago.process_validate("age", &settings).await;
    assert_eq!(validation["status"], "fail");

    let created_two_days_ago =
        mock_context(&json!({"createdAt": iso_time_ago(SignedDuration::from_hours(2 * 24))})).await;
    let validation = created_two_days_ago
        .process_validate("age", &settings)
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn that_updated_at_option_is_working() {
    let settings = json!({"do": "age", "updated_at": {"days": 1}});
    let created_at = iso_time_ago(SignedDuration::from_hours(3 * 24));

    let updated_an_hour_ago = mock_context(
        &json!({"createdAt": created_at, "updatedAt": iso_time_ago(SignedDuration::from_hours(1))}),
    )
    .await;
    let validation = updated_an_hour_ago.process_validate("age", &settings).await;
    assert_eq!(validation["status"], "fail");

    let updated_two_days_ago = mock_context(
        &json!({"createdAt": created_at, "updatedAt": iso_time_ago(SignedDuration::from_hours(2 * 24))}),
    )
    .await;
    let validation = updated_two_days_ago
        .process_validate("age", &settings)
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn that_message_option_works_properly() {
    let settings = json!({
        "do": "age",
        "created_at": {"days": 1, "message": "You need to wait at least one day before you can merge the PR"},
    });
    let github =
        mock_context(&json!({"createdAt": iso_time_ago(SignedDuration::from_hours(1))})).await;
    let validation = github.process_validate("age", &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "You need to wait at least one day before you can merge the PR"
    );
}
