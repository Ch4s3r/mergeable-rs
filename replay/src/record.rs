use crate::app_process::AppProgram;
use crate::case::{Case, Expected};
use crate::compare::{CallComparison, compare_calls};
use crate::runner::{
    CaseRun, RunOptions, for_each_in_parallel, run_case_in_fresh_process, run_cases_in_one_process,
};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct RecordedCase {
    pub status: u16,
    pub call_count: usize,
    pub unordered: bool,
    pub noise: Vec<String>,
}

pub struct RecordOutcome {
    pub name: String,
    pub result: Result<RecordedCase, String>,
}

fn save_both_passes(
    case: &mut Case,
    first_run: CaseRun,
    second_run: &CaseRun,
) -> Result<RecordedCase> {
    let mut noise = Vec::new();
    if !first_run.settled || !second_run.settled {
        noise.push("GitHub calls still arriving after the 40 s limit".to_owned());
    }
    if first_run.status != second_run.status {
        noise.push(format!(
            "webhook status {} vs {}",
            first_run.status, second_run.status
        ));
    }
    let unordered = match compare_calls(&first_run.calls, &second_run.calls) {
        CallComparison::Equal => false,
        CallComparison::OrderDiffers { .. } => true,
        CallComparison::Different {
            first_differing_position,
            ..
        } => {
            noise.push(format!(
                "the two runs differ at call {first_differing_position}"
            ));
            false
        }
    };
    let expected = Expected {
        status: first_run.status,
        calls: first_run.calls,
    };
    case.save_expected(&expected, unordered)?;
    Ok(RecordedCase {
        status: expected.status,
        call_count: expected.calls.len(),
        unordered,
        noise,
    })
}

fn report_progress(outcome: &RecordOutcome, finished: usize, total: usize) {
    let state = match &outcome.result {
        Ok(_) => "recorded",
        Err(_) => "FAILED",
    };
    eprintln!("[{finished}/{total}] {state} {}", outcome.name);
}

async fn record_in_fresh_processes(
    program: AppProgram,
    cases: Vec<Case>,
    options: RunOptions,
) -> Result<Vec<RecordOutcome>> {
    let total = cases.len();
    let finished = Arc::new(AtomicUsize::new(0));
    let jobs = options.jobs;
    for_each_in_parallel(cases, jobs, move |mut case| {
        let program = program.clone();
        let options = options.clone();
        let finished = finished.clone();
        async move {
            let name = case.definition.name.clone();
            let result = async {
                let first_run =
                    run_case_in_fresh_process(&program, &case, &options, &format!("{name}.pass1"))
                        .await?;
                let second_run =
                    run_case_in_fresh_process(&program, &case, &options, &format!("{name}.pass2"))
                        .await?;
                save_both_passes(&mut case, first_run, &second_run)
            }
            .await
            .map_err(|error| format!("{error:#}"));
            let outcome = RecordOutcome { name, result };
            report_progress(
                &outcome,
                finished.fetch_add(1, Ordering::Relaxed) + 1,
                total,
            );
            outcome
        }
    })
    .await
}

async fn record_in_one_process(
    program: &AppProgram,
    mut cases: Vec<Case>,
    options: &RunOptions,
) -> Result<Vec<RecordOutcome>> {
    let first_runs = run_cases_in_one_process(program, &cases, options, "reused.pass1").await?;
    let second_runs = run_cases_in_one_process(program, &cases, options, "reused.pass2").await?;
    let mut outcomes = Vec::with_capacity(cases.len());
    for ((case, first_run), second_run) in cases.iter_mut().zip(first_runs).zip(second_runs) {
        let result = match (first_run, second_run) {
            (Ok(first_run), Ok(second_run)) => save_both_passes(case, first_run, &second_run),
            (Err(error), _) | (_, Err(error)) => Err(error),
        };
        outcomes.push(RecordOutcome {
            name: case.definition.name.clone(),
            result: result.map_err(|error| format!("{error:#}")),
        });
    }
    Ok(outcomes)
}

fn print_summary(outcomes: &[RecordOutcome]) {
    let mut noisy = Vec::new();
    let mut unordered = Vec::new();
    let mut not_ok_status = Vec::new();
    let mut failed = Vec::new();
    for outcome in outcomes {
        match &outcome.result {
            Ok(recorded) => {
                println!(
                    "RECORDED {}: status {}, {} calls{}",
                    outcome.name,
                    recorded.status,
                    recorded.call_count,
                    if recorded.unordered {
                        ", unordered"
                    } else {
                        ""
                    }
                );
                for reason in &recorded.noise {
                    println!("NOISE {}: {reason}; kept the first run", outcome.name);
                    noisy.push(outcome.name.as_str());
                }
                if recorded.unordered {
                    unordered.push(outcome.name.as_str());
                }
                if recorded.status != 200 {
                    not_ok_status.push(format!("{} ({})", outcome.name, recorded.status));
                }
            }
            Err(error) => {
                println!("FAILED {}: {error}", outcome.name);
                failed.push(outcome.name.as_str());
            }
        }
    }
    noisy.dedup();
    println!(
        "\nrecorded {} of {} cases",
        outcomes.len() - failed.len(),
        outcomes.len()
    );
    println!("noise ({}): {}", noisy.len(), noisy.join(", "));
    println!("unordered ({}): {}", unordered.len(), unordered.join(", "));
    println!(
        "status not 200 ({}): {}",
        not_ok_status.len(),
        not_ok_status.join(", ")
    );
    println!("failed ({}): {}", failed.len(), failed.join(", "));
}

pub async fn record(program: AppProgram, cases: Vec<Case>, options: &RunOptions) -> Result<()> {
    let outcomes = if options.reuse_process {
        record_in_one_process(&program, cases, options).await?
    } else {
        record_in_fresh_processes(program, cases, options.clone()).await?
    };
    print_summary(&outcomes);
    if outcomes.iter().any(|outcome| outcome.result.is_err()) {
        return Err(anyhow!("some cases failed to record"));
    }
    Ok(())
}
