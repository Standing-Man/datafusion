// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use clap::Parser;
use datafusion::common::instant::Instant;
use datafusion::common::utils::get_available_parallelism;
use datafusion::common::{exec_datafusion_err, exec_err, DataFusionError, Result};
use datafusion::common::runtime::SpawnedTask;
use datafusion_sqllogictest::{DataFusion, TestContext};
use futures::stream::StreamExt;
use indicatif::{
    HumanDuration, MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle,
};
use itertools::Itertools;
use log::Level::{Info, Warn};
use log::{info, log_enabled, warn};
use sqllogictest::{
    parse_file, strict_column_validator, AsyncDB, Condition, Normalizer, Record,
    Validator,
};

#[cfg(feature = "postgres")]
use crate::postgres_container::{
    initialize_postgres_container, terminate_postgres_container,
};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(feature = "postgres")]
mod postgres_container;

const TEST_DIRECTORY: &str = "test_files/";
const DATAFUSION_TESTING_TEST_DIRECTORY: &str = "../../datafusion-testing/data/";
const PG_COMPAT_FILE_PREFIX: &str = "pg_compat_";
const SQLITE_PREFIX: &str = "sqlite";

pub fn main() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_tests())
}

// Trailing whitespace from lines in SLT will typically be removed, but do not fail if it is not
// If particular test wants to cover trailing whitespace on a value,
// it should project additional non-whitespace column on the right.
#[allow(clippy::ptr_arg)]
fn value_normalizer(s: &String) -> String {
    s.trim_end().to_string()
}

fn sqlite_value_validator(
    normalizer: Normalizer,
    actual: &[Vec<String>],
    expected: &[String],
) -> bool {
    let normalized_expected = expected.iter().map(normalizer).collect::<Vec<_>>();
    let normalized_actual = actual
        .iter()
        .map(|strs| strs.iter().map(normalizer).join(" "))
        .collect_vec();

    if log_enabled!(Info) && normalized_actual != normalized_expected {
        info!("sqlite validation failed. actual vs expected:");
        for i in 0..normalized_actual.len() {
            info!("[{i}] {}<eol>", normalized_actual[i]);
            info!(
                "[{i}] {}<eol>",
                if normalized_expected.len() >= i {
                    &normalized_expected[i]
                } else {
                    "No more results"
                }
            );
        }
    }

    normalized_actual == normalized_expected
}

fn df_value_validator(
    normalizer: Normalizer,
    actual: &[Vec<String>],
    expected: &[String],
) -> bool {
    let normalized_expected = expected.iter().map(normalizer).collect::<Vec<_>>();
    let normalized_actual = actual
        .iter()
        .map(|strs| strs.iter().join(" "))
        .map(|str| str.trim_end().to_string())
        .collect_vec();

    if log_enabled!(Warn) && normalized_actual != normalized_expected {
        warn!("df validation failed. actual vs expected:");
        for i in 0..normalized_actual.len() {
            warn!("[{i}] {}<eol>", normalized_actual[i]);
            warn!(
                "[{i}] {}<eol>",
                if normalized_expected.len() >= i {
                    &normalized_expected[i]
                } else {
                    "No more results"
                }
            );
        }
    }

    normalized_actual == normalized_expected
}

/// Sets up an empty directory at test_files/scratch/<name>
/// creating it if needed and clearing any file contents if it exists
/// This allows tests for inserting to external tables or copy to
/// persist data to disk and have consistent state when running
/// a new test
fn setup_scratch_dir(name: &Path) -> Result<()> {
    // go from copy.slt --> copy
    let file_stem = name.file_stem().expect("File should have a stem");
    let path = PathBuf::from("test_files").join("scratch").join(file_stem);

    info!("Creating scratch dir in {path:?}");
    if path.exists() {
        fs::remove_dir_all(&path)?;
    }
    fs::create_dir_all(&path)?;
    Ok(())
}

async fn run_tests() -> Result<()> {
    // Enable logging (e.g. set RUST_LOG=debug to see debug logs)
    env_logger::init();

    let options: Options = Parser::parse();
    if options.list {
        // nextest parses stdout, so print messages to stderr
        eprintln!("NOTICE: --list option unsupported, quitting");
        // return Ok, not error so that tools like nextest which are listing all
        // workspace tests (by running `cargo test ... --list --format terse`)
        // do not fail when they encounter this binary. Instead, print nothing
        // to stdout and return OK so they can continue listing other tests.
        return Ok(());
    }
    options.warn_on_ignored();

    #[cfg(feature = "postgres")]
    initialize_postgres_container(&options).await?;

    // Run all tests in parallel, reporting failures at the end
    //
    // Doing so is safe because each slt file runs with its own
    // `SessionContext` and should not have side effects (like
    // modifying shared state like `/tmp/`)
    let m = MultiProgress::with_draw_target(ProgressDrawTarget::stderr_with_hz(1));
    let m_style = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
        .unwrap()
        .progress_chars("##-");

    let start = Instant::now();

    let errors: Vec<_> = futures::stream::iter(read_test_files(&options)?)
        .map(|test_file| {
            let validator = if options.include_sqlite
                && test_file.relative_path.starts_with(SQLITE_PREFIX)
            {
                sqlite_value_validator
            } else {
                df_value_validator
            };

            let m_clone = m.clone();
            let m_style_clone = m_style.clone();

            SpawnedTask::spawn(async move {
                match (options.postgres_runner, options.complete) {
                    (false, false) => {
                        run_test_file(test_file, validator, m_clone, m_style_clone)
                            .await?
                    }
                    (false, true) => {
                        run_complete_file(test_file, validator, m_clone, m_style_clone)
                            .await?
                    }
                    (true, false) => {
                        run_test_file_with_postgres(
                            test_file,
                            validator,
                            m_clone,
                            m_style_clone,
                        )
                            .await?
                    }
                    (true, true) => {
                        run_complete_file_with_postgres(
                            test_file,
                            validator,
                            m_clone,
                            m_style_clone,
                        )
                            .await?
                    }
                }
                Ok(()) as Result<()>
            })
                .join()
        })
        // run up to num_cpus streams in parallel
        .buffer_unordered(get_available_parallelism())
        .flat_map(|result| {
            // Filter out any Ok() leaving only the DataFusionErrors
            futures::stream::iter(match result {
                // Tokio panic error
                Err(e) => Some(DataFusionError::External(Box::new(e))),
                Ok(thread_result) => match thread_result {
                    // Test run error
                    Err(e) => Some(e),
                    // success
                    Ok(_) => None,
                },
            })
        })
        .collect()
        .await;

    m.println(format!("Completed in {}", HumanDuration(start.elapsed())))?;

    #[cfg(feature = "postgres")]
    terminate_postgres_container().await?;

    // report on any errors
    if !errors.is_empty() {
        for e in &errors {
            println!("{e}");
        }
        exec_err!("{} failures", errors.len())
    } else {
        Ok(())
    }
}

async fn run_test_file(
    test_file: TestFile,
    validator: Validator,
    mp: MultiProgress,
    mp_style: ProgressStyle,
) -> Result<()> {
    let TestFile {
        path,
        relative_path,
    } = test_file;
    let Some(test_ctx) = TestContext::try_new_for_test_file(&relative_path).await else {
        info!("Skipping: {}", path.display());
        return Ok(());
    };
    setup_scratch_dir(&relative_path)?;

    let count: u64 = get_record_count(&path, "Datafusion".to_string());
    let pb = mp.add(ProgressBar::new(count));

    pb.set_style(mp_style);
    pb.set_message(format!("{:?}", &relative_path));

    let mut runner = sqllogictest::Runner::new(|| async {
        Ok(DataFusion::new(
            test_ctx.session_ctx().clone(),
            relative_path.clone(),
            pb.clone(),
        ))
    });
    runner.add_label("Datafusion");
    runner.with_column_validator(strict_column_validator);
    runner.with_normalizer(value_normalizer);
    runner.with_validator(validator);

    let res = runner
        .run_file_async(path)
        .await
        .map_err(|e| DataFusionError::External(Box::new(e)));

    pb.finish_and_clear();

    res
}

fn get_record_count(path: &PathBuf, label: String) -> u64 {
    let records: Vec<Record<<DataFusion as AsyncDB>::ColumnType>> =
        parse_file(path).unwrap();
    let mut count: u64 = 0;

    records.iter().for_each(|rec| match rec {
        Record::Query { conditions, .. } => {
            if conditions.is_empty()
                || !conditions.contains(&Condition::SkipIf {
                label: label.clone(),
            })
                || conditions.contains(&Condition::OnlyIf {
                label: label.clone(),
            })
            {
                count += 1;
            }
        }
        Record::Statement { conditions, .. } => {
            if conditions.is_empty()
                || !conditions.contains(&Condition::SkipIf {
                label: label.clone(),
            })
                || conditions.contains(&Condition::OnlyIf {
                label: label.clone(),
            })
            {
                count += 1;
            }
        }
        _ => {}
    });

    count
}

#[cfg(feature = "postgres")]
async fn run_test_file_with_postgres(
    test_file: TestFile,
    validator: Validator,
    mp: MultiProgress,
    mp_style: ProgressStyle,
) -> Result<()> {
    use datafusion_sqllogictest::Postgres;
    let TestFile {
        path,
        relative_path,
    } = test_file;
    setup_scratch_dir(&relative_path)?;

    let count: u64 = get_record_count(&path, "postgresql".to_string());
    let pb = mp.add(ProgressBar::new(count));

    pb.set_style(mp_style);
    pb.set_message(format!("{:?}", &relative_path));

    let mut runner = sqllogictest::Runner::new(|| {
        Postgres::connect(relative_path.clone(), pb.clone())
    });
    runner.add_label("postgres");
    runner.with_column_validator(strict_column_validator);
    runner.with_normalizer(value_normalizer);
    runner.with_validator(validator);
    runner
        .run_file_async(path)
        .await
        .map_err(|e| DataFusionError::External(Box::new(e)))?;

    pb.finish_and_clear();

    Ok(())
}

#[cfg(not(feature = "postgres"))]
async fn run_test_file_with_postgres(
    _test_file: TestFile,
    _validator: Validator,
    _mp: MultiProgress,
    _mp_style: ProgressStyle,
) -> Result<()> {
    use datafusion::common::plan_err;
    plan_err!("Can not run with postgres as postgres feature is not enabled")
}

async fn run_complete_file(
    test_file: TestFile,
    _validator: Validator,
    mp: MultiProgress,
    mp_style: ProgressStyle,
) -> Result<()> {
    let TestFile {
        path,
        relative_path,
    } = test_file;

    info!("Using complete mode to complete: {}", path.display());

    let Some(_test_ctx) = TestContext::try_new_for_test_file(&relative_path).await else {
        info!("Skipping: {}", path.display());
        return Ok(());
    };
    setup_scratch_dir(&relative_path)?;

    let count: u64 = get_record_count(&path, "Datafusion".to_string());
    let pb = mp.add(ProgressBar::new(count));

    pb.set_style(mp_style);
    pb.set_message(format!("{:?}", &relative_path));

    // let mut runner = sqllogictest::Runner::new(|| async {
    //     Ok(DataFusion::new(
    //         test_ctx.session_ctx().clone(),
    //         relative_path.clone(),
    //         pb.clone(),
    //     ))
    // });
    //
    // let col_separator = " ";
    // let res = runner
    //     .update_test_file(
    //         path,
    //         col_separator,
    //         validator,
    //         value_normalizer,
    //         strict_column_validator,
    //     )
    //     .await
    //     // Can't use e directly because it isn't marked Send, so turn it into a string.
    //     .map_err(|e| {
    //         DataFusionError::Execution(format!("Error completing {relative_path:?}: {e}"))
    //     });

    pb.finish_and_clear();

    // res
    Ok(())
}

#[cfg(feature = "postgres")]
async fn run_complete_file_with_postgres(
    test_file: TestFile,
    validator: Validator,
    mp: MultiProgress,
    mp_style: ProgressStyle,
) -> Result<()> {
    use datafusion_sqllogictest::Postgres;
    let TestFile {
        path,
        relative_path,
    } = test_file;
    info!(
        "Using complete mode to complete with Postgres runner: {}",
        path.display()
    );
    setup_scratch_dir(&relative_path)?;

    let count: u64 = get_record_count(&path, "postgresql".to_string());
    let pb = mp.add(ProgressBar::new(count));

    pb.set_style(mp_style);
    pb.set_message(format!("{:?}", &relative_path));

    let Some(test_ctx) = TestContext::try_new_for_test_file(&relative_path).await else {
        info!("Skipping: {}", path.display());
        return Ok(());
    };
    let mut runner = sqllogictest::Runner::new(|| async {
        Ok(DataFusion::new(
            test_ctx.session_ctx().clone(),
            relative_path.clone(),
            pb.clone(),
        ))
    });
    runner.add_label("Datafusion");
    runner.with_column_validator(strict_column_validator);
    runner.with_normalizer(value_normalizer);
    runner.with_validator(validator);

    let mut pg = sqllogictest::Runner::new(|| {
        Postgres::connect(relative_path.clone(), pb.clone())
    });
    pg.add_label("postgres");
    pg.with_column_validator(strict_column_validator);
    pg.with_normalizer(value_normalizer);
    pg.with_validator(validator);

    let col_separator = " ";
    let res = runner
        .update_test_file(
            path,
            col_separator,
            validator,
            value_normalizer,
            strict_column_validator,
            Some(pg),
        )
        .await
        // Can't use e directly because it isn't marked Send, so turn it into a string.
        .map_err(|e| {
            DataFusionError::Execution(format!("Error completing {relative_path:?}: {e}"))
        });

    pb.finish_and_clear();

    res
}

#[cfg(not(feature = "postgres"))]
async fn run_complete_file_with_postgres(
    _test_file: TestFile,
    _validator: Validator,
    _mp: MultiProgress,
    _mp_style: ProgressStyle,
) -> Result<()> {
    use datafusion::common::plan_err;
    plan_err!("Can not run with postgres as postgres feature is not enabled")
}

/// Represents a parsed test file
#[derive(Debug)]
struct TestFile {
    /// The absolute path to the file
    pub path: PathBuf,
    /// The relative path of the file (used for display)
    pub relative_path: PathBuf,
}

impl TestFile {
    fn new(path: PathBuf) -> Self {
        let p = path.to_string_lossy();
        let relative_path = PathBuf::from(if p.starts_with(TEST_DIRECTORY) {
            p.strip_prefix(TEST_DIRECTORY).unwrap()
        } else if p.starts_with(DATAFUSION_TESTING_TEST_DIRECTORY) {
            p.strip_prefix(DATAFUSION_TESTING_TEST_DIRECTORY).unwrap()
        } else {
            ""
        });

        Self {
            path,
            relative_path,
        }
    }

    fn is_slt_file(&self) -> bool {
        self.path.extension() == Some(OsStr::new("slt"))
    }

    fn check_sqlite(&self, options: &Options) -> bool {
        if !self.relative_path.starts_with(SQLITE_PREFIX) {
            return true;
        }

        options.include_sqlite
    }

    fn check_tpch(&self, options: &Options) -> bool {
        if !self.relative_path.starts_with("tpch") {
            return true;
        }

        options.include_tpch
    }
}

fn read_test_files<'a>(
    options: &'a Options,
) -> Result<Box<dyn Iterator<Item = TestFile> + 'a>> {
    let mut paths = read_dir_recursive(TEST_DIRECTORY)?
        .into_iter()
        .map(TestFile::new)
        .filter(|f| options.check_test_file(&f.relative_path))
        .filter(|f| f.is_slt_file())
        .filter(|f| f.check_tpch(options))
        .filter(|f| f.check_sqlite(options))
        .filter(|f| options.check_pg_compat_file(f.path.as_path()))
        .collect::<Vec<_>>();
    if options.include_sqlite {
        let mut sqlite_paths = read_dir_recursive(DATAFUSION_TESTING_TEST_DIRECTORY)?
            .into_iter()
            .map(TestFile::new)
            .filter(|f| options.check_test_file(&f.relative_path))
            .filter(|f| f.is_slt_file())
            .filter(|f| f.check_sqlite(options))
            .filter(|f| options.check_pg_compat_file(f.path.as_path()))
            .collect::<Vec<_>>();

        paths.append(&mut sqlite_paths)
    }

    Ok(Box::new(paths.into_iter()))
}

fn read_dir_recursive<P: AsRef<Path>>(path: P) -> Result<Vec<PathBuf>> {
    let mut dst = vec![];
    read_dir_recursive_impl(&mut dst, path.as_ref())?;
    Ok(dst)
}

/// Append all paths recursively to dst
fn read_dir_recursive_impl(dst: &mut Vec<PathBuf>, path: &Path) -> Result<()> {
    let entries = fs::read_dir(path)
        .map_err(|e| exec_datafusion_err!("Error reading directory {path:?}: {e}"))?;
    for entry in entries {
        let path = entry
            .map_err(|e| {
                exec_datafusion_err!("Error reading entry in directory {path:?}: {e}")
            })?
            .path();

        if path.is_dir() {
            read_dir_recursive_impl(dst, &path)?;
        } else {
            dst.push(path);
        }
    }

    Ok(())
}

/// Parsed command line options
///
/// This structure attempts to mimic the command line options of the built-in rust test runner
/// accepted by IDEs such as CLion that pass arguments
///
/// See <https://github.com/apache/datafusion/issues/8287> for more details
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about= None)]
struct Options {
    #[clap(long, help = "Auto complete mode to fill out expected results")]
    complete: bool,

    #[clap(
        long,
        env = "PG_COMPAT",
        help = "Run Postgres compatibility tests with Postgres runner"
    )]
    postgres_runner: bool,

    #[clap(long, env = "INCLUDE_SQLITE", help = "Include sqlite files")]
    include_sqlite: bool,

    #[clap(long, env = "INCLUDE_TPCH", help = "Include tpch files")]
    include_tpch: bool,

    #[clap(
        action,
        help = "regex like arguments passed to the program which are treated as cargo test filter (substring match on filenames)"
    )]
    filters: Vec<String>,

    #[clap(
        long,
        help = "IGNORED (for compatibility with built in rust test runner)"
    )]
    format: Option<String>,

    #[clap(
        short = 'Z',
        long,
        help = "IGNORED (for compatibility with built in rust test runner)"
    )]
    z_options: Option<String>,

    #[clap(
        long,
        help = "IGNORED (for compatibility with built in rust test runner)"
    )]
    show_output: bool,

    #[clap(
        long,
        help = "Quits immediately, not listing anything (for compatibility with built-in rust test runner)"
    )]
    list: bool,

    #[clap(
        long,
        help = "IGNORED (for compatibility with built-in rust test runner)"
    )]
    ignored: bool,
}

impl Options {
    /// Because this test can be run as a cargo test, commands like
    ///
    /// ```shell
    /// cargo test foo
    /// ```
    ///
    /// Will end up passing `foo` as a command line argument.
    ///
    /// To be compatible with this, treat the command line arguments as a
    /// filter and that does a substring match on each input.  returns
    /// true f this path should be run
    fn check_test_file(&self, relative_path: &Path) -> bool {
        if self.filters.is_empty() {
            return true;
        }

        // otherwise check if any filter matches
        self.filters
            .iter()
            .any(|filter| relative_path.to_string_lossy().contains(filter))
    }

    /// Postgres runner executes only tests in files with specific names or in
    /// specific folders
    fn check_pg_compat_file(&self, path: &Path) -> bool {
        let file_name = path.file_name().unwrap().to_str().unwrap().to_string();
        !self.postgres_runner
            || file_name.starts_with(PG_COMPAT_FILE_PREFIX)
            || (self.include_sqlite && path.to_string_lossy().contains(SQLITE_PREFIX))
    }

    /// Logs warning messages to stdout if any ignored options are passed
    fn warn_on_ignored(&self) {
        if self.format.is_some() {
            eprintln!("WARNING: Ignoring `--format` compatibility option");
        }

        if self.z_options.is_some() {
            eprintln!("WARNING: Ignoring `-Z` compatibility option");
        }

        if self.show_output {
            eprintln!("WARNING: Ignoring `--show-output` compatibility option");
        }
    }
}
