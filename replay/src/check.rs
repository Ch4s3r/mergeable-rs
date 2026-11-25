use crate::app_process::AppProgram;
use crate::case::Case;
use crate::compare::{CallComparison, compare_calls};
use crate::runner::{
    CaseRun, RunOptions, for_each_in_parallel, run_case_in_fresh_process, run_cases_in_one_process,
};
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

const HARNESS_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    PassOrderDiffers,
    Fail,
}

#[derive(Debug, Serialize)]
pub struct CaseReport {
    pub name: String,
    pub outcome: Outcome,
    pub known_diff: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub app: String,
    pub passed: usize,
    pub passed_with_order_difference: usize,
    pub failed: usize,
    pub known_diffs: usize,
    pub cases: Vec<CaseReport>,
}

fn pretty_or_missing(call: Option<&Value>) -> String {
    call.and_then(|call| serde_json::to_string_pretty(call).ok())
        .unwrap_or_else(|| "<no call>".to_owned())
}

fn judge(case: &Case, run: &CaseRun) -> (Outcome, Option<String>) {
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

async fn run_all(
    program: AppProgram,
    cases: Arc<Vec<Case>>,
    options: &RunOptions,
) -> Result<Vec<Result<CaseRun>>> {
    if options.reuse_process {
        return run_cases_in_one_process(&program, &cases, options, "reused.check").await;
    }
    let options_for_jobs = options.clone();
    let case_indices: Vec<usize> = (0..cases.len()).collect();
    for_each_in_parallel(case_indices, options.jobs, move |case_index| {
        let program = program.clone();
        let options = options_for_jobs.clone();
        let cases = cases.clone();
        async move {
            let case = &cases[case_index];
            let log_name = format!("{}.check", case.definition.name);
            let mut outcome = run_case_in_fresh_process(&program, case, &options, &log_name).await;
            for _ in 1..HARNESS_ATTEMPTS {
                if outcome.is_ok() {
                    break;
                }
                outcome = run_case_in_fresh_process(&program, case, &options, &log_name).await;
            }
            outcome
        }
    })
    .await
}

pub async fn check(binary: &Path, cases: Vec<Case>, options: &RunOptions) -> Result<CheckReport> {
    let program = AppProgram::binary(binary)?;
    let cases = Arc::new(cases);
    let runs = run_all(program.clone(), cases.clone(), options).await?;
    let mut reports = Vec::with_capacity(cases.len());
    for (case, run) in cases.iter().zip(&runs) {
        let (outcome, detail) = match run {
            Ok(run) => judge(case, run),
            Err(error) => (Outcome::Fail, Some(format!("run failed: {error:#}"))),
        };
        let known_diff = case.definition.known_diff.clone();
        let label = match (outcome, &known_diff) {
            (Outcome::Pass, _) => "PASS",
            (Outcome::PassOrderDiffers, _) => "PASS (order differs)",
            (Outcome::Fail, Some(_)) => "KNOWN DIFF",
            (Outcome::Fail, None) => "FAIL",
        };
        println!("{label} {}", case.definition.name);
        if let Some(reason) = &known_diff {
            println!("  known diff: {reason}");
        }
        if let Some(detail) = &detail {
            for line in detail.lines() {
                println!("  {line}");
            }
        }
        reports.push(CaseReport {
            name: case.definition.name.clone(),
            outcome,
            known_diff,
            detail,
        });
    }
    let count = |wanted: Outcome, known: bool| {
        reports
            .iter()
            .filter(|report| report.outcome == wanted && report.known_diff.is_some() == known)
            .count()
    };
    let report = CheckReport {
        app: program.label(),
        passed: count(Outcome::Pass, false) + count(Outcome::Pass, true),
        passed_with_order_difference: count(Outcome::PassOrderDiffers, false)
            + count(Outcome::PassOrderDiffers, true),
        failed: count(Outcome::Fail, false),
        known_diffs: reports
            .iter()
            .filter(|report| report.known_diff.is_some())
            .count(),
        cases: reports,
    };
    let order_differences: Vec<&str> = report
        .cases
        .iter()
        .filter(|case_report| case_report.outcome == Outcome::PassOrderDiffers)
        .map(|case_report| case_report.name.as_str())
        .collect();
    println!(
        "\n{} passed, {} passed with different call order, {} failed, {} with known diff",
        report.passed, report.passed_with_order_difference, report.failed, report.known_diffs
    );
    if !order_differences.is_empty() {
        println!("order differs: {}", order_differences.join(", "));
    }
    Ok(report)
}
