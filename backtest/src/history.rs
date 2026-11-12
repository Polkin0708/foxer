use chrono::{Datelike, NaiveDate};
use forex_core::{
    engine::{Engine, Timeframe},
    feed::{Aggregator, Tick},
    model::Strategy,
    prices,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
pub fn hash(path: &Path) -> Result<String, String> {
    let mut f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
pub fn verify(path: &Path, expected: &str) -> Result<(), String> {
    if hash(path)? != expected {
        return Err(format!("checksum mismatch: {}", path.display()));
    }
    Ok(())
}
pub fn json_file(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| e.to_string())
}
pub fn read_archive(
    path: &Path,
    symbol: &str,
    source: &str,
    expected: &str,
    gaps: &[(i64, i64)],
    mut visit: impl FnMut(Tick) -> Result<(), String>,
) -> Result<(), String> {
    if source.trim().is_empty() {
        return Err("missing data source".into());
    }
    verify(path, expected)?;
    let mut zip = zip::ZipArchive::new(File::open(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let names: Vec<_> = zip
        .file_names()
        .filter(|n| n.starts_with(&format!("{symbol}-index-")) && n.ends_with(".csv"))
        .map(str::to_owned)
        .collect();
    if names.len() != 1 {
        return Err("expected one symbol index CSV in archive".into());
    }
    let file = zip.by_name(&names[0]).map_err(|e| e.to_string())?;
    let mut reader = csv::Reader::from_reader(file);
    if reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .ne(["timestamp_ms", "price"])
    {
        return Err("invalid index tick columns".into());
    }
    let mut previous: Option<Tick> = None;
    for row in reader.records() {
        let row = row.map_err(|e| e.to_string())?;
        let millis: i64 = row[0].parse().map_err(|_| "invalid index timestamp")?;
        if millis < 0 {
            return Err("invalid index timestamp".into());
        }
        let tick = Tick {
            symbol: symbol.into(),
            source: source.into(),
            millis,
            price: prices::price(&row[1])?,
        };
        if gaps.iter().any(|&(a, b)| millis >= a && millis < b) {
            continue;
        }
        if previous.as_ref().is_some_and(|p| p.millis > millis) {
            return Err("unordered quote outside quarantine".into());
        }
        if previous.as_ref() == Some(&tick) {
            continue;
        }
        previous = Some(tick.clone());
        visit(tick)?;
    }
    Ok(())
}
#[derive(Clone)]
pub struct Archive {
    pub path: PathBuf,
    pub sha256: String,
    pub start: i64,
    pub end: i64,
}
pub struct Dataset {
    pub source: String,
    pub roots: Vec<PathBuf>,
    pub gaps: Vec<(i64, i64)>,
    pub archives: Vec<Archive>,
    pub provenance: Vec<Value>,
}
impl Dataset {
    pub fn open(roots: Vec<PathBuf>, symbol: &str, from: i64, to: i64) -> Result<Self, String> {
        let mut gaps = Vec::new();
        let mut archives = Vec::new();
        let mut provenance = Vec::new();
        let mut source: Option<String> = None;
        for root in &roots {
            let q = json_file(&root.join("quality-report.json"))?;
            let name = q["source"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("missing data source")?;
            if source.as_ref().is_some_and(|old| old != name) {
                return Err("mixed history sources for instrument".into());
            }
            source = Some(name.to_owned());
            if q["timezone"] != "UTC" || q["price_basis"] != "index" {
                return Err("expected UTC index history".into());
            }
            if q.get("symbol").is_some_and(|s| s != symbol) {
                return Err("data symbol mismatch".into());
            }
            for g in q["gaps"]
                .as_array()
                .ok_or("missing index gap information")?
            {
                let a = g["from_ms"].as_i64().ok_or("invalid gap")?;
                let b = g["to_ms"].as_i64().ok_or("invalid gap")?;
                if a < 0 || b <= a {
                    return Err("invalid gap".into());
                }
                gaps.push((a, b));
            }
            let m = json_file(&root.join("manifest.json"))?;
            provenance.push(json!({"root":root,"source":name,"manifest_sha256":hash(&root.join("manifest.json"))?,"quality_sha256":hash(&root.join("quality-report.json"))?}));
            for f in m["files"].as_array().ok_or("missing manifest files")? {
                let month = f["month"].as_str().ok_or("missing month")?;
                let d = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
                    .map_err(|_| "invalid archive month")?;
                let next = if d.month() == 12 {
                    NaiveDate::from_ymd_opt(d.year() + 1, 1, 1)
                } else {
                    NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1)
                }
                .ok_or("month overflow")?;
                let start = (d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()) * 1000;
                let end = (next.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()) * 1000;
                if start < to && end > from {
                    let rel = Path::new(f["file"].as_str().ok_or("missing archive path")?);
                    if rel.is_absolute()
                        || rel
                            .components()
                            .any(|c| matches!(c, std::path::Component::ParentDir))
                    {
                        return Err("invalid archive path".into());
                    }
                    archives.push(Archive {
                        path: root.join(rel),
                        sha256: f["sha256"].as_str().ok_or("missing archive hash")?.into(),
                        start,
                        end,
                    });
                }
            }
        }
        archives.sort_by_key(|a| a.start);
        let mut covered = from;
        for a in &archives {
            if a.start > covered {
                return Err("missing index history in requested period".into());
            }
            if a.end <= covered {
                return Err("overlapping archive month".into());
            }
            covered = a.end;
        }
        if covered < to {
            return Err("missing index history in requested period".into());
        }
        gaps.sort_unstable();
        gaps.dedup();
        Ok(Self {
            source: source.ok_or("missing history source")?,
            roots,
            gaps,
            archives,
            provenance,
        })
    }
    pub fn prepare(
        &mut self,
        symbol: &str,
        from: i64,
        cfg: &Strategy,
    ) -> Result<(Engine, Aggregator, Value), String> {
        let cutoff = from / 1000;
        let mut files = Vec::new();
        for root in &self.roots {
            let q = json_file(&root.join("quality-report.json"))?;
            for (name, v) in q["outputs"].as_object().ok_or("missing prepared outputs")? {
                if name.ends_with(".csv")
                    && name.starts_with(symbol)
                    && (name.contains("M1") || name == &format!("{symbol}.csv"))
                {
                    files.push((root.join("prepared").join(name), v.clone()));
                }
            }
        }
        files.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));
        let mut agg = Aggregator::default();
        let mut previous = None;
        let mut gap_idx = 0;
        let mut bars: BTreeMap<Timeframe, VecDeque<forex_core::trading::Candle>> = BTreeMap::new();
        let limits = |tf| match tf {
            Timeframe::W => cfg.context.bars_analyzed.w,
            Timeframe::D => cfg.context.bars_analyzed.d,
            Timeframe::H4 => cfg.context.bars_analyzed.h4,
        };
        let mut keep = |tf, b| {
            let list = bars.entry(tf).or_default();
            list.push_back(b);
            while list.len() > limits(tf) as usize {
                list.pop_front();
            }
        };
        for (path, meta) in files {
            verify(
                &path,
                meta["sha256"].as_str().ok_or("missing prepared hash")?,
            )?;
            self.provenance
                .push(json!({"prepared":path,"sha256":meta["sha256"]}));
            let mut reader = csv::Reader::from_path(&path).map_err(|e| e.to_string())?;
            let headers = reader.headers().map_err(|e| e.to_string())?.clone();
            for row in reader.records() {
                let row = row.map_err(|e| e.to_string())?;
                let m = prices::parse_record(&headers, &row)?;
                if m.timestamp + 60 > cutoff {
                    break;
                }
                if previous.is_some_and(|p| p >= m.timestamp) {
                    return Err("unordered or overlapping prepared minutes".into());
                }
                previous = Some(m.timestamp);
                while gap_idx < self.gaps.len() && self.gaps[gap_idx].0 < (m.timestamp + 60) * 1000
                {
                    if self.gaps[gap_idx].1 > m.timestamp * 1000 {
                        return Err("prepared minute overlaps quarantine".into());
                    }
                    agg.disconnected();
                    gap_idx += 1;
                }
                for (tf, b) in agg.minute(symbol, &self.source, &m)? {
                    keep(tf, b);
                }
            }
        }
        for (tf, b) in agg.closed_before(cutoff) {
            keep(tf, b);
        }
        let mut engine = Engine::default();
        let mut counts = BTreeMap::new();
        for (tf, min) in [
            (Timeframe::W, cfg.context.minimum_bars.w),
            (Timeframe::D, cfg.context.minimum_bars.d),
            (Timeframe::H4, cfg.context.minimum_bars.h4),
        ] {
            let list = bars.remove(&tf).unwrap_or_default();
            if list.len() < min as usize {
                return Err(format!(
                    "{symbol}: insufficient {tf:?} bars: {} < {min}",
                    list.len()
                ));
            }
            counts.insert(
                format!("{tf:?}"),
                json!({"available":list.len(),"requested":limits(tf)}),
            );
            for b in list {
                engine.seed(tf, b, cfg)?;
            }
        }
        Ok((engine, agg, json!(counts)))
    }
}
