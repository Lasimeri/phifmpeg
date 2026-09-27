//! `phifmpeg prof`: turn the card profiler's samples into a table.
//!
//! The card-side profiler (`card/phix`, `PHIX_PROF`) writes one
//! little-endian `u64` per sample: the instruction pointer the process was
//! at. The binaries are static, non-PIE executables, so an address is the
//! same at run time as in the file and needs no load offset. Each sample is
//! attributed to the symbol with the greatest address at or below it, within
//! an executable section; a symbol without a size (NASM's labels) is taken to
//! run to the next one.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use object::{Object, ObjectSection, ObjectSymbol, SectionKind};

/// One function-like symbol.
struct Sym {
    addr: u64,
    size: u64,
    name: String,
}

/// Print the `top` functions by sample count.
pub fn report(elf: &Path, samples: &Path, top: usize) -> Result<()> {
    let data = std::fs::read(elf).with_context(|| format!("reading {}", elf.display()))?;
    let file = object::File::parse(&*data).with_context(|| format!("parsing {}", elf.display()))?;
    let mut syms: Vec<Sym> = Vec::new();
    for s in file.symbols() {
        let Some(idx) = s.section_index() else {
            continue;
        };
        let Ok(sec) = file.section_by_index(idx) else {
            continue;
        };
        if sec.kind() != SectionKind::Text {
            continue;
        }
        let Ok(name) = s.name() else { continue };
        if name.is_empty() || s.address() == 0 {
            continue;
        }
        syms.push(Sym {
            addr: s.address(),
            size: s.size(),
            name: demangle(name),
        });
    }
    if syms.is_empty() {
        bail!(
            "{}: no symbols in executable sections (stripped binary?)",
            elf.display()
        );
    }
    syms.sort_by_key(|s| s.addr);

    let raw = std::fs::read(samples).with_context(|| format!("reading {}", samples.display()))?;
    let pcs: Vec<u64> = raw
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if pcs.is_empty() {
        bail!("{}: no samples", samples.display());
    }

    let mut counts: HashMap<usize, u64> = HashMap::new();
    let mut unknown = 0u64;
    for &pc in &pcs {
        let i = syms.partition_point(|s| s.addr <= pc);
        if i == 0 {
            unknown += 1;
            continue;
        }
        let s = &syms[i - 1];
        let end = if s.size > 0 {
            s.addr + s.size
        } else {
            syms.get(i).map(|n| n.addr).unwrap_or(u64::MAX)
        };
        if pc < end {
            *counts.entry(i - 1).or_default() += 1;
        } else {
            unknown += 1;
        }
    }
    let total = pcs.len() as f64;
    let mut rows: Vec<(usize, u64)> = counts.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    println!("{} samples, {} outside any symbol", pcs.len(), unknown);
    println!("{:>7} {:>7} {:>9}  function", "self%", "cum%", "samples");
    let mut cum = 0u64;
    for (i, n) in rows.iter().take(top) {
        cum += n;
        println!(
            "{:>6.2}% {:>6.2}% {:>9}  {}",
            *n as f64 * 100.0 / total,
            cum as f64 * 100.0 / total,
            n,
            syms[*i].name
        );
    }
    Ok(())
}

/// C++ names readable; anything else unchanged.
fn demangle(name: &str) -> String {
    if name.starts_with("_Z") {
        if let Ok(sym) = cpp_demangle::Symbol::new(name) {
            if let Ok(s) = sym.demangle(&cpp_demangle::DemangleOptions::default()) {
                return s;
            }
        }
    }
    name.to_string()
}
