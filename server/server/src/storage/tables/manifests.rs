//! What a table's manifests say about its data without reading the data: per column, how many values and nulls, and the smallest
//! and largest. Iceberg writes these when files are written, so a profile needs no compute.

use std::collections::BTreeMap;

use apache_avro::{Reader, types::Value};

use super::model::{Bound, decode_bound, show_bound};

/// What one data file recorded about itself.
#[derive(Debug, Default, PartialEq)]
pub struct FileStats {
    pub records: i64,
    pub values: BTreeMap<i64, i64>,
    pub nulls: BTreeMap<i64, i64>,
    pub lower: BTreeMap<i64, Vec<u8>>,
    pub upper: BTreeMap<i64, Vec<u8>>,
}

fn plain(v: &Value) -> &Value {
    match v {
        Value::Union(_, inner) => plain(inner),
        other => other,
    }
}

fn field<'a>(record: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    record
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| plain(v))
}

fn number(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Long(n) => Some(*n),
        Value::Int(n) => Some(i64::from(*n)),
        _ => None,
    }
}

/// Iceberg stores a per-column map as an array of `{key, value}` records.
fn pairs(v: Option<&Value>) -> Vec<(i64, &Value)> {
    let Some(Value::Array(items)) = v else {
        return vec![];
    };
    items
        .iter()
        .filter_map(|item| match plain(item) {
            Value::Record(r) => Some((number(field(r, "key"))?, field(r, "value")?)),
            _ => None,
        })
        .collect()
}

fn reader(bytes: &[u8]) -> Result<Reader<'_, &[u8]>, String> {
    Reader::new(bytes).map_err(|e| format!("not a manifest: {e}"))
}

/// The manifests a snapshot's manifest list names, as the paths it recorded.
///
/// # Errors
/// The bytes are not an Avro manifest list.
pub fn manifest_paths(list: &[u8]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for value in reader(list)? {
        let value = value.map_err(|e| format!("a manifest list is unreadable: {e}"))?;
        if let Value::Record(r) = plain(&value)
            && let Some(Value::String(p)) = field(r, "manifest_path")
        {
            out.push(p.clone());
        }
    }
    Ok(out)
}

/// The live data files of one manifest (deleted entries and delete files are left out), with what each recorded.
///
/// # Errors
/// The bytes are not an Avro manifest.
pub fn data_files(manifest: &[u8]) -> Result<Vec<FileStats>, String> {
    let mut out = Vec::new();
    for value in reader(manifest)? {
        let value = value.map_err(|e| format!("a manifest is unreadable: {e}"))?;
        let Value::Record(entry) = plain(&value) else {
            continue;
        };
        // 0 existing, 1 added, 2 deleted.
        if number(field(entry, "status")) == Some(2) {
            continue;
        }
        let Some(Value::Record(file)) = field(entry, "data_file") else {
            continue;
        };
        if number(field(file, "content")).unwrap_or(0) != 0 {
            continue;
        }
        let bytes_of = |name: &str| -> BTreeMap<i64, Vec<u8>> {
            pairs(field(file, name))
                .into_iter()
                .filter_map(|(k, v)| match plain(v) {
                    Value::Bytes(b) => Some((k, b.clone())),
                    _ => None,
                })
                .collect()
        };
        let counts = |name: &str| -> BTreeMap<i64, i64> {
            pairs(field(file, name))
                .into_iter()
                .filter_map(|(k, v)| Some((k, number(Some(v))?)))
                .collect()
        };
        out.push(FileStats {
            records: number(field(file, "record_count")).unwrap_or(0),
            values: counts("value_counts"),
            nulls: counts("null_value_counts"),
            lower: bytes_of("lower_bounds"),
            upper: bytes_of("upper_bounds"),
        });
    }
    Ok(out)
}

/// One column of the table, over all its files.
#[derive(Debug, PartialEq)]
pub struct ColumnStat {
    pub id: i64,
    pub name: String,
    pub values: i64,
    pub nulls: i64,
    pub min: Option<String>,
    pub max: Option<String>,
}

/// Combines the files' records for the columns of the current schema `(id, name, type)`. A bound is shown only when every file that
/// has values for the column recorded one; otherwise the range would be a guess.
#[must_use]
pub fn combine(files: &[FileStats], columns: &[(i64, String, String)]) -> Vec<ColumnStat> {
    columns
        .iter()
        .map(|(id, name, ty)| {
            let values = files.iter().filter_map(|f| f.values.get(id)).sum();
            let nulls = files.iter().filter_map(|f| f.nulls.get(id)).sum();
            let with_values = files
                .iter()
                .filter(|f| f.values.get(id).is_some_and(|v| *v > 0));
            let bounds = |pick: fn(&FileStats) -> &BTreeMap<i64, Vec<u8>>| -> Option<Vec<Bound>> {
                with_values
                    .clone()
                    .map(|f| pick(f).get(id).and_then(|b| decode_bound(ty, b)))
                    .collect()
            };
            let min = bounds(|f| &f.lower)
                .and_then(|b| b.into_iter().reduce(|a, b| if b < a { b } else { a }));
            let max = bounds(|f| &f.upper)
                .and_then(|b| b.into_iter().reduce(|a, b| if b > a { b } else { a }));
            ColumnStat {
                id: *id,
                name: name.clone(),
                values,
                nulls,
                min: min.map(|b| show_bound(ty, &b)),
                max: max.map(|b| show_bound(ty, &b)),
            }
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use apache_avro::{Schema, Writer, types::Value};

    use super::*;

    fn pair(key: i32, value: Value) -> Value {
        Value::Record(vec![
            ("key".into(), Value::Int(key)),
            ("value".into(), value),
        ])
    }

    /// A manifest like the ones `DuckDB` writes, with two files over `id` (1) and `region` (2).
    fn manifest() -> Vec<u8> {
        let schema = Schema::parse_str(
            r#"{"type":"record","name":"manifest_entry","fields":[
              {"name":"status","type":"int"},
              {"name":"data_file","type":{"type":"record","name":"r2","fields":[
                {"name":"content","type":"int"},
                {"name":"record_count","type":"long"},
                {"name":"value_counts","type":{"type":"array","items":{"type":"record","name":"k1_v2","fields":[{"name":"key","type":"int"},{"name":"value","type":"long"}]}}},
                {"name":"null_value_counts","type":{"type":"array","items":{"type":"record","name":"k3_v4","fields":[{"name":"key","type":"int"},{"name":"value","type":"long"}]}}},
                {"name":"lower_bounds","type":{"type":"array","items":{"type":"record","name":"k5_v6","fields":[{"name":"key","type":"int"},{"name":"value","type":"bytes"}]}}},
                {"name":"upper_bounds","type":{"type":"array","items":{"type":"record","name":"k7_v8","fields":[{"name":"key","type":"int"},{"name":"value","type":"bytes"}]}}}
              ]}}
            ]}"#,
        )
        .unwrap();
        let file =
            |status: i32, content: i32, rows: i64, lo: i64, hi: i64, region: (&str, &str)| {
                Value::Record(vec![
                    ("status".into(), Value::Int(status)),
                    (
                        "data_file".into(),
                        Value::Record(vec![
                            ("content".into(), Value::Int(content)),
                            ("record_count".into(), Value::Long(rows)),
                            (
                                "value_counts".into(),
                                Value::Array(vec![
                                    pair(1, Value::Long(rows)),
                                    pair(2, Value::Long(rows)),
                                ]),
                            ),
                            (
                                "null_value_counts".into(),
                                Value::Array(vec![
                                    pair(1, Value::Long(0)),
                                    pair(2, Value::Long(1)),
                                ]),
                            ),
                            (
                                "lower_bounds".into(),
                                Value::Array(vec![
                                    pair(1, Value::Bytes(lo.to_le_bytes().to_vec())),
                                    pair(2, Value::Bytes(region.0.as_bytes().to_vec())),
                                ]),
                            ),
                            (
                                "upper_bounds".into(),
                                Value::Array(vec![
                                    pair(1, Value::Bytes(hi.to_le_bytes().to_vec())),
                                    pair(2, Value::Bytes(region.1.as_bytes().to_vec())),
                                ]),
                            ),
                        ]),
                    ),
                ])
            };
        let mut w = Writer::new(&schema, Vec::new());
        w.append(file(1, 0, 3, 1, 3, ("north", "south"))).unwrap();
        w.append(file(1, 0, 2, 10, 50, ("east", "west"))).unwrap();
        w.append(file(2, 0, 99, -1, 999, ("a", "z"))).unwrap(); // deleted: not counted
        w.append(file(1, 1, 7, -1, 999, ("a", "z"))).unwrap(); // a delete file: not data
        w.into_inner().unwrap()
    }

    #[test]
    fn a_profile_comes_from_what_the_files_recorded() {
        let files = data_files(&manifest()).unwrap();
        assert_eq!(
            files.len(),
            2,
            "deleted entries and delete files are left out"
        );
        let cols = vec![
            (1, "id".to_owned(), "long".to_owned()),
            (2, "region".to_owned(), "string".to_owned()),
        ];
        let stats = combine(&files, &cols);
        assert_eq!(
            stats[0],
            ColumnStat {
                id: 1,
                name: "id".into(),
                values: 5,
                nulls: 0,
                min: Some("1".into()),
                max: Some("50".into())
            }
        );
        assert_eq!(
            (
                stats[1].nulls,
                stats[1].min.as_deref(),
                stats[1].max.as_deref()
            ),
            (2, Some("east"), Some("west"))
        );
    }

    #[test]
    fn a_range_is_not_guessed_when_a_file_did_not_record_one() {
        let mut files = data_files(&manifest()).unwrap();
        files[1].lower.clear();
        let stats = combine(&files, &[(1, "id".into(), "long".into())]);
        assert_eq!(
            (stats[0].min.as_deref(), stats[0].max.as_deref()),
            (None, Some("50"))
        );
        assert_eq!(stats[0].values, 5, "the counts are still right");
    }

    #[test]
    fn bytes_that_are_not_a_manifest_are_refused() {
        assert!(data_files(b"nope").is_err());
        assert!(manifest_paths(b"nope").is_err());
    }
}
