use localwright_core::{read_bounded, Error, Job, Result};
use std::{collections::HashMap, fmt::Write, path::Path};

const MAX_INPUT_BYTES: u64 = 1024 * 1024;
const MAX_LINES: usize = 50_000;
const MAX_MATRIX_CELLS: usize = 2_000_000;

fn load_text(path: &Path) -> Result<String> {
    String::from_utf8(read_bounded(path, MAX_INPUT_BYTES)?)
        .map_err(|_| Error::input("Comparison inputs must be UTF-8 text"))
}

pub fn compare(before: &Path, after: &Path, job: &Job) -> Result<String> {
    let before = load_text(before)?;
    job.check()?;
    let after = load_text(after)?;
    let before = lines(&before)?;
    let after = lines(&after)?;

    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    let before = &before[prefix..];
    let after = &after[prefix..];
    let suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let before = &before[..before.len() - suffix];
    let after = &after[..after.len() - suffix];

    let width = after.len() + 1;
    let cells = (before.len() + 1).saturating_mul(width);
    if cells > MAX_MATRIX_CELLS {
        return Err(Error::input("Comparison exceeds the line-diff work limit"));
    }

    let mut symbols = HashMap::new();
    let mut intern = |line| {
        let next = symbols.len();
        *symbols.entry(line).or_insert(next)
    };
    let before_codes: Vec<_> = before.iter().copied().map(&mut intern).collect();
    let after_codes: Vec<_> = after.iter().copied().map(intern).collect();

    // Each cell is the longest common subsequence from the corresponding pair of lines.
    let mut common = vec![0u32; cells];
    for i in (0..before.len()).rev() {
        job.check()?;
        for j in (0..after.len()).rev() {
            common[i * width + j] = if before_codes[i] == after_codes[j] {
                1 + common[(i + 1) * width + j + 1]
            } else {
                common[(i + 1) * width + j].max(common[i * width + j + 1])
            };
        }
    }

    let unchanged = prefix + suffix + common[0] as usize;
    let mut report = format!(
        "Unchanged: {unchanged}; removed: {}; added: {}\n",
        before.len() + prefix + suffix - unchanged,
        after.len() + prefix + suffix - unchanged
    );
    let (mut i, mut j) = (0, 0);
    while i < before.len() || j < after.len() {
        job.check()?;
        if i < before.len() && j < after.len() && before_codes[i] == after_codes[j] {
            i += 1;
            j += 1;
        } else if i < before.len()
            && (j == after.len() || common[(i + 1) * width + j] >= common[i * width + j + 1])
        {
            let _ = writeln!(report, "- {}: {:?}", prefix + i + 1, before[i]);
            i += 1;
        } else {
            let _ = writeln!(report, "+ {}: {:?}", prefix + j + 1, after[j]);
            j += 1;
        }
    }
    Ok(report)
}

fn lines(text: &str) -> Result<Vec<&str>> {
    let lines = text
        .split_inclusive('\n')
        .take(MAX_LINES + 1)
        .collect::<Vec<_>>();
    if lines.len() > MAX_LINES {
        return Err(Error::input("Comparison input exceeds 50,000 lines"));
    }
    Ok(lines)
}
