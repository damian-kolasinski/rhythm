//! Read macOS crontabs without retaining the commands, account names, or environment settings.
use crate::{
    bounded_command::{self, Output},
    definition_file,
    routines::{CronEntry, CronTable, Provider, Scan, Source},
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(crate) struct Cron {
    crontab: PathBuf,
    id: PathBuf,
    system: PathBuf,
    spool: PathBuf,
}

impl Cron {
    pub fn current() -> Self {
        Self {
            crontab: "/usr/bin/crontab".into(),
            id: "/usr/bin/id".into(),
            system: "/etc/crontab".into(),
            spool: "/usr/lib/cron/tabs".into(),
        }
    }

    fn read_user(&self, scan: &mut Scan) {
        match query(&self.crontab, &["-l"]) {
            Some(output) if output.status.success() => {
                entries(&output.stdout, CronTable::User, &mut scan.cron);
            }
            Some(output)
                if output.status.code() == Some(1)
                    && String::from_utf8_lossy(&output.stderr).lines().any(|line| {
                        line.trim_start_matches("crontab: ")
                            .starts_with("no crontab for ")
                    }) => {}
            _ => {
                scan.incomplete.insert(Source::UserCrontab);
            }
        }
    }

    fn read_other_users(&self, scan: &mut Scan) -> io::Result<()> {
        let mut tables = match fs::read_dir(&self.spool) {
            Ok(tables) => tables.collect::<io::Result<Vec<_>>>()?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if tables.is_empty() {
            return Ok(());
        }
        let owner = query(&self.id, &["-un"])
            .filter(|output| output.status.success())
            .ok_or(io::ErrorKind::NotFound)?;
        let owner = std::str::from_utf8(&owner.stdout)
            .map_err(|_| io::ErrorKind::InvalidData)?
            .trim();
        tables.sort_by_key(fs::DirEntry::file_name);
        for (index, table) in tables
            .into_iter()
            .filter(|table| {
                let name = table.file_name();
                name != owner && !name.as_encoded_bytes().starts_with(b".")
            })
            .enumerate()
        {
            read_file(&table.path(), CronTable::Other(index + 1), scan);
        }
        Ok(())
    }
}

impl Provider for Cron {
    fn scan(&self) -> Scan {
        let mut scan = Scan::default();
        self.read_user(&mut scan);
        read_file(&self.system, CronTable::System, &mut scan);
        if self.read_other_users(&mut scan).is_err() {
            scan.incomplete.insert(Source::SystemCrontabs);
        }
        scan
    }
}

fn query(path: &Path, args: &[&str]) -> Option<Output> {
    let mut command = Command::new(path);
    command.args(args).env("LC_ALL", "C");
    bounded_command::run(command, Duration::from_secs(3), definition_file::MAX_BYTES).ok()
}

fn read_file(path: &Path, table: CronTable, scan: &mut Scan) {
    match definition_file::read(path) {
        Ok(bytes) => entries(&bytes, table, &mut scan.cron),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {
            scan.incomplete.insert(Source::SystemCrontabs);
        }
    }
}

fn entries(bytes: &[u8], table: CronTable, jobs: &mut Vec<CronEntry>) {
    let text = String::from_utf8_lossy(bytes);
    for (index, line) in text.lines().enumerate() {
        if let Some(schedule) = schedule(line, table == CronTable::System) {
            jobs.push(CronEntry {
                table,
                line: index + 1,
                schedule,
            });
        }
    }
}

fn schedule(line: &str, system: bool) -> Option<String> {
    let mut words = line.split_ascii_whitespace();
    let first = words.next()?;
    if first.starts_with('@') {
        let schedule = match first {
            "@annually" | "@yearly" => "Yearly",
            "@monthly" => "Monthly",
            "@weekly" => "Weekly",
            "@daily" | "@midnight" => "Daily 00:00",
            "@hourly" => "Hourly",
            _ => return None,
        };
        words.nth(usize::from(system))?;
        return Some(schedule.into());
    }
    let fields = [
        first,
        words.next()?,
        words.next()?,
        words.next()?,
        words.next()?,
    ];
    words.nth(usize::from(system))?;
    let bounds = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)];
    const MONTHS: &[&str] = &[
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    const DAYS: &[&str] = &["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
    let names: [&[&str]; 5] = [&[], &[], &[], MONTHS, DAYS];
    for ((field, (min, max)), names) in fields.iter().zip(bounds).zip(names) {
        if !names.iter().any(|name| field.eq_ignore_ascii_case(name))
            && !field.split(',').all(|term| cron_term(term, min, max))
        {
            return None;
        }
    }
    if let [minute, hour, "*", "*", "*"] = fields
        && let (Some(minute), Some(hour)) = (number(minute), number(hour))
    {
        return Some(format!("Daily {hour:02}:{minute:02}"));
    }
    Some(fields.join(" "))
}

fn number(text: &str) -> Option<u64> {
    text.bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| text.parse().ok())
        .flatten()
}

fn cron_term(term: &str, min: u64, max: u64) -> bool {
    let range = match term.split_once('/') {
        Some((range, step)) if number(step).is_some_and(|step| step > 0) => range,
        Some(_) => return false,
        None => term,
    };
    if range == "*" {
        return true;
    }
    let within = |text| number(text).filter(|value| (min..=max).contains(value));
    match range.split_once('-') {
        Some((start, end)) => within(start)
            .zip(within(end))
            .is_some_and(|(start, end)| start <= end),
        None => within(range).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn extracts_only_repeating_schedule_fields() {
        for (line, system, expected) in [
            ("15 3 * * * /bin/example", false, Some("Daily 03:15")),
            (
                "*/5 1-3 * DEC MON /bin/example",
                false,
                Some("*/5 1-3 * DEC MON"),
            ),
            ("@weekly example /bin/example", true, Some("Weekly")),
            ("@hourly /bin/example", true, None),
            ("@reboot /bin/example", false, None),
            ("MAILTO = user@example.invalid", false, None),
            ("# 1 2 * * * /bin/example", false, None),
            ("60 * * * * /bin/example", false, None),
        ] {
            assert_eq!(schedule(line, system).as_deref(), expected);
        }
    }

    #[test]
    fn collects_readable_system_tables_even_when_user_query_fails() {
        let temp = tempfile::tempdir().unwrap();
        let spool = temp.path().join("tabs");
        let system = temp.path().join("crontab");
        let id = temp.path().join("id");
        fs::create_dir(&spool).unwrap();
        fs::write(spool.join("sample"), "@daily /bin/example\n").unwrap();
        fs::write(spool.join("different"), "@hourly /bin/example\n").unwrap();
        fs::write(&system, "# comment\n15 10 * * * sample /bin/example\n").unwrap();
        fs::write(&id, "#!/bin/sh\nprintf 'sample\\n'\n").unwrap();
        fs::set_permissions(&id, fs::Permissions::from_mode(0o700)).unwrap();
        let scan = Cron {
            crontab: temp.path().join("missing"),
            id,
            system,
            spool,
        }
        .scan();
        assert_eq!(scan.incomplete, [Source::UserCrontab].into());
        assert_eq!(
            scan.cron,
            [
                CronEntry {
                    table: CronTable::System,
                    line: 2,
                    schedule: "Daily 10:15".into()
                },
                CronEntry {
                    table: CronTable::Other(1),
                    line: 1,
                    schedule: "Hourly".into()
                },
            ]
        );
    }
}
